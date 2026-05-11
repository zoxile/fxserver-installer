
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::models::artifact::{
    ArtifactInstallRequest, ArtifactInstallResult, ArtifactMetadata, InstalledArtifactInfo,
};

#[tauri::command]
pub async fn get_windows_artifact_metadata() -> Result<ArtifactMetadata, String> {
    tokio::task::spawn_blocking(fetch_artifact_metadata_blocking)
        .await
        .map_err(|error| format!("Artifact metadata task failed: {error}"))?
}

#[tauri::command]
pub fn get_installed_windows_artifact_info(
    destination: String,
) -> Result<InstalledArtifactInfo, String> {
    let destination = PathBuf::from(destination.trim());
    if destination.as_os_str().is_empty() {
        return Err("Choose an install folder before checking installed artifacts.".to_string());
    }

    let marker_path = destination.join(".fxserver-artifact-version");
    let executable_path = destination.join("FXServer.exe");
    let has_fxserver_executable = executable_path.exists();
    let version = if marker_path.exists() {
        Some(
            fs::read_to_string(&marker_path)
                .map_err(|error| format!("Failed to read installed artifact marker: {error}"))?
                .trim()
                .to_string(),
        )
        .filter(|value| !value.is_empty())
    } else {
        None
    };

    let installed = version.is_some() || has_fxserver_executable;
    let detection_source = if version.is_some() {
        "marker"
    } else if has_fxserver_executable {
        "executable"
    } else {
        "none"
    };

    Ok(InstalledArtifactInfo {
        installed,
        version,
        destination: destination.to_string_lossy().to_string(),
        marker_path: marker_path.to_string_lossy().to_string(),
        has_fxserver_executable,
        detection_source: detection_source.to_string(),
    })
}

fn fetch_artifact_metadata_blocking() -> Result<ArtifactMetadata, String> {
    let script = r#"
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
(Invoke-WebRequest -Uri "https://artifacts.jgscripts.com/jsonv2" -UseBasicParsing).Content
"#;

    let output = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .output()
        .map_err(|error| format!("Failed to fetch JG Scripts artifact metadata: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "JG Scripts artifact metadata request failed without output.".to_string()
        } else {
            stderr
        });
    }

    let content = String::from_utf8_lossy(&output.stdout);
    let metadata: ArtifactMetadata = serde_json::from_str(content.trim())
        .map_err(|error| format!("Failed to parse JG Scripts artifact metadata: {error}"))?;

    if metadata.recommended_artifact.trim().is_empty() || metadata.windows_download_link.trim().is_empty() {
        return Err("JG Scripts artifact metadata did not include a Windows artifact.".to_string());
    }

    Ok(metadata)
}

#[tauri::command]
pub async fn install_windows_artifact(
    request: ArtifactInstallRequest,
) -> Result<ArtifactInstallResult, String> {
    tokio::task::spawn_blocking(move || install_windows_artifact_blocking(request))
        .await
        .map_err(|error| format!("Artifact install task failed: {error}"))?
}

fn install_windows_artifact_blocking(
    request: ArtifactInstallRequest,
) -> Result<ArtifactInstallResult, String> {
    if !cfg!(target_os = "windows") {
        return Err("Artifact installation is only supported on Windows right now.".to_string());
    }

    if !request.url.starts_with("https://") || !request.url.contains("runtime.fivem.net") {
        return Err("Artifact download URL must be an official HTTPS FiveM runtime URL.".to_string());
    }

    let destination = PathBuf::from(request.destination.trim());
    if destination.as_os_str().is_empty() {
        return Err("Choose a destination folder before installing artifacts.".to_string());
    }

    fs::create_dir_all(&destination)
        .map_err(|error| format!("Failed to create artifact destination: {error}"))?;

    let zip_path = destination.join(format!("fxserver-artifact-{}.zip", request.version));
    let marker_path = destination.join(".fxserver-artifact-version");

    run_install_script(
        &request.url,
        &zip_path,
        &destination,
        &request.version,
        &marker_path,
    )?;

    Ok(ArtifactInstallResult {
        version: request.version,
        destination: destination.to_string_lossy().to_string(),
        marker_path: marker_path.to_string_lossy().to_string(),
    })
}

fn run_install_script(
    url: &str,
    zip_path: &Path,
    destination: &Path,
    version: &str,
    marker_path: &Path,
) -> Result<(), String> {
    let script = r#"
param(
    [string] $Url,
    [string] $ZipPath,
    [string] $Destination,
    [string] $Version,
    [string] $MarkerPath
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

Invoke-WebRequest -Uri $Url -OutFile $ZipPath -UseBasicParsing
Expand-Archive -LiteralPath $ZipPath -DestinationPath $Destination -Force
Set-Content -LiteralPath $MarkerPath -Value $Version -Encoding UTF8
Remove-Item -LiteralPath $ZipPath -Force
"#;

    let output = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .arg(url)
        .arg(zip_path.to_string_lossy().to_string())
        .arg(destination.to_string_lossy().to_string())
        .arg(version)
        .arg(marker_path.to_string_lossy().to_string())
        .output()
        .map_err(|error| format!("Failed to start PowerShell artifact installer: {error}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if stderr.is_empty() { stdout } else { stderr };

    Err(if detail.is_empty() {
        "Artifact installer failed without output.".to_string()
    } else {
        detail
    })
}
