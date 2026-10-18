// Enhanced edition integration adapted from Huntercorlett's fork (53f1834).
#[cfg(windows)]
use std::process::Command;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

#[path = "artifact/catalog.rs"]
mod catalog;
#[path = "artifact/download_page.rs"]
mod download_page;
#[path = "artifact/enhanced.rs"]
mod enhanced;
#[path = "artifact/install.rs"]
mod install;
#[path = "artifact/tar_archive.rs"]
mod tar_archive;
use super::server_exe::{find_server_executable, ServerEdition};
static INSTALL_OPERATION: Mutex<()> = Mutex::new(());

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn get_enhanced_artifact_catalog(
) -> Result<crate::models::artifact::EnhancedArtifactCatalog, String> {
    super::run_blocking(load_enhanced_catalog).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn install_enhanced_artifact(
    manager: tauri::State<'_, super::fxserver::FxserverManager>,
    request: crate::models::artifact::EnhancedInstallRequest,
) -> Result<crate::models::artifact::ArtifactInstallResult, String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || install_enhanced_for_manager(&manager, request)).await
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn get_windows_artifact_catalog(
    refresh: bool,
) -> Result<crate::models::artifact::ArtifactCatalog, String> {
    super::run_blocking(move || load_catalog(refresh)).await
}

use crate::models::artifact::{
    ArtifactCatalog, ArtifactInstallRequest, ArtifactInstallResult, ArtifactMetadata,
    ArtifactPlatform, EnhancedArtifactCatalog, EnhancedInstallRequest, InstalledArtifactInfo,
};
#[cfg(windows)]
use crate::process::CommandNoWindowExt;

pub(crate) fn load_catalog(refresh: bool) -> Result<ArtifactCatalog, String> {
    catalog::load_catalog(refresh)
}

pub(crate) fn load_enhanced_catalog() -> Result<EnhancedArtifactCatalog, String> {
    enhanced::load_catalog()
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn get_windows_artifact_metadata() -> Result<ArtifactMetadata, String> {
    super::run_blocking(fetch_artifact_metadata_blocking).await
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn get_installed_windows_artifact_info(
    destination: String,
) -> Result<InstalledArtifactInfo, String> {
    super::run_blocking(move || get_installed_artifact_info(&destination)).await
}

pub(crate) fn get_installed_artifact_info(
    destination: &str,
) -> Result<InstalledArtifactInfo, String> {
    let destination = PathBuf::from(destination.trim());
    if destination.as_os_str().is_empty() {
        return Err("Choose an install folder before checking installed artifacts.".to_string());
    }

    let marker_path = destination.join(".fxserver-artifact-version");
    let executable = find_server_executable(&destination)?;
    let edition = executable.as_ref().map(|(_, edition)| *edition);
    let has_fxserver_executable = executable.is_some();
    let citizen_server_impl_path = find_citizen_server_impl(&destination);
    let version_info = citizen_server_impl_path
        .as_deref()
        .and_then(read_file_version_info);
    let marker_version = read_marker_version(&marker_path)?;
    let version = if edition == Some(ServerEdition::Enhanced) {
        marker_version
    } else {
        version_info
            .as_ref()
            .and_then(artifact_version_from_file_info)
            .or(marker_version)
    };

    let installed =
        version.is_some() || citizen_server_impl_path.is_some() || has_fxserver_executable;
    let detection_source = if edition == Some(ServerEdition::Enhanced) {
        if version.is_some() {
            "marker"
        } else {
            "executable"
        }
    } else if version_info.is_some() {
        "citizen-server-impl"
    } else if version.is_some() {
        "marker"
    } else if citizen_server_impl_path.is_some() {
        "citizen-server-impl"
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
        citizen_server_impl_path: citizen_server_impl_path
            .map(|path| path.to_string_lossy().to_string()),
        file_version: version_info
            .as_ref()
            .and_then(|info| info.file_version.clone()),
        product_version: version_info.and_then(|info| info.product_version),
        has_fxserver_executable,
        edition: edition.map(|edition| edition.as_str().to_string()),
        detection_source: detection_source.to_string(),
    })
}

#[derive(Debug)]
struct FileVersionInfo {
    file_version: Option<String>,
    product_version: Option<String>,
    file_private_part: Option<u64>,
    product_private_part: Option<u64>,
}

fn read_marker_version(marker_path: &Path) -> Result<Option<String>, String> {
    if !marker_path.exists() {
        return Ok(None);
    }

    Ok(Some(
        fs::read_to_string(marker_path)
            .map_err(|error| format!("Failed to read installed artifact marker: {error}"))?
            .trim()
            .to_string(),
    )
    .filter(|value| !value.is_empty()))
}

fn artifact_version_from_file_info(info: &FileVersionInfo) -> Option<String> {
    info.product_private_part
        .or(info.file_private_part)
        .filter(|version| *version > 0)
        .map(|version| version.to_string())
        .or_else(|| {
            info.product_version
                .as_deref()
                .or(info.file_version.as_deref())
                .and_then(|version| {
                    version
                        .split(|character: char| !character.is_ascii_digit())
                        .rfind(|part| !part.is_empty())
                })
                .map(str::to_string)
        })
}

fn find_citizen_server_impl(destination: &Path) -> Option<PathBuf> {
    let direct_candidates = [
        destination.join("citizen-server-impl.dll"),
        destination.join("citizen-server-impl"),
        destination
            .join("citizen")
            .join("system")
            .join("citizen-server-impl.dll"),
    ];

    for candidate in direct_candidates {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    find_file_by_stem(destination, "citizen-server-impl", 6)
}

fn find_file_by_stem(directory: &Path, expected_stem: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 || !directory.is_dir() {
        return None;
    }

    let entries = fs::read_dir(directory).ok()?;
    let mut first_match = None;

    for entry in entries.flatten() {
        let path = entry.path();
        let metadata = entry.file_type().ok()?;
        if metadata.is_symlink() {
            continue;
        }
        if metadata.is_file() {
            let stem_matches = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| {
                    stem.eq_ignore_ascii_case(expected_stem)
                        || stem.to_ascii_lowercase().starts_with(expected_stem)
                })
                .unwrap_or(false);
            let name_matches = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| {
                    let name = name.to_ascii_lowercase();
                    name == expected_stem || name.starts_with(&format!("{expected_stem}."))
                })
                .unwrap_or(false);

            if stem_matches || name_matches {
                let is_dll = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(|extension| extension.eq_ignore_ascii_case("dll"))
                    .unwrap_or(false);

                if is_dll {
                    return Some(path);
                }

                first_match.get_or_insert(path);
            }
        } else if metadata.is_dir() {
            if let Some(found) = find_file_by_stem(&path, expected_stem, depth - 1) {
                return Some(found);
            }
        }
    }

    first_match
}

