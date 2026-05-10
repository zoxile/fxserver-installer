use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

use serde::Serialize;
use tauri::{AppHandle, Manager};

const LOG_FOLDER: &str = "logs";
const LOG_FILE: &str = "fxserver-installer.log";

#[derive(Serialize)]
pub struct AppLogFile {
    path: String,
    entries: Vec<String>,
}

#[tauri::command]
pub fn read_app_logs(app: AppHandle) -> Result<AppLogFile, String> {
    let path = log_path(&app)?;
    if !path.exists() {
        return Ok(AppLogFile {
            path: path.to_string_lossy().to_string(),
            entries: Vec::new(),
        });
    }

    let content = fs::read_to_string(&path).map_err(|error| format!("Failed to read application log file: {error}"))?;

    Ok(AppLogFile {
        path: path.to_string_lossy().to_string(),
        entries: content.lines().map(str::to_string).collect(),
    })
}

#[tauri::command]
pub fn append_app_log(app: AppHandle, entry: String) -> Result<(), String> {
    let path = log_path(&app)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("Failed to open application log file: {error}"))?;

    writeln!(file, "{entry}").map_err(|error| format!("Failed to write application log file: {error}"))
}

#[tauri::command]
pub fn clear_app_logs(app: AppHandle) -> Result<(), String> {
    let path = log_path(&app)?;
    fs::write(&path, "").map_err(|error| format!("Failed to clear application log file: {error}"))
}

fn log_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve application data directory: {error}"))?
        .join(LOG_FOLDER);

    fs::create_dir_all(&directory).map_err(|error| format!("Failed to create application log directory: {error}"))?;

    Ok(directory.join(LOG_FILE))
}
