use super::install::{run_process, InstallOutput};
use crate::models::mariadb::{MariaDBRelease, MariaDBSeries};
use serde::Deserialize;
use std::{
    collections::HashMap,
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

// Established LTS with Community maintenance until 2029-05-29:
// https://mariadb.org/about/#maintenance-policy
pub const DEFAULT_SERIES: &str = "11.4";
static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Package)>>> = OnceLock::new();

pub(super) fn resolve_package(requested: Option<&str>) -> Result<Package, String> {
    resolve_with_policy(requested.unwrap_or(DEFAULT_SERIES), false)
}

pub(super) fn resolve_update_package(series: &str) -> Result<Package, String> {
    if !valid_series(series) {
        return Err("A known installed series is required for updates.".into());
    }
    resolve_with_policy(series, true)
}

fn resolve_with_policy(requested: &str, allow_rolling: bool) -> Result<Package, String> {
    if !valid_series(requested) && !valid_release(requested) {
        return Err("MariaDB version must be a series (11.4) or exact release (11.4.13).".into());
    }
    let series = requested.split('.').take(2).collect::<Vec<_>>().join(".");
    let mut cached = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "MariaDB package cache is unavailable.".to_string())?;
    let key = format!("{allow_rolling}:{requested}");
    if let Some((time, package)) = cached.get(&key) {
        if time.elapsed() < Duration::from_secs(15 * 60) {
            return Ok(package.clone());
        }
    }
    let script = format!(
        "$requested = '{requested}'\n$series = '{series}'\n$allowRolling = ${allow_rolling}\n$resolveOnly = $true\n{}\n{}",
        include_str!("package-versions.ps1"),
        include_str!("package-metadata.ps1")
    );
    let output = run_process(
        "powershell",
        &["-NoProfile", "-Command", &script],
        Duration::from_secs(75),
    )?;
    if !output.success {
        return Err(format!(
            "Could not resolve the MariaDB installer: {}",
            output.stderr
        ));
    }
    let package = parse_package(&output.stdout)?;
    validate_selection(&package.version, requested)?;
    cached.retain(|_, (time, _)| time.elapsed() < Duration::from_secs(15 * 60));
    if cached.len() >= 32 {
        cached.clear();
    }
    cached.insert(key, (Instant::now(), package.clone()));
    Ok(package)
}

fn valid_parts(value: &str, count: usize) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == count
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 4
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

pub(super) fn valid_series(value: &str) -> bool {
    valid_parts(value, 2)
}
pub(super) fn valid_release(value: &str) -> bool {
    valid_parts(value, 3)
}

fn validate_selection(version: &str, requested: &str) -> Result<(), String> {
    if (valid_release(requested) && version == requested)
        || (valid_series(requested) && version.starts_with(&format!("{requested}.")))
    {
        Ok(())
    } else {
        Err(format!(
            "MariaDB returned {version} instead of {requested}; no substitute will be installed."
        ))
    }
}

fn run_listing<T: serde::de::DeserializeOwned>(series: Option<&str>) -> Result<Vec<T>, String> {
    let script = format!(
        "$series = '{}'\n$resolveOnly = $false\n{}",
        series.unwrap_or_default(),
        include_str!("package-versions.ps1")
    );
    let output = run_process(
        "powershell",
        &["-NoProfile", "-Command", &script],
        Duration::from_secs(75),
    )?;
    if !output.success {
        return Err(format!(
            "Could not load MariaDB versions: {}",
            output.stderr
        ));
    }
    let list: Vec<T> = serde_json::from_str(&output.stdout)
        .map_err(|error| format!("Invalid MariaDB version listing: {error}"))?;
    if list.len() > 256 {
        return Err("MariaDB version listing exceeded its limit.".into());
    }
    Ok(list)
}

pub fn list_series() -> Result<Vec<MariaDBSeries>, String> {
    let list: Vec<MariaDBSeries> = run_listing(None)?;
    if list.iter().any(|item| !valid_series(&item.series)) {
        return Err("Invalid MariaDB series metadata.".into());
    }
    Ok(list)
}

pub fn list_releases(series: &str) -> Result<Vec<MariaDBRelease>, String> {
    if !valid_series(series) {
        return Err("Invalid MariaDB series.".into());
    }
    let list: Vec<MariaDBRelease> = run_listing(Some(series))?;
    for item in &list {
        if !valid_release(&item.version) {
            return Err("Invalid MariaDB release metadata.".into());
        }
        validate_selection(&item.version, series)?;
    }
    Ok(list)
}

