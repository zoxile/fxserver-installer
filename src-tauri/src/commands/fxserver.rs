use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, SystemTime},
};

use crate::models::fxserver::{
    FxserverEnvironmentVariable, FxserverLaunchRequest, FxserverLaunchResult, FxserverResources,
    FxserverStatus, TxDataLogRequest, TxDataLogResult, TxDataProfilesResult,
};

#[derive(Default)]
pub struct FxserverManager {
    process: Mutex<Option<ManagedFxserverProcess>>,
}

struct ManagedFxserverProcess {
    child: Child,
    artifact_path: PathBuf,
    started_at: SystemTime,
}

#[tauri::command]
pub fn start_fxserver(
    request: FxserverLaunchRequest,
    manager: tauri::State<'_, FxserverManager>,
) -> Result<FxserverLaunchResult, String> {
    if !cfg!(target_os = "windows") {
        return Err(
            "FXServer process management is only supported on Windows right now.".to_string(),
        );
    }

    let mut guard = manager
        .process
        .lock()
        .map_err(|_| "FXServer process state is unavailable.".to_string())?;

    if let Some(process) = guard.as_mut() {
        if process_is_running(process)? {
            return Err("FXServer is already running from this app.".to_string());
        }
        *guard = None;
    }

    let artifact_path = PathBuf::from(request.artifact_path.trim());
    if artifact_path.as_os_str().is_empty() {
        return Err("Choose an FXServer artifact folder before starting the server.".to_string());
    }

    let executable_path = artifact_path.join("FXServer.exe");
    if !executable_path.is_file() {
        return Err("FXServer.exe was not found in the selected artifact folder.".to_string());
    }

    let mut command = Command::new(&executable_path);
    command
        .current_dir(&artifact_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    for variable in sanitize_environment(request.environment)? {
        command.env(variable.key, variable.value);
    }

    if let Some(profile) = request.server_profile.map(|value| value.trim().to_string()) {
        if !profile.is_empty() {
            command.arg("+set").arg("serverProfile").arg(profile);
        }
    }

    let child = command
        .spawn()
        .map_err(|error| format!("Failed to start FXServer.exe: {error}"))?;
    let pid = child.id();
    let started_at = SystemTime::now();
    let started_at_label = system_time_to_label(started_at);

    *guard = Some(ManagedFxserverProcess {
        child,
        artifact_path: artifact_path.clone(),
        started_at,
    });

    Ok(FxserverLaunchResult {
        pid,
        artifact_path: artifact_path.to_string_lossy().to_string(),
        started_at: started_at_label,
    })
}

#[tauri::command]
pub fn stop_fxserver(manager: tauri::State<'_, FxserverManager>) -> Result<(), String> {
    let mut guard = manager
        .process
        .lock()
        .map_err(|_| "FXServer process state is unavailable.".to_string())?;

    let Some(mut process) = guard.take() else {
        return Ok(());
    };

    if process
        .child
        .try_wait()
        .map_err(|error| format!("Failed to inspect FXServer: {error}"))?
        .is_none()
    {
        process
            .child
            .kill()
            .map_err(|error| format!("Failed to stop FXServer: {error}"))?;
        let _ = process.child.wait();
    }

    Ok(())
}

#[tauri::command]
pub fn get_fxserver_status(
    manager: tauri::State<'_, FxserverManager>,
) -> Result<FxserverStatus, String> {
    let mut guard = manager
        .process
        .lock()
        .map_err(|_| "FXServer process state is unavailable.".to_string())?;

    let Some(process) = guard.as_mut() else {
        return Ok(FxserverStatus {
            running: false,
            pid: None,
            artifact_path: None,
            started_at: None,
            uptime_seconds: None,
            resources: None,
        });
    };

    if !process_is_running(process)? {
        *guard = None;
        return Ok(FxserverStatus {
            running: false,
            pid: None,
            artifact_path: None,
            started_at: None,
            uptime_seconds: None,
            resources: None,
        });
    }

    let pid = process.child.id();
    let uptime_seconds = process
        .started_at
        .elapsed()
        .unwrap_or(Duration::from_secs(0))
        .as_secs();

    Ok(FxserverStatus {
        running: true,
        pid: Some(pid),
        artifact_path: Some(process.artifact_path.to_string_lossy().to_string()),
        started_at: Some(system_time_to_label(process.started_at)),
        uptime_seconds: Some(uptime_seconds),
        resources: read_process_resources(pid),
    })
}

#[tauri::command]
pub fn read_txdata_log(request: TxDataLogRequest) -> Result<TxDataLogResult, String> {
    let log_name = request.log_name.trim();
    if !matches!(log_name, "fxserver.log" | "admin.log" | "server.log") {
        return Err("Only fxserver.log, admin.log, and server.log can be opened.".to_string());
    }

    let data_path = PathBuf::from(request.data_path.trim());
    if data_path.as_os_str().is_empty() {
        return Err("Set TXHOST_DATA_PATH before opening txData logs.".to_string());
    }

    let profile = request.profile.unwrap_or_default();
    let profile = profile.trim();
    let max_lines = request.max_lines.unwrap_or(500).clamp(50, 5000);
    let log_path = resolve_log_path(data_path, profile, log_name);

    let content = fs::read_to_string(&log_path)
        .map_err(|error| format!("Failed to read {}: {error}", log_path.to_string_lossy()))?;
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    let tailed = lines[start..].join("\n");

    Ok(TxDataLogResult {
        path: log_path.to_string_lossy().to_string(),
        log_name: log_name.to_string(),
        content: tailed,
        line_count: lines.len(),
    })
}

#[tauri::command]
pub fn list_txdata_profiles(data_path: String) -> Result<TxDataProfilesResult, String> {
    let data_path = PathBuf::from(data_path.trim());
    if data_path.as_os_str().is_empty() {
        return Err("Choose a txData folder before scanning profiles.".to_string());
    }

    let entries = fs::read_dir(&data_path)
        .map_err(|error| format!("Failed to inspect {}: {error}", data_path.to_string_lossy()))?;
    let mut profiles = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| format!("Failed to inspect txData profile: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Failed to inspect txData profile type: {error}"))?;

        if !file_type.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if name.eq_ignore_ascii_case("logs") || name.starts_with('.') {
            continue;
        }

        profiles.push(name);
    }

    profiles.sort_by_key(|profile| profile.to_ascii_lowercase());

    Ok(TxDataProfilesResult {
        data_path: data_path.to_string_lossy().to_string(),
        profiles,
        has_root_logs: data_path.join("logs").is_dir(),
    })
}

