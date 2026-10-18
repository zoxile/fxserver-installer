use super::{json, redact, Action, Context, Field, Outcome, Submission};
use crate::{
    commands::{
        self, artifact, diagnostics, fxserver,
        live_bridge::{self, BridgeTarget},
        managed_rcon,
        profiler_capture::{
            self, ProfilerCaptureManager, ProfilerCaptureRequest, ProfilerCaptureResult,
        },
        resource_updates::{self, ApplyRequest, PreviewRequest, ResourceTarget},
    },
    models::{
        artifact::{ArtifactInstallRequest, EnhancedInstallRequest},
        fxserver::{FxserverCommandRequest, FxserverRconConfig, ResourceScanRequest},
    },
    platform,
};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::Path,
    sync::{Mutex, OnceLock},
};

static CAPTURE_RESULT: Mutex<Option<ProfilerCaptureResult>> = Mutex::new(None);

fn action(
    id: &'static str,
    group: &'static str,
    title: &'static str,
    description: &'static str,
    fields: Vec<Field>,
    confirmation: Option<&'static str>,
) -> Action {
    Action {
        id,
        group,
        title,
        description,
        fields,
        confirmation,
    }
}

fn profile_fields(context: &Context) -> Vec<Field> {
    vec![
        Field::text("tx_data", "txData folder", &context.settings.tx_data_path),
        Field::text("profile", "Profile", &context.settings.profile),
    ]
}

fn resource_fields(context: &Context) -> Vec<Field> {
    let mut fields = profile_fields(context);
    fields.extend([
        Field::text("workspace", "Workspace ID", "terminal"),
        Field::text("resource", "Resource folder", ""),
    ]);
    fields
}

fn bridge_fields(context: &Context) -> Vec<Field> {
    let mut fields = profile_fields(context);
    fields.extend([
        Field::text("workspace", "Workspace ID", "terminal"),
        Field::text(
            "port",
            "Local FXServer HTTP port",
            context.rcon.port.to_string(),
        ),
    ]);
    fields
}

fn rcon_fields(context: &Context) -> Vec<Field> {
    vec![
        Field::text("host", "RCON host", &context.rcon.host),
        Field::text("port", "RCON port", context.rcon.port.to_string()),
        Field::text("password", "RCON password (blank uses session)", "")
            .optional()
            .secret(),
    ]
}

fn with(mut fields: Vec<Field>, extra: impl IntoIterator<Item = Field>) -> Vec<Field> {
    fields.extend(extra);
    fields
}

