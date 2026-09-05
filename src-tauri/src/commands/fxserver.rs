use std::{
    collections::VecDeque,
    env, fs,
    io::{BufRead, BufReader, Write},
    net::UdpSocket,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use tauri::{AppHandle, Emitter};

use crate::{
    models::fxserver::{
        FxserverCommandRequest, FxserverEnvironmentVariable, FxserverLaunchRequest,
        FxserverLaunchResult, FxserverRconConfig, FxserverResourceInfo, FxserverResources,
        FxserverStatus, FxserverTerminalEntry, FxserverTerminalResult, FxserverTerminalSegment,
        ResourceScanRequest, ResourceScanResult, SaveServerConfigRequest, ServerConfigFile,
        ServerConfigRequest, ServerConfigResult, TxDataLogRequest, TxDataLogResult,
        TxDataProfilesResult,
    },
    process::CommandNoWindowExt,
    FXSERVER_WATCHDOG_ARG,
};

const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(4);
const FORCE_STOP_WAIT_TIMEOUT: Duration = Duration::from_secs(3);
static RCON_PASSWORD_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone)]
pub struct FxserverManager {
    process: Arc<Mutex<Option<ManagedFxserverProcess>>>,
    lifecycle: Arc<Mutex<()>>,
    terminal: Arc<Mutex<TerminalState>>,
    launch_intent: Arc<Mutex<LaunchIntent>>,
    shutting_down: Arc<AtomicBool>,
}

#[derive(Default)]
struct LaunchIntent {
    generation: u64,
    expected_running: bool,
    launch: Option<SavedLaunch>,
}

#[derive(Clone)]
struct SavedLaunch {
    artifact_path: String,
    environment: Vec<FxserverEnvironmentVariable>,
    server_profile: Option<String>,
}

impl SavedLaunch {
    fn from_request(request: &FxserverLaunchRequest) -> Self {
        Self {
            artifact_path: request.artifact_path.clone(),
            environment: request.environment.clone(),
            server_profile: request.server_profile.clone(),
        }
    }

    fn into_request(self) -> FxserverLaunchRequest {
        FxserverLaunchRequest {
            artifact_path: self.artifact_path,
            environment: self.environment,
            server_profile: self.server_profile,
        }
    }
}

#[derive(Default)]
pub(crate) struct HealthResourceSampler {
    pid: Option<u32>,
    previous: Option<ResourceSample>,
}

pub(crate) struct HealthProcessSample {
    pub generation: u64,
    pub expected_running: bool,
    pub running: bool,
    pub pid: Option<u32>,
    pub resources: Option<FxserverResources>,
}

pub(crate) enum RecoveryOutcome {
    Busy,
    Cancelled,
    Started,
    Failed(String),
}

struct ManagedFxserverProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    artifact_path: PathBuf,
    started_at: SystemTime,
    resource_sample: Option<ResourceSample>,
    _cleanup_job: Option<Arc<ProcessCleanupJob>>,
}

#[derive(Default)]
struct TerminalState {
    entries: Vec<FxserverTerminalEntry>,
    incidents: VecDeque<ConsoleIncident>,
    workspace_id: String,
    next_id: u64,
    generation: u64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConsoleIncident {
    id: u64,
    workspace_id: String,
    timestamp: u64,
    level: &'static str,
    message: String,
}

#[cfg(target_os = "windows")]
struct ProcessCleanupJob(isize);

#[cfg(target_os = "windows")]
impl ProcessCleanupJob {
    fn handle(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.0 as windows_sys::Win32::Foundation::HANDLE
    }
}

#[cfg(target_os = "windows")]
impl Drop for ProcessCleanupJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle());
        }
    }
}

#[cfg(not(target_os = "windows"))]
struct ProcessCleanupJob;

#[derive(Clone, Copy)]
struct ResourceSample {
    cpu_seconds: f64,
    sampled_at: Instant,
}

#[cfg(target_os = "windows")]
struct WindowsProcessInfo {
    pid: u32,
    exe_name: String,
    thread_count: u32,
}

impl Default for FxserverManager {
    fn default() -> Self {
        Self {
            process: Arc::new(Mutex::new(None)),
            lifecycle: Arc::new(Mutex::new(())),
            terminal: Arc::new(Mutex::new(TerminalState::default())),
            launch_intent: Arc::new(Mutex::new(LaunchIntent::default())),
            shutting_down: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl FxserverManager {
    pub(crate) fn set_incident_workspace(&self, workspace_id: &str) -> Result<(), String> {
        self.terminal
            .lock()
            .map_err(|_| "FXServer terminal is unavailable.")?
            .workspace_id = workspace_id.to_string();
        Ok(())
    }

    pub fn start_incident_events(&self, app: AppHandle) {
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if manager.shutting_down.load(Ordering::Acquire) {
                    break;
                }
                let batch = manager
                    .terminal
                    .try_lock()
                    .ok()
                    .map(|mut state| state.incidents.drain(..).collect::<Vec<_>>())
                    .unwrap_or_default();
                if !batch.is_empty() {
                    let _ = app.emit("fxserver-console-incidents", batch);
                }
            }
        });
    }

    pub fn stop_running_process(&self) -> Result<(), String> {
        self.disarm_recovery();
        let _operation = self
            .lifecycle
            .lock()
            .map_err(|_| "FXServer lifecycle is unavailable.".to_string())?;
        self.stop_process()
    }

    pub fn begin_shutdown(&self) {
        self.shutting_down.store(true, Ordering::Release);
        self.disarm_recovery();
    }

    pub fn disarm_recovery(&self) {
        if let Ok(mut intent) = self.launch_intent.lock() {
            intent.expected_running = false;
            intent.launch = None;
            intent.generation = intent.generation.wrapping_add(1);
        }
    }

    fn launch_generation(&self) -> Result<u64, String> {
        self.launch_intent
            .lock()
            .map(|intent| intent.generation)
            .map_err(|_| "FXServer launch settings are unavailable.".to_string())
    }

    fn remember_launch(&self, launch: SavedLaunch, generation: u64) -> Result<bool, String> {
        let mut intent = self
            .launch_intent
            .lock()
            .map_err(|_| "FXServer launch settings are unavailable.".to_string())?;
        if intent.generation != generation || self.shutting_down.load(Ordering::Acquire) {
            return Ok(false);
        }
        intent.launch = Some(launch);
        intent.expected_running = true;
        intent.generation = intent.generation.wrapping_add(1);
        Ok(true)
    }

    pub(crate) fn with_stopped_server<T>(
        &self,
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _operation = self
            .lifecycle
            .try_lock()
            .map_err(|_| "Wait for the current FXServer action to finish.".to_string())?;
        let mut process = self
            .process
            .lock()
            .map_err(|_| "FXServer process state is unavailable.".to_string())?;
        if let Some(process) = process.as_mut() {
            if process_is_running(process)? {
                return Err("Stop FXServer before changing managed server files.".to_string());
            }
        }
        drop(process);
        action()
    }

    pub(crate) fn prepare_workspace_switch(
        &self,
        switch: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let _operation = self.lifecycle.try_lock().map_err(|_| {
            "Wait for the current FXServer action before switching workspaces.".to_string()
        })?;
        let mut process = self
            .process
            .lock()
            .map_err(|_| "FXServer process state is unavailable.".to_string())?;
        if let Some(process) = process.as_mut() {
            if process_is_running(process)? {
                return Err("Stop FXServer before switching workspaces.".to_string());
            }
        }
        *process = None;
        drop(process);
        self.disarm_recovery();
        switch()?;
        clear_terminal(&self.terminal)
    }

    pub(crate) fn sample_health(
        &self,
        sampler: &mut HealthResourceSampler,
        include_resources: bool,
    ) -> Result<Option<HealthProcessSample>, String> {
        let Ok(operation) = self.lifecycle.try_lock() else {
            return Ok(None);
        };
        let Ok(mut process) = self.process.try_lock() else {
            return Ok(None);
        };
        let pid = if let Some(child) = process.as_mut() {
            if process_is_running(child)? {
                Some(child.child.id())
            } else {
                None
            }
        } else {
            None
        };
        if pid.is_none() {
            *process = None;
        }
        let intent = self
            .launch_intent
            .lock()
            .map_err(|_| "FXServer launch settings are unavailable.".to_string())?;
        let generation = intent.generation;
        let expected_running =
            intent.expected_running && !self.shutting_down.load(Ordering::Acquire);
        drop(intent);
        drop(process);
        drop(operation);

        if sampler.pid != pid {
            sampler.previous = None;
            sampler.pid = pid;
        }
        let resources = if include_resources {
            pid.and_then(|pid| read_process_resources(pid, &mut sampler.previous))
        } else {
            None
        };
        Ok(Some(HealthProcessSample {
            generation,
            expected_running,
            running: pid.is_some(),
            pid,
            resources,
        }))
    }

