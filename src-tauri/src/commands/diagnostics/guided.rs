use super::*;
use crate::commands::config_history::{self, ConfigChangeReason, ConfigFileRequest};
use crate::models::fxserver::ServerConfigFile;
use sha2::{Digest, Sha256};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Guidance {
    steps: Vec<&'static str>,
    page: &'static str,
    label: &'static str,
    patch_available: bool,
}

pub(super) fn guidance(check: &DiagnosticCheck, inspection: &Inspection) -> Option<Guidance> {
    let (page, label, steps): (_, _, Vec<_>) = match check.code.as_str() {
        "artifact-missing" => ("artifact-install", "Open artifacts", vec![
            "Choose the existing artifact folder containing FXServer.exe, or review an artifact installation.",
            "Rerun checks after correcting the artifact path. No executable is downloaded or launched by this check.",
        ]),
        "profile-invalid" | "profile-not-selected" | "txdata-missing" => ("server-manage", "Open server settings", vec![
            "Select the txData folder and profile. Verify the profile config.json points to the intended absolute dataPath.",
            "Rerun checks for that profile. Profile paths are not repaired automatically.",
        ]),
        "server-config-missing" | "config-unreadable" => ("server-configure", "Open configuration", vec![
            "Verify the profile dataPath and locate its server.cfg. Recover a known-good cfg from your backup if it is missing.",
            "Check file access, UTF-8 encoding, and the 512 KiB limit. No blank cfg or credentials will be generated.",
        ]),
        "exec-unresolved" | "config-parse-uncertain" | "config-repeated" => ("server-configure", "Open configuration", vec![
            "Review the reported cfg and line. Check quoting, the target spelling, and whether the included file exists.",
            "Plain exec paths resolve from the server data directory; @resource/file paths require an installed resource. Keep includes inside dataPath.",
            "Correct the reference in the editor and review the change before saving. Dynamic or repeated includes require manual review.",
        ]),
        "dependency-missing" | "configured-resource-missing" | "resources-missing" | "duplicate-resource" => ("resource-manager", "Open resources", vec![
            "Inspect the named resource, its manifest dependency, and the reported startup entry. Check for a renamed folder, missing manifest, or duplicate copy.",
            "Obtain a missing dependency from its trusted project source and review compatibility before installing. Do not remove a required dependency just to silence the check.",
            "Rerun checks after the resource files or reviewed startup configuration change. No download or resource start is performed here.",
        ]),
        "rcon-not-configured" => ("server-configure", "Open RCON configuration", vec![
            "Review rcon_password in the executed cfg files, including later overrides and empty values.",
            "Set or replace the credential explicitly in the configuration editor and review the save. This diagnostic never creates, reveals, or rotates a credential.",
        ]),
        "rconlog-not-started" => ("server-configure", "Open configuration", vec![
            "Verify that rconlog is installed and that an executed cfg starts it directly or through its resource group.",
            "A reviewed patch is available only for an unambiguous installed rconlog with no resource dependencies and a complete static config scan.",
            "The patch adds only ensure rconlog for the next server start. It does not change credentials, start resources, or change services.",
        ]),
        "database-unavailable" => ("mariadb", "Open MariaDB", vec![
            "Review the selected database, client installation, and session connection settings in Manage MariaDB.",
            "Retry the read-only connection test. No service, account, database, or credential is changed by this check.",
        ]),
        "port-in-use" | "port-unavailable" | "endpoints-missing" | "endpoint-dynamic" => ("server-configure", "Open endpoint configuration", vec![
            "Review the configured endpoint and identify any existing listener before changing a port.",
            "Keep firewall and service changes manual. Rerun port checks with FXServer stopped.",
        ]),
        _ => return None,
    };
    Some(Guidance {
        steps,
        page,
        label,
        patch_available: check.code == "rconlog-not-started" && patch_target(inspection).is_ok(),
    })
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigPatchPreview {
    id: String,
    expires_at: u64,
    path: String,
    before: String,
    after: String,
}

struct PendingPatch {
    request: PreflightRequest,
    target: ConfigFileRequest,
    revision: String,
    evidence: String,
    after: String,
    created: Instant,
}

#[tauri::command]
pub async fn preview_diagnostic_config_patch(
    request: PreflightRequest,
) -> Result<ConfigPatchPreview, String> {
    super::super::run_blocking(move || prepare_patch(request)).await
}

#[tauri::command]
pub async fn apply_diagnostic_config_patch(
    app: AppHandle,
    preview_id: String,
    manager: tauri::State<'_, super::super::fxserver::FxserverManager>,
) -> Result<ServerConfigFile, String> {
    let manager = manager.inner().clone();
    super::super::run_blocking(move || {
        manager
            .with_stopped_server(|| apply_patch(&config_history::history_root(&app)?, &preview_id))
    })
    .await
}

fn pending() -> &'static Mutex<BTreeMap<String, PendingPatch>> {
    static PENDING: OnceLock<Mutex<BTreeMap<String, PendingPatch>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn patch_target(inspection: &Inspection) -> Result<&Config, String> {
    let fail = "A safe rconlog patch is not available. Resolve the config/resource findings and review configuration manually.";
    let root = inspection.data_root.as_ref().ok_or(fail)?;
    if !inspection
        .checks
        .iter()
        .any(|check| check.code == "rconlog-not-started")
        || inspection.checks.iter().any(|check| {
            check.severity == Severity::Error
                || matches!(
                    check.code.as_str(),
                    "exec-unresolved"
                        | "config-unreadable"
                        | "config-limit"
                        | "config-repeated"
                        | "config-parse-uncertain"
                        | "scan-limit"
                        | "check-limit"
                        | "manifest-unreadable"
                        | "resource-unreadable"
                        | "resource-link-cycle"
                        | "resource-link-broken"
                        | "dynamic-resource-reference"
                )
        })
    {
        return Err(fail.into());
    }
    let resources: Vec<_> = inspection
        .resources
        .iter()
        .filter(|resource| resource.name.eq_ignore_ascii_case("rconlog"))
        .collect();
    if resources.len() != 1
        || resources[0].manifest.dynamic
        || !resources[0].manifest.dependencies.is_empty()
        || inspection
            .configs
            .iter()
            .flat_map(|config| &config.commands)
            .any(|(_, words)| {
                matches!(words[0].to_ascii_lowercase().as_str(), "stop" | "restart")
                    && words.get(1).is_some_and(|target| {
                        target.eq_ignore_ascii_case("rconlog")
                            || resources[0]
                                .groups
                                .iter()
                                .any(|group| group.eq_ignore_ascii_case(target))
                    })
            })
    {
        return Err(fail.into());
    }
    inspection
        .configs
        .iter()
        .find(|config| config.path == root.join("server.cfg"))
        .ok_or_else(|| fail.into())
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn evidence(inspection: &Inspection) -> Result<String, String> {
    let configs: BTreeMap<_, _> = inspection
        .configs
        .iter()
        .map(|config| (&config.path, digest(config.source.as_bytes())))
        .collect();
    let resources: BTreeMap<_, _> = inspection
        .resources
        .iter()
        .map(|resource| {
            (
                &resource.path,
                (&resource.resolved_path, &resource.revision),
            )
        })
        .collect();
    serde_json::to_vec(&(&inspection.data_root, configs, resources))
        .map(|bytes| digest(&bytes))
        .map_err(|_| "Cannot prepare diagnostic evidence.".into())
}

fn prepare_patch(mut request: PreflightRequest) -> Result<ConfigPatchPreview, String> {
    request.credentials = None;
    request.check_ports = false;
    let inspection = inspect(&request);
    let config = patch_target(&inspection)?;
    let newline = if config.source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let after = format!(
        "{}{}ensure rconlog{newline}",
        config.source,
        if config.source.is_empty() || config.source.ends_with('\n') {
            ""
        } else {
            newline
        }
    );
    if after.len() as u64 > MAX_FILE_BYTES {
        return Err("The patched config would exceed 512 KiB.".into());
    }
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let id = format!("rconlog-{}-{}", now(), NEXT.fetch_add(1, Ordering::Relaxed));
    let preview = ConfigPatchPreview {
        id: id.clone(),
        expires_at: now() + PREVIEW_TTL.as_secs(),
        path: config.path.to_string_lossy().into(),
        before: config.source.clone(),
        after: after.clone(),
    };
    let target = ConfigFileRequest {
        tx_data_path: request.tx_data_path.clone(),
        profile: request.profile.clone(),
        path: preview.path.clone(),
    };
    let patch = PendingPatch {
        request,
        target,
        revision: digest(config.source.as_bytes()),
        evidence: evidence(&inspection)?,
        after,
        created: Instant::now(),
    };
    let mut cache = pending()
        .lock()
        .map_err(|_| "Diagnostic patch previews are unavailable.")?;
    cache.retain(|_, patch| patch.created.elapsed() < PREVIEW_TTL);
    if cache.len() >= 4 {
        if let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, patch)| patch.created)
            .map(|(id, _)| id.clone())
        {
            cache.remove(&oldest);
        }
    }
    cache.insert(id, patch);
    Ok(preview)
}

fn apply_patch(store: &Path, id: &str) -> Result<ServerConfigFile, String> {
    let patch = pending()
        .lock()
        .map_err(|_| "Diagnostic patch previews are unavailable.")?
        .remove(id)
        .ok_or("Repair preview expired or was already used. Review a new patch.")?;
    if patch.created.elapsed() >= PREVIEW_TTL {
        return Err("Repair preview expired. Review a new patch.".into());
    }
    let inspection = inspect(&patch.request);
    let config = patch_target(&inspection)?;
    if config.path.to_string_lossy() != patch.target.path
        || evidence(&inspection)? != patch.evidence
    {
        return Err("CONFIG_CHANGED: Configuration or resource evidence changed after review. Rerun diagnostics and review a new patch.".into());
    }
    config_history::save_config_with_revision(
        store,
        &patch.target,
        &patch.revision,
        &patch.after,
        ConfigChangeReason::Patch,
    )
}

#[cfg(test)]
mod tests;