pub(super) fn list(context: &Context) -> Vec<Action> {
    let preview_id = || Field::text("preview", "Preview ID", "");
    let snapshot_id = || Field::text("snapshot", "Snapshot ID", "");
    let destination = || {
        Field::text(
            "destination",
            "Artifact folder",
            &context.settings.artifact_path,
        )
    };
    let branch = || Field::text("branch", "GitHub branch/ref", "HEAD");
    vec![
        action("server.rcon-save", "Server", "Remember RCON password", "Encrypt a password for the current txData/profile and session RCON host/port.",
            vec![Field::text("password", "RCON password (blank uses session)", "").optional().secret()],
            Some("Remember this RCON password for the current profile and endpoint, replacing any saved password for that scope?")),
        action("server.rcon-load", "Server", "Load remembered RCON password", "Load the encrypted password for the current profile and endpoint into this session.", vec![], None),
        action("server.rcon-forget", "Server", "Forget RCON password", "Delete the saved password for this profile and endpoint and clear the session password.", vec![],
            Some("Forget the saved RCON password for this profile and endpoint and clear it from this session?")),
        action("server.status", "Server", "Server status", "Inspect the managed process.", vec![], None),
        action("server.profiles", "Server", "Discover profiles", "List txAdmin profiles in the selected txData folder.",
            vec![Field::text("tx_data", "txData folder", &context.settings.tx_data_path)], None),
        action("server.preflight", "Server", "Preflight report", "Run shared diagnostics without starting FXServer.",
            with(profile_fields(context), [destination(),
                Field::text("ports", "Check ports", "yes").choice(&["yes", "no"]),
                Field::text("database", "Check session database credentials", "no").choice(&["no", "yes"])]), None),
        action("server.dependencies", "Server", "Resource dependencies", "Inspect dependency and startup order diagnostics.",
            with(profile_fields(context), [destination()]), None),
        action("server.rcon", "Server", "Send RCON command", "Use the shared command validator and transport.",
            with(rcon_fields(context), [Field::text("command", "Command", "status")]),
            Some("Send this RCON command to the selected endpoint? Commands may change live server state.")),
        action("server.verify-rcon", "Server", "Verify managed RCON", "Match a local endpoint to managed-process stdout using a nonce.",
            rcon_fields(context), Some("Send a verification echo to the local RCON endpoint?")),
        action("resource.inspect", "Resources", "Inspect resources", "Read local resource manifests and versions.", profile_fields(context), None),
        action("resource.check-version", "Resources", "Check resource version", "Compare the local version with a bounded GitHub manifest download.",
            with(resource_fields(context), [branch()]), None),
        action("resource.preview", "Resources", "Preview resource update", "Stage the archive and inspect file changes and default protections.",
            with(resource_fields(context), [branch()]), None),
        action("resource.apply", "Resources", "Apply resource update", "Retain default protections and create a snapshot. Extra paths use semicolons; literal semicolons in filenames are not supported.",
            with(resource_fields(context), [preview_id(), Field::text("protected", "Additional protected paths (semicolon-separated)", "").optional()]),
            Some("Apply this preview to the stopped server? Default protected and local-only files remain preserved; a rollback snapshot is created.")),
        action("resource.discard", "Resources", "Discard update preview", "Remove a staged download without touching the resource.", vec![preview_id()],
            Some("Discard this staged preview?")),
        action("resource.snapshots", "Resources", "List resource snapshots", "Inspect retained rollback snapshots for this resource and workspace ID.", resource_fields(context), None),
        action("resource.rollback-preview", "Resources", "Preview resource rollback", "Inspect the exact file changes from a verified snapshot.",
            with(resource_fields(context), [snapshot_id()]), None),
        action("resource.rollback", "Resources", "Apply resource rollback", "Restore a fresh rollback preview after rechecking current files.",
            with(resource_fields(context), [preview_id()]),
            Some("Restore this rollback preview on the stopped server? All resource files, including configuration, are restored. A safety snapshot is created first.")),
        action("resource.delete-snapshot", "Resources", "Delete resource snapshot", "Delete one verified snapshot belonging to this resource.",
            with(resource_fields(context), [snapshot_id()]), Some("Permanently delete this rollback snapshot?")),
        action("artifact.installed", "Artifacts", "Inspect installed artifact", "Read the installed artifact version and edition.", vec![destination()], None),
        action("artifact.legacy-list", "Artifacts", "List Legacy artifacts", "Load the official host-platform catalog and build health.",
            vec![Field::text("refresh", "Refresh catalog", "no").choice(&["no", "yes"])], None),
        action("artifact.legacy-install", "Artifacts", "Install Legacy artifact", "Install an exact catalog version under the stopped-server guard.",
            vec![destination(), Field::text("version", "Exact version", ""), Field::text("risk", "Acknowledge unhealthy/unknown build risk", "no").choice(&["no", "yes"])],
            Some("Replace the artifact files with this catalog version? FXServer must be stopped.")),
        action("artifact.enhanced-list", "Artifacts", "List Enhanced artifacts", "Load the published Enhanced host-platform catalog. Enhanced is early access and may be unstable.", vec![], None),
        action("artifact.enhanced-install", "Artifacts", "Install Enhanced artifact", "Enhanced is early access and may be unstable. Install an exact published version with manager and filesystem guards.",
            vec![destination(), Field::text("version", "Exact version", "")],
            Some("Enhanced is early access and may be unstable. Replace the artifact files with this Enhanced version? FXServer must be stopped.")),
        action("server.bridge-installation", "Live Bridge", "Inspect bridge installation", "Inspect managed bridge files and pairing availability without revealing keys.", bridge_fields(context), None),
        action("server.bridge-preview", "Live Bridge", "Preview bridge install/remove", "Review bridge files and the managed server.cfg block before changing anything.",
            with(bridge_fields(context), [Field::text("operation", "Operation", "install").choice(&["install", "remove"])]), None),
        action("server.bridge-apply", "Live Bridge", "Apply bridge preview", "Apply a reviewed bridge change with config history and rollback protection.", with(bridge_fields(context), [preview_id()]),
            Some("Apply this bridge installation/removal preview to the stopped server? Managed bridge files and server.cfg will change.")),
        action("server.bridge-snapshot", "Live Bridge", "Live resources and players", "Fetch the paired local bridge snapshot, including its instance ID.", bridge_fields(context), None),
        action("resource.bridge-action", "Live Bridge", "Send bridge resource action", "Dispatch a validated action only to the reviewed bridge instance.",
            with(bridge_fields(context), [Field::text("instance", "Instance ID from snapshot", ""),
                Field::text("resource", "Resource name", ""), Field::text("operation", "Action", "ensure").choice(&["ensure", "start", "stop", "restart"])]),
            Some("Send this resource action to the live bridge? Players may be affected.")),
        action("server.profiler-capture", "Profiler", "Capture server profile", "Run a frame-bounded, managed-endpoint-verified capture. Retain its JSON for export until explicitly discarded.",
            with(rcon_fields(context), [Field::text("frames", "Frames", "100").choice(&["100", "250", "500"])]),
            Some("Record this frame-bounded profile on the already-running managed server? No server will be started.")),
        action("server.profiler-export", "Profiler", "Export captured profile", "Write the retained capture JSON to a new file without overwriting anything.",
            vec![Field::text("capture", "Capture ID", ""), Field::text("path", "Output JSON path", "")], None),
        action("server.profiler-discard", "Profiler", "Discard captured profile", "Release the retained JSON so a new capture can be made.",
            vec![Field::text("capture", "Capture ID", "")], Some("Discard this retained capture? Export it first if it is still needed.")),
    ]
}

