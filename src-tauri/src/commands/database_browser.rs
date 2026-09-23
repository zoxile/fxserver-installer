use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use crate::{
    models::mariadb::MariaDBCredentials,
    services::mariadb::query::{
        configure_query_command, configured_client as json_client, run_client, QUERY_TIMEOUT,
    },
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
#[cfg(all(test, windows))]
use {crate::process::CommandNoWindowExt, std::process::Command};

const MAX_STRUCTURED_ROWS: usize = 10_000;
const MAX_CELL: usize = 4096;
const MAX_EXPORT: usize = 5000;
const MAX_PAGE_CELLS: usize = 4000;

#[cfg(all(test, windows))]
#[path = "database_boundary_integration.rs"]
mod boundary_integration;

fn bounded_page_size(requested: usize, columns: usize) -> usize {
    requested.min(200).min(MAX_PAGE_CELLS / columns.max(1))
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserColumn {
    pub name: String,
    pub column_type: String,
    #[serde(deserialize_with = "database_bool")]
    pub nullable: bool,
    pub default_value: Option<String>,
    pub extra: String,
    #[serde(deserialize_with = "database_bool")]
    pub binary: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserIndex {
    pub name: String,
    pub column: Option<String>,
    pub sequence: u32,
    #[serde(deserialize_with = "database_bool")]
    pub unique: bool,
    pub index_type: String,
    pub prefix_length: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserMetadata {
    columns: Vec<BrowserColumn>,
    indexes: Vec<BrowserIndex>,
    editable: bool,
    edit_reason: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterOperator {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    Contains,
    IsNull,
    IsNotNull,
}

#[derive(Clone, Deserialize)]
pub struct BrowserFilter {
    column: String,
    operator: FilterOperator,
    value: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRequest {
    database: String,
    table: String,
    #[serde(default)]
    filters: Vec<BrowserFilter>,
    sort_column: Option<String>,
    #[serde(default)]
    descending: bool,
    offset: usize,
    page_size: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPage {
    rows: Vec<Vec<Option<String>>>,
    has_more: bool,
    truncated_cells: bool,
    page_size: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserExport {
    path: String,
    rows: usize,
    has_more: bool,
}

fn database_bool<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Bool(value) => Ok(value),
        serde_json::Value::Number(value) if value.as_u64() == Some(0) => Ok(false),
        serde_json::Value::Number(value) if value.as_u64() == Some(1) => Ok(true),
        serde_json::Value::String(value) if value == "true" => Ok(true),
        serde_json::Value::String(value) if value == "false" => Ok(false),
        _ => Err(serde::de::Error::custom("Expected a database boolean")),
    }
}

pub(crate) fn quote_identifier(value: &str) -> Result<String, String> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err("Invalid database, table or column identifier.".into());
    }
    Ok(format!("`{}`", value.replace('`', "``")))
}

pub(crate) fn sql_text(value: &str) -> String {
    if value.is_empty() {
        return "''".into();
    }
    let hex: String = value.bytes().map(|b| format!("{b:02x}")).collect();
    format!("CONVERT(0x{hex} USING utf8mb4)")
}

fn parse_json_output<T: DeserializeOwned>(stdout: Vec<u8>) -> Result<Vec<T>, String> {
    let text = String::from_utf8(stdout).map_err(|_| "MariaDB output was not UTF-8.")?;
    text.lines()
        .filter(|line| !line.is_empty())
        .enumerate()
        .map(|(index, line)| {
            if index >= MAX_STRUCTURED_ROWS {
                return Err("Query returned more than 10,000 records. Narrow the query.".into());
            }
            // Deserializer errors can echo SQL data or credentials; expose only the location.
            serde_json::from_str(line).map_err(|error| {
                format!(
                    "Invalid structured MariaDB result at line {} column {}.",
                    error.line(),
                    error.column()
                )
            })
        })
        .collect()
}

#[cfg(all(test, windows))]
pub(crate) fn isolated_test_client(
    credentials: &MariaDBCredentials,
) -> Option<Result<(Command, crate::services::mariadb::query::CredentialFile), String>> {
    tests::isolated_client(credentials)
}

/// Also used for commits and restores: this is deliberately a single-execution
/// client path, with no native routing, SQL-prefix guessing, retries, or fallback.
pub(crate) fn query_json<T: DeserializeOwned>(
    credentials: &MariaDBCredentials,
    sql: &str,
) -> Result<Vec<T>, String> {
    let result = (|| {
        if sql.len() > 24000 {
            return Err("Query is too large. Reduce the number or length of filters.".into());
        }
        let (mut command, _credentials_file) = json_client(credentials)?;
        configure_query_command(&mut command, None)?;
        command.arg("--skip-column-names");
        let output = run_client(&mut command, sql.to_owned(), QUERY_TIMEOUT)?;
        if !output.status.success() {
            return Err(super::backup_manager::storage::client_failure(
                &String::from_utf8_lossy(&output.stderr),
            ));
        }
        parse_json_output(output.stdout)
    })();
    result.map_err(|error: String| redacted_error(&error, &credentials.password))
}

fn redacted_error(error: &str, password: &str) -> String {
    // Redact before bounding, including a password crossing the display boundary.
    let error = if password.is_empty() {
        error.to_owned()
    } else {
        error.replace(password, "[redacted]")
    };
    error.chars().take(4000).collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum MetadataRow {
    TableType(String),
    Column(BrowserColumn),
    Index(BrowserIndex),
    Safe(u8),
}

fn columns_sql(database: &str, table: &str) -> String {
    let schema = sql_text(database);
    let name = sql_text(table);
    // The fork's columns-only optimization skipped the base-table check. Keep it
    // in this query so every page/export still refuses views and inaccessible tables.
    format!("SELECT JSON_OBJECT('column',JSON_OBJECT('name',COLUMN_NAME,'columnType',COLUMN_TYPE,'nullable',IF(IS_NULLABLE='YES',JSON_EXTRACT('true','$'),JSON_EXTRACT('false','$')),'defaultValue',COLUMN_DEFAULT,'extra',EXTRA,'binary',IF(DATA_TYPE IN ('binary','varbinary','tinyblob','blob','mediumblob','longblob','bit','geometry'),JSON_EXTRACT('true','$'),JSON_EXTRACT('false','$')))) FROM information_schema.COLUMNS WHERE TABLE_SCHEMA={schema} AND TABLE_NAME={name} AND EXISTS(SELECT 1 FROM information_schema.TABLES WHERE TABLE_SCHEMA={schema} AND TABLE_NAME={name} AND TABLE_TYPE='BASE TABLE') ORDER BY ORDINAL_POSITION LIMIT 129;")
}

fn validate_columns(columns: &[BrowserColumn]) -> Result<(), String> {
    if columns.is_empty() || columns.len() > 128 {
        return Err(
            "Choose an accessible base table with 1 to 128 columns. Views are not browsed.".into(),
        );
    }
    Ok(())
}

/// Column-only reads and client caching are adapted from Hunter Corlett's fork
/// 53f183437438109093b4a73a747b1d650ce31dc9; no native execution/fallback is reused.
fn browser_columns(
    credentials: &MariaDBCredentials,
    database: &str,
    table: &str,
) -> Result<Vec<BrowserColumn>, String> {
    quote_identifier(database)?;
    quote_identifier(table)?;
    let rows = query_json(
        credentials,
        &format!(
            "SET SESSION max_statement_time=20; START TRANSACTION READ ONLY; {} ROLLBACK;",
            columns_sql(database, table)
        ),
    )?;
    decode_columns(rows)
}

fn decode_columns(rows: Vec<MetadataRow>) -> Result<Vec<BrowserColumn>, String> {
    let columns = rows
        .into_iter()
        .map(|row| match row {
            MetadataRow::Column(column) => Ok(column),
            _ => Err("Unexpected column metadata result.".to_string()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_columns(&columns)?;
    Ok(columns)
}

fn metadata_sql(database: &str, table: &str) -> Result<String, String> {
    quote_identifier(database)?;
    quote_identifier(table)?;
    let schema = sql_text(database);
    let name = sql_text(table);
    // Tag each result so independent metadata reads share one connection and one
    // 30-second client deadline without confusing result sets or caching editability.
    Ok(format!(
        "SET SESSION max_statement_time=20; START TRANSACTION READ ONLY; SELECT JSON_OBJECT('tableType',TABLE_TYPE) FROM information_schema.TABLES WHERE TABLE_SCHEMA={schema} AND TABLE_NAME={name} LIMIT 2; {} SELECT JSON_OBJECT('index',JSON_OBJECT('name',INDEX_NAME,'column',COLUMN_NAME,'sequence',SEQ_IN_INDEX,'unique',IF(NON_UNIQUE=0,JSON_EXTRACT('true','$'),JSON_EXTRACT('false','$')),'indexType',INDEX_TYPE,'prefixLength',SUB_PART)) FROM information_schema.STATISTICS WHERE TABLE_SCHEMA={schema} AND TABLE_NAME={name} ORDER BY INDEX_NAME,SEQ_IN_INDEX LIMIT 1025; SELECT JSON_OBJECT('safe',JSON_EXTRACT(IF({},'1','0'),'$')); ROLLBACK;",
        columns_sql(database, table), safe_table(database, table)
    ))
}

fn metadata(
    credentials: &MariaDBCredentials,
    database: &str,
    table: &str,
) -> Result<BrowserMetadata, String> {
    decode_metadata(query_json(credentials, &metadata_sql(database, table)?)?)
}

fn decode_metadata(rows: Vec<MetadataRow>) -> Result<BrowserMetadata, String> {
    let mut kinds = Vec::new();
    let mut columns = Vec::new();
    let mut indexes = Vec::new();
    let mut safe = Vec::new();
    for row in rows {
        match row {
            MetadataRow::TableType(kind) => kinds.push(kind),
            MetadataRow::Column(column) => columns.push(column),
            MetadataRow::Index(index) => indexes.push(index),
            MetadataRow::Safe(value) => safe.push(value),
        }
    }
    if kinds != ["BASE TABLE"] {
        return Err("Choose an accessible base table. Views are not browsed.".into());
    }
    validate_columns(&columns)?;
    if indexes.len() > 1024 || safe.len() != 1 {
        return Err("Incomplete table safety metadata. Refresh the table.".into());
    }
    let mut value = BrowserMetadata {
        columns,
        indexes,
        editable: false,
        edit_reason: None,
    };
    value.edit_reason = edit_columns(&value).err();
    if value.edit_reason.is_none() && safe != [1] {
        value.edit_reason = Some("Editing requires InnoDB without triggers, check constraints or foreign-key relationships.".into());
    }
    value.editable = value.edit_reason.is_none();
    Ok(value)
}

fn safe_table(database: &str, table: &str) -> String {
    let d = sql_text(database);
    let t = sql_text(table);
    format!("(SELECT COUNT(*) FROM information_schema.TABLES WHERE TABLE_SCHEMA={d} AND TABLE_NAME={t} AND TABLE_TYPE='BASE TABLE' AND ENGINE='InnoDB')=1 AND (SELECT COUNT(*) FROM information_schema.TRIGGERS WHERE EVENT_OBJECT_SCHEMA={d} AND EVENT_OBJECT_TABLE={t})=0 AND (SELECT COUNT(*) FROM information_schema.TABLE_CONSTRAINTS WHERE TABLE_SCHEMA={d} AND TABLE_NAME={t} AND CONSTRAINT_TYPE NOT IN ('PRIMARY KEY','UNIQUE'))=0 AND (SELECT COUNT(*) FROM information_schema.KEY_COLUMN_USAGE WHERE REFERENCED_TABLE_SCHEMA={d} AND REFERENCED_TABLE_NAME={t})=0")
}

fn edit_visibility(credentials: &MariaDBCredentials) -> Result<(), String> {
    let visible: Vec<u8> = query_json(credentials, "SELECT JSON_EXTRACT(IF(Select_priv='Y' AND Trigger_priv='Y','1','0'),'$') FROM mysql.user WHERE CONCAT(User,'@',Host)=CURRENT_USER();")?;
    if visible != [1] {
        return Err("Safe editing requires global SELECT and TRIGGER visibility to rule out hidden triggers and cascading relationships.".into());
    }
    Ok(())
}

fn schema_hash(database: &str, table: &str) -> String {
    let scope = format!(
        "TABLE_SCHEMA={} AND TABLE_NAME={}",
        sql_text(database),
        sql_text(table)
    );
    format!("SHA2(CONCAT(COALESCE((SELECT GROUP_CONCAT(JSON_ARRAY(COLUMN_NAME,COLUMN_TYPE,IS_NULLABLE,COLUMN_DEFAULT,EXTRA,COLLATION_NAME) ORDER BY ORDINAL_POSITION) FROM information_schema.COLUMNS WHERE {scope}),''),COALESCE((SELECT GROUP_CONCAT(JSON_ARRAY(INDEX_NAME,COLUMN_NAME,SEQ_IN_INDEX,SUB_PART,NON_UNIQUE) ORDER BY INDEX_NAME,SEQ_IN_INDEX) FROM information_schema.STATISTICS WHERE {scope}),'')),256)")
}

fn read_schema_hash(
    credentials: &MariaDBCredentials,
    database: &str,
    table: &str,
) -> Result<String, String> {
    let hash: Vec<String> = query_json(
        credentials,
        &format!(
            "SET SESSION group_concat_max_len=1048576; SELECT JSON_QUOTE({});",
            schema_hash(database, table)
        ),
    )?;
    if hash.len() != 1 {
        return Err("Table schema fingerprint unavailable.".into());
    }
    Ok(hash.into_iter().next().unwrap())
}

fn numeric_column(column: &BrowserColumn) -> bool {
    [
        "tinyint",
        "smallint",
        "mediumint",
        "int",
        "integer",
        "bigint",
        "decimal",
        "numeric",
    ]
    .contains(&column.column_type.split(['(', ' ']).next().unwrap_or(""))
}

fn edit_columns(metadata: &BrowserMetadata) -> Result<Vec<String>, String> {
    if metadata.columns.len() > 32 {
        return Err("Row editing supports at most 32 columns.".into());
    }
    for column in &metadata.columns {
        let kind = column.column_type.split(['(', ' ']).next().unwrap_or("");
        if column.binary
            || (!numeric_column(column)
                && ![
                    "char",
                    "varchar",
                    "tinytext",
                    "text",
                    "mediumtext",
                    "longtext",
                    "date",
                    "datetime",
                    "timestamp",
                    "time",
                    "year",
                ]
                .contains(&kind))
            || (!column.extra.is_empty() && column.extra != "auto_increment")
        {
            return Err("This table has an unsupported type or generated/default expression. Row editing is disabled.".into());
        }
    }
    let primary = metadata
        .indexes
        .iter()
        .filter(|index| index.name == "PRIMARY")
        .collect::<Vec<_>>();
    if primary.is_empty()
        || primary.iter().any(|index| {
            index.prefix_length.is_some()
                || index.column.as_deref().is_none_or(|name| {
                    !metadata
                        .columns
                        .iter()
                        .any(|column| column.name == name && !column.nullable)
                })
        })
    {
        return Err("Editing requires a complete, non-null primary key without prefix or expression columns.".into());
    }
    Ok(primary
        .into_iter()
        .filter_map(|index| index.column.clone())
        .collect())
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum CellInput {
    Null,
    Text(String),
    Number(String),
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ColumnInput {
    column: String,
    value: CellInput,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Insert,
    Update,
    Delete,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserChange {
    workspace_id: String,
    database: String,
    table: String,
    kind: ChangeKind,
    values: Vec<ColumnInput>,
    original: Option<Vec<Option<String>>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePreview {
    token: String,
    sql: String,
    parameters: Vec<ColumnInput>,
    confirmation: String,
    expires_at: u64,
    kind: ChangeKind,
    host: String,
    port: u16,
}

struct ChangePermit {
    preview: ChangePreview,
    change: BrowserChange,
    credentials: MariaDBCredentials,
    schema_hash: String,
}
static CHANGES: OnceLock<Mutex<HashMap<String, ChangePermit>>> = OnceLock::new();
fn changes() -> &'static Mutex<HashMap<String, ChangePermit>> {
    CHANGES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn validate_change_permit(
    permit: &ChangePermit,
    workspace: &str,
    confirmation: &str,
    now: u64,
) -> Result<(), String> {
    if permit.change.workspace_id != workspace
        || permit.preview.expires_at <= now
        || permit.preview.confirmation != confirmation
    {
        return Err("Use the current preview and confirm the exact database.table name.".into());
    }
    Ok(())
}

fn change_transaction(permit: &ChangePermit, mutation: &str, locking: &str) -> String {
    let gate = format!("{}={} AND ({}) AND (SELECT COUNT(*) FROM mysql.user WHERE CONCAT(User,'@',Host)=CURRENT_USER() AND Select_priv='Y' AND Trigger_priv='Y')=1", schema_hash(&permit.change.database, &permit.change.table), sql_text(&permit.schema_hash), safe_table(&permit.change.database, &permit.change.table));
    format!("SET SESSION sql_mode='STRICT_ALL_TABLES,NO_AUTO_VALUE_ON_ZERO',group_concat_max_len=1048576,innodb_lock_wait_timeout=5,lock_wait_timeout=5,max_statement_time=15; START TRANSACTION; {locking} SET @fx_safe=({gate}); SET @fx_change=IF(@fx_safe,{},'SELECT JSON_ARRAY()'); PREPARE fx_change FROM @fx_change; EXECUTE fx_change; GET DIAGNOSTICS @fx_warnings=NUMBER,@fx_affected=ROW_COUNT; DEALLOCATE PREPARE fx_change; SET @fx_commit=(@fx_safe AND @fx_affected=1 AND @fx_warnings=0); SET @fx_finish=IF(@fx_commit,'COMMIT','ROLLBACK'); PREPARE fx_finish FROM @fx_finish; EXECUTE fx_finish; DEALLOCATE PREPARE fx_finish; SELECT JSON_OBJECT('affected',IF(@fx_commit,1,0));", sql_text(mutation.trim_end_matches(';')))
}

fn input_sql(column: &BrowserColumn, input: &CellInput) -> Result<String, String> {
    match input {
        CellInput::Null if column.nullable => Ok("NULL".into()),
        CellInput::Text(value) if !numeric_column(column) && value.len() <= MAX_CELL => {
            Ok(sql_text(value))
        }
        CellInput::Number(value) if numeric_column(column) && value.len() <= 128 => {
            let unsigned = value
                .strip_prefix('-')
                .or_else(|| value.strip_prefix('+'))
                .unwrap_or(value);
            let mut dots = 0;
            if unsigned.is_empty()
                || !unsigned.bytes().any(|b| b.is_ascii_digit())
                || !unsigned.bytes().all(|b| {
                    if b == b'.' {
                        dots += 1;
                        dots <= 1
                    } else {
                        b.is_ascii_digit()
                    }
                })
            {
                return Err("Numeric fields accept a literal number, never an expression.".into());
            }
            Ok(value.clone())
        }
        _ => Err(format!(
            "Invalid value type, NULL or length for {}.",
            column.name
        )),
    }
}

fn mutation_sql(
    change: &BrowserChange,
    metadata: &BrowserMetadata,
) -> Result<(String, String), String> {
    let primary = edit_columns(metadata)?;
    let table = format!(
        "{}.{}",
        quote_identifier(&change.database)?,
        quote_identifier(&change.table)?
    );
    let mut assignments = Vec::new();
    let mut names = Vec::new();
    let mut values = Vec::new();
    for input in &change.values {
        let column = metadata
            .columns
            .iter()
            .find(|column| column.name == input.column)
            .ok_or("Input column no longer exists.")?;
        if names.contains(&input.column) {
            return Err("Duplicate input column.".into());
        }
        let value = input_sql(column, &input.value)?;
        assignments.push(format!("{}={value}", quote_identifier(&column.name)?));
        names.push(input.column.clone());
        values.push(value);
    }
    if change.kind == ChangeKind::Insert {
        if change.original.is_some() || values.is_empty() {
            return Err("Insert requires literal column values and no original row.".into());
        }
        if metadata
            .columns
            .iter()
            .any(|column| column.extra != "auto_increment" && !names.contains(&column.name))
        {
            return Err("Insert requires an explicit literal or NULL for every non-auto-increment column; default expressions are not evaluated.".into());
        }
        let columns = names
            .iter()
            .map(|name| quote_identifier(name))
            .collect::<Result<Vec<_>, _>>()?
            .join(",");
        return Ok((
            format!(
                "INSERT INTO {table} ({columns}) VALUES ({});",
                values.join(",")
            ),
            format!("SELECT JSON_ARRAY() FROM {table} LIMIT 1 FOR UPDATE;"),
        ));
    }
    let original = change
        .original
        .as_ref()
        .filter(|row| row.len() == metadata.columns.len())
        .ok_or("A complete original row is required.")?;
    if original
        .iter()
        .flatten()
        .any(|value| value.len() > MAX_CELL || value.ends_with(" [truncated]"))
    {
        return Err("Truncated or oversized rows cannot be edited.".into());
    }
    let mut key = Vec::new();
    let mut conditions = Vec::new();
    for (column, original) in metadata.columns.iter().zip(original) {
        let identifier = quote_identifier(&column.name)?;
        let comparison = match original {
            None => format!("{identifier} IS NULL"),
            Some(value) => format!(
                "BINARY CAST({identifier} AS CHAR CHARACTER SET utf8mb4) <=> BINARY {}",
                sql_text(value)
            ),
        };
        if primary.contains(&column.name) {
            let original = original
                .as_ref()
                .ok_or("A primary-key value cannot be NULL.")?;
            let input = if numeric_column(column) {
                CellInput::Number(original.clone())
            } else {
                CellInput::Text(original.clone())
            };
            key.push(format!("{identifier} <=> {}", input_sql(column, &input)?));
        }
        conditions.push(comparison);
    }
    let locking = format!(
        "SELECT JSON_ARRAY() FROM {table} WHERE {} LIMIT 1 FOR UPDATE;",
        key.join(" AND ")
    );
    let predicate = [key.join(" AND "), conditions.join(" AND ")].join(" AND ");
    let sql = match change.kind {
        ChangeKind::Update if !assignments.is_empty() => format!(
            "UPDATE {table} SET {} WHERE {predicate} LIMIT 1;",
            assignments.join(",")
        ),
        ChangeKind::Delete if assignments.is_empty() => {
            format!("DELETE FROM {table} WHERE {predicate} LIMIT 1;")
        }
        _ => return Err("Choose literal changes for update, or no values for delete.".into()),
    };
    Ok((sql, locking))
}

#[tauri::command]
pub async fn preview_database_browser_change(
    credentials: MariaDBCredentials,
    change: BrowserChange,
) -> Result<ChangePreview, String> {
    super::run_blocking(move || {
        use super::backup_manager::storage::{now_ms, secure_token, validate_id};
        let _guard = super::mariadb::database_access()?;
        validate_id(&change.workspace_id)?;
        edit_visibility(&credentials)?;
        let schema_hash = read_schema_hash(&credentials, &change.database, &change.table)?;
        let metadata = metadata(&credentials, &change.database, &change.table)?;
        if !metadata.editable {
            return Err(metadata
                .edit_reason
                .unwrap_or("Row editing is unavailable.".into()));
        }
        let (sql, _) = mutation_sql(&change, &metadata)?;
        if sql.len() > 10000 {
            return Err(
                "Change preview exceeds the safe CLI statement size. Edit a smaller row.".into(),
            );
        }
        if read_schema_hash(&credentials, &change.database, &change.table)? != schema_hash {
            return Err("Table schema changed during preview. Refresh and review again.".into());
        }
        let preview = ChangePreview {
            token: secure_token()?,
            sql,
            parameters: change.values.clone(),
            confirmation: format!("{}.{}", change.database, change.table),
            expires_at: now_ms() + 120_000,
            kind: change.kind.clone(),
            host: credentials.host.clone(),
            port: credentials.port,
        };
        let mut pending = changes()
            .lock()
            .map_err(|_| "Change preview lock unavailable.")?;
        pending.retain(|_, permit| permit.preview.expires_at > now_ms());
        if pending.len() >= 10 {
            return Err("Too many pending changes. Wait for previews to expire.".into());
        }
        pending.insert(
            preview.token.clone(),
            ChangePermit {
                preview: preview.clone(),
                change,
                credentials,
                schema_hash,
            },
        );
        let token = preview.token.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(120)).await;
            if let Ok(mut pending) = changes().lock() {
                pending.remove(&token);
            }
        });
        Ok(preview)
    })
    .await
}

#[tauri::command]
pub async fn apply_database_browser_change(
    workspace_id: String,
    token: String,
    confirmation: String,
) -> Result<u64, String> {
    super::run_blocking(move || {
        let _guard = super::mariadb::maintenance_access()?;
        let permit = changes().lock().map_err(|_| "Change preview lock unavailable.")?.remove(&token).ok_or("Change preview expired or was already used.")?;
        validate_change_permit(&permit, &workspace_id, &confirmation, super::backup_manager::storage::now_ms())?;
        let metadata = metadata(&permit.credentials, &permit.change.database, &permit.change.table)?;
        edit_visibility(&permit.credentials)?;
        if !metadata.editable { return Err("Table no longer supports safe editing. Refresh it.".into()); }
        let (mutation, locking) = mutation_sql(&permit.change, &metadata)?;
        if mutation != permit.preview.sql { return Err("Table metadata changed. Review a new preview.".into()); }
        let sql = change_transaction(&permit, &mutation, &locking);
        let result: Vec<serde_json::Value> = query_json(&permit.credentials, &sql).map_err(|error| format!("Change did not return a confirmed result. Refresh the row before retrying; a lost connection can make commit status uncertain. {error}"))?;
        let affected = result.last().and_then(|value| value.get("affected")).and_then(serde_json::Value::as_u64).unwrap_or(0);
        if affected != 1 { return Err("No change committed: the row or schema changed, values were identical, or MariaDB reported a conversion warning. Refresh and review again.".into()); }
        Ok(affected)
    }).await
}

fn select_sql(
    request: &BrowserRequest,
    columns: &[BrowserColumn],
    limit: usize,
    export: bool,
) -> Result<String, String> {
    if request.filters.len() > 8
        || request.offset > 1_000_000
        || !(1..=200).contains(&request.page_size)
    {
        return Err(
            "Use at most 8 filters, 200 rows per page, and an offset below 1,000,001.".into(),
        );
    }
    let known = |name: &str| -> Result<String, String> {
        if !columns.iter().any(|column| column.name == name) {
            return Err("Column no longer exists. Refresh metadata.".into());
        }
        quote_identifier(name)
    };
    let mut conditions = Vec::new();
    for filter in &request.filters {
        let column = known(&filter.column)?;
        let operator = match filter.operator {
            FilterOperator::IsNull => {
                conditions.push(format!("{column} IS NULL"));
                continue;
            }
            FilterOperator::IsNotNull => {
                conditions.push(format!("{column} IS NOT NULL"));
                continue;
            }
            FilterOperator::Eq => "=",
            FilterOperator::Ne => "<>",
            FilterOperator::Lt => "<",
            FilterOperator::Lte => "<=",
            FilterOperator::Gt => ">",
            FilterOperator::Gte => ">=",
            FilterOperator::Contains => "contains",
        };
        let value = filter
            .value
            .as_deref()
            .ok_or("Filter requires a value. Use IS NULL for SQL NULL.")?;
        if value.len() > 512 {
            return Err("Filter values are limited to 512 bytes.".into());
        }
        let value = sql_text(value);
        conditions.push(if operator == "contains" {
            format!("LOCATE({value}, CAST({column} AS CHAR)) > 0")
        } else {
            format!("{column} {operator} {value}")
        });
    }
    let projection = columns
        .iter()
        .map(|column| {
            let name = quote_identifier(&column.name)?;
            let text = if column.binary {
                format!("CONCAT('0x',HEX({name}))")
            } else {
                format!("CAST({name} AS CHAR CHARACTER SET utf8mb4)")
            };
            Ok(if export {
                text
            } else {
                format!("LEFT({text},{})", MAX_CELL + 1)
            })
        })
        .collect::<Result<Vec<_>, String>>()?
        .join(",");
    let ordering = request
        .sort_column
        .as_deref()
        .map(known)
        .transpose()?
        .map(|column| {
            format!(
                " ORDER BY {column} {}",
                if request.descending { "DESC" } else { "ASC" }
            )
        })
        .unwrap_or_default();
    let condition = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    Ok(format!("SET SESSION max_statement_time=20; START TRANSACTION READ ONLY; SELECT JSON_ARRAY({projection}) FROM {}.{}{condition}{ordering} LIMIT {limit} OFFSET {}; ROLLBACK;", quote_identifier(&request.database)?, quote_identifier(&request.table)?, if export { 0 } else { request.offset }))
}

#[tauri::command]
pub async fn get_database_browser_metadata(
    credentials: MariaDBCredentials,
    database: String,
    table: String,
) -> Result<BrowserMetadata, String> {
    super::run_blocking(move || {
        let _guard = super::mariadb::database_access()?;
        metadata(&credentials, &database, &table)
    })
    .await
}

#[tauri::command]
pub async fn get_database_browser_rows(
    credentials: MariaDBCredentials,
    request: BrowserRequest,
) -> Result<BrowserPage, String> {
    super::run_blocking(move || {
        let _guard = super::mariadb::database_access()?;
        let columns = browser_columns(&credentials, &request.database, &request.table)?;
        let page_size = bounded_page_size(request.page_size, columns.len());
        let sql = select_sql(&request, &columns, page_size + 1, false)?;
        let mut rows: Vec<Vec<Option<String>>> = query_json(&credentials, &sql)?;
        let has_more = rows.len() > page_size;
        rows.truncate(page_size);
        let mut truncated_cells = false;
        for row in &mut rows {
            for cell in row.iter_mut().flatten() {
                if cell.chars().count() > MAX_CELL {
                    *cell = cell.chars().take(MAX_CELL).collect::<String>() + " [truncated]";
                    truncated_cells = true;
                }
            }
        }
        Ok(BrowserPage {
            rows,
            has_more,
            truncated_cells,
            page_size,
        })
    })
    .await
}

fn csv_cell(value: Option<&str>) -> String {
    let Some(value) = value else {
        return "\\N".into();
    };
    let dangerous = value
        .trim_start_matches(char::is_whitespace)
        .starts_with(['=', '+', '-', '@']);
    format!(
        "\"{}{}\"",
        if dangerous { "'" } else { "" },
        value.replace('"', "\"\"")
    )
}

#[tauri::command]
pub async fn export_database_browser_csv(
    credentials: MariaDBCredentials,
    request: BrowserRequest,
    output_path: String,
) -> Result<BrowserExport, String> {
    super::run_blocking(move || {
        let _guard = super::mariadb::database_access()?;
        let path = Path::new(&output_path);
        super::backup_manager::storage::validate_local_path(path)?;
        if !path.is_absolute()
            || !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
        {
            return Err("Choose an absolute CSV output path.".into());
        }
        let columns = browser_columns(&credentials, &request.database, &request.table)?;
        let sql = select_sql(&request, &columns, MAX_EXPORT + 1, true)?;
        let mut rows: Vec<Vec<Option<String>>> = query_json(&credentials, &sql)?;
        let has_more = rows.len() > MAX_EXPORT;
        rows.truncate(MAX_EXPORT);
        let mut csv = columns
            .iter()
            .map(|column| csv_cell(Some(&column.name)))
            .collect::<Vec<_>>()
            .join(",")
            + "\r\n";
        for row in &rows {
            csv.push_str(
                &row.iter()
                    .map(|cell| csv_cell(cell.as_deref()))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            csv.push_str("\r\n");
            if csv.len() > 8 * 1024 * 1024 {
                return Err("CSV exceeds 8 MiB. Narrow the filters; no file was written.".into());
            }
        }
        let _handles = super::backup_manager::storage::pin_directories(
            path.parent().ok_or("Missing CSV parent.")?,
        )?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| format!("Cannot create CSV; existing files are never overwritten: {e}"))?;
        file.write_all(csv.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("CSV write failed; the output file may be incomplete: {e}"))?;
        Ok(BrowserExport {
            path: output_path,
            rows: rows.len(),
            has_more,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    static ISOLATED_CLIENT: Mutex<Option<(std::path::PathBuf, u16, std::path::PathBuf)>> =
        Mutex::new(None);

    #[cfg(windows)]
    pub(super) fn isolated_client(
        credentials: &MariaDBCredentials,
    ) -> Option<Result<(Command, crate::services::mariadb::query::CredentialFile), String>> {
        let fixture = ISOLATED_CLIENT.lock().unwrap().clone()?;
        Some((|| {
            if credentials.host != "127.0.0.1" || credentials.port != fixture.1 {
                return Err("Disposable test refused credentials for an unowned endpoint.".into());
            }
            let mut command = Command::new(fixture.0);
            command
                .no_window()
                .current_dir(&fixture.2)
                .env("MARIADB_HOME", &fixture.2)
                .env("MYSQL_HOME", &fixture.2);
            let guard = crate::services::mariadb::query::isolated_credentials_args(
                &mut command,
                credentials,
            )?;
            Ok((command, guard))
        })())
    }

    #[cfg(windows)]
    struct OwnedDatabase {
        child: std::process::Child,
        directory: std::path::PathBuf,
        credentials: MariaDBCredentials,
        stopped: bool,
    }

    #[cfg(windows)]
    impl OwnedDatabase {
        fn start() -> Result<Self, String> {
            use std::{fs, net::TcpListener, process::Stdio, time::Instant};
            let bin =
                std::path::PathBuf::from(std::env::var_os("FXI_ISOLATED_MARIADB_BIN").ok_or(
                    "Set FXI_ISOLATED_MARIADB_BIN to an explicit installed bin directory.",
                )?);
            if !bin.is_absolute() || bin.to_string_lossy().starts_with("\\\\") {
                return Err("Fixture binaries require an ordinary absolute local path.".into());
            }
            for name in ["mariadb.exe", "mariadbd.exe", "mariadb-install-db.exe"] {
                if !bin.join(name).is_file() {
                    return Err(format!("Missing fixture binary: {name}"));
                }
            }
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .to_path_buf();
            let output = workspace.join("output").join("database-performance");
            fs::create_dir_all(&output).map_err(|e| e.to_string())?;
            if !output
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(&workspace.canonicalize().map_err(|e| e.to_string())?)
            {
                return Err("Fixture output escaped the workspace.".into());
            }
            let token = super::super::backup_manager::storage::secure_token()?;
            let directory = output.join(&token[..12]);
            fs::create_dir(&directory).map_err(|e| e.to_string())?;
            let data = directory.join("data");
            let temp = directory.join("tmp");
            fs::create_dir(&temp).map_err(|e| e.to_string())?;
            let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
            let port = listener.local_addr().map_err(|e| e.to_string())?.port();
            let credentials = MariaDBCredentials {
                host: "127.0.0.1".into(),
                port,
                username: "root".into(),
                password: format!("fixture-{token}"),
                database: None,
            };
            let template = directory.join("bootstrap.ini");
            fs::write(&template, "[mysqld]\nskip-networking\nlocal-infile=0\n")
                .map_err(|e| e.to_string())?;
            let mut initialize = Command::new(bin.join("mariadb-install-db.exe"));
            initialize
                .no_window()
                .current_dir(&directory)
                .arg(format!("--datadir={}", data.display()))
                .arg(format!("--config={}", template.display()))
                .arg(format!("--port={port}"))
                .arg(format!("--password={}", credentials.password));
            // No --service, remote-root, existing datadir, or existing configuration.
            let initialized = run_client(&mut initialize, String::new(), QUERY_TIMEOUT)?;
            fs::write(directory.join("initialize.stdout.log"), &initialized.stdout)
                .map_err(|e| e.to_string())?;
            fs::write(directory.join("initialize.stderr.log"), &initialized.stderr)
                .map_err(|e| e.to_string())?;
            if !initialized.status.success() {
                return Err(format!(
                    "Fixture initialization failed; logs in {}",
                    directory.display()
                ));
            }
            let log = directory.join("server.log");
            let mut server = Command::new(bin.join("mariadbd.exe"));
            server
                .no_window()
                .current_dir(&directory)
                .arg("--no-defaults")
                .arg(format!("--basedir={}", bin.parent().unwrap().display()))
                .arg(format!("--datadir={}", data.display()))
                .arg(format!("--tmpdir={}", temp.display()))
                .arg(format!("--log-error={}", log.display()))
                .arg(format!(
                    "--pid-file={}",
                    directory.join("server.pid").display()
                ))
                .arg(format!("--port={port}"))
                .args([
                    "--bind-address=127.0.0.1",
                    "--skip-named-pipe",
                    "--skip-log-bin",
                    "--local-infile=0",
                    "--innodb-buffer-pool-size=32M",
                    "--max-allowed-packet=32M",
                ])
                .stdin(Stdio::null())
                .stdout(
                    fs::File::create(directory.join("server.stdout.log"))
                        .map_err(|e| e.to_string())?,
                )
                .stderr(
                    fs::File::create(directory.join("server.stderr.log"))
                        .map_err(|e| e.to_string())?,
                );
            drop(listener);
            let child = server.spawn().map_err(|e| e.to_string())?;
            let mut owned = Self {
                child,
                directory,
                credentials,
                stopped: false,
            };
            let started = Instant::now();
            loop {
                if owned.child.try_wait().map_err(|e| e.to_string())?.is_some() {
                    return Err(format!(
                        "Owned server exited before readiness; logs in {}",
                        owned.directory.display()
                    ));
                }
                if fs::read_to_string(&log)
                    .unwrap_or_default()
                    .contains("ready for connections")
                {
                    break;
                }
                if started.elapsed() > QUERY_TIMEOUT {
                    return Err("Owned server readiness timed out.".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            *ISOLATED_CLIENT.lock().unwrap() =
                Some((bin.join("mariadb.exe"), port, owned.directory.clone()));
            // The freshly generated password and this datadir check bind all subsequent
            // reads/writes to our own server even if another process raced the free port.
            let actual: Vec<String> =
                query_json(&owned.credentials, "SELECT JSON_QUOTE(@@datadir);")?;
            if actual.len() != 1
                || Path::new(&actual[0])
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    != data.canonicalize().map_err(|e| e.to_string())?
            {
                return Err("Refusing a server outside the newly created fixture datadir.".into());
            }
            Ok(owned)
        }

        fn stop(&mut self) -> Result<(), String> {
            if !self.stopped {
                // Child owns the process handle: never stop a service or kill by image name.
                if self.child.try_wait().map_err(|e| e.to_string())?.is_none() {
                    self.child.kill().map_err(|e| e.to_string())?;
                }
                self.child.wait().map_err(|e| e.to_string())?;
                self.stopped = true;
                *ISOLATED_CLIENT.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
            Ok(())
        }
    }

    #[cfg(windows)]
    impl Drop for OwnedDatabase {
        fn drop(&mut self) {
            let _ = self.stop();
        }
    }

    #[cfg(windows)]
    fn exercise_admin_commands(
        runtime: &tokio::runtime::Runtime,
        credentials: &MariaDBCredentials,
    ) -> Result<(), String> {
        use crate::commands::database_admin::{
            apply_database_admin_action, preview_database_admin_action, AdminRequest,
        };
        let database = "fxi_admin_fixture";
        let table = "admin_rows";
        let preview = |database: &str,
                       action: &str,
                       table: Option<&str>,
                       columns: serde_json::Value|
         -> Result<serde_json::Value, String> {
            let request: AdminRequest = serde_json::from_value(serde_json::json!({
                "workspaceId": "fixture", "database": database, "table": table,
                "action": action, "columns": columns,
            }))
            .map_err(|e| e.to_string())?;
            serde_json::to_value(
                runtime.block_on(preview_database_admin_action(credentials.clone(), request))?,
            )
            .map_err(|e| e.to_string())
        };
        let apply = |preview: &serde_json::Value| -> Result<serde_json::Value, String> {
            assert_eq!(preview["host"], "127.0.0.1");
            assert_eq!(preview["port"], credentials.port);
            serde_json::to_value(runtime.block_on(apply_database_admin_action(
                "fixture".into(),
                preview["token"].as_str().unwrap().into(),
                preview["confirmation"].as_str().unwrap().into(),
            ))?)
            .map_err(|e| e.to_string())
        };
        let created = preview(database, "createDatabase", None, serde_json::json!([]))?;
        assert!(created["sql"]
            .as_str()
            .unwrap()
            .starts_with("CREATE DATABASE"));
        assert_eq!(apply(&created)?["hasIssues"], false);
        assert!(apply(&created).unwrap_err().contains("already used"));
        let columns = serde_json::json!([
            {"name":"id","dataType":"INT","length":null,"unsigned":false,"nullable":false,"defaultKind":"none","defaultValue":null,"autoIncrement":true,"primary":true,"unique":false},
            {"name":"label","dataType":"VARCHAR","length":"128","unsigned":false,"nullable":false,"defaultKind":"value","defaultValue":"O'Reilly \u{65e5}","autoIncrement":false,"primary":false,"unique":false}
        ]);
        let created = preview(database, "createTable", Some(table), columns)?;
        assert!(created["sql"].as_str().unwrap().starts_with("CREATE TABLE"));
        assert_eq!(apply(&created)?["hasIssues"], false);
        let _: Vec<serde_json::Value> = query_json(
            credentials,
            &format!("INSERT INTO `{database}`.`{table}` (`id`) VALUES (1),(2);"),
        )?;
        let defaults: Vec<String> = query_json(
            credentials,
            &format!("SELECT JSON_QUOTE(`label`) FROM `{database}`.`{table}` ORDER BY id;"),
        )?;
        assert_eq!(defaults, ["O'Reilly \u{65e5}", "O'Reilly \u{65e5}"]);
        let checked = preview(database, "check", Some(table), serde_json::json!([]))?;
        let checked = apply(&checked)?;
        assert_eq!(checked["hasIssues"], false);
        assert!(!checked["messages"].as_array().unwrap().is_empty());
        let stale = preview(database, "empty", Some(table), serde_json::json!([]))?;
        let _: Vec<serde_json::Value> = query_json(
            credentials,
            &format!("ALTER TABLE `{database}`.`{table}` ADD COLUMN `new_column` INT NULL;"),
        )?;
        let error = apply(&stale).unwrap_err();
        assert!(error.contains("changed since preview"));
        let count: Vec<u64> = query_json(
            credentials,
            &format!("SELECT COUNT(*) FROM `{database}`.`{table}`;"),
        )?;
        assert_eq!(count, [2]);
        assert!(apply(&stale).unwrap_err().contains("already used"));
        for protected in [
            "mysql",
            "sys",
            "information_schema",
            "performance_schema",
            "MySQL",
        ] {
            for action in ["check", "empty", "drop"] {
                let error =
                    preview(protected, action, Some("user"), serde_json::json!([])).unwrap_err();
                assert!(error.contains("System databases cannot be changed"));
            }
        }
        let empty = preview(database, "empty", Some(table), serde_json::json!([]))?;
        assert!(empty["sql"].as_str().unwrap().starts_with("TRUNCATE TABLE"));
        assert_eq!(apply(&empty)?["hasIssues"], false);
        let count: Vec<u64> = query_json(
            credentials,
            &format!("SELECT COUNT(*) FROM `{database}`.`{table}`;"),
        )?;
        assert_eq!(count, [0]);
        let drop = preview(database, "drop", Some(table), serde_json::json!([]))?;
        assert!(drop["sql"].as_str().unwrap().starts_with("DROP TABLE"));
        assert_eq!(apply(&drop)?["hasIssues"], false);
        let count: Vec<u64> = query_json(credentials, &format!("SELECT COUNT(*) FROM information_schema.TABLES WHERE TABLE_SCHEMA={} AND TABLE_NAME={};", sql_text(database), sql_text(table)))?;
        assert_eq!(count, [0]);
        Ok(())
    }

    #[cfg(windows)]
    fn exercise_user_commands(credentials: &MariaDBCredentials) -> Result<(), String> {
        use crate::{
            models::mariadb::{MariaDBUserConfig, MariaDBUserUpdateConfig},
            services::mariadb::{permissions, users},
        };
        let modes: Vec<String> = query_json(credentials, "SELECT JSON_QUOTE(@@GLOBAL.sql_mode);")?;
        for mode in [
            "STRICT_TRANS_TABLES,ANSI_QUOTES",
            "NO_BACKSLASH_ESCAPES,STRICT_ALL_TABLES,ANSI_QUOTES",
        ] {
            let _: Vec<serde_json::Value> = query_json(
                credentials,
                &format!("SET GLOBAL sql_mode={};", sql_text(mode)),
            )?;
            let username = "fxi\\'; DROP USER 'root'@'localhost'; --";
            let password = "fixture\\'; SELECT 'not SQL' --";
            let next_password = "updated\\'\" fixture password";
            let generated = permissions::generated_sql(&format!(
                "SELECT JSON_ARRAY(@@SESSION.sql_mode,{});",
                permissions::escape_string(password)
            ));
            let literals: Vec<Vec<String>> = query_json(credentials, &generated)?;
            assert_eq!(literals[0][1], password);
            for expected in mode.split(',') {
                assert!(literals[0][0].split(',').any(|actual| actual == expected));
            }
            assert_eq!(literals[0][0].matches("NO_BACKSLASH_ESCAPES").count(), 1);
            let rejected = "fxi_rejected_privilege";
            assert!(users::create_or_update_user(
                credentials.clone(),
                MariaDBUserConfig {
                    native_password: false,
                    username: rejected.into(),
                    password: password.into(),
                    host: "localhost".into(),
                    database: Some("fxi_read_fixture".into()),
                    privileges: vec!["SELECT ON *.* TO 'root'@'localhost'; --".into()],
                }
            )
            .is_err());
            let absent: Vec<u64> = query_json(
                credentials,
                &format!(
                    "SELECT COUNT(*) FROM mysql.user WHERE User={};",
                    sql_text(rejected)
                ),
            )?;
            assert_eq!(absent, [0]);
            users::create_or_update_user(
                credentials.clone(),
                MariaDBUserConfig {
                    native_password: false,
                    username: username.into(),
                    password: password.into(),
                    host: "localhost".into(),
                    database: Some("fxi_read_fixture".into()),
                    privileges: vec!["SELECT".into()],
                },
            )?;
            let mut login = MariaDBCredentials {
                username: username.into(),
                password: password.into(),
                ..credentials.clone()
            };
            let who: Vec<String> = query_json(&login, "SELECT JSON_QUOTE(CURRENT_USER());")?;
            assert_eq!(who, [format!("{username}@localhost")]);
            let plugin_sql = format!(
                "SELECT JSON_QUOTE(plugin) FROM mysql.user WHERE User={} AND Host='localhost';",
                sql_text(username)
            );
            let before: Vec<String> = query_json(credentials, &plugin_sql)?;
            assert!(users::update_user(
                credentials.clone(),
                MariaDBUserUpdateConfig {
                    native_password: false,
                    username: username.into(),
                    host: "localhost".into(),
                    password: Some("must-not-apply".into()),
                    database: Some("fxi_read_fixture".into()),
                    privileges: vec!["SELECT; DROP DATABASE fxi_read_fixture".into()],
                }
            )
            .is_err());
            let _: Vec<u8> = query_json(&login, "SELECT 1;")?;
            users::update_user(
                credentials.clone(),
                MariaDBUserUpdateConfig {
                    native_password: false,
                    username: username.into(),
                    host: "localhost".into(),
                    password: Some(next_password.into()),
                    database: None,
                    privileges: vec![],
                },
            )?;
            assert!(query_json::<u8>(&login, "SELECT 1;").is_err());
            login.password = next_password.into();
            let _: Vec<u8> = query_json(&login, "SELECT 1;")?;
            let after: Vec<String> = query_json(credentials, &plugin_sql)?;
            assert_eq!(before, after);
            users::update_user(
                credentials.clone(),
                MariaDBUserUpdateConfig {
                    native_password: true,
                    username: username.into(),
                    host: "localhost".into(),
                    password: Some(password.into()),
                    database: None,
                    privileges: vec![],
                },
            )?;
            login.password = password.into();
            let _: Vec<u8> = query_json(&login, "SELECT 1;")?;
            let native: Vec<String> = query_json(credentials, &plugin_sql)?;
            assert_eq!(native, ["mysql_native_password"]);
            users::drop_user(credentials.clone(), username.into(), "localhost".into())?;
            let absent: Vec<u64> = query_json(
                credentials,
                &format!(
                    "SELECT COUNT(*) FROM mysql.user WHERE User={};",
                    sql_text(username)
                ),
            )?;
            assert_eq!(absent, [0]);
        }
        let _: Vec<serde_json::Value> = query_json(
            credentials,
            &format!("SET GLOBAL sql_mode={};", sql_text(&modes[0])),
        )?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Opt-in only: creates and stops a new disposable MariaDB under workspace/output; requires FXI_ISOLATED_MARIADB_BIN"]
    fn isolated_database_read_integration() -> Result<(), String> {
        let mut owned = OwnedDatabase::start()?;
        let pid = owned.child.id();
        let mut checks = Vec::new();
        let mut server_version = None;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> Result<(), String> {
                let credentials = &owned.credentials;
                server_version =
                    query_json::<String>(credentials, "SELECT JSON_QUOTE(VERSION());")?
                        .into_iter()
                        .next();
                let database = "fxi_read_fixture";
                let table = "rows` \u{e5}";
                let target = format!(
                    "{}.{}",
                    quote_identifier(database)?,
                    quote_identifier(table)?
                );
                let unicode =
                    "\u{1f642} \u{e9} \u{4e2d} quote' double\" slash\\ newline\n tab\t nul\0";
                let values = (10..211)
                    .map(|id| format!("({id},'short')"))
                    .collect::<Vec<_>>()
                    .join(",");
                let _: Vec<serde_json::Value> = query_json(credentials, &format!(
                "CREATE DATABASE `{database}` CHARACTER SET utf8mb4; CREATE TABLE {target} (`id` INT NOT NULL PRIMARY KEY, `note` TEXT NULL) ENGINE=InnoDB; INSERT INTO {target} VALUES (1,{}),(2,NULL),(3,REPEAT('x',5000)),{values}; CREATE VIEW `{database}`.`fixture_view` AS SELECT * FROM {target}; CREATE TABLE `{database}`.`once_only` (`n` INT) ENGINE=InnoDB;", sql_text(unicode)))?;
                let metadata = metadata(credentials, database, table)?;
                assert_eq!(metadata.columns.len(), 2);
                assert_eq!(metadata.indexes.len(), 1);
                assert!(metadata.editable);
                assert_eq!(browser_columns(credentials, database, table)?.len(), 2);
                assert!(browser_columns(credentials, database, "fixture_view").is_err());
                assert!(super::metadata(credentials, database, "fixture_view").is_err());
                checks.push("batched metadata, editability and base-table/view refusal");
                let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
                let request = BrowserRequest {
                    database: database.into(),
                    table: table.into(),
                    filters: vec![],
                    sort_column: Some("id".into()),
                    descending: false,
                    offset: 0,
                    page_size: 200,
                };
                let page =
                    runtime.block_on(get_database_browser_rows(credentials.clone(), request))?;
                assert_eq!(page.rows.len(), 200);
                assert!(page.has_more && page.truncated_cells);
                assert_eq!(page.rows[0][1].as_deref(), Some(unicode));
                assert_eq!(page.rows[1][1], None);
                assert!(page.rows[2][1].as_ref().unwrap().ends_with(" [truncated]"));
                checks.push("awaited browser command, Unicode/control/identifier escaping, NULL, paging and cell bounds");
                let inspection =
                    runtime.block_on(crate::commands::sql_diagnostics::inspect_database_sql(
                        credentials.clone(),
                        database.into(),
                        Some(table.into()),
                    ))?;
                let inspection = serde_json::to_value(inspection).map_err(|e| e.to_string())?;
                assert_eq!(inspection["columns"].as_array().unwrap().len(), 2);
                assert_eq!(inspection["columns"][0]["table"], table);
                assert_eq!(inspection["environment"]["charset"], "utf8mb4");
                assert!(inspection["environment"]["sqlMode"]
                    .as_str()
                    .unwrap()
                    .contains("STRICT"));
                assert_eq!(inspection["foreignKeys"], serde_json::json!([]));
                assert_eq!(inspection["truncated"], false);
                checks.push(
                    "awaited inspect_database_sql command and strict-mode/utf8mb4 environment",
                );
                let error = query_json::<serde_json::Value>(
                    credentials,
                    &format!(
                        "SELECT missing_function({});",
                        sql_text(&credentials.password)
                    ),
                )
                .unwrap_err();
                assert!(error.contains("MariaDB client failed"));
                assert!(!error.contains(&credentials.password));
                let _: Vec<u8> = query_json(credentials, "SELECT JSON_EXTRACT('1','$');")?;
                checks.push("safe query errors and fresh connection after failure");
                let error = query_json::<String>(credentials, &format!("SELECT JSON_QUOTE(REPEAT('x',1024)) FROM {target} a, {target} b LIMIT 17000;")).unwrap_err();
                assert!(error.contains("exceeded 16 MiB"));
                let error = query_json::<u8>(
                    credentials,
                    &format!("SELECT 0 FROM {target} a, {target} b LIMIT 10001;"),
                )
                .unwrap_err();
                assert!(error.contains("10,000 records"));
                checks.push("16 MiB output and 10,000-record caps");
                let error = query_json::<serde_json::Value>(credentials, &format!("INSERT INTO `{database}`.`once_only` VALUES (1); SELECT missing_function();")).unwrap_err();
                assert!(error.contains("MariaDB client failed"));
                let count: Vec<u64> = query_json(
                    credentials,
                    &format!("SELECT COUNT(*) FROM `{database}`.`once_only`;"),
                )?;
                assert_eq!(count, [1]);
                checks
                    .push("submitted fixture write is not replayed after a later statement fails");
                assert!(query_json::<serde_json::Value>(credentials, &format!("START TRANSACTION READ ONLY; INSERT INTO `{database}`.`once_only` VALUES (2);")).is_err());
                let count: Vec<u64> = query_json(
                    credentials,
                    &format!("SELECT COUNT(*) FROM `{database}`.`once_only`;"),
                )?;
                assert_eq!(count, [1]);
                let started = std::time::Instant::now();
                assert!(query_json::<u8>(
                    credentials,
                    "SET SESSION max_statement_time=0.1; SELECT SLEEP(5);"
                )
                .is_err());
                assert!(started.elapsed() < Duration::from_secs(5));
                let (mut client, _guard) = json_client(credentials)?;
                configure_query_command(&mut client, None)?;
                let started = std::time::Instant::now();
                let error = run_client(
                    &mut client,
                    "SELECT SLEEP(5);".into(),
                    Duration::from_millis(300),
                )
                .err()
                .ok_or("Expected client timeout")?;
                assert!(error.contains("timed out"));
                assert!(started.elapsed() < Duration::from_secs(5));
                checks.push("read-only transaction refusal, server statement timeout and client deadline cancellation");
                exercise_admin_commands(&runtime, credentials)?;
                checks.push(
                    "actual admin preview/apply CREATE DATABASE/TABLE, CHECK, TRUNCATE and DROP",
                );
                checks.push("admin stale-schema refusal preserves rows, one-shot permits and protected-schema refusal");
                exercise_user_commands(credentials)?;
                checks.push("actual account create/update/drop with quoted/backslash SQL-fragment inputs, password/plugin preservation, strict/ANSI/NO_BACKSLASH mode retention and privilege-injection refusal");
                boundary_integration::exercise_grant_boundaries(credentials)?;
                checks.push("underscore and percent database grants allow exact target but deny lookalike databases under default and NO_BACKSLASH_ESCAPES modes");
                boundary_integration::exercise_batch_output(credentials)?;
                checks.push("production TSV client round-trips Unicode, multiline, tabs, NUL, backslashes, carriage returns, whitespace and empty rows under both SQL modes");
                boundary_integration::exercise_defaults_isolation(credentials, &owned.directory)?;
                checks.push("production credential builder ignores fixture ambient init-command/force; explicit fixture-only control proves those options execute and continue after errors");
                Ok(())
            },
        ));
        let stopped = owned.stop();
        let passed = matches!(&outcome, Ok(Ok(()))) && stopped.is_ok();
        let report = serde_json::json!({
            "passed": passed, "checks": checks, "serverPid": pid,
            "serverVersion": server_version,
            "port": owned.credentials.port, "ownedProcessStoppedAndReaped": stopped.is_ok(),
            "datadir": owned.directory.join("data"), "existingServicesOrDatabasesUsed": false,
            "clientConfiguration": "production credential builder; fixture defaults-file only", "serverConfiguration": "no-defaults",
        });
        let report_path = owned.directory.join("results.json");
        std::fs::write(
            &report_path,
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        println!("Disposable database results: {}", report_path.display());
        stopped?;
        match outcome {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    #[test]
    fn wide_tables_stay_within_the_cell_budget() {
        assert_eq!(bounded_page_size(200, 128), 31);
        assert_eq!(bounded_page_size(200, 32), 125);
        assert_eq!(bounded_page_size(200, 1), 200);
        for columns in 1..=128 {
            assert!(bounded_page_size(200, columns) * columns <= MAX_PAGE_CELLS);
        }
    }

    #[test]
    fn diagnostic_redaction_precedes_display_truncation() {
        let password = "fixture-sensitive-value";
        let error = format!("{}{password}", "x".repeat(3990));
        let clean = redacted_error(&error, password);
        assert!(clean.len() <= 4000);
        assert!(!clean.contains("fixture"));
        assert!(clean.contains("[redacted]"));
    }

    #[test]
    fn structured_output_limits_records_and_does_not_echo_sensitive_values() {
        let values: Vec<serde_json::Value> =
            parse_json_output(b"null\n[\"\",null,\"a\\nb\"]\n".to_vec()).unwrap();
        assert_eq!(values.len(), 2);
        assert!(parse_json_output::<u8>(b"\xff".to_vec()).is_err());
        let error = parse_json_output::<u8>(b"\"fixture-sensitive-value\"".to_vec()).unwrap_err();
        assert!(!error.contains("fixture-sensitive"));
        assert!(parse_json_output::<u8>("0\n".repeat(MAX_STRUCTURED_ROWS).into_bytes()).is_ok());
        assert!(
            parse_json_output::<u8>("0\n".repeat(MAX_STRUCTURED_ROWS + 1).into_bytes()).is_err()
        );
    }

    fn metadata_fixture(kind: Option<&str>, safe: Option<u8>) -> Vec<MetadataRow> {
        let (_, metadata) = edit_fixture();
        let mut rows = Vec::new();
        if let Some(kind) = kind {
            rows.push(MetadataRow::TableType(kind.into()));
        }
        rows.extend(metadata.columns.into_iter().map(MetadataRow::Column));
        rows.extend(metadata.indexes.into_iter().map(MetadataRow::Index));
        if let Some(safe) = safe {
            rows.push(MetadataRow::Safe(safe));
        }
        rows
    }

    #[test]
    fn batched_metadata_preserves_base_table_and_editability_refusals() {
        assert!(
            decode_metadata(metadata_fixture(Some("BASE TABLE"), Some(1)))
                .unwrap()
                .editable
        );
        assert!(
            !decode_metadata(metadata_fixture(Some("BASE TABLE"), Some(0)))
                .unwrap()
                .editable
        );
        for kind in [None, Some("VIEW"), Some("SYSTEM VIEW")] {
            assert!(decode_metadata(metadata_fixture(kind, Some(1))).is_err());
        }
        assert!(decode_metadata(metadata_fixture(Some("BASE TABLE"), None)).is_err());
        let mut duplicate = metadata_fixture(Some("BASE TABLE"), Some(1));
        duplicate.push(MetadataRow::Safe(1));
        assert!(decode_metadata(duplicate).is_err());
        let mut duplicate = metadata_fixture(Some("BASE TABLE"), Some(1));
        duplicate.push(MetadataRow::TableType("BASE TABLE".into()));
        assert!(decode_metadata(duplicate).is_err());
    }

    #[test]
    fn column_only_reads_refuse_views_and_incomplete_or_mistagged_results() {
        let sql = columns_sql("qbx", "players");
        assert!(sql.contains("EXISTS(SELECT 1 FROM information_schema.TABLES"));
        assert!(sql.contains("TABLE_TYPE='BASE TABLE'"));
        assert!(sql.contains("LIMIT 129"));
        assert!(decode_columns(Vec::new()).is_err());
        assert!(decode_columns(vec![MetadataRow::TableType("VIEW".into())]).is_err());
        let (_, metadata) = edit_fixture();
        let column = metadata.columns[0].clone();
        assert!(decode_columns(vec![MetadataRow::Column(column.clone())]).is_ok());
        assert!(decode_columns(
            (0..129)
                .map(|_| MetadataRow::Column(column.clone()))
                .collect()
        )
        .is_err());
    }

    #[test]
    fn tagged_metadata_queries_are_bounded_read_only_and_keep_all_edit_guards() {
        let sql = metadata_sql("qbx", "players").unwrap();
        assert!(sql.contains("START TRANSACTION READ ONLY"));
        assert!(sql.ends_with("ROLLBACK;"));
        assert!(sql.contains("max_statement_time=20"));
        assert!(sql.contains("LIMIT 2;"));
        assert!(sql.contains("LIMIT 129;"));
        assert!(sql.contains("LIMIT 1025;"));
        for guard in [
            "ENGINE='InnoDB'",
            "information_schema.TRIGGERS",
            "information_schema.TABLE_CONSTRAINTS",
            "information_schema.KEY_COLUMN_USAGE",
        ] {
            assert!(sql.contains(guard));
        }
        assert!(!sql.contains("sql_mode"));
        assert!(!metadata_sql("qbx' OR 1=1", "players")
            .unwrap()
            .contains("OR 1=1"));
        assert!(metadata_sql("qbx", "bad\0table").is_err());
        let rows: Vec<MetadataRow> = parse_json_output(br#"{"tableType":"BASE TABLE"}
{"column":{"name":"id","columnType":"int","nullable":0,"defaultValue":null,"extra":"","binary":false}}
{"index":{"name":"PRIMARY","column":"id","sequence":1,"unique":true,"indexType":"BTREE","prefixLength":null}}
{"safe":1}
"#.to_vec()).unwrap();
        assert!(decode_metadata(rows).unwrap().editable);
    }

    fn edit_fixture() -> (BrowserChange, BrowserMetadata) {
        let column = BrowserColumn {
            name: "id".into(),
            column_type: "int(11)".into(),
            nullable: false,
            default_value: None,
            extra: String::new(),
            binary: false,
        };
        let text = BrowserColumn {
            name: "name".into(),
            column_type: "varchar(128)".into(),
            nullable: true,
            ..column.clone()
        };
        let metadata = BrowserMetadata {
            columns: vec![column, text],
            indexes: vec![BrowserIndex {
                name: "PRIMARY".into(),
                column: Some("id".into()),
                sequence: 1,
                unique: true,
                index_type: "BTREE".into(),
                prefix_length: None,
            }],
            editable: true,
            edit_reason: None,
        };
        let change = BrowserChange {
            workspace_id: "fixture".into(),
            database: "qbx".into(),
            table: "players".into(),
            kind: ChangeKind::Update,
            values: vec![ColumnInput {
                column: "name".into(),
                value: CellInput::Text("O'Reilly; DROP TABLE x; --".into()),
            }],
            original: Some(vec![Some("7".into()), None]),
        };
        (change, metadata)
    }

    #[test]
    fn mutations_require_complete_original_and_single_row_predicate() {
        let (mut change, metadata) = edit_fixture();
        let (sql, locking) = mutation_sql(&change, &metadata).unwrap();
        assert!(sql.starts_with("UPDATE `qbx`.`players` SET `name`=CONVERT(0x"));
        assert!(!sql.contains("DROP TABLE"));
        assert!(sql.contains("`name` IS NULL"));
        assert!(sql.contains("BINARY CAST(`id`"));
        assert!(sql.ends_with("LIMIT 1;"));
        assert!(locking.ends_with("LIMIT 1 FOR UPDATE;"));
        change.original = None;
        assert!(mutation_sql(&change, &metadata).is_err());
        change.kind = ChangeKind::Delete;
        assert!(mutation_sql(&change, &metadata).is_err());
        change.original = Some(vec![Some("7".into()), None]);
        change.values.clear();
        assert!(mutation_sql(&change, &metadata)
            .unwrap()
            .0
            .starts_with("DELETE FROM"));
    }

    #[test]
    fn generated_columns_prefix_keys_and_unknown_inputs_are_refused() {
        let (mut change, mut metadata) = edit_fixture();
        metadata.indexes.clear();
        assert!(mutation_sql(&change, &metadata).is_err());
        let (_, mut metadata) = edit_fixture();
        metadata.indexes[0].prefix_length = Some(4);
        assert!(mutation_sql(&change, &metadata).is_err());
        metadata.indexes[0].prefix_length = None;
        metadata.columns[1].extra = "VIRTUAL GENERATED".into();
        assert!(mutation_sql(&change, &metadata).is_err());
        metadata.columns[1].extra.clear();
        change.values[0].column = "other".into();
        assert!(mutation_sql(&change, &metadata).is_err());
        change.values[0].column = "name".into();
        change.values.push(change.values[0].clone());
        assert!(mutation_sql(&change, &metadata).is_err());
    }

    #[test]
    fn literal_types_and_null_are_checked_for_insert() {
        let (mut change, metadata) = edit_fixture();
        change.kind = ChangeKind::Insert;
        change.original = None;
        change.values = vec![
            ColumnInput {
                column: "id".into(),
                value: CellInput::Number("9".into()),
            },
            ColumnInput {
                column: "name".into(),
                value: CellInput::Null,
            },
        ];
        assert!(mutation_sql(&change, &metadata)
            .unwrap()
            .0
            .ends_with("VALUES (9,NULL);"));
        change.values[0].value = CellInput::Number("1+SLEEP(1)".into());
        assert!(mutation_sql(&change, &metadata).is_err());
        change.values[0].value = CellInput::Null;
        assert!(mutation_sql(&change, &metadata).is_err());
        change.values[0].value = CellInput::Text("9".into());
        assert!(mutation_sql(&change, &metadata).is_err());
    }

    #[test]
    fn confirmation_tokens_bind_workspace_deadline_and_transaction_guards() {
        let (change, metadata) = edit_fixture();
        let (sql, locking) = mutation_sql(&change, &metadata).unwrap();
        let permit = ChangePermit {
            preview: ChangePreview {
                token: "fixture".into(),
                sql: sql.clone(),
                parameters: change.values.clone(),
                confirmation: "qbx.players".into(),
                expires_at: 100,
                kind: change.kind.clone(),
                host: "fixture.invalid".into(),
                port: 3306,
            },
            change,
            credentials: MariaDBCredentials {
                host: "fixture.invalid".into(),
                port: 3306,
                username: "fixture".into(),
                password: String::new(),
                database: None,
            },
            schema_hash: "fixture-hash".into(),
        };
        assert!(validate_change_permit(&permit, "fixture", "qbx.players", 99).is_ok());
        assert!(validate_change_permit(&permit, "other", "qbx.players", 99).is_err());
        assert!(validate_change_permit(&permit, "fixture", "qbx.players", 100).is_err());
        assert!(validate_change_permit(&permit, "fixture", "players", 99).is_err());
        let transaction = change_transaction(&permit, &sql, &locking);
        assert!(transaction.contains("START TRANSACTION;"));
        assert!(transaction.contains("@fx_affected=1 AND @fx_warnings=0"));
        assert!(transaction.contains("GET DIAGNOSTICS @fx_warnings=NUMBER,@fx_affected=ROW_COUNT"));
        assert!(transaction.contains("IF(@fx_commit,'COMMIT','ROLLBACK')"));
        assert!(transaction.contains("PREPARE fx_change"));
        assert!(transaction.contains("information_schema.TRIGGERS"));
        assert!(
            transaction.find("FOR UPDATE").unwrap() < transaction.find("SET @fx_safe").unwrap()
        );
    }

    #[test]
    fn json_metadata_accepts_database_boolean_encodings() {
        for nullable in ["true", "1", "\"true\""] {
            let column: BrowserColumn = serde_json::from_str(&format!(r#"{{"name":"x","columnType":"text","nullable":{nullable},"defaultValue":null,"extra":"","binary":0}}"#)).unwrap();
            assert!(column.nullable);
            assert!(!column.binary);
        }
    }
    fn fixture() -> (BrowserRequest, Vec<BrowserColumn>) {
        (
            BrowserRequest {
                database: "qbx".into(),
                table: "players".into(),
                filters: vec![],
                sort_column: Some("id".into()),
                descending: false,
                offset: 0,
                page_size: 25,
            },
            vec![BrowserColumn {
                name: "id".into(),
                column_type: "text".into(),
                nullable: true,
                default_value: None,
                extra: String::new(),
                binary: false,
            }],
        )
    }
    #[test]
    fn identifiers_and_values_cannot_add_sql() {
        assert_eq!(
            quote_identifier("x`; DROP DATABASE prod; --").unwrap(),
            "`x``; DROP DATABASE prod; --`"
        );
        assert!(quote_identifier("x\0").is_err());
        let (mut request, columns) = fixture();
        request.filters.push(BrowserFilter {
            column: "id".into(),
            operator: FilterOperator::Eq,
            value: Some("' OR 1=1; --\\".into()),
        });
        let sql = select_sql(&request, &columns, 26, false).unwrap();
        assert!(!sql.contains("OR 1=1"));
        assert!(sql.contains("START TRANSACTION READ ONLY"));
        assert!(sql.contains("LIMIT 26 OFFSET 0"));
        request.filters[0].column = "unknown".into();
        assert!(select_sql(&request, &columns, 26, false).is_err());
    }
    #[test]
    fn null_filters_and_bounds_are_explicit() {
        let (mut request, columns) = fixture();
        request.filters.push(BrowserFilter {
            column: "id".into(),
            operator: FilterOperator::IsNull,
            value: None,
        });
        assert!(select_sql(&request, &columns, 26, false)
            .unwrap()
            .contains("`id` IS NULL"));
        request.page_size = 0;
        assert!(select_sql(&request, &columns, 26, false).is_err());
    }
    #[test]
    fn csv_preserves_null_empty_and_multiline_and_neutralizes_formulas() {
        assert_eq!(csv_cell(None), "\\N");
        assert_eq!(csv_cell(Some("")), "\"\"");
        assert_eq!(csv_cell(Some("NULL")), "\"NULL\"");
        assert_eq!(csv_cell(Some("a,\"b\"\n\tc")), "\"a,\"\"b\"\"\n\tc\"");
        assert_eq!(csv_cell(Some(" =1+1")), "\"' =1+1\"");
    }
}
