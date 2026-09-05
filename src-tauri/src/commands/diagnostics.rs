pub mod guided;
mod parsing;
mod redaction;

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    net::{SocketAddr, TcpListener, UdpSocket},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::{models::mariadb::MariaDBCredentials, process::CommandNoWindowExt};

const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_CONFIGS: usize = 128;
const MAX_RESOURCES: usize = 5000;
const MAX_RESOURCE_ENTRIES: usize = 20_000;
const MAX_CHECKS: usize = 2000;
const PREVIEW_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightRequest {
    pub artifact_path: String,
    pub tx_data_path: String,
    pub profile: String,
    pub credentials: Option<MariaDBCredentials>,
    #[serde(default = "enabled")]
    pub check_ports: bool,
}

fn enabled() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
    Pass,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCheck {
    pub category: String,
    pub code: String,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub resource: Option<String>,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub guidance: Option<guided::Guidance>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightReport {
    pub checked_at: u64,
    pub blocking: bool,
    pub error_count: usize,
    pub warning_count: usize,
    pub resource_count: usize,
    pub config_count: usize,
    pub checks: Vec<DiagnosticCheck>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticPreviewRequest {
    pub preflight: PreflightRequest,
    #[serde(default)]
    pub include_application_log: bool,
    #[serde(default)]
    pub include_server_log: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEntry {
    pub name: String,
    pub content: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticPreview {
    pub id: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub entries: Vec<DiagnosticEntry>,
    pub total_bytes: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticExportResult {
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Default)]
struct Inspection {
    checks: Vec<DiagnosticCheck>,
    resources: Vec<Resource>,
    configs: Vec<Config>,
    executed_commands: Vec<Vec<String>>,
    secrets: Vec<String>,
    data_root: Option<PathBuf>,
    database_version: Option<String>,
}

struct Resource {
    name: String,
    path: PathBuf,
    resolved_path: PathBuf,
    groups: Vec<String>,
    manifest: parsing::Manifest,
    revision: String,
}

struct Config {
    path: PathBuf,
    commands: Vec<(usize, Vec<String>)>,
    source: String,
}

#[tauri::command]
pub async fn run_fxserver_preflight(request: PreflightRequest) -> Result<PreflightReport, String> {
    super::run_blocking(move || {
        let inspection = inspect(&request);
        Ok(report(&inspection))
    })
    .await
}

#[tauri::command]
pub async fn preview_diagnostic_export(
    app: AppHandle,
    request: DiagnosticPreviewRequest,
) -> Result<DiagnosticPreview, String> {
    super::run_blocking(move || {
        let app_log = app
            .path()
            .app_data_dir()
            .ok()
            .map(|path| path.join("logs/fxserver-installer.log"));
        prepare_preview(
            &request,
            app_log.as_deref(),
            &app.package_info().version.to_string(),
        )
    })
    .await
}

#[tauri::command]
pub async fn export_diagnostic_zip(
    preview_id: String,
    path: String,
) -> Result<DiagnosticExportResult, String> {
    super::run_blocking(move || export_preview(&preview_id, Path::new(&path))).await
}

fn check(
    inspection: &mut Inspection,
    category: &str,
    code: &str,
    severity: Severity,
    title: &str,
    detail: impl Into<String>,
) {
    inspection.checks.push(DiagnosticCheck {
        category: category.into(),
        code: code.into(),
        severity,
        title: title.into(),
        detail: detail.into(),
        resource: None,
        file: None,
        line: None,
        guidance: None,
    });
}

fn report(inspection: &Inspection) -> PreflightReport {
    let mut checks = inspection.checks.clone();
    for item in &mut checks {
        item.guidance = guided::guidance(item, inspection);
        item.detail = redact_known(&item.detail, &inspection.secrets);
        item.resource = item
            .resource
            .as_ref()
            .map(|value| redact_known(value, &inspection.secrets));
        item.file = item
            .file
            .as_ref()
            .map(|value| redact_known(value, &inspection.secrets));
    }
    checks.sort_by_key(|item| match item.severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
        Severity::Pass => 3,
    });
    let error_count = checks
        .iter()
        .filter(|item| item.severity == Severity::Error)
        .count();
    let warning_count = checks
        .iter()
        .filter(|item| item.severity == Severity::Warning)
        .count();
    PreflightReport {
        checked_at: now(),
        blocking: error_count > 0,
        error_count,
        warning_count,
        resource_count: inspection.resources.len(),
        config_count: inspection.configs.len(),
        checks,
    }
}

fn resolve_data_root(request: &PreflightRequest) -> Result<PathBuf, String> {
    super::fxserver::resolve_profile_data_path(
        request.tx_data_path.clone(),
        request.profile.clone(),
    )
    .map(|(_, _, _, root)| root)
}

fn inspect(request: &PreflightRequest) -> Inspection {
    let mut inspection = Inspection::default();
    if let Some(credentials) = &request.credentials {
        if !credentials.password.is_empty() {
            inspection.secrets.push(credentials.password.clone());
        }
    }
    let artifact = Path::new(request.artifact_path.trim());
    if !request.artifact_path.trim().is_empty() && artifact.join("FXServer.exe").is_file() {
        check(
            &mut inspection,
            "Paths",
            "artifact-found",
            Severity::Pass,
            "FXServer executable",
            "FXServer.exe is present in the artifact directory.",
        );
    } else {
        check(
            &mut inspection,
            "Paths",
            "artifact-missing",
            Severity::Error,
            "FXServer executable missing",
            "Install an artifact or choose the directory containing FXServer.exe.",
        );
    }
    if request.profile.trim().is_empty() {
        if !request.tx_data_path.trim().is_empty()
            && !Path::new(request.tx_data_path.trim()).is_dir()
        {
            check(&mut inspection, "Paths", "txdata-missing", Severity::Error, "Configured txData directory missing", "The configured txData directory does not exist. Correct the path or clear it for a fresh txAdmin setup.");
        } else {
            check(&mut inspection, "Paths", "profile-not-selected", Severity::Warning, "No txAdmin profile selected", "FXServer can open txAdmin for initial setup. Select a profile afterward to check server configs, resources, and ports.");
        }
    } else {
        match resolve_data_root(request) {
            Ok(root) => {
                check(
                    &mut inspection,
                    "Paths",
                    "profile-found",
                    Severity::Pass,
                    "txAdmin profile",
                    "The profile resolves to an existing server data directory.",
                );
                inspection.data_root = Some(root.clone());
                let resources = root.join("resources");
                if resources.is_dir() {
                    scan_resources(
                        &resources,
                        &resources,
                        0,
                        &mut ResourceScan::default(),
                        &mut inspection,
                    );
                    inspection
                        .resources
                        .sort_by_key(|resource| resource.name.to_ascii_lowercase());
                } else {
                    check(
                        &mut inspection,
                        "Resources",
                        "resources-missing",
                        Severity::Error,
                        "Resources directory missing",
                        "The resources folder is missing, unreadable, or its link target is unavailable.",
                    );
                }
                read_config(
                    &root.join("server.cfg"),
                    &root,
                    &mut HashSet::new(),
                    &mut inspection,
                    true,
                );
                inspect_dependencies(&root, &mut inspection);
                inspect_config(&root, request.check_ports, &mut inspection);
            }
            Err(detail) => check(
                &mut inspection,
                "Paths",
                "profile-invalid",
                Severity::Error,
                "txAdmin profile unavailable",
                detail,
            ),
        }
    }
    match &request.credentials {
        Some(credentials) => match validate_database(credentials) {
            Ok(version) => {
                inspection.database_version = Some(version);
                check(&mut inspection, "Database", "database-connected", Severity::Pass, "Database connection", "The supplied credentials connected successfully using a read-only version query.");
            }
            Err(detail) => check(&mut inspection, "Database", "database-unavailable", Severity::Warning, "Database connection failed", detail),
        },
        None => check(&mut inspection, "Database", "database-skipped", Severity::Info, "Database connection not checked", "No session credentials were supplied. No connection string is extracted or executed from config files."),
    }
    inspection
        .secrets
        .sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    inspection.secrets.dedup();
    inspection
}

fn safe_component(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".." && !value.contains(['/', '\\', ':', '\0'])
}

fn read_bounded(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|_| "File is unreadable.".to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "File is unreadable.".to_string())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("File exceeds the diagnostic size limit.".into());
    }
    String::from_utf8(bytes).map_err(|_| "File is not valid UTF-8.".into())
}

#[derive(Default)]
struct ResourceScan {
    ancestors: HashSet<PathBuf>,
    entries: usize,
}

fn scan_resources(
    path: &Path,
    root: &Path,
    depth: usize,
    scan: &mut ResourceScan,
    inspection: &mut Inspection,
) {
    if depth > 12
        || inspection.resources.len() >= MAX_RESOURCES
        || scan.entries >= MAX_RESOURCE_ENTRIES
    {
        check(
            inspection,
            "Resources",
            "scan-limit",
            Severity::Warning,
            "Resource scan limit reached",
            "Part of the resource tree was skipped because of its size or nesting depth.",
        );
        return;
    }
    let Ok(resolved) = path.canonicalize() else {
        check(
            inspection,
            "Resources",
            "resource-unreadable",
            Severity::Warning,
            "Resource directory unavailable",
            format!(
                "{} could not be resolved. Check its link target and access permissions.",
                relative(path, root)
            ),
        );
        return;
    };
    if scan.ancestors.contains(&resolved) {
        check(
            inspection,
            "Resources",
            "resource-link-cycle",
            Severity::Warning,
            "Cyclic resource link skipped",
            format!("{} links back to a parent directory.", relative(path, root)),
        );
        return;
    }
    if path != root {
        let manifest_path = ["fxmanifest.lua", "__resource.lua"]
            .iter()
            .map(|name| path.join(name))
            .find(|path| path.is_file());
        if let Some(manifest_path) = manifest_path {
            if !manifest_path
                .canonicalize()
                .is_ok_and(|path| path.starts_with(&resolved))
            {
                check(
                    inspection,
                    "Resources",
                    "manifest-unreadable",
                    Severity::Warning,
                    "Resource manifest outside its resource",
                    "A manifest resolves outside its resource's target directory and was not read.",
                );
                return;
            }
            match read_bounded(&manifest_path) {
                Ok(source) => inspection.resources.push(Resource {
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    path: path.to_path_buf(),
                    resolved_path: resolved,
                    groups: path
                        .strip_prefix(root)
                        .unwrap_or(path)
                        .components()
                        .filter_map(|part| {
                            let name = part.as_os_str().to_string_lossy();
                            (name.starts_with('[') && name.ends_with(']'))
                                .then(|| name.into_owned())
                        })
                        .collect(),
                    manifest: parsing::manifest(&source),
                    revision: guided::digest(source.as_bytes()),
                }),
                Err(_) => check(
                    inspection,
                    "Resources",
                    "manifest-unreadable",
                    Severity::Warning,
                    "Resource manifest unreadable",
                    format!(
                        "A resource manifest under {} could not be parsed.",
                        relative(path, root)
                    ),
                ),
            }
            return;
        }
    }
    let Ok(entries) = fs::read_dir(path) else {
        check(
            inspection,
            "Resources",
            "resource-unreadable",
            Severity::Warning,
            "Resource directory unreadable",
            "Some resources could not be inspected.",
        );
        return;
    };
    // Track ancestors, not all visited targets: separate resource aliases remain valid.
    scan.ancestors.insert(resolved.clone());
    for entry in entries.flatten() {
        if inspection.resources.len() >= MAX_RESOURCES || scan.entries >= MAX_RESOURCE_ENTRIES {
            check(
                inspection,
                "Resources",
                "scan-limit",
                Severity::Warning,
                "Resource scan limit reached",
                "Additional resource entries were skipped after the diagnostic limit.",
            );
            break;
        }
        scan.entries += 1;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.')
            || matches!(
                name.to_ascii_lowercase().as_str(),
                "node_modules" | "cache" | "tmp" | "temp"
            )
        {
            continue;
        }
        let child = entry.path();
        if child.is_dir() {
            scan_resources(&child, root, depth + 1, scan, inspection);
        } else if entry.file_type().is_ok_and(|kind| kind.is_symlink()) && !child.exists() {
            check(
                inspection,
                "Resources",
                "resource-link-broken",
                Severity::Warning,
                "Broken resource link",
                format!(
                    "{} points to an unavailable target.",
                    relative(&child, root)
                ),
            );
        }
    }
    scan.ancestors.remove(&resolved);
}

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map(|value| value.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| "[outside data directory]".into())
}

