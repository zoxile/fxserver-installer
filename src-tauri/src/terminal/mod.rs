mod database;
mod settings;
mod ui;

use crate::{
    commands::{
        artifact, diagnostics,
        fxserver::{self, FxserverManager},
    },
    models::{
        artifact::ArtifactInstallRequest,
        fxserver::{FxserverCommandRequest, FxserverRconConfig, ResourceScanRequest},
        mariadb::MariaDBCredentials,
    },
    services::mariadb,
};
use clap::{Args, Parser, Subcommand};
use settings::Settings;
use std::{
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
    time::Duration,
};

#[derive(Parser)]
#[command(
    name = "fxserver-manager",
    version,
    about = "FXServer desktop services, from your terminal"
)]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[arg(long, global = true)]
    artifact_path: Option<String>,
    #[arg(long, global = true)]
    tx_data_path: Option<String>,
    #[arg(long, global = true)]
    profile: Option<String>,
    #[command(subcommand)]
    command: Option<Action>,
}

#[derive(Subcommand)]
enum Action {
    /// Open the interactive terminal manager (the default).
    Tui,
    /// Write a settings template. Never includes passwords or overwrites a file.
    Init,
    /// Check this profile without starting or modifying the server.
    Doctor,
    /// Discover txAdmin profiles.
    Profiles,
    /// List resources from the chosen profile.
    Resources,
    /// Run a server in the foreground until Ctrl+C, then stop its process tree.
    Run {
        /// Start despite preflight errors after reviewing them.
        #[arg(long)]
        force: bool,
    },
    /// List the host platform's available Legacy artifacts.
    Artifacts {
        #[arg(long)]
        refresh: bool,
    },
    /// Install an exact version from the official catalog.
    InstallArtifact {
        version: String,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        acknowledge_risk: bool,
    },
    /// Send one RCON command; the password is prompted, never a CLI argument.
    Rcon {
        command: String,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        password_stdin: bool,
    },
    /// Inspect MariaDB installation and service status without changing it.
    DatabaseStatus,
    /// MariaDB packages, service control, and database backups.
    Database {
        #[command(subcommand)]
        command: database::DatabaseAction,
    },
    /// Execute SQL using a hidden password prompt. Review mutations carefully.
    Query {
        #[arg(conflicts_with = "file", required_unless_present = "file")]
        sql: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[command(flatten)]
        connection: Connection,
        /// Required for execution; without this flag the SQL is only displayed.
        #[arg(long)]
        execute: bool,
    },
}

#[derive(Args)]
struct Connection {
    #[arg(long)]
    host: Option<String>,
    #[arg(long)]
    port: Option<u16>,
    #[arg(long)]
    user: Option<String>,
    #[arg(long)]
    database: Option<String>,
    #[arg(long)]
    password_stdin: bool,
}

pub fn run() -> Result<(), String> {
    let cli = Cli::parse();
    let path = match cli.config {
        Some(path) => path,
        None => settings::default_path()?,
    };
    if matches!(cli.command, Some(Action::Init)) {
        settings::write_example(&path)?;
        println!("Created {}", path.display());
        return Ok(());
    }
    let mut config = settings::load(&path)?;
    if let Some(value) = cli.artifact_path {
        config.artifact_path = value;
    }
    if let Some(value) = cli.tx_data_path {
        config.tx_data_path = value;
    }
    if let Some(value) = cli.profile {
        config.profile = value;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    match cli.command.unwrap_or(Action::Tui) {
        Action::Tui => ui::run(config, path, runtime),
        Action::Init => unreachable!(),
        Action::Doctor => {
            let report = runtime.block_on(preflight(&config, true))?;
            print_json(&report)?;
            if report.blocking {
                Err("Preflight found blocking errors. No server was started.".into())
            } else {
                Ok(())
            }
        }
        Action::Profiles => print_json(&runtime.block_on(fxserver::list_txdata_profiles(
            config.tx_data_path,
            Some(config.artifact_path),
            Some(true),
        ))?),
        Action::Resources => print_json(&runtime.block_on(fxserver::scan_fxserver_resources(
            ResourceScanRequest {
                tx_data_path: config.tx_data_path,
                profile: config.profile,
            },
        ))?),
        Action::Artifacts { refresh } => {
            print_json(&runtime.block_on(artifact::get_windows_artifact_catalog(refresh))?)
        }
        Action::DatabaseStatus => print_json(&mariadb::detect::detect_mariadb()),
        Action::Database { command } => database::execute(command, &config),
        Action::Run { force } => runtime.block_on(run_server(config, force)),
        Action::InstallArtifact {
            version,
            yes,
            acknowledge_risk,
        } => {
            if !yes {
                return Err("Review the version and destination, back up the server, then pass --yes to install.".into());
            }
            if config.artifact_path.is_empty() {
                return Err("Choose --artifact-path before installing.".into());
            }
            let catalog = runtime.block_on(artifact::get_windows_artifact_catalog(true))?;
            let build = catalog
                .builds
                .into_iter()
                .find(|b| b.version == version)
                .ok_or("That version is not in the official host-platform catalog.")?;
            let result = artifact::install_legacy_for_manager(
                &FxserverManager::default(),
                ArtifactInstallRequest {
                    version,
                    url: build.download_url,
                    destination: config.artifact_path,
                    acknowledge_risk,
                },
            )?;
            print_json(&result)
        }
        Action::Rcon {
            command,
            host,
            port,
            password_stdin,
        } => {
            let rcon = FxserverRconConfig {
                host: host.unwrap_or(config.rcon_host),
                port: port.unwrap_or(config.rcon_port),
                password: password("RCON password: ", password_stdin)?,
            };
            let response = runtime.block_on(fxserver::send_fxserver_rcon_command(
                FxserverCommandRequest { command, rcon },
            ))?;
            println!("{}", safe_text(&response));
            Ok(())
        }
        Action::Query {
            sql,
            file,
            connection,
            execute,
        } => {
            let sql = match file {
                Some(path) => read_sql(&path)?,
                None => sql.unwrap_or_default(),
            };
            if !execute {
                println!("{}", safe_text(&sql));
                println!("Preview only. Pass --execute to run this SQL.");
                return Ok(());
            }
            let credentials = MariaDBCredentials {
                host: connection.host.unwrap_or(config.database_host),
                port: connection.port.unwrap_or(config.database_port),
                username: connection.user.unwrap_or(config.database_user),
                database: connection
                    .database
                    .or_else(|| (!config.database.is_empty()).then_some(config.database)),
                password: password("Database password: ", connection.password_stdin)?,
            };
            let _lease = mariadb::access::database_access()?;
            let result = mariadb::query::execute_query(credentials, sql)?;
            print_json(&result)?;
            if result.success {
                Ok(())
            } else {
                Err("The SQL client reported an error; see the result above.".into())
            }
        }
    }
}

fn password(prompt: &str, from_stdin: bool) -> Result<String, String> {
    if from_stdin {
        let mut bytes = Vec::new();
        io::stdin()
            .take(8193)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 8192 {
            return Err("Password input exceeds 8 KiB.".into());
        }
        let value = String::from_utf8(bytes).map_err(|_| "Password input must be UTF-8.")?;
        let value = value.trim_end_matches(['\r', '\n']);
        if value.contains(['\r', '\n', '\0']) {
            return Err("Password input must contain one line.".into());
        }
        return Ok(value.into());
    }
    if !io::stdin().is_terminal() {
        return Err("A hidden password prompt needs a terminal. Use --password-stdin with a trusted secret source instead.".into());
    }
    rpassword::prompt_password(prompt).map_err(|e| e.to_string())
}

pub(super) fn read_sql(path: &std::path::Path) -> Result<String, String> {
    let file = open_regular_file(path).map_err(|e| e.to_string())?;
    let mut sql = String::new();
    file.take(10 * 1024 * 1024 + 1)
        .read_to_string(&mut sql)
        .map_err(|e| e.to_string())?;
    if sql.len() > 10 * 1024 * 1024 {
        return Err("SQL file exceeds 10 MiB.".into());
    }
    Ok(sql)
}

fn open_regular_file(path: &std::path::Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Choose a regular file.",
        ));
    }
    Ok(file)
}

