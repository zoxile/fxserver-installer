use std::{fs::File, io::Read, path::Path, process::Command};

use serde::Serialize;

const MAX_TEXT_BYTES: u64 = 10 * 1024 * 1024;

#[cfg(target_os = "windows")]
use crate::process::CommandNoWindowExt;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub os: &'static str,
    pub arch: &'static str,
    pub artifact_platform: &'static str,
    pub client_logs_supported: bool,
    pub secure_storage_supported: bool,
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub fn get_platform_info() -> PlatformInfo {
    PlatformInfo {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        artifact_platform: if cfg!(windows) { "windows" } else { "linux" },
        client_logs_supported: cfg!(windows),
        secure_storage_supported: crate::platform::secure_storage_supported(),
    }
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn open_external_url(url: String) -> Result<(), String> {
    super::run_blocking(move || open_external_url_blocking(url)).await
}

fn open_external_url_blocking(url: String) -> Result<(), String> {
    open_url(validate_external_url(&url)?.as_str())
}

fn validate_external_url(url: &str) -> Result<reqwest::Url, String> {
    let trimmed = url.trim();

    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("Only HTTP and HTTPS links can be opened externally.".to_string());
    }

    if trimmed.chars().any(|character| character.is_control()) {
        return Err("External link contains invalid control characters.".to_string());
    }

    let parsed = reqwest::Url::parse(trimmed)
        .map_err(|_| "External link is not a valid HTTP or HTTPS URL.".to_string())?;
    if parsed.host_str().is_none() || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("External links must have a host and must not contain credentials.".into());
    }
    Ok(parsed)
}

#[cfg_attr(feature = "desktop", tauri::command)]
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

    let mut file =
        File::open(path).map_err(|error| format!("Failed to open selected file: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Failed to inspect selected file: {error}"))?;
    let limit = max_bytes.unwrap_or(MAX_TEXT_BYTES).min(MAX_TEXT_BYTES);
    if metadata.len() > limit {
        return Err(format!(
            "Selected file is too large. Maximum supported size is {} MB.",
            limit / 1024 / 1024
        ));
    }

    let mut content = String::new();
    file.by_ref()
        .take(limit + 1)
        .read_to_string(&mut content)
        .map_err(|error| format!("Failed to read selected file as UTF-8 text: {error}"))?;
    if content.len() as u64 > limit {
        return Err("Selected file grew beyond the supported size while reading.".into());
    }
    Ok(content)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_capabilities_use_the_frontend_contract() {
        let info = get_platform_info();
        assert_eq!(info.os, std::env::consts::OS);
        assert_eq!(info.arch, std::env::consts::ARCH);
        assert_eq!(info.client_logs_supported, cfg!(windows));
        assert_eq!(
            info.artifact_platform,
            if cfg!(windows) { "windows" } else { "linux" }
        );
        let value = serde_json::to_value(info).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 5);
        assert!(value["artifactPlatform"].is_string());
        assert!(value["clientLogsSupported"].is_boolean());
        assert!(value["secureStorageSupported"].is_boolean());
    }

    #[test]
    fn external_urls_are_parsed_without_launching_a_handler() {
        for url in [
            "file:///C:/secret",
            "https://",
            "http://user:password@example.com",
            "https://example.com/\ncommand",
        ] {
            assert!(validate_external_url(url).is_err(), "accepted {url}");
        }
        assert_eq!(
            validate_external_url("https://example.com/a b")
                .unwrap()
                .as_str(),
            "https://example.com/a%20b"
        );
    }

    #[test]
    fn caller_cannot_disable_the_text_file_limit() {
        let path = std::env::temp_dir().join(format!(
            "fxi-text-limit-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let file = File::create(&path).unwrap();
        file.set_len(MAX_TEXT_BYTES + 1).unwrap();
        drop(file);
        let result = read_text_file_blocking(path.to_string_lossy().into(), Some(u64::MAX));
        std::fs::remove_file(path).unwrap();
        assert!(result.unwrap_err().contains("too large"));
    }
}
