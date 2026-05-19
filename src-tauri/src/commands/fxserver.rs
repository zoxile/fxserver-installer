use std::{
    fs,
    io::{BufRead, BufReader},
    net::UdpSocket,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime},
};

use crate::models::fxserver::{
    FxserverCommandRequest, FxserverEnvironmentVariable, FxserverLaunchRequest,
    FxserverLaunchResult, FxserverRconConfig, FxserverResources, FxserverStatus,
    FxserverTerminalEntry, FxserverTerminalResult, SaveServerConfigRequest, ServerConfigFile,
    ServerConfigRequest, ServerConfigResult, TxDataLogRequest, TxDataLogResult,
    TxDataProfilesResult,
};

pub struct FxserverManager {
    process: Mutex<Option<ManagedFxserverProcess>>,
    terminal: Arc<Mutex<TerminalState>>,
}

struct ManagedFxserverProcess {
    child: Child,
    artifact_path: PathBuf,
    started_at: SystemTime,
    resource_sample: Option<ResourceSample>,
}

#[derive(Default)]
struct TerminalState {
    entries: Vec<FxserverTerminalEntry>,
    next_id: u64,
}

#[derive(Clone, Copy)]
struct ResourceSample {
    cpu_seconds: f64,
    sampled_at: Instant,
}

impl Default for FxserverManager {
    fn default() -> Self {
        Self {
            process: Mutex::new(None),
            terminal: Arc::new(Mutex::new(TerminalState::default())),
        }
    }
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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for variable in sanitize_environment(request.environment)? {
        command.env(variable.key, variable.value);
    }

    if let Some(profile) = request.server_profile.map(|value| value.trim().to_string()) {
        if !profile.is_empty() {
            command.arg("+set").arg("serverProfile").arg(profile);
        }
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("Failed to start FXServer.exe: {error}"))?;
    let pid = child.id();
    let started_at = SystemTime::now();
    let started_at_label = system_time_to_label(started_at);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    clear_terminal(&manager.terminal)?;
    append_terminal_line(
        &manager.terminal,
        "system",
        format!(
            "Started FXServer.exe from {}",
            artifact_path.to_string_lossy()
        ),
    )?;

    if let Some(stdout) = stdout {
        spawn_terminal_reader(manager.terminal.clone(), "stdout", stdout);
    }

    if let Some(stderr) = stderr {
        spawn_terminal_reader(manager.terminal.clone(), "stderr", stderr);
    }

    *guard = Some(ManagedFxserverProcess {
        child,
        artifact_path: artifact_path.clone(),
        started_at,
        resource_sample: None,
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

    append_terminal_line(&manager.terminal, "system", "FXServer stopped.")?;

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
        resources: read_process_resources(pid, &mut process.resource_sample),
    })
}

#[tauri::command]
pub fn get_fxserver_terminal(
    max_lines: Option<usize>,
    manager: tauri::State<'_, FxserverManager>,
) -> Result<FxserverTerminalResult, String> {
    let max_lines = max_lines.unwrap_or(500).clamp(50, 5000);
    let terminal = manager
        .terminal
        .lock()
        .map_err(|_| "FXServer terminal output is unavailable.".to_string())?;
    let start = terminal.entries.len().saturating_sub(max_lines);

    Ok(FxserverTerminalResult {
        entries: terminal.entries[start..].to_vec(),
    })
}