#[cfg(not(windows))]
fn read_file_version_info(_path: &Path) -> Option<FileVersionInfo> {
    None
}

#[cfg(windows)]
fn read_file_version_info(path: &Path) -> Option<FileVersionInfo> {
    let path_literal = powershell_string_literal(&path.to_string_lossy());
    let script = format!(
        r#"
$ErrorActionPreference = "Stop"
$info = [System.Diagnostics.FileVersionInfo]::GetVersionInfo({path_literal})
[pscustomobject]@{{
    FileVersion = $info.FileVersion
    ProductVersion = $info.ProductVersion
    FilePrivatePart = $info.FilePrivatePart
    ProductPrivatePart = $info.ProductPrivatePart
}} | ConvertTo-Json -Compress
"#
    );

    let output = Command::new("powershell")
        .no_window()
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let content = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(content.trim()).ok()?;

    Some(FileVersionInfo {
        file_version: value
            .get("FileVersion")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        product_version: value
            .get("ProductVersion")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        file_private_part: number_from_json(value.get("FilePrivatePart")),
        product_private_part: number_from_json(value.get("ProductPrivatePart")),
    })
}

#[cfg(windows)]
fn number_from_json(value: Option<&serde_json::Value>) -> Option<u64> {
    value.and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
    })
}