fn parse_package(json: &str) -> Result<Package, String> {
    let package: Package = serde_json::from_str(json)
        .map_err(|error| format!("Invalid MariaDB download metadata: {error}"))?;
    if !valid_release(&package.version)
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
}}
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Security\Microsoft.PowerShell.Security.psd1') -ErrorAction Stop
$signature = Get-AuthenticodeSignature -LiteralPath '{path}' -ErrorAction Stop
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '(?:^|,\s*)O=(?:"MariaDB USA, Inc\."|"?MariaDB (?:plc|Corporation(?: Ab)?)"?)(?:,|$)') {{
    throw 'MariaDB installer Authenticode signature or publisher verification failed.'
}}"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_version_inputs_and_exact_selection() {
        for input in [
            "",
            " 11.4",
            "11.4 ",
            "11.4';exit",
            "11.4/../../",
            "011.4",
            "11..4",
            "99999.4",
            "11.4.13-rc",
        ] {
            assert!(!valid_series(input) && !valid_release(input), "{input}");
        }
        assert!(valid_series("11.4"));
        assert!(valid_release("11.4.13"));
        assert!(validate_selection("11.4.13", "11.4").is_ok());
        assert!(validate_selection("11.4.13", "11.4.13").is_ok());
        assert!(validate_selection("11.8.3", "11.4").is_err());
        assert!(validate_selection("11.4.13", "11.4.12").is_err());
    }

    #[test]
    fn verification_checks_both_checksum_and_publisher() {
        let script = verify_checksum_script(Path::new("fixture.msi"), &"a".repeat(64));
        assert!(script.contains("SHA256"));
        assert!(script.contains("Get-AuthenticodeSignature -LiteralPath"));
        assert!(script.contains("$signature.Status -ne 'Valid'"));
        assert!(script.contains("SignerCertificate.Subject"));
    }

    #[cfg(windows)]
    #[test]
    fn metadata_filter_rejects_prereleases_eol_and_missing_msi_without_network() {
        let source = include_str!("package-versions.ps1");
        let fixture = r#"
function Get-MariaDBMetadata([string]$path) {
    if (-not $path) {
        return @{ major_releases = @(
            @{ release_id='11.4'; release_status='Stable'; release_support_type='Long Term Support'; release_eol_date='2099-05-29' },
            @{ release_id='12.2'; release_status='Stable'; release_support_type='Rolling'; release_eol_date=$null },
            @{ release_id='10.6'; release_status='Stable'; release_support_type='Long Term Support'; release_eol_date='2000-01-01' },
            @{ release_id='99.1'; release_status='RC'; release_support_type='Long Term Support'; release_eol_date='2099-01-01' }
        ) }
    }
    $file = @{os='Windows';cpu='x86_64';file_name='mariadb-11.4.13-winx64.msi';checksum=@{sha256sum=('a'*64)}}
    return @{ releases = [pscustomobject]@{
        '11.4.13' = @{ release_id='11.4.13'; release_name='MariaDB Server 11.4.13'; date_of_release='2026-01-01'; files=@($file) }
        '11.4.1' = @{ release_id='11.4.1'; release_name='MariaDB Server 11.4.1 RC'; date_of_release='2024-01-01'; files=@($file) }
        '11.4.14' = @{ release_id='11.4.14'; release_name='MariaDB Server 11.4.14'; date_of_release='2026-01-01'; files=@() }
    } }
}
$series = @(Get-MariaDBSeries)
if ($series.Count -ne 1 -or $series[0].series -ne '11.4') { throw 'series filter failed' }
$updateSeries = @(Get-MariaDBSeries $true)
if ($updateSeries.Count -ne 2 -or -not ($updateSeries | Where-Object { $_.series -eq '12.2' })) { throw 'supported rolling updates were excluded' }
$releases = @(Get-MariaDBReleases '11.4')
if ($releases.Count -ne 1 -or $releases[0].release_id -ne '11.4.13') { throw 'release filter failed' }
Write-Output 'fixture passed'
"#;
        let script = format!("$resolveOnly = $true\n{source}\n{fixture}");
        let result = run_process(
            "powershell",
            &["-NoProfile", "-Command", &script],
            Duration::from_secs(15),
        )
        .unwrap();
        assert!(result.success, "{}", result.stderr);
        assert!(result.stdout.contains("fixture passed"));
    }

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
        let package = resolve_package(None).expect("official metadata");
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
