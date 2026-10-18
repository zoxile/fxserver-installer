#[cfg(any(feature = "desktop", all(test, unix)))]
use std::io::Write;
#[cfg(any(feature = "desktop", test))]
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
#[cfg(feature = "desktop")]
use std::{sync::Mutex, time::SystemTime};

use serde::{Deserialize, Serialize};
#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter, Manager};

const LOG_FOLDER: &str = "logs";
const LOG_FILE: &str = "fxserver-installer.log";
#[cfg(feature = "desktop")]
static LOG_WRITE: Mutex<()> = Mutex::new(());
#[cfg(any(feature = "desktop", test))]
static BACKGROUND_LOG_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
pub struct AppLogFile {
    path: String,
    entries: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct ClientLogRequest {
    pub directory: Option<String>,
    pub file_name: Option<String>,
    pub max_lines: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientLogFile {
    name: String,
    path: String,
    size: u64,
    modified: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientLogResult {
    directory: String,
    files: Vec<ClientLogFile>,
    selected_file: Option<String>,
    path: Option<String>,
    content: String,
    line_count: usize,
}

const CLIENT_LOG_MAX_BYTES: u64 = 4 * 1024 * 1024;
#[cfg(windows)]
const MAX_LOG_DIRECTORY_ENTRIES: usize = 4096;
#[cfg(windows)]
const MAX_LOG_FILES: usize = 256;

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn read_app_logs(app: AppHandle) -> Result<AppLogFile, String> {
    super::run_blocking(move || read_app_logs_blocking(app)).await
}

#[cfg(feature = "desktop")]
fn read_app_logs_blocking(app: AppHandle) -> Result<AppLogFile, String> {
    let path = log_path(&app)?;
    if !path.exists() {
        return Ok(AppLogFile {
            path: path.to_string_lossy().to_string(),
            entries: Vec::new(),
        });
    }

    let content = tail_file(&path, 700)?;

    Ok(AppLogFile {
        path: path.to_string_lossy().to_string(),
        entries: content.lines().map(str::to_string).collect(),
    })
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn append_app_log(app: AppHandle, entry: String) -> Result<(), String> {
    super::run_blocking(move || append_app_log_blocking(app, entry)).await
}

#[cfg(feature = "desktop")]
fn append_app_log_blocking(app: AppHandle, entry: String) -> Result<(), String> {
    let _guard = LOG_WRITE
        .lock()
        .map_err(|_| "Application log lock is unavailable.".to_string())?;
    let path = log_path(&app)?;
    let mut file = log_write_options()
        .append(true)
        .open(&path)
        .map_err(|error| format!("Failed to open application log file: {error}"))?;

    file.write_all(format!("{entry}\n").as_bytes())
        .map_err(|error| format!("Failed to write application log file: {error}"))
}

#[cfg(feature = "desktop")]
pub(crate) fn append_background_log(app: &AppHandle, level: &str, scope: &str, message: &str) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = BACKGROUND_LOG_ID.fetch_add(1, Ordering::Relaxed);
    let entry = serde_json::json!({
        "id": format!("background-{timestamp}-{sequence}"),
        "timestamp": timestamp as u64,
        "level": level,
        "scope": scope,
        "message": message,
    });
    if let Err(error) = append_app_log_blocking(app.clone(), entry.to_string()) {
        log::error!("Could not persist background log: {error}");
    }
    let _ = app.emit("background-app-log", entry);
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn clear_app_logs(app: AppHandle) -> Result<(), String> {
    super::run_blocking(move || clear_app_logs_blocking(app)).await
}

#[cfg(feature = "desktop")]
fn clear_app_logs_blocking(app: AppHandle) -> Result<(), String> {
    let _guard = LOG_WRITE
        .lock()
        .map_err(|_| "Application log lock is unavailable.".to_string())?;
    let path = log_path(&app)?;
    log_write_options()
        .truncate(true)
        .open(&path)
        .map(|_| ())
        .map_err(|error| format!("Failed to clear application log file: {error}"))
}

#[cfg(any(feature = "desktop", all(test, unix)))]
fn log_write_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        #[cfg(target_os = "linux")]
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    options
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn read_client_logs(request: ClientLogRequest) -> Result<ClientLogResult, String> {
    super::run_blocking(move || read_client_logs_blocking(request)).await
}

#[cfg(not(windows))]
fn read_client_logs_blocking(_request: ClientLogRequest) -> Result<ClientLogResult, String> {
    Err("FiveM client logs are only supported on Windows. Server and application logs remain available on this platform.".into())
}

#[cfg(windows)]
fn read_client_logs_blocking(request: ClientLogRequest) -> Result<ClientLogResult, String> {
    let directory = client_log_directory(request.directory.as_deref())?;
    let max_lines = request.max_lines.unwrap_or(700).clamp(1, 5000);
    let files = list_client_log_files(&directory)?;
    let selected = select_client_log_file(&files, request.file_name.as_deref());

    let Some(selected_file) = selected else {
        if request
            .file_name
            .as_deref()
            .is_some_and(|name| !name.trim().is_empty())
        {
            return Err(
                "The selected client log is no longer available. List the log files again.".into(),
            );
        }
        return Ok(ClientLogResult {
            directory: directory.to_string_lossy().to_string(),
            files,
            selected_file: None,
            path: None,
            content: String::new(),
            line_count: 0,
        });
    };

    let path = PathBuf::from(&selected_file.path);
    let content = tail_file(&path, max_lines)?;
    let line_count = content.lines().count();

    Ok(ClientLogResult {
        directory: directory.to_string_lossy().to_string(),
        files,
        selected_file: Some(selected_file.name),
        path: Some(path.to_string_lossy().to_string()),
        content,
        line_count,
    })
}

#[cfg(feature = "desktop")]
fn log_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve application data directory: {error}"))?
        .join(LOG_FOLDER);

    super::config_history::ensure_unlinked_path(&directory)?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Failed to create application log directory: {error}"))?;

    let path = directory.join(LOG_FILE);
    super::config_history::ensure_unlinked_path(&path)?;
    if path
        .try_exists()
        .map_err(|error| format!("Failed to inspect application log: {error}"))?
        && !path.is_file()
    {
        return Err("Application logs require a regular file.".into());
    }
    Ok(path)
}

#[cfg(feature = "terminal")]
pub(crate) fn core_app_log_path() -> Result<PathBuf, String> {
    // Tauri app_data_dir uses roaming data on Windows, unlike local config history.
    #[cfg(windows)]
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("The Windows roaming application data directory is unavailable.")?
        .join("com.fxserver.installer");
    #[cfg(not(windows))]
    let root = crate::platform::data_dir()?;
    Ok(root.join(LOG_FOLDER).join(LOG_FILE))
}

#[cfg(any(test, feature = "terminal"))]
pub(crate) fn list_app_logs_at(path: &Path) -> Result<Vec<ClientLogFile>, String> {
    super::config_history::ensure_unlinked_path(path)?;
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(vec![log_metadata(path, &metadata)]),
        Ok(_) => Err("Application logs require a regular file.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err("The application log cannot be inspected.".into()),
    }
}

#[cfg(any(test, feature = "terminal"))]
pub(crate) fn read_app_logs_at(path: &Path, max_lines: usize) -> Result<AppLogFile, String> {
    if !(1..=5000).contains(&max_lines) {
        return Err("Log tails require between 1 and 5000 lines.".into());
    }
    let content = tail_file(path, max_lines)?;
    Ok(AppLogFile {
        path: path.to_string_lossy().into(),
        entries: content.lines().map(str::to_string).collect(),
    })
}

#[cfg(feature = "terminal")]
pub(crate) async fn list_client_logs(
    directory: Option<String>,
) -> Result<Vec<ClientLogFile>, String> {
    super::run_blocking(move || {
        #[cfg(windows)]
        { list_client_log_files(&client_log_directory(directory.as_deref())?) }
        #[cfg(not(windows))]
        {
            let _ = directory;
            Err("FiveM client logs are only supported on Windows. Server and application logs remain available on this platform.".into())
        }
    }).await
}

#[cfg(windows)]
fn client_log_directory(directory: Option<&str>) -> Result<PathBuf, String> {
    let directory = directory
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(default_client_log_directory)?;
    if !directory.is_absolute() {
        return Err("Choose an absolute FiveM client log directory.".into());
    }
    super::config_history::ensure_unlinked_path(&directory)?;
    Ok(directory)
}

#[cfg(any(windows, test, feature = "terminal"))]
fn log_metadata(path: &Path, metadata: &fs::Metadata) -> ClientLogFile {
    ClientLogFile {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        path: path.to_string_lossy().into(),
        size: metadata.len(),
        modified: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs()),
    }
}

#[cfg(windows)]
fn default_client_log_directory() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|path| path.join("FiveM/FiveM.app/logs"))
        .ok_or_else(|| "The Windows local application data directory is unavailable. Select a FiveM client log directory explicitly.".into())
}