fn resource_target(submission: &Submission) -> Result<ResourceTarget, String> {
    Ok(ResourceTarget {
        workspace_id: submission.required("workspace")?.into(),
        tx_data_path: submission.required("tx_data")?.into(),
        profile: submission.required("profile")?.into(),
        resource_path: submission.required("resource")?.into(),
    })
}

fn bridge_target(submission: &Submission) -> Result<BridgeTarget, String> {
    Ok(BridgeTarget {
        workspace_id: submission.required("workspace")?.into(),
        tx_data_path: submission.required("tx_data")?.into(),
        profile: submission.required("profile")?.into(),
        port: submission.number("port")?,
    })
}

fn rcon(context: &Context, submission: &Submission) -> Result<FxserverRconConfig, String> {
    Ok(FxserverRconConfig {
        host: submission.required("host")?.into(),
        port: submission.number("port")?,
        password: if submission.get("password").is_empty() {
            context.rcon.password.clone()
        } else {
            submission.get("password").into()
        },
    })
}

fn yes(submission: &Submission, key: &str) -> Result<bool, String> {
    match submission.required(key)? {
        "yes" => Ok(true),
        "no" => Ok(false),
        _ => Err(format!("{key} must be yes or no.")),
    }
}

fn capture_manager() -> &'static ProfilerCaptureManager {
    static MANAGER: OnceLock<ProfilerCaptureManager> = OnceLock::new();
    MANAGER.get_or_init(ProfilerCaptureManager::default)
}

// The process manager guard protects this session; the lease also excludes other UIs.
pub(super) async fn stopped_lease(context: &Context) -> Result<platform::ServerLease, String> {
    context.settings.validate_server()?;
    let path = context.settings.artifact_path.clone();
    commands::run_blocking(move || platform::ServerLease::acquire(Path::new(&path))).await
}