#[tauri::command]
pub fn send_fxserver_command(
    request: FxserverCommandRequest,
    manager: tauri::State<'_, FxserverManager>,
) -> Result<(), String> {
    let command = request.command.trim();
    if command.is_empty() {
        return Ok(());
    }

    let mut guard = manager
        .process
        .lock()
        .map_err(|_| "FXServer process state is unavailable.".to_string())?;

    let Some(process) = guard.as_mut() else {
        return Err("FXServer is not running from this app.".to_string());
    };

    if !process_is_running(process)? {
        *guard = None;
        return Err("FXServer is not running from this app.".to_string());
    }

    append_terminal_line(&manager.terminal, "command", format!("rcon> {command}"))?;

    let response = send_rcon_command(&request.rcon, command)?;

    if response.trim().is_empty() {
        append_terminal_line(&manager.terminal, "system", "RCON command accepted.")?;
    } else {
        for line in response.lines() {
            append_terminal_line(&manager.terminal, "rcon", line)?;
        }
    }

    Ok(())
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

#[tauri::command]
pub fn read_server_config(request: ServerConfigRequest) -> Result<ServerConfigResult, String> {
    let tx_data_path = PathBuf::from(request.tx_data_path.trim());
    if tx_data_path.as_os_str().is_empty() {
        return Err("Set TXHOST_DATA_PATH before configuring server files.".to_string());
    }

    let profile = request.profile.trim();
    if profile.is_empty() {
        return Err("Choose a txData profile before configuring server files.".to_string());
    }

    let profile_config_path = tx_data_path.join(profile).join("config.json");
    let profile_config = fs::read_to_string(&profile_config_path).map_err(|error| {
        format!(
            "Failed to read txData profile config {}: {error}",
            profile_config_path.to_string_lossy()
        )
    })?;
    let profile_config: serde_json::Value =
        serde_json::from_str(&profile_config).map_err(|error| {
            format!(
                "Failed to parse txData profile config {}: {error}",
                profile_config_path.to_string_lossy()
            )
        })?;
    let data_path = profile_config
        .get("dataPath")
        .or_else(|| {
            profile_config
                .get("server")
                .and_then(|server| server.get("dataPath"))
        })
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            format!(
                "txData profile config {} does not include a dataPath value.",
                profile_config_path.to_string_lossy()
            )
        })?;

    if !data_path.is_dir() {
        return Err(format!(
            "Configured server data path was not found: {}",
            data_path.to_string_lossy()
        ));
    }

    let files = read_cfg_files(&data_path)?;
    let rcon = find_rcon_password(&files);
    let rconlog = find_rconlog(&files);

    Ok(ServerConfigResult {
        tx_data_path: tx_data_path.to_string_lossy().to_string(),
        profile: profile.to_string(),
        profile_config_path: profile_config_path.to_string_lossy().to_string(),
        data_path: data_path.to_string_lossy().to_string(),
        files,
        rcon_password_found: rcon.is_some(),
        rcon_password_file: rcon.as_ref().map(|(file, _)| file.clone()),
        rcon_password_line: rcon.map(|(_, line)| line),
        rconlog_found: rconlog.is_some(),
        rconlog_line: rconlog.map(|(_, line)| line),
    })
}

#[tauri::command]
pub fn save_server_config(request: SaveServerConfigRequest) -> Result<ServerConfigFile, String> {
    let path = PathBuf::from(request.path.trim());
    if path.as_os_str().is_empty() {
        return Err("Choose a config file before saving.".to_string());
    }

    if !path.is_file() || !is_cfg_file(&path) {
        return Err("Only existing .cfg files can be saved from this editor.".to_string());
    }

    fs::write(&path, request.content)
        .map_err(|error| format!("Failed to save {}: {error}", path.to_string_lossy()))?;
    read_cfg_file(&path)
}

fn process_is_running(process: &mut ManagedFxserverProcess) -> Result<bool, String> {
    Ok(process
        .child
        .try_wait()
        .map_err(|error| format!("Failed to inspect FXServer: {error}"))?
        .is_none())
}

fn clear_terminal(terminal: &Arc<Mutex<TerminalState>>) -> Result<(), String> {
    let mut terminal = terminal
        .lock()
        .map_err(|_| "FXServer terminal output is unavailable.".to_string())?;
    terminal.entries.clear();
    terminal.next_id = 0;
    Ok(())
}

fn append_terminal_line(
    terminal: &Arc<Mutex<TerminalState>>,
    stream: &str,
    line: impl Into<String>,
) -> Result<(), String> {
    let mut terminal = terminal
        .lock()
        .map_err(|_| "FXServer terminal output is unavailable.".to_string())?;
    let entry = FxserverTerminalEntry {
        id: terminal.next_id,
        stream: stream.to_string(),
        line: line.into(),
        timestamp: system_time_to_label(SystemTime::now()),
    };
    terminal.next_id += 1;
    terminal.entries.push(entry);

    if terminal.entries.len() > 5000 {
        let overflow = terminal.entries.len() - 5000;
        terminal.entries.drain(0..overflow);
    }

    Ok(())
}