fn config_target(reference: &str, root: &Path, resources: &[Resource]) -> Option<PathBuf> {
    if reference.contains(['$', '*', '?', '\0']) {
        return None;
    }
    let (path, allowed_root) = if let Some(resource_ref) = reference.strip_prefix('@') {
        let (name, file) = resource_ref.split_once('/')?;
        let resource = resources
            .iter()
            .find(|resource| resource.name.eq_ignore_ascii_case(name))?;
        (resource.path.join(file), resource.resolved_path.as_path())
    } else {
        (root.join(reference), root)
    };
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    let resolved = path.canonicalize().ok()?;
    (resolved.starts_with(allowed_root) && resolved.is_file()).then_some(resolved)
}

fn read_config(
    path: &Path,
    root: &Path,
    visited: &mut HashSet<PathBuf>,
    inspection: &mut Inspection,
    required: bool,
) {
    let Ok(path) = path.canonicalize() else {
        check(
            inspection,
            "Configuration",
            "server-config-missing",
            Severity::Error,
            "server.cfg missing",
            "The server data directory must contain a readable server.cfg.",
        );
        return;
    };
    if !path.starts_with(root)
        && !inspection
            .resources
            .iter()
            .any(|resource| path.starts_with(&resource.resolved_path))
    {
        check(
            inspection,
            "Configuration",
            "exec-unresolved",
            Severity::Warning,
            "Config outside dataPath",
            "A config resolved outside the server data directory and was not read.",
        );
        return;
    }
    if !visited.insert(path.clone()) {
        check(
            inspection,
            "Configuration",
            "config-repeated",
            Severity::Warning,
            "Repeated config include",
            "A repeated or cyclic exec was read only once. Review execution order manually.",
        );
        return;
    }
    if visited.len() > MAX_CONFIGS {
        check(
            inspection,
            "Configuration",
            "config-limit",
            Severity::Warning,
            "Config scan limit reached",
            "Additional exec files were skipped after 128 files.",
        );
        return;
    }
    let source = match read_bounded(&path) {
        Ok(source) => source,
        Err(_) => {
            check(
                inspection,
                "Configuration",
                "config-unreadable",
                if required {
                    Severity::Error
                } else {
                    Severity::Warning
                },
                "Config file unreadable",
                format!(
                    "{} is unreadable or exceeds the size limit.",
                    relative(&path, root)
                ),
            );
            return;
        }
    };
    let (commands, uncertain) = parsing::config_commands_checked(&source);
    if uncertain {
        check(
            inspection,
            "Configuration",
            "config-parse-uncertain",
            Severity::Warning,
            "Config quoting needs review",
            "An unterminated quoted value prevents a complete static config review.",
        );
        if let Some(check) = inspection.checks.last_mut() {
            check.file = Some(relative(&path, root));
        }
    }
    for (_, words) in &commands {
        let offset = usize::from(matches!(
            words[0].to_ascii_lowercase().as_str(),
            "set" | "setr" | "sets"
        ));
        if let (Some(name), Some(value)) = (words.get(offset), words.get(offset + 1)) {
            let lowered = name.to_ascii_lowercase();
            if ["password", "secret", "token", "key", "connection_string"]
                .iter()
                .any(|key| lowered.contains(key))
                && !value.is_empty()
            {
                inspection.secrets.push(value.clone());
            }
        }
    }
    for (line, words) in &commands {
        inspection.executed_commands.push(words.clone());
        if words[0].eq_ignore_ascii_case("exec") {
            if let Some(reference) = words.get(1) {
                if let Some(target) = config_target(reference, root, &inspection.resources) {
                    read_config(&target, root, visited, inspection, false);
                } else {
                    check(
                        inspection,
                        "Configuration",
                        "exec-unresolved",
                        Severity::Warning,
                        "Included config not resolved",
                        "An exec target is missing, dynamic, or outside the server data directory.",
                    );
                    if let Some(check) = inspection.checks.last_mut() {
                        check.file = Some(relative(&path, root));
                        check.line = Some(*line);
                    }
                }
            } else {
                check(
                    inspection,
                    "Configuration",
                    "exec-unresolved",
                    Severity::Warning,
                    "Included config not specified",
                    "An exec command has no target file.",
                );
                if let Some(check) = inspection.checks.last_mut() {
                    check.file = Some(relative(&path, root));
                    check.line = Some(*line);
                }
            }
        }
    }
    inspection.configs.push(Config {
        path,
        commands,
        source,
    });
}

