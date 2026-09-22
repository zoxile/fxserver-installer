use super::database_browser::{query_json, quote_identifier, sql_text};
use crate::models::mariadb::MariaDBCredentials;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlEnvironment {
    version: String,
    sql_mode: String,
    charset: String,
    collation: String,
    schema_collation: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDiagnostic {
    table: String,
    column: String,
    column_type: String,
    charset: Option<String>,
    collation: Option<String>,
    engine: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyDiagnostic {
    constraint: String,
    column: String,
    parent_database: String,
    parent_table: String,
    parent_column: String,
    parent_type: Option<String>,
    parent_collation: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlInspection {
    environment: SqlEnvironment,
    columns: Vec<ColumnDiagnostic>,
    foreign_keys: Vec<ForeignKeyDiagnostic>,
    truncated: bool,
}

fn column_query(database: &str, table: Option<&str>) -> Result<String, String> {
    quote_identifier(database)?;
    let table_filter = match table {
        Some(table) => {
            quote_identifier(table)?;
            format!(" AND c.TABLE_NAME = {}", sql_text(table))
        }
        None => String::new(),
    };
    Ok(format!("SET SESSION max_statement_time=10; SELECT JSON_OBJECT('table',c.TABLE_NAME,'column',c.COLUMN_NAME,'columnType',LEFT(c.COLUMN_TYPE,1024),'charset',c.CHARACTER_SET_NAME,'collation',c.COLLATION_NAME,'engine',t.ENGINE) FROM information_schema.COLUMNS c JOIN information_schema.TABLES t ON t.TABLE_SCHEMA=c.TABLE_SCHEMA AND t.TABLE_NAME=c.TABLE_NAME WHERE c.TABLE_SCHEMA={} AND t.TABLE_TYPE='BASE TABLE'{table_filter} ORDER BY c.TABLE_NAME,c.ORDINAL_POSITION LIMIT 501;", sql_text(database)))
}

#[tauri::command]
pub async fn inspect_database_sql(
    credentials: MariaDBCredentials,
    database: String,
    table: Option<String>,
) -> Result<SqlInspection, String> {
    super::run_blocking(move || {
        let _access = super::mariadb::database_access()?;
        let columns_sql = column_query(&database, table.as_deref())?;
        let mut credentials = credentials;
        credentials.database = None;
        let environment = query_json::<SqlEnvironment>(&credentials, &format!("SET SESSION max_statement_time=10; SELECT JSON_OBJECT('version',VERSION(),'sqlMode',@@GLOBAL.sql_mode,'charset',@@character_set_connection,'collation',@@collation_connection,'schemaCollation',(SELECT DEFAULT_COLLATION_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME={}));", sql_text(&database)))?.into_iter().next().ok_or("No server metadata was returned.")?;
        if environment.schema_collation.is_none() { return Err("Database does not exist or is not visible to this account.".into()); }
        let mut columns = query_json::<ColumnDiagnostic>(&credentials, &columns_sql)?;
        let mut foreign_keys = if let Some(table) = table.as_deref() {
            query_json::<ForeignKeyDiagnostic>(&credentials, &format!("SET SESSION max_statement_time=10; SELECT JSON_OBJECT('constraint',k.CONSTRAINT_NAME,'column',k.COLUMN_NAME,'parentDatabase',k.REFERENCED_TABLE_SCHEMA,'parentTable',k.REFERENCED_TABLE_NAME,'parentColumn',k.REFERENCED_COLUMN_NAME,'parentType',LEFT(c.COLUMN_TYPE,1024),'parentCollation',c.COLLATION_NAME) FROM information_schema.KEY_COLUMN_USAGE k LEFT JOIN information_schema.COLUMNS c ON c.TABLE_SCHEMA=k.REFERENCED_TABLE_SCHEMA AND c.TABLE_NAME=k.REFERENCED_TABLE_NAME AND c.COLUMN_NAME=k.REFERENCED_COLUMN_NAME WHERE k.TABLE_SCHEMA={} AND k.TABLE_NAME={} AND k.REFERENCED_TABLE_NAME IS NOT NULL ORDER BY k.CONSTRAINT_NAME,k.ORDINAL_POSITION LIMIT 501;", sql_text(&database), sql_text(table)))?
        } else { Vec::new() };
        let truncated = columns.len() > 500 || foreign_keys.len() > 500;
        columns.truncate(500);
        foreign_keys.truncate(500);
        Ok(SqlInspection { environment, columns, foreign_keys, truncated })
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_names_are_literals_not_executable_sql() {
        let sql = column_query("db'; DROP DATABASE data; --", Some("table`'\\name")).unwrap();
        assert!(!sql.contains("DROP DATABASE"));
        assert!(!sql.contains("table`'"));
        assert!(sql.starts_with("SET SESSION max_statement_time=10; SELECT "));
        assert!(sql.ends_with("LIMIT 501;"));
        assert!(column_query("", None).is_err());
        assert!(column_query("db", Some("bad\nname")).is_err());
    }
}
