use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Manager;

const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;
const MAX_TOTAL: u64 = 2 * 1024 * 1024 * 1024;
const MAX_FILE: u64 = 256 * 1024 * 1024;
const MAX_FILES: usize = 50_000;
const PREVIEW_LIFETIME: u64 = 30 * 60;
const MAX_SNAPSHOTS: usize = 20;
const MAX_SNAPSHOT_BYTES: u64 = 10 * 1024 * 1024 * 1024;
static OPERATION: Mutex<()> = Mutex::new(());
static PREVIEWS: OnceLock<Mutex<HashMap<String, Prepared>>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTarget {
    workspace_id: String,
    tx_data_path: String,
    profile: String,
    resource_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRequest {
    target: ResourceTarget,
    branch: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    target: ResourceTarget,
    preview_id: String,
    protected_paths: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Fingerprint {
    size: u64,
    sha256: String,
}

type Inventory = BTreeMap<String, Fingerprint>;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    path: String,
    kind: String,
    old_size: Option<u64>,
    new_size: Option<u64>,
    preserve: bool,
    can_preserve: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePreview {
    id: String,
    resource_name: String,
    repository: String,
    branch: String,
    archive_sha256: String,
    archive_bytes: u64,
    changes: Vec<FileChange>,
    created_at: u64,
}

struct Prepared {
    target: ResourceTarget,
    resource: PathBuf,
    directory: PathBuf,
    local: Inventory,
    remote: Inventory,
    preview: ResourcePreview,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSnapshot {
    id: String,
    resource_name: String,
    created_at: u64,
    file_count: usize,
    size_bytes: u64,
    reason: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotRecord {
    snapshot: ResourceSnapshot,
    resource_path: PathBuf,
    workspace_id: String,
    files: Inventory,
}

#[tauri::command]
pub async fn preview_resource_update(
    app: tauri::AppHandle,
    request: PreviewRequest,
) -> Result<ResourcePreview, String> {
    let storage = storage_root(&app)?;
    super::run_blocking(move || {
        let _operation = OPERATION
            .try_lock()
            .map_err(|_| "Another resource operation is in progress.")?;
        prepare(&storage, request)
    })
    .await
}

#[tauri::command]
pub async fn apply_resource_update(
    app: tauri::AppHandle,
    request: ApplyRequest,
    manager: tauri::State<'_, super::fxserver::FxserverManager>,
) -> Result<ResourceSnapshot, String> {
    let storage = storage_root(&app)?;
    let manager = manager.inner().clone();
    super::run_blocking(move || {
        let _operation = OPERATION
            .try_lock()
            .map_err(|_| "Another resource operation is in progress.")?;
        let mut previews = previews()
            .lock()
            .map_err(|_| "Resource preview lock failed.")?;
        let prepared = previews
            .get(&request.preview_id)
            .ok_or("Preview expired. Preview the update again.")?;
        let result = manager.with_stopped_server(|| apply_prepared(&storage, prepared, &request));
        if result.is_ok() {
            if let Some(prepared) = previews.remove(&request.preview_id) {
                let _ = remove_owned(&storage, &prepared.directory);
            }
        }
        result
    })
    .await
}

#[tauri::command]
pub async fn discard_resource_preview(
    app: tauri::AppHandle,
    preview_id: String,
) -> Result<(), String> {
    let storage = storage_root(&app)?;
    super::run_blocking(move || {
        if let Some(prepared) = previews()
            .lock()
            .map_err(|_| "Resource preview lock failed.")?
            .remove(&preview_id)
        {
            remove_owned(&storage, &prepared.directory)?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn list_resource_snapshots(
    app: tauri::AppHandle,
    target: ResourceTarget,
) -> Result<Vec<ResourceSnapshot>, String> {
    let storage = storage_root(&app)?;
    super::run_blocking(move || {
        let resource = resolve_resource(&target)?;
        let folder = snapshot_folder(&storage, &target, &resource);
        if !folder.exists() {
            return Ok(Vec::new());
        }
        check_no_links(&folder)?;
        let mut snapshots = Vec::new();
        for entry in fs::read_dir(folder).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            check_no_links(&entry.path())?;
            if !entry.file_type().map_err(io_error)?.is_dir() {
                continue;
            }
            let record = read_snapshot(&entry.path())?;
            if record.resource_path == resource && record.workspace_id == target.workspace_id {
                snapshots.push(record.snapshot);
            }
        }
        snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        Ok(snapshots)
    })
    .await
}

#[tauri::command]
pub async fn rollback_resource_update(
    app: tauri::AppHandle,
    target: ResourceTarget,
    snapshot_id: String,
    manager: tauri::State<'_, super::fxserver::FxserverManager>,
) -> Result<ResourceSnapshot, String> {
    let storage = storage_root(&app)?;
    let manager = manager.inner().clone();
    super::run_blocking(move || {
        let _operation = OPERATION
            .try_lock()
            .map_err(|_| "Another resource operation is in progress.")?;
        manager.with_stopped_server(|| rollback(&storage, &target, &snapshot_id))
    })
    .await
}

#[tauri::command]
pub async fn delete_resource_snapshot(
    app: tauri::AppHandle,
    target: ResourceTarget,
    snapshot_id: String,
) -> Result<(), String> {
    let storage = storage_root(&app)?;
    super::run_blocking(move || {
        let _operation = OPERATION
            .try_lock()
            .map_err(|_| "Another resource operation is in progress.")?;
        validate_id(&snapshot_id)?;
        let resource = resolve_resource(&target)?;
        let folder = snapshot_folder(&storage, &target, &resource).join(snapshot_id);
        let record = read_snapshot(&folder)?;
        if record.resource_path != resource || record.workspace_id != target.workspace_id {
            return Err("Snapshot belongs to another workspace or resource.".into());
        }
        remove_owned(&storage, &folder)
    })
    .await
}

fn previews() -> &'static Mutex<HashMap<String, Prepared>> {
    PREVIEWS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn storage_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(io_error)?
        .join("resource-updates");
    create_checked_directory(&root)?;
    root.canonicalize().map_err(io_error)
}

fn prepare(storage: &Path, request: PreviewRequest) -> Result<ResourcePreview, String> {
    let resource = resolve_resource(&request.target)?;
    let (owner, repo) = repository(&resource)?;
    validate_branch(&request.branch)?;
    cleanup_expired_previews(storage, SystemTime::now())?;
    let preview_root = storage.join("previews");
    if preview_root.exists() && fs::read_dir(&preview_root).map_err(io_error)?.count() >= 8 {
        return Err("Preview storage is full. Close an existing preview or wait for abandoned previews to expire (30 minutes).".into());
    }
    ensure_free_space(storage, MAX_ARCHIVE + MAX_TOTAL)?;
    let id = unique_id();
    let directory = storage.join("previews").join(&id);
    create_checked_directory(&directory)?;
    let result = (|| {
        let archive = directory.join("download.zip");
        let archive_bytes = download(&owner, &repo, &request.branch, &archive)?;
        let archive_sha256 = fingerprint(&archive)?.sha256;
        let extracted = directory.join("remote");
        extract_archive(&archive, &extracted)?;
        require_manifest(&extracted)?;
        let local = inventory(&resource)?;
        let remote = inventory(&extracted)?;
        let changes = file_changes(&local, &remote);
        let preview = ResourcePreview {
            id: id.clone(),
            resource_name: file_name(&resource),
            repository: format!("https://github.com/{owner}/{repo}"),
            branch: request.branch,
            archive_sha256,
            archive_bytes,
            changes,
            created_at: now(),
        };
        let mut previews = previews()
            .lock()
            .map_err(|_| "Resource preview lock failed.")?;
        previews.retain(|_, old| {
            let keep = now().saturating_sub(old.preview.created_at) < PREVIEW_LIFETIME;
            if !keep {
                let _ = remove_owned(storage, &old.directory);
            }
            keep
        });
        if previews.len() >= 8 {
            return Err("Close an existing update preview before preparing another.".to_string());
        }
        previews.insert(
            id,
            Prepared {
                target: request.target,
                resource,
                directory: directory.clone(),
                local,
                remote,
                preview: preview.clone(),
            },
        );
        log::info!(
            "Prepared resource update preview for {} ({} changed files)",
            preview.resource_name,
            preview.changes.len()
        );
        Ok(preview)
    })();
    if result.is_err() {
        let _ = remove_owned(storage, &directory);
    }
    result
}

fn apply_prepared(
    storage: &Path,
    prepared: &Prepared,
    request: &ApplyRequest,
) -> Result<ResourceSnapshot, String> {
    let resource = resolve_resource(&request.target)?;
    if resource != prepared.resource || request.target.workspace_id != prepared.target.workspace_id
    {
        return Err("The workspace or resource changed. Preview the update again.".into());
    }
    if now().saturating_sub(prepared.preview.created_at) >= PREVIEW_LIFETIME {
        return Err("Preview expired. Preview the update again.".into());
    }
    if inventory(&resource)? != prepared.local {
        return Err("Local files changed after the preview. Preview again to avoid overwriting new changes.".into());
    }
    let remote = prepared.directory.join("remote");
    if inventory(&remote)? != prepared.remote {
        return Err("Staged download changed. Preview the update again.".into());
    }
    let protected: BTreeSet<_> = request.protected_paths.iter().cloned().collect();
    for path in &protected {
        safe_relative(path)?;
        if !prepared.local.contains_key(path) {
            return Err(format!("Cannot preserve missing local file: {path}"));
        }
    }
    let snapshot = create_snapshot(
        storage,
        &request.target,
        &resource,
        "Before update",
        &prepared.local,
    )?;
    replace_resource(
        &resource,
        &staging_parent(&request.target, &resource)?,
        &remote,
        &prepared.remote,
        &protected,
        &prepared.local,
    )?;
    log::info!(
        "Updated resource {} with snapshot {}",
        file_name(&resource),
        snapshot.id
    );
    Ok(snapshot)
}

fn rollback(
    storage: &Path,
    target: &ResourceTarget,
    snapshot_id: &str,
) -> Result<ResourceSnapshot, String> {
    validate_id(snapshot_id)?;
    let resource = resolve_resource(target)?;
    let folder = snapshot_folder(storage, target, &resource).join(snapshot_id);
    let record = read_snapshot(&folder)?;
    if record.resource_path != resource || record.workspace_id != target.workspace_id {
        return Err("Snapshot belongs to another workspace or resource.".into());
    }
    let source = folder.join("files");
    if inventory(&source)? != record.files {
        return Err("Snapshot integrity check failed. No files were changed.".into());
    }
    require_manifest(&source)?;
    let local = inventory(&resource)?;
    let safety_snapshot = create_snapshot(storage, target, &resource, "Before rollback", &local)?;
    replace_resource(
        &resource,
        &staging_parent(target, &resource)?,
        &source,
        &record.files,
        &BTreeSet::new(),
        &local,
    )?;
    log::info!(
        "Rolled back resource {} to snapshot {}",
        file_name(&resource),
        snapshot_id
    );
    Ok(safety_snapshot)
}

fn create_snapshot(
    storage: &Path,
    target: &ResourceTarget,
    resource: &Path,
    reason: &str,
    files: &Inventory,
) -> Result<ResourceSnapshot, String> {
    let existing = snapshot_folder(storage, target, resource);
    let mut count = 0;
    let mut bytes: u64 = files.values().map(|file| file.size).sum();
    if existing.exists() {
        check_no_links(&existing)?;
        for entry in fs::read_dir(&existing).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            if !entry.file_type().map_err(io_error)?.is_dir() {
                continue;
            }
            let record = read_snapshot(&entry.path())?;
            count += 1;
            bytes = bytes
                .checked_add(record.files.values().map(|file| file.size).sum())
                .ok_or("Snapshot size overflow.")?;
        }
    }
    if count >= MAX_SNAPSHOTS || bytes > MAX_SNAPSHOT_BYTES {
        return Err("Snapshot storage limit reached (20 snapshots or 10 GiB per resource). Remove an older snapshot from Snapshots before continuing.".into());
    }
    ensure_free_space(storage, files.values().map(|file| file.size).sum())?;
    let id = unique_id();
    let folder = snapshot_folder(storage, target, resource).join(&id);
    create_checked_directory(&folder)?;
    let result = (|| {
        copy_inventory(resource, &folder.join("files"), files)?;
        if inventory(&folder.join("files"))? != *files {
            return Err("Snapshot verification failed. No files were changed.".into());
        }
        let snapshot = ResourceSnapshot {
            id,
            resource_name: file_name(resource),
            created_at: now(),
            file_count: files.len(),
            size_bytes: files.values().map(|f| f.size).sum(),
            reason: reason.into(),
        };
        let record = SnapshotRecord {
            snapshot: snapshot.clone(),
            resource_path: resource.into(),
            workspace_id: target.workspace_id.clone(),
            files: files.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&record).map_err(io_error)?;
        let mut output = File::create(folder.join("snapshot.json")).map_err(io_error)?;
        output.write_all(&bytes).map_err(io_error)?;
        output.sync_all().map_err(io_error)?;
        Ok(snapshot)
    })();
    if result.is_err() {
        let _ = remove_owned(storage, &folder);
    }
    result
}

fn replace_resource(
    resource: &Path,
    parent: &Path,
    source: &Path,
    remote: &Inventory,
    protected: &BTreeSet<String>,
    local: &Inventory,
) -> Result<(), String> {
    check_no_links(parent)?;
    ensure_free_space(
        parent,
        remote.values().map(|file| file.size).sum::<u64>()
            + local.values().map(|file| file.size).sum::<u64>(),
    )?;
    let transaction = parent.join(format!(".fxserver-update-{}", unique_id()));
    fs::create_dir(&transaction).map_err(io_error)?;
    check_no_links(&transaction)?;
    let staged = transaction.join("new");
    let original = transaction.join("original");
    let result = (|| {
        let mut expected = remote.clone();
        for path in protected {
            expected.insert(path.clone(), local[path].clone());
        }
        copy_inventory(source, &staged, remote)?;
        for path in protected {
            let destination = staged.join(safe_relative(path)?);
            if let Some(parent) = destination.parent() {
                create_checked_directory(parent)?;
            }
            check_no_links(&resource.join(path))?;
            if fs::symlink_metadata(&destination).is_ok() {
                check_no_links(&destination)?;
            }
            if let Some(parent) = destination.parent() {
                check_no_links(parent)?;
            }
            fs::copy(resource.join(path), destination).map_err(io_error)?;
        }
        if inventory(&staged)? != expected {
            return Err("Staged files failed verification. Original resource is unchanged.".into());
        }
        if inventory(resource)? != *local {
            return Err(
                "Local files changed while staging the update. No files were replaced.".into(),
            );
        }
        check_no_links(resource)?;
        check_no_links(&staged)?;
        fs::rename(resource, &original).map_err(|e| {
            format!(
                "Could not move resource for replacement. Stop the resource/server and retry: {e}"
            )
        })?;
        if !inventory(&original).is_ok_and(|files| files == *local) {
            fs::rename(&original, resource).map_err(|error| format!("Resource changed during replacement and could not be restored ({error}). Original files remain at {}.", original.display()))?;
            return Err(
                "Resource changed during replacement. Original files were restored; preview again."
                    .into(),
            );
        }
        if let Err(error) = fs::rename(&staged, resource) {
            if let Err(restore) = fs::rename(&original, resource) {
                return Err(format!("Replacement failed ({error}); restoration failed ({restore}). Original files remain at {}. Restore them before starting the server.", original.display()));
            }
            return Err(format!(
                "Replacement failed; original files were restored: {error}"
            ));
        }
        Ok(())
    })();
    // Never remove the only original if a failed rename prevented restoration.
    if resource.exists() && (!original.exists() || result.is_ok()) {
        if let Err(error) = remove_owned(parent, &transaction) {
            log::warn!("Resource staging cleanup failed: {error}");
        }
    } else if !original.exists() {
        let _ = remove_owned(parent, &transaction);
    }
    result
}

fn resource_root(target: &ResourceTarget) -> Result<PathBuf, String> {
    if target.workspace_id.is_empty() || target.workspace_id.len() > 128 {
        return Err("Invalid workspace ID.".into());
    }
    let profile = safe_relative(&target.profile)?;
    if profile.components().count() != 1 {
        return Err("Invalid txData profile name.".into());
    }
    let config_path = Path::new(&target.tx_data_path)
        .join(profile)
        .join("config.json");
    check_no_links(&config_path)?;
    let config: serde_json::Value =
        serde_json::from_str(&super::config_history::read_bounded_config(&config_path)?)
            .map_err(io_error)?;
    let data = config
        .pointer("/server/dataPath")
        .or_else(|| config.get("dataPath"))
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .ok_or("Profile is missing server.dataPath.")?;
    if !Path::new(data).is_absolute() {
        return Err("Profile dataPath must be an absolute directory path.".into());
    }
    let root = Path::new(data).join("resources");
    check_no_links(&root)?;
    root.canonicalize().map_err(io_error)
}

fn staging_parent(target: &ResourceTarget, resource: &Path) -> Result<PathBuf, String> {
    let root = resource_root(target)?;
    if !resource.starts_with(&root) {
        return Err("Profile path changed. Preview the update again.".into());
    }
    Ok(root
        .parent()
        .ok_or("Server data path is unavailable.")?
        .to_path_buf())
}

fn resolve_resource(target: &ResourceTarget) -> Result<PathBuf, String> {
    let root = resource_root(target)?;
    let resource = PathBuf::from(&target.resource_path);
    check_no_links(&resource)?;
    let resource = resource.canonicalize().map_err(io_error)?;
    if !resource.starts_with(&root) || resource == root {
        return Err("Resource must be inside the selected profile's resources folder.".into());
    }
    let relative = resource.strip_prefix(&root).map_err(io_error)?;
    if relative.components().any(|p| {
        p.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("[cfx-default]")
    }) {
        return Err("CFX default resources are updated with FXServer artifacts.".into());
    }
    require_manifest(&resource)?;
    if manifest_repository(&resource)?.is_some_and(|value| {
        reqwest::Url::parse(&value).ok().is_some_and(|url| {
            url.host_str() == Some("github.com")
                && url
                    .path()
                    .trim_start_matches('/')
                    .split('/')
                    .next()
                    .is_some_and(|owner| owner.eq_ignore_ascii_case("citizenfx"))
        })
    }) {
        return Err("CitizenFX resources are updated with FXServer artifacts.".into());
    }
    Ok(resource)
}

fn manifest_repository(resource: &Path) -> Result<Option<String>, String> {
    let manifest = require_manifest(resource)?;
    if fs::metadata(&manifest).map_err(io_error)?.len() > 1024 * 1024 {
        return Err("Resource manifest is too large.".into());
    }
    let content = fs::read_to_string(manifest).map_err(io_error)?;
    Ok(content.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix("repository")?;
        if rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        let start = rest.find(['\'', '"'])?;
        let quote = rest.as_bytes()[start] as char;
        Some(rest[start + 1..].split(quote).next()?.trim().to_string())
    }))
}

fn repository(resource: &Path) -> Result<(String, String), String> {
    let value = manifest_repository(resource)?
        .ok_or("Repository not found in the local resource manifest.")?;
    let url = reqwest::Url::parse(&value).map_err(|_| "Repository must be a GitHub HTTPS URL.")?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err("Only GitHub repository URLs can be updated automatically.".into());
    }
    let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
    if parts.len() != 2 {
        return Err("Use the repository root URL in the manifest.".into());
    }
    let owner = parts[0];
    let repo = parts[1].strip_suffix(".git").unwrap_or(parts[1]);
    if [owner, repo].iter().any(|s| {
        s.is_empty()
            || *s == "."
            || *s == ".."
            || !s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    }) {
        return Err("Invalid GitHub repository.".into());
    }
    if owner.eq_ignore_ascii_case("citizenfx") {
        return Err("CitizenFX resources are updated with FXServer artifacts.".into());
    }
    Ok((owner.into(), repo.into()))
}

fn validate_branch(branch: &str) -> Result<(), String> {
    if branch.is_empty()
        || branch.len() > 200
        || branch
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ['-', '_', '/', '.'].contains(&c))
    {
        return Err("Invalid GitHub branch.".into());
    }
    Ok(())
}

fn download(owner: &str, repo: &str, branch: &str, path: &Path) -> Result<u64, String> {
    let mut url = reqwest::Url::parse("https://codeload.github.com").map_err(io_error)?;
    url.path_segments_mut()
        .map_err(|_| "Invalid download URL.")?
        .extend([owner, repo, "zip", branch]);
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("FXServer-Installer")
        .build()
        .map_err(io_error)?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|e| format!("GitHub archive download failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("GitHub archive download failed: {e}"))?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_ARCHIVE)
    {
        return Err("Resource download exceeds 512 MiB.".into());
    }
    let mut file = File::create(path).map_err(io_error)?;
    let bytes =
        std::io::copy(&mut response.by_ref().take(MAX_ARCHIVE + 1), &mut file).map_err(io_error)?;
    if bytes > MAX_ARCHIVE {
        return Err("Resource download exceeds 512 MiB.".into());
    }
    Ok(bytes)
}

fn extract_archive(archive: &Path, destination: &Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(File::open(archive).map_err(io_error)?).map_err(io_error)?;
    if zip.len() > MAX_FILES {
        return Err("Resource archive contains too many entries.".into());
    }
    create_checked_directory(destination)?;
    let mut root = None;
    let mut paths = BTreeSet::new();
    let mut casing = BTreeMap::new();
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(io_error)?;
        let path = safe_relative(entry.name().trim_end_matches('/'))?;
        if entry
            .unix_mode()
            .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000))
        {
            return Err("Archive contains a link or special file.".into());
        }
        let mut components = path.components();
        let first = components
            .next()
            .ok_or("Empty archive path.")?
            .as_os_str()
            .to_os_string();
        if root.as_ref().is_some_and(|root| root != &first) {
            return Err("Archive must have one repository root.".into());
        }
        root = Some(first);
        let relative = components.as_path();
        if relative.as_os_str().is_empty() {
            if entry.is_dir() {
                continue;
            }
            return Err("Archive has no repository root directory.".into());
        }
        let key = relative.to_string_lossy().to_lowercase();
        if !paths.insert(key) {
            return Err("Archive contains duplicate or case-colliding paths.".into());
        }
        let mut prefix = PathBuf::new();
        for component in relative.components() {
            prefix.push(component);
            let original = prefix.to_string_lossy().into_owned();
            if casing
                .insert(original.to_lowercase(), original.clone())
                .is_some_and(|previous| previous != original)
            {
                return Err("Archive contains case-colliding directories.".into());
            }
        }
        total = total
            .checked_add(entry.size())
            .ok_or("Archive size overflow.")?;
        if entry.size() > MAX_FILE || total > MAX_TOTAL {
            return Err(
                "Resource archive exceeds extraction limits (256 MiB/file, 2 GiB total).".into(),
            );
        }
        let output = destination.join(relative);
        if entry.is_dir() {
            create_checked_directory(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            create_checked_directory(parent)?;
        }
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .open(output)
            .map_err(io_error)?;
        let size = entry.size();
        let copied =
            std::io::copy(&mut entry.by_ref().take(MAX_FILE + 1), &mut file).map_err(io_error)?;
        if copied != size {
            return Err("Archive entry size mismatch.".into());
        }
    }
    Ok(())
}