fn spawn_terminal_reader<R>(terminal: Arc<Mutex<TerminalState>>, stream: &'static str, reader: R)
where
    R: std::io::Read + Send + 'static,
{
    thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let _ = append_terminal_line(&terminal, stream, line);
                }
                Err(error) => {
                    let _ = append_terminal_line(
                        &terminal,
                        "system",
                        format!("Stopped reading {stream}: {error}"),
                    );
                    break;
                }
            }
        }
    });
}

fn resolve_log_path(data_path: PathBuf, profile: &str, log_name: &str) -> PathBuf {
    let direct_logs = data_path.join("logs").join(log_name);
    if direct_logs.is_file() || profile.is_empty() {
        return direct_logs;
    }

    data_path.join(profile).join("logs").join(log_name)
}

fn read_cfg_files(data_path: &Path) -> Result<Vec<ServerConfigFile>, String> {
    let entries = fs::read_dir(data_path).map_err(|error| {
        format!(
            "Failed to inspect server data path {}: {error}",
            data_path.to_string_lossy()
        )
    })?;
    let mut paths = Vec::new();

    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Failed to inspect server config file: {error}"))?;
        let path = entry.path();
        if path.is_file() && is_cfg_file(&path) {
            paths.push(path);
        }
    }

    paths.sort_by(|left, right| {
        cfg_sort_rank(left)
            .cmp(&cfg_sort_rank(right))
            .then_with(|| cfg_name(left).cmp(&cfg_name(right)))
    });

    paths.into_iter().map(|path| read_cfg_file(&path)).collect()
}

fn read_cfg_file(path: &Path) -> Result<ServerConfigFile, String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("Failed to read {}: {error}", path.to_string_lossy()))?;
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Failed to inspect {}: {error}", path.to_string_lossy()))?;

    Ok(ServerConfigFile {
        name: cfg_name(path),
        path: path.to_string_lossy().to_string(),
        has_rcon_password: cfg_has_rcon_password(&content),
        has_rconlog: cfg_has_rconlog(&content),
        content,
        size: metadata.len(),
        modified: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs()),
    })
}

fn is_cfg_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("cfg"))
        .unwrap_or(false)
}

fn cfg_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn cfg_sort_rank(path: &Path) -> usize {
    match cfg_name(path).to_ascii_lowercase().as_str() {
        "server.cfg" => 0,
        "permissions.cfg" => 1,
        "misc.cfg" => 2,
        "ox.cfg" => 3,
        "voice.cfg" => 4,
        _ => 10,
    }
}

fn cfg_has_rcon_password(content: &str) -> bool {
    content.lines().any(is_rcon_password_line)
}

fn cfg_has_rconlog(content: &str) -> bool {
    content.lines().any(is_rconlog_line)
}

fn find_rcon_password(files: &[ServerConfigFile]) -> Option<(String, usize)> {
    files
        .iter()
        .filter(|file| file.name.eq_ignore_ascii_case("server.cfg"))
        .find_map(|file| {
            file.content
                .lines()
                .position(is_rcon_password_line)
                .map(|index| (file.name.clone(), index + 1))
        })
}

fn is_rcon_password_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') || trimmed.starts_with("//") {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("rcon_password") || lower.starts_with("set rcon_password")
}

fn find_rconlog(files: &[ServerConfigFile]) -> Option<(String, usize)> {
    files
        .iter()
        .filter(|file| file.name.eq_ignore_ascii_case("server.cfg"))
        .find_map(|file| {
            file.content
                .lines()
                .position(is_rconlog_line)
                .map(|index| (file.name.clone(), index + 1))
        })
}