fn inspect_dependencies(root: &Path, inspection: &mut Inspection) {
    let mut checks = Vec::new();
    let mut names: BTreeMap<String, Vec<&Resource>> = BTreeMap::new();
    for resource in &inspection.resources {
        names
            .entry(resource.name.to_ascii_lowercase())
            .or_default()
            .push(resource);
    }
    let providers: BTreeMap<String, &Resource> = inspection
        .resources
        .iter()
        .flat_map(|resource| {
            resource
                .manifest
                .provides
                .iter()
                .map(move |name| (name.to_ascii_lowercase(), resource))
        })
        .collect();
    let mut enabled = BTreeSet::new();
    for config in &inspection.configs {
        for (line, words) in &config.commands {
            if checks.len() >= MAX_CHECKS {
                break;
            }
            if !matches!(words[0].to_ascii_lowercase().as_str(), "ensure" | "start") {
                continue;
            }
            let Some(target) = words.get(1) else {
                continue;
            };
            if target.contains(['$', '*', '?']) {
                checks.push(DiagnosticCheck {
                    category: "Resources".into(), code: "dynamic-resource-reference".into(), severity: Severity::Warning,
                    title: "Resource reference needs review".into(), detail: "A dynamic startup reference could not be resolved without executing configuration commands.".into(),
                    resource: None, file: Some(relative(&config.path, root)), line: Some(*line),
                    guidance: None,
                });
                continue;
            }
            let found: Vec<_> = inspection
                .resources
                .iter()
                .filter(|resource| {
                    resource.name.eq_ignore_ascii_case(target)
                        || resource
                            .groups
                            .iter()
                            .any(|group| group.eq_ignore_ascii_case(target))
                })
                .collect();
            if found.is_empty() && !providers.contains_key(&target.to_ascii_lowercase()) {
                checks.push(DiagnosticCheck {
                    category: "Resources".into(),
                    code: "configured-resource-missing".into(),
                    severity: Severity::Error,
                    title: "Configured resource missing".into(),
                    detail: format!("No resource or non-empty group matches {target}."),
                    resource: Some(target.clone()),
                    file: Some(relative(&config.path, root)),
                    line: Some(*line),
                    guidance: None,
                });
            }
            for resource in found {
                enabled.insert(resource.name.to_ascii_lowercase());
            }
            if let Some(provider) = providers.get(&target.to_ascii_lowercase()) {
                enabled.insert(provider.name.to_ascii_lowercase());
            }
        }
    }
    let mut pending: Vec<_> = enabled.iter().cloned().collect();
    while let Some(name) = pending.pop() {
        if let Some(resources) = names.get(&name) {
            for resource in resources {
                for dependency in &resource.manifest.dependencies {
                    let key = dependency.to_ascii_lowercase();
                    let target = providers
                        .get(&key)
                        .map(|provider| provider.name.to_ascii_lowercase())
                        .unwrap_or(key);
                    if !dependency.starts_with('/')
                        && names.contains_key(&target)
                        && enabled.insert(target.clone())
                    {
                        pending.push(target);
                    }
                }
            }
        }
    }
    for resources in names.values().filter(|resources| resources.len() > 1) {
        if checks.len() >= MAX_CHECKS {
            break;
        }
        checks.push(DiagnosticCheck {
            category: "Resources".into(),
            code: "duplicate-resource".into(),
            severity: Severity::Error,
            title: "Duplicate resource name".into(),
            detail: format!(
                "{} exists in {} folders. FXServer resource resolution is ambiguous.",
                resources[0].name,
                resources.len()
            ),
            resource: Some(resources[0].name.clone()),
            file: None,
            line: None,
            guidance: None,
        });
    }
    for resource in &inspection.resources {
        for dependency in &resource.manifest.dependencies {
            if checks.len() >= MAX_CHECKS {
                break;
            }
            if dependency.starts_with('/')
                || names.contains_key(&dependency.to_ascii_lowercase())
                || providers.contains_key(&dependency.to_ascii_lowercase())
            {
                continue;
            }
            checks.push(DiagnosticCheck {
                category: "Dependencies".into(),
                code: "dependency-missing".into(),
                severity: if enabled.contains(&resource.name.to_ascii_lowercase()) {
                    Severity::Error
                } else {
                    Severity::Warning
                },
                title: "Required dependency missing".into(),
                detail: format!("{} requires {dependency}.", resource.name),
                resource: Some(resource.name.clone()),
                file: Some(relative(&resource.path, root)),
                line: None,
                guidance: None,
            });
        }
        if resource.manifest.dynamic && checks.len() < MAX_CHECKS {
            checks.push(DiagnosticCheck {
                category: "Dependencies".into(),
                code: "dynamic-dependency".into(),
                severity: Severity::Info,
                title: "Dynamic manifest metadata".into(),
                detail:
                    "Non-literal dependencies need manual review; manifest Lua is never executed."
                        .into(),
                resource: Some(resource.name.clone()),
                file: None,
                line: None,
                guidance: None,
            });
        }
    }
    let limit_reached = checks.len() >= MAX_CHECKS;
    inspection.checks.extend(checks);
    if limit_reached {
        check(inspection, "Resources", "check-limit", Severity::Warning, "Additional findings omitted", "Only the first 2,000 resource findings are shown. Resolve these findings and run the checks again.");
    }
    check(inspection, "Resources", "resource-scan", Severity::Info, "Resource inventory", format!("{} manifests inspected. Runtime constraints such as /server and /onesync are not treated as resource names.", inspection.resources.len()));
}

