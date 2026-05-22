use std::{path::PathBuf, process::Command};

use crate::models::mariadb::{MariaDBCredentials, MariaDBQueryResult};
use crate::process::CommandNoWindowExt;
use crate::services::mariadb::{detect::get_install_path, permissions::escape_identifier};

pub fn execute_query(
    credentials: MariaDBCredentials,
    query: String,
) -> Result<MariaDBQueryResult, String> {
    if query.trim().is_empty() {
        return Err("Query cannot be empty.".to_string());
    }

    let client = find_mariadb_client().ok_or_else(|| {
        "Could not find mariadb.exe. Install MariaDB or add its bin folder to PATH.".to_string()
    })?;

    let mut command = Command::new(client);
    command.no_window();
    apply_credentials_args(&mut command, &credentials);
    command.arg("--batch").arg("--raw").arg("-e").arg(query);

    if let Some(database) = credentials
        .database
        .filter(|value| !value.trim().is_empty())
    {
        command.arg(database);
    }

    let output = command
        .output()
        .map_err(|error| format!("Failed to execute MariaDB client: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let (columns, rows) = parse_tabular_output(&stdout);

    Ok(MariaDBQueryResult {
        success: output.status.success(),
        stdout,
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        columns,
        rows,
    })
}

pub fn run_admin_query(credentials: MariaDBCredentials, query: String) -> Result<(), String> {
    let result = execute_query(credentials, query)?;

    if result.success {
        Ok(())
    } else {
        Err(if result.stderr.is_empty() {
            "MariaDB rejected the query.".to_string()
        } else {
            result.stderr
        })
    }
}

pub fn validate_connection(credentials: MariaDBCredentials) -> Result<(), String> {
    run_admin_query(credentials, "SELECT 1;".to_string())
}

pub fn list_databases(credentials: MariaDBCredentials) -> Result<Vec<String>, String> {
    let result = execute_query(credentials, "SHOW DATABASES;".to_string())?;
    if !result.success {
        return Err(if result.stderr.is_empty() {
            "MariaDB rejected the database list query.".to_string()
        } else {
            result.stderr
        });
    }

    Ok(result
        .rows
        .into_iter()
        .filter_map(|row| row.into_iter().next())
        .filter(|database| !database.trim().is_empty())
        .collect())
}

pub fn list_tables(
    credentials: MariaDBCredentials,
    database: String,
) -> Result<Vec<String>, String> {
    let database = escape_identifier(&database)?;
    let result = execute_query(
        credentials,
        format!("SHOW FULL TABLES FROM {database} WHERE Table_type = 'BASE TABLE';"),
    )?;
    if !result.success {
        return Err(if result.stderr.is_empty() {
            "MariaDB rejected the table list query.".to_string()
        } else {
            result.stderr
        });
    }

    Ok(result
        .rows
        .into_iter()
        .filter_map(|row| row.into_iter().next())
        .filter(|table| !table.trim().is_empty())
        .collect())
}

pub(crate) fn apply_credentials_args(command: &mut Command, credentials: &MariaDBCredentials) {
    command
        .arg("--protocol=tcp")
        .arg("--ssl=0")
        .arg(format!("--user={}", credentials.username))
        .arg(format!("--password={}", credentials.password))
        .arg(format!("--host={}", credentials.host))
        .arg(format!("--port={}", credentials.port));
}

pub(crate) fn find_mariadb_client() -> Option<String> {
    if let Some(install_path) = get_install_path() {
        let client_path = PathBuf::from(install_path).join("bin").join("mariadb.exe");
        if client_path.exists() {
            return Some(client_path.to_string_lossy().to_string());
        }
    }

    for command in ["mariadb", "mariadb.exe"] {
        if let Ok(output) = Command::new(command).no_window().arg("--version").output() {
            let version = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if output.status.success() && version.contains("mariadb") {
                return Some(command.to_string());
            }
        }
    }

    None
}

fn parse_tabular_output(stdout: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut lines = stdout.lines();
    let Some(header) = lines.next() else {
        return (Vec::new(), Vec::new());
    };

    if !header.contains('\t') && lines.clone().next().is_none() {
        return (Vec::new(), Vec::new());
    }

    let columns = header.split('\t').map(str::to_string).collect();
    let rows = lines
        .map(|line| line.split('\t').map(str::to_string).collect())
        .collect();

    (columns, rows)
}