    pub(crate) fn recover_last_launch(
        &self,
        generation: u64,
        enabled: &AtomicBool,
    ) -> RecoveryOutcome {
        let Ok(_operation) = self.lifecycle.try_lock() else {
            return RecoveryOutcome::Busy;
        };
        if self.shutting_down.load(Ordering::Acquire) || !enabled.load(Ordering::Acquire) {
            return RecoveryOutcome::Cancelled;
        }
        let launch = match self.launch_intent.lock() {
            Ok(intent) if intent.expected_running && intent.generation == generation => {
                intent.launch.clone()
            }
            _ => None,
        };
        let Some(launch) = launch else {
            return RecoveryOutcome::Cancelled;
        };
        if let Ok(mut process) = self.process.lock() {
            if process
                .as_mut()
                .is_some_and(|process| process_is_running(process).unwrap_or(true))
            {
                return RecoveryOutcome::Cancelled;
            }
        }
        match start_fxserver_blocking(launch.into_request(), self) {
            Ok(_) => {
                let still_expected = self
                    .launch_intent
                    .lock()
                    .map(|intent| intent.expected_running && intent.generation == generation)
                    .unwrap_or(false);
                if !still_expected || self.shutting_down.load(Ordering::Acquire) {
                    let _ = self.stop_process();
                    RecoveryOutcome::Cancelled
                } else {
                    RecoveryOutcome::Started
                }
            }
            Err(error) => RecoveryOutcome::Failed(error),
        }
    }

    fn stop_process(&self) -> Result<(), String> {
        let mut guard = self
            .process
            .lock()
            .map_err(|_| "FXServer process state is unavailable.".to_string())?;

        let Some(mut process) = guard.take() else {
            return Ok(());
        };
        drop(guard);

        if process_is_running(&mut process)? {
            let pid = process.child.id();
            append_terminal_line(&self.terminal, "system", "Stopping FXServer gracefully...")?;

            match request_graceful_fxserver_stop(&mut process) {
                Ok(true) => {
                    if wait_for_child_exit(&mut process.child, GRACEFUL_STOP_TIMEOUT) {
                        append_terminal_line(&self.terminal, "system", "FXServer stopped.")?;
                        return Ok(());
                    }

                    append_terminal_line(
                        &self.terminal,
                        "system",
                        "FXServer did not exit after the graceful stop request; forcing shutdown.",
                    )?;
                }
                Ok(false) => {
                    append_terminal_line(
                        &self.terminal,
                        "system",
                        "FXServer console input was unavailable; forcing shutdown.",
                    )?;
                }
                Err(error) => {
                    append_terminal_line(
                        &self.terminal,
                        "system",
                        format!("Graceful FXServer stop failed: {error}. Forcing shutdown."),
                    )?;
                }
            }

            force_stop_fxserver_process(&mut process.child, pid)?;
        }

        append_terminal_line(&self.terminal, "system", "FXServer stopped.")?;

        Ok(())
    }
}

#[tauri::command]
pub async fn start_fxserver(
    request: FxserverLaunchRequest,
    manager: tauri::State<'_, FxserverManager>,
) -> Result<FxserverLaunchResult, String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || {
        let _operation = manager
            .lifecycle
            .try_lock()
            .map_err(|_| "Another FXServer action is in progress.".to_string())?;
        let launch = SavedLaunch::from_request(&request);
        let generation = manager.launch_generation()?;
        let result = start_fxserver_blocking(request, &manager)?;
        if !manager.remember_launch(launch, generation)? {
            manager.stop_process()?;
            return Err("FXServer start was cancelled by a stop or shutdown request.".to_string());
        }
        Ok(result)
    })
    .await
}

fn start_fxserver_blocking(
    request: FxserverLaunchRequest,
    manager: &FxserverManager,
) -> Result<FxserverLaunchResult, String> {
    if manager.shutting_down.load(Ordering::Acquire) {
        return Err("FXServer Installer is shutting down.".to_string());
    }
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
    drop(guard);

    let artifact_path = PathBuf::from(request.artifact_path.trim());
    if artifact_path.as_os_str().is_empty() {
        return Err("Choose an FXServer artifact folder before starting the server.".to_string());
    }

    let executable_path = artifact_path.join("FXServer.exe");
    if !executable_path.is_file() {
        return Err("FXServer.exe was not found in the selected artifact folder.".to_string());
    }

    let mut command = Command::new(&executable_path);
    command.no_window();
    command
        .current_dir(&artifact_path)
        .stdin(Stdio::piped())
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
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let cleanup_job = create_process_cleanup_job(&child);
    let cleanup_warning = cleanup_job.as_ref().err().cloned();
    let cleanup_job = cleanup_job.ok();
    let watchdog_warning = spawn_fxserver_watchdog(pid).err();

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

    if let Some(warning) = cleanup_warning {
        append_terminal_line(&manager.terminal, "system", warning)?;
    }

    if let Some(warning) = watchdog_warning {
        append_terminal_line(&manager.terminal, "system", warning)?;
    }

    if let Some(job) = cleanup_job.clone() {
        attach_process_tree_to_cleanup_job_later(job, pid);
    }

    *manager
        .process
        .lock()
        .map_err(|_| "FXServer process state is unavailable.".to_string())? =
        Some(ManagedFxserverProcess {
            child,
            stdin,
            artifact_path: artifact_path.clone(),
            started_at,
            resource_sample: None,
            _cleanup_job: cleanup_job,
        });

    Ok(FxserverLaunchResult {
        pid,
        artifact_path: artifact_path.to_string_lossy().to_string(),
        started_at: started_at_label,
    })
}

#[tauri::command]
pub async fn stop_fxserver(manager: tauri::State<'_, FxserverManager>) -> Result<(), String> {
    manager.disarm_recovery();
    let manager = manager.inner().clone();
    super::run_blocking(move || stop_fxserver_blocking(&manager)).await
}

fn stop_fxserver_blocking(manager: &FxserverManager) -> Result<(), String> {
    manager.disarm_recovery();
    let _operation = manager
        .lifecycle
        .try_lock()
        .map_err(|_| "Another FXServer action is in progress.".to_string())?;
    manager.stop_process()
}

#[tauri::command]
pub async fn restart_fxserver(
    mut request: FxserverLaunchRequest,
    manager: tauri::State<'_, FxserverManager>,
) -> Result<FxserverLaunchResult, String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || {
        let _operation = manager
            .lifecycle
            .try_lock()
            .map_err(|_| "Another FXServer action is in progress.".to_string())?;
        if !Path::new(request.artifact_path.trim())
            .join("FXServer.exe")
            .is_file()
        {
            return Err("FXServer.exe was not found in the selected artifact folder.".to_string());
        }
        request.environment = sanitize_environment(request.environment)?;
        manager.disarm_recovery();
        manager.stop_process()?;
        let launch = SavedLaunch::from_request(&request);
        let generation = manager.launch_generation()?;
        let result = start_fxserver_blocking(request, &manager)?;
        if !manager.remember_launch(launch, generation)? {
            manager.stop_process()?;
            return Err(
                "FXServer restart was cancelled by a stop or shutdown request.".to_string(),
            );
        }
        Ok(result)
    })
    .await
}

#[tauri::command]
pub async fn get_fxserver_status(
    manager: tauri::State<'_, FxserverManager>,
) -> Result<FxserverStatus, String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || get_fxserver_status_blocking(&manager)).await
}

fn get_fxserver_status_blocking(manager: &FxserverManager) -> Result<FxserverStatus, String> {
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
    let artifact_path = process.artifact_path.to_string_lossy().to_string();
    let started_at = system_time_to_label(process.started_at);
    let mut resource_sample = process.resource_sample;
    drop(guard);
    let resources = read_process_resources(pid, &mut resource_sample);
    if let Ok(mut guard) = manager.process.try_lock() {
        if let Some(process) = guard.as_mut().filter(|process| process.child.id() == pid) {
            process.resource_sample = resource_sample;
        }
    }
    Ok(FxserverStatus {
        running: true,
        pid: Some(pid),
        artifact_path: Some(artifact_path),
        started_at: Some(started_at),
        uptime_seconds: None,
        resources,
    })
}