fn inspect_config(root: &Path, check_ports: bool, inspection: &mut Inspection) {
    let mut rcon_configured = false;
    let mut rconlog = false;
    let mut endpoints = BTreeSet::new();
    for words in &inspection.executed_commands {
        let command = words[0].to_ascii_lowercase();
        let offset = usize::from(matches!(command.as_str(), "set" | "setr" | "sets"));
        if words
            .get(offset)
            .is_some_and(|name| name.eq_ignore_ascii_case("rcon_password"))
        {
            rcon_configured = words
                .get(offset + 1)
                .is_some_and(|value| !value.is_empty() && !value.contains('$'));
        }
        if matches!(command.as_str(), "ensure" | "start" | "stop") {
            if let Some(target) = words.get(1) {
                let matches = target.eq_ignore_ascii_case("rconlog")
                    || inspection.resources.iter().any(|resource| {
                        resource.name.eq_ignore_ascii_case("rconlog")
                            && resource
                                .groups
                                .iter()
                                .any(|group| group.eq_ignore_ascii_case(target))
                    });
                if matches {
                    rconlog = command != "stop";
                }
            }
        }
        if matches!(command.as_str(), "endpoint_add_tcp" | "endpoint_add_udp") {
            if let Some(endpoint) = words.get(1) {
                endpoints.insert((command, endpoint.clone()));
            }
        }
    }
    if !rcon_configured {
        check(inspection, "RCON", "rcon-not-configured", Severity::Warning, "RCON password missing", "Set a non-empty rcon_password in an executed cfg to use console input and resource controls.");
    }
    if !rconlog {
        check(
            inspection,
            "RCON",
            "rconlog-not-started",
            Severity::Warning,
            "RCON logging not configured",
            "Add ensure rconlog to an executed cfg, or start the resource's group.",
        );
    }
    if rcon_configured && rconlog {
        check(inspection, "RCON", "rcon-configured", Severity::Pass, "RCON configuration", "A non-empty credential and a startup entry for rconlog were found. Authentication has not been attempted.");
    }
    if inspection
        .configs
        .iter()
        .any(|config| config.path == root.join("server.cfg"))
    {
        check(
            inspection,
            "Configuration",
            "config-loaded",
            Severity::Pass,
            "Config files inspected",
            format!(
                "Read server.cfg and {} included cfg file(s).",
                inspection.configs.len().saturating_sub(1)
            ),
        );
    }
    if !check_ports {
        check(
            inspection,
            "Ports",
            "ports-skipped",
            Severity::Info,
            "Port availability not checked",
            "Port availability was not checked for this request. Existing server connections are left untouched.",
        );
    } else if endpoints.is_empty() {
        check(
            inspection,
            "Ports",
            "endpoints-missing",
            Severity::Warning,
            "No static endpoints found",
            "Review endpoint_add_tcp and endpoint_add_udp in the executed cfg files.",
        );
    } else {
        for (protocol, endpoint) in endpoints {
            let Ok(address) = endpoint.parse::<SocketAddr>() else {
                check(
                    inspection,
                    "Ports",
                    "endpoint-dynamic",
                    Severity::Warning,
                    "Endpoint not checked",
                    "A configured endpoint is dynamic or not an IP address and port.",
                );
                continue;
            };
            let result = if protocol.ends_with("tcp") {
                TcpListener::bind(address).map(|_| ())
            } else {
                UdpSocket::bind(address).map(|_| ())
            };
            let protocol = if protocol.ends_with("tcp") {
                "TCP"
            } else {
                "UDP"
            };
            match result {
                Ok(()) => check(
                    inspection,
                    "Ports",
                    "port-available",
                    Severity::Pass,
                    "Configured port available",
                    format!(
                        "{protocol} port {} was available at check time.",
                        address.port()
                    ),
                ),
                Err(error) => {
                    let occupied = error.kind() == std::io::ErrorKind::AddrInUse;
                    check(
                        inspection,
                        "Ports",
                        if occupied {
                            "port-in-use"
                        } else {
                            "port-unavailable"
                        },
                        if occupied {
                            Severity::Error
                        } else {
                            Severity::Warning
                        },
                        "Configured port unavailable",
                        format!(
                            "{protocol} port {} {}. No firewall or network settings were changed.",
                            address.port(),
                            if occupied {
                                "is already in use"
                            } else {
                                "could not be bound on this machine"
                            }
                        ),
                    );
                }
            }
        }
    }
}

