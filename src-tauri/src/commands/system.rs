use std::process::Command;

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();

    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("Only HTTP and HTTPS links can be opened externally.".to_string());
    }

    if trimmed.chars().any(|character| character.is_control()) {
        return Err("External link contains invalid control characters.".to_string());
    }

    open_url(trimmed)
}

#[cfg(target_os = "windows")]
fn open_url(url: &str) -> Result<(), String> {
    Command::new("rundll32")
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
