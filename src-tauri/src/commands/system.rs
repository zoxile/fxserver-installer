use std::{fs, path::Path, process::Command};

#[cfg(target_os = "windows")]
use crate::process::CommandNoWindowExt;

#[tauri::command]
pub async fn open_external_url(url: String) -> Result<(), String> {
    super::run_blocking(move || open_external_url_blocking(url)).await
}

fn open_external_url_blocking(url: String) -> Result<(), String> {
    let trimmed = url.trim();

    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("Only HTTP and HTTPS links can be opened externally.".to_string());
    }

    if trimmed.chars().any(|character| character.is_control()) {
        return Err("External link contains invalid control characters.".to_string());
    }

    open_url(trimmed)
}

#[tauri::command]
pub async fn read_text_file(path: String, max_bytes: Option<u64>) -> Result<String, String> {
    super::run_blocking(move || read_text_file_blocking(path, max_bytes)).await
}

fn read_text_file_blocking(path: String, max_bytes: Option<u64>) -> Result<String, String> {
    let path = Path::new(path.trim());

    if !path.exists() {
        return Err("Selected file does not exist.".to_string());
    }

    if !path.is_file() {
        return Err("Selected path is not a file.".to_string());
    }

    let metadata =
        fs::metadata(path).map_err(|error| format!("Failed to inspect selected file: {error}"))?;
    let limit = max_bytes.unwrap_or(10 * 1024 * 1024);
    if metadata.len() > limit {
        return Err(format!(
            "Selected file is too large. Maximum supported size is {} MB.",
            limit / 1024 / 1024
        ));
    }

    fs::read_to_string(path)
        .map_err(|error| format!("Failed to read selected file as UTF-8 text: {error}"))
}

#[cfg(target_os = "windows")]
fn open_url(url: &str) -> Result<(), String> {
    Command::new("rundll32")
        .no_window()
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .map_err(|error| format!("Failed to open external link: {error}"))?;

    Ok(())
}

#[cfg(target_os = "macos")]
fn open_url(url: &str) -> Result<(), String> {
    Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|error| format!("Failed to open external link: {error}"))?;

    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_url(url: &str) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map_err(|error| format!("Failed to open external link: {error}"))?;

    Ok(())
}
