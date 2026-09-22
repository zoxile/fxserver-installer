//! Bounded, read-only Database Browser inspection. No caller-provided SQL.
use serde::{Deserialize, Serialize};

use super::database_browser::{query_json, quote_identifier, sql_text};
use crate::models::mariadb::MariaDBCredentials;

const LIMIT: usize = 200;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InspectionView {
    Overview,
    Status,
    Processes,
    Variables,
    Charsets,
    Engines,
    Users,
    Grants,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionResult {
    columns: Vec<String>,
    rows: Vec<Vec<Option<String>>>,
    has_more: bool,
    limit: usize,
    notice: String,
}

fn inspection_sql(view: InspectionView, database: &str) -> Result<(Vec<String>, String), String> {
    let (columns, expressions, source) = match view {
        InspectionView::Overview => {
            quote_identifier(database)?;
            (vec!["Database", "Table", "Kind", "Engine", "Estimated rows", "Data bytes", "Index bytes", "Free bytes", "Collation"],
             vec!["TABLE_SCHEMA", "TABLE_NAME", "TABLE_TYPE", "ENGINE", "TABLE_ROWS", "DATA_LENGTH", "INDEX_LENGTH", "DATA_FREE", "TABLE_COLLATION"],
             format!("information_schema.TABLES WHERE TABLE_SCHEMA={} ORDER BY TABLE_NAME", sql_text(database)))
        }
        InspectionView::Status => (vec!["Variable", "Value"], vec!["VARIABLE_NAME", "VARIABLE_VALUE"], "information_schema.GLOBAL_STATUS ORDER BY VARIABLE_NAME".into()),
        InspectionView::Variables => (vec!["Variable", "Value"], vec!["VARIABLE_NAME", "CASE WHEN LOWER(VARIABLE_NAME) REGEXP 'password|secret|token|key|credential' THEN '[redacted]' ELSE VARIABLE_VALUE END"], "information_schema.GLOBAL_VARIABLES ORDER BY VARIABLE_NAME".into()),
        InspectionView::Processes => (vec!["ID", "User", "Host", "Database", "Command", "Seconds", "State"], vec!["ID", "USER", "HOST", "DB", "COMMAND", "TIME", "STATE"], "information_schema.PROCESSLIST ORDER BY TIME DESC,ID".into()),
        InspectionView::Charsets => (vec!["Charset", "Default collation", "Description", "Max bytes"], vec!["CHARACTER_SET_NAME", "DEFAULT_COLLATE_NAME", "DESCRIPTION", "MAXLEN"], "information_schema.CHARACTER_SETS ORDER BY CHARACTER_SET_NAME".into()),
        InspectionView::Engines => (vec!["Engine", "Support", "Transactions", "Savepoints", "Description"], vec!["ENGINE", "SUPPORT", "TRANSACTIONS", "SAVEPOINTS", "COMMENT"], "information_schema.ENGINES ORDER BY ENGINE".into()),
        InspectionView::Users => (vec!["User", "Host", "Authentication plugin"], vec!["User", "Host", "plugin"], "mysql.user ORDER BY User,Host".into()),
        InspectionView::Grants => (vec!["Grantee", "Scope", "Database", "Table", "Column", "Privilege", "Grantable"], vec!["GRANTEE", "scope", "db", "tbl", "col", "PRIVILEGE_TYPE", "IS_GRANTABLE"], "(SELECT GRANTEE,'GLOBAL' AS scope,NULL AS db,NULL AS tbl,NULL AS col,PRIVILEGE_TYPE,IS_GRANTABLE FROM information_schema.USER_PRIVILEGES UNION ALL SELECT GRANTEE,'DATABASE',TABLE_SCHEMA,NULL,NULL,PRIVILEGE_TYPE,IS_GRANTABLE FROM information_schema.SCHEMA_PRIVILEGES UNION ALL SELECT GRANTEE,'TABLE',TABLE_SCHEMA,TABLE_NAME,NULL,PRIVILEGE_TYPE,IS_GRANTABLE FROM information_schema.TABLE_PRIVILEGES UNION ALL SELECT GRANTEE,'COLUMN',TABLE_SCHEMA,TABLE_NAME,COLUMN_NAME,PRIVILEGE_TYPE,IS_GRANTABLE FROM information_schema.COLUMN_PRIVILEGES) AS grants_view ORDER BY GRANTEE,scope,db,tbl,col,PRIVILEGE_TYPE".into()),
    };
    let cells = expressions
        .iter()
        .map(|expression| format!("LEFT(CAST({expression} AS CHAR),1024)"))
        .collect::<Vec<_>>()
        .join(",");
    Ok((
        columns.into_iter().map(str::to_owned).collect(),
        format!(
            "SET SESSION max_statement_time=10; SELECT JSON_ARRAY({cells}) FROM {source} LIMIT {};",
            LIMIT + 1
        ),
    ))
}

#[tauri::command]
pub async fn inspect_database(
    credentials: MariaDBCredentials,
    view: InspectionView,
    database: String,
) -> Result<InspectionResult, String> {
    super::run_blocking(move || {
        let _guard = super::mariadb::database_access()?;
        let (columns, sql) = inspection_sql(view, &database)?;
        let mut rows: Vec<Vec<Option<String>>> = query_json(&credentials, &sql)?;
        let has_more = rows.len() > LIMIT;
        rows.truncate(LIMIT);
        Ok(InspectionResult {
            columns, rows, has_more, limit: LIMIT,
            notice: match view {
                InspectionView::Overview => "Row counts and storage statistics are estimates.",
                InspectionView::Processes => "Only visible sessions are included. SQL text is omitted to avoid exposing query secrets.",
                InspectionView::Variables => "Potentially sensitive variable values are redacted.",
                InspectionView::Users => "Account names and authentication plugins only; no password hashes.",
                InspectionView::Grants => "Visible explicit privileges only. Role inheritance and effective permissions are not expanded.",
                _ => "Read-only server snapshot.",
            }.into(),
        })
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_views_are_fixed_bounded_reads() {
        for view in [
            InspectionView::Overview,
            InspectionView::Status,
            InspectionView::Processes,
            InspectionView::Variables,
            InspectionView::Charsets,
            InspectionView::Engines,
            InspectionView::Users,
            InspectionView::Grants,
        ] {
            let (columns, sql) = inspection_sql(view, "a'\\`b").unwrap();
            assert!(!columns.is_empty());
            assert!(sql.ends_with("LIMIT 201;"));
            assert!(sql.starts_with("SET SESSION max_statement_time=10; SELECT JSON_ARRAY("));
            assert!(sql.contains(",1024)"));
            assert!(!sql.contains("a'\\`b"));
        }
    }

    #[test]
    fn secrets_are_not_selected() {
        let (_, users) = inspection_sql(InspectionView::Users, "").unwrap();
        assert!(!users.contains("authentication_string"));
        assert!(!users.contains("Password"));
        let (_, processes) = inspection_sql(InspectionView::Processes, "").unwrap();
        assert!(!processes.contains("INFO"));
        let (_, variables) = inspection_sql(InspectionView::Variables, "").unwrap();
        assert!(variables.contains("'[redacted]'"));
    }
}
