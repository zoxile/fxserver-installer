use std::{
    path::Path,
    process::Command,
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::{models::mariadb::MariaDBStatus, process::CommandNoWindowExt};

const CACHE_TTL: Duration = Duration::from_secs(60);
static INSTALL_PATH: Mutex<Option<(Instant, String)>> = Mutex::new(None);
static VERSION: Mutex<Option<(Instant, String)>> = Mutex::new(None);

fn cached(slot: &Mutex<Option<(Instant, String)>>) -> Option<String> {
    slot.lock()
        .ok()?
        .as_ref()
        .filter(|(time, _)| time.elapsed() < CACHE_TTL)
        .map(|(_, value)| value.clone())
}

fn remember(slot: &Mutex<Option<(Instant, String)>>, value: Option<String>) -> Option<String> {
    if let Ok(mut slot) = slot.lock() {
        *slot = value.clone().map(|value| (Instant::now(), value));
    }
    value
}

pub fn clear_detection_cache() {
    if let Ok(mut slot) = INSTALL_PATH.lock() {
        *slot = None;
    }
    if let Ok(mut slot) = VERSION.lock() {
        *slot = None;
    }
    super::query::clear_client_cache();
}

pub fn detect_mariadb() -> MariaDBStatus {
    let service = find_service();
    let version = get_version();
    let install_path = get_install_path();

    if let Some((service_name, service_display_name)) = service {
        let running = is_service_running(&service_name);

        return MariaDBStatus {
            installed: true,
            running,
            version,
            service_name: Some(service_name),
            service_display_name: Some(service_display_name),
            install_path,
        };
    }

    MariaDBStatus {
        installed: version.is_some() || install_path.is_some(),
        running: false,
        version,
        service_name: None,
        service_display_name: None,
        install_path,
    }
}

pub fn is_service_running(service_name: &str) -> bool {
    if super::service::validate_service_name(service_name).is_err() {
        return false;
    }
    let output = Command::new("sc")
        .no_window()
        .args(["query", service_name])
        .output();

    output
        .map(|result| {
            result.status.success() && running_from_sc(&String::from_utf8_lossy(&result.stdout))
        })
        .unwrap_or(false)
}

fn running_from_sc(output: &str) -> bool {
    output
        .lines()
        .filter_map(|line| line.split_once(':'))
        .any(|(key, value)| key.trim() == "STATE" && value.split_whitespace().next() == Some("4"))
}

pub fn find_service_name() -> Option<String> {
    find_service().map(|(service_name, _)| service_name)
}

pub fn get_install_path() -> Option<String> {
    if let Some(path) = cached(&INSTALL_PATH).filter(|p| Path::new(p).exists()) {
        return Some(path);
    }
    remember(
        &INSTALL_PATH,
        get_install_path_from_service().or_else(get_install_path_from_registry),
    )
}

fn find_service() -> Option<(String, String)> {
    let output = Command::new("powershell")
        .no_window()
        .args([
            "-NoProfile",
            "-Command",
            r#"Get-CimInstance Win32_Service -ErrorAction Stop | Where-Object { $_.PathName -match '(?i)mariadb' -and $_.PathName -match '(?i)(?:^|[\\/])(?:mariadbd|mysqld)\.exe(?:"|\s|$)' } | Select-Object -First 1 Name,DisplayName | ConvertTo-Json -Compress"#,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return None;
    }

    let service_name = extract_json_string(&stdout, "Name")?;
    let display_name =
        extract_json_string(&stdout, "DisplayName").unwrap_or_else(|| service_name.clone());

    Some((service_name, display_name))
}

fn get_version() -> Option<String> {
    if let Some(version) = cached(&VERSION) {
        return Some(version);
    }
    remember(&VERSION, get_version_uncached())
}

fn get_version_uncached() -> Option<String> {
    if let Some(install_path) = get_install_path() {
        let client_path = Path::new(&install_path).join("bin").join("mariadbd.exe");
        if let Some(version) = run_version_command(client_path.to_string_lossy().as_ref()) {
            return Some(version);
        }
    }

    get_install_path().and_then(|path| {
        run_version_command(&Path::new(&path).join("bin/mysqld.exe").to_string_lossy())
    })
}

fn run_version_command(command: &str) -> Option<String> {
    let output = Command::new(command)
        .no_window()
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.to_lowercase().contains("mariadb") {
        Some(version)
    } else {
        None
    }
}

fn get_install_path_from_registry() -> Option<String> {
    let output = Command::new("powershell")
        .no_window()
        .args([
            "-NoProfile",
            "-Command",
            "$paths = 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*','HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*'; Get-ItemProperty $paths -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like 'MariaDB*' -and $_.InstallLocation } | Sort-Object DisplayVersion -Descending | Select-Object -First 1 -ExpandProperty InstallLocation",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    normalize_existing_path(String::from_utf8_lossy(&output.stdout).trim())
}

fn get_install_path_from_service() -> Option<String> {
    let service_name = find_service_name()?;
    super::service::validate_service_name(&service_name).ok()?;
    let command = format!(
        "(Get-CimInstance Win32_Service -Filter \"Name='{}'\").PathName",
        service_name.replace('\'', "''")
    );
    let output = Command::new("powershell")
        .no_window()
        .args(["-NoProfile", "-Command", &command])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let path_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let executable_path = extract_executable_path(&path_name)?;
    let bin_path = Path::new(&executable_path).parent()?;
    let install_path = if bin_path
        .file_name()?
        .to_string_lossy()
        .eq_ignore_ascii_case("bin")
    {
        bin_path.parent()?
    } else {
        bin_path
    };

    normalize_existing_path(&install_path.to_string_lossy())
}

fn normalize_existing_path(path: &str) -> Option<String> {
    let trimmed = path.trim().trim_matches('"').trim_end_matches('\\');
    if trimmed.is_empty() || !Path::new(trimmed).exists() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn extract_executable_path(path_name: &str) -> Option<String> {
    if let Some(rest) = path_name.strip_prefix('"') {
        return rest.split('"').next().map(str::to_string);
    }

    path_name
        .split_whitespace()
        .next()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()?
        .get(key)?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn running_status_is_not_inferred_from_service_name() {
        assert!(running_from_sc("SERVICE_NAME: MariaDB\n STATE : 4 RUNNING"));
        assert!(!running_from_sc(
            "SERVICE_NAME: MariaDB_RUNNING\n STATE : 1 STOPPED"
        ));
        assert!(!running_from_sc("STATE : 3 STOP_PENDING"));
    }
    #[test]
    fn expired_discovery_is_not_reused() {
        let slot = Mutex::new(Some((Instant::now() - CACHE_TTL, "old".into())));
        assert!(cached(&slot).is_none());
        assert_eq!(remember(&slot, Some("new".into())).as_deref(), Some("new"));
        assert_eq!(cached(&slot).as_deref(), Some("new"));
    }
}