fn safe_relative(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.len() > 2048 || value.contains('\\') {
        return Err("Unsafe resource path.".into());
    }
    let parts: Vec<_> = value.split('/').collect();
    if parts.len() > 32 {
        return Err("Resource path is nested too deeply.".into());
    }
    for part in &parts {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2"
                        | "3"
                        | "4"
                        | "5"
                        | "6"
                        | "7"
                        | "8"
                        | "9"
                        | "\u{b9}"
                        | "\u{b2}"
                        | "\u{b3}"
                )
            });
        if part.is_empty()
            || *part == "."
            || *part == ".."
            || part.ends_with(['.', ' '])
            || part
                .chars()
                .any(|c| c.is_control() || ":*?\"<>|".contains(c))
            || reserved
        {
            return Err(format!("Unsafe resource path: {value}"));
        }
    }
    Ok(parts.iter().collect())
}

fn check_no_links(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(io_error)?;
        if metadata.file_type().is_symlink() {
            return Err("Symbolic links are not supported for resource updates.".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(
                    "Junctions and reparse points are not supported for resource updates.".into(),
                );
            }
        }
    }
    Ok(())
}

fn create_checked_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            check_no_links(path)?;
            if !metadata.is_dir() {
                return Err("Resource directory path is not a directory.".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or("Resource directory has no parent.")?;
            create_checked_directory(parent)?;
            fs::create_dir(path).map_err(io_error)?;
            check_no_links(path)?;
        }
        Err(error) => return Err(io_error(error)),
    }
    Ok(())
}