#[tauri::command]
pub async fn get_fxserver_terminal(
    max_lines: Option<usize>,
    after_id: Option<u64>,
    manager: tauri::State<'_, FxserverManager>,
) -> Result<FxserverTerminalResult, String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || get_fxserver_terminal_blocking(max_lines, after_id, &manager)).await
}

fn get_fxserver_terminal_blocking(
    max_lines: Option<usize>,
    after_id: Option<u64>,
    manager: &FxserverManager,
) -> Result<FxserverTerminalResult, String> {
    let max_lines = max_lines.unwrap_or(500).clamp(50, 5000);
    let terminal = manager
        .terminal
        .lock()
        .map_err(|_| "FXServer terminal output is unavailable.".to_string())?;
    let start = if let Some(after_id) = after_id {
        let first_new = terminal
            .entries
            .iter()
            .position(|entry| entry.id > after_id)
            .unwrap_or(terminal.entries.len());
        let new_count = terminal.entries.len().saturating_sub(first_new);

        if new_count > max_lines {
            terminal.entries.len().saturating_sub(max_lines)
        } else {
            first_new
        }
    } else {
        terminal.entries.len().saturating_sub(max_lines)
    };

    Ok(FxserverTerminalResult {
        entries: terminal.entries[start..].to_vec(),
    })
}

#[tauri::command]
pub async fn get_fxserver_rcon_password(
    workspace_id: Option<String>,
) -> Result<Option<String>, String> {
    super::run_blocking(move || get_fxserver_rcon_password_blocking(workspace_id.as_deref())).await
}

fn get_fxserver_rcon_password_blocking(
    workspace_id: Option<&str>,
) -> Result<Option<String>, String> {
    let _lock = RCON_PASSWORD_LOCK
        .lock()
        .map_err(|_| "RCON password store is unavailable.".to_string())?;
    let path = rcon_password_path(workspace_id)?;
    if !path.exists() {
        return Ok(None);
    }

    let encrypted_hex = fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read saved RCON password: {error}"))?;
    let encrypted = decode_hex(encrypted_hex.trim())?;
    let decrypted = decrypt_secret(&encrypted)?;
    String::from_utf8(decrypted)
        .map(Some)
        .map_err(|_| "Saved RCON password is not valid UTF-8.".to_string())
}

#[tauri::command]
pub async fn save_fxserver_rcon_password(
    password: String,
    workspace_id: Option<String>,
) -> Result<(), String> {
    super::run_blocking(move || {
        save_fxserver_rcon_password_blocking(password, workspace_id.as_deref())
    })
    .await
}

fn save_fxserver_rcon_password_blocking(
    password: String,
    workspace_id: Option<&str>,
) -> Result<(), String> {
    if password.is_empty() {
        return clear_fxserver_rcon_password_blocking(workspace_id);
    }

    let _lock = RCON_PASSWORD_LOCK
        .lock()
        .map_err(|_| "RCON password store is unavailable.".to_string())?;
    let path = rcon_password_path(workspace_id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create RCON password store: {error}"))?;
    }

    let encrypted = encrypt_secret(password.as_bytes())?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, encode_hex(&encrypted))
        .map_err(|error| format!("Failed to save RCON password securely: {error}"))?;
    replace_secret_file(&temporary, &path)
}

#[tauri::command]
pub async fn clear_fxserver_rcon_password(workspace_id: Option<String>) -> Result<(), String> {
    super::run_blocking(move || clear_fxserver_rcon_password_blocking(workspace_id.as_deref()))
        .await
}

fn clear_fxserver_rcon_password_blocking(workspace_id: Option<&str>) -> Result<(), String> {
    let _lock = RCON_PASSWORD_LOCK
        .lock()
        .map_err(|_| "RCON password store is unavailable.".to_string())?;
    let path = rcon_password_path(workspace_id)?;
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| format!("Failed to clear saved RCON password: {error}"))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn send_fxserver_command(
    request: FxserverCommandRequest,
    manager: tauri::State<'_, FxserverManager>,
) -> Result<(), String> {
    let manager = manager.inner().clone();
    super::run_blocking(move || send_fxserver_command_blocking(request, &manager)).await
}

fn send_fxserver_command_blocking(
    request: FxserverCommandRequest,
    manager: &FxserverManager,
) -> Result<(), String> {
    let command = request.command.trim();
    if command.is_empty() {
        return Ok(());
    }
    let generation = manager
        .terminal
        .lock()
        .map_err(|_| "FXServer terminal output is unavailable.".to_string())?
        .generation;

    {
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
    }

    append_terminal_generation(
        &manager.terminal,
        "command",
        format!("rcon> {command}"),
        Some(generation),
    )?;

    if command
        .split_whitespace()
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("quit"))
    {
        manager.disarm_recovery();
    }

    let response = send_rcon_command(&request.rcon, command)?;

    if response.trim().is_empty() {
        append_terminal_generation(
            &manager.terminal,
            "system",
            "RCON packet sent; no response received.",
            Some(generation),
        )?;
    } else {
        for line in response.lines() {
            append_terminal_generation(&manager.terminal, "rcon", line, Some(generation))?;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn read_txdata_log(request: TxDataLogRequest) -> Result<TxDataLogResult, String> {
    super::run_blocking(move || read_txdata_log_blocking(request)).await
}

fn read_txdata_log_blocking(request: TxDataLogRequest) -> Result<TxDataLogResult, String> {
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
pub async fn list_txdata_profiles(data_path: String) -> Result<TxDataProfilesResult, String> {
    super::run_blocking(move || list_txdata_profiles_blocking(data_path)).await
}

fn list_txdata_profiles_blocking(data_path: String) -> Result<TxDataProfilesResult, String> {
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
pub async fn read_server_config(
    request: ServerConfigRequest,
) -> Result<ServerConfigResult, String> {
    super::run_blocking(move || read_server_config_blocking(request)).await
}

fn read_server_config_blocking(request: ServerConfigRequest) -> Result<ServerConfigResult, String> {
    let (tx_data_path, profile, profile_config_path, data_path) =
        resolve_profile_data_path(request.tx_data_path, request.profile)?;

    let files = super::config_history::read_profile_configs(&data_path)?;
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
pub async fn save_server_config(
    app: AppHandle,
    request: SaveServerConfigRequest,
    tx_data_path: Option<String>,
    profile: Option<String>,
    expected_content: Option<String>,
) -> Result<ServerConfigFile, String> {
    super::run_blocking(move || {
        let target = super::config_history::ConfigFileRequest {
            tx_data_path: tx_data_path.ok_or("Reload the selected profile before saving.")?,
            profile: profile.ok_or("Reload the selected profile before saving.")?,
            path: request.path,
        };
        let expected = expected_content.ok_or("Reload and review the file before saving.")?;
        super::config_history::save_config_atomic(
            &super::config_history::history_root(&app)?,
            &target,
            &expected,
            &request.content,
            super::config_history::ConfigChangeReason::Save,
        )
    })
    .await
}

pub(crate) fn config_file_metadata(path: &Path) -> Result<ServerConfigFile, String> {
    read_cfg_file(path)
}

#[tauri::command]
pub async fn scan_fxserver_resources(
    request: ResourceScanRequest,
) -> Result<ResourceScanResult, String> {
    super::run_blocking(move || scan_fxserver_resources_blocking(request)).await
}

fn scan_fxserver_resources_blocking(
    request: ResourceScanRequest,
) -> Result<ResourceScanResult, String> {
    let (tx_data_path, profile, _, data_path) =
        resolve_profile_data_path(request.tx_data_path, request.profile)?;
    let resource_root = data_path.join("resources");

    if !resource_root.is_dir() {
        return Err(format!(
            "Resource folder was not found: {}",
            resource_root.to_string_lossy()
        ));
    }

    let resources = scan_resource_directory(&resource_root, 8)?;

    Ok(ResourceScanResult {
        tx_data_path: tx_data_path.to_string_lossy().to_string(),
        profile,
        data_path: data_path.to_string_lossy().to_string(),
        resource_root: resource_root.to_string_lossy().to_string(),
        resources,
    })
}

#[tauri::command]
pub async fn send_fxserver_rcon_command(request: FxserverCommandRequest) -> Result<String, String> {
    super::run_blocking(move || send_fxserver_rcon_command_blocking(request)).await
}

fn send_fxserver_rcon_command_blocking(request: FxserverCommandRequest) -> Result<String, String> {
    send_rcon_command(&request.rcon, &request.command)
}

fn process_is_running(process: &mut ManagedFxserverProcess) -> Result<bool, String> {
    Ok(process
        .child
        .try_wait()
        .map_err(|error| format!("Failed to inspect FXServer: {error}"))?
        .is_none())
}

#[cfg(target_os = "windows")]
fn create_process_cleanup_job(child: &Child) -> Result<Arc<ProcessCleanupJob>, String> {
    use std::{mem, os::windows::io::AsRawHandle, ptr};
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
    };

    let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if job.is_null() {
        return Err(format!(
            "FXServer cleanup job could not be created: {}",
            std::io::Error::last_os_error()
        ));
    }

    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &mut limits as *mut _ as *mut _,
            mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };

    if configured == 0 {
        let error = std::io::Error::last_os_error();
        unsafe {
            CloseHandle(job);
        }
        return Err(format!(
            "FXServer cleanup job could not be configured: {error}"
        ));
    }

    let assigned = unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as _) };
    if assigned == 0 {
        let error = std::io::Error::last_os_error();
        unsafe {
            CloseHandle(job);
        }
        return Err(format!(
            "FXServer cleanup job could not attach to the server process: {error}"
        ));
    }

    Ok(Arc::new(ProcessCleanupJob(job as isize)))
}

#[cfg(not(target_os = "windows"))]
fn create_process_cleanup_job(_: &Child) -> Result<Arc<ProcessCleanupJob>, String> {
    Ok(Arc::new(ProcessCleanupJob))
}

#[cfg(target_os = "windows")]
fn spawn_fxserver_watchdog(fxserver_pid: u32) -> Result<(), String> {
    let current_exe = env::current_exe()
        .map_err(|error| format!("FXServer watchdog could not find the app executable: {error}"))?;
    Command::new(current_exe)
        .no_window()
        .arg(FXSERVER_WATCHDOG_ARG)
        .arg(std::process::id().to_string())
        .arg(fxserver_pid.to_string())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("FXServer watchdog could not start: {error}"))
}