pub(super) fn require_selected_profile(
    context: &Context,
    tx_data: &str,
    profile: &str,
) -> Result<(), String> {
    let normalize = |value: &str| {
        if cfg!(windows) {
            value
                .trim()
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_ascii_lowercase()
        } else {
            value.trim().trim_end_matches('/').to_string()
        }
    };
    if normalize(tx_data) != normalize(&context.settings.tx_data_path)
        || normalize(profile) != normalize(&context.settings.profile)
    {
        return Err("Select this txData folder and profile in Settings before applying server-file changes; the stopped-server lease must match the selected server.".into());
    }
    Ok(())
}

pub(super) async fn execute(context: &Context, submission: &Submission) -> Result<Outcome, String> {
    let result = execute_inner(context, submission).await;
    let redact = |text: String| {
        redact(
            &text,
            &[
                &context.rcon.password,
                &context.credentials.password,
                submission.get("password"),
            ],
        )
    };
    result
        .map(|outcome| match outcome {
            Outcome::Text(text) => Outcome::Text(redact(text)),
            other => other,
        })
        .map_err(redact)
}

fn rcon_scope(context: &Context) -> Result<String, String> {
    let settings = &context.settings;
    if !Path::new(&settings.tx_data_path).is_absolute()
        || settings.profile.is_empty()
        || context.rcon.host.trim().is_empty()
        || context.rcon.port == 0
    {
        return Err("Select an absolute txData folder, profile, and RCON endpoint before remembering a password.".into());
    }
    let mut path = settings.tx_data_path.trim().to_string();
    let mut profile = settings.profile.clone();
    if cfg!(windows) {
        path = path.replace('\\', "/").to_ascii_lowercase();
        profile.make_ascii_lowercase();
    }
    let tuple = serde_json::to_vec(&(
        "fxserver-terminal-rcon-v1",
        path.trim_end_matches('/'),
        profile,
        context.rcon.host.trim().to_ascii_lowercase(),
        context.rcon.port,
    ))
    .map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(tuple)))
}

pub(in crate::terminal) async fn load_rcon(context: &Context) -> Result<Option<String>, String> {
    if context.settings.tx_data_path.is_empty() {
        return Ok(None);
    }
    fxserver::get_fxserver_rcon_password(Some(rcon_scope(context)?)).await
}

