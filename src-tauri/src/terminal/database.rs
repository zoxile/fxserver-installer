use super::{password, print_json, safe_text, Connection, Settings};
use crate::{
    models::mariadb::{MariaDBBackupOptions, MariaDBCredentials, MariaDBInstallOptions},
    services::mariadb::{access, backup, detect, install, service},
};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(super) enum DatabaseAction {
    Status,
    /// Installed version and recommended update from the configured package source.
    Packages,
    /// Supported series, or versions available in one series.
    Versions {
        series: Option<String>,
    },
    /// Install MariaDB; Windows prompts for a root password, Linux preserves distro auth.
    Install {
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        password_stdin: bool,
    },
    Update {
        #[arg(long)]
        yes: bool,
    },
    /// Remove server software without intentionally deleting database files.
    Uninstall {
        #[arg(long)]
        yes: bool,
    },
    Start,
    Stop {
        #[arg(long)]
        yes: bool,
    },
    Restart {
        #[arg(long)]
        yes: bool,
    },
    /// Export one explicitly selected database. Never overwrites an existing dump.
    Backup {
        #[arg(long)]
        output: PathBuf,
        #[command(flatten)]
        connection: Connection,
    },
}

pub(super) fn execute(action: DatabaseAction, config: &Settings) -> Result<(), String> {
    let report = |message: &str| eprintln!("{}", safe_text(message));
    match action {
        DatabaseAction::Status => print_json(&detect::detect_mariadb()),
        DatabaseAction::Packages => print_json(&install::get_package_info()),
        DatabaseAction::Versions {
            series: Some(series),
        } => print_json(&install::list_releases(&series)?),
        DatabaseAction::Versions { series: None } => print_json(&install::list_series()?),
        DatabaseAction::Install {
            version,
            yes,
            password_stdin,
        } => {
            confirm(yes)?;
            let root_password = if cfg!(windows) {
                password("New MariaDB root password: ", password_stdin)?
            } else {
                if password_stdin {
                    return Err("Linux distro installation does not accept a root password. Configure a dedicated database login separately.".into());
                }
                String::new()
            };
            let options = MariaDBInstallOptions {
                version,
                root_password,
                service_name: "mariadb".into(),
                port: 3306,
                install_dir: None,
                data_dir: None,
                allow_remote_root_access: false,
                create_anonymous_user: false,
                skip_networking: false,
                optimize_for_transactions: false,
                use_utf8: false,
                page_size: None,
                buffer_pool_size: None,
                install_heidi_sql: false,
                install_development_files: false,
            };
            let _lease = access::maintenance_access()?;
            println!(
                "{}",
                safe_text(&install::install_mariadb(options, &report)?)
            );
            Ok(())
        }
        DatabaseAction::Update { yes } => {
            confirm(yes)?;
            let _lease = access::maintenance_access()?;
            println!("{}", safe_text(&install::update_mariadb(&report)?));
            Ok(())
        }
        DatabaseAction::Uninstall { yes } => {
            confirm(yes)?;
            let _lease = access::maintenance_access()?;
            println!("{}", safe_text(&install::uninstall_mariadb(&report)?));
            Ok(())
        }
        DatabaseAction::Start => {
            let _lease = access::maintenance_access()?;
            service::start_service(None)
        }
        DatabaseAction::Stop { yes } => {
            confirm(yes)?;
            let _lease = access::maintenance_access()?;
            service::stop_service(None)
        }
        DatabaseAction::Restart { yes } => {
            confirm(yes)?;
            let _lease = access::maintenance_access()?;
            service::restart_service(None)
        }
        DatabaseAction::Backup { output, connection } => {
            let database = connection
                .database
                .or_else(|| (!config.database.is_empty()).then(|| config.database.clone()))
                .ok_or("Choose --database for this backup.")?;
            let credentials = MariaDBCredentials {
                host: connection
                    .host
                    .unwrap_or_else(|| config.database_host.clone()),
                port: connection.port.unwrap_or(config.database_port),
                username: connection
                    .user
                    .unwrap_or_else(|| config.database_user.clone()),
                database: Some(database.clone()),
                password: password("Database password: ", connection.password_stdin)?,
            };
            let options = MariaDBBackupOptions {
                output_dir: output.to_string_lossy().into_owned(),
                file_name: None,
                database: Some(database),
                tables: Vec::new(),
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
            let _lease = access::database_access()?;
            print_json(&backup::create_backup(credentials, options)?)
        }
    }
}

fn confirm(yes: bool) -> Result<(), String> {
    if yes {
        Ok(())
    } else {
        Err("Back up databases and review this operation, then pass --yes. Package operations may restart the service; uninstall keeps data but is not a substitute for a backup.".into())
    }
}