fn is_rconlog_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') || trimmed.starts_with("//") {
        return false;
    }

    let mut parts = trimmed.split_whitespace();
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(command), Some(resource), None)
            if command.eq_ignore_ascii_case("ensure") && resource.eq_ignore_ascii_case("rconlog")
    )
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
fn read_process_resources(
    pid: u32,
    previous_sample: &mut Option<ResourceSample>,
) -> Option<FxserverResources> {
    let script = format!(
        r#"
$ErrorActionPreference = "Stop"

$rootPid = {pid}

function Get-ChildProcessIds {{
    param([UInt32[]]$ParentIds)

    $all = @()
    $frontier = $ParentIds

    while ($frontier.Count -gt 0) {{
        $children = Get-CimInstance Win32_Process |
            Where-Object {{ $frontier -contains [uint32]$_.ParentProcessId }} |
            Select-Object -ExpandProperty ProcessId

        $children = @($children | Where-Object {{ $null -ne $_ }})

        if ($children.Count -eq 0) {{
            break
        }}

        $all += $children
        $frontier = $children
    }}

    return $all
}}

$pids = @($rootPid) + (Get-ChildProcessIds -ParentIds @($rootPid))
$pids = $pids | Select-Object -Unique

function Get-FxServerProcesses {{
    param($ProcessIds)

    $items = @()

    foreach ($id in $ProcessIds) {{
        $p = Get-Process -Id $id -ErrorAction SilentlyContinue

        if ($p -and $p.ProcessName -eq "FXServer") {{
            $items += $p
        }}
    }}

    return $items
}}

$procs = Get-FxServerProcesses -ProcessIds $pids

$cpuSeconds = ($procs | Measure-Object -Property CPU -Sum).Sum
if ($null -eq $cpuSeconds) {{ $cpuSeconds = 0 }}

$os = Get-CimInstance Win32_OperatingSystem

$totalMemory = [uint64]$os.TotalVisibleMemorySize * 1024

# Real resident RAM usage (matches actual system memory pressure)
$memory = [uint64](($procs | Measure-Object -Property WorkingSet64 -Sum).Sum)

$threads = [uint32]((
    $procs |
    ForEach-Object {{ $_.Threads.Count }} |
    Measure-Object -Sum
).Sum)

$handles = [uint32]((
    $procs |
    Measure-Object -Property HandleCount -Sum
).Sum)

$memoryPercent = 0

if ($totalMemory -gt 0) {{
    $memoryPercent = ($memory / $totalMemory) * 100
}}

[pscustomobject]@{{
    CpuSeconds = [Math]::Round($cpuSeconds, 4)
    MemoryBytes = $memory
    TotalMemoryBytes = $totalMemory
    MemoryPercent = [Math]::Round($memoryPercent, 2)
    ThreadCount = $threads
    HandleCount = $handles
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
    let current_sample = ResourceSample {
        cpu_seconds: number_from_json(value.get("CpuSeconds")).unwrap_or(0.0),
        sampled_at: Instant::now(),
    };
    let cpu_percent = previous_sample
        .and_then(|previous| {
            let elapsed = current_sample
                .sampled_at
                .duration_since(previous.sampled_at)
                .as_secs_f64();
            if elapsed <= 0.0 {
                return None;
            }
            let cpu_delta = current_sample.cpu_seconds - previous.cpu_seconds;
            if cpu_delta < 0.0 {
                return None;
            }

            let logical_processors = std::thread::available_parallelism()
                .map(|value| value.get() as f64)
                .unwrap_or(1.0)
                .max(1.0);
            Some(((cpu_delta / elapsed) * 100.0 / logical_processors).clamp(0.0, 100.0))
        })
        .unwrap_or(0.0);
    *previous_sample = Some(current_sample);

    Some(FxserverResources {
        cpu_percent: (cpu_percent * 100.0).round() / 100.0,

        memory_bytes: integer_from_json(value.get("MemoryBytes")).unwrap_or(0),

        total_memory_bytes: integer_from_json(value.get("TotalMemoryBytes")).unwrap_or(0),

        memory_percent: number_from_json(value.get("MemoryPercent")).unwrap_or(0.0),

        thread_count: integer_from_json(value.get("ThreadCount")).unwrap_or(0) as u32,

        handle_count: integer_from_json(value.get("HandleCount")).unwrap_or(0) as u32,
    })
}

#[cfg(not(target_os = "windows"))]
fn read_process_resources(
    _pid: u32,
    _previous_sample: &mut Option<ResourceSample>,
) -> Option<FxserverResources> {
    None
}

fn send_rcon_command(config: &FxserverRconConfig, command: &str) -> Result<String, String> {
    let host = config.host.trim();
    let password = config.password.trim();
    let command = command.trim();

    if host.is_empty() {
        return Err("Set an RCON host before sending console commands.".to_string());
    }

    if password.is_empty() {
        return Err("Set the server rcon_password before sending console commands.".to_string());
    }

    if command.is_empty() {
        return Err("Type a command before sending RCON input.".to_string());
    }

    let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|error| format!("Failed to open local RCON UDP socket: {error}"))?;
    socket
        .set_read_timeout(Some(rcon_response_timeout()))
        .map_err(|error| format!("Failed to configure RCON read timeout: {error}"))?;
    socket
        .set_write_timeout(Some(Duration::from_secs(4)))
        .map_err(|error| format!("Failed to configure RCON write timeout: {error}"))?;

    let address = format!("{host}:{}", config.port);
    let mut packet = Vec::with_capacity(password.len() + command.len() + 12);
    packet.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    packet.extend_from_slice(format!("rcon {password} {command}").as_bytes());
    socket.send_to(&packet, &address).map_err(|error| {
        format!(
            "Failed to send RCON command to {host}:{}: {error}",
            config.port
        )
    })?;

    let mut responses = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match socket.recv_from(&mut buffer) {
            Ok((length, _)) => {
                let response = parse_quake_rcon_response(&buffer[..length]);
                if !response.trim().is_empty() {
                    responses.push(response);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error) => return Err(format!("Failed to read RCON response: {error}")),
        }
    }

    let response = responses.join("\n").trim().to_string();
    if response.to_ascii_lowercase().contains("bad rconpassword") {
        return Err("RCON authentication failed. Check rcon_password.".to_string());
    }

    Ok(response)
}

fn parse_quake_rcon_response(packet: &[u8]) -> String {
    let payload = packet
        .strip_prefix(&[0xff, 0xff, 0xff, 0xff])
        .unwrap_or(packet);
    String::from_utf8_lossy(payload)
        .trim_start_matches("print\n")
        .trim_start_matches("print")
        .trim_matches(char::from(0))
        .trim()
        .to_string()
}

#[cfg(not(test))]
fn rcon_response_timeout() -> Duration {
    Duration::from_secs(4)
}

#[cfg(test)]
fn rcon_response_timeout() -> Duration {
    Duration::from_millis(100)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn quake_rcon_sends_udp_packet_and_reads_response() {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("mock rcon bind");
        let port = socket.local_addr().expect("mock rcon address").port();
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            let mut buffer = [0_u8; 1024];
            let (length, peer) = socket.recv_from(&mut buffer).expect("rcon request");
            sender
                .send(buffer[..length].to_vec())
                .expect("request sent");
            socket
                .send_to(b"\xff\xff\xff\xffprint\ncommand ran\n", peer)
                .expect("rcon response");
        });

        let response = send_rcon_command(
            &FxserverRconConfig {
                host: "127.0.0.1".to_string(),
                port,
                password: "secret".to_string(),
            },
            "say hello",
        )
        .expect("rcon command");

        assert_eq!(
            receiver.recv().expect("request"),
            b"\xff\xff\xff\xffrcon secret say hello"
        );
        assert_eq!(response, "command ran");
    }

    #[test]
    fn quake_rcon_reports_bad_password_response() {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("mock rcon bind");
        let port = socket.local_addr().expect("mock rcon address").port();

        thread::spawn(move || {
            let mut buffer = [0_u8; 1024];
            let (_, peer) = socket.recv_from(&mut buffer).expect("rcon request");
            socket
                .send_to(b"\xff\xff\xff\xffprint\nBad rconpassword.\n", peer)
                .expect("rcon response");
        });

        let error = send_rcon_command(
            &FxserverRconConfig {
                host: "127.0.0.1".to_string(),
                port,
                password: "wrong".to_string(),
            },
            "status",
        )
        .expect_err("bad password");

        assert!(error.contains("RCON authentication failed"));
    }
}
