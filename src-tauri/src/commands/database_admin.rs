//! Adapted from Hunter Corlett's fork 53f183437438109093b4a73a747b1d650ce31dc9:
//! table metadata, column definitions, primary/unique keys and default helpers.
//! Mutations use new single-target, one-shot previews; no FK-check bypass.
use super::{
    backup_manager::storage::{now_ms, secure_token, validate_id},
    database_browser::{query_json, quote_identifier, sql_text},
};
use crate::{models::mariadb::MariaDBCredentials, services::mariadb::query::execute_query};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
    time::Duration,
};

const MAX_COLUMNS: usize = 32;
const MAX_SQL: usize = 8000;
const TTL: u64 = 90_000;
const TYPES: &[&str] = &[
    "TINYINT",
    "SMALLINT",
    "INT",
    "BIGINT",
    "DECIMAL",
    "BOOLEAN",
    "CHAR",
    "VARCHAR",
    "TEXT",
    "JSON",
    "DATE",
    "TIME",
    "DATETIME",
    "TIMESTAMP",
];
const INTEGERS: &[&str] = &["TINYINT", "SMALLINT", "INT", "BIGINT"];

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ColumnSpec {
    name: String,
    data_type: String,
    length: Option<String>,
    unsigned: bool,
    nullable: bool,
    default_kind: String,
    default_value: Option<String>,
    auto_increment: bool,
    primary: bool,
    unique: bool,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum AdminAction {
    CreateDatabase,
    CreateTable,
    Empty,
    Drop,
    Optimize,
    Analyze,
    Check,
    Repair,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdminRequest {
    workspace_id: String,
    database: String,
    table: Option<String>,
    action: AdminAction,
    #[serde(default)]
    columns: Vec<ColumnSpec>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableInfo {
    name: String,
    kind: String,
    engine: Option<String>,
    rows: Option<u64>,
    collation: Option<String>,
    data_bytes: u64,
    index_bytes: u64,
    free_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableInfoPage {
    tables: Vec<TableInfo>,
    has_more: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminPreview {
    token: String,
    sql: String,
    confirmation: String,
    expires_at: u64,
    host: String,
    port: u16,
    database: String,
    table: Option<String>,
    warning: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminResult {
    message: String,
    messages: Vec<Vec<String>>,
    has_issues: bool,
}

struct Permit {
    request: AdminRequest,
    credentials: MariaDBCredentials,
    preview: AdminPreview,
    fingerprint: String,
}
static PERMITS: OnceLock<Mutex<HashMap<String, Permit>>> = OnceLock::new();
fn permits() -> &'static Mutex<HashMap<String, Permit>> {
    PERMITS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn name(value: &str) -> Result<String, String> {
    if value.chars().count() > 64
        || value.ends_with(' ')
        || value.chars().any(|c| matches!(c, '/' | '\\' | '.'))
    {
        return Err(
            "Names must be 1-64 characters without slashes, dots or trailing spaces.".into(),
        );
    }
    quote_identifier(value)
}

fn unprotected(database: &str) -> Result<(), String> {
    name(database)?;
    if ["mysql", "sys", "information_schema", "performance_schema"]
        .contains(&database.to_ascii_lowercase().as_str())
    {
        return Err("System databases cannot be changed or maintained here.".into());
    }
    Ok(())
}

fn number(value: &str) -> Result<String, String> {
    let value_digits = value.strip_prefix('-').unwrap_or(value);
    let parts = value_digits.split('.').collect::<Vec<_>>();
    if value.len() > 128
        || parts.len() > 2
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("Default must be a plain decimal number.".into());
    }
    Ok(value.into())
}

fn column_sql(column: &ColumnSpec) -> Result<String, String> {
    if column.data_type.len() > 16
        || column.length.as_ref().is_some_and(|v| v.len() > 8)
        || column.default_kind.len() > 32
        || column.default_value.as_ref().is_some_and(|v| v.len() > 512)
    {
        return Err("Column definition exceeds input limits.".into());
    }
    let quoted = name(&column.name)?;
    let kind = column.data_type.to_ascii_uppercase();
    if !TYPES.contains(&kind.as_str()) {
        return Err("Unsupported column type.".into());
    }
    let numeric = INTEGERS.contains(&kind.as_str()) || kind == "DECIMAL" || kind == "BOOLEAN";
    let mut sql = format!("{quoted} {kind}");
    let length = column.length.as_deref().filter(|s| !s.is_empty());
    match kind.as_str() {
        "VARCHAR" | "CHAR" => {
            let value = length
                .unwrap_or("255")
                .parse::<u16>()
                .map_err(|_| "Invalid character length.")?;
            if value == 0 || value > if kind == "CHAR" { 255 } else { 4096 } {
                return Err("Character length is outside the supported range.".into());
            }
            sql.push_str(&format!("({value})"));
        }
        "DECIMAL" => {
            let parts = length
                .unwrap_or("10,0")
                .split(',')
                .map(str::parse::<u8>)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "Invalid decimal precision/scale.")?;
            if parts.is_empty()
                || parts.len() > 2
                || !(1..=65).contains(&parts[0])
                || parts.get(1).is_some_and(|s| *s > parts[0] || *s > 30)
            {
                return Err("Invalid decimal precision/scale.".into());
            }
            sql.push_str(&format!("({},{})", parts[0], parts.get(1).unwrap_or(&0)));
        }
        _ if length.is_some() => {
            return Err("Length is only supported for CHAR, VARCHAR and DECIMAL.".into())
        }
        _ => {}
    }
    if column.unsigned {
        if !numeric {
            return Err("UNSIGNED requires a numeric column.".into());
        }
        sql.push_str(" UNSIGNED");
    }
    if column.primary && column.nullable {
        return Err("Primary keys cannot be nullable.".into());
    }
    sql.push_str(if column.nullable {
        " NULL"
    } else {
        " NOT NULL"
    });
    match column.default_kind.as_str() {
        "none" => {}
        "null" if column.nullable => sql.push_str(" DEFAULT NULL"),
        "currentTimestamp" if ["TIMESTAMP", "DATETIME"].contains(&kind.as_str()) => {
            sql.push_str(" DEFAULT CURRENT_TIMESTAMP")
        }
        "value" => {
            let value = column
                .default_value
                .as_deref()
                .ok_or("Default value is missing.")?;
            if value.len() > 512 {
                return Err("Default exceeds 512 bytes.".into());
            }
            let literal = if numeric {
                number(value)?
            } else {
                format!("({})", sql_text(value))
            };
            sql.push_str(&format!(" DEFAULT {literal}"));
        }
        _ => return Err("Default is incompatible with this column.".into()),
    }
    if column.auto_increment {
        if !INTEGERS.contains(&kind.as_str()) || !column.primary || column.default_kind != "none" {
            return Err("AUTO_INCREMENT requires an integer primary key without a default.".into());
        }
        sql.push_str(" AUTO_INCREMENT");
    }
    Ok(sql)
}

fn action_sql(request: &AdminRequest) -> Result<String, String> {
    validate_id(&request.workspace_id)?;
    unprotected(&request.database)?;
    let database = name(&request.database)?;
    let sql = if request.action == AdminAction::CreateDatabase {
        if request.table.is_some() || !request.columns.is_empty() {
            return Err("Create database accepts no table or columns.".into());
        }
        format!("CREATE DATABASE {database} CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci")
    } else {
        let table = name(
            request
                .table
                .as_deref()
                .ok_or("Select exactly one table.")?,
        )?;
        let target = format!("{database}.{table}");
        if request.action == AdminAction::CreateTable {
            if request.columns.is_empty() || request.columns.len() > MAX_COLUMNS {
                return Err("A table requires 1-32 columns.".into());
            }
            let mut names = HashSet::new();
            let mut definitions = Vec::new();
            let mut primary = Vec::new();
            let mut auto = None;
            for column in &request.columns {
                if !names.insert(column.name.to_lowercase()) {
                    return Err("Duplicate column name.".into());
                }
                definitions.push(column_sql(column)?);
                let quoted = name(&column.name)?;
                if column.primary {
                    primary.push(quoted.clone());
                }
                if column.auto_increment && auto.replace(quoted.clone()).is_some() {
                    return Err("Only one AUTO_INCREMENT column is allowed.".into());
                }
                if column.unique {
                    definitions.push(format!("UNIQUE KEY ({quoted})"));
                }
            }
            if auto
                .as_ref()
                .is_some_and(|auto| primary.first() != Some(auto))
            {
                return Err("AUTO_INCREMENT must be first in the primary key.".into());
            }
            if !primary.is_empty() {
                definitions.push(format!("PRIMARY KEY ({})", primary.join(",")));
            }
            format!("CREATE TABLE {target} ({}) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci", definitions.join(", "))
        } else {
            if !request.columns.is_empty() {
                return Err("Table maintenance accepts no column definitions.".into());
            }
            let verb = match request.action {
                AdminAction::Empty => "TRUNCATE",
                AdminAction::Drop => "DROP",
                AdminAction::Optimize => "OPTIMIZE",
                AdminAction::Analyze => "ANALYZE",
                AdminAction::Check => "CHECK",
                AdminAction::Repair => "REPAIR",
                _ => unreachable!(),
            };
            format!("{verb} TABLE {target}")
        }
    };
    if sql.len() > MAX_SQL {
        return Err("Definition exceeds the 8,000-byte SQL limit.".into());
    }
    Ok(sql)
}

// Every aggregate fails closed before GROUP_CONCAT's byte limit can truncate it.
fn metadata_part(source: &str, scope: &str, fields: &str, order: &str, limit: usize) -> String {
    let row = format!("JSON_ARRAY({fields})");
    format!("(SELECT IF(COUNT(*)<={limit} AND COALESCE(SUM(OCTET_LENGTH({row})+1),0)<131072,SHA2(COALESCE(GROUP_CONCAT({row} ORDER BY {order} SEPARATOR ','),''),256),NULL) FROM information_schema.{source} WHERE {scope})")
}

fn fingerprint_sql(request: &AdminRequest) -> String {
    let d = sql_text(&request.database);
    let mut parts = vec![
        "JSON_ARRAY(@@hostname,@@port,@@server_id,VERSION(),CURRENT_USER())".into(),
        metadata_part(
            "SCHEMATA",
            &format!("SCHEMA_NAME={d}"),
            "SCHEMA_NAME,DEFAULT_CHARACTER_SET_NAME,DEFAULT_COLLATION_NAME",
            "SCHEMA_NAME",
            1,
        ),
    ];
    if let Some(table) = &request.table {
        let t = sql_text(table);
        let scope = format!("TABLE_SCHEMA={d} AND TABLE_NAME={t}");
        parts.push(metadata_part("TABLES", &scope, "TABLE_NAME,TABLE_TYPE,ENGINE,ROW_FORMAT,TABLE_COLLATION,CREATE_OPTIONS,TABLE_COMMENT,CREATE_TIME", "TABLE_NAME", 1));
        parts.push(metadata_part("COLUMNS", &scope, "COLUMN_NAME,COLUMN_TYPE,IS_NULLABLE,COLUMN_DEFAULT,EXTRA,COLLATION_NAME,GENERATION_EXPRESSION", "ORDINAL_POSITION", 128));
        parts.push(metadata_part(
            "STATISTICS",
            &scope,
            "INDEX_NAME,COLUMN_NAME,SEQ_IN_INDEX,SUB_PART,NON_UNIQUE,INDEX_TYPE",
            "INDEX_NAME,SEQ_IN_INDEX",
            256,
        ));
        parts.push(metadata_part(
            "TABLE_CONSTRAINTS",
            &scope,
            "CONSTRAINT_NAME,CONSTRAINT_TYPE",
            "CONSTRAINT_NAME",
            256,
        ));
        parts.push(metadata_part(
            "CHECK_CONSTRAINTS",
            &format!("CONSTRAINT_SCHEMA={d} AND TABLE_NAME={t}"),
            "CONSTRAINT_NAME,CHECK_CLAUSE",
            "CONSTRAINT_NAME",
            128,
        ));
        parts.push(metadata_part("KEY_COLUMN_USAGE", &format!("({scope}) OR (REFERENCED_TABLE_SCHEMA={d} AND REFERENCED_TABLE_NAME={t})"), "CONSTRAINT_SCHEMA,CONSTRAINT_NAME,TABLE_SCHEMA,TABLE_NAME,COLUMN_NAME,ORDINAL_POSITION,REFERENCED_TABLE_SCHEMA,REFERENCED_TABLE_NAME,REFERENCED_COLUMN_NAME", "TABLE_SCHEMA,TABLE_NAME,CONSTRAINT_NAME,ORDINAL_POSITION", 256));
        parts.push(metadata_part("REFERENTIAL_CONSTRAINTS", &format!("(CONSTRAINT_SCHEMA={d} AND TABLE_NAME={t}) OR (UNIQUE_CONSTRAINT_SCHEMA={d} AND REFERENCED_TABLE_NAME={t})"), "CONSTRAINT_SCHEMA,CONSTRAINT_NAME,TABLE_NAME,REFERENCED_TABLE_NAME,MATCH_OPTION,UPDATE_RULE,DELETE_RULE", "CONSTRAINT_SCHEMA,CONSTRAINT_NAME", 256));
        parts.push(metadata_part(
            "TRIGGERS",
            &format!("EVENT_OBJECT_SCHEMA={d} AND EVENT_OBJECT_TABLE={t}"),
            "TRIGGER_NAME,ACTION_TIMING,EVENT_MANIPULATION,ACTION_STATEMENT,SQL_MODE,DEFINER",
            "TRIGGER_NAME",
            64,
        ));
        parts.push(metadata_part("PARTITIONS", &scope, "PARTITION_NAME,SUBPARTITION_NAME,PARTITION_METHOD,SUBPARTITION_METHOD,PARTITION_EXPRESSION,SUBPARTITION_EXPRESSION,PARTITION_DESCRIPTION", "PARTITION_ORDINAL_POSITION,SUBPARTITION_ORDINAL_POSITION", 256));
    }
    format!("SHA2(CONCAT({}),256)", parts.join(","))
}

fn visibility(credentials: &MariaDBCredentials) -> Result<(), String> {
    let visible: Vec<u8> = query_json(credentials, "SET SESSION max_statement_time=10; SELECT JSON_EXTRACT(IF(Select_priv='Y' AND Trigger_priv='Y','1','0'),'$') FROM mysql.user WHERE CONCAT(User,'@',Host)=CURRENT_USER() LIMIT 2;")?;
    if visible != [1] {
        return Err("Administration requires global SELECT and TRIGGER visibility to detect hidden schema dependencies.".into());
    }
    Ok(())
}

fn read_fingerprint(
    credentials: &MariaDBCredentials,
    request: &AdminRequest,
) -> Result<String, String> {
    let values: Vec<Option<String>> = query_json(
        credentials,
        &format!(
            "SET SESSION sql_mode='STRICT_ALL_TABLES,NO_BACKSLASH_ESCAPES',group_concat_max_len=262144,max_statement_time=10; SELECT JSON_QUOTE({});",
            fingerprint_sql(request)
        ),
    )?;
    values
        .into_iter()
        .next()
        .flatten()
        .ok_or("Metadata exceeds safe inspection limits. No action was sent.".into())
}

fn validate_target(credentials: &MariaDBCredentials, request: &AdminRequest) -> Result<(), String> {
    let schemas: Vec<String> = query_json(credentials, &format!("SET SESSION max_statement_time=10; SELECT JSON_QUOTE(SCHEMA_NAME) FROM information_schema.SCHEMATA WHERE SCHEMA_NAME={} LIMIT 2;", sql_text(&request.database)))?;
    if request.action == AdminAction::CreateDatabase {
        if !schemas.is_empty() {
            return Err("Database already exists.".into());
        }
    } else {
        if schemas != [request.database.clone()] {
            return Err(
                "Exact database name is no longer accessible. Refresh and review again.".into(),
            );
        }
        let table = request.table.as_deref().ok_or("Select a table.")?;
        let tables: Vec<(String, String, Option<String>)> = query_json(credentials, &format!("SELECT JSON_ARRAY(TABLE_NAME,TABLE_TYPE,ENGINE) FROM information_schema.TABLES WHERE TABLE_SCHEMA={} AND TABLE_NAME={} LIMIT 2;", sql_text(&request.database), sql_text(table)))?;
        if request.action == AdminAction::CreateTable {
            if !tables.is_empty() {
                return Err("Table already exists.".into());
            }
        } else {
            if tables.len() != 1 || tables[0].0 != table || tables[0].1 != "BASE TABLE" {
                return Err("Select an existing base table with its exact name.".into());
            }
            if request.action == AdminAction::Repair
                && !matches!(
                    tables[0].2.as_deref(),
                    Some("MyISAM" | "Aria" | "ARCHIVE" | "CSV")
                )
            {
                return Err("This engine does not support REPAIR TABLE.".into());
            }
        }
    }
    Ok(())
}

fn validate_permit(
    permit: &Permit,
    workspace: &str,
    confirmation: &str,
    now: u64,
) -> Result<(), String> {
    if permit.request.workspace_id != workspace
        || permit.preview.confirmation != confirmation
        || permit.preview.expires_at <= now
    {
        return Err("Preview expired or target confirmation does not match. Review again.".into());
    }
    Ok(())
}

fn take_permit(workspace: &str, token: &str, confirmation: &str) -> Result<Permit, String> {
    if token.len() > 256 || confirmation.len() > 520 {
        return Err("Invalid confirmation input.".into());
    }
    let permit = permits()
        .lock()
        .map_err(|_| "Admin preview lock unavailable.")?
        .remove(token)
        .ok_or("Preview expired or was already used.")?;
    validate_permit(&permit, workspace, confirmation, now_ms())?;
    Ok(permit)
}

fn execution_sql(permit: &Permit) -> String {
    // A false or NULL gate prepares invalid SQL and fails closed on this connection.
    format!("SET SESSION sql_mode='STRICT_ALL_TABLES,NO_BACKSLASH_ESCAPES',foreign_key_checks=1,group_concat_max_len=262144,lock_wait_timeout=5,max_statement_time=15; SET @fx_admin=IF(({})={}, {}, 'FX_ADMIN_STALE_PREVIEW'); PREPARE fx_admin FROM @fx_admin; EXECUTE fx_admin; DEALLOCATE PREPARE fx_admin;", fingerprint_sql(&permit.request), sql_text(&permit.fingerprint), sql_text(&permit.preview.sql))
}

#[tauri::command]
pub async fn list_database_admin_tables(
    credentials: MariaDBCredentials,
    database: String,
) -> Result<TableInfoPage, String> {
    super::run_blocking(move || {
        let _guard = super::mariadb::database_access()?;
        name(&database)?;
        let mut tables: Vec<TableInfo> = query_json(&credentials, &format!("SET SESSION max_statement_time=10; SELECT JSON_OBJECT('name',TABLE_NAME,'kind',TABLE_TYPE,'engine',ENGINE,'rows',TABLE_ROWS,'collation',TABLE_COLLATION,'dataBytes',COALESCE(DATA_LENGTH,0),'indexBytes',COALESCE(INDEX_LENGTH,0),'freeBytes',COALESCE(DATA_FREE,0)) FROM information_schema.TABLES WHERE TABLE_SCHEMA={} ORDER BY TABLE_NAME LIMIT 501;", sql_text(&database)))?;
        let has_more = tables.len() > 500;
        tables.truncate(500);
        Ok(TableInfoPage { tables, has_more })
    }).await
}

#[tauri::command]
pub async fn preview_database_admin_action(
    credentials: MariaDBCredentials,
    request: AdminRequest,
) -> Result<AdminPreview, String> {
    super::run_blocking(move || {
        let _guard = super::mariadb::database_access()?;
        let sql = action_sql(&request)?;
        visibility(&credentials)?;
        let fingerprint = read_fingerprint(&credentials, &request)?;
        validate_target(&credentials, &request)?;
        if read_fingerprint(&credentials, &request)? != fingerprint { return Err("Schema changed during preview. Review again.".into()); }
        let target = match &request.table { Some(table) => format!("{}.{}", request.database, table), None => request.database.clone() };
        let preview = AdminPreview {
            token: secure_token()?, sql, confirmation: target, expires_at: now_ms() + TTL,
            host: credentials.host.clone(), port: credentials.port, database: request.database.clone(), table: request.table.clone(),
            warning: "This action can commit independently and cannot be rolled back. Empty and drop permanently remove data. Concurrent external DDL cannot be made atomic with this review; keep other schema administrators idle. Refresh before retrying any interrupted operation.".into(),
        };
        let mut pending = permits().lock().map_err(|_| "Admin preview lock unavailable.")?;
        pending.retain(|_, permit| permit.preview.expires_at > now_ms());
        if pending.len() >= 8 { return Err("Too many pending previews. Wait for them to expire.".into()); }
        pending.insert(preview.token.clone(), Permit { request, credentials, preview: preview.clone(), fingerprint });
        let token = preview.token.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(TTL)).await;
            if let Ok(mut pending) = permits().lock() { pending.remove(&token); }
        });
        Ok(preview)
    }).await
}

#[tauri::command]
pub async fn apply_database_admin_action(
    workspace_id: String,
    token: String,
    confirmation: String,
) -> Result<AdminResult, String> {
    super::run_blocking(move || {
        // Serializes against this application's queries, row edits, backups and restores.
        let _guard = super::mariadb::maintenance_access()?;
        let permit = take_permit(&workspace_id, &token, &confirmation)?;
        unprotected(&permit.request.database)?;
        visibility(&permit.credentials)?;
        validate_target(&permit.credentials, &permit.request)?;
        if read_fingerprint(&permit.credentials, &permit.request)? != permit.fingerprint { return Err("Schema or server changed since preview. No action was sent. Refresh and review again.".into()); }
        validate_permit(&permit, &workspace_id, &confirmation, now_ms())?;
        let sql = execution_sql(&permit);
        let mut credentials = permit.credentials.clone();
        credentials.database = None;
        let uncertain = |error: String| {
            let error = if permit.credentials.password.is_empty() { error } else { error.replace(&permit.credentials.password, "[redacted]") };
            format!("The action did not return a confirmed success. Nontransactional DDL/maintenance may have completed or partially changed the target; no rollback is promised. Refresh {} before retrying. {}", permit.preview.confirmation, error.chars().take(2000).collect::<String>())
        };
        let result = execute_query(credentials, sql).map_err(&uncertain)?;
        if !result.success { return Err(uncertain(result.stderr)); }
        let maintenance = matches!(permit.request.action, AdminAction::Optimize | AdminAction::Analyze | AdminAction::Check | AdminAction::Repair);
        let issues = !result.stderr.is_empty() || result.rows.len() > 32 || (maintenance && result.rows.is_empty()) || result.rows.iter().any(|row| row.get(2).is_some_and(|kind| matches!(kind.to_ascii_lowercase().as_str(), "error" | "warning")));
        let mut messages = result.rows.into_iter().take(32).map(|row| row.into_iter().take(4).map(|cell| cell.chars().take(1024).collect()).collect()).collect::<Vec<Vec<String>>>();
        if !result.stderr.is_empty() { messages.push(vec![result.stderr.chars().take(2000).collect()]); }
        Ok(AdminResult { message: if issues { "Action returned warnings or errors. Nontransactional changes may already have occurred. Inspect the result before retrying." } else { "Administrative action completed." }.into(), messages, has_issues: issues })
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, data_type: &str) -> ColumnSpec {
        ColumnSpec {
            name: name.into(),
            data_type: data_type.into(),
            length: None,
            unsigned: false,
            nullable: false,
            default_kind: "none".into(),
            default_value: None,
            auto_increment: false,
            primary: false,
            unique: false,
        }
    }
    fn request(action: AdminAction) -> AdminRequest {
        AdminRequest {
            workspace_id: "fixture".into(),
            database: "shop".into(),
            table: Some("items".into()),
            action,
            columns: vec![],
        }
    }
    fn fixture_permit() -> Permit {
        Permit {
            request: request(AdminAction::Drop),
            credentials: MariaDBCredentials {
                host: "fixture.invalid".into(),
                port: 3306,
                username: "fixture".into(),
                password: "".into(),
                database: None,
            },
            preview: AdminPreview {
                token: "fixture-admin-token".into(),
                sql: "DROP TABLE `shop`.`items`".into(),
                confirmation: "shop.items".into(),
                expires_at: now_ms() + TTL,
                host: "fixture.invalid".into(),
                port: 3306,
                database: "shop".into(),
                table: Some("items".into()),
                warning: "".into(),
            },
            fingerprint: "fixture-hash".into(),
        }
    }

    #[test]
    fn protects_system_schemas_for_every_action() {
        for action in [
            AdminAction::CreateDatabase,
            AdminAction::CreateTable,
            AdminAction::Empty,
            AdminAction::Drop,
            AdminAction::Optimize,
            AdminAction::Analyze,
            AdminAction::Check,
            AdminAction::Repair,
        ] {
            for database in [
                "mysql",
                "MYSQL",
                "sys",
                "information_schema",
                "performance_schema",
            ] {
                let mut req = request(action.clone());
                req.database = database.into();
                assert!(action_sql(&req).is_err(), "{action:?} on {database}");
            }
        }
    }

    #[test]
    fn quotes_single_targets_and_rejects_bypass_fields() {
        let mut req = request(AdminAction::Drop);
        req.table = Some("a`;DROP DATABASE x;--".into());
        assert_eq!(
            action_sql(&req).unwrap(),
            "DROP TABLE `shop`.`a``;DROP DATABASE x;--`"
        );
        let mut value = serde_json::to_value(&req).unwrap();
        value["disableForeignKeyChecks"] = true.into();
        assert!(serde_json::from_value::<AdminRequest>(value).is_err());
        req.table = Some("a.b".into());
        assert!(action_sql(&req).is_err());
        req.table = None;
        assert!(action_sql(&req).is_err());
    }

    #[test]
    fn builds_keys_and_mode_independent_defaults() {
        let mut id = column("id", "INT");
        id.primary = true;
        id.auto_increment = true;
        id.unsigned = true;
        let mut title = column("na`me", "VARCHAR");
        title.unique = true;
        title.default_kind = "value".into();
        title.default_value = Some("a'\\b".into());
        let mut timestamp = column("created_at", "TIMESTAMP");
        timestamp.default_kind = "currentTimestamp".into();
        let mut req = request(AdminAction::CreateTable);
        req.columns = vec![id, title, timestamp];
        let sql = action_sql(&req).unwrap();
        assert!(sql.contains("`id` INT UNSIGNED NOT NULL AUTO_INCREMENT"));
        assert!(sql.contains("DEFAULT (CONVERT(0x61275c62 USING utf8mb4))"));
        assert!(sql.contains("UNIQUE KEY (`na``me`)"));
        assert!(sql.contains("PRIMARY KEY (`id`)"));
        assert!(sql.contains("DEFAULT CURRENT_TIMESTAMP"));
        assert!(!sql.contains("a'\\b"));
    }

    #[test]
    fn rejects_invalid_definitions_and_limits() {
        let mut req = request(AdminAction::CreateTable);
        for columns in [
            vec![],
            vec![column("a", "INT"); 33],
            vec![column("a", "INT"), column("A", "INT")],
            vec![column("a", "INT); DROP DATABASE x")],
        ] {
            req.columns = columns;
            assert!(action_sql(&req).is_err());
        }
        for length in ["0", "4097", "1);--", "65536"] {
            let mut c = column("a", "VARCHAR");
            c.length = Some(length.into());
            req.columns = vec![c];
            assert!(action_sql(&req).is_err());
        }
        for length in ["0,0", "66,0", "10,11", "10,31", "10,0,0"] {
            let mut c = column("a", "DECIMAL");
            c.length = Some(length.into());
            req.columns = vec![c];
            assert!(action_sql(&req).is_err());
        }
        for value in ["", "1.", ".5", "1e5", "1;DROP", "--1", "1.2.3"] {
            assert!(number(value).is_err());
        }
        assert_eq!(number("-12.50").unwrap(), "-12.50");
        let mut c = column("a", "INT");
        c.auto_increment = true;
        req.columns = vec![c.clone()];
        assert!(action_sql(&req).is_err());
        c.primary = true;
        c.default_kind = "value".into();
        c.default_value = Some("0".into());
        req.columns = vec![c];
        assert!(action_sql(&req).is_err());
    }

    #[test]
    fn preview_binds_exact_workspace_target_and_expiry_and_is_one_shot() {
        let permit = fixture_permit();
        assert!(validate_permit(&permit, "other", "shop.items", now_ms()).is_err());
        assert!(validate_permit(&permit, "fixture", "items", now_ms()).is_err());
        assert!(validate_permit(&permit, "fixture", "SHOP.items", now_ms()).is_err());
        assert!(
            validate_permit(&permit, "fixture", "shop.items", permit.preview.expires_at).is_err()
        );
        assert!(validate_permit(&permit, "fixture", "shop.items", now_ms()).is_ok());
        let token = permit.preview.token.clone();
        permits().lock().unwrap().insert(token.clone(), permit);
        assert!(take_permit("fixture", &token, "shop.items").is_ok());
        assert!(take_permit("fixture", &token, "shop.items").is_err());
    }

    #[test]
    fn schema_fingerprint_covers_dependencies_and_fails_closed_on_size() {
        let sql = fingerprint_sql(&request(AdminAction::Drop));
        for part in [
            "SCHEMATA",
            "TABLES",
            "COLUMNS",
            "STATISTICS",
            "TABLE_CONSTRAINTS",
            "CHECK_CONSTRAINTS",
            "KEY_COLUMN_USAGE",
            "REFERENTIAL_CONSTRAINTS",
            "TRIGGERS",
            "PARTITIONS",
            "@@hostname",
            "CURRENT_USER()",
            "CREATE_TIME",
            "GENERATION_EXPRESSION",
            "DELETE_RULE",
        ] {
            assert!(sql.contains(part), "{part}");
        }
        assert!(sql.contains("COUNT(*)<=128"));
        assert!(sql.contains("<131072"));
        assert!(sql.contains(",NULL)"));
        assert!(sql.len() < 24000);
    }

    #[test]
    fn execution_keeps_fk_checks_and_embeds_only_reviewed_sql() {
        let permit = fixture_permit();
        let sql = execution_sql(&permit);
        assert!(sql.contains("foreign_key_checks=1"));
        assert!(!sql.to_lowercase().contains("foreign_key_checks=0"));
        assert!(sql.contains("NO_BACKSLASH_ESCAPES"));
        assert!(sql.contains("FX_ADMIN_STALE_PREVIEW"));
        assert!(sql.contains(&sql_text(&permit.preview.sql)));
        assert!(sql.contains(&sql_text(&permit.fingerprint)));
        assert_eq!(sql.matches("EXECUTE fx_admin;").count(), 1);
    }
}