#[cfg(not(target_os = "windows"))]
fn spawn_fxserver_watchdog(_: u32) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "windows")]
fn attach_process_tree_to_cleanup_job_later(job: Arc<ProcessCleanupJob>, root_pid: u32) {
    thread::spawn(move || {
        for _ in 0..10 {
            thread::sleep(Duration::from_millis(500));
            let _ = attach_process_tree_to_cleanup_job(&job, root_pid);
        }
    });
}

#[cfg(not(target_os = "windows"))]
fn attach_process_tree_to_cleanup_job_later(_: Arc<ProcessCleanupJob>, _: u32) {}

#[cfg(target_os = "windows")]
fn attach_process_tree_to_cleanup_job(
    job: &ProcessCleanupJob,
    root_pid: u32,
) -> Result<(), String> {
    use windows_sys::Win32::System::{
        JobObjects::AssignProcessToJobObject,
        Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
    };

    for pid in process_tree_ids(root_pid)? {
        let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
        if process.is_null() {
            continue;
        }

        unsafe {
            AssignProcessToJobObject(job.handle(), process);
            windows_sys::Win32::Foundation::CloseHandle(process);
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn process_tree_ids(root_pid: u32) -> Result<Vec<u32>, String> {
    use std::mem;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(format!(
            "Failed to inspect Windows process tree: {}",
            std::io::Error::last_os_error()
        ));
    }

    let mut entries = Vec::new();
    let mut entry: PROCESSENTRY32W = unsafe { mem::zeroed() };
    entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while has_entry {
        entries.push((entry.th32ProcessID, entry.th32ParentProcessID));
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }

    unsafe {
        CloseHandle(snapshot);
    }

    let mut all = vec![root_pid];
    let mut frontier = vec![root_pid];

    while !frontier.is_empty() {
        let mut next = Vec::new();
        for (pid, parent_pid) in &entries {
            if frontier.contains(parent_pid) && !all.contains(pid) {
                all.push(*pid);
                next.push(*pid);
            }
        }
        frontier = next;
    }

    Ok(all)
}

#[cfg(target_os = "windows")]
pub(crate) fn terminate_process_tree(root_pid: u32) -> Result<(), String> {
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    let mut ids = process_tree_ids(root_pid)?;
    ids.reverse();

    for pid in ids {
        let process = unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        if process.is_null() {
            continue;
        }

        unsafe {
            TerminateProcess(process, 1);
            windows_sys::Win32::Foundation::CloseHandle(process);
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn terminate_process_tree(_: u32) -> Result<(), String> {
    Ok(())
}

fn request_graceful_fxserver_stop(process: &mut ManagedFxserverProcess) -> Result<bool, String> {
    let Some(mut stdin) = process.stdin.take() else {
        return Ok(false);
    };

    stdin
        .write_all(b"quit\n")
        .and_then(|_| stdin.flush())
        .map_err(|error| format!("Failed to send quit to FXServer console: {error}"))?;

    Ok(true)
}

fn force_stop_fxserver_process(child: &mut Child, pid: u32) -> Result<(), String> {
    if let Err(error) = terminate_process_tree(pid) {
        child
            .kill()
            .map_err(|kill_error| format!("Failed to force stop FXServer after process tree cleanup failed ({error}): {kill_error}"))?;
    }

    wait_for_child_exit(child, FORCE_STOP_WAIT_TIMEOUT);
    Ok(())
}

fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(_) => return true,
        }
    }
    false
}

pub(crate) fn resolve_profile_data_path(
    tx_data_path: String,
    profile: String,
) -> Result<(PathBuf, String, PathBuf, PathBuf), String> {
    let tx_data_path = PathBuf::from(tx_data_path.trim());
    if tx_data_path.as_os_str().is_empty() {
        return Err("Set TXHOST_DATA_PATH before reading txData profile data.".to_string());
    }

    let profile = profile.trim().to_string();
    if profile.is_empty()
        || profile == "."
        || profile == ".."
        || profile.contains(['/', '\\', ':', '\0'])
        || profile.ends_with([' ', '.'])
    {
        return Err("Choose a valid txData profile name.".to_string());
    }

    let tx_data_path = tx_data_path
        .canonicalize()
        .map_err(|_| "The txData directory cannot be opened.".to_string())?;
    let profile_root = tx_data_path
        .join(&profile)
        .canonicalize()
        .map_err(|_| "The selected profile directory cannot be opened.".to_string())?;
    if profile_root.parent() != Some(tx_data_path.as_path()) {
        return Err("The profile directory must stay inside the selected txData directory.".into());
    }
    let profile_config_path = profile_root
        .join("config.json")
        .canonicalize()
        .map_err(|_| "The selected profile config.json cannot be opened.".to_string())?;
    if profile_config_path.parent() != Some(profile_root.as_path()) {
        return Err("The profile config.json must stay inside the selected profile.".into());
    }
    let profile_config =
        super::config_history::read_bounded_config(&profile_config_path).map_err(|error| {
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

    if !data_path.is_absolute() {
        return Err("The profile dataPath must be an absolute directory path.".into());
    }
    let data_path = data_path
        .canonicalize()
        .map_err(|_| "The profile data directory cannot be opened.".to_string())?;

    Ok((tx_data_path, profile, profile_config_path, data_path))
}

fn scan_resource_directory(
    resource_root: &Path,
    max_depth: usize,
) -> Result<Vec<FxserverResourceInfo>, String> {
    let mut resources = Vec::new();
    scan_resource_directory_inner(resource_root, max_depth, &mut resources)?;
    resources.sort_by_key(|resource| resource.name.to_ascii_lowercase());
    Ok(resources)
}

fn scan_resource_directory_inner(
    directory: &Path,
    depth: usize,
    resources: &mut Vec<FxserverResourceInfo>,
) -> Result<(), String> {
    if depth == 0 || !directory.is_dir() {
        return Ok(());
    }

    if let Some(resource) = read_resource_manifest(directory)? {
        resources.push(resource);
        return Ok(());
    }

    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "Failed to inspect resource folder {}: {error}",
            directory.to_string_lossy()
        )
    })?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("Failed to inspect resource entry: {error}"))?;
        let path = entry.path();
        if !path.is_dir() || should_skip_resource_scan_dir(&path) {
            continue;
        }

        scan_resource_directory_inner(&path, depth - 1, resources)?;
    }

    Ok(())
}

fn should_skip_resource_scan_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                ".git" | "node_modules" | ".vscode" | ".idea" | "cache" | "tmp" | "temp"
            )
        })
        .unwrap_or(false)
}