fn inventory(root: &Path) -> Result<Inventory, String> {
    check_no_links(root)?;
    let mut result = Inventory::new();
    let mut total = 0;
    fn visit(
        root: &Path,
        path: &Path,
        result: &mut Inventory,
        total: &mut u64,
    ) -> Result<(), String> {
        for entry in fs::read_dir(path).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            check_no_links(&path)?;
            let relative = path
                .strip_prefix(root)
                .map_err(io_error)?
                .to_string_lossy()
                .replace('\\', "/");
            safe_relative(&relative)?;
            if entry.file_type().map_err(io_error)?.is_dir() {
                visit(root, &path, result, total)?;
                continue;
            }
            if !entry.file_type().map_err(io_error)?.is_file() {
                return Err("Resource contains a special file.".into());
            }
            if fs::metadata(&path).map_err(io_error)?.len() > MAX_FILE {
                return Err("Resource file exceeds 256 MiB.".into());
            }
            let file = fingerprint(&path)?;
            *total += file.size;
            if result.len() >= MAX_FILES || *total > MAX_TOTAL {
                return Err("Resource exceeds safety limits (50,000 files or 2 GiB).".into());
            }
            result.insert(relative, file);
        }
        Ok(())
    }
    visit(root, root, &mut result, &mut total)?;
    Ok(result)
}

