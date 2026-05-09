use std::process::Command;

use crate::services::mariadb::detect::find_service_name;

pub fn start_service(service_name: Option<String>) -> Result<(), String> {
    control_service("start", service_name)
}

pub fn stop_service(service_name: Option<String>) -> Result<(), String> {
    control_service("stop", service_name)
}

pub fn restart_service(service_name: Option<String>) -> Result<(), String> {
    let service_name = service_name
        .or_else(find_service_name)
        .ok_or_else(missing_service_message)?;
    let _ = run_sc("stop", &service_name);
    run_sc("start", &service_name)
}

fn control_service(action: &str, service_name: Option<String>) -> Result<(), String> {
    let service_name = service_name
        .or_else(find_service_name)
        .ok_or_else(missing_service_message)?;
    run_sc(action, &service_name)
}

fn run_sc(action: &str, service_name: &str) -> Result<(), String> {
    let output = Command::new("sc")
        .args([action, service_name])
        .output()
        .map_err(|error| format!("Failed to run service command: {error}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

fn missing_service_message() -> String {
    "No MariaDB Windows service was found.".to_string()
}
