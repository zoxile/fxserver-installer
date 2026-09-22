use std::{
    ffi::OsString,
    io::{Read, Write},
    net::IpAddr,
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[path = "query_credentials.rs"]
mod credential_file;
pub(crate) use credential_file::CredentialFile;

const MAX_QUERY_BYTES: usize = 10 * 1024 * 1024;
const MAX_QUERY_OUTPUT: usize = 16 * 1024 * 1024;
const MAX_QUERY_ROWS: usize = 10_000;
const MAX_QUERY_CELLS: usize = 100_000;
pub(crate) const QUERY_TIMEOUT: Duration = Duration::from_secs(30);

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
    if query.len() > MAX_QUERY_BYTES {
        return Err("Query exceeds the 10 MiB input limit.".into());
    }

    let (mut command, _credentials_file) = configured_client(&credentials)?;
    configure_query_command(&mut command, credentials.database.as_deref())?;
    let mut result = run_query_client(&mut command, query, QUERY_TIMEOUT)?;
    if !credentials.password.is_empty() {
        result.stderr = result.stderr.replace(&credentials.password, "[redacted]");
    }
    Ok(result)
}

pub(crate) fn configured_client(
    credentials: &MariaDBCredentials,
) -> Result<(Command, CredentialFile), String> {
    #[cfg(all(test, windows))]
    if let Some(client) = crate::commands::database_browser::isolated_test_client(credentials) {
        return client;
    }
    let client = find_mariadb_client().ok_or_else(|| {
        "Could not find mariadb.exe. Install MariaDB or add its bin folder to PATH.".to_string()
    })?;
    let mut command = Command::new(client);
    command.no_window();
    let guard = apply_credentials_args(&mut command, credentials)?;
    Ok((command, guard))
}

pub(crate) fn configure_query_command(
    command: &mut Command,
    database: Option<&str>,
) -> Result<(), String> {
    command.args([
        "--batch",
        "--raw",
        "--quick",
        "--binary-mode",
        "--skip-reconnect",
        "--local-infile=0",
        "--connect-timeout=10",
        "--default-character-set=utf8mb4",
    ]);
    if let Some(database) = database.filter(|database| !database.trim().is_empty()) {
        validate_database_argument(database)?;
        command.arg(format!("--database={database}"));
    }
    Ok(())
}

fn bounded_output(mut stream: impl Read) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    stream
        .by_ref()
        .take((MAX_QUERY_OUTPUT + 1) as u64)
        .read_to_end(&mut output)
        .map_err(|_| "Cannot read MariaDB client output.")?;
    if output.len() > MAX_QUERY_OUTPUT {
        return Err("Query output exceeded 16 MiB. Narrow the query.".into());
    }
    Ok(output)
}

fn read_client_output(stream: impl Read, failed: mpsc::Sender<String>) -> Result<Vec<u8>, String> {
    let result = bounded_output(stream);
    if let Err(error) = &result {
        let _ = failed.send(error.clone());
    }
    result
}

fn run_query_client(
    command: &mut Command,
    query: String,
    timeout: Duration,
) -> Result<MariaDBQueryResult, String> {
    let output = run_client(command, query, timeout)?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let (columns, rows) = parse_tabular_output(&stdout)?;
    Ok(MariaDBQueryResult {
        success: output.status.success(),
        stdout,
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        columns,
        rows,
    })
}