#[cfg(windows)]
fn list_client_log_files(directory: &Path) -> Result<Vec<ClientLogFile>, String> {
    super::config_history::ensure_unlinked_path(directory)?;
    if !directory.is_dir() {
        return Err(format!(
            "FiveM client log folder was not found: {}",
            directory.to_string_lossy()
        ));
    }

    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "Failed to inspect FiveM client log folder {}: {error}",
            directory.to_string_lossy()
        )
    })?;
    let mut files = Vec::new();

    for (index, entry) in entries.enumerate() {
        if index >= MAX_LOG_DIRECTORY_ENTRIES {
            return Err("The client log directory contains more than 4096 entries.".into());
        }
        let entry = entry.map_err(|error| format!("Failed to inspect client log file: {error}"))?;
        let path = entry.path();
        if !is_log_like_file(&path) {
            continue;
        }
        super::config_history::ensure_unlinked_path(&path)?;
        let metadata = entry
            .metadata()
            .map_err(|error| format!("Failed to inspect {}: {error}", path.to_string_lossy()))?;

        if !metadata.is_file() {
            continue;
        }
        if files.len() >= MAX_LOG_FILES {
            return Err("The client log directory contains more than 256 log files.".into());
        }
        files.push(log_metadata(&path, &metadata));
    }

    files.sort_by(|left, right| {
        right.modified.cmp(&left.modified).then_with(|| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
        })
    });

    Ok(files)
}