#[cfg(windows)]
fn powershell_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(crate) fn fetch_artifact_metadata_blocking() -> Result<ArtifactMetadata, String> {
    let platform = ArtifactPlatform::host()?;
    let response = enhanced::client(std::time::Duration::from_secs(30))?
        .get("https://artifacts.jgscripts.com/jsonv2")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("Failed to fetch JG Scripts artifact metadata: {error}"))?;
    if !response.status().is_success() {
        return Err("Metadata redirects are not accepted.".into());
    }
    let bytes = download_page::read_bounded(response, 1024 * 1024)?;
    let metadata: ArtifactMetadata = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Failed to parse JG Scripts artifact metadata: {error}"))?;

    let link = metadata.download_link(platform).ok_or_else(|| {
        format!(
            "JG Scripts metadata did not include a {} artifact.",
            platform.as_str()
        )
    })?;
    legacy_archive_url(link, &metadata.recommended_artifact, platform)?;

    Ok(metadata)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn install_windows_artifact(
    manager: tauri::State<'_, super::fxserver::FxserverManager>,
    request: ArtifactInstallRequest,
) -> Result<ArtifactInstallResult, String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || install_legacy_for_manager(&manager, request)).await
}

/// Called only while holding the manager's stopped-server guard.
fn install_legacy_artifact(
    request: ArtifactInstallRequest,
) -> Result<ArtifactInstallResult, String> {
    let _operation = INSTALL_OPERATION
        .try_lock()
        .map_err(|_| "Another artifact installation is in progress.")?;
    let platform = ArtifactPlatform::host()?;
    let url = legacy_archive_url(request.url.trim(), &request.version, platform)?;
    catalog::validate_install_risk(&request.version, request.acknowledge_risk)?;

    install::install(
        &url,
        &request.destination,
        &request.version,
        ServerEdition::Legacy,
        platform,
    )
}

fn install_enhanced_artifact_blocking(
    request: EnhancedInstallRequest,
) -> Result<ArtifactInstallResult, String> {
    let _operation = INSTALL_OPERATION
        .try_lock()
        .map_err(|_| "Another artifact installation is in progress.")?;
    let platform = ArtifactPlatform::host()?;
    let version = enhanced::validate_published_download(&request.url)?;
    install::install(
        request.url.trim(),
        &request.destination,
        &version,
        ServerEdition::Enhanced,
        platform,
    )
}

pub(crate) fn install_legacy_for_manager(
    manager: &super::fxserver::FxserverManager,
    request: ArtifactInstallRequest,
) -> Result<ArtifactInstallResult, String> {
    manager.with_stopped_server(|| install_legacy_artifact(request))
}

pub(crate) fn install_enhanced_for_manager(
    manager: &super::fxserver::FxserverManager,
    request: EnhancedInstallRequest,
) -> Result<ArtifactInstallResult, String> {
    manager.with_stopped_server(|| install_enhanced_artifact_blocking(request))
}

fn legacy_archive_url(
    url: &str,
    version: &str,
    platform: ArtifactPlatform,
) -> Result<String, String> {
    // The official listing exposes .7z links; the same build also publishes server.zip.
    let url = (platform == ArtifactPlatform::Windows)
        .then_some(url)
        .and_then(|url| {
            url.strip_suffix("/server.7z")
                .map(|base| format!("{base}/server.zip"))
        })
        .unwrap_or_else(|| url.to_string());
    catalog::validate_download(&url, version, platform)?;
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_zip_and_seven_zip_links_resolve_to_the_same_validated_build() {
        let base = "https://runtime.fivem.net/artifacts/fivem/build_server_windows/master/123-0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            legacy_archive_url(
                &format!("{base}/server.7z"),
                "123",
                ArtifactPlatform::Windows
            )
            .unwrap(),
            format!("{base}/server.zip")
        );
        assert_eq!(
            legacy_archive_url(
                &format!("{base}/server.zip"),
                "123",
                ArtifactPlatform::Windows
            )
            .unwrap(),
            format!("{base}/server.zip")
        );
        assert!(legacy_archive_url(
            &format!("{base}/server.7z"),
            "124",
            ArtifactPlatform::Windows
        )
        .is_err());
        assert!(legacy_archive_url(
            "https://evil.example/server.7z",
            "123",
            ArtifactPlatform::Windows
        )
        .is_err());
        assert!(legacy_archive_url(
            &format!("{base}/server.zip"),
            "123",
            ArtifactPlatform::Linux
        )
        .is_err());
        let linux_url = format!(
            "{}123-0123456789abcdef0123456789abcdef01234567/fx.tar.xz",
            ArtifactPlatform::Linux.legacy_base()
        );
        assert_eq!(
            legacy_archive_url(&linux_url, "123", ArtifactPlatform::Linux).unwrap(),
            linux_url
        );
    }
}