pub(crate) struct ClientOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Executes exactly once, including on transport failure or uncertain commit status.
/// The deadline covers stdin and both output streams, not just process exit.
pub(crate) fn run_client(
    command: &mut Command,
    query: String,
    timeout: Duration,
) -> Result<ClientOutput, String> {
    let started = Instant::now();
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to execute MariaDB client: {error}"))?;
    // All three pipes were requested above; the threads own them until EOF or cancellation.
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let mut stdin = child.stdin.take().expect("piped stdin");
    let (failed, failures) = mpsc::channel();
    let stdout_failed = failed.clone();
    let output = thread::spawn(move || read_client_output(stdout, stdout_failed));
    let errors = thread::spawn(move || read_client_output(stderr, failed));
    let input = thread::spawn(move || stdin.write_all(query.as_bytes()));
    let status = loop {
        let failure = failures.try_recv().ok().or_else(|| {
            (started.elapsed() >= timeout).then(|| "Database query timed out.".to_string())
        });
        if let Some(error) = failure {
            let _ = crate::commands::fxserver::terminate_process_tree(child.id());
            let _ = child.kill();
            let _ = child.wait();
            break Err(error);
        }
        match child.try_wait() {
            Ok(Some(status))
                if output.is_finished() && errors.is_finished() && input.is_finished() =>
            {
                break Ok(status)
            }
            Ok(_) => thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                let _ = crate::commands::fxserver::terminate_process_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("Cannot wait for MariaDB client: {error}"));
            }
        }
    };
    // Do not wait indefinitely for pipes retained by an unexpected descendant after timeout.
    let status = status?;
    let stdout = output.join().map_err(|_| "Query output worker failed.")??;
    let stderr = errors.join().map_err(|_| "Query error worker failed.")??;
    let written = input.join().map_err(|_| "Query input worker failed.")?;
    if status.success() {
        written.map_err(|_| "Could not send the complete database query.")?;
    }
    Ok(ClientOutput {
        status,
        stdout,
        stderr,
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

pub(crate) fn apply_credentials_args(
    command: &mut Command,
    credentials: &MariaDBCredentials,
) -> Result<CredentialFile, String> {
    configure_credentials_args(command, credentials, "--defaults-extra-file=")
}

#[cfg(all(test, windows))]
pub(crate) fn isolated_credentials_args(
    command: &mut Command,
    credentials: &MariaDBCredentials,
) -> Result<CredentialFile, String> {
    if credentials.host != "127.0.0.1" {
        return Err("Isolated tests require literal IPv4 loopback.".into());
    }
    configure_credentials_args(command, credentials, "--defaults-file=")
}

fn configure_credentials_args(
    command: &mut Command,
    credentials: &MariaDBCredentials,
    option_file_argument: &str,
) -> Result<CredentialFile, String> {
    if command.get_args().next().is_some() {
        return Err("The credential option file must be the first MariaDB argument.".into());
    }
    if credentials.host.is_empty()
        || credentials.host.len() > 255
        || credentials.host.trim() != credentials.host
        || credentials.host.chars().any(char::is_control)
        || credentials.port == 0
    {
        return Err("Choose an explicit MariaDB host and nonzero port.".into());
    }
    let contents = format!(
        "[client]\nuser={}\npassword={}\n",
        option_value(&credentials.username)?,
        option_value(&credentials.password)?
    );
    let guard = CredentialFile::create(&contents)?;
    let mut option = OsString::from(option_file_argument);
    option.push(guard.path());
    command
        .arg(option)
        .env_remove("MYSQL_PWD")
        .arg("--protocol=tcp")
        .arg(format!("--host={}", credentials.host))
        .arg(format!("--port={}", credentials.port));
    if !allows_plaintext(&credentials.host) {
        command.args(["--ssl", "--ssl-verify-server-cert"]);
    }
    Ok(guard)
}

fn allows_plaintext(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn option_value(value: &str) -> Result<String, String> {
    if value.len() > 1024 {
        return Err("Client credential exceeds the option-file length limit.".into());
    }
    let mut quoted = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            '\u{8}' => quoted.push_str("\\b"),
            character if character.is_control() => {
                return Err("Client credential contains an unsupported control character.".into())
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

pub(crate) fn validate_database_argument(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 256
        || value.starts_with('-')
        || value.chars().any(char::is_control)
    {
        return Err("Invalid database or table command argument.".into());
    }
    Ok(())
}

/// Installer/service callers must clear discovery before and after replacement/removal.
pub(crate) fn clear_client_cache() {
    super::read_cache::reset();
}

pub(crate) fn find_mariadb_client() -> Option<String> {
    super::read_cache::find(find_mariadb_client_uncached)
        .map(|path| path.to_string_lossy().into_owned())
}

fn find_mariadb_client_uncached() -> Option<PathBuf> {
    if let Some(install_path) = get_install_path() {
        let client_path = PathBuf::from(install_path).join("bin").join("mariadb.exe");
        if client_path.is_file() {
            return client_path.canonicalize().ok();
        }
    }

    // Resolve PATH once. Never cache a bare command that Windows can later resolve
    // against a different working directory or PATH, and never search the CWD implicitly.
    let search_path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&search_path).filter(|path| path.is_absolute()) {
        let candidate = directory.join("mariadb.exe");
        if !candidate.is_file() {
            continue;
        }
        let Ok(candidate) = candidate.canonicalize() else {
            continue;
        };
        let mut command = Command::new(&candidate);
        command.no_window().arg("--version");
        if let Ok(output) = run_client(&mut command, String::new(), Duration::from_secs(2)) {
            if output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .to_lowercase()
                    .contains("mariadb")
            {
                return Some(candidate);
            }
        }
    }

    None
}

fn parse_tabular_output(stdout: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let mut lines = stdout.lines();
    let Some(header) = lines.next() else {
        return Ok((Vec::new(), Vec::new()));
    };

    if !header.contains('\t') && lines.clone().next().is_none() {
        return Ok((Vec::new(), Vec::new()));
    }

    let mut cells = 0;
    let mut parse_line = |line: &str| -> Result<Vec<String>, String> {
        line.split('\t')
            .map(|value| {
                cells += 1;
                if cells > MAX_QUERY_CELLS {
                    return Err("Query exceeded 100,000 result cells. Narrow the query.".into());
                }
                Ok(value.to_string())
            })
            .collect()
    };
    let columns = parse_line(header)?;
    let mut rows = Vec::new();
    for line in lines {
        if rows.len() >= MAX_QUERY_ROWS {
            return Err("Query returned more than 10,000 rows. Narrow the query.".into());
        }
        rows.push(parse_line(line)?);
    }
    Ok((columns, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(host: &str) -> MariaDBCredentials {
        MariaDBCredentials {
            host: host.into(),
            port: 3306,
            username: "fixture-user".into(),
            password: "fixture-password".into(),
            database: None,
        }
    }

    #[test]
    fn plaintext_is_limited_to_explicit_loopback_hosts() {
        for host in [
            "localhost",
            "LOCALHOST",
            "127.0.0.1",
            "127.12.34.56",
            "::1",
            "0:0:0:0:0:0:0:1",
        ] {
            assert!(allows_plaintext(host), "loopback {host}");
        }
        for host in [
            "",
            "localhost.",
            "localhost.example",
            "127.0.0.1.example",
            "127.1",
            "2130706433",
            "0.0.0.0",
            "::",
            "192.168.1.2",
            "db.example",
        ] {
            assert!(!allows_plaintext(host), "remote {host}");
        }
    }

    #[test]
    fn option_values_quote_and_escape_without_injecting_new_lines() {
        assert_eq!(
            option_value(" leading # ; = ' \" \\ \n\r\t\u{8} trailing ").unwrap(),
            "\" leading # ; = ' \\\" \\\\ \\n\\r\\t\\b trailing \""
        );
        assert_eq!(option_value("").unwrap(), "\"\"");
        assert!(option_value("nul\0value").is_err());
        assert!(option_value(&"x".repeat(1025)).is_err());
    }

    #[test]
    fn database_names_cannot_supply_client_options() {
        for database in ["", "--ssl=0", "-psecret", "db\nother"] {
            assert!(validate_database_argument(database).is_err());
        }
        let mut command = Command::new("inert");
        configure_query_command(&mut command, Some("my database")).unwrap();
        assert!(command
            .get_args()
            .any(|arg| arg == "--database=my database"));
        assert!(!command.get_args().any(|arg| arg == "-e"));
    }

    #[test]
    fn all_query_paths_disable_local_infile_and_reconnect_without_clearing_modes() {
        let mut command = Command::new("inert");
        configure_query_command(&mut command, None).unwrap();
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>();
        assert!(args.iter().any(|arg| arg == "--local-infile=0"));
        assert!(args.iter().any(|arg| arg == "--skip-reconnect"));
        assert!(!args.iter().any(|arg| arg.contains("sql_mode")));
        assert!(!args.iter().any(|arg| arg.contains("--ssl=0")));
        assert_eq!(QUERY_TIMEOUT, Duration::from_secs(30));
    }

    #[cfg(windows)]
    #[test]
    fn credential_arguments_preserve_defaults_and_force_remote_verification() {
        for host in ["127.0.0.1", "db.example"] {
            let mut command = Command::new("inert");
            command.env("MYSQL_PWD", "must-not-inherit");
            let guard = apply_credentials_args(&mut command, &fixture(host)).unwrap();
            let args: Vec<_> = command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            assert!(args[0].starts_with("--defaults-extra-file="));
            assert!(!args.iter().any(|arg| arg.contains("fixture-password")
                || arg == "--no-defaults"
                || arg == "--ssl=0"));
            assert_eq!(args.iter().any(|arg| arg == "--ssl"), host == "db.example");
            assert_eq!(
                args.iter().any(|arg| arg == "--ssl-verify-server-cert"),
                host == "db.example"
            );
            assert!(command
                .get_envs()
                .any(|(key, value)| key == "MYSQL_PWD" && value.is_none()));
            assert!(guard.path().exists());
            assert!(apply_credentials_args(&mut command, &fixture(host)).is_err());
        }
    }

    #[cfg(windows)]
    #[test]
    fn credential_file_is_removed_after_spawn_failure() {
        let path;
        {
            let mut command = Command::new("fxi-deliberately-missing-test-program.exe");
            let guard = apply_credentials_args(&mut command, &fixture("localhost")).unwrap();
            path = guard.path().to_owned();
            assert!(command.spawn().is_err());
            assert!(path.exists());
        }
        assert!(!path.exists());
    }

    #[cfg(windows)]
    #[test]
    fn legacy_query_streams_sql_to_inert_stdin_without_argv_exposure() {
        let sql = "SELECT 'fixture-private-sql';";
        let mut command = Command::new("powershell");
        command.no_window().args([
            "-NoProfile",
            "-Command",
            "[Console]::Out.Write([Console]::In.ReadToEnd())",
        ]);
        assert!(!format!("{command:?}").contains(sql));
        let result = run_query_client(&mut command, sql.into(), Duration::from_secs(10)).unwrap();
        assert!(result.success);
        assert_eq!(result.stdout, sql);
    }

    #[cfg(windows)]
    #[test]
    fn legacy_query_timeout_is_bounded() {
        let mut command = Command::new("powershell");
        command
            .no_window()
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"]);
        let started = Instant::now();
        let result = run_query_client(&mut command, "fixture".into(), Duration::from_millis(200));
        assert!(result.err().unwrap().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(windows)]
    #[test]
    fn output_overflow_terminates_a_client_that_keeps_running() {
        for stream in ["Out", "Error"] {
            let mut command = Command::new("powershell");
            command.no_window().args([
                "-NoProfile",
                "-Command",
                &format!(
                    "[Console]::{stream}.Write(('x' * {})); Start-Sleep -Seconds 30",
                    MAX_QUERY_OUTPUT + 1
                ),
            ]);
            let started = Instant::now();
            let result = run_client(&mut command, String::new(), Duration::from_secs(10));
            assert!(result.err().unwrap().contains("exceeded 16 MiB"));
            assert!(started.elapsed() < Duration::from_secs(8));
        }
    }

    #[cfg(windows)]
    #[test]
    fn blocked_input_is_covered_by_the_same_deadline() {
        let mut command = Command::new("powershell");
        command
            .no_window()
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"]);
        let started = Instant::now();
        let result = run_client(
            &mut command,
            "x".repeat(MAX_QUERY_BYTES),
            Duration::from_millis(200),
        );
        assert!(result.err().unwrap().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(windows)]
    #[test]
    fn submitted_sql_is_never_replayed_after_client_failure() {
        let counter = std::env::temp_dir().join(format!(
            "fxi-query-once-{}.txt",
            crate::commands::backup_manager::storage::unique_id()
        ));
        let mut command = Command::new("powershell");
        command.no_window().env("FXI_TEST_COUNTER", &counter).args([
            "-NoProfile", "-Command",
            "[Console]::In.ReadToEnd() | Out-Null; [IO.File]::AppendAllText($env:FXI_TEST_COUNTER, 'submitted'); [Console]::Error.Write('fixture transport failed'); exit 1",
        ]);
        let result = run_client(
            &mut command,
            "fixture SQL never sent to a database".into(),
            Duration::from_secs(10),
        )
        .unwrap();
        let submissions = std::fs::read_to_string(&counter).unwrap();
        std::fs::remove_file(counter).unwrap();
        assert!(!result.status.success());
        assert_eq!(submissions, "submitted");
        assert_eq!(result.stderr, b"fixture transport failed");
    }

    #[test]
    fn query_output_has_a_hard_limit() {
        assert!(bounded_output(std::io::repeat(b'x').take((MAX_QUERY_OUTPUT + 1) as u64)).is_err());
    }

    #[test]
    fn tabular_parsing_bounds_small_row_and_cell_allocations() {
        let (columns, rows) = parse_tabular_output("name\tvalue\nfixture\t1").unwrap();
        assert_eq!(columns, ["name", "value"]);
        assert_eq!(rows, [vec!["fixture", "1"]]);
        assert!(parse_tabular_output(&format!("name\n{}", "x\n".repeat(MAX_QUERY_ROWS))).is_ok());
        assert!(
            parse_tabular_output(&format!("name\n{}", "x\n".repeat(MAX_QUERY_ROWS + 1))).is_err()
        );
        assert!(parse_tabular_output(&"\t".repeat(MAX_QUERY_CELLS)).is_err());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Requires FXI_TEST_MARIADB_CLIENT; executes only --print-defaults with fixture credentials"]
    fn installed_mariadb_option_parser_reads_held_credential_file() {
        let client = std::env::var_os("FXI_TEST_MARIADB_CLIENT")
            .expect("set explicit installed client path");
        for (index, password) in [
            "",
            " leading and trailing ",
            "slashes\\ and \"quotes\" ' # ; =",
            "line\nnext\r\tend",
        ]
        .into_iter()
        .enumerate()
        {
            let contents = format!(
                "[client]\nuser=\"fixture-parser-user\"\npassword={}\nport=3317\n",
                option_value(password).unwrap()
            );
            let guard = CredentialFile::create(&contents).unwrap();
            let path = guard.path().to_owned();
            let mut option = OsString::from("--defaults-extra-file=");
            option.push(&path);
            let mut command = Command::new(&client);
            command
                .no_window()
                .env_remove("MYSQL_PWD")
                .arg(option)
                .arg("--print-defaults");
            let output =
                run_query_client(&mut command, String::new(), Duration::from_secs(10)).unwrap();
            assert!(
                output.success,
                "installed parser could not read protected option file"
            );
            assert!(
                output.stdout.contains("--user=fixture-parser-user"),
                "fixture file was not loaded"
            );
            assert!(
                output
                    .stdout
                    .replace("\r\n", "\n")
                    .contains(&format!("--password={password} --port=3317")),
                "credential fixture {index} did not round-trip through installed option parser"
            );
            assert!(path.exists(), "guard must outlive child exit");
            drop(guard);
            assert!(!path.exists(), "guard must remove fixture credentials");
        }
    }
}
