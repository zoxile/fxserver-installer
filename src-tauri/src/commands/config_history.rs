use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use super::fxserver::{
    config_file_metadata, decrypt_secret, encrypt_secret, resolve_profile_data_path,
};
use crate::models::fxserver::ServerConfigFile;

const MAX_CONFIG_BYTES: usize = 512 * 1024;
const MAX_VERSIONS: usize = 20;
const MAX_HISTORY_BYTES: usize = 4 * 1024 * 1024;
const MAX_STORE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_HISTORY_FILES: usize = 256;
const MAX_DIRECTORY_ENTRIES: usize = 4096;
const HISTORY_ERROR: &str = "Configuration history is unreadable, altered, or belongs to another Windows account. The config was not changed.";
const STALE_ERROR: &str = "CONFIG_CHANGED: This file changed outside this editor. Reload and review the current file before saving or restoring.";
static HISTORY_LOCK: Mutex<()> = Mutex::new(());
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFileRequest {
    pub tx_data_path: String,
    pub profile: String,
    pub path: String,
}

#[derive(Clone, Copy)]
pub(crate) enum ConfigChangeReason {
    Save,
    Restore,
    Patch,
}

impl ConfigChangeReason {
    fn label(self) -> &'static str {
        match self {
            Self::Save => "save",
            Self::Restore => "restore",
            Self::Patch => "patch",
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigHistoryVersion {
    pub id: String,
    pub created_at: u64,
    pub reason: String,
    pub size: usize,
    pub digest: String,
}

#[derive(Deserialize, Serialize)]
struct Snapshot {
    metadata: ConfigHistoryVersion,
    content: String,
}

#[derive(Deserialize, Serialize)]
struct Journal {
    format: u32,
    identity: String,
    snapshots: Vec<Snapshot>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigHistoryContent {
    pub version: ConfigHistoryVersion,
    pub content: String,
}

struct Target {
    path: PathBuf,
    identity: String,
}

pub(crate) fn history_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("config-history"))
        .map_err(|_| "The configuration history directory is unavailable.".into())
}

#[tauri::command]
pub async fn read_config_history_file(
    request: ConfigFileRequest,
) -> Result<ServerConfigFile, String> {
    super::run_blocking(move || {
        let target = resolve_target(&request)?;
        read_bounded_config(&target.path)?;
        config_file_metadata(&target.path)
    })
    .await
}

#[tauri::command]
pub async fn list_config_history(
    app: AppHandle,
    request: ConfigFileRequest,
) -> Result<Vec<ConfigHistoryVersion>, String> {
    super::run_blocking(move || {
        let _lock = HISTORY_LOCK.lock().map_err(|_| HISTORY_ERROR)?;
        let target = resolve_target(&request)?;
        let journal = load_journal(&history_root(&app)?, &target)?;
        Ok(journal
            .snapshots
            .into_iter()
            .rev()
            .map(|item| item.metadata)
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn read_config_history_version(
    app: AppHandle,
    request: ConfigFileRequest,
    version_id: String,
) -> Result<ConfigHistoryContent, String> {
    super::run_blocking(move || {
        let _lock = HISTORY_LOCK.lock().map_err(|_| HISTORY_ERROR)?;
        read_version(
            &history_root(&app)?,
            &resolve_target(&request)?,
            &version_id,
        )
    })
    .await
}

#[tauri::command]
pub async fn restore_config_history_version(
    app: AppHandle,
    request: ConfigFileRequest,
    version_id: String,
    expected_content: String,
    manager: tauri::State<'_, super::fxserver::FxserverManager>,
) -> Result<ServerConfigFile, String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || {
        manager.with_stopped_server(|| {
            restore_version(
                &history_root(&app)?,
                &request,
                &version_id,
                &expected_content,
            )
        })
    })
    .await
}

fn restore_version(
    store: &Path,
    request: &ConfigFileRequest,
    version_id: &str,
    expected_content: &str,
) -> Result<ServerConfigFile, String> {
    let _lock = HISTORY_LOCK.lock().map_err(|_| HISTORY_ERROR)?;
    let target = resolve_target(request)?;
    let version = read_version(store, &target, version_id)?;
    save_locked(
        store,
        request,
        &target,
        expected_content,
        &version.content,
        ConfigChangeReason::Restore,
    )
}

fn read_version(
    store: &Path,
    target: &Target,
    version_id: &str,
) -> Result<ConfigHistoryContent, String> {
    let item = load_journal(store, target)?
        .snapshots
        .into_iter()
        .find(|item| item.metadata.id == version_id)
        .ok_or("This version is no longer in configuration history. Reload history.")?;
    Ok(ConfigHistoryContent {
        version: item.metadata,
        content: item.content,
    })
}

pub(crate) fn save_config_atomic(
    history_root: &Path,
    request: &ConfigFileRequest,
    expected_content: &str,
    content: &str,
    reason: ConfigChangeReason,
) -> Result<ServerConfigFile, String> {
    let _lock = HISTORY_LOCK.lock().map_err(|_| HISTORY_ERROR)?;
    let target = resolve_target(request)?;
    save_locked(
        history_root,
        request,
        &target,
        expected_content,
        content,
        reason,
    )
}

pub(crate) fn save_config_with_revision(
    history_root: &Path,
    request: &ConfigFileRequest,
    expected_revision: &str,
    content: &str,
    reason: ConfigChangeReason,
) -> Result<ServerConfigFile, String> {
    let _lock = HISTORY_LOCK.lock().map_err(|_| HISTORY_ERROR)?;
    let target = resolve_target(request)?;
    let current = read_bounded_config(&target.path)?;
    if digest(current.as_bytes()) != expected_revision {
        return Err(STALE_ERROR.into());
    }
    save_locked(history_root, request, &target, &current, content, reason)
}

fn save_locked(
    store: &Path,
    request: &ConfigFileRequest,
    target: &Target,
    expected: &str,
    content: &str,
    reason: ConfigChangeReason,
) -> Result<ServerConfigFile, String> {
    if content.len() > MAX_CONFIG_BYTES || expected.len() > MAX_CONFIG_BYTES {
        return Err("Config files are limited to 512 KiB.".into());
    }
    if resolve_target(request)?.identity != target.identity {
        return Err(STALE_ERROR.into());
    }
    let current = read_bounded_config(&target.path)?;
    if current != expected {
        return Err(STALE_ERROR.into());
    }
    if current == content {
        return config_file_metadata(&target.path);
    }
    for version in [&current[..], content] {
        let encoded_size = serde_json::to_vec(version)
            .map_err(|_| HISTORY_ERROR)?
            .len();
        if encoded_size > MAX_HISTORY_BYTES / 2 - 4096 {
            return Err("The encoded config is too large to retain both the previous and new version within 4 MiB. No config was changed.".into());
        }
    }
    let mut journal = load_journal(store, target)?;
    push_snapshot(
        &mut journal,
        &current,
        &format!("before-{}", reason.label()),
    );
    // Persist the recovery version before touching the live file.
    persist_journal(store, target, &mut journal)?;
    let staged = stage_file(&target.path, content.as_bytes())?;
    let resolved_again = resolve_target(request)?;
    if resolved_again.identity != target.identity
        || read_bounded_config(&target.path)? != expected
        || read_limit(&staged.0, MAX_CONFIG_BYTES)? != content.as_bytes()
    {
        return Err(STALE_ERROR.into());
    }
    atomic_replace(&staged.0, &target.path)?;
    push_snapshot(&mut journal, content, reason.label());
    // The before-version is durable even if saving post-write metadata fails.
    if persist_journal(store, target, &mut journal).is_err() {
        return Err("The config was saved, but its final history entry could not be recorded. The previous version is preserved; reload the file before continuing.".into());
    }
    config_file_metadata(&target.path)
}

fn resolve_target(request: &ConfigFileRequest) -> Result<Target, String> {
    let (tx, profile, _, root) =
        resolve_profile_data_path(request.tx_data_path.clone(), request.profile.clone())?;
    let path = Path::new(&request.path);
    if !path.is_absolute()
        || path.components().any(|part| {
            matches!(part, Component::ParentDir)
                || matches!(part, Component::Normal(name) if name.to_string_lossy().contains(':'))
        })
    {
        return Err("Choose an existing cfg inside the selected profile dataPath.".into());
    }
    ensure_unlinked_path(path)?;
    let path = path.canonicalize().map_err(|_| {
        "The config file no longer exists. Reload the selected profile.".to_string()
    })?;
    if !path.starts_with(&root)
        || !path.is_file()
        || !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cfg"))
    {
        return Err("Config files must stay inside the selected profile dataPath.".into());
    }
    let identity = serde_json::to_string(&(tx, profile, root, &path)).map_err(|_| HISTORY_ERROR)?;
    Ok(Target { path, identity })
}

pub(crate) fn read_bounded_config(path: &Path) -> Result<String, String> {
    let bytes = read_limit(path, MAX_CONFIG_BYTES)?;
    String::from_utf8(bytes).map_err(|_| "The config is not valid UTF-8.".into())
}

fn read_limit(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|_| "The file cannot be read.".to_string())?;
    if !file
        .metadata()
        .map_err(|_| "The file cannot be inspected.")?
        .is_file()
    {
        return Err("Choose a regular file.".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The file cannot be read.".to_string())?;
    if bytes.len() > limit {
        return Err("The file exceeds the configuration size limit.".into());
    }
    Ok(bytes)
}

pub(crate) fn read_profile_configs(root: &Path) -> Result<Vec<ServerConfigFile>, String> {
    let root = root
        .canonicalize()
        .map_err(|_| "The dataPath cannot be opened.".to_string())?;
    let mut files = Vec::new();
    for (index, entry) in fs::read_dir(&root)
        .map_err(|_| "The dataPath cannot be read.".to_string())?
        .enumerate()
    {
        if index >= MAX_DIRECTORY_ENTRIES {
            return Err("The dataPath contains too many entries to inspect.".into());
        }
        let entry = entry.map_err(|_| "A config entry cannot be read.".to_string())?;
        if !entry
            .path()
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cfg"))
        {
            continue;
        }
        ensure_unlinked_path(&entry.path())?;
        let path = entry
            .path()
            .canonicalize()
            .map_err(|_| "A cfg cannot be resolved.".to_string())?;
        if !path.starts_with(&root) {
            return Err("A cfg points outside the selected profile dataPath.".into());
        }
        if !path.is_file() {
            continue;
        }
        if files.len() >= 128 {
            return Err("The profile contains more than 128 top-level cfg files.".into());
        }
        read_bounded_config(&path)?;
        files.push(config_file_metadata(&path)?);
    }
    files.sort_by_key(|file| {
        (
            file.name.to_lowercase() != "server.cfg",
            file.name.to_lowercase(),
        )
    });
    Ok(files)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn journal_path(store: &Path, target: &Target) -> PathBuf {
    store.join(format!("{}.dpapi", digest(target.identity.as_bytes())))
}

fn load_journal(store: &Path, target: &Target) -> Result<Journal, String> {
    let path = journal_path(store, target);
    ensure_unlinked_path(&path).map_err(|_| HISTORY_ERROR)?;
    if !path.try_exists().map_err(|_| HISTORY_ERROR)? {
        return Ok(Journal {
            format: 1,
            identity: target.identity.clone(),
            snapshots: Vec::new(),
        });
    }
    check_store_file(store, &path)?;
    let encrypted = read_limit(&path, MAX_HISTORY_BYTES).map_err(|_| HISTORY_ERROR)?;
    let decrypted = decrypt_secret(&encrypted).map_err(|_| HISTORY_ERROR)?;
    if decrypted.len() > MAX_HISTORY_BYTES {
        return Err(HISTORY_ERROR.into());
    }
    let journal: Journal = serde_json::from_slice(&decrypted).map_err(|_| HISTORY_ERROR)?;
    if journal.format != 1
        || journal.identity != target.identity
        || journal.snapshots.len() > MAX_VERSIONS
        || journal
            .snapshots
            .iter()
            .map(|item| item.content.len())
            .sum::<usize>()
            > MAX_HISTORY_BYTES
        || journal.snapshots.iter().any(|item| {
            item.content.len() > MAX_CONFIG_BYTES
                || item.metadata.size != item.content.len()
                || item.metadata.digest != digest(item.content.as_bytes())
                || ![
                    "save",
                    "restore",
                    "patch",
                    "before-save",
                    "before-restore",
                    "before-patch",
                ]
                .contains(&item.metadata.reason.as_str())
        })
    {
        return Err(HISTORY_ERROR.into());
    }
    Ok(journal)
}

fn push_snapshot(journal: &mut Journal, content: &str, reason: &str) {
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let id = format!(
        "{created_at}-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    journal.snapshots.push(Snapshot {
        metadata: ConfigHistoryVersion {
            id,
            created_at,
            reason: reason.into(),
            size: content.len(),
            digest: digest(content.as_bytes()),
        },
        content: content.into(),
    });
    while journal.snapshots.len() > MAX_VERSIONS
        || journal
            .snapshots
            .iter()
            .map(|item| item.content.len())
            .sum::<usize>()
            > MAX_HISTORY_BYTES
    {
        journal.snapshots.remove(0);
    }
}

fn check_store_file(store: &Path, path: &Path) -> Result<(), String> {
    ensure_unlinked_path(path).map_err(|_| HISTORY_ERROR)?;
    let root = store.canonicalize().map_err(|_| HISTORY_ERROR)?;
    let resolved = path.canonicalize().map_err(|_| HISTORY_ERROR)?;
    if resolved.parent() != Some(root.as_path()) || !resolved.is_file() {
        return Err(HISTORY_ERROR.into());
    }
    Ok(())
}

fn persist_journal(store: &Path, target: &Target, journal: &mut Journal) -> Result<(), String> {
    ensure_unlinked_path(store).map_err(|_| HISTORY_ERROR)?;
    fs::create_dir_all(store).map_err(|_| "Cannot create encrypted config history.".to_string())?;
    let path = journal_path(store, target);
    ensure_unlinked_path(&path).map_err(|_| HISTORY_ERROR)?;
    if path.try_exists().map_err(|_| HISTORY_ERROR)? {
        check_store_file(store, &path)?;
    }
    let encrypted = loop {
        let bytes = serde_json::to_vec(&journal).map_err(|_| HISTORY_ERROR)?;
        let encrypted = encrypt_secret(&bytes).map_err(|_| {
            "Windows could not encrypt configuration history. Nothing was written in plaintext."
                .to_string()
        })?;
        if encrypted.len() <= MAX_HISTORY_BYTES {
            break encrypted;
        }
        if journal.snapshots.len() <= 1 {
            return Err(
                "The encrypted snapshot exceeds the 4 MiB history limit. No config was changed."
                    .into(),
            );
        }
        journal.snapshots.remove(0);
    };
    let mut total = encrypted.len() as u64;
    let mut count = 1;
    for (index, entry) in fs::read_dir(store).map_err(|_| HISTORY_ERROR)?.enumerate() {
        if index >= MAX_DIRECTORY_ENTRIES {
            return Err("Encrypted configuration history contains too many entries.".into());
        }
        let entry = entry.map_err(|_| HISTORY_ERROR)?;
        if entry.path() == path || entry.path().extension().is_none_or(|ext| ext != "dpapi") {
            continue;
        }
        total = total.saturating_add(entry.metadata().map_err(|_| HISTORY_ERROR)?.len());
        count += 1;
        if total > MAX_STORE_BYTES || count > MAX_HISTORY_FILES {
            break;
        }
    }
    if total > MAX_STORE_BYTES || count > MAX_HISTORY_FILES {
        return Err(
            "Encrypted configuration history is full (64 MiB / 256 files). No config was changed."
                .into(),
        );
    }
    let staged = stage_file(&path, &encrypted)?;
    atomic_replace(&staged.0, &path)
}

struct StagedFile(PathBuf);
impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn stage_file(destination: &Path, bytes: &[u8]) -> Result<StagedFile, String> {
    ensure_unlinked_path(destination)?;
    let path = destination.with_file_name(format!(
        ".config-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| {
            "Cannot stage the configuration update; the original was preserved.".to_string()
        })?;
    let staged = StagedFile(path);
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| {
            "Cannot flush the configuration update; the original was preserved.".to_string()
        })?;
    Ok(staged)
}

pub(crate) fn ensure_unlinked_path(path: &Path) -> Result<(), String> {
    if path.components().any(|part| {
        matches!(part, Component::ParentDir)
            || matches!(part, Component::Normal(name) if name.to_string_lossy().contains(':'))
    }) {
        return Err("File paths cannot contain traversal or alternate data streams.".into());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(
                            "Configuration and diagnostic files cannot use reparse points.".into(),
                        );
                    }
                }
                if metadata.file_type().is_symlink() {
                    return Err(
                        "Configuration and diagnostic files cannot use symbolic links.".into(),
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err("The file path cannot be inspected.".into()),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    ensure_unlinked_path(source)?;
    ensure_unlinked_path(destination)?;
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_WRITE_THROUGH,
    };
    let src: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let dst: Vec<_> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let result = unsafe {
        if destination.exists() {
            ReplaceFileW(
                dst.as_ptr(),
                src.as_ptr(),
                ptr::null(),
                0,
                ptr::null(),
                ptr::null(),
            )
        } else {
            MoveFileExW(src.as_ptr(), dst.as_ptr(), MOVEFILE_WRITE_THROUGH)
        }
    };
    if result == 0 {
        return Err(
            "Cannot atomically replace the file. Close other editors and reload before retrying."
                .into(),
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    ensure_unlinked_path(source)?;
    ensure_unlinked_path(destination)?;
    fs::rename(source, destination).map_err(|_| "Cannot atomically replace the file.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        request: ConfigFileRequest,
        store: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "config-history-test-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let data = root.join("data");
            let tx = root.join("txData");
            fs::create_dir_all(tx.join("profile")).unwrap();
            fs::create_dir_all(&data).unwrap();
            fs::write(
                tx.join("profile/config.json"),
                serde_json::to_vec(&serde_json::json!({"dataPath": data})).unwrap(),
            )
            .unwrap();
            let path = data.join("server.cfg");
            fs::write(&path, "set secret_token \"fixture-secret\"\r\n").unwrap();
            Self {
                request: ConfigFileRequest {
                    tx_data_path: tx.to_string_lossy().into(),
                    profile: "profile".into(),
                    path: path.to_string_lossy().into(),
                },
                store: root.join("history"),
                root,
            }
        }
        fn content(&self) -> String {
            fs::read_to_string(&self.request.path).unwrap()
        }

        fn link_directory(&self, target: &Path, link: &Path) {
            #[cfg(windows)]
            {
                use crate::process::CommandNoWindowExt;
                let output = std::process::Command::new("powershell.exe")
                    .args(["-NoProfile", "-NonInteractive", "-Command",
                        "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:FXSI_TEST_LINK -Target $env:FXSI_TEST_TARGET | Out-Null"])
                    .env("FXSI_TEST_LINK", link)
                    .env("FXSI_TEST_TARGET", target)
                    .no_window().output().unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, link).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn linked_history_stores_and_config_parents_are_rejected() {
        let fixture = Fixture::new();
        let original = fixture.content();
        let outside = fixture.root.join("outside-history");
        fs::create_dir(&outside).unwrap();
        fixture.link_directory(&outside, &fixture.store);
        let target = resolve_target(&fixture.request).unwrap();
        assert!(load_journal(&fixture.store, &target).is_err());
        assert!(save_config_atomic(
            &fixture.store,
            &fixture.request,
            &original,
            "new",
            ConfigChangeReason::Save
        )
        .is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        assert_eq!(fixture.content(), original);
        #[cfg(windows)]
        fs::remove_dir(&fixture.store).unwrap();
        #[cfg(unix)]
        fs::remove_file(&fixture.store).unwrap();

        let alias = fixture.root.join("data/alias");
        fixture.link_directory(&fixture.root.join("data"), &alias);
        let mut request = fixture.request.clone();
        request.path = alias.join("server.cfg").to_string_lossy().into();
        assert!(resolve_target(&request).is_err());
        #[cfg(windows)]
        fs::remove_dir(alias).unwrap();
        #[cfg(unix)]
        fs::remove_file(alias).unwrap();
    }

    #[test]
    fn changed_profile_identity_is_rejected_even_when_file_content_matches() {
        let fixture = Fixture::new();
        let target = resolve_target(&fixture.request).unwrap();
        let original = fixture.content();
        fs::write(
            fixture.root.join("txData/profile/config.json"),
            serde_json::to_vec(&serde_json::json!({ "dataPath": fixture.root })).unwrap(),
        )
        .unwrap();
        assert!(save_locked(
            &fixture.store,
            &fixture.request,
            &target,
            &original,
            "new",
            ConfigChangeReason::Save
        )
        .unwrap_err()
        .contains("CONFIG_CHANGED"));
        assert!(!fixture.store.exists());
        assert_eq!(fixture.content(), original);
    }

    #[cfg(windows)]
    #[test]
    fn alternate_data_streams_are_not_config_targets() {
        let fixture = Fixture::new();
        let mut request = fixture.request.clone();
        request.path.push_str(":hidden.cfg");
        fs::write(&request.path, "hidden fixture").unwrap();
        assert!(resolve_target(&request).is_err());
    }

    #[test]
    fn stale_save_and_outside_paths_are_rejected() {
        let fixture = Fixture::new();
        let original = fixture.content();
        assert!(save_config_atomic(
            &fixture.store,
            &fixture.request,
            "stale",
            "new",
            ConfigChangeReason::Save
        )
        .unwrap_err()
        .contains("CONFIG_CHANGED"));
        assert_eq!(fixture.content(), original);
        assert!(!fixture.store.exists());
        let mut request = fixture.request.clone();
        request.path = fixture.root.join("outside.cfg").to_string_lossy().into();
        fs::write(&request.path, "outside").unwrap();
        assert!(resolve_target(&request).is_err());
        request = fixture.request.clone();
        request.profile = "../profile".into();
        assert!(resolve_target(&request).is_err());
    }

    #[test]
    fn history_retention_is_bounded() {
        let mut journal = Journal {
            format: 1,
            identity: "fixture".into(),
            snapshots: Vec::new(),
        };
        for _ in 0..40 {
            push_snapshot(&mut journal, &"a".repeat(MAX_CONFIG_BYTES), "save");
        }
        assert!(journal.snapshots.len() <= MAX_VERSIONS);
        assert!(
            journal
                .snapshots
                .iter()
                .map(|item| item.content.len())
                .sum::<usize>()
                <= MAX_HISTORY_BYTES
        );
    }

    #[test]
    fn revision_saves_reject_stale_content_before_creating_history() {
        let fixture = Fixture::new();
        let original = fixture.content();
        let revision = digest(original.as_bytes());
        fs::write(&fixture.request.path, "external editor change").unwrap();
        assert!(save_config_with_revision(
            &fixture.store,
            &fixture.request,
            &revision,
            "new",
            ConfigChangeReason::Patch
        )
        .unwrap_err()
        .contains("CONFIG_CHANGED"));
        assert_eq!(fixture.content(), "external editor change");
        assert!(!fixture.store.exists());
    }

    #[test]
    fn oversized_and_excessively_escaped_configs_preserve_the_original() {
        let fixture = Fixture::new();
        let original = fixture.content();
        for content in [
            "a".repeat(MAX_CONFIG_BYTES + 1),
            "\0".repeat(MAX_CONFIG_BYTES),
        ] {
            assert!(save_config_atomic(
                &fixture.store,
                &fixture.request,
                &original,
                &content,
                ConfigChangeReason::Save
            )
            .is_err());
            assert_eq!(fixture.content(), original);
            assert!(!fixture.store.exists());
        }
    }

    #[cfg(windows)]
    #[test]
    fn encrypted_file_limit_accounts_for_json_escaping_and_dpapi_overhead() {
        let fixture = Fixture::new();
        let target = resolve_target(&fixture.request).unwrap();
        let mut journal = load_journal(&fixture.store, &target).unwrap();
        for _ in 0..8 {
            push_snapshot(&mut journal, &"\t".repeat(MAX_CONFIG_BYTES), "save");
        }
        persist_journal(&fixture.store, &target, &mut journal).unwrap();
        assert!(
            fs::metadata(journal_path(&fixture.store, &target))
                .unwrap()
                .len()
                <= MAX_HISTORY_BYTES as u64
        );
        let loaded = load_journal(&fixture.store, &target).unwrap();
        assert!(loaded.snapshots.len() >= 2);
        assert!(loaded.snapshots.len() < 8);
    }

    #[cfg(windows)]
    #[test]
    fn encrypted_versions_restore_and_reject_tampering() {
        let fixture = Fixture::new();
        let original = fixture.content();
        save_config_atomic(
            &fixture.store,
            &fixture.request,
            &original,
            "ensure fixture\r\n",
            ConfigChangeReason::Save,
        )
        .unwrap();
        let target = resolve_target(&fixture.request).unwrap();
        let path = journal_path(&fixture.store, &target);
        let bytes = fs::read(&path).unwrap();
        assert!(!bytes
            .windows(b"fixture-secret".len())
            .any(|chunk| chunk == b"fixture-secret"));
        let journal = load_journal(&fixture.store, &target).unwrap();
        assert_eq!(journal.snapshots.len(), 2);
        let first = &journal.snapshots[0].metadata.id;
        assert!(
            restore_version(&fixture.store, &fixture.request, first, "stale")
                .unwrap_err()
                .contains("CONFIG_CHANGED")
        );
        restore_version(
            &fixture.store,
            &fixture.request,
            first,
            "ensure fixture\r\n",
        )
        .unwrap();
        assert_eq!(fixture.content(), original);
        let journal = load_journal(&fixture.store, &target).unwrap();
        assert_eq!(journal.snapshots[2].metadata.reason, "before-restore");
        assert_eq!(journal.snapshots[2].content, "ensure fixture\r\n");
        fs::write(&path, b"tampered").unwrap();
        assert!(save_config_atomic(
            &fixture.store,
            &fixture.request,
            &original,
            "other",
            ConfigChangeReason::Save
        )
        .is_err());
        assert_eq!(fixture.content(), original);
        fs::write(&path, bytes).unwrap();
        let mut other = fixture.request.clone();
        other.path = fixture.root.join("data/other.cfg").to_string_lossy().into();
        fs::write(&other.path, "other").unwrap();
        let other_target = resolve_target(&other).unwrap();
        fs::copy(&path, journal_path(&fixture.store, &other_target)).unwrap();
        assert!(load_journal(&fixture.store, &other_target).is_err());
    }
}
