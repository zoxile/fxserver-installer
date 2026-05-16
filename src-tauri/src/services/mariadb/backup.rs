use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    models::mariadb::{MariaDBBackupOptions, MariaDBBackupResult, MariaDBCredentials},
    services::mariadb::{
        detect::get_install_path,
        query::{find_mariadb_client, write_defaults_file},
    },
};

pub fn create_backup(
    credentials: MariaDBCredentials,
    options: MariaDBBackupOptions,
) -> Result<MariaDBBackupResult, String> {
    validate_backup_options(&options)?;

    let dump_client = find_dump_client().ok_or_else(|| {
        "Could not find mariadb-dump.exe or mysqldump.exe. Install MariaDB or add its bin folder to PATH."
            .to_string()
    })?;
    let defaults_path = write_defaults_file(&credentials)?;
    let output_path = backup_path(&options)?;

    let mut command = Command::new(dump_client);
    command
        .arg(format!("--defaults-extra-file={}", defaults_path.display()))
        .arg("--result-file")
        .arg(&output_path)
        .arg("--hex-blob")
        .arg("--default-character-set=utf8mb4");

    if options.single_transaction {
        command.arg("--single-transaction").arg("--quick");
    }

    if options.schema_only {
        command.arg("--no-data");
    }

    if options.data_only {
        command.arg("--no-create-info");
    }

    if options.include_routines {
        command.arg("--routines");
    }

    if options.include_triggers {
        command.arg("--triggers");
    } else {
        command.arg("--skip-triggers");
    }

    if options.include_events {
        command.arg("--events");
    }

    if options.add_drop_statements {
        command.arg("--add-drop-database").arg("--add-drop-table");
    }

    if let Some(where_clause) = normalized_optional(&options.where_clause) {
        command.arg(format!("--where={where_clause}"));
    }

    if options.all_databases {
        command.arg("--all-databases");
    } else {
        let database = normalized_optional(&options.database)
            .or_else(|| normalized_optional(&credentials.database))
            .ok_or_else(|| "Choose a database or enable all-databases backup.".to_string())?;
        command.arg(database);

        for table in options
            .tables
            .iter()
            .filter_map(|table| normalized_str(table))
        {
            command.arg(table);
        }
    }

    let output = command
        .output()
        .map_err(|error| format!("Failed to run MariaDB backup: {error}"));

    let _ = fs::remove_file(&defaults_path);

    let output = output?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let _ = fs::remove_file(&output_path);
        return Err(if stderr.is_empty() {
            "MariaDB backup failed.".to_string()
        } else {
            stderr
        });
    }

    let metadata = fs::metadata(&output_path).map_err(|error| {
        format!("Backup finished but the output file could not be inspected: {error}")
    })?;

    Ok(MariaDBBackupResult {
        path: output_path.to_string_lossy().to_string(),
        size_bytes: metadata.len(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

fn validate_backup_options(options: &MariaDBBackupOptions) -> Result<(), String> {
    if options.output_dir.trim().is_empty() {
        return Err("Choose an output folder for the backup.".to_string());
    }

    if options.schema_only && options.data_only {
        return Err("Choose either schema-only or data-only, not both.".to_string());
    }

    if options.all_databases && !options.tables.is_empty() {
        return Err("Table selection is only available for a single database backup.".to_string());
    }

    if options.all_databases && normalized_optional(&options.where_clause).is_some() {
        return Err("WHERE filters are only available for table backups.".to_string());
    }

    if normalized_optional(&options.where_clause).is_some() && options.tables.len() != 1 {
        return Err("A WHERE filter requires exactly one selected table.".to_string());
    }

    Ok(())
}

fn backup_path(options: &MariaDBBackupOptions) -> Result<PathBuf, String> {
    let output_dir = PathBuf::from(options.output_dir.trim());
    if !output_dir.is_dir() {
        return Err("The backup output folder does not exist.".to_string());
    }

    let file_name = normalized_optional(&options.file_name).unwrap_or_else(|| {
        let scope = if options.all_databases {
            "all-databases".to_string()
        } else {
            normalized_optional(&options.database).unwrap_or_else(|| "database".to_string())
        };
        format!("mariadb-{scope}-{}.sql", timestamp_label())
    });

    let sanitized = file_name
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => character,
        })
        .collect::<String>();
    let file_name = if sanitized.to_ascii_lowercase().ends_with(".sql") {
        sanitized
    } else {
        format!("{sanitized}.sql")
    };

    Ok(Path::new(&output_dir).join(file_name))
}

fn find_dump_client() -> Option<String> {
    if let Some(install_path) = get_install_path() {
        for executable in ["mariadb-dump.exe", "mysqldump.exe"] {
            let path = PathBuf::from(&install_path).join("bin").join(executable);
            if path.exists() {
                return Some(path.to_string_lossy().to_string());
            }
        }
    }

    for command in [
        "mariadb-dump",
        "mariadb-dump.exe",
        "mysqldump",
        "mysqldump.exe",
    ] {
        if let Ok(output) = Command::new(command).arg("--version").output() {
            if output.status.success() {
                return Some(command.to_string());
            }
        }
    }

    find_mariadb_client().and_then(|client| {
        let client_path = PathBuf::from(client);
        let parent = client_path.parent()?;
        for executable in ["mariadb-dump.exe", "mysqldump.exe"] {
            let path = parent.join(executable);
            if path.exists() {
                return Some(path.to_string_lossy().to_string());
            }
        }
        None
    })
}

fn normalized_optional(value: &Option<String>) -> Option<String> {
    value.as_deref().and_then(normalized_str)
}

fn normalized_str(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn timestamp_label() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
