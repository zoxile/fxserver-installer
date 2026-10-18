use crate::models::mariadb::MariaDBCredentials;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;
#[path = "workspace_clone_database.rs"]
mod database;

const MAX_FILES: usize = 50_000;
const MAX_FILE: u64 = 256 * 1024 * 1024;
const MAX_TOTAL: u64 = 20 * 1024 * 1024 * 1024;
const MAX_TEXT: u64 = 8 * 1024 * 1024;
const MANIFEST: &str = "clone-manifest.json";
const LIVE_BRIDGE_RESOURCE: &str = "fxserver_installer_bridge";
const LIVE_BRIDGE_BEGIN: &str = "# BEGIN FXSERVER INSTALLER LIVE BRIDGE";
const LIVE_BRIDGE_END: &str = "# END FXSERVER INSTALLER LIVE BRIDGE";
const LIVE_BRIDGE_EXCLUSION: &str =
    "Machine-paired Live Bridge excluded; reinstall and pair Live Bridge in the new workspace.";
static OPERATION: Mutex<()> = Mutex::new(());
static PREVIEWS: OnceLock<Mutex<BTreeMap<String, Prepared>>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloneRequest {
    pub source_path: String,
    pub destination_path: String,
    pub mode: CloneMode,
    pub resources: Vec<String>,
    pub configs: Vec<String>,
    pub server_port: u16,
    pub tx_admin_port: u16,
    pub source_server_port: u16,
    pub source_tx_admin_port: u16,
    #[serde(default)]
    pub database: Option<database::DatabaseSelection>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CloneMode {
    Clone,
    Export,
    Import,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneChoices {
    source_path: String,
    resources: Vec<String>,
    configs: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PackageFile {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PackageManifest {
    schema_version: u32,
    usage: String,
    server_port: u16,
    tx_admin_port: u16,
    files: Vec<PackageFile>,
    #[serde(default)]
    database: Option<database::DatabasePackage>,
}

#[derive(Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CloneExclusion {
    path: String,
    reason: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClonePreview {
    id: String,
    source_path: String,
    destination_path: String,
    mode: CloneMode,
    server_port: u16,
    tx_admin_port: u16,
    files: Vec<PackageFile>,
    excluded: Vec<CloneExclusion>,
    total_bytes: u64,
    expires_at: u64,
    database: Option<database::DatabasePreview>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneResult {
    destination_path: String,
    server_data_path: String,
    tx_data_path: String,
    artifact_path: String,
    file_count: usize,
    database: Option<database::DatabaseDefaults>,
}

struct Prepared {
    request: CloneRequest,
    preview: ClonePreview,
}
struct PlannedFile {
    source: Option<PathBuf>,
    file: PackageFile,
    original_sha256: String,
    generated: Option<Vec<u8>>,
}
struct Plan {
    root: PathBuf,
    destination: PathBuf,
    files: Vec<PlannedFile>,
    excluded: Vec<CloneExclusion>,
    database: Option<database::DatabasePlan>,
}

#[tauri::command]
pub async fn list_workspace_clone_choices(source_path: String) -> Result<CloneChoices, String> {
    super::run_blocking(move || list_choices(&source_path)).await
}

#[tauri::command]
pub async fn preview_workspace_clone(request: CloneRequest) -> Result<ClonePreview, String> {
    super::run_blocking(move || {
        let _operation = OPERATION
            .try_lock()
            .map_err(|_| "A clone operation is already running.")?;
        super::require_other_work_idle()?;
        let plan = build_plan(&request)?;
        let total_bytes = plan_bytes(&plan);
        check_disk(
            plan.destination
                .parent()
                .ok_or("Missing destination parent.")?,
            total_bytes,
        )?;
        let preview = ClonePreview {
            id: unique_id(),
            source_path: display(&plan.root),
            destination_path: display(&plan.destination),
            mode: request.mode.clone(),
            server_port: request.server_port,
            tx_admin_port: request.tx_admin_port,
            files: plan.files.iter().map(|item| item.file.clone()).collect(),
            excluded: plan.excluded,
            total_bytes,
            expires_at: now() + 900,
            database: plan
                .database
                .as_ref()
                .map(|database| database::preview(database, &request))
                .transpose()?,
        };
        let mut previews = previews()
            .lock()
            .map_err(|_| "Clone preview storage is unavailable.")?;
        previews.retain(|_, value| value.preview.expires_at > now());
        if previews.len() >= 4 {
            return Err("Close an existing clone preview before preparing another.".into());
        }
        previews.insert(
            preview.id.clone(),
            Prepared {
                request,
                preview: preview.clone(),
            },
        );
        Ok(preview)
    })
    .await
}

#[tauri::command]
pub async fn discard_workspace_clone_preview(preview_id: String) -> Result<(), String> {
    super::run_blocking(move || {
        previews()
            .lock()
            .map_err(|_| "Clone preview storage is unavailable.")?
            .remove(&preview_id);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn execute_workspace_clone(
    app: tauri::AppHandle,
    preview_id: String,
    confirmed_destination: String,
    private_copy_confirmed: bool,
    database_credentials: Option<MariaDBCredentials>,
    confirmed_database: Option<String>,
) -> Result<CloneResult, String> {
    super::run_blocking(move || {
        let _operation = OPERATION
            .try_lock()
            .map_err(|_| "A clone operation is already running.")?;
        super::require_other_work_idle()?;
        if !private_copy_confirmed {
            return Err("Confirm that you have permission to make this private copy.".into());
        }
        let prepared = previews()
            .lock()
            .map_err(|_| "Clone preview storage is unavailable.")?
            .remove(&preview_id)
            .ok_or("Preview expired. Preview the clone again.")?;
        if prepared.preview.expires_at <= now() {
            return Err("Preview expired. Preview the clone again.".into());
        }
        if confirmed_destination != prepared.preview.destination_path {
            return Err("The destination does not match the reviewed preview.".into());
        }
        let plan = build_plan(&prepared.request)?;
        let files: Vec<_> = plan.files.iter().map(|item| item.file.clone()).collect();
        if files != prepared.preview.files
            || plan.excluded != prepared.preview.excluded
            || display(&plan.root) != prepared.preview.source_path
            || display(&plan.destination) != prepared.preview.destination_path
        {
            return Err(
                "The source changed after preview. Review a new preview before copying.".into(),
            );
        }
        if plan
            .database
            .as_ref()
            .map(|db| (&db.package.sha256, db.package.size_bytes))
            != prepared
                .preview
                .database
                .as_ref()
                .map(|db| (&db.sha256, db.size_bytes))
        {
            return Err("Database dump changed after preview.".into());
        }
        let imports_database =
            plan.database.is_some() && prepared.request.mode != CloneMode::Export;
        let _maintenance = if imports_database {
            Some(super::mariadb::maintenance_access()?)
        } else {
            None
        };
        let mut database_run = if imports_database {
            let root = app.path().app_local_data_dir().map_err(io_error)?;
            check_no_links(&root)?;
            let evidence_root = root.join("workspace-clone-evidence");
            #[cfg(target_os = "linux")]
            linux_fs::mkdir_all(&evidence_root)?;
            #[cfg(not(target_os = "linux"))]
            match fs::create_dir(&evidence_root) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io_error(error)),
            }
            Some(database::DatabaseRun::new(
                plan.database.as_ref().unwrap(),
                prepared.preview.database.as_ref().unwrap(),
                database_credentials.ok_or("Enter target database credentials.")?,
                confirmed_database.as_deref().unwrap_or(""),
                prepared.request.database.as_ref().unwrap(),
                &evidence_root,
            )?)
        } else {
            None
        };
        let mut result = execute_plan(&prepared.request, &plan, || {
            super::require_other_work_idle()?;
            if let Some(run) = &mut database_run {
                run.import()?;
            }
            Ok(())
        });
        if let Some(run) = &database_run {
            run.finish(result.is_ok())?;
            if let Ok(result) = &mut result {
                result.database = Some(run.defaults());
            }
        }
        result
    })
    .await
}

fn previews() -> &'static Mutex<BTreeMap<String, Prepared>> {
    PREVIEWS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn unique_id() -> String {
    format!(
        "{}-{}-{}",
        std::process::id(),
        now(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )
}
fn io_error(error: impl std::fmt::Display) -> String {
    format!("Clone filesystem operation failed: {error}")
}
fn display(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string()
}

fn validate_segment(segment: &str) -> Result<(), String> {
    let base = segment.split('.').next().unwrap_or("").to_ascii_uppercase();
    if segment.is_empty()
        || segment == "."
        || segment == ".."
        || segment.ends_with(['.', ' '])
        || segment
            .chars()
            .any(|c| c.is_control() || r#"<>:"/\|?*"#.contains(c))
        || matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && base.as_bytes()[3].is_ascii_digit())
    {
        return Err("Invalid, reserved, or unsafe path component.".into());
    }
    Ok(())
}

fn relative(value: &str) -> Result<PathBuf, String> {
    if value.len() > 1024 || value.contains('\\') {
        return Err("Package paths must use relative forward-slash paths.".into());
    }
    for segment in value.split('/') {
        validate_segment(segment)?;
    }
    let path = PathBuf::from(value);
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("External path references are not allowed.".into());
    }
    Ok(path)
}

fn validate_absolute(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("Choose an absolute local path without traversal.".into());
    }
    #[cfg(windows)]
    if !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
    {
        return Err("Only local drive paths are supported, not network or device paths.".into());
    }
    for part in path.components() {
        if let Component::Normal(value) = part {
            let value = value
                .to_str()
                .ok_or("Non-Unicode paths are not supported.")?;
            #[cfg(windows)]
            validate_segment(value)?;
            #[cfg(not(windows))]
            if value.chars().any(char::is_control) {
                return Err("Control characters are not supported in filesystem paths.".into());
            }
        }
    }
    Ok(())
}

fn check_metadata(metadata: &fs::Metadata) -> Result<(), String> {
    if metadata.file_type().is_symlink() {
        return Err("Symbolic links are not allowed in clone paths.".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Junctions and reparse points are not allowed in clone paths.".into());
        }
    }
    Ok(())
}

fn check_no_links(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        check_metadata(&fs::symlink_metadata(ancestor).map_err(io_error)?)?;
    }
    Ok(())
}

fn pin_directories(path: &Path) -> Result<Vec<File>, String> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
            FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        let mut handles = Vec::new();
        for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
            // Attribute-only handles do not enforce Windows delete-sharing checks.
            let handle = OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(ancestor)
                .map_err(|error| {
                    format!(
                        "Cannot safely pin clone directory {}: {error}",
                        display(ancestor)
                    )
                })?;
            let metadata = handle.metadata().map_err(io_error)?;
            check_metadata(&metadata)?;
            if !metadata.is_dir() {
                return Err("A clone path component is not a directory.".into());
            }
            handles.push(handle);
        }
        Ok(handles)
    }
    #[cfg(target_os = "linux")]
    {
        Ok(vec![linux_fs::directory(path)?])
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = path;
        Ok(Vec::new())
    }
}

fn source_root(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    validate_absolute(path)?;
    let _handles = pin_directories(path)?;
    check_no_links(path)?;
    if !path.is_dir() {
        return Err("The source must be an existing folder.".into());
    }
    #[cfg(target_os = "linux")]
    return Ok(path.to_path_buf());
    #[cfg(not(target_os = "linux"))]
    path.canonicalize().map_err(io_error)
}

fn destination_path(value: &str, source: &Path) -> Result<PathBuf, String> {
    let path = Path::new(value);
    validate_absolute(path)?;
    let parent = source_root(
        path.parent()
            .and_then(Path::to_str)
            .ok_or("Choose an existing destination parent folder.")?,
    )?;
    let target = parent.join(
        path.file_name()
            .ok_or("Choose a new destination folder name.")?,
    );
    if target.starts_with(source) || source.starts_with(&target) {
        return Err("Source and destination must be separate, non-nested folders.".into());
    }
    require_missing(&target)?;
    Ok(target)
}

fn require_missing(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err("The destination already exists. Nothing will be overwritten.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

fn list_choices(value: &str) -> Result<CloneChoices, String> {
    let root = source_root(value)?;
    let mut choices = CloneChoices {
        source_path: display(&root),
        resources: Vec::new(),
        configs: Vec::new(),
    };
    let resources = root.join("resources");
    if resources.exists() {
        discover_resources(&resources, &resources, 0, &mut choices.resources)?;
    }
    for entry in fs::read_dir(&root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.to_ascii_lowercase().ends_with(".cfg") {
            check_no_links(&entry.path())?;
            if entry.file_type().map_err(io_error)?.is_file() {
                relative(&name)?;
                choices.configs.push(name);
            }
        }
    }
    choices.resources.sort();
    choices.configs.sort();
    Ok(choices)
}

fn discover_resources(
    root: &Path,
    folder: &Path,
    depth: usize,
    output: &mut Vec<String>,
) -> Result<(), String> {
    let _handles = pin_directories(folder)?;
    check_no_links(folder)?;
    if depth > 8 || output.len() > 5000 {
        return Err("Resource discovery limit exceeded.".into());
    }
    for entry in fs::read_dir(folder).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        check_no_links(&entry.path())?;
        if !entry.file_type().map_err(io_error)?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        validate_segment(&name)?;
        if name.starts_with('[') && name.ends_with(']') {
            discover_resources(root, &entry.path(), depth + 1, output)?;
        } else {
            output.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(io_error)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}

fn excluded_name(path: &str) -> bool {
    path.split('/').any(|part| {
        let part = part.to_ascii_lowercase();
        let ext = part.rsplit('.').next().unwrap_or("");
        matches!(
            part.as_str(),
            ".git"
                | ".svn"
                | ".hg"
                | "node_modules"
                | "cache"
                | "logs"
                | "crashes"
                | "backups"
                | "admin.json"
                | "admins.json"
                | "players.json"
                | "id_rsa"
                | "id_dsa"
                | "id_ecdsa"
                | "id_ed25519"
        ) || part == LIVE_BRIDGE_RESOURCE
            || part.starts_with(".env")
            || part.starts_with('.')
            || matches!(
                ext,
                "sql"
                    | "db"
                    | "sqlite"
                    | "sqlite3"
                    | "log"
                    | "bak"
                    | "pem"
                    | "key"
                    | "pfx"
                    | "p12"
                    | "zip"
                    | "7z"
                    | "rar"
                    | "exe"
                    | "dll"
            )
            || [
                "password",
                "passwd",
                "secret",
                "credential",
                "token",
                "licensekey",
                "api_key",
                "api-key",
                "apikey",
            ]
            .iter()
            .any(|word| part.contains(word))
    })
}

fn exclude_bridge_resource(plan: &mut Plan, path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    let Some(index) = parts
        .iter()
        .position(|part| part.eq_ignore_ascii_case(LIVE_BRIDGE_RESOURCE))
    else {
        return false;
    };
    let path = parts[..=index].join("/");
    if !plan
        .excluded
        .iter()
        .any(|item| item.path.eq_ignore_ascii_case(&path))
    {
        plan.excluded.push(CloneExclusion {
            path,
            reason: LIVE_BRIDGE_EXCLUSION.into(),
        });
    }
    true
}

fn sensitive_text(text: &str) -> bool {
    let value = text.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "dbpass",
        "db_pass",
        "pwd=",
        "pwd =",
        "\"pwd\"",
        "'pwd'",
        "`pwd`",
        "secret",
        "token",
        "licensekey",
        "license_key",
        "api_key",
        "apikey",
        "api-key",
        "mysql_connection",
        "connectionstring",
        "connection_string",
        "authorization",
        "webhook",
        "private key",
        "private_key",
        "privatekey",
        "private-key",
        "access_key",
        "accesskey",
        "steam_webapikey",
        "cfxk_",
        "github_pat_",
        "ghp_",
        "bearer ",
        "mysql://",
        "mariadb://",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

fn external_reference(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains(":/")
        || lower.contains(":\\")
        || lower.contains("../")
        || lower.contains("..\\")
        || lower.contains("\\\\")
        || lower.contains("file:")
        || lower
            .split([' ', '\t', '"', '\'', '='])
            .any(|word| word.starts_with('/') || word.starts_with('\\'))
}

fn sanitize_cfg(text: &str) -> Vec<u8> {
    let mut output = String::new();
    let mut bridge_block_depth = 0usize;
    for line in text.lines() {
        let trimmed = line.trim_start_matches('\u{feff}').trim();
        if trimmed == LIVE_BRIDGE_BEGIN {
            bridge_block_depth += 1;
            continue;
        }
        if trimmed == LIVE_BRIDGE_END {
            bridge_block_depth = bridge_block_depth.saturating_sub(1);
            continue;
        }
        if bridge_block_depth > 0 {
            continue;
        }
        let mut words = trimmed.split_whitespace();
        let command = words
            .next()
            .unwrap_or("")
            .trim_matches(['"', '\''])
            .to_ascii_lowercase();
        if matches!(command.as_str(), "ensure" | "start")
            && words.next().is_some_and(|resource| {
                resource
                    .trim_matches(['"', '\''])
                    .eq_ignore_ascii_case(LIVE_BRIDGE_RESOURCE)
            })
        {
            continue;
        }
        if trimmed.is_empty() {
            output.push('\n');
            continue;
        }
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        if sensitive_text(line)
            || line.contains('\0')
            || external_reference(line)
            || line.contains(';')
            || matches!(
                command.as_str(),
                "exec"
                    | "endpoint_add_tcp"
                    | "endpoint_add_udp"
                    | "rcon_password"
                    | "setrconpassword"
                    | "txadminport"
            )
            || ["txhost_", "sv_listing", "sv_master", "sv_endpoint"]
                .iter()
                .any(|word| trimmed.to_ascii_lowercase().contains(word))
        {
            output.push_str(
                "# Excluded during private clone: secret, endpoint, or external dependency.\n",
            );
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    output.into_bytes()
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let _handles = pin_directories(path.parent().ok_or("Missing source parent.")?)?;
    check_no_links(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ);
    }
    #[cfg(not(target_os = "linux"))]
    let mut file = options.open(path).map_err(io_error)?;
    #[cfg(target_os = "linux")]
    let mut file = linux_fs::read(path)?;
    let metadata = file.metadata().map_err(io_error)?;
    check_metadata(&metadata)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err("A selected file is not regular or exceeds the clone size limit.".into());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    #[cfg(target_os = "linux")]
    linux_fs::unchanged(&file, &metadata)?;
    if bytes.len() as u64 > limit {
        return Err("A source file grew beyond the clone size limit.".into());
    }
    Ok(bytes)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn transformed(path: &str, bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    if excluded_name(path) {
        return Ok(None);
    }
    // UTF-16 ASCII keys otherwise appear as valid UTF-8 with interleaved NULs.
    if bytes.contains(&0) && sensitive_text(&String::from_utf8_lossy(bytes).replace('\0', "")) {
        return Ok(None);
    }
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if ext == "cfg" {
        if bytes.len() as u64 > MAX_TEXT {
            return Err("Configuration exceeds the text size limit.".into());
        }
        return Ok(Some(sanitize_cfg(
            std::str::from_utf8(bytes).map_err(|_| "Configuration must be UTF-8 text.")?,
        )));
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        if text.contains('\0') {
            return Ok(None);
        }
        let name = path.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
        let license_notice = name == "license"
            || name.starts_with("license.")
            || name == "copying"
            || name.starts_with("copying.")
            || name == "notice"
            || name.starts_with("notice.");
        if sensitive_text(text) || (!license_notice && external_reference(text)) {
            return Ok(None);
        }
        return Ok(Some(bytes.to_vec()));
    }
    if matches!(
        ext.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "webp"
            | "gif"
            | "ico"
            | "ogg"
            | "wav"
            | "mp3"
            | "mp4"
            | "woff"
            | "woff2"
            | "ttf"
            | "ytd"
            | "ydr"
            | "yft"
            | "ybn"
            | "ymap"
            | "ytyp"
            | "ycd"
            | "ydd"
            | "ymt"
            | "awc"
            | "gfx"
    ) {
        let text = String::from_utf8_lossy(bytes);
        if sensitive_text(&text) {
            return Ok(None);
        }
        return Ok(Some(bytes.to_vec()));
    }
    Ok(None)
}

fn add_file(
    plan: &mut Plan,
    source: PathBuf,
    output: String,
    seen: &mut BTreeSet<String>,
) -> Result<(), String> {
    relative(&output)?;
    if !seen.insert(output.to_ascii_lowercase()) {
        return Err("Case-insensitive path collision in selected files.".into());
    }
    if seen.len() > MAX_FILES {
        return Err("Clone file limit exceeded.".into());
    }
    if exclude_bridge_resource(plan, &output) {
        return Ok(());
    }
    if excluded_name(&output) {
        plan.excluded.push(CloneExclusion {
            path: output,
            reason: "Private data, generated files, or unsupported payload".into(),
        });
        return Ok(());
    }
    let bytes = read_bounded(&source, MAX_FILE)?;
    let original_sha256 = digest(&bytes);
    match transformed(&output, &bytes)? {
        Some(data) => {
            if data != bytes {
                let has_bridge_setup = String::from_utf8_lossy(&bytes)
                    .to_ascii_lowercase()
                    .contains(LIVE_BRIDGE_RESOURCE)
                    || bytes
                        .windows(LIVE_BRIDGE_BEGIN.len())
                        .any(|part| part == LIVE_BRIDGE_BEGIN.as_bytes());
                plan.excluded.push(CloneExclusion {
                    path: output.clone(),
                    reason: if has_bridge_setup {
                        format!("Sensitive settings and machine-paired Live Bridge setup removed. {LIVE_BRIDGE_EXCLUSION}")
                    } else {
                        "Sensitive settings, endpoints, and exec dependencies removed".into()
                    },
                });
            }
            plan.files.push(PlannedFile {
                source: Some(source),
                file: PackageFile {
                    path: output,
                    size: data.len() as u64,
                    sha256: digest(&data),
                },
                original_sha256,
                generated: None,
            });
        }
        None => plan.excluded.push(CloneExclusion {
            path: output,
            reason: "Potential secret, external reference, or opaque binary".into(),
        }),
    }
    Ok(())
}

fn walk(
    plan: &mut Plan,
    folder: &Path,
    data_root: &Path,
    seen: &mut BTreeSet<String>,
    depth: usize,
) -> Result<(), String> {
    if depth > 40 {
        return Err("Resource nesting limit exceeded.".into());
    }
    let _handles = pin_directories(folder)?;
    check_no_links(folder)?;
    let rel = folder
        .strip_prefix(data_root)
        .map_err(io_error)?
        .to_string_lossy()
        .replace('\\', "/");
    if exclude_bridge_resource(plan, &format!("server-data/{rel}")) {
        return Ok(());
    }
    for entry in fs::read_dir(folder).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        check_no_links(&entry.path())?;
        let rel = entry
            .path()
            .strip_prefix(data_root)
            .map_err(io_error)?
            .to_string_lossy()
            .replace('\\', "/");
        let output = format!("server-data/{rel}");
        relative(&output)?;
        if exclude_bridge_resource(plan, &output) {
            continue;
        }
        if entry.file_type().map_err(io_error)?.is_dir() {
            if excluded_name(&output) {
                plan.excluded.push(CloneExclusion {
                    path: output,
                    reason: "Private or generated folder".into(),
                });
            } else {
                walk(plan, &entry.path(), data_root, seen, depth + 1)?;
            }
        } else {
            add_file(plan, entry.path(), output, seen)?;
        }
        if plan.excluded.len() + plan.files.len() > MAX_FILES {
            return Err("Clone entry limit exceeded.".into());
        }
    }
    Ok(())
}

fn validate_ports(request: &CloneRequest) -> Result<(), String> {
    if request.server_port == 0
        || request.tx_admin_port == 0
        || request.server_port == request.tx_admin_port
        || request.server_port == 40120
        || request.tx_admin_port == 30120
        || [request.source_server_port, request.source_tx_admin_port].contains(&request.server_port)
        || [request.source_server_port, request.source_tx_admin_port]
            .contains(&request.tx_admin_port)
    {
        return Err(
            "Choose distinct server and txAdmin ports, different from both source ports.".into(),
        );
    }
    Ok(())
}

fn build_plan(request: &CloneRequest) -> Result<Plan, String> {
    validate_ports(request)?;
    let root = source_root(&request.source_path)?;
    let destination = destination_path(&request.destination_path, &root)?;
    let mut plan = Plan {
        root: root.clone(),
        destination,
        files: Vec::new(),
        excluded: Vec::new(),
        database: None,
    };
    let mut seen = BTreeSet::new();
    if request.mode == CloneMode::Import {
        let manifest: PackageManifest =
            serde_json::from_slice(&read_bounded(&root.join(MANIFEST), 16 * 1024 * 1024)?)
                .map_err(|_| "Invalid clone package manifest.")?;
        if manifest.schema_version != 1
            || manifest.usage != "private-user-copy"
            || manifest.files.len() > MAX_FILES
            || [manifest.server_port, manifest.tx_admin_port].contains(&request.server_port)
            || [manifest.server_port, manifest.tx_admin_port].contains(&request.tx_admin_port)
        {
            return Err(
                "Unsupported package or destination ports collide with package ports.".into(),
            );
        }
        plan.database = database::prepare(request, manifest.database.as_ref())?;
        if manifest.database.is_some() && request.database.is_none() {
            plan.excluded.push(CloneExclusion {
                path: "database.sql".into(),
                reason: "Database copy was not selected".into(),
            });
        }
        for entry in manifest.files {
            let path = relative(&entry.path)?;
            if !entry.path.starts_with("server-data/")
                || entry.path == "server-data/"
                || entry.size > MAX_FILE
                || entry.sha256.len() != 64
            {
                return Err("Invalid clone package entry.".into());
            }
            let payload = read_bounded(&root.join(&path), MAX_FILE)?;
            if payload.len() as u64 != entry.size || digest(&payload) != entry.sha256 {
                return Err("Package file does not match its reviewed manifest.".into());
            }
            add_file(&mut plan, root.join(path), entry.path, &mut seen)?;
        }
    } else {
        plan.database = database::prepare(request, None)?;
        let choices = list_choices(&request.source_path)?;
        if request.resources.len() + request.configs.len() > 5000 {
            return Err("Too many clone selections.".into());
        }
        for resource in &request.resources {
            if !choices.resources.contains(resource) {
                return Err("A selected resource is no longer available.".into());
            }
            let folder = root.join("resources").join(relative(resource)?);
            walk(&mut plan, &folder, &root, &mut seen, 0)?;
        }
        for config in &request.configs {
            if !choices.configs.contains(config) {
                return Err("A selected configuration is no longer available.".into());
            }
            add_file(
                &mut plan,
                root.join(relative(config)?),
                format!("server-data/{config}"),
                &mut seen,
            )?;
        }
    }
    let mut cfg = if let Some(index) = plan.files.iter().position(|item| {
        item.file
            .path
            .eq_ignore_ascii_case("server-data/server.cfg")
    }) {
        let item = plan.files.remove(index);
        sanitize_cfg(
            std::str::from_utf8(&read_bounded(item.source.as_ref().unwrap(), MAX_TEXT)?)
                .map_err(|_| "server.cfg must be UTF-8.")?,
        )
    } else {
        b"# Private clone. Configure license key and database separately before starting.\n"
            .to_vec()
    };
    cfg.extend_from_slice(
        format!(
            "\nendpoint_add_tcp \"0.0.0.0:{}\"\nendpoint_add_udp \"0.0.0.0:{}\"\n",
            request.server_port, request.server_port
        )
        .as_bytes(),
    );
    plan.files.push(PlannedFile {
        source: None,
        file: PackageFile {
            path: "server-data/server.cfg".into(),
            size: cfg.len() as u64,
            sha256: digest(&cfg),
        },
        original_sha256: String::new(),
        generated: Some(cfg),
    });
    plan.files.sort_by(|a, b| a.file.path.cmp(&b.file.path));
    plan.excluded.sort_by(|a, b| a.path.cmp(&b.path));
    let total = plan_bytes(&plan);
    if total > MAX_TOTAL {
        return Err("Clone exceeds the 20 GiB package limit.".into());
    }
    Ok(plan)
}

fn plan_bytes(plan: &Plan) -> u64 {
    plan.files.iter().map(|item| item.file.size).sum::<u64>()
        + plan.database.as_ref().map_or(0, |db| db.package.size_bytes)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let _handles = pin_directories(path.parent().ok_or("Missing output parent.")?)?;
    check_no_links(path.parent().ok_or("Missing output parent.")?)?;
    #[cfg(not(target_os = "linux"))]
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_error)?;
    #[cfg(target_os = "linux")]
    let mut file = linux_fs::create(path)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)
}

fn execute_plan(
    request: &CloneRequest,
    plan: &Plan,
    before_promote: impl FnOnce() -> Result<(), String>,
) -> Result<CloneResult, String> {
    let parent = plan
        .destination
        .parent()
        .ok_or("Missing destination parent.")?;
    let _handles = pin_directories(parent)?;
    check_no_links(parent)?;
    require_missing(&plan.destination)?;
    check_disk(parent, plan_bytes(plan))?;
    let stage = parent.join(format!(
        ".fxclone-stage-{}",
        super::backup_manager::storage::secure_token()?
    ));
    create_directory(&stage)?;
    let mut written = BTreeMap::new();
    let result = (|| {
        for folder in ["server-data", "txData", "artifacts"] {
            create_directory(&stage.join(folder))?;
        }
        for item in &plan.files {
            let output = stage.join(relative(&item.file.path)?);
            create_owned_parents(&stage, output.parent().ok_or("Missing output parent.")?)?;
            let data = if let Some(data) = &item.generated {
                data.clone()
            } else {
                let bytes = read_bounded(
                    item.source.as_ref().ok_or("Missing source file.")?,
                    MAX_FILE,
                )?;
                if digest(&bytes) != item.original_sha256 {
                    return Err("Source changed during copy. No destination was created.".into());
                }
                transformed(&item.file.path, &bytes)?
                    .ok_or("Source sanitization changed during copy.")?
            };
            if data.len() as u64 != item.file.size || digest(&data) != item.file.sha256 {
                return Err("Staged content does not match the preview.".into());
            }
            write_new(&output, &data)?;
            written.insert(output, digest(&data));
        }
        if let Some(database) = &plan.database {
            let bytes = read_bounded(&database.source, 32 * 1024 * 1024)?;
            if digest(&bytes) != database.package.sha256 {
                return Err("Database dump changed during staging.".into());
            }
            write_new(&stage.join("database.sql"), &bytes)?;
            written.insert(stage.join("database.sql"), digest(&bytes));
        }
        let manifest = PackageManifest {
            schema_version: 1,
            usage: "private-user-copy".into(),
            server_port: request.server_port,
            tx_admin_port: request.tx_admin_port,
            files: plan.files.iter().map(|item| item.file.clone()).collect(),
            database: plan.database.as_ref().map(|db| db.package.clone()),
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(io_error)?;
        write_new(&stage.join(MANIFEST), &manifest_bytes)?;
        written.insert(stage.join(MANIFEST), digest(&manifest_bytes));
        let current = build_plan(request)?;
        if current
            .files
            .iter()
            .map(|item| &item.file)
            .collect::<Vec<_>>()
            != plan.files.iter().map(|item| &item.file).collect::<Vec<_>>()
            || current.excluded != plan.excluded
            || current.database != plan.database
        {
            return Err("Source changed during copy. Review a new preview.".into());
        }
        before_promote()?;
        check_no_links(parent)?;
        check_no_links(&stage)?;
        inspect_stage(&stage, &written)?;
        require_missing(&plan.destination)?;
        promote(&stage, &plan.destination)?;
        #[cfg(target_os = "linux")]
        {
            let promoted = written
                .iter()
                .map(|(path, hash)| {
                    Ok((
                        plan.destination
                            .join(path.strip_prefix(&stage).map_err(io_error)?),
                        hash.clone(),
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, String>>()?;
            inspect_stage(&plan.destination, &promoted).map_err(|error| format!("Promoted clone changed during verification: {error}. Destination was preserved for review."))?;
        }
        Ok(CloneResult {
            destination_path: display(&plan.destination),
            server_data_path: display(&plan.destination.join("server-data")),
            tx_data_path: display(&plan.destination.join("txData")),
            artifact_path: display(&plan.destination.join("artifacts")),
            file_count: plan.files.len(),
            database: None,
        })
    })();
    if let Err(error) = &result {
        if let Err(cleanup) = remove_stage(parent, &stage, &written) {
            return Err(format!(
                "{error} Staging was preserved at {}: {cleanup}",
                display(&stage)
            ));
        }
    }
    result
}

fn create_owned_parents(stage: &Path, parent: &Path) -> Result<(), String> {
    let suffix = parent.strip_prefix(stage).map_err(io_error)?;
    let mut current = stage.to_path_buf();
    for part in suffix.components() {
        if !matches!(part, Component::Normal(_)) {
            return Err("Unsafe staged path.".into());
        }
        current.push(part);
        if !current.try_exists().map_err(io_error)? {
            create_directory(&current)?;
        }
        check_no_links(&current)?;
        if !current.is_dir() {
            return Err("Staged path collision.".into());
        }
    }
    Ok(())
}

fn remove_stage(
    parent: &Path,
    stage: &Path,
    written: &BTreeMap<PathBuf, String>,
) -> Result<(), String> {
    let _handles = pin_directories(parent)?;
    check_no_links(parent)?;
    if stage.parent() != Some(parent)
        || !stage
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".fxclone-stage-"))
    {
        return Err("Refusing to clean an unowned directory.".into());
    }
    let directories = inspect_stage(stage, written)?;
    for (path, expected) in written {
        if !path.starts_with(stage) {
            return Err("Unowned staging file; cleanup refused.".into());
        }
        #[cfg(target_os = "linux")]
        linux_fs::remove_file(path, Some(expected))?;
        #[cfg(not(target_os = "linux"))]
        super::backup_manager::storage::remove_snapshot(path, expected)?;
    }
    for directory in directories.into_iter().rev() {
        check_no_links(&directory)?;
        // Non-recursive removal preserves anything added since the inventory check.
        #[cfg(target_os = "linux")]
        linux_fs::remove_empty_directory(&directory)?;
        #[cfg(not(target_os = "linux"))]
        fs::remove_dir(directory).map_err(io_error)?;
    }
    Ok(())
}

fn inspect_stage(
    stage: &Path,
    written: &BTreeMap<PathBuf, String>,
) -> Result<Vec<PathBuf>, String> {
    fn inspect_tree(
        path: &Path,
        written: &BTreeMap<PathBuf, String>,
        directories: &mut Vec<PathBuf>,
        found: &mut usize,
    ) -> Result<(), String> {
        let _handles = pin_directories(path)?;
        check_no_links(path)?;
        directories.push(path.to_path_buf());
        for entry in fs::read_dir(path).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            check_no_links(&entry.path())?;
            if entry.file_type().map_err(io_error)?.is_dir() {
                inspect_tree(&entry.path(), written, directories, found)?;
            } else {
                let path = entry.path();
                let expected = written
                    .get(&path)
                    .ok_or("Untracked staging content; cleanup refused.")?;
                if digest(&read_bounded(&path, MAX_FILE)?) != *expected {
                    return Err("Staging content changed; cleanup refused.".into());
                }
                *found += 1;
            }
        }
        Ok(())
    }
    let mut directories = Vec::new();
    let mut found = 0;
    inspect_tree(stage, written, &mut directories, &mut found)?;
    if found != written.len() {
        return Err("Staging files are missing; operation refused.".into());
    }
    Ok(directories)
}

#[cfg(windows)]
fn check_disk(path: &Path, bytes: u64) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0;
    if unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    if available < bytes.saturating_add(64 * 1024 * 1024) {
        return Err("Not enough disk space to stage the clone with a 64 MiB reserve.".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn check_disk(path: &Path, bytes: u64) -> Result<(), String> {
    if linux_fs::available_bytes(path)? < bytes.saturating_add(64 * 1024 * 1024) {
        return Err("Not enough disk space to stage the clone with a 64 MiB reserve.".into());
    }
    Ok(())
}

#[cfg(windows)]
fn promote(stage: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let from: Vec<u16> = stage.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // No REPLACE_EXISTING or COPY_ALLOWED: same-volume promotion must not overwrite.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn promote(stage: &Path, target: &Path) -> Result<(), String> {
    linux_fs::rename(stage, target, false)
}

fn create_directory(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return linux_fs::mkdir(path);
    #[cfg(not(target_os = "linux"))]
    fs::create_dir(path).map_err(io_error)
}

// Descriptor-relative operations are shared only by the desktop workflows in this scope.
// A Unix directory descriptor does not pin its pathname as Windows sharing flags do.
#[cfg(target_os = "linux")]
pub(crate) mod linux_fs {
    use std::{
        ffi::{CString, OsStr},
        fs::{self, File},
        io::{Read, Write},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::MetadataExt},
        },
        path::{Component, Path, PathBuf},
    };

    fn error() -> String {
        std::io::Error::last_os_error().to_string()
    }
    fn name(value: &OsStr) -> Result<CString, String> {
        CString::new(value.as_bytes()).map_err(|_| "NUL in filesystem path.".into())
    }
    fn opened(fd: i32) -> Result<File, String> {
        if fd < 0 {
            return Err(error());
        }
        // The successful syscall transfers this new descriptor to File.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn open_at(parent: &File, child: &OsStr, flags: i32) -> Result<File, String> {
        let child = name(child)?;
        opened(unsafe {
            libc::openat(
                parent.as_raw_fd(),
                child.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                0o600,
            )
        })
    }
    pub(crate) fn directory(path: &Path) -> Result<File, String> {
        if !path.is_absolute() {
            return Err("An absolute local path is required.".into());
        }
        let mut file = opened(unsafe {
            libc::open(
                c"/".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        })?;
        for part in path.components() {
            match part {
                Component::RootDir => (),
                Component::Normal(child) => {
                    file = open_at(&file, child, libc::O_RDONLY | libc::O_DIRECTORY)?
                }
                _ => return Err("Path traversal is not allowed.".into()),
            }
        }
        Ok(file)
    }
    fn parent(path: &Path) -> Result<(File, &OsStr), String> {
        Ok((
            directory(path.parent().ok_or("Missing parent directory.")?)?,
            path.file_name().ok_or("Missing file name.")?,
        ))
    }
    pub(crate) fn read(path: &Path) -> Result<File, String> {
        let (parent, child) = parent(path)?;
        let file = open_at(&parent, child, libc::O_RDONLY)?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err("Only regular, non-hardlinked files are allowed.".into());
        }
        // Advisory locking coordinates app writers; callers still validate hashes/lengths.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } != 0 {
            return Err(error());
        }
        Ok(file)
    }
    pub(crate) fn unchanged(file: &File, before: &fs::Metadata) -> Result<(), String> {
        let after = file.metadata().map_err(|e| e.to_string())?;
        let stamp = |meta: &fs::Metadata| {
            (
                meta.dev(),
                meta.ino(),
                meta.len(),
                meta.mtime(),
                meta.mtime_nsec(),
                meta.ctime(),
                meta.ctime_nsec(),
                meta.nlink(),
            )
        };
        if stamp(before) != stamp(&after) {
            return Err("File changed while being read.".into());
        }
        Ok(())
    }
    pub(crate) fn create(path: &Path) -> Result<File, String> {
        let (parent, child) = parent(path)?;
        open_at(&parent, child, libc::O_RDWR | libc::O_CREAT | libc::O_EXCL)
    }
    pub(crate) fn mkdir(path: &Path) -> Result<(), String> {
        let (parent, child) = parent(path)?;
        let child = name(child)?;
        if unsafe { libc::mkdirat(parent.as_raw_fd(), child.as_ptr(), 0o700) } != 0 {
            return Err(error());
        }
        Ok(())
    }
    pub(crate) fn mkdir_all(path: &Path) -> Result<(), String> {
        if directory(path).is_ok() {
            return Ok(());
        }
        mkdir_all(
            path.parent()
                .filter(|p| *p != path)
                .ok_or("Invalid directory path.")?,
        )?;
        match mkdir(path) {
            Ok(()) => Ok(()),
            Err(_) => directory(path).map(|_| ()),
        }
    }
    pub(crate) fn available_bytes(path: &Path) -> Result<u64, String> {
        let directory = directory(path)?;
        let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::fstatvfs(directory.as_raw_fd(), stats.as_mut_ptr()) } != 0 {
            return Err(error());
        }
        let stats = unsafe { stats.assume_init() };
        (u128::from(stats.f_bavail) * u128::from(stats.f_frsize))
            .try_into()
            .map_err(|_| "Filesystem capacity overflow.".into())
    }
    pub(crate) fn rename(from: &Path, to: &Path, replace: bool) -> Result<(), String> {
        let (source, from) = parent(from)?;
        let (destination, to) = parent(to)?;
        let file = open_at(&source, from, libc::O_PATH)?;
        let original = file.metadata().map_err(|e| e.to_string())?;
        if !original.is_file() && !original.is_dir() {
            return Err("Only regular files and directories may be promoted.".into());
        }
        let (from_c, to_c) = (name(from)?, name(to)?);
        if unsafe {
            libc::renameat2(
                source.as_raw_fd(),
                from_c.as_ptr(),
                destination.as_raw_fd(),
                to_c.as_ptr(),
                if replace { 0 } else { libc::RENAME_NOREPLACE },
            )
        } != 0
        {
            return Err(error());
        }
        let moved = open_at(&destination, to, libc::O_PATH)?
            .metadata()
            .map_err(|e| e.to_string())?;
        if (original.dev(), original.ino()) != (moved.dev(), moved.ino()) {
            let restored = unsafe {
                libc::renameat2(
                    destination.as_raw_fd(),
                    to_c.as_ptr(),
                    source.as_raw_fd(),
                    from_c.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            } == 0;
            return Err(format!(
                "Promotion source changed; {}.",
                if restored {
                    "moved entry restored"
                } else {
                    "moved entry preserved at the destination"
                }
            ));
        }
        Ok(())
    }
    pub(crate) fn token() -> Result<String, String> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| format!("Secure randomness unavailable: {e}"))?;
        Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }
    pub(crate) fn copy(from: &Path, to: &Path) -> Result<(), String> {
        let mut input = read(from)?;
        let temp = to.with_file_name(format!(".fxsi-copy-{}", token()?));
        let mut output = create(&temp)?;
        let metadata = input.metadata().map_err(|e| e.to_string())?;
        let size = metadata.len();
        if size > 512 * 1024 * 1024 {
            return Err("Copy exceeds the 512 MiB file limit.".into());
        }
        let copied = std::io::copy(&mut Read::by_ref(&mut input).take(size + 1), &mut output)
            .map_err(|e| e.to_string())?;
        if copied != size {
            return Err("Source changed during copy; temporary file preserved.".into());
        }
        unchanged(&input, &metadata)?;
        output
            .flush()
            .and_then(|_| output.sync_all())
            .map_err(|e| e.to_string())?;
        rename(&temp, to, true)
    }
    fn unlink(parent: &File, child: &OsStr, directory: bool) -> Result<(), String> {
        let child = name(child)?;
        if unsafe {
            libc::unlinkat(
                parent.as_raw_fd(),
                child.as_ptr(),
                if directory { libc::AT_REMOVEDIR } else { 0 },
            )
        } != 0
        {
            return Err(error());
        }
        Ok(())
    }
    pub(crate) fn remove_empty_directory(path: &Path) -> Result<(), String> {
        let (parent, child) = parent(path)?;
        open_at(&parent, child, libc::O_RDONLY | libc::O_DIRECTORY)?;
        unlink(&parent, child, true)
    }
    pub(crate) fn remove_file(path: &Path, expected: Option<&str>) -> Result<(), String> {
        remove_file_identity(path, expected, None)
    }
    pub(crate) fn remove_file_identity(
        path: &Path,
        expected: Option<&str>,
        identity: Option<(u64, u64)>,
    ) -> Result<(), String> {
        let (parent, child) = parent(path)?;
        // Verify after moving into a private directory. Other users with write access to
        // the original parent cannot substitute the name between verification and unlink.
        let quarantine = format!(".fxsi-remove-{}", token()?);
        let child_c = name(child)?;
        let quarantine_c = name(OsStr::new(&quarantine))?;
        if unsafe { libc::mkdirat(parent.as_raw_fd(), quarantine_c.as_ptr(), 0o700) } != 0 {
            return Err(error());
        }
        let protected = open_at(
            &parent,
            OsStr::new(&quarantine),
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        let protected_meta = protected.metadata().map_err(|e| e.to_string())?;
        if protected_meta.uid() != unsafe { libc::geteuid() } || protected_meta.mode() & 0o077 != 0
        {
            return Err("Cleanup directory is not private; no file was removed.".into());
        }
        if unsafe {
            libc::renameat2(
                parent.as_raw_fd(),
                child_c.as_ptr(),
                protected.as_raw_fd(),
                c"entry".as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } != 0
        {
            let failure = error();
            let _ = unlink(&parent, OsStr::new(&quarantine), true);
            return Err(failure);
        }
        let result = (|| {
            let mut file = open_at(&protected, OsStr::new("entry"), libc::O_RDONLY)?;
            let meta = file.metadata().map_err(|e| e.to_string())?;
            if !meta.is_file() || meta.nlink() != 1 {
                return Err("Linked or special file preserved.".into());
            }
            if identity.is_some_and(|id| id != (meta.dev(), meta.ino())) {
                return Err("Replaced file preserved.".into());
            }
            if let Some(expected) = expected {
                use sha2::{Digest, Sha256};
                let mut hash = Sha256::new();
                let mut buffer = [0u8; 65536];
                loop {
                    let size = file.read(&mut buffer).map_err(|e| e.to_string())?;
                    if size == 0 {
                        break;
                    }
                    hash.update(&buffer[..size]);
                }
                if format!("{:x}", hash.finalize()) != expected {
                    return Err("Changed file preserved.".into());
                }
            }
            unlink(&protected, OsStr::new("entry"), false)
        })();
        if let Err(failure) = result {
            let restored = unsafe {
                libc::renameat2(
                    protected.as_raw_fd(),
                    c"entry".as_ptr(),
                    parent.as_raw_fd(),
                    child_c.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            } == 0;
            if restored {
                let _ = unlink(&parent, OsStr::new(&quarantine), true);
            }
            return Err(format!(
                "{failure} {}",
                if restored {
                    "Original name restored.".into()
                } else {
                    format!("Recovery file retained as {quarantine}/entry.")
                }
            ));
        }
        unlink(&parent, OsStr::new(&quarantine), true)
    }
    pub(crate) fn remove_tree(root: &Path, target: &Path) -> Result<(), String> {
        let relative = target
            .strip_prefix(root)
            .map_err(|_| "Cleanup is outside owned storage.")?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err("Refusing broad or traversing cleanup.".into());
        }
        let root = directory(root)?;
        let mut parent = root;
        let parts: Vec<_> = relative.components().collect();
        for part in &parts[..parts.len() - 1] {
            parent = open_at(
                &parent,
                part.as_os_str(),
                libc::O_RDONLY | libc::O_DIRECTORY,
            )?;
        }
        fn remove_at(parent: &File, child: &OsStr, depth: usize) -> Result<(), String> {
            if depth > 64 {
                return Err("Cleanup tree is too deep.".into());
            }
            let dir = open_at(parent, child, libc::O_RDONLY | libc::O_DIRECTORY)?;
            let base = PathBuf::from(format!("/proc/self/fd/{}", dir.as_raw_fd()));
            for entry in fs::read_dir(base).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let kind = entry.file_type().map_err(|e| e.to_string())?;
                if kind.is_dir() {
                    remove_at(&dir, &entry.file_name(), depth + 1)?;
                } else if kind.is_file() {
                    unlink(&dir, &entry.file_name(), false)?;
                } else {
                    return Err("Cleanup refused a symbolic link or special file.".into());
                }
            }
            unlink(parent, child, true)
        }
        remove_at(&parent, parts.last().unwrap().as_os_str(), 0)
    }
}

#[cfg(test)]
#[path = "workspace_clone_tests.rs"]
mod tests;
