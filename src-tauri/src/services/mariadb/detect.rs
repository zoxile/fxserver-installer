use std::{path::Path, process::Command};

use crate::models::mariadb::MariaDBStatus;

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
    let output = Command::new("sc").args(["query", service_name]).output();

    output
        .map(|result| String::from_utf8_lossy(&result.stdout).contains("RUNNING"))
        .unwrap_or(false)
}

pub fn find_service_name() -> Option<String> {
    find_service().map(|(service_name, _)| service_name)
}

pub fn get_install_path() -> Option<String> {
    get_install_path_from_registry().or_else(get_install_path_from_service)
}

fn find_service() -> Option<(String, String)> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-Service | Where-Object { $_.Name -like 'MariaDB*' -or $_.DisplayName -like 'MariaDB*' } | Select-Object -First 1 Name,DisplayName | ConvertTo-Json -Compress",
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
    if let Some(install_path) = get_install_path() {
        let client_path = Path::new(&install_path).join("bin").join("mariadb.exe");
        if let Some(version) = run_version_command(client_path.to_string_lossy().as_ref()) {
            return Some(version);
        }
    }

    run_version_command("mariadb").or_else(|| run_version_command("mariadb.exe"))
}

fn run_version_command(command: &str) -> Option<String> {
    let output = Command::new(command).arg("--version").output().ok()?;
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
    let command = format!(
        "(Get-CimInstance Win32_Service -Filter \"Name='{}'\").PathName",
        service_name.replace('\'', "''")
    );
    let output = Command::new("powershell")
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
    let marker = format!("\"{key}\":\"");
    let start = json.find(&marker)? + marker.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].replace("\\\"", "\""))
}