fn read_resource_manifest(directory: &Path) -> Result<Option<FxserverResourceInfo>, String> {
    let manifest_path = ["fxmanifest.lua", "__resource.lua"]
        .into_iter()
        .map(|name| directory.join(name))
        .find(|path| path.is_file());

    let Some(manifest_path) = manifest_path else {
        return Ok(None);
    };

    let content = fs::read_to_string(&manifest_path).map_err(|error| {
        format!(
            "Failed to read resource manifest {}: {error}",
            manifest_path.to_string_lossy()
        )
    })?;
    let manifest_name = manifest_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "fxmanifest.lua".to_string());
    let name = directory
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| directory.to_string_lossy().to_string());

    Ok(Some(FxserverResourceInfo {
        name,
        path: directory.to_string_lossy().to_string(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        manifest_name,
        version: parse_manifest_value(&content, "version"),
        repository: parse_manifest_value(&content, "repository"),
    }))
}

fn parse_manifest_value(content: &str, key: &str) -> Option<String> {
    for raw_line in content.lines() {
        let line = raw_line.trim_start();
        if line.starts_with("--") || line.starts_with('#') || !line.starts_with(key) {
            continue;
        }

        let remainder = &line[key.len()..];
        if remainder
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            continue;
        }

        if let Some(value) = first_quoted_value(remainder) {
            return Some(value);
        }
    }

    None
}

fn first_quoted_value(value: &str) -> Option<String> {
    let mut quote = None;
    let mut start = 0;

    for (index, character) in value.char_indices() {
        if character == '\'' || character == '"' {
            quote = Some(character);
            start = index + character.len_utf8();
            break;
        }
    }

    let quote = quote?;
    let mut escaped = false;
    let mut output = String::new();

    for character in value[start..].chars() {
        if escaped {
            output.push(character);
            escaped = false;
            continue;
        }

        if character == '\\' {
            escaped = true;
            continue;
        }

        if character == quote {
            let output = output.trim().to_string();
            return (!output.is_empty()).then_some(output);
        }

        output.push(character);
    }

    None
}

fn clear_terminal(terminal: &Arc<Mutex<TerminalState>>) -> Result<(), String> {
    let mut terminal = terminal
        .lock()
        .map_err(|_| "FXServer terminal output is unavailable.".to_string())?;
    terminal.entries.clear();
    terminal.incidents.clear();
    terminal.generation = terminal.generation.wrapping_add(1);
    Ok(())
}

struct ParsedTerminalLine {
    plain_line: String,
    segments: Vec<FxserverTerminalSegment>,
}

fn parse_terminal_line(line: &str) -> ParsedTerminalLine {
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut active_color: Option<String> = None;
    let mut plain_line = String::new();
    let mut text = String::new();
    let mut segments = Vec::new();

    while index < bytes.len() {
        if line[index..].starts_with("]0;") {
            break;
        }

        match bytes[index] {
            0x1b => {
                flush_terminal_text(&mut segments, &mut text, active_color.as_deref());
                if index + 1 < bytes.len() && bytes[index + 1] == b']' {
                    index = skip_ansi_osc(bytes, index + 2);
                } else if index + 1 < bytes.len() && bytes[index + 1] == b'[' {
                    let (next, params, final_byte) = parse_ansi_csi(bytes, index + 2);
                    if final_byte == Some(b'm') {
                        active_color = apply_ansi_sgr_color(&params, active_color);
                    }
                    index = next;
                } else {
                    index += 1;
                }
            }
            b'^' if index + 1 < bytes.len() && bytes[index + 1].is_ascii_digit() => {
                flush_terminal_text(&mut segments, &mut text, active_color.as_deref());
                let code = (bytes[index + 1] as char).to_string();
                active_color = terminal_color_code(&code).map(str::to_string);
                index += 2;
            }
            0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f => {
                index += 1;
            }
            _ => {
                let Some(character) = line[index..].chars().next() else {
                    break;
                };
                text.push(character);
                plain_line.push(character);
                index += character.len_utf8();
            }
        }
    }

    flush_terminal_text(&mut segments, &mut text, active_color.as_deref());

    ParsedTerminalLine {
        plain_line,
        segments,
    }
}

fn skip_ansi_osc(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() {
        if bytes[index] == 0x07 {
            return index + 1;
        }
        if bytes[index] == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b'\\' {
            return index + 2;
        }
        index += 1;
    }
    bytes.len()
}

fn parse_ansi_csi(bytes: &[u8], mut index: usize) -> (usize, String, Option<u8>) {
    let start = index;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b')' {
            let params = String::from_utf8_lossy(&bytes[start..index]).to_string();
            return (index + 1, params, None);
        }
        if (0x40..=0x7e).contains(&byte) {
            let params = String::from_utf8_lossy(&bytes[start..index]).to_string();
            return (index + 1, params, Some(byte));
        }
        index += 1;
    }

    (bytes.len(), String::new(), None)
}

fn flush_terminal_text(
    segments: &mut Vec<FxserverTerminalSegment>,
    text: &mut String,
    active_color: Option<&str>,
) {
    if text.is_empty() {
        return;
    }

    push_terminal_text_segments(segments, text, active_color);
    text.clear();
}

fn push_terminal_text_segments(
    segments: &mut Vec<FxserverTerminalSegment>,
    text: &str,
    active_color: Option<&str>,
) {
    let mut index = 0;

    while index < text.len() {
        let next_bracket = text[index..].find('[').map(|offset| index + offset);
        let next_script = text[index..].find("script:").map(|offset| index + offset);
        let next_rcon_script = text[index..]
            .find("rcon/script:")
            .map(|offset| index + offset);
        let Some(next_tag) = [next_bracket, next_script, next_rcon_script]
            .into_iter()
            .flatten()
            .min()
        else {
            push_terminal_segment(segments, &text[index..], active_color, false);
            break;
        };

        push_terminal_segment(segments, &text[index..next_tag], active_color, false);

        if text[next_tag..].starts_with('[') {
            if let Some((tag, next_index)) = parse_bracket_tag(text, next_tag) {
                push_terminal_tag_segment(segments, tag, active_color);
                index = next_index;
            } else {
                push_terminal_segment(segments, "[", active_color, false);
                index = next_tag + 1;
            }
        } else {
            let next_index = parse_script_tag_end(text, next_tag);
            push_terminal_tag_segment(segments, &text[next_tag..next_index], active_color);
            index = next_index;
        }
    }
}

fn parse_bracket_tag(text: &str, start: usize) -> Option<(&str, usize)> {
    let close_offset = text[start..].find(']')?;
    let end = start + close_offset + 1;
    let len = end.saturating_sub(start);
    if !(3..=66).contains(&len) || text[start..end].contains(['\r', '\n']) {
        return None;
    }
    Some((&text[start..end], end))
}

fn parse_script_tag_end(text: &str, start: usize) -> usize {
    let mut end = start;
    for (offset, character) in text[start..].char_indices() {
        if !(character.is_ascii_alphanumeric() || matches!(character, ':' | '/' | '_' | '.' | '-'))
        {
            break;
        }
        end = start + offset + character.len_utf8();
    }
    end.max(start + 1)
}

fn push_terminal_tag_segment(
    segments: &mut Vec<FxserverTerminalSegment>,
    tag: &str,
    active_color: Option<&str>,
) {
    let inner = tag
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(tag)
        .trim();
    let color = if inner.eq_ignore_ascii_case("warn") || inner.eq_ignore_ascii_case("warning") {
        Some("#facc15".to_string())
    } else if inner.eq_ignore_ascii_case("error")
        || inner.eq_ignore_ascii_case("script error")
        || inner.eq_ignore_ascii_case("fatal")
    {
        Some("#f87171".to_string())
    } else if inner.eq_ignore_ascii_case("debug") {
        Some("#60a5fa".to_string())
    } else {
        let resource = inner
            .strip_prefix("rcon/script:")
            .or_else(|| inner.strip_prefix("script:"))
            .unwrap_or(inner);
        Some(resource_color(resource))
    };

    segments.push(FxserverTerminalSegment {
        text: tag.to_string(),
        color: color.or_else(|| active_color.map(str::to_string)),
        emphasis: Some(true),
    });
}

fn push_terminal_segment(
    segments: &mut Vec<FxserverTerminalSegment>,
    text: &str,
    active_color: Option<&str>,
    emphasis: bool,
) {
    if text.is_empty() {
        return;
    }

    segments.push(FxserverTerminalSegment {
        text: text.to_string(),
        color: active_color.map(str::to_string),
        emphasis: emphasis.then_some(true),
    });
}