fn validate_database(credentials: &MariaDBCredentials) -> Result<String, &'static str> {
    use crate::services::mariadb::query::{apply_credentials_args, find_mariadb_client};
    let client = find_mariadb_client()
        .ok_or("MariaDB client was not found. Verify the connection in Manage MariaDB.")?;
    let mut command = Command::new(client);
    command.no_window();
    apply_credentials_args(&mut command, credentials);
    command
        .args([
            "--connect-timeout=3",
            "--batch",
            "--skip-column-names",
            "-e",
            "SELECT VERSION();",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(database) = credentials
        .database
        .as_ref()
        .filter(|database| !database.trim().is_empty())
    {
        command.arg(format!("--database={database}"));
    }
    let mut child = command
        .spawn()
        .map_err(|_| "MariaDB client could not be started.")?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err("Connection or authentication failed. Check the selected database and credentials in Manage MariaDB.");
                }
                let mut version = String::new();
                if let Some(stdout) = child.stdout.take() {
                    let _ = stdout.take(1024).read_to_string(&mut version);
                }
                return Ok(redaction::text(version.trim()));
            }
            Ok(None) if started.elapsed() < Duration::from_secs(6) => {
                std::thread::sleep(Duration::from_millis(50))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Database connection check timed out.");
            }
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn redact_known(value: &str, secrets: &[String]) -> String {
    let mut result = value.to_string();
    for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
        result = result.replace(secret, "[redacted]");
    }
    result
}