fn fingerprint(path: &Path) -> Result<Fingerprint, String> {
    let size = fs::metadata(path).map_err(io_error)?.len();
    if size > MAX_ARCHIVE {
        return Err("File exceeds checksum size limit.".into());
    }
    let mut input = File::open(path).map_err(io_error)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut count = 0_u64;
    loop {
        let n = input.read(&mut buffer).map_err(io_error)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > MAX_ARCHIVE {
            return Err("File changed while hashing.".into());
        }
        hasher.update(&buffer[..n]);
    }
    if count != size {
        return Err("File changed while hashing.".into());
    }
    Ok(Fingerprint {
        size,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn copy_inventory(source: &Path, destination: &Path, files: &Inventory) -> Result<(), String> {
    create_checked_directory(destination)?;
    for path in files.keys() {
        let relative = safe_relative(path)?;
        let input = source.join(&relative);
        check_no_links(&input)?;
        let output = destination.join(relative);
        if let Some(parent) = output.parent() {
            create_checked_directory(parent)?;
        }
        if fs::symlink_metadata(&output).is_ok() {
            check_no_links(&output)?;
        }
        fs::copy(input, output).map_err(io_error)?;
    }
    Ok(())
}

fn default_protected(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    let name = path.rsplit('/').next().unwrap_or(&path);
    name.ends_with(".cfg")
        || name.ends_with(".json")
        || name == ".env"
        || name.starts_with(".env.")
        || (name.starts_with("config") && name.ends_with(".lua"))
        || path
            .split('/')
            .any(|part| matches!(part, "config" | "configs" | "configuration" | ".git"))
}

fn file_changes(local: &Inventory, remote: &Inventory) -> Vec<FileChange> {
    let paths: BTreeSet<_> = local.keys().chain(remote.keys()).collect();
    paths
        .into_iter()
        .filter_map(|path| {
            let old = local.get(path);
            let new = remote.get(path);
            if old == new {
                return None;
            }
            Some(FileChange {
                path: path.clone(),
                kind: if old.is_none() {
                    "added"
                } else if new.is_none() {
                    "removed"
                } else {
                    "modified"
                }
                .into(),
                old_size: old.map(|f| f.size),
                new_size: new.map(|f| f.size),
                preserve: old.is_some() && (new.is_none() || default_protected(path)),
                can_preserve: old.is_some(),
            })
        })
        .collect()
}

fn require_manifest(resource: &Path) -> Result<PathBuf, String> {
    let manifest = ["fxmanifest.lua", "__resource.lua"]
        .into_iter()
        .map(|name| resource.join(name))
        .find(|path| path.is_file())
        .ok_or("Resource must contain a manifest at its root.")?;
    check_no_links(&manifest)?;
    Ok(manifest)
}

fn snapshot_folder(storage: &Path, target: &ResourceTarget, resource: &Path) -> PathBuf {
    let key = format!("{}\0{}", target.workspace_id, resource.to_string_lossy());
    storage
        .join("snapshots")
        .join(format!("{:x}", Sha256::digest(key.as_bytes())))
}

fn read_snapshot(folder: &Path) -> Result<SnapshotRecord, String> {
    let path = folder.join("snapshot.json");
    check_no_links(&path)?;
    if fs::metadata(&path).map_err(io_error)?.len() > 32 * 1024 * 1024 {
        return Err("Snapshot index is too large.".into());
    }
    serde_json::from_reader(File::open(path).map_err(io_error)?).map_err(io_error)
}

fn cleanup_expired_previews(storage: &Path, current: SystemTime) -> Result<(), String> {
    let folder = storage.join("previews");
    if !folder.exists() {
        return Ok(());
    }
    check_no_links(&folder)?;
    for entry in fs::read_dir(&folder).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        if !entry.file_type().map_err(io_error)?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if validate_id(&id).is_err() {
            continue;
        }
        let expired = current
            .duration_since(
                entry
                    .metadata()
                    .map_err(io_error)?
                    .modified()
                    .map_err(io_error)?,
            )
            .unwrap_or_default()
            .as_secs()
            > PREVIEW_LIFETIME;
        if expired {
            remove_owned(storage, &entry.path())?;
        }
    }
    Ok(())
}

fn ensure_free_space(path: &Path, required: u64) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut available = 0;
        let success = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                path.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if success == 0 {
            return Err(format!(
                "Could not check resource-update disk space: {}",
                std::io::Error::last_os_error()
            ));
        }
        if available < required.saturating_add(64 * 1024 * 1024) {
            return Err("Not enough disk space to stage and snapshot the resource safely.".into());
        }
    }
    #[cfg(not(windows))]
    let _ = (path, required);
    Ok(())
}

