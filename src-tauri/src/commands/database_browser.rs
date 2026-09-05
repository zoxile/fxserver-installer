use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use crate::{
    models::mariadb::MariaDBCredentials,
    process::CommandNoWindowExt,
    services::mariadb::query::{apply_credentials_args, find_mariadb_client},
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

const MAX_OUTPUT: usize = 16 * 1024 * 1024;
const MAX_CELL: usize = 4096;
const MAX_EXPORT: usize = 5000;
const MAX_PAGE_CELLS: usize = 4000;

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

fn bounded_read(mut input: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take((MAX_OUTPUT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_OUTPUT {
        return Err("Query output exceeded 16 MiB. Narrow the filters or page size.".into());
    }
    Ok(bytes)
}

pub(crate) fn query_json<T: DeserializeOwned>(
    credentials: &MariaDBCredentials,
    sql: &str,
) -> Result<Vec<T>, String> {
    let result = (|| {
        if sql.len() > 24000 {
            return Err("Query is too large. Reduce the number or length of filters.".into());
        }
        let client = find_mariadb_client().ok_or("MariaDB client is unavailable.")?;
        let mut command = Command::new(client);
        command.no_window().arg("--no-defaults");
        apply_credentials_args(&mut command, credentials);
        command
            .args([
                "--batch",
                "--raw",
                "--skip-column-names",
                "--quick",
                "--binary-mode",
                "--skip-reconnect",
                "--local-infile=0",
                "--connect-timeout=10",
                "--default-character-set=utf8mb4",
                "--init-command=SET SESSION sql_mode=''",
            ])
            .arg("-e")
            .arg(sql)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot start MariaDB: {e}"))?;
        let stdout = child.stdout.take().ok_or("MariaDB output unavailable.")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("MariaDB error output unavailable.")?;
        let output = thread::spawn(move || bounded_read(stdout));
        let errors = thread::spawn(move || bounded_read(stderr));
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if started.elapsed() < Duration::from_secs(30) => {
                    thread::sleep(Duration::from_millis(25))
                }
                outcome => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(match outcome {
                        Err(e) => e.to_string(),
                        _ => "Database query timed out after 30 seconds.".into(),
                    });
                }
            }
        };
        let stdout = output.join().map_err(|_| "Query output worker failed.")??;
        let stderr = errors.join().map_err(|_| "Query error worker failed.")??;
        if !status?.success() {
            return Err(String::from_utf8_lossy(&stderr)
                .chars()
                .take(4000)
                .collect());
        }
        let text = String::from_utf8(stdout).map_err(|_| "MariaDB output was not UTF-8.")?;
        text.lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                serde_json::from_str(line)
                    .map_err(|e| format!("Invalid structured MariaDB result: {e}"))
            })
            .collect()
    })();
    result.map_err(|error: String| {
        if credentials.password.is_empty() {
            error
        } else {
            error.replace(&credentials.password, "[redacted]")
        }
    })
}

