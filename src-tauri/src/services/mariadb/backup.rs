use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(any(not(target_os = "linux"), test))]
use std::fs::OpenOptions;

#[cfg(not(target_os = "linux"))]
use super::{detect::get_install_path, query::find_mariadb_client};
use crate::{
    models::mariadb::{MariaDBBackupOptions, MariaDBBackupResult, MariaDBCredentials},
    process::CommandNoWindowExt,
    services::mariadb::query::{apply_credentials_args, validate_database_argument},
};

pub fn create_backup(
    credentials: MariaDBCredentials,
    options: MariaDBBackupOptions,
) -> Result<MariaDBBackupResult, String> {
    validate_backup_options(&options)?;

    let dump_client = find_dump_client().ok_or_else(|| {
        #[cfg(target_os = "linux")]
        let message = "Could not find a MariaDB mariadb-dump/mysqldump client. Install the distro MariaDB clients package.";
        #[cfg(not(target_os = "linux"))]
        let message = "Could not find mariadb-dump.exe or mysqldump.exe. Install MariaDB or add its bin folder to PATH.";
        message.to_string()
    })?;
    let output_path = backup_path(&options)?;

    let mut command = Command::new(dump_client);
    command.no_window();
    let _credentials_file = apply_credentials_args(&mut command, &credentials)?;
    #[cfg(not(target_os = "linux"))]
    command.arg("--result-file").arg(&output_path);
    configure_dump_args(&mut command, &credentials, &options)?;

    #[cfg(target_os = "linux")]
    let (stderr, metadata) = {
        let handles = super::linux::filesystem::pin_directories(
            output_path.parent().ok_or("Missing backup parent.")?,
        )?;
        let file = super::linux::filesystem::create_private_at(
            handles.last().unwrap(),
            output_path.file_name().ok_or("Missing backup filename.")?,
        )?;
        let stderr = run_client_with_output(
            &mut command,
            "MariaDB backup",
            Duration::from_secs(3600),
            Some(file.try_clone().map_err(|e| e.to_string())?),
        )
        .map_err(|error| {
            format!(
                "{} Partial output retained at {}.",
                super::backup_storage::client_failure(&error),
                output_path.display()
            )
        })?;
        file.sync_all().map_err(|e| e.to_string())?;
        (stderr, file.metadata().map_err(|e| e.to_string())?)
    };
    #[cfg(not(target_os = "linux"))]
    let (stderr, metadata) = {
        reserve_backup_file(&output_path)?;
        let stderr = match run_backup_client(&mut command, "MariaDB backup") {
            Ok(stderr) => stderr,
            Err(error) => {
                let _ = fs::remove_file(&output_path);
                return Err(error);
            }
        };

        let metadata = fs::metadata(&output_path).map_err(|error| {
            format!("Backup finished but the output file could not be inspected: {error}")
        })?;
        (stderr, metadata)
    };

    Ok(MariaDBBackupResult {
        path: output_path.to_string_lossy().to_string(),
        size_bytes: metadata.len(),
        stderr,
    })
}

fn configure_dump_args(
    command: &mut Command,
    credentials: &MariaDBCredentials,
    options: &MariaDBBackupOptions,
) -> Result<(), String> {
    // mariadb-dump does not accept the query client's --connect-timeout option.
    command
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
        validate_database_argument(&database)?;
        command.arg(database);

        for table in options
            .tables
            .iter()
            .filter_map(|table| normalized_str(table))
        {
            validate_database_argument(&table)?;
            command.arg(table);
        }
    }

    Ok(())
}

#[cfg(any(not(target_os = "linux"), test))]
fn reserve_backup_file(path: &Path) -> Result<(), String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|_| ())
        .map_err(|error| {
            format!("Cannot create backup without overwriting an existing file: {error}")
        })
}

pub(crate) fn run_backup_client(command: &mut Command, operation: &str) -> Result<String, String> {
    run_client_with_timeout(command, operation, Duration::from_secs(3600))
}

fn run_client_with_timeout(
    command: &mut Command,
    operation: &str,
    timeout: Duration,
) -> Result<String, String> {
    run_client_with_output(command, operation, timeout, None)
}