async fn execute_inner(context: &Context, submission: &Submission) -> Result<Outcome, String> {
    match submission.id.as_str() {
        "server.rcon-save" => {
            let password = if submission.get("password").is_empty() {
                context.rcon.password.clone()
            } else {
                submission.get("password").to_owned()
            };
            if password.is_empty()
                || password
                    .chars()
                    .any(|ch| ch.is_whitespace() || ch.is_control())
            {
                return Err(
                    "Enter a nonempty RCON password without whitespace or control characters."
                        .into(),
                );
            }
            fxserver::save_fxserver_rcon_password(password.clone(), Some(rcon_scope(context)?))
                .await?;
            Ok(Outcome::RconPassword(password))
        }
        "server.rcon-load" => {
            let password = load_rcon(context)
                .await?
                .ok_or("No RCON password is saved for this profile and endpoint.")?;
            Ok(Outcome::RconPassword(password))
        }
        "server.rcon-forget" => {
            fxserver::clear_fxserver_rcon_password(Some(rcon_scope(context)?)).await?;
            Ok(Outcome::RconPassword(String::new()))
        }
        "server.status" => {
            let manager = context.manager.clone();
            json(&commands::run_blocking(move || manager.status()).await?)
        }
        "server.profiles" => json(
            &fxserver::list_txdata_profiles(
                submission.required("tx_data")?.into(),
                Some(context.settings.artifact_path.clone()),
                Some(true),
            )
            .await?,
        ),
        "server.preflight" => json(
            &diagnostics::run_fxserver_preflight(diagnostics::PreflightRequest {
                artifact_path: submission.required("destination")?.into(),
                tx_data_path: submission.required("tx_data")?.into(),
                profile: submission.required("profile")?.into(),
                credentials: yes(submission, "database")?.then(|| context.credentials.clone()),
                check_ports: yes(submission, "ports")?,
            })
            .await?,
        ),
        "server.dependencies" => json(
            &diagnostics::resource_dependencies::scan_resource_dependencies(
                diagnostics::resource_dependencies::DependencyRequest {
                    artifact_path: submission.required("destination")?.into(),
                    tx_data_path: submission.required("tx_data")?.into(),
                    profile: submission.required("profile")?.into(),
                },
            )
            .await?,
        ),
        "server.rcon" => Ok(Outcome::Text(
            fxserver::send_fxserver_rcon_command(FxserverCommandRequest {
                command: submission.required("command")?.into(),
                rcon: rcon(context, submission)?,
            })
            .await?,
        )),
        "server.verify-rcon" => {
            let manager = context.manager.clone();
            let status = commands::run_blocking(move || manager.status()).await?;
            let pid = status
                .pid
                .filter(|_| status.running)
                .ok_or("Start a managed server before verifying RCON.")?;
            managed_rcon::verify_for_manager(
                pid,
                rcon(context, submission)?,
                context.manager.clone(),
            )
            .await?;
            Ok(Outcome::Text(
                "Local RCON endpoint verified against managed stdout.".into(),
            ))
        }
        "resource.inspect" => json(
            &fxserver::scan_fxserver_resources(ResourceScanRequest {
                tx_data_path: submission.required("tx_data")?.into(),
                profile: submission.required("profile")?.into(),
            })
            .await?,
        ),
        "resource.check-version" => json(
            &resource_updates::check_version(
                resource_target(submission)?,
                submission.required("branch")?.into(),
            )
            .await?,
        ),
        "resource.preview" => json(
            &resource_updates::preview_at(
                platform::data_dir()?,
                PreviewRequest {
                    target: resource_target(submission)?,
                    branch: submission.required("branch")?.into(),
                },
            )
            .await?,
        ),
        "resource.apply" => {
            let request = ApplyRequest {
                target: resource_target(submission)?,
                preview_id: submission.required("preview")?.into(),
                protected_paths: protected_paths(submission.get("protected")),
            };
            require_selected_profile(
                context,
                &request.target.tx_data_path,
                &request.target.profile,
            )?;
            let lease = stopped_lease(context).await?;
            json(
                &resource_updates::apply_at(
                    platform::data_dir()?,
                    context.manager.clone(),
                    request,
                    true,
                    Some(lease),
                )
                .await?,
            )
        }
        "resource.discard" => {
            resource_updates::discard_at(
                platform::data_dir()?,
                submission.required("preview")?.into(),
            )
            .await?;
            Ok(Outcome::Text("Staged preview discarded.".into()))
        }
        "resource.snapshots" => json(
            &resource_updates::snapshots_at(platform::data_dir()?, resource_target(submission)?)
                .await?,
        ),
        "resource.rollback-preview" => json(
            &resource_updates::preview_rollback_at(
                platform::data_dir()?,
                resource_target(submission)?,
                submission.required("snapshot")?.into(),
            )
            .await?,
        ),
        "resource.rollback" => {
            let target = resource_target(submission)?;
            require_selected_profile(context, &target.tx_data_path, &target.profile)?;
            let preview = submission.required("preview")?.into();
            let lease = stopped_lease(context).await?;
            json(
                &resource_updates::apply_rollback_preview_at(
                    platform::data_dir()?,
                    target,
                    preview,
                    context.manager.clone(),
                    Some(lease),
                )
                .await?,
            )
        }
        "resource.delete-snapshot" => {
            resource_updates::delete_snapshot_at(
                platform::data_dir()?,
                resource_target(submission)?,
                submission.required("snapshot")?.into(),
            )
            .await?;
            Ok(Outcome::Text("Resource snapshot deleted.".into()))
        }
        "artifact.installed" => json(
            &artifact::get_installed_windows_artifact_info(
                submission.required("destination")?.into(),
            )
            .await?,
        ),
        "artifact.legacy-list" => {
            json(&artifact::get_windows_artifact_catalog(yes(submission, "refresh")?).await?)
        }
        "artifact.enhanced-list" => json(&artifact::get_enhanced_artifact_catalog().await?),
        "artifact.legacy-install" => {
            let version = submission.required("version")?.to_string();
            let destination = submission.required("destination")?.to_string();
            let acknowledge_risk = yes(submission, "risk")?;
            let manager = context.manager.clone();
            json(
                &commands::run_blocking(move || {
                    let catalog = artifact::load_catalog(true)?;
                    let build = catalog
                        .builds
                        .iter()
                        .find(|build| build.version == version)
                        .ok_or("Exact version was not found in the official catalog.")?;
                    artifact::install_legacy_for_manager(
                        &manager,
                        ArtifactInstallRequest {
                            version,
                            url: build.download_url.clone(),
                            destination,
                            acknowledge_risk,
                        },
                    )
                })
                .await?,
            )
        }
        "artifact.enhanced-install" => {
            let version = submission.required("version")?.to_string();
            let destination = submission.required("destination")?.to_string();
            let manager = context.manager.clone();
            json(
                &commands::run_blocking(move || {
                    let catalog = artifact::load_enhanced_catalog()?;
                    let build = catalog
                        .builds
                        .iter()
                        .find(|build| build.version == version)
                        .ok_or("Exact version was not found in the published Enhanced catalog.")?;
                    artifact::install_enhanced_for_manager(
                        &manager,
                        EnhancedInstallRequest {
                            url: build.download_url.clone(),
                            destination,
                        },
                    )
                })
                .await?,
            )
        }
        "server.bridge-installation" => {
            json(&live_bridge::inspect_at(platform::data_dir()?, bridge_target(submission)?).await?)
        }
        "server.bridge-preview" => {
            let remove = match submission.required("operation")? {
                "install" => false,
                "remove" => true,
                _ => return Err("Choose install or remove.".into()),
            };
            json(
                &live_bridge::preview_at(platform::data_dir()?, bridge_target(submission)?, remove)
                    .await?,
            )
        }
        "server.bridge-apply" => {
            let preview = submission.required("preview")?.into();
            let target = bridge_target(submission)?;
            require_selected_profile(context, &target.tx_data_path, &target.profile)?;
            let lease = stopped_lease(context).await?;
            json(
                &live_bridge::apply_at(
                    platform::data_dir()?,
                    preview,
                    target,
                    context.manager.clone(),
                    Some(lease),
                )
                .await?,
            )
        }
        "server.bridge-snapshot" => json(
            &live_bridge::snapshot_at(platform::data_dir()?, bridge_target(submission)?).await?,
        ),
        "resource.bridge-action" => {
            live_bridge::action_at(
                platform::data_dir()?,
                bridge_target(submission)?,
                submission.required("instance")?.into(),
                submission.required("operation")?.into(),
                submission.required("resource")?.into(),
            )
            .await?;
            Ok(Outcome::Text("Bridge accepted the resource action.".into()))
        }
        "server.profiler-capture" => {
            if CAPTURE_RESULT
                .lock()
                .map_err(|_| "Capture result is unavailable.")?
                .is_some()
            {
                return Err(
                    "Export and discard the retained capture before recording another.".into(),
                );
            }
            let capture = profiler_capture::capture_for_manager(
                ProfilerCaptureRequest {
                    frames: submission.number("frames")?,
                    rcon: rcon(context, submission)?,
                },
                context.manager.clone(),
                capture_manager().clone(),
            );
            tokio::pin!(capture);
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut previous = String::new();
            let capture = loop {
                tokio::select! {
                    result = &mut capture => break result?,
                    _ = interval.tick() => {
                        if let Some(status) = capture_manager().status()? {
                            let message = format!("Profiler {}: {}/{} frames", status.phase, status.captured_frames, status.frames);
                            if message != previous {
                                context.report(&message, &[]);
                                previous = message;
                            }
                        }
                    }
                }
            };
            let outcome = json(&serde_json::json!({
                "id": capture.id, "fileName": capture.file_name,
                "frames": capture.frames, "bytes": capture.json.len(), "retainedForExport": true,
            }))?;
            *CAPTURE_RESULT
                .lock()
                .map_err(|_| "Capture result is unavailable.")? = Some(capture);
            Ok(outcome)
        }
        "server.profiler-export" => {
            let id = submission.required("capture")?.to_owned();
            let path = submission.required("path")?.to_owned();
            commands::run_blocking(move || {
                let result = CAPTURE_RESULT
                    .lock()
                    .map_err(|_| "Capture result is unavailable.")?;
                let capture = result
                    .as_ref()
                    .filter(|capture| capture.id == id)
                    .ok_or("That capture is not retained in this session.")?;
                export_capture(&path, &capture.json)
            })
            .await?;
            Ok(Outcome::Text("Captured profile exported. The in-memory copy remains until discarded or the terminal exits.".into()))
        }
        "server.profiler-discard" => {
            let id = submission.required("capture")?;
            let mut result = CAPTURE_RESULT
                .lock()
                .map_err(|_| "Capture result is unavailable.")?;
            if result.as_ref().is_none_or(|capture| capture.id != id) {
                return Err("That capture is not retained in this session.".into());
            }
            *result = None;
            Ok(Outcome::Text("Retained capture discarded.".into()))
        }
        _ => Err("Unknown server/resource/artifact action.".into()),
    }
}

