use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

const LOG_FOLDER: &str = "logs";
const LOG_FILE: &str = "fxserver-installer.log";
static LOG_WRITE: Mutex<()> = Mutex::new(());
static BACKGROUND_LOG_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
pub struct AppLogFile {
    path: String,
    entries: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientLogRequest {
    directory: Option<String>,
    file_name: Option<String>,
    max_lines: Option<usize>,
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

#[tauri::command]
pub async fn read_app_logs(app: AppHandle) -> Result<AppLogFile, String> {
    super::run_blocking(move || read_app_logs_blocking(app)).await
}

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

#[tauri::command]
pub async fn append_app_log(app: AppHandle, entry: String) -> Result<(), String> {
    super::run_blocking(move || append_app_log_blocking(app, entry)).await
}

fn append_app_log_blocking(app: AppHandle, entry: String) -> Result<(), String> {
    let _guard = LOG_WRITE
        .lock()
        .map_err(|_| "Application log lock is unavailable.".to_string())?;
    let path = log_path(&app)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("Failed to open application log file: {error}"))?;

    file.write_all(format!("{entry}\n").as_bytes())
        .map_err(|error| format!("Failed to write application log file: {error}"))
}

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

#[tauri::command]
pub async fn clear_app_logs(app: AppHandle) -> Result<(), String> {
    super::run_blocking(move || clear_app_logs_blocking(app)).await
}

fn clear_app_logs_blocking(app: AppHandle) -> Result<(), String> {
    let _guard = LOG_WRITE
        .lock()
        .map_err(|_| "Application log lock is unavailable.".to_string())?;
    let path = log_path(&app)?;
    fs::write(&path, "").map_err(|error| format!("Failed to clear application log file: {error}"))
}

#[tauri::command]
pub async fn read_client_logs(request: ClientLogRequest) -> Result<ClientLogResult, String> {
    super::run_blocking(move || read_client_logs_blocking(request)).await
}

fn read_client_logs_blocking(request: ClientLogRequest) -> Result<ClientLogResult, String> {
    let directory = request
        .directory
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_client_log_directory);
    let max_lines = request.max_lines.unwrap_or(700).clamp(50, 5000);
    let files = list_client_log_files(&directory)?;
    let selected = select_client_log_file(&files, request.file_name.as_deref());

    let Some(selected_file) = selected else {
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

fn log_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve application data directory: {error}"))?
        .join(LOG_FOLDER);

    fs::create_dir_all(&directory)
        .map_err(|error| format!("Failed to create application log directory: {error}"))?;

    Ok(directory.join(LOG_FILE))
}

fn default_client_log_directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(""))
        .join("FiveM")
        .join("FiveM.app")
        .join("logs")
}

fn list_client_log_files(directory: &Path) -> Result<Vec<ClientLogFile>, String> {
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

    for entry in entries {
        let entry = entry.map_err(|error| format!("Failed to inspect client log file: {error}"))?;
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|error| format!("Failed to inspect {}: {error}", path.to_string_lossy()))?;

        if !metadata.is_file() || !is_log_like_file(&path) {
            continue;
        }

        files.push(ClientLogFile {
            name: entry.file_name().to_string_lossy().to_string(),
            path: path.to_string_lossy().to_string(),
            size: metadata.len(),
            modified: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs()),
        });
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

fn is_log_like_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "log" | "txt"))
        .unwrap_or(false)
}

fn select_client_log_file(
    files: &[ClientLogFile],
    requested_file_name: Option<&str>,
) -> Option<ClientLogFile> {
    let requested_file_name = requested_file_name
        .map(str::trim)
        .filter(|value| !value.is_empty());

    requested_file_name
        .and_then(|file_name| files.iter().find(|file| file.name == file_name))
        .or_else(|| files.first())
        .map(|file| ClientLogFile {
            name: file.name.clone(),
            path: file.path.clone(),
            size: file.size,
            modified: file.modified,
        })
}

fn tail_file(path: &Path, max_lines: usize) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("Failed to open {}: {error}", path.to_string_lossy()))?;
    let file_size = file
        .metadata()
        .map_err(|error| format!("Failed to inspect {}: {error}", path.to_string_lossy()))?
        .len();

    if file_size == 0 {
        return Ok(String::new());
    }

    let mut offset = file_size;
    let mut buffer = Vec::new();
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
        chunk.extend(buffer);
        buffer = chunk;

        if buffer.iter().filter(|byte| **byte == b'\n').count() >= target_newlines {
            break;
        }
    }

    let content = String::from_utf8_lossy(&buffer);
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    Ok(lines[start..].join("\n"))
}