fn run_client_with_output(
    command: &mut Command,
    operation: &str,
    timeout: Duration,
    output: Option<fs::File>,
) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .stdout(output.map(Stdio::from).unwrap_or_else(Stdio::null))
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start {operation}: {error}"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or("MariaDB stderr was unavailable.")?;
    let reader = thread::spawn(move || {
        let mut retained = Vec::new();
        let mut buffer = [0; 8192];
        while let Ok(count) = stderr.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let keep = count.min(16384usize.saturating_sub(retained.len()));
            retained.extend_from_slice(&buffer[..keep]);
        }
        String::from_utf8_lossy(&retained).trim().to_string()
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) if reader.is_finished() => break Ok(status),
            Ok(_) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(100));
            }
            outcome => {
                #[cfg(target_os = "linux")]
                super::linux::runtime::terminate_group(&mut child);
                let _ = child.kill();
                let _ = child.wait();
                break Err(match outcome {
                    Err(error) => format!("Failed to wait for {operation}: {error}"),
                    _ => format!("{operation} exceeded its one-hour timeout."),
                });
            }
        }
    };
    let status = status?;
    let stderr = reader.join().unwrap_or_default();
    if status.success() {
        Ok(stderr)
    } else if stderr.is_empty() {
        Err(format!("{operation} failed."))
    } else {
        Err(stderr)
    }
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

#[cfg(target_os = "linux")]
fn find_dump_client() -> Option<String> {
    super::detect::find_binary(&["mariadb-dump", "mysqldump"])
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(not(target_os = "linux"))]
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
        if let Ok(output) = Command::new(command).no_window().arg("--version").output() {
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

#[cfg(test)]
mod safety_tests {
    use super::*;

    #[test]
    fn dump_arguments_exclude_query_only_options_and_keep_requested_scope() {
        let credentials = MariaDBCredentials {
            host: "127.0.0.1".into(),
            port: 3306,
            username: "fixture".into(),
            password: "not-an-argument".into(),
            database: Some("fixture_db".into()),
        };
        let mut options = MariaDBBackupOptions {
            output_dir: String::new(),
            file_name: None,
            database: None,
            tables: vec![],
            all_databases: false,
            schema_only: false,
            data_only: false,
            include_routines: true,
            include_triggers: true,
            include_events: true,
            single_transaction: true,
            add_drop_statements: false,
            where_clause: None,
        };
        let mut command = Command::new("unused");
        configure_dump_args(&mut command, &credentials, &options).unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy())
            .collect();
        assert_eq!(
            args,
            [
                "--hex-blob",
                "--default-character-set=utf8mb4",
                "--single-transaction",
                "--quick",
                "--routines",
                "--triggers",
                "--events",
                "fixture_db",
            ]
        );
        options.include_triggers = false;
        options.tables = vec!["items".into()];
        let mut command = Command::new("unused");
        configure_dump_args(&mut command, &credentials, &options).unwrap();
        assert!(command.get_args().any(|arg| arg == "--skip-triggers"));
        assert!(command.get_args().any(|arg| arg == "items"));
    }

    #[test]
    fn existing_backup_is_never_overwritten() {
        let path = std::env::temp_dir().join(format!(
            "fx-dump-reservation-{}-{}.sql",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, b"existing backup").unwrap();
        assert!(reserve_backup_file(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"existing backup");
        fs::remove_file(path).unwrap();
    }

    #[cfg(windows)]
    fn mock_client(script: &str) -> Command {
        let mut command = Command::new("powershell.exe");
        command
            .no_window()
            .args(["-NoProfile", "-NonInteractive", "-Command", script]);
        command
    }

    #[cfg(windows)]
    #[test]
    fn client_errors_and_large_stderr_are_bounded() {
        let mut command = mock_client("[Console]::Error.Write(('x' * 100000)); exit 1");
        let error = run_client_with_timeout(&mut command, "mock backup", Duration::from_secs(15))
            .unwrap_err();
        assert_eq!(error.len(), 16384);
        let mut command = mock_client("[Console]::Error.Write('warning'); exit 0");
        assert_eq!(
            run_client_with_timeout(&mut command, "mock backup", Duration::from_secs(15)).unwrap(),
            "warning"
        );
    }

    #[cfg(windows)]
    #[test]
    fn stuck_client_is_terminated_and_reaped() {
        let mut command = mock_client("Start-Sleep -Seconds 30");
        let started = Instant::now();
        assert!(
            run_client_with_timeout(&mut command, "mock backup", Duration::from_millis(200))
                .unwrap_err()
                .contains("timeout")
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[cfg(windows)]
    #[test]
    fn restore_input_streams_from_a_file_without_loading_it_into_memory() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!(
            "fx-restore-stream-{}-{}.sql",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        for _ in 0..128 {
            file.write_all(&[b'x'; 65536]).unwrap();
        }
        drop(file);
        let mut command = mock_client("$stream = [Console]::OpenStandardInput(); $buffer = New-Object byte[] 65536; $total = 0; while (($count = $stream.Read($buffer, 0, $buffer.Length)) -gt 0) { $total += $count }; [Console]::Error.Write($total)");
        command.stdin(Stdio::from(fs::File::open(&path).unwrap()));
        let result = run_client_with_timeout(&mut command, "mock restore", Duration::from_secs(20));
        fs::remove_file(&path).unwrap();
        assert_eq!(result.unwrap(), "8388608");
    }
}
