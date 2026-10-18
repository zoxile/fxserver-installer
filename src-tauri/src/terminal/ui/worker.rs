use super::{operations, redact_rcon_password, Job, Update};
use crate::{
    commands::{
        artifact,
        fxserver::{self, FxserverManager},
    },
    models::{
        artifact::ArtifactInstallRequest,
        fxserver::{ResourceScanRequest, ServerConfigRequest},
    },
    services::mariadb,
    terminal::{preflight, read_sql, settings},
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::runtime::Runtime;

pub(super) fn worker(
    manager: FxserverManager,
    runtime: Runtime,
    jobs: Receiver<Job>,
    updates: SyncSender<Update>,
) -> Result<(), String> {
    loop {
        match jobs.recv_timeout(Duration::from_millis(100)) {
            Ok(Job::Quit) | Err(RecvTimeoutError::Disconnected) => {
                manager.begin_shutdown();
                let result = manager.stop_running_process();
                let _ = updates.send(Update::Stopped(result.clone()));
                return result;
            }
            Ok(job) => {
                let event = handle_job(&manager, &runtime, job);
                if updates.send(event).is_err() {
                    manager.begin_shutdown();
                    return manager.stop_running_process();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn handle_job(manager: &FxserverManager, runtime: &Runtime, job: Job) -> Update {
    match job {
        Job::LoadCredentials(context) => {
            let database =
                runtime.block_on(operations::database::load_saved_login(&context.settings));
            let rcon = runtime.block_on(operations::server::load_rcon(&context));
            Update::Credentials(context, database, rcon)
        }
        Job::Profiles(settings) => {
            let result = runtime.block_on(fxserver::list_txdata_profiles(
                settings.tx_data_path,
                Some(settings.artifact_path),
                Some(true),
            ));
            Update::Profiles(result)
        }
        Job::LoadConfig(settings) => {
            let result = runtime
                .block_on(fxserver::read_server_config(ServerConfigRequest {
                    tx_data_path: settings.tx_data_path.clone(),
                    profile: settings.profile.clone(),
                }))
                .map(|result| result.files);
            Update::Config(settings, result)
        }
        Job::SaveConfig(settings, request, original) => {
            let result = runtime.block_on(fxserver::save_server_config(
                request,
                Some(settings.tx_data_path),
                Some(settings.profile),
                Some(original),
            ));
            Update::SavedConfig(result)
        }
        Job::SaveSettings(path, config) => {
            let result = settings::save(&path, &config)
                .map(|_| "Settings saved. Passwords are never written to terminal.json.".into());
            Update::Complete(result)
        }
        Job::Action(request) => {
            let (context, submission) = *request;
            let fields = operations::list(&context)
                .into_iter()
                .find(|action| action.id == submission.id)
                .map(|action| action.fields)
                .unwrap_or_default();
            let mut secrets = vec![
                context.credentials.password.clone(),
                context.rcon.password.clone(),
            ];
            secrets.extend(
                fields
                    .iter()
                    .filter(|field| matches!(field.kind, operations::FieldKind::Secret))
                    .map(|field| submission.get(field.key).to_owned()),
            );
            let redact = |text: String| {
                operations::redact(
                    &text,
                    &secrets.iter().map(String::as_str).collect::<Vec<_>>(),
                )
            };
            let result = runtime.block_on(operations::execute(&context, &submission));
            Update::Action(
                result
                    .map(|outcome| match outcome {
                        operations::Outcome::Text(text) => operations::Outcome::Text(redact(text)),
                        other => other,
                    })
                    .map_err(redact),
            )
        }
        Job::Start(settings, force) => {
            let result = runtime.block_on(async {
                let report = preflight(&settings, true).await?;
                if report.blocking && !force {
                    return Err(format!(
                        "Preflight blocked start: {} errors. Run Checks to review, or explicitly force start.",
                        report.error_count
                    ));
                }
                manager
                    .start(settings.launch()?)
                    .map(|result| format!("Started process {}", result.pid))
            });
            Update::Complete(result)
        }
        Job::Stop => {
            let result = manager
                .stop_running_process()
                .map(|_| "Server stopped.".into());
            Update::Complete(result)
        }
        Job::Restart(settings) => {
            let result = settings
                .launch()
                .and_then(|request| manager.restart(request))
                .map(|result| format!("Restarted process {}", result.pid));
            Update::Complete(result)
        }
        Job::Rcon(request) => {
            let password = request.rcon.password.clone();
            let result = runtime.block_on(fxserver::send_fxserver_rcon_command(request));
            Update::Rcon(redact_rcon_password(result, &password))
        }
        Job::Resources(settings) => {
            let result = runtime
                .block_on(fxserver::scan_fxserver_resources(ResourceScanRequest {
                    tx_data_path: settings.tx_data_path,
                    profile: settings.profile,
                }))
                .map(|result| result.resources);
            Update::Resources(result)
        }
        Job::Catalog => {
            let result = runtime
                .block_on(artifact::get_windows_artifact_catalog(true))
                .map(|result| result.builds);
            Update::Catalog(result)
        }
        Job::Install(build, destination) => {
            let result = artifact::install_legacy_for_manager(
                manager,
                ArtifactInstallRequest {
                    version: build.version,
                    url: build.download_url,
                    destination,
                    acknowledge_risk: true,
                },
            )
            .map(|result| format!("Installed {} in {}", result.version, result.destination));
            Update::Complete(result)
        }
        Job::Query(credentials, sql) => {
            let result = (|| {
                let _lease = mariadb::access::database_access()?;
                let result = mariadb::query::execute_query(credentials, sql)?;
                if result.success {
                    Ok(format!("{}\n{}", result.stdout, result.stderr))
                } else {
                    Err(result.stderr)
                }
            })();
            Update::Complete(result)
        }
        Job::Databases(credentials) => {
            let result = (|| {
                let _lease = mariadb::access::database_access()?;
                mariadb::query::list_databases(credentials)
            })();
            Update::Databases(result)
        }
        Job::Check(settings) => {
            let result = runtime.block_on(preflight(&settings, true)).map(|report| {
                report
                    .checks
                    .iter()
                    .map(|check| {
                        format!("{:?} | {}\n{}", check.severity, check.title, check.detail)
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n")
            });
            Update::Complete(result)
        }
        Job::ReadSql(path) => Update::SqlFile(read_sql(&path)),
        Job::Quit => unreachable!(),
    }
}

pub(super) fn monitor(
    manager: FxserverManager,
    updates: SyncSender<Update>,
    stopping: Arc<AtomicBool>,
) {
    let mut cursor = None;
    let mut last_status = Instant::now() - Duration::from_secs(2);
    while !stopping.load(Ordering::Relaxed) {
        let entries = manager
            .terminal(Some(500), cursor)
            .map(|r| r.entries)
            .unwrap_or_default();
        let status = if last_status.elapsed() >= Duration::from_secs(1) {
            last_status = Instant::now();
            manager.status().ok()
        } else {
            None
        };
        let next = entries.last().map(|e| e.id).or(cursor);
        if (status.is_some() || !entries.is_empty())
            && updates.try_send(Update::Sample(status, entries)).is_ok()
        {
            cursor = next;
        }
        thread::sleep(Duration::from_millis(100));
    }
}