fn process_is_running(process: &mut ManagedFxserverProcess) -> Result<bool, String> {
    Ok(process
        .child
        .try_wait()
        .map_err(|error| format!("Failed to inspect FXServer: {error}"))?
        .is_none())
}

fn resolve_log_path(data_path: PathBuf, profile: &str, log_name: &str) -> PathBuf {
    let direct_logs = data_path.join("logs").join(log_name);
    if direct_logs.is_file() || profile.is_empty() {
        return direct_logs;
    }

    data_path.join(profile).join("logs").join(log_name)
}

fn sanitize_environment(
    variables: Vec<FxserverEnvironmentVariable>,
) -> Result<Vec<FxserverEnvironmentVariable>, String> {
    let mut sanitized = Vec::new();

    for variable in variables {
        let key = variable.key.trim().to_ascii_uppercase();
        let value = variable.value.trim().to_string();

        if key.is_empty() || value.is_empty() {
            continue;
        }

        if !key.starts_with("TXHOST_") {
            return Err(format!(
                "{key} is not a supported TXHOST environment variable."
            ));
        }

        if key.chars().any(|character| {
            !(character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_')
        }) {
            return Err(format!(
                "{key} contains invalid environment variable characters."
            ));
        }

        sanitized.push(FxserverEnvironmentVariable { key, value });
    }

    Ok(sanitized)
}

fn system_time_to_label(value: SystemTime) -> String {
    value
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

#[cfg(target_os = "windows")]
fn read_process_resources(pid: u32) -> Option<FxserverResources> {
    let script = format!(
        r#"
$ErrorActionPreference = "Stop"
$pidValue = {pid}
$proc = Get-Process -Id $pidValue -ErrorAction Stop
$os = Get-CimInstance Win32_OperatingSystem
$perf = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object {{ $_.IDProcess -eq $pidValue }} | Select-Object -First 1
$logicalProcessors = [Environment]::ProcessorCount
$rawCpu = if ($perf -and $null -ne $perf.PercentProcessorTime) {{ [double]$perf.PercentProcessorTime }} else {{ 0 }}
$cpu = if ($logicalProcessors -gt 0) {{ [Math]::Min(100, $rawCpu / $logicalProcessors) }} else {{ [Math]::Min(100, $rawCpu) }}
$totalMemory = [uint64]$os.TotalVisibleMemorySize * 1024
$memory = [uint64]$proc.WorkingSet64
$memoryPercent = if ($totalMemory -gt 0) {{ ($memory / $totalMemory) * 100 }} else {{ 0 }}
[pscustomobject]@{{
    CpuPercent = [Math]::Round($cpu, 2)
    MemoryBytes = $memory
    TotalMemoryBytes = $totalMemory
    MemoryPercent = [Math]::Round($memoryPercent, 2)
    ThreadCount = $proc.Threads.Count
    HandleCount = $proc.HandleCount
}} | ConvertTo-Json -Compress
"#
    );

    let output = Command::new("powershell")
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

    Some(FxserverResources {
        cpu_percent: number_from_json(value.get("CpuPercent")).unwrap_or(0.0),
        memory_bytes: integer_from_json(value.get("MemoryBytes")).unwrap_or(0),
        total_memory_bytes: integer_from_json(value.get("TotalMemoryBytes")).unwrap_or(0),
        memory_percent: number_from_json(value.get("MemoryPercent")).unwrap_or(0.0),
        thread_count: integer_from_json(value.get("ThreadCount")).unwrap_or(0) as u32,
        handle_count: integer_from_json(value.get("HandleCount")).unwrap_or(0) as u32,
    })
}

#[cfg(not(target_os = "windows"))]
fn read_process_resources(_pid: u32) -> Option<FxserverResources> {
    None
}

fn number_from_json(value: Option<&serde_json::Value>) -> Option<f64> {
    value.and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str()?.parse::<f64>().ok())
    })
}

fn integer_from_json(value: Option<&serde_json::Value>) -> Option<u64> {
    value.and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str()?.parse::<u64>().ok())
    })
}