fn metadata(
    credentials: &MariaDBCredentials,
    database: &str,
    table: &str,
) -> Result<BrowserMetadata, String> {
    quote_identifier(database)?;
    quote_identifier(table)?;
    let schema = sql_text(database);
    let name = sql_text(table);
    let kinds: Vec<String> = query_json(credentials, &format!("SELECT JSON_QUOTE(TABLE_TYPE) FROM information_schema.TABLES WHERE TABLE_SCHEMA={schema} AND TABLE_NAME={name};"))?;
    if kinds != ["BASE TABLE"] {
        return Err("Choose an accessible base table. Views are not browsed.".into());
    }
    let columns = query_json(credentials, &format!("SELECT JSON_OBJECT('name',COLUMN_NAME,'columnType',COLUMN_TYPE,'nullable',IF(IS_NULLABLE='YES',JSON_EXTRACT('true','$'),JSON_EXTRACT('false','$')),'defaultValue',COLUMN_DEFAULT,'extra',EXTRA,'binary',IF(DATA_TYPE IN ('binary','varbinary','tinyblob','blob','mediumblob','longblob','bit','geometry'),JSON_EXTRACT('true','$'),JSON_EXTRACT('false','$'))) FROM information_schema.COLUMNS WHERE TABLE_SCHEMA={schema} AND TABLE_NAME={name} ORDER BY ORDINAL_POSITION LIMIT 129;"))?;
    let indexes = query_json(credentials, &format!("SELECT JSON_OBJECT('name',INDEX_NAME,'column',COLUMN_NAME,'sequence',SEQ_IN_INDEX,'unique',IF(NON_UNIQUE=0,JSON_EXTRACT('true','$'),JSON_EXTRACT('false','$')),'indexType',INDEX_TYPE,'prefixLength',SUB_PART) FROM information_schema.STATISTICS WHERE TABLE_SCHEMA={schema} AND TABLE_NAME={name} ORDER BY INDEX_NAME,SEQ_IN_INDEX LIMIT 1024;"))?;
    let mut value = BrowserMetadata {
        columns,
        indexes,
        editable: false,
        edit_reason: None,
    };
    if value.columns.is_empty() || value.columns.len() > 128 {
        return Err("Browser supports tables with 1 to 128 columns.".into());
    }
    value.edit_reason = edit_columns(&value).err();
    if value.edit_reason.is_none() {
        let safe: Vec<u8> = query_json(
            credentials,
            &format!(
                "SELECT JSON_EXTRACT(IF({},'1','0'),'$');",
                safe_table(database, table)
            ),
        )?;
        if safe != [1] {
            value.edit_reason = Some("Editing requires InnoDB without triggers, check constraints or foreign-key relationships.".into());
        }
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
                || index.column.as_deref().map_or(true, |name| {
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
    format!("SET SESSION sql_mode='STRICT_ALL_TABLES,NO_AUTO_VALUE_ON_ZERO',group_concat_max_len=1048576,innodb_lock_wait_timeout=5,max_statement_time=15; START TRANSACTION; {locking} SET @fx_safe=({gate}); SET @fx_change=IF(@fx_safe,{},'SELECT JSON_ARRAY()'); PREPARE fx_change FROM @fx_change; EXECUTE fx_change; SET @fx_affected=ROW_COUNT(); DEALLOCATE PREPARE fx_change; SET @fx_finish=IF(@fx_safe AND @fx_affected=1,'COMMIT','ROLLBACK'); PREPARE fx_finish FROM @fx_finish; EXECUTE fx_finish; DEALLOCATE PREPARE fx_finish; SELECT JSON_OBJECT('affected',IF(@fx_safe,@fx_affected,0));", sql_text(mutation.trim_end_matches(';')))
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
        let hash: Vec<String> = query_json(
            &credentials,
            &format!(
                "SET SESSION group_concat_max_len=1048576; SELECT JSON_QUOTE({});",
                schema_hash(&change.database, &change.table)
            ),
        )?;
        let schema_hash = hash
            .into_iter()
            .next()
            .ok_or("Table schema fingerprint unavailable.")?;
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
        if affected != 1 { return Err("No change committed: the row or schema changed, or the new values were identical. Refresh and review again.".into()); }
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
        let columns = metadata(&credentials, &request.database, &request.table)?.columns;
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
        if !path.is_absolute()
            || !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
        {
            return Err("Choose an absolute CSV output path.".into());
        }
        let columns = metadata(&credentials, &request.database, &request.table)?.columns;
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

    #[test]
    fn wide_tables_stay_within_the_cell_budget() {
        assert_eq!(bounded_page_size(200, 128), 31);
        assert_eq!(bounded_page_size(200, 32), 125);
        assert_eq!(bounded_page_size(200, 1), 200);
        for columns in 1..=128 {
            assert!(bounded_page_size(200, columns) * columns <= MAX_PAGE_CELLS);
        }
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
        assert!(transaction.contains("@fx_affected=1,'COMMIT','ROLLBACK'"));
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
        assert!(bounded_read(vec![0; MAX_OUTPUT + 1].as_slice()).is_err());
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