fn apply_ansi_sgr_color(params: &str, current_color: Option<String>) -> Option<String> {
    let codes = if params.is_empty() {
        vec![0]
    } else {
        params
            .split([';', ':'])
            .map(|part| part.parse::<u16>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let mut color = current_color;
    let mut index = 0;

    while index < codes.len() {
        let code = codes[index];
        if matches!(code, 0 | 39) {
            color = None;
        } else if (30..=37).contains(&code) || (90..=97).contains(&code) {
            color = ansi_basic_color(code).map(str::to_string);
        } else if code == 38 && codes.get(index + 1) == Some(&5) {
            if let Some(value) = codes.get(index + 2) {
                color = Some(ansi_256_color(*value));
            }
            index += 2;
        } else if code == 38 && codes.get(index + 1) == Some(&2) {
            if let (Some(red), Some(green), Some(blue)) = (
                codes.get(index + 2),
                codes.get(index + 3),
                codes.get(index + 4),
            ) {
                color = Some(format!("rgb({red}, {green}, {blue})"));
            }
            index += 4;
        }
        index += 1;
    }

    color
}

fn terminal_color_code(code: &str) -> Option<&'static str> {
    match code {
        "0" | "7" => Some("var(--foreground)"),
        "1" => Some("#f87171"),
        "2" => Some("#4ade80"),
        "3" => Some("#facc15"),
        "4" => Some("#60a5fa"),
        "5" => Some("#22d3ee"),
        "6" => Some("#e879f9"),
        "8" => Some("#94a3b8"),
        "9" => Some("#fb923c"),
        _ => None,
    }
}

fn ansi_basic_color(code: u16) -> Option<&'static str> {
    match code {
        30 => Some("#94a3b8"),
        31 => Some("#f87171"),
        32 => Some("#4ade80"),
        33 => Some("#facc15"),
        34 => Some("#60a5fa"),
        35 => Some("#e879f9"),
        36 => Some("#22d3ee"),
        37 => Some("#e5e7eb"),
        90 => Some("#64748b"),
        91 => Some("#fca5a5"),
        92 => Some("#86efac"),
        93 => Some("#fde047"),
        94 => Some("#93c5fd"),
        95 => Some("#f0abfc"),
        96 => Some("#67e8f9"),
        97 => Some("#f8fafc"),
        _ => None,
    }
}

fn ansi_256_color(code: u16) -> String {
    let value = code.min(255);
    const BASIC: [&str; 16] = [
        "#111827", "#ef4444", "#22c55e", "#eab308", "#3b82f6", "#d946ef", "#06b6d4", "#e5e7eb",
        "#64748b", "#f87171", "#4ade80", "#facc15", "#60a5fa", "#e879f9", "#22d3ee", "#f8fafc",
    ];

    if value < 16 {
        return BASIC[value as usize].to_string();
    }
    if value >= 232 {
        let gray = 8 + (value - 232) * 10;
        return format!("rgb({gray}, {gray}, {gray})");
    }

    let shifted = value - 16;
    let red = shifted / 36;
    let green = (shifted % 36) / 6;
    let blue = shifted % 6;
    let channel = |part: u16| if part == 0 { 0 } else { 55 + part * 40 };
    format!(
        "rgb({}, {}, {})",
        channel(red),
        channel(green),
        channel(blue)
    )
}

fn resource_color(resource_name: &str) -> String {
    const PALETTE: [&str; 10] = [
        "#67e8f9", "#86efac", "#facc15", "#f0abfc", "#93c5fd", "#fb7185", "#c4b5fd", "#fdba74",
        "#5eead4", "#bef264",
    ];
    let hash = resource_name.bytes().fold(0_u32, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(byte as u32)
    });
    PALETTE[hash as usize % PALETTE.len()].to_string()
}

fn append_terminal_line(
    terminal: &Arc<Mutex<TerminalState>>,
    stream: &str,
    line: impl Into<String>,
) -> Result<(), String> {
    append_terminal_generation(terminal, stream, line, None)
}

fn append_terminal_generation(
    terminal: &Arc<Mutex<TerminalState>>,
    stream: &str,
    line: impl Into<String>,
    generation: Option<u64>,
) -> Result<(), String> {
    let line = line.into();
    let parsed = parse_terminal_line(&line);
    let incident_level = console_incident_level(&parsed.plain_line);
    let mut terminal = terminal
        .lock()
        .map_err(|_| "FXServer terminal output is unavailable.".to_string())?;
    if generation.is_some_and(|generation| generation != terminal.generation) {
        return Ok(());
    }
    let entry = FxserverTerminalEntry {
        id: terminal.next_id,
        stream: stream.to_string(),
        line: parsed.plain_line.clone(),
        plain_line: parsed.plain_line,
        segments: parsed.segments,
        timestamp: system_time_to_label(SystemTime::now()),
    };
    terminal.next_id += 1;
    if let Some(level) = incident_level {
        if terminal.incidents.len() >= 100 {
            terminal.incidents.pop_front();
        }
        let workspace_id = if terminal.workspace_id.is_empty() {
            "default".into()
        } else {
            terminal.workspace_id.clone()
        };
        terminal.incidents.push_back(ConsoleIncident {
            id: entry.id,
            workspace_id,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            level,
            message: entry.plain_line.chars().take(2000).collect(),
        });
    }
    terminal.entries.push(entry);

    if terminal.entries.len() > 5000 {
        let overflow = terminal.entries.len() - 5000;
        terminal.entries.drain(0..overflow);
    }

    Ok(())
}

fn console_incident_level(line: &str) -> Option<&'static str> {
    let bytes = line.as_bytes();
    let head = &bytes[..bytes.len().min(512)];
    let contains = |word: &[u8]| {
        head.windows(word.len())
            .any(|part| part.eq_ignore_ascii_case(word))
    };
    if contains(b"script error") || contains(b"[error]") || contains(b"fatal error") {
        Some("error")
    } else if contains(b"warning:") || contains(b"[warn]") || contains(b"hitch warning") {
        Some("warn")
    } else {
        None
    }
}

fn spawn_terminal_reader<R>(terminal: Arc<Mutex<TerminalState>>, stream: &'static str, reader: R)
where
    R: std::io::Read + Send + 'static,
{
    let Ok(generation) = terminal.lock().map(|state| state.generation) else {
        return;
    };
    thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let _ = append_terminal_generation(&terminal, stream, line, Some(generation));
                }
                Err(error) => {
                    let _ = append_terminal_generation(
                        &terminal,
                        "system",
                        format!("Stopped reading {stream}: {error}"),
                        Some(generation),
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

fn read_cfg_file(path: &Path) -> Result<ServerConfigFile, String> {
    let content = super::config_history::read_bounded_config(path)?;
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

fn cfg_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
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
    use std::mem;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ},
        },
    };

    let processes = windows_fxserver_processes(pid).ok()?;
    if processes.is_empty() {
        return None;
    }

    let mut cpu_seconds = 0.0;
    let mut memory_bytes = 0_u64;
    let mut thread_count = 0_u32;
    let mut handle_count = 0_u32;

    for process_info in processes {
        let process = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                0,
                process_info.pid,
            )
        };
        if process.is_null() {
            continue;
        }

        cpu_seconds += process_cpu_seconds(process).unwrap_or(0.0);
        memory_bytes = memory_bytes.saturating_add(process_working_set(process).unwrap_or(0));
        thread_count = thread_count.saturating_add(process_info.thread_count);
        handle_count = handle_count.saturating_add(process_handle_count(process).unwrap_or(0));

        unsafe {
            CloseHandle(process);
        }
    }

    let mut memory_status: MEMORYSTATUSEX = unsafe { mem::zeroed() };
    memory_status.dwLength = mem::size_of::<MEMORYSTATUSEX>() as u32;
    let total_memory_bytes = if unsafe { GlobalMemoryStatusEx(&mut memory_status) } != 0 {
        memory_status.ullTotalPhys
    } else {
        0
    };
    let memory_percent = if total_memory_bytes > 0 {
        ((memory_bytes as f64 / total_memory_bytes as f64) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let current_sample = ResourceSample {
        cpu_seconds,
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

        memory_bytes,

        total_memory_bytes,

        memory_percent: (memory_percent * 100.0).round() / 100.0,

        thread_count,

        handle_count,
    })
}

#[cfg(target_os = "windows")]
fn windows_fxserver_processes(root_pid: u32) -> Result<Vec<WindowsProcessInfo>, String> {
    let processes = windows_process_tree(root_pid)?;
    Ok(processes
        .into_iter()
        .filter(|process| process.exe_name.eq_ignore_ascii_case("fxserver.exe"))
        .collect())
}

