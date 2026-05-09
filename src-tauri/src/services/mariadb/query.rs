use std::{
    env, fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::models::mariadb::{MariaDBCredentials, MariaDBQueryResult};
use crate::services::mariadb::detect::get_install_path;

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
    let defaults_path = write_defaults_file(&credentials)?;

    let mut command = Command::new(client);
    command
        .arg(format!("--defaults-extra-file={}", defaults_path.display()))
        .arg("--batch")
        .arg("--raw")
        .arg("-e")
        .arg(query);

    if let Some(database) = credentials
        .database
        .filter(|value| !value.trim().is_empty())
    {
        command.arg(database);
    }

    let output = command
        .output()
        .map_err(|error| format!("Failed to execute MariaDB client: {error}"));

    let _ = fs::remove_file(&defaults_path);

    let output = output?;
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

fn write_defaults_file(credentials: &MariaDBCredentials) -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("System clock error: {error}"))?
        .as_millis();
    let path = env::temp_dir().join(format!("fxserver_mariadb_{timestamp}.cnf"));
    let content = format!(
        "[client]\nuser={}\npassword={}\nhost={}\nport={}\n",
        credentials.username, credentials.password, credentials.host, credentials.port
    );

    fs::write(&path, content)
        .map_err(|error| format!("Failed to write temporary credentials: {error}"))?;
    Ok(path)
}

fn find_mariadb_client() -> Option<String> {
    if let Some(install_path) = get_install_path() {
        let client_path = PathBuf::from(install_path).join("bin").join("mariadb.exe");
        if client_path.exists() {
            return Some(client_path.to_string_lossy().to_string());
        }
    }

    for command in ["mariadb", "mariadb.exe"] {
        if let Ok(output) = Command::new(command).arg("--version").output() {
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