fn protected_paths(value: &str) -> Vec<String> {
    // Literal semicolons in file names cannot be entered in this single-line form.
    value
        .split(';')
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect()
}

fn export_capture(path: &str, json: &str) -> Result<(), String> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err("Choose an absolute output path.".into());
    }
    commands::config_history::ensure_unlinked_path(path)?;
    let mut file = platform::private_options()
        .create_new(true)
        .open(path)
        .map_err(|error| {
            format!("Cannot create capture output (existing files are never overwritten): {error}")
        })?;
    file.write_all(json.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::super::FieldKind;
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn context() -> Context {
        Context {
            settings: Default::default(),
            manager: Default::default(),
            progress: None,
            credentials: crate::models::mariadb::MariaDBCredentials {
                host: "localhost".into(),
                port: 3306,
                username: "root".into(),
                password: "database-secret".into(),
                database: None,
            },
            rcon: FxserverRconConfig {
                host: "127.0.0.1".into(),
                port: 30120,
                password: "rcon-secret".into(),
            },
        }
    }

    #[test]
    fn catalog_has_unique_ids_confirmed_mutations_and_no_prefilled_secrets() {
        let actions = list(&context());
        let mut ids = BTreeSet::new();
        for action in &actions {
            assert!(ids.insert(action.id));
            assert!(matches!(
                action.id.split('.').next(),
                Some("server" | "resource" | "artifact")
            ));
            let mut fields = BTreeSet::new();
            for field in &action.fields {
                assert!(fields.insert(field.key));
                assert!(!field.value.contains("-secret"));
                if matches!(field.kind, FieldKind::Secret) {
                    assert!(field.value.is_empty());
                }
            }
        }
        for id in [
            "server.rcon-save",
            "server.rcon-forget",
            "server.rcon",
            "resource.apply",
            "resource.rollback",
            "resource.delete-snapshot",
            "resource.discard",
            "artifact.legacy-install",
            "artifact.enhanced-install",
            "server.bridge-apply",
            "resource.bridge-action",
            "server.profiler-capture",
            "server.profiler-discard",
        ] {
            assert!(
                actions
                    .iter()
                    .find(|action| action.id == id)
                    .unwrap()
                    .confirmation
                    .is_some(),
                "{id}"
            );
        }
    }

    #[test]
    fn redaction_covers_raw_and_json_escaped_passwords() {
        let secret = "quoted\"\\secret";
        let encoded = serde_json::to_string(&serde_json::json!({ "message": secret })).unwrap();
        let result = redact(&format!("{secret}\n{encoded}"), &[secret, ""]);
        assert!(!result.contains("secret"));
        assert!(result.contains("[redacted]"));
    }

    #[tokio::test]
    async fn unknown_actions_and_invalid_inputs_fail_without_operations() {
        let context = context();
        for (id, values) in [
            ("server.unknown", BTreeMap::new()),
            ("resource.apply", BTreeMap::new()),
            (
                "server.bridge-preview",
                BTreeMap::from([("operation".into(), "erase".into())]),
            ),
            (
                "server.profiler-capture",
                BTreeMap::from([
                    ("frames".into(), "999".into()),
                    ("host".into(), "127.0.0.1".into()),
                    ("port".into(), "30120".into()),
                ]),
            ),
        ] {
            assert!(execute(
                &context,
                &Submission {
                    id: id.into(),
                    values
                }
            )
            .await
            .is_err());
        }
    }

    #[test]
    fn choices_and_secret_fallback_are_explicit() {
        let mut submission = Submission {
            id: String::new(),
            values: BTreeMap::from([
                ("host".into(), "127.0.0.1".into()),
                ("port".into(), "30120".into()),
                ("ports".into(), "maybe".into()),
            ]),
        };
        assert!(yes(&submission, "ports").is_err());
        assert_eq!(
            rcon(&context(), &submission).unwrap().password,
            "rcon-secret"
        );
        submission
            .values
            .insert("password".into(), "override".into());
        assert_eq!(rcon(&context(), &submission).unwrap().password, "override");
        assert!(std::ptr::eq(capture_manager(), capture_manager()));
        assert_eq!(
            protected_paths(" config.lua ; data/local.json ;; "),
            vec!["config.lua", "data/local.json"]
        );
        assert!(!list(&context()).iter().any(|action| matches!(
            action.id,
            "server.profiler-cancel" | "server.profiler-status"
        )));
    }

    #[test]
    fn mutations_must_use_the_selected_profile_and_enhanced_warns_about_risk() {
        let mut context = context();
        context.settings.tx_data_path = std::env::temp_dir()
            .join("txData-fixture")
            .to_string_lossy()
            .into();
        assert!(require_selected_profile(
            &context,
            &context.settings.tx_data_path,
            &context.settings.profile
        )
        .is_ok());
        assert!(
            require_selected_profile(&context, &context.settings.tx_data_path, "other").is_err()
        );
        assert!(
            require_selected_profile(&context, "other-folder", &context.settings.profile).is_err()
        );
        let actions = list(&context);
        let enhanced = actions
            .iter()
            .find(|action| action.id == "artifact.enhanced-install")
            .unwrap();
        assert!(enhanced.description.contains("early access"));
        assert!(enhanced.confirmation.unwrap().contains("unstable"));
    }

    #[test]
    fn capture_export_never_overwrites_a_file() {
        let root = std::env::temp_dir().join(format!(
            "fxsi-capture-export-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("capture.json");
        export_capture(&path.to_string_lossy(), "{\"traceEvents\":[]}").unwrap();
        assert!(export_capture(&path.to_string_lossy(), "changed").is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"traceEvents\":[]}"
        );
        assert!(export_capture("relative.json", "{}").is_err());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn remembered_rcon_scope_is_stable_and_separates_profile_and_endpoint() {
        let mut context = context();
        context.settings.tx_data_path = std::env::temp_dir()
            .join("txData-fixture")
            .to_string_lossy()
            .into();
        let original = rcon_scope(&context).unwrap();
        assert_eq!(original, rcon_scope(&context).unwrap());
        assert_eq!(original.len(), 64);
        assert!(original.bytes().all(|byte| byte.is_ascii_hexdigit()));
        for field in ["path", "profile", "host", "port"] {
            let mut other = context.clone();
            match field {
                "path" => other.settings.tx_data_path.push_str("-other"),
                "profile" => other.settings.profile = "other".into(),
                "host" => other.rcon.host = "192.0.2.1".into(),
                "port" => other.rcon.port += 1,
                _ => unreachable!(),
            }
            assert_ne!(original, rcon_scope(&other).unwrap(), "{field}");
        }
        context.rcon.password = "different secret".into();
        assert_eq!(original, rcon_scope(&context).unwrap());
    }

    #[tokio::test]
    async fn unconfigured_rcon_load_does_not_touch_the_password_store() {
        assert!(load_rcon(&context()).await.unwrap().is_none());
    }
}