#[cfg(target_os = "windows")]
fn windows_process_tree(root_pid: u32) -> Result<Vec<WindowsProcessInfo>, String> {
    use std::mem;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(format!(
            "Failed to inspect Windows process tree: {}",
            std::io::Error::last_os_error()
        ));
    }

    let mut entries = Vec::new();
    let mut entry: PROCESSENTRY32W = unsafe { mem::zeroed() };
    entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while has_entry {
        entries.push((
            entry.th32ProcessID,
            entry.th32ParentProcessID,
            utf16_z_to_string(&entry.szExeFile),
            entry.cntThreads,
        ));
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }

    unsafe {
        CloseHandle(snapshot);
    }

    let mut all = vec![root_pid];
    let mut frontier = vec![root_pid];

    while !frontier.is_empty() {
        let mut next = Vec::new();
        for (pid, parent_pid, _, _) in &entries {
            if frontier.contains(parent_pid) && !all.contains(pid) {
                all.push(*pid);
                next.push(*pid);
            }
        }
        frontier = next;
    }

    Ok(entries
        .into_iter()
        .filter(|(pid, _, _, _)| all.contains(pid))
        .map(|(pid, _, exe_name, thread_count)| WindowsProcessInfo {
            pid,
            exe_name,
            thread_count,
        })
        .collect())
}

#[cfg(target_os = "windows")]
fn process_cpu_seconds(process: windows_sys::Win32::Foundation::HANDLE) -> Option<f64> {
    use std::mem;
    use windows_sys::Win32::{Foundation::FILETIME, System::Threading::GetProcessTimes};

    let mut creation: FILETIME = unsafe { mem::zeroed() };
    let mut exit: FILETIME = unsafe { mem::zeroed() };
    let mut kernel: FILETIME = unsafe { mem::zeroed() };
    let mut user: FILETIME = unsafe { mem::zeroed() };

    if unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return None;
    }

    Some((filetime_to_u64(kernel) + filetime_to_u64(user)) as f64 / 10_000_000.0)
}

#[cfg(target_os = "windows")]
fn process_working_set(process: windows_sys::Win32::Foundation::HANDLE) -> Option<u64> {
    use std::mem;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };

    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { mem::zeroed() };
    counters.cb = mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;

    if unsafe {
        K32GetProcessMemoryInfo(
            process,
            &mut counters,
            mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    } == 0
    {
        return None;
    }

    Some(counters.WorkingSetSize as u64)
}

#[cfg(target_os = "windows")]
fn process_handle_count(process: windows_sys::Win32::Foundation::HANDLE) -> Option<u32> {
    use windows_sys::Win32::System::Threading::GetProcessHandleCount;

    let mut count = 0_u32;
    if unsafe { GetProcessHandleCount(process, &mut count) } == 0 {
        return None;
    }
    Some(count)
}

#[cfg(target_os = "windows")]
fn filetime_to_u64(value: windows_sys::Win32::Foundation::FILETIME) -> u64 {
    ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64
}

#[cfg(target_os = "windows")]
fn utf16_z_to_string(value: &[u16]) -> String {
    let len = value
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..len])
}

#[cfg(not(target_os = "windows"))]
fn read_process_resources(
    _pid: u32,
    _previous_sample: &mut Option<ResourceSample>,
) -> Option<FxserverResources> {
    None
}

fn send_rcon_command(config: &FxserverRconConfig, command: &str) -> Result<String, String> {
    send_rcon_command_with_timeout(config, command, Duration::from_secs(4))
}