#[cfg(windows)]
fn is_log_like_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "log" | "txt"))
        .unwrap_or(false)
}

#[cfg(windows)]
fn select_client_log_file(
    files: &[ClientLogFile],
    requested_file_name: Option<&str>,
) -> Option<ClientLogFile> {
    let requested_file_name = requested_file_name
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match requested_file_name {
        Some(name) => files.iter().find(|file| file.name == name),
        None => files.first(),
    }
    .map(|file| ClientLogFile {
        name: file.name.clone(),
        path: file.path.clone(),
        size: file.size,
        modified: file.modified,
    })
}

fn tail_file(path: &Path, max_lines: usize) -> Result<String, String> {
    if !(1..=5000).contains(&max_lines) {
        return Err("Log tails require between 1 and 5000 lines.".into());
    }
    super::config_history::ensure_unlinked_path(path)?;
    if !path.is_file() {
        return Err("Logs can only be read from regular files.".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("Failed to open {}: {error}", path.to_string_lossy()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Failed to inspect {}: {error}", path.to_string_lossy()))?;
    if !metadata.is_file() {
        return Err("Logs can only be read from regular files.".into());
    }
    let file_size = metadata.len();

    if file_size == 0 {
        return Ok(String::new());
    }

    let mut offset = file_size;
    let mut buffer = Vec::new();
    let mut newlines = 0;
    let chunk_size = 64 * 1024_u64;
    let target_newlines = max_lines.saturating_add(1);

    while offset > 0 && buffer.len() < CLIENT_LOG_MAX_BYTES as usize {
        let read_size = offset.min(chunk_size);
        offset -= read_size;
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| format!("Failed to read {}: {error}", path.to_string_lossy()))?;

        let mut chunk = vec![0_u8; read_size as usize];
        file.read_exact(&mut chunk)
            .map_err(|error| format!("Failed to read {}: {error}", path.to_string_lossy()))?;
        newlines += chunk.iter().filter(|byte| **byte == b'\n').count();
        chunk.extend(buffer);
        buffer = chunk;

        if newlines >= target_newlines {
            break;
        }
    }

    let content = String::from_utf8_lossy(&buffer);
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    Ok(lines[start..].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "fxi-log-tools-{}-{}",
                std::process::id(),
                BACKGROUND_LOG_ID.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn application_listing_does_not_read_contents_or_create_directories() {
        let fixture = Fixture::new();
        let missing = fixture.0.join("missing/app.log");
        assert!(list_app_logs_at(&missing).unwrap().is_empty());
        assert!(!missing.parent().unwrap().exists());
        let path = fixture.0.join("app.log");
        fs::write(&path, b"token=fixture-secret\n").unwrap();
        let list = list_app_logs_at(&path).unwrap();
        assert_eq!(list.len(), 1);
        assert!(!serde_json::to_string(&list)
            .unwrap()
            .contains("fixture-secret"));
    }

    #[test]
    fn tails_are_line_and_byte_bounded_and_reject_invalid_limits() {
        let fixture = Fixture::new();
        let path = fixture.0.join("app.log");
        fs::write(&path, "one\ntwo\nthree\n").unwrap();
        assert_eq!(tail_file(&path, 2).unwrap(), "two\nthree");
        assert_eq!(read_app_logs_at(&path, 1).unwrap().entries, ["three"]);
        assert!(tail_file(&path, 0).is_err());
        assert!(tail_file(&path, 5001).is_err());
        assert!(tail_file(&fixture.0, 1).is_err());
        fs::write(&path, vec![b'x'; CLIENT_LOG_MAX_BYTES as usize + 100]).unwrap();
        assert_eq!(
            tail_file(&path, 1).unwrap().len(),
            CLIENT_LOG_MAX_BYTES as usize
        );
    }

    #[cfg(windows)]
    #[test]
    fn client_listing_is_metadata_only_and_explicit_selection_never_falls_back() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("selected.log"), "fixture-secret").unwrap();
        fs::write(fixture.0.join("ignored.json"), "{}").unwrap();
        let files = list_client_log_files(&fixture.0).unwrap();
        assert_eq!(files.len(), 1);
        assert!(!serde_json::to_string(&files)
            .unwrap()
            .contains("fixture-secret"));
        assert!(select_client_log_file(&files, Some("missing.log")).is_none());
        assert!(select_client_log_file(&files, Some("../selected.log")).is_none());
        assert_eq!(
            select_client_log_file(&files, None).unwrap().name,
            "selected.log"
        );
        assert!(read_client_logs_blocking(ClientLogRequest {
            directory: Some(fixture.0.to_string_lossy().into()),
            file_name: Some("missing.log".into()),
            max_lines: Some(1),
        })
        .is_err());
        assert!(client_log_directory(Some("relative/logs")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn client_log_file_listing_is_bounded() {
        let fixture = Fixture::new();
        for index in 0..=MAX_LOG_FILES {
            fs::write(fixture.0.join(format!("{index}.log")), "").unwrap();
        }
        assert!(list_client_log_files(&fixture.0).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn linked_logs_and_directories_are_rejected() {
        let fixture = Fixture::new();
        let path = fixture.0.join("app.log");
        let link = fixture.0.join("linked.log");
        fs::write(&path, "fixture").unwrap();
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(tail_file(&link, 1).is_err());
        assert!(list_app_logs_at(&link).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn application_log_files_are_private_and_truncate_without_changing_mode() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!(
            "fxi-log-{}-{}",
            std::process::id(),
            BACKGROUND_LOG_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = log_write_options()
            .create_new(true)
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"fixture\n").unwrap();
        drop(file);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(log_write_options().truncate(true).open(&path).unwrap());
        assert_eq!(fs::metadata(&path).unwrap().len(), 0);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_file(path).unwrap();
    }

    #[cfg(not(windows))]
    #[test]
    fn client_logs_reject_both_default_and_explicit_directories() {
        for directory in [None, Some("/tmp/FiveM/logs".into())] {
            let result = read_client_logs_blocking(ClientLogRequest {
                directory,
                file_name: None,
                max_lines: None,
            });
            assert!(matches!(result, Err(error) if error.contains("only supported on Windows")));
        }
    }
}