async fn preflight(
    config: &Settings,
    check_ports: bool,
) -> Result<diagnostics::PreflightReport, String> {
    config.validate_server()?;
    diagnostics::run_fxserver_preflight(diagnostics::PreflightRequest {
        artifact_path: config.artifact_path.clone(),
        tx_data_path: config.tx_data_path.clone(),
        profile: config.profile.clone(),
        credentials: None,
        check_ports,
    })
    .await
}

async fn run_server(config: Settings, force: bool) -> Result<(), String> {
    let report = preflight(&config, true).await?;
    if report.blocking && !force {
        print_json(&report)?;
        return Err(
            "Start blocked by preflight. Review the report before considering --force.".into(),
        );
    }
    let manager = FxserverManager::default();
    let worker = manager.clone();
    let request = config.launch()?;
    tokio::task::spawn_blocking(move || worker.start(request))
        .await
        .map_err(|e| e.to_string())??;
    eprintln!("Server started. Ctrl+C stops this managed server and its children.");
    let mut cursor = None;
    let mut timer = tokio::time::interval(Duration::from_millis(100));
    let outcome = async {
        loop {
            tokio::select! {
                signal = tokio::signal::ctrl_c() => { break signal.map_err(|e| e.to_string()); },
                _ = timer.tick() => {
                    let snapshot = manager.terminal(Some(1000), cursor)?;
                    for entry in snapshot.entries {
                        cursor = Some(entry.id);
                        writeln!(io::stdout().lock(), "{}", safe_text(&entry.plain_line))
                            .map_err(|error| error.to_string())?;
                    }
                    if !manager.status()?.running { break Ok(()); }
                }
            }
        }
    }
    .await;
    manager.begin_shutdown();
    let stopped = tokio::task::spawn_blocking(move || manager.stop_running_process())
        .await
        .map_err(|e| e.to_string())?;
    outcome.and(stopped)
}

fn print_json(value: &impl serde::Serialize) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    Ok(())
}

/// Do not let server output or database cells inject terminal control sequences.
pub fn safe_text(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    #[test]
    fn command_schema_is_consistent_and_passwords_are_not_arguments() {
        Cli::command().debug_assert();
        assert!(Cli::try_parse_from([
            "fxserver-manager",
            "rcon",
            "status",
            "--password",
            "secret"
        ])
        .is_err());
        assert!(
            Cli::try_parse_from(["fxserver-manager", "query", "--file", "a.sql", "select 1"])
                .is_err()
        );
    }
    #[test]
    fn strips_terminal_control_codes() {
        assert_eq!(safe_text("a\u{1b}]52;c;secret\u{7}b\n"), "a]52;c;secretb\n");
    }

    #[cfg(unix)]
    #[test]
    fn file_input_rejects_pipes_without_waiting_for_a_writer() {
        use std::os::unix::ffi::OsStrExt;
        let path = std::env::temp_dir().join(format!(
            "fxi-input-{}",
            crate::commands::backup_manager::storage::unique_id()
        ));
        let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        assert!(open_regular_file(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