fn send_rcon_command_with_timeout(
    config: &FxserverRconConfig,
    command: &str,
    timeout: Duration,
) -> Result<String, String> {
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

    let deadline = Instant::now() + timeout;
    let mut responses = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let wait = if responses.is_empty() {
            remaining
        } else {
            remaining.min(Duration::from_millis(150))
        };
        socket
            .set_read_timeout(Some(wait))
            .map_err(|error| format!("Failed to configure RCON response timeout: {error}"))?;
        match socket.recv_from(&mut buffer) {
            Ok((length, _)) => {
                let response = parse_quake_rcon_response(&buffer[..length]);
                if !response.trim().is_empty() {
                    if response.to_ascii_lowercase().contains("bad rconpassword") {
                        return Err("RCON authentication failed. Check rcon_password.".to_string());
                    }
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

    Ok(responses.join("\n").trim().to_string())
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

fn rcon_password_path(workspace_id: Option<&str>) -> Result<PathBuf, String> {
    let app_data = env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "Windows APPDATA folder is unavailable.".to_string())?;
    scoped_rcon_password_path(
        &app_data.join("fxserver-installer").join("secrets"),
        workspace_id,
    )
}

fn scoped_rcon_password_path(root: &Path, workspace_id: Option<&str>) -> Result<PathBuf, String> {
    let id = workspace_id.unwrap_or("default");
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("Invalid workspace ID for the RCON password store.".to_string());
    }
    let id = id.to_ascii_lowercase();
    let path = root.join(format!("workspace-{id}-rcon.dpapi"));
    let legacy = root.join("fxserver-rcon-password.dpapi");
    if id == "default" && !path.exists() && legacy.is_file() {
        fs::rename(&legacy, &path)
            .map_err(|error| format!("Failed to migrate the saved RCON password: {error}"))?;
    }
    Ok(path)
}

#[cfg(target_os = "windows")]
fn replace_secret_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(format!(
            "Failed to replace saved RCON password: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn replace_secret_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination)
        .map_err(|error| format!("Failed to replace saved RCON password: {error}"))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 {
        return Err("Saved RCON password data is malformed.".to_string());
    }

    let mut bytes = Vec::with_capacity(value.len() / 2);
    let raw = value.as_bytes();
    for index in (0..raw.len()).step_by(2) {
        let high = hex_value(raw[index])?;
        let low = hex_value(raw[index + 1])?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn hex_value(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("Saved RCON password data is malformed.".to_string()),
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn encrypt_secret(secret: &[u8]) -> Result<Vec<u8>, String> {
    use std::ptr;
    use std::slice;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: secret.len() as u32,
        pbData: secret.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };

    if ok == 0 {
        return Err("Windows could not protect the RCON password.".to_string());
    }

    let protected =
        unsafe { slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(protected)
}

#[cfg(target_os = "windows")]
pub(crate) fn decrypt_secret(secret: &[u8]) -> Result<Vec<u8>, String> {
    use std::ptr;
    use std::slice;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: secret.len() as u32,
        pbData: secret.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };

    if ok == 0 {
        return Err("Windows could not unlock the saved RCON password.".to_string());
    }

    let unprotected =
        unsafe { slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(unprotected)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn encrypt_secret(_: &[u8]) -> Result<Vec<u8>, String> {
    Err("Secure RCON password storage is only supported on Windows.".to_string())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn decrypt_secret(_: &[u8]) -> Result<Vec<u8>, String> {
    Err("Secure RCON password storage is only supported on Windows.".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn console_incidents_are_filtered_bounded_and_cleared() {
        let manager = super::FxserverManager::default();
        manager.set_incident_workspace("original").unwrap();
        super::append_terminal_line(&manager.terminal, "stdout", "normal output").unwrap();
        assert!(manager.terminal.lock().unwrap().incidents.is_empty());
        for _ in 0..200 {
            super::append_terminal_line(&manager.terminal, "stdout", "^1SCRIPT ERROR: fixture")
                .unwrap();
        }
        assert_eq!(manager.terminal.lock().unwrap().incidents.len(), 100);
        manager.set_incident_workspace("next").unwrap();
        assert_eq!(
            manager.terminal.lock().unwrap().incidents[0].workspace_id,
            "original"
        );
        super::clear_terminal(&manager.terminal).unwrap();
        assert!(manager.terminal.lock().unwrap().incidents.is_empty());
        assert_eq!(
            super::console_incident_level("[WARN] slow tick"),
            Some("warn")
        );
    }

    use super::*;
    use std::sync::mpsc;

    fn fixture_secret_directory() -> PathBuf {
        let path = env::temp_dir().join(format!(
            "fxserver-secret-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn workspace_secret_paths_are_isolated_and_reject_traversal() {
        let root = fixture_secret_directory();
        let default = scoped_rcon_password_path(&root, None).unwrap();
        let other = scoped_rcon_password_path(&root, Some("server-2")).unwrap();
        assert_ne!(default, other);
        assert_eq!(default.parent(), Some(root.as_path()));
        for invalid in [
            "",
            "..",
            "../outside",
            "C:\\outside",
            "other/server",
            "a.b",
            "id with spaces",
        ] {
            assert!(scoped_rcon_password_path(&root, Some(invalid)).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_default_workspace_migrates_the_legacy_secret() {
        let root = fixture_secret_directory();
        let legacy = root.join("fxserver-rcon-password.dpapi");
        fs::write(&legacy, b"encrypted-fixture").unwrap();
        let other = scoped_rcon_password_path(&root, Some("server-2")).unwrap();
        assert!(!other.exists());
        assert!(legacy.exists());
        let migrated = scoped_rcon_password_path(&root, None).unwrap();
        assert_eq!(fs::read(&migrated).unwrap(), b"encrypted-fixture");
        assert!(!legacy.exists());
        assert_eq!(
            scoped_rcon_password_path(&root, Some("default")).unwrap(),
            migrated
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn replacing_encrypted_secret_preserves_the_new_password() {
        let root = fixture_secret_directory();
        let path = scoped_rcon_password_path(&root, Some("fixture")).unwrap();
        fs::write(&path, encode_hex(&encrypt_secret(b"old-password").unwrap())).unwrap();
        let password = "new-password-!&^\"-\u{1f512}";
        let encrypted = encrypt_secret(password.as_bytes()).unwrap();
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, encode_hex(&encrypted)).unwrap();
        replace_secret_file(&temporary, &path).unwrap();
        let saved = decode_hex(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(decrypt_secret(&saved).unwrap(), password.as_bytes());
        assert!(!temporary.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_switch_clears_logs_and_rejects_delayed_reader_output() {
        let manager = FxserverManager::default();
        append_terminal_line(&manager.terminal, "stdout", "old workspace").unwrap();
        let generation = manager.terminal.lock().unwrap().generation;
        manager.prepare_workspace_switch(|| Ok(())).unwrap();
        append_terminal_generation(
            &manager.terminal,
            "stdout",
            "delayed output",
            Some(generation),
        )
        .unwrap();
        assert!(manager.terminal.lock().unwrap().entries.is_empty());
        append_terminal_line(&manager.terminal, "system", "new workspace").unwrap();
        assert_eq!(manager.terminal.lock().unwrap().entries.len(), 1);
    }

    #[test]
    fn health_sampling_skips_busy_lifecycle_without_waiting() {
        let manager = FxserverManager::default();
        let _operation = manager.lifecycle.lock().unwrap();
        assert!(manager
            .sample_health(&mut HealthResourceSampler::default(), true)
            .unwrap()
            .is_none());
        assert!(manager.prepare_workspace_switch(|| Ok(())).is_err());
    }

    #[test]
    fn stop_and_shutdown_disarm_successful_launch_intent() {
        let manager = FxserverManager::default();
        let launch = SavedLaunch {
            artifact_path: "fixture".into(),
            environment: Vec::new(),
            server_profile: None,
        };
        assert!(manager.remember_launch(launch.clone(), 0).unwrap());
        let generation = manager.launch_generation().unwrap();
        assert!(manager.launch_intent.lock().unwrap().expected_running);
        stop_fxserver_blocking(&manager).unwrap();
        assert!(!manager.launch_intent.lock().unwrap().expected_running);
        assert!(!manager.remember_launch(launch.clone(), generation).unwrap());
        let generation = manager.launch_generation().unwrap();
        manager.begin_shutdown();
        assert!(!manager.remember_launch(launch, generation).unwrap());
        assert!(matches!(
            manager.recover_last_launch(generation, &AtomicBool::new(true)),
            RecoveryOutcome::Cancelled
        ));
    }

    #[test]
    fn recovery_requires_explicit_opt_in_and_a_successful_launch() {
        let manager = FxserverManager::default();
        assert!(matches!(
            manager.recover_last_launch(0, &AtomicBool::new(true)),
            RecoveryOutcome::Cancelled
        ));
        assert!(matches!(
            manager.recover_last_launch(0, &AtomicBool::new(false)),
            RecoveryOutcome::Cancelled
        ));
    }

    #[test]
    fn lifecycle_actions_cannot_overlap_across_manager_clones() {
        let manager = FxserverManager::default();
        let clone = manager.clone();
        let guard = manager.lifecycle.lock().unwrap();
        assert!(stop_fxserver_blocking(&clone)
            .unwrap_err()
            .contains("in progress"));
        drop(guard);
        stop_fxserver_blocking(&clone).unwrap();
    }

    #[test]
    fn clearing_terminal_keeps_cursor_ids_monotonic() {
        let manager = FxserverManager::default();
        append_terminal_line(&manager.terminal, "stdout", "before").unwrap();
        let next_id = manager.terminal.lock().unwrap().next_id;
        clear_terminal(&manager.terminal).unwrap();
        append_terminal_line(&manager.terminal, "stdout", "after").unwrap();
        assert_eq!(manager.terminal.lock().unwrap().next_id, next_id + 1);
    }

    #[test]
    fn quake_rcon_collects_multiple_packets_without_waiting_four_seconds() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let port = socket.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (_, peer) = socket.recv_from(&mut [0; 1024]).unwrap();
            socket
                .send_to(b"\xff\xff\xff\xffprint\nfirst", peer)
                .unwrap();
            thread::sleep(Duration::from_millis(30));
            socket
                .send_to(b"\xff\xff\xff\xffprint\nsecond", peer)
                .unwrap();
        });
        let started = Instant::now();
        let result = send_rcon_command(
            &FxserverRconConfig {
                host: "127.0.0.1".into(),
                port,
                password: "test".into(),
            },
            "status",
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(result, "first\nsecond");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn quake_rcon_has_an_absolute_deadline_even_with_continuous_output() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let port = socket.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (_, peer) = socket.recv_from(&mut [0; 1024]).unwrap();
            for _ in 0..40 {
                let _ = socket.send_to(b"\xff\xff\xff\xffprint\noutput", peer);
                thread::sleep(Duration::from_millis(20));
            }
        });
        let started = Instant::now();
        let result = send_rcon_command_with_timeout(
            &FxserverRconConfig {
                host: "127.0.0.1".into(),
                port,
                password: "test".into(),
            },
            "status",
            Duration::from_millis(200),
        )
        .unwrap();
        let elapsed = started.elapsed();
        server.join().unwrap();
        assert!(!result.is_empty());
        assert!(elapsed < Duration::from_millis(700));
    }

    #[test]
    fn quake_rcon_silent_response_is_bounded() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        let started = Instant::now();
        let result = send_rcon_command_with_timeout(
            &FxserverRconConfig {
                host: "127.0.0.1".into(),
                port,
                password: "test".into(),
            },
            "status",
            Duration::from_millis(100),
        )
        .unwrap();
        assert!(result.is_empty());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

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

    #[test]
    fn terminal_parser_strips_title_sequences_and_control_codes() {
        let parsed = parse_terminal_line("\x1b]0;FXServer\x07\x1b[38;5;86m[core]\x1b[0m hello");

        assert_eq!(parsed.plain_line, "[core] hello");
        assert_eq!(parsed.segments[0].text, "[core]");
        assert!(parsed.segments[0].emphasis.unwrap_or(false));
        assert_eq!(parsed.segments.last().expect("text").text, " hello");

        let title_only = parse_terminal_line("]0;FXServer title");
        assert_eq!(title_only.plain_line, "");
        assert!(title_only.segments.is_empty());
    }

    #[test]
    fn terminal_parser_preserves_split_ansi_colors() {
        let parsed = parse_terminal_line("\x1b[94mWelcome to \x1b[33mQbox!\x1b[0m");

        assert_eq!(parsed.plain_line, "Welcome to Qbox!");
        assert_eq!(parsed.segments.len(), 2);
        assert_eq!(parsed.segments[0].text, "Welcome to ");
        assert_eq!(parsed.segments[0].color.as_deref(), Some("#93c5fd"));
        assert_eq!(parsed.segments[1].text, "Qbox!");
        assert_eq!(parsed.segments[1].color.as_deref(), Some("#facc15"));
    }

    #[test]
    fn terminal_parser_keeps_text_after_malformed_csi() {
        let parsed = parse_terminal_line("\x1b[0;(1) FiveMQbox - txAdmin");

        assert_eq!(parsed.plain_line, " FiveMQbox - txAdmin");
        assert_eq!(
            parsed.segments.first().expect("text").text,
            " FiveMQbox - txAdmin"
        );
    }
}