fn json_entry(
    name: &str,
    mut value: serde_json::Value,
    secrets: &[String],
) -> Result<DiagnosticEntry, String> {
    redaction::json(&mut value);
    redact_json_values(&mut value, secrets);
    let content = serde_json::to_string_pretty(&value)
        .map_err(|_| "Could not prepare diagnostic JSON.".to_string())?;
    Ok(DiagnosticEntry {
        name: name.into(),
        content,
    })
}

fn redact_json_values(value: &mut serde_json::Value, secrets: &[String]) {
    match value {
        serde_json::Value::String(value) => *value = redact_known(value, secrets),
        serde_json::Value::Array(values) => values
            .iter_mut()
            .for_each(|value| redact_json_values(value, secrets)),
        serde_json::Value::Object(values) => values
            .values_mut()
            .for_each(|value| redact_json_values(value, secrets)),
        _ => {}
    }
}

fn prepare_preview(
    request: &DiagnosticPreviewRequest,
    app_log: Option<&Path>,
    version: &str,
) -> Result<DiagnosticPreview, String> {
    let mut preflight = request.preflight.clone();
    preflight.check_ports = false;
    let inspection = inspect(&preflight);
    let artifact_version =
        read_bounded(&Path::new(&preflight.artifact_path).join(".fxserver-artifact-version"))
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| {
                !value.is_empty()
                    && value
                        .chars()
                        .all(|character| character.is_ascii_digit() || character == '.')
            });
    let created_at = now();
    let mut entries = vec![
        json_entry(
            "manifest.json",
            json!({ "formatVersion": 1, "createdAt": created_at, "appVersion": version, "platform": std::env::consts::OS, "architecture": std::env::consts::ARCH, "artifactVersion": artifact_version, "databaseVersion": inspection.database_version, "contents": "Redacted summaries; no full cfg, database data, or credential files.", "logLimit": "Last 200 lines per log; 64 KiB read limit." }),
            &inspection.secrets,
        )?,
        json_entry(
            "preflight.json",
            serde_json::to_value(report(&inspection))
                .map_err(|_| "Could not prepare diagnostic report.".to_string())?,
            &inspection.secrets,
        )?,
        json_entry(
            "configuration.json",
            json!({ "fileCount": inspection.configs.len(), "files": inspection.configs.iter().enumerate().map(|(index, config)| {
            let mut command_counts = BTreeMap::<String, usize>::new();
            for (_, words) in &config.commands {
                let command = words[0].to_ascii_lowercase();
                let name = if matches!(command.as_str(), "set" | "setr" | "sets" | "exec" | "ensure" | "start" | "stop" | "add_ace" | "add_principal" | "endpoint_add_tcp" | "endpoint_add_udp") { command } else { "other".into() };
                *command_counts.entry(name).or_default() += 1;
            }
            json!({ "file": format!("config-{}", index + 1), "commands": command_counts })
        }).collect::<Vec<_>>() }),
            &inspection.secrets,
        )?,
        json_entry(
            "resources.json",
            json!({ "totalResources": inspection.resources.len(), "includedResources": inspection.resources.len().min(1000), "resources": inspection.resources.iter().take(1000).map(|resource| json!({ "name": resource.name, "dependencyCount": resource.manifest.dependencies.len(), "dependencies": resource.manifest.dependencies.iter().take(40).collect::<Vec<_>>(), "provides": resource.manifest.provides.iter().take(10).collect::<Vec<_>>(), "dynamicMetadata": resource.manifest.dynamic })).collect::<Vec<_>>() }),
            &inspection.secrets,
        )?,
    ];
    if request.include_application_log {
        entries.push(log_entry(
            "logs/application.log",
            app_log,
            &inspection.secrets,
        ));
    }
    if request.include_server_log {
        let directory = Path::new(&preflight.tx_data_path)
            .join(&preflight.profile)
            .join("logs");
        let log = if safe_component(&preflight.profile) {
            latest_server_log(&directory)
        } else {
            None
        };
        entries.push(log_entry(
            "logs/server.log",
            log.as_deref(),
            &inspection.secrets,
        ));
    }
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    let id = format!(
        "diagnostics-{created_at}-{}",
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    );
    let total_bytes = entries.iter().map(|entry| entry.content.len()).sum();
    let preview = DiagnosticPreview {
        id,
        created_at,
        expires_at: created_at + PREVIEW_TTL.as_secs(),
        entries,
        total_bytes,
    };
    let mut cache = previews()
        .lock()
        .map_err(|_| "Diagnostic preview cache is unavailable.".to_string())?;
    cache.retain(|_, preview| preview.expires_at > now());
    if cache.len() >= 4 {
        if let Some(id) = cache
            .values()
            .min_by_key(|preview| preview.created_at)
            .map(|preview| preview.id.clone())
        {
            cache.remove(&id);
        }
    }
    cache.insert(preview.id.clone(), preview.clone());
    Ok(preview)
}