fn remove_owned(root: &Path, target: &Path) -> Result<(), String> {
    check_no_links(target)?;
    let root = root.canonicalize().map_err(io_error)?;
    let target = target.canonicalize().map_err(io_error)?;
    if target == root || !target.starts_with(&root) {
        return Err("Refusing to remove a folder outside resource update storage.".into());
    }
    fs::remove_dir_all(target).map_err(io_error)
}

fn validate_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 100
        || !value.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
    {
        return Err("Invalid snapshot ID.".into());
    }
    Ok(())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn unique_id() -> String {
    format!(
        "{:x}-{:x}-{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )
}
fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
fn io_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        storage: PathBuf,
        resource: PathBuf,
        target: ResourceTarget,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("fxserver-resource-test-{}", unique_id()));
            let resource = root.join("server/resources/[local]/example");
            let storage = root.join("storage");
            fs::create_dir_all(&resource).unwrap();
            fs::create_dir_all(&storage).unwrap();
            fs::create_dir_all(root.join("txData/default")).unwrap();
            fs::write(
                resource.join("fxmanifest.lua"),
                "version '1.0.0'\nrepository 'https://github.com/example/resource'\n",
            )
            .unwrap();
            fs::write(resource.join("config.lua"), "secret = 'keep me'").unwrap();
            fs::write(resource.join("server.lua"), "old code").unwrap();
            fs::write(
                root.join("txData/default/config.json"),
                serde_json::to_vec(
                    &serde_json::json!({ "server": { "dataPath": root.join("server") } }),
                )
                .unwrap(),
            )
            .unwrap();
            let target = ResourceTarget {
                workspace_id: "test".into(),
                tx_data_path: root.join("txData").to_string_lossy().into(),
                profile: "default".into(),
                resource_path: resource.to_string_lossy().into(),
            };
            Self {
                root,
                storage,
                resource,
                target,
            }
        }

        fn prepared(&self) -> Prepared {
            let directory = self.storage.join("previews/fixture");
            let remote = directory.join("remote");
            fs::create_dir_all(&remote).unwrap();
            fs::write(
                remote.join("fxmanifest.lua"),
                "version '2.0.0'\nrepository 'https://github.com/example/resource'\n",
            )
            .unwrap();
            fs::write(remote.join("config.lua"), "secret = 'default'").unwrap();
            fs::write(remote.join("server.lua"), "new code").unwrap();
            fs::write(remote.join("added.lua"), "new file").unwrap();
            Prepared {
                target: self.target.clone(),
                resource: self.resource.canonicalize().unwrap(),
                directory,
                local: inventory(&self.resource).unwrap(),
                remote: inventory(&remote).unwrap(),
                preview: ResourcePreview {
                    id: "fixture".into(),
                    resource_name: "example".into(),
                    repository: "https://github.com/example/resource".into(),
                    branch: "main".into(),
                    archive_sha256: String::new(),
                    archive_bytes: 0,
                    changes: vec![],
                    created_at: now(),
                },
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn rejects_traversal_and_windows_ambiguous_paths() {
        for path in [
            "../server.cfg",
            "/root/file",
            "C:/Windows/file",
            "root/../file",
            "root\\file",
            "root/NUL.txt",
            "root/file.",
            "root/file:stream",
            "root//file",
            "./file",
            "CON",
            "CONOUT$",
            "root/COM\u{b9}.txt",
            "root/LPT\u{b2}",
        ] {
            assert!(safe_relative(path).is_err(), "accepted {path}");
        }
        assert!(safe_relative("config/settings.lua").is_ok());
    }

    #[test]
    fn protects_configs_and_local_only_files_by_default() {
        for path in [
            "server.cfg",
            "Config.lua",
            ".env",
            ".env.production",
            "shared/config/items.lua",
            "settings.json",
            ".git/config",
        ] {
            assert!(default_protected(path), "unprotected {path}");
        }
        assert!(!default_protected("server/main.lua"));
        let file = Fingerprint {
            size: 1,
            sha256: "old".into(),
        };
        let local = BTreeMap::from([("local.lua".into(), file)]);
        let changes = file_changes(&local, &Inventory::new());
        assert_eq!(changes[0].kind, "removed");
        assert!(changes[0].preserve);
    }

    #[test]
    fn confines_targets_to_profile_resources_and_excludes_cfx() {
        let fixture = Fixture::new();
        assert!(resolve_resource(&fixture.target).is_ok());
        let mut target = fixture.target.clone();
        target.profile = "../default".into();
        assert!(resolve_resource(&target).is_err());
        target = fixture.target.clone();
        target.resource_path = fixture.root.to_string_lossy().into();
        assert!(resolve_resource(&target).is_err());
        fs::write(
            fixture.resource.join("fxmanifest.lua"),
            "repository 'https://github.com/citizenfx/fivem'",
        )
        .unwrap();
        assert!(resolve_resource(&fixture.target).is_err());
    }

    #[test]
    fn update_preserves_config_and_rollback_restores_every_file() {
        let fixture = Fixture::new();
        let original = inventory(&fixture.resource).unwrap();
        let prepared = fixture.prepared();
        let snapshot = apply_prepared(
            &fixture.storage,
            &prepared,
            &ApplyRequest {
                target: fixture.target.clone(),
                preview_id: "fixture".into(),
                protected_paths: vec!["config.lua".into()],
            },
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(fixture.resource.join("server.lua")).unwrap(),
            "new code"
        );
        assert_eq!(
            fs::read_to_string(fixture.resource.join("config.lua")).unwrap(),
            "secret = 'keep me'"
        );
        assert!(fixture.resource.join("added.lua").exists());
        let safety = rollback(&fixture.storage, &fixture.target, &snapshot.id).unwrap();
        assert_eq!(safety.reason, "Before rollback");
        assert!(inventory(&fixture.resource).unwrap() == original);
    }

    #[test]
    fn changed_files_abort_before_writing() {
        let fixture = Fixture::new();
        let prepared = fixture.prepared();
        fs::write(fixture.resource.join("server.lua"), "new user changes").unwrap();
        let error = apply_prepared(
            &fixture.storage,
            &prepared,
            &ApplyRequest {
                target: fixture.target.clone(),
                preview_id: "fixture".into(),
                protected_paths: vec![],
            },
        )
        .unwrap_err();
        assert!(error.contains("Local files changed"));
        assert_eq!(
            fs::read_to_string(fixture.resource.join("server.lua")).unwrap(),
            "new user changes"
        );
    }

    #[test]
    fn corrupt_snapshot_cannot_overwrite_resource() {
        let fixture = Fixture::new();
        let files = inventory(&fixture.resource).unwrap();
        let resource = fixture.resource.canonicalize().unwrap();
        let snapshot =
            create_snapshot(&fixture.storage, &fixture.target, &resource, "Test", &files).unwrap();
        let folder =
            snapshot_folder(&fixture.storage, &fixture.target, &resource).join(&snapshot.id);
        fs::write(folder.join("files/server.lua"), "tampered").unwrap();
        assert!(rollback(&fixture.storage, &fixture.target, &snapshot.id)
            .unwrap_err()
            .contains("integrity"));
        assert!(inventory(&fixture.resource).unwrap() == files);
    }

    fn archive(path: &Path, entries: &[(&str, &str)]) {
        let mut archive = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, content) in entries {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(content.as_bytes()).unwrap();
        }
        archive.finish().unwrap();
    }

    #[test]
    fn archives_reject_escape_and_case_collisions() {
        let fixture = Fixture::new();
        for (index, entries) in [
            vec![("repo/../outside.lua", "bad")],
            vec![("repo/A.lua", "a"), ("repo/a.lua", "b")],
            vec![("repo/Foo/a.lua", "a"), ("repo/foo/b.lua", "b")],
            vec![("repo/a.lua", "a"), ("other/b.lua", "b")],
        ]
        .into_iter()
        .enumerate()
        {
            let input = fixture.root.join(format!("{index}.zip"));
            archive(&input, &entries);
            assert!(
                extract_archive(&input, &fixture.root.join(format!("output-{index}"))).is_err()
            );
        }
    }

    #[test]
    fn valid_archive_extracts_without_repository_wrapper() {
        let fixture = Fixture::new();
        let input = fixture.root.join("valid.zip");
        archive(
            &input,
            &[
                ("repo-main/fxmanifest.lua", "version '1'"),
                ("repo-main/client/main.lua", "code"),
            ],
        );
        let output = fixture.root.join("extracted");
        extract_archive(&input, &output).unwrap();
        assert!(output.join("fxmanifest.lua").exists());
        assert_eq!(
            fs::read_to_string(output.join("client/main.lua")).unwrap(),
            "code"
        );
    }

    #[test]
    fn failed_staging_leaves_original_untouched() {
        let fixture = Fixture::new();
        let local = inventory(&fixture.resource).unwrap();
        let prepared = fixture.prepared();
        fs::write(
            prepared.directory.join("remote/server.lua"),
            "tampered remote",
        )
        .unwrap();
        assert!(replace_resource(
            &fixture.resource,
            &fixture.root,
            &prepared.directory.join("remote"),
            &prepared.remote,
            &BTreeSet::new(),
            &local
        )
        .is_err());
        assert!(inventory(&fixture.resource).unwrap() == local);
    }

    #[test]
    fn archives_reject_symbolic_links() {
        let fixture = Fixture::new();
        let input = fixture.root.join("link.zip");
        let mut archive = zip::ZipWriter::new(File::create(&input).unwrap());
        archive
            .add_symlink(
                "repo/link",
                "../../outside",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.finish().unwrap();
        assert!(extract_archive(&input, &fixture.root.join("extracted"))
            .unwrap_err()
            .contains("link"));
    }

    #[cfg(windows)]
    #[test]
    fn archive_and_copy_reject_junctions_before_creating_children() {
        use crate::process::CommandNoWindowExt;
        let fixture = Fixture::new();
        let outside = fixture.root.join("outside");
        fs::create_dir(&outside).unwrap();
        let linked = fixture.root.join("linked");
        let script = format!(
            "New-Item -ItemType Junction -Path '{}' -Target '{}' -ErrorAction Stop | Out-Null",
            linked.to_string_lossy().replace('\'', "''"),
            outside.to_string_lossy().replace('\'', "''"),
        );
        let status = std::process::Command::new("powershell")
            .no_window()
            .args(["-NoProfile", "-Command", &script])
            .status()
            .unwrap();
        assert!(status.success());
        let input = fixture.root.join("junction.zip");
        archive(&input, &[("repo/new/file.lua", "escape")]);
        let extraction = extract_archive(&input, &linked.join("extracted"));
        let copy = copy_inventory(
            &fixture.resource,
            &linked.join("copied"),
            &inventory(&fixture.resource).unwrap(),
        );
        let writes = fs::read_dir(&outside).unwrap().count();
        fs::remove_dir(&linked).unwrap();
        assert!(extraction.is_err());
        assert!(copy.is_err());
        assert_eq!(writes, 0);
    }

    #[test]
    fn conflicting_preserved_file_does_not_replace_user_data() {
        let fixture = Fixture::new();
        let mut prepared = fixture.prepared();
        let remote = prepared.directory.join("remote");
        fs::remove_file(remote.join("config.lua")).unwrap();
        fs::create_dir(remote.join("config.lua")).unwrap();
        fs::write(remote.join("config.lua/default.lua"), "new default").unwrap();
        prepared.remote = inventory(&remote).unwrap();
        let result = apply_prepared(
            &fixture.storage,
            &prepared,
            &ApplyRequest {
                target: fixture.target.clone(),
                preview_id: "fixture".into(),
                protected_paths: vec!["config.lua".into()],
            },
        );
        assert!(result.is_err());
        assert!(inventory(&fixture.resource).unwrap() == prepared.local);
    }

    #[test]
    fn snapshot_limit_never_prunes_existing_backups() {
        let fixture = Fixture::new();
        let resource = fixture.resource.canonicalize().unwrap();
        let files = inventory(&resource).unwrap();
        for _ in 0..MAX_SNAPSHOTS {
            create_snapshot(&fixture.storage, &fixture.target, &resource, "Test", &files).unwrap();
        }
        assert!(
            create_snapshot(&fixture.storage, &fixture.target, &resource, "Test", &files)
                .unwrap_err()
                .contains("storage limit")
        );
        assert_eq!(
            fs::read_dir(snapshot_folder(
                &fixture.storage,
                &fixture.target,
                &resource
            ))
            .unwrap()
            .count(),
            MAX_SNAPSHOTS
        );
        assert!(inventory(&resource).unwrap() == files);
    }

    #[test]
    fn expired_preview_cleanup_is_confined_to_owned_directories() {
        let fixture = Fixture::new();
        let old = fixture.storage.join("previews/abc-123");
        let recent = fixture.storage.join("previews/abc-456");
        let other = fixture.storage.join("previews/not-an-owned-id");
        for path in [&old, &recent, &other] {
            fs::create_dir_all(path).unwrap();
        }
        cleanup_expired_previews(&fixture.storage, SystemTime::now()).unwrap();
        assert!(old.exists());
        assert!(recent.exists());
        cleanup_expired_previews(
            &fixture.storage,
            SystemTime::now() + Duration::from_secs(PREVIEW_LIFETIME + 60),
        )
        .unwrap();
        assert!(!old.exists());
        assert!(!recent.exists());
        assert!(other.exists());
    }
}
