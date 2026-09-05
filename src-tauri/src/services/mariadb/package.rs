use super::install::{run_process, InstallOutput};
use serde::Deserialize;
use std::{
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Clone, Deserialize)]
pub(super) struct Package {
    pub version: String,
    pub file_name: String,
    pub sha256: String,
}

static CACHE: OnceLock<Mutex<Option<(Instant, Package)>>> = OnceLock::new();

pub(super) fn latest_package() -> Result<Package, String> {
    let mut cached = CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "MariaDB package cache is unavailable.".to_string())?;
    if let Some((time, package)) = &*cached {
        if time.elapsed() < Duration::from_secs(15 * 60) {
            return Ok(package.clone());
        }
    }
    let output = run_process(
        "powershell",
        &[
            "-NoProfile",
            "-Command",
            include_str!("package-metadata.ps1"),
        ],
        Duration::from_secs(75),
    )?;
    if !output.success {
        return Err(format!(
            "Could not resolve the MariaDB installer: {}",
            output.stderr
        ));
    }
    let package = parse_package(&output.stdout)?;
    *cached = Some((Instant::now(), package.clone()));
    Ok(package)
}

fn parse_package(json: &str) -> Result<Package, String> {
    let package: Package = serde_json::from_str(json)
        .map_err(|error| format!("Invalid MariaDB download metadata: {error}"))?;
    let valid_version = package.version.split('.').count() == 3
        && package
            .version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    if !valid_version
        || package.file_name != format!("mariadb-{}-winx64.msi", package.version)
        || package.sha256.len() != 64
        || !package.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(
            "MariaDB download metadata did not contain a valid Windows MSI and SHA-256."
                .to_string(),
        );
    }
    Ok(package)
}

pub(super) fn download_package(
    package: &Package,
    destination: &Path,
) -> Result<InstallOutput, String> {
    let url = format!(
        "https://downloads.mariadb.org/rest-api/mariadb/{}/{}",
        package.version, package.file_name
    );
    let path = destination.to_string_lossy().replace('\'', "''");
    let verify = verify_checksum_script(destination, &package.sha256);
    let script = format!(
        r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
try {{
    Invoke-WebRequest -UseBasicParsing -Uri '{url}' -OutFile '{path}' -TimeoutSec 600
    {verify}
    Write-Output 'MariaDB installer SHA-256 verified.'
}} catch {{
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}}
"#
    );
    run_process(
        "powershell",
        &["-NoProfile", "-Command", &script],
        Duration::from_secs(660),
    )
}

pub(super) fn verify_checksum_script(path: &Path, checksum: &str) -> String {
    let path = path.to_string_lossy().replace('\'', "''");
    format!(
        r#"$stream = [IO.File]::OpenRead('{path}')
$sha = [Security.Cryptography.SHA256]::Create()
try {{
    $hash = [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '')
    if ($hash -ne '{checksum}') {{ throw 'MariaDB installer checksum verification failed.' }}
}} finally {{
    $sha.Dispose()
    $stream.Dispose()
}}"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unverified_or_unexpected_installers() {
        let valid = serde_json::json!({"version": "12.3.3", "file_name": "mariadb-12.3.3-winx64.msi", "sha256": "a".repeat(64)});
        assert!(parse_package(&valid.to_string()).is_ok());
        for (field, value) in [
            ("sha256", ""),
            ("file_name", "../other.msi"),
            ("version", "12.3.3-rc"),
        ] {
            let mut bad = valid.clone();
            bad[field] = value.into();
            assert!(parse_package(&bad.to_string()).is_err());
        }
    }

    #[test]
    #[ignore = "downloads the official MSI without installing it"]
    fn downloads_and_verifies_official_msi() {
        let package = latest_package().expect("official metadata");
        let path =
            std::env::temp_dir().join(format!("fxi-package-test-{}.msi", std::process::id()));
        let output = download_package(&package, &path);
        let size = std::fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let _ = std::fs::remove_file(path);
        let output = output.expect("download task");
        assert!(output.success, "{}", output.stderr);
        assert!(size > 1024 * 1024);
        eprintln!(
            "Verified MariaDB {} Windows MSI ({size} bytes)",
            package.version
        );
    }
}