fn previews() -> &'static Mutex<BTreeMap<String, DiagnosticPreview>> {
    static PREVIEWS: OnceLock<Mutex<BTreeMap<String, DiagnosticPreview>>> = OnceLock::new();
    PREVIEWS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn latest_server_log(directory: &Path) -> Option<PathBuf> {
    fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            name.starts_with("fxserver")
                && name.ends_with(".log")
                && entry
                    .file_type()
                    .is_ok_and(|kind| kind.is_file() && !kind.is_symlink())
        })
        .max_by_key(|entry| {
            entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
        })
        .map(|entry| entry.path())
}

fn log_entry(name: &str, path: Option<&Path>, secrets: &[String]) -> DiagnosticEntry {
    let content = path
        .and_then(|path| tail_log(path).ok())
        .map(|content| redaction::logs(&redact_known(&content, secrets)))
        .unwrap_or_else(|| "Log unavailable; no file was included.".into());
    DiagnosticEntry {
        name: name.into(),
        content,
    }
}

fn tail_log(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let start = size.saturating_sub(64 * 1024);
    file.seek(SeekFrom::Start(start))?;
    let mut content = Vec::new();
    file.take(64 * 1024).read_to_end(&mut content)?;
    let content = String::from_utf8_lossy(&content);
    let mut lines: Vec<_> = content.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    Ok(lines[lines.len().saturating_sub(200)..].join("\n"))
}

fn export_preview(id: &str, path: &Path) -> Result<DiagnosticExportResult, String> {
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        return Err("Choose an absolute .zip file path.".into());
    }
    let preview = previews()
        .lock()
        .map_err(|_| "Diagnostic preview cache is unavailable.".to_string())?
        .get(id)
        .cloned()
        .filter(|preview| preview.expires_at > now())
        .ok_or("The diagnostic preview expired. Generate and review a new preview.")?;
    write_archive(path, &preview.entries)?;
    let size_bytes = fs::metadata(path)
        .map_err(|_| "Could not verify the diagnostic archive.".to_string())?
        .len();
    if let Ok(mut cache) = previews().lock() {
        cache.remove(id);
    }
    Ok(DiagnosticExportResult {
        path: path.to_string_lossy().into_owned(),
        size_bytes,
    })
}

fn write_archive(path: &Path, entries: &[DiagnosticEntry]) -> Result<(), String> {
    let file = OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|_| "Could not create the ZIP. Choose a new file name in a writable directory; existing files are never overwritten.".to_string())?;
    let result = (|| -> Result<(), String> {
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for entry in entries {
            archive
                .start_file(&entry.name, options)
                .map_err(|_| "Could not create an archive entry.")?;
            archive
                .write_all(entry.content.as_bytes())
                .map_err(|_| "Could not write the archive. Check free disk space.")?;
        }
        archive
            .finish()
            .map_err(|_| "Could not finish the diagnostic archive.")?
            .sync_all()
            .map_err(|_| "Could not flush the diagnostic archive to disk.")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

#[cfg(test)]
mod tests;
