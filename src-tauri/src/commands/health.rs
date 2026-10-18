use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use super::fxserver::{
    FxserverManager, HealthProcessSample, HealthResourceSampler, RecoveryOutcome,
};

#[path = "health_policy.rs"]
mod policy;
use policy::{RecoveryPolicy, ThresholdGate};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
const EVENT_LIMIT: usize = 200;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthConfig {
    pub alerts_enabled: bool,
    pub recovery_enabled: bool,
    pub cpu_threshold_percent: f64,
    pub memory_threshold_percent: f64,
    pub minimum_free_disk_gb: f64,
    pub disk_path: String,
    pub sustained_seconds: u64,
    pub alert_cooldown_seconds: u64,
    pub recovery_backoff_seconds: u64,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            alerts_enabled: false,
            recovery_enabled: false,
            cpu_threshold_percent: 90.0,
            memory_threshold_percent: 80.0,
            minimum_free_disk_gb: 5.0,
            disk_path: String::new(),
            sustained_seconds: 15,
            alert_cooldown_seconds: 300,
            recovery_backoff_seconds: 30,
        }
    }
}

impl HealthConfig {
    fn validate(&mut self) -> Result<(), String> {
        if !valid_range(self.cpu_threshold_percent, 1.0, 100.0)
            || !valid_range(self.memory_threshold_percent, 1.0, 100.0)
        {
            return Err("CPU and RAM thresholds must be between 1 and 100 percent.".to_string());
        }
        if !valid_range(self.minimum_free_disk_gb, 0.0, 1_000_000.0) {
            return Err("Minimum free disk space must be between 0 and 1,000,000 GiB.".to_string());
        }
        if !(10..=600).contains(&self.sustained_seconds)
            || !(30..=3600).contains(&self.alert_cooldown_seconds)
            || !(10..=300).contains(&self.recovery_backoff_seconds)
        {
            return Err("Use a sustained period of 10-600 seconds, cooldown of 30-3600 seconds, and recovery backoff of 10-300 seconds.".to_string());
        }
        self.disk_path = self.disk_path.trim().to_string();
        if !self.disk_path.is_empty() {
            // Availability belongs to sampling: a vanished folder must not block opt-out.
            self.disk_path = local_disk_path(&self.disk_path)?
                .to_string_lossy()
                .to_string();
        }
        Ok(())
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthEvent {
    id: u64,
    timestamp: u64,
    level: String,
    kind: String,
    message: String,
    workspace_id: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSample {
    timestamp: u64,
    running: Option<bool>,
    pid: Option<u32>,
    cpu_percent: Option<f64>,
    memory_percent: Option<f64>,
    free_disk_gb: Option<f64>,
    disk_path: Option<String>,
    process_error: Option<String>,
    disk_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthStatus {
    workspace_id: String,
    config: HealthConfig,
    sample: Option<HealthSample>,
    events: Vec<HealthEvent>,
    recovery_armed: bool,
    recovery_blocked: bool,
    recovery_attempts: usize,
    next_recovery_seconds: Option<u64>,
}

struct MonitorState {
    app: Option<AppHandle>,
    workspace_id: String,
    workspace_initialized: bool,
    config: HealthConfig,
    revision: u64,
    sample: Option<HealthSample>,
    events: VecDeque<HealthEvent>,
    pending_events: VecDeque<HealthEvent>,
    next_event: u64,
    recovery: RecoveryPolicy,
    recovery_armed: bool,
    cpu: ThresholdGate,
    memory: ThresholdGate,
    disk: ThresholdGate,
    read_error: ThresholdGate,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            app: None,
            workspace_id: "default".to_string(),
            workspace_initialized: false,
            config: HealthConfig::default(),
            revision: 0,
            sample: None,
            events: VecDeque::new(),
            pending_events: VecDeque::new(),
            next_event: 0,
            recovery: RecoveryPolicy::default(),
            recovery_armed: false,
            cpu: ThresholdGate::default(),
            memory: ThresholdGate::default(),
            disk: ThresholdGate::default(),
            read_error: ThresholdGate::default(),
        }
    }
}

impl MonitorState {
    fn event(&mut self, level: &str, kind: &str, message: impl Into<String>) {
        self.next_event += 1;
        let message = message.into();
        let event = HealthEvent {
            id: self.next_event,
            timestamp: timestamp(),
            level: level.to_string(),
            kind: kind.to_string(),
            message,
            workspace_id: self.workspace_id.clone(),
        };
        self.pending_events.push_back(event.clone());
        self.events.push_back(event);
        while self.events.len() > EVENT_LIMIT {
            self.events.pop_front();
        }
        while self.pending_events.len() > EVENT_LIMIT {
            self.pending_events.pop_front();
        }
    }

    fn status(&self) -> HealthStatus {
        let now = Instant::now();
        HealthStatus {
            workspace_id: self.workspace_id.clone(),
            config: self.config.clone(),
            sample: self.sample.clone(),
            events: self.events.iter().rev().cloned().collect(),
            recovery_armed: self.recovery_armed
                && self.config.recovery_enabled
                && !self.recovery.blocked,
            recovery_blocked: self.recovery.blocked,
            recovery_attempts: self.recovery.attempt_count(now),
            next_recovery_seconds: if self.config.recovery_enabled {
                self.recovery.next_in_seconds(now)
            } else {
                None
            },
        }
    }

    fn reset_thresholds(&mut self) {
        self.cpu = ThresholdGate::default();
        self.memory = ThresholdGate::default();
        self.disk = ThresholdGate::default();
        self.read_error = ThresholdGate::default();
    }

    fn observe_sample(
        &mut self,
        process: Result<Option<HealthProcessSample>, String>,
        disk: Result<(PathBuf, f64), String>,
        now: Instant,
    ) -> Option<u64> {
        let (process, process_error) = match process {
            Ok(Some(process)) => (Some(process), None),
            Ok(None) => (None, Some("Server lifecycle is busy.".to_string())),
            Err(error) => (None, Some(error)),
        };
        let (disk_path, free_disk_gb, disk_error) = match disk {
            Ok((path, value)) => (Some(path.to_string_lossy().into_owned()), Some(value), None),
            Err(error) => (None, None, Some(error)),
        };
        let resources = process
            .as_ref()
            .filter(|process| process.running)
            .and_then(|process| process.resources.as_ref());
        let cpu_percent = resources
            .map(|resource| resource.cpu_percent)
            .filter(|value| valid_range(*value, 0.0, 100.0));
        let memory_percent = resources
            .map(|resource| resource.memory_percent)
            .filter(|value| valid_range(*value, 0.0, 100.0));
        let process_error = process_error.or_else(|| {
            (process.as_ref().is_some_and(|process| process.running)
                && (cpu_percent.is_none() || memory_percent.is_none()))
            .then(|| "Process metrics are unavailable. Check process access.".to_string())
        });
        let read_failed = (self.config.minimum_free_disk_gb > 0.0 && disk_error.is_some())
            || process_error.is_some();
        self.sample = Some(HealthSample {
            timestamp: timestamp(),
            running: process.as_ref().map(|process| process.running),
            pid: process
                .as_ref()
                .filter(|process| process.running)
                .and_then(|process| process.pid),
            cpu_percent,
            memory_percent,
            free_disk_gb,
            disk_path,
            process_error,
            disk_error,
        });

        let config = self.config.clone();
        let sustain = Duration::from_secs(config.sustained_seconds);
        let cooldown = Duration::from_secs(config.alert_cooldown_seconds);
        let backoff = Duration::from_secs(config.recovery_backoff_seconds);
        if config.alerts_enabled {
            if self.cpu.observe(
                cpu_percent.is_some_and(|value| value >= config.cpu_threshold_percent),
                now,
                sustain,
                cooldown,
            ) {
                self.event(
                    "warn",
                    "cpu",
                    format!(
                        "FXServer CPU usage has remained above {:.0}% for {} seconds.",
                        config.cpu_threshold_percent, config.sustained_seconds
                    ),
                );
            }
            if self.memory.observe(
                memory_percent.is_some_and(|value| value >= config.memory_threshold_percent),
                now,
                sustain,
                cooldown,
            ) {
                self.event("warn", "memory", format!("FXServer RAM usage has remained above {:.0}% of physical memory for {} seconds.", config.memory_threshold_percent, config.sustained_seconds));
            }
            if self.disk.observe(
                free_disk_gb.is_some_and(|value| value < config.minimum_free_disk_gb),
                now,
                sustain,
                cooldown,
            ) {
                self.event(
                    "warn",
                    "disk",
                    format!(
                        "Free disk space is {:.1} GiB, below the {:.1} GiB threshold.",
                        free_disk_gb.unwrap_or_default(),
                        config.minimum_free_disk_gb
                    ),
                );
            }
            if self.read_error.observe(read_failed, now, sustain, cooldown) {
                self.event("warn", "sampling", "Some native health metrics are unavailable. Check the selected disk folder and process access.");
            }
        }

        // Busy or failed reads are unknown, not evidence of a crash.
        let process = process?;
        self.recovery_armed = process.expected_running;
        let crashed = self.recovery.observe(
            process.generation,
            process.expected_running,
            process.running,
            now,
            backoff,
        );
        if crashed && (config.alerts_enabled || config.recovery_enabled) {
            self.event("warn", "crash", "FXServer exited unexpectedly.");
        }
        if !config.recovery_enabled {
            self.recovery.disable();
            return None;
        }
        self.recovery.due(now).then_some(process.generation)
    }
}

#[derive(Default)]
struct MonitorInner {
    state: Mutex<MonitorState>,
    wake: Condvar,
    started: AtomicBool,
    stopped: AtomicBool,
    recovery_enabled: AtomicBool,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

#[derive(Clone, Default)]
pub struct HealthMonitor {
    inner: Arc<MonitorInner>,
}

impl HealthMonitor {
    fn configure(
        &self,
        mut config: HealthConfig,
        workspace_id: &str,
    ) -> Result<HealthStatus, String> {
        config.validate()?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| "Health monitor is unavailable.".to_string())?;
        if state.workspace_id != workspace_id {
            return Err("The active workspace changed. Reload health settings.".to_string());
        }
        if self.inner.stopped.load(Ordering::Acquire) {
            return Err("FXServer Installer is shutting down.".to_string());
        }
        self.inner
            .recovery_enabled
            .store(config.recovery_enabled, Ordering::Release);
        if !config.recovery_enabled {
            state.recovery.disable()
        }
        if config.recovery_enabled && !state.config.recovery_enabled {
            state.recovery.resume()
        }
        state.config = config;
        state.sample = None;
        state.revision = state.revision.wrapping_add(1);
        state.reset_thresholds();
        state.event(
            "info",
            "settings",
            "Health monitoring settings updated for this session.",
        );
        let status = state.status();
        drop(state);
        self.inner.wake.notify_all();
        self.publish_events();
        Ok(status)
    }

    pub fn start(&self, manager: FxserverManager, app: AppHandle) -> Result<(), String> {
        if self.inner.started.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        self.inner
            .state
            .lock()
            .map_err(|_| "Health monitor is unavailable.".to_string())?
            .app = Some(app);
        let monitor = self.clone();
        match thread::Builder::new()
            .name("fxserver-health".to_string())
            .spawn(move || monitor.run(manager))
        {
            Ok(worker) => {
                *self
                    .inner
                    .worker
                    .lock()
                    .map_err(|_| "Health worker is unavailable.".to_string())? = Some(worker)
            }
            Err(error) => {
                self.inner.started.store(false, Ordering::Release);
                return Err(format!("Failed to start health monitoring: {error}"));
            }
        }
        Ok(())
    }

    pub fn stop(&self, manager: &FxserverManager) {
        self.inner.recovery_enabled.store(false, Ordering::Release);
        manager.begin_shutdown();
        let _state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.inner.stopped.store(true, Ordering::Release);
        self.inner.wake.notify_all();
    }

    pub fn wait_stopped(&self) {
        if let Some(worker) = self
            .inner
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = worker.join();
        }
    }

    fn publish_events(&self) {
        let (app, events) = {
            let Ok(mut state) = self.inner.state.lock() else {
                return;
            };
            (state.app.clone(), std::mem::take(&mut state.pending_events))
        };
        if let Some(app) = app {
            for event in events {
                super::logs::append_background_log(
                    &app,
                    &event.level,
                    "fxserver.health",
                    &event.message,
                );
                let _ = app.emit("fxserver-health-event", event);
            }
        }
    }

    fn run(&self, manager: FxserverManager) {
        let mut sampler = HealthResourceSampler::default();
        loop {
            let Ok(state) = self.inner.state.lock() else {
                break;
            };
            if self.inner.stopped.load(Ordering::Acquire) {
                break;
            }
            let config = state.config.clone();
            let revision = state.revision;
            drop(state);
            self.tick(&manager, &mut sampler, &config, revision);
            self.publish_events();
            let Ok(state) = self.inner.state.lock() else {
                break;
            };
            // Sample immediately, then sleep. Revision checks also catch updates
            // made during a sample and prevent spurious wakes from busy-looping.
            let Ok((_state, _)) =
                self.inner
                    .wake
                    .wait_timeout_while(state, SAMPLE_INTERVAL, |state| {
                        state.revision == revision && !self.inner.stopped.load(Ordering::Acquire)
                    })
            else {
                break;
            };
        }
        self.publish_events();
    }

    fn tick(
        &self,
        manager: &FxserverManager,
        sampler: &mut HealthResourceSampler,
        config: &HealthConfig,
        revision: u64,
    ) {
        let process = manager.sample_health(sampler, true);
        let disk = sample_disk(&config.disk_path);
        let Ok(mut state) = self.inner.state.lock() else {
            return;
        };
        if state.revision != revision || self.inner.stopped.load(Ordering::Acquire) {
            return;
        }
        let backoff = Duration::from_secs(config.recovery_backoff_seconds);
        let Some(generation) = state.observe_sample(process, disk, Instant::now()) else {
            return;
        };
        drop(state);
        let outcome = manager.recover_last_launch(generation, &self.inner.recovery_enabled);
        let Ok(mut state) = self.inner.state.lock() else {
            return;
        };
        if state.revision != revision
            || !state.recovery.matches_generation(generation)
            || self.inner.stopped.load(Ordering::Acquire)
        {
            return;
        }
        let succeeded = match outcome {
            RecoveryOutcome::Busy => return,
            RecoveryOutcome::Cancelled => {
                state.recovery.disable();
                return;
            }
            RecoveryOutcome::Started => {
                state.event(
                    "info",
                    "recovery",
                    "FXServer was restarted after an unexpected exit.",
                );
                true
            }
            RecoveryOutcome::Failed(error) => {
                state.event(
                    "error",
                    "recovery",
                    format!("FXServer recovery failed: {error}"),
                );
                false
            }
        };
        if state
            .recovery
            .record_attempt(Instant::now(), backoff, succeeded)
        {
            state.event("warn", "recovery-limit", "Automatic recovery is paused after three attempts in ten minutes. Check the server logs and start the server manually to re-arm recovery.");
        }
    }
}

#[tauri::command]
pub async fn get_health_status(
    monitor: tauri::State<'_, HealthMonitor>,
) -> Result<HealthStatus, String> {
    monitor
        .inner
        .state
        .lock()
        .map(|state| state.status())
        .map_err(|_| "Health monitor is unavailable.".to_string())
}

#[tauri::command]
pub async fn configure_health(
    config: HealthConfig,
    workspace_id: String,
    monitor: tauri::State<'_, HealthMonitor>,
) -> Result<HealthStatus, String> {
    let monitor = monitor.inner().clone();
    super::run_blocking(move || monitor.configure(config, &workspace_id)).await
}

#[tauri::command]
pub async fn clear_health_events(monitor: tauri::State<'_, HealthMonitor>) -> Result<(), String> {
    monitor
        .inner
        .state
        .lock()
        .map_err(|_| "Health monitor is unavailable.".to_string())?
        .events
        .clear();
    Ok(())
}

#[tauri::command]
pub async fn prepare_workspace_switch(
    workspace_id: String,
    manager: tauri::State<'_, FxserverManager>,
    monitor: tauri::State<'_, HealthMonitor>,
) -> Result<(), String> {
    if workspace_id.is_empty()
        || workspace_id.len() > 64
        || !workspace_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("Invalid workspace ID.".to_string());
    }
    let manager = manager.inner().clone();
    let monitor = monitor.inner().clone();
    super::run_blocking(move || {
        manager.prepare_workspace_switch(|| {
            super::require_other_work_idle()?;
            let mut state = monitor
                .inner
                .state
                .lock()
                .map_err(|_| "Health monitor is unavailable.".to_string())?;
            monitor
                .inner
                .recovery_enabled
                .store(false, Ordering::Release);
            let revision = state.revision.wrapping_add(1);
            let next_event = state.next_event;
            let app = state.app.clone();
            let pending_events = std::mem::take(&mut state.pending_events);
            manager.set_incident_workspace(&workspace_id)?;
            *state = MonitorState {
                app,
                pending_events,
                workspace_id,
                workspace_initialized: true,
                revision,
                next_event,
                ..MonitorState::default()
            };
            Ok(())
        })?;
        monitor.inner.wake.notify_all();
        monitor.publish_events();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn initialize_health_workspace(
    workspace_id: String,
    monitor: tauri::State<'_, HealthMonitor>,
    manager: tauri::State<'_, FxserverManager>,
) -> Result<(), String> {
    if workspace_id.is_empty()
        || workspace_id.len() > 64
        || !workspace_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("Invalid workspace ID.".to_string());
    }
    let mut state = monitor
        .inner
        .state
        .lock()
        .map_err(|_| "Health monitor is unavailable.".to_string())?;
    if state.workspace_initialized && state.workspace_id != workspace_id {
        return Err("Use the workspace switch action to change the active workspace.".to_string());
    }
    manager.set_incident_workspace(&workspace_id)?;
    state.workspace_id = workspace_id;
    state.workspace_initialized = true;
    Ok(())
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn valid_range(value: f64, min: f64, max: f64) -> bool {
    value.is_finite() && (min..=max).contains(&value)
}

fn validate_disk_path(path: &str) -> Result<PathBuf, String> {
    let path = local_disk_path(path)?;
    validate_local_disk_folder(&path)?;
    Ok(path)
}

#[cfg(not(target_os = "linux"))]
fn local_disk_path(path: &str) -> Result<PathBuf, String> {
    let bytes = path.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
        || path.chars().any(char::is_control)
        || path[3..].contains([':', '"', '<', '>', '|', '?', '*'])
        || path[3..].split(['\\', '/']).any(|part| {
            let name = part
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            part == "."
                || part == ".."
                || part.ends_with(['.', ' '])
                || matches!(name.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (name.len() == 4
                    && (name.starts_with("COM") || name.starts_with("LPT"))
                    && name.as_bytes()[3].is_ascii_digit())
        })
    {
        return Err("Choose an existing folder on a local drive, or leave the disk folder blank to use the app folder. Network, device, and relative paths are not supported.".to_string());
    }
    Ok(PathBuf::from(path))
}

#[cfg(target_os = "windows")]
fn validate_local_disk_folder(path: &Path) -> Result<(), String> {
    use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, FILE_ATTRIBUTE_REPARSE_POINT};

    let root = path.ancestors().last().ok_or("Invalid disk folder.")?;
    let root: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    // DRIVE_REMOVABLE, DRIVE_FIXED, and DRIVE_RAMDISK only. Check before
    // touching metadata so a mapped network drive is never traversed.
    if !matches!(unsafe { GetDriveTypeW(root.as_ptr()) }, 2 | 3 | 6) {
        return Err("Disk monitoring requires an available local drive.".to_string());
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if !current.has_root() {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&current).map_err(|_| {
            "The disk monitoring folder does not exist or is inaccessible.".to_string()
        })?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("Disk monitoring does not follow symbolic links or junctions.".to_string());
        }
        if !metadata.is_dir() {
            return Err("The disk monitoring path must be a folder.".to_string());
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn local_disk_path(path: &str) -> Result<PathBuf, String> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err("Choose an absolute local folder without traversal or symbolic links.".into());
    }
    Ok(PathBuf::from(path))
}

#[cfg(target_os = "linux")]
fn validate_local_disk_folder(path: &Path) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    let directory = super::workspace_clone::linux_fs::directory(path)?;
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(directory.as_raw_fd(), stats.as_mut_ptr()) } != 0 {
        return Err(format!(
            "Cannot inspect disk filesystem: {}",
            std::io::Error::last_os_error()
        ));
    }
    let kind = unsafe { stats.assume_init() }.f_type as u64;
    // NFS, SMB/CIFS, 9P and pseudo-filesystems are not local disk capacity.
    if matches!(
        kind,
        0x6969 | 0xff534d42 | 0xfe534d42 | 0x517b | 0x9fa0 | 0x62656572 | 0x63677270 | 0x27e0eb
    ) {
        return Err("Disk monitoring requires a local storage filesystem.".into());
    }
    Ok(())
}

fn sample_disk(configured_path: &str) -> Result<(PathBuf, f64), String> {
    let path = if configured_path.trim().is_empty() {
        let executable = std::env::current_exe()
            .map_err(|_| "The app folder is unavailable. Choose a disk folder.".to_string())?;
        let folder = executable
            .parent()
            .ok_or("The app folder is unavailable.")?;
        let folder = folder.to_string_lossy();
        // current_exe may use a verbatim local path; user input may not.
        validate_disk_path(folder.strip_prefix("\\\\?\\").unwrap_or(&folder))?
    } else {
        validate_disk_path(configured_path)?
    };
    let value = free_disk_gb(&path)?;
    Ok((path, value))
}

#[cfg(target_os = "windows")]
fn free_disk_gb(path: &Path) -> Result<f64, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0;
    if unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(format!(
            "Failed to read available disk space: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(available as f64 / 1_073_741_824.0)
}

#[cfg(target_os = "linux")]
fn free_disk_gb(path: &Path) -> Result<f64, String> {
    Ok(super::workspace_clone::linux_fs::available_bytes(path)? as f64 / 1_073_741_824.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alerts_and_recovery_default_to_disabled() {
        let status = MonitorState::default().status();
        assert!(!status.config.alerts_enabled);
        assert!(!status.config.recovery_enabled);
        assert!(!status.recovery_armed);
    }

    #[test]
    fn invalid_thresholds_and_backoff_are_rejected() {
        for threshold in [f64::NAN, f64::INFINITY, 0.0, 101.0] {
            let mut config = HealthConfig {
                cpu_threshold_percent: threshold,
                ..HealthConfig::default()
            };
            assert!(config.validate().is_err());
        }
        let mut config = HealthConfig {
            recovery_backoff_seconds: 0,
            ..HealthConfig::default()
        };
        assert!(config.validate().is_err());
        config.recovery_backoff_seconds = 30;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn history_is_bounded_and_newest_first() {
        let mut state = MonitorState::default();
        for i in 0..250 {
            state.event("info", "test", i.to_string())
        }
        assert_eq!(state.status().events.len(), EVENT_LIMIT);
        assert_eq!(state.status().events[0].message, "249");
    }

    #[test]
    fn disk_validation_rejects_network_and_relative_paths() {
        for path in [
            "",
            ".",
            "..",
            "\\\\host\\share",
            "C:relative",
            "C:\\path\0suffix",
            "\\\\?\\C:\\folder",
            "\\\\.\\C:\\folder",
            "C:\\folder\\..\\other",
            "C:\\folder\\.\\other",
            "C:\\folder:stream",
            "C:\\folder.\\other",
            "C:\\folder \\other",
            "C:\\NUL",
            "C:\\COM1\\folder",
            "C:\\folder\nname",
            "C:\\*",
        ] {
            assert!(validate_disk_path(path).is_err());
        }
    }

    fn process_fixture(running: bool, expected_running: bool) -> HealthProcessSample {
        HealthProcessSample {
            generation: 1,
            expected_running,
            running,
            pid: running.then_some(1234),
            resources: running.then_some(crate::models::fxserver::FxserverResources {
                cpu_percent: 12.5,
                memory_bytes: 24,
                total_memory_bytes: 100,
                memory_percent: 24.0,
                thread_count: 1,
                handle_count: 1,
            }),
        }
    }

    fn disk_fixture() -> Result<(PathBuf, f64), String> {
        Ok((PathBuf::from("C:\\health-fixture"), 42.0))
    }

    #[test]
    fn passive_sampling_records_resources_without_opt_ins() {
        let mut state = MonitorState::default();
        assert_eq!(
            state.observe_sample(
                Ok(Some(process_fixture(true, true))),
                disk_fixture(),
                Instant::now()
            ),
            None
        );
        let status = state.status();
        let sample = status.sample.unwrap();
        assert_eq!(sample.running, Some(true));
        assert_eq!(sample.cpu_percent, Some(12.5));
        assert_eq!(sample.memory_percent, Some(24.0));
        assert_eq!(sample.free_disk_gb, Some(42.0));
        assert_eq!(sample.disk_path.as_deref(), Some("C:\\health-fixture"));
        assert!(sample.timestamp > 0);
        assert!(!status.config.alerts_enabled);
        assert!(!status.config.recovery_enabled);
        assert!(!status.recovery_armed);
        assert_eq!(status.next_recovery_seconds, None);
        assert!(status.events.is_empty());
    }

    #[test]
    fn passive_thresholds_crashes_and_errors_do_not_alert_or_schedule_recovery() {
        let mut state = MonitorState::default();
        let now = Instant::now();
        for seconds in [0, 30, 60] {
            let mut process = process_fixture(true, true);
            let resources = process.resources.as_mut().unwrap();
            resources.cpu_percent = 99.0;
            resources.memory_percent = 99.0;
            assert_eq!(
                state.observe_sample(
                    Ok(Some(process)),
                    Ok((PathBuf::from("C:\\fixture"), 0.1)),
                    now + Duration::from_secs(seconds)
                ),
                None
            );
        }
        for seconds in [90, 120, 180] {
            assert_eq!(
                state.observe_sample(
                    Ok(Some(process_fixture(false, true))),
                    disk_fixture(),
                    now + Duration::from_secs(seconds)
                ),
                None
            );
            assert!(!state.recovery.due(now + Duration::from_secs(3600)));
        }
        for seconds in [210, 240, 600] {
            assert_eq!(
                state.observe_sample(
                    Err("fixture process failure".into()),
                    Err("fixture disk failure".into()),
                    now + Duration::from_secs(seconds)
                ),
                None
            );
        }
        assert!(state.events.is_empty());
        assert!(state.pending_events.is_empty());
        assert_eq!(state.status().recovery_attempts, 0);
        assert!(!state.status().recovery_armed);
    }

    #[test]
    fn stopped_and_unavailable_samples_replace_old_process_metrics() {
        let mut state = MonitorState::default();
        let now = Instant::now();
        state.observe_sample(Ok(Some(process_fixture(true, true))), disk_fixture(), now);
        state.observe_sample(Ok(Some(process_fixture(false, false))), disk_fixture(), now);
        let sample = state.sample.as_ref().unwrap();
        assert_eq!(sample.running, Some(false));
        assert_eq!(sample.pid, None);
        assert_eq!(sample.cpu_percent, None);
        assert_eq!(sample.memory_percent, None);
        assert_eq!(sample.free_disk_gb, Some(42.0));
        assert!(sample.process_error.is_none());
        state.observe_sample(Ok(None), disk_fixture(), now);
        let sample = state.sample.as_ref().unwrap();
        assert_eq!(sample.running, None);
        assert_eq!(sample.free_disk_gb, Some(42.0));
        assert!(sample.process_error.is_some());
        state.observe_sample(
            Err("process failure".into()),
            Err("disk failure".into()),
            now,
        );
        let sample = state.sample.as_ref().unwrap();
        assert_eq!(sample.running, None);
        assert_eq!(sample.free_disk_gb, None);
        assert_eq!(sample.disk_path, None);
        assert_eq!(sample.process_error.as_deref(), Some("process failure"));
        assert_eq!(sample.disk_error.as_deref(), Some("disk failure"));
    }

    #[test]
    fn missing_or_invalid_resources_do_not_look_like_a_stopped_server() {
        let mut state = MonitorState::default();
        for resources in [None, Some((f64::NAN, f64::INFINITY)), Some((-1.0, 101.0))] {
            let mut process = process_fixture(true, true);
            if let Some((cpu, memory)) = resources {
                process.resources.as_mut().unwrap().cpu_percent = cpu;
                process.resources.as_mut().unwrap().memory_percent = memory;
            } else {
                process.resources = None;
            }
            state.observe_sample(Ok(Some(process)), disk_fixture(), Instant::now());
            let sample = state.sample.as_ref().unwrap();
            assert_eq!(sample.running, Some(true));
            assert_eq!(sample.pid, Some(1234));
            assert_eq!(sample.cpu_percent, None);
            assert_eq!(sample.memory_percent, None);
            assert!(sample.process_error.is_some());
        }
    }

    #[test]
    fn alerts_require_opt_in_and_do_not_enable_recovery() {
        let mut state = MonitorState {
            config: HealthConfig {
                alerts_enabled: true,
                cpu_threshold_percent: 10.0,
                memory_threshold_percent: 20.0,
                minimum_free_disk_gb: 50.0,
                ..HealthConfig::default()
            },
            ..MonitorState::default()
        };
        let now = Instant::now();
        for seconds in [0, 15, 30] {
            assert_eq!(
                state.observe_sample(
                    Ok(Some(process_fixture(true, true))),
                    disk_fixture(),
                    now + Duration::from_secs(seconds)
                ),
                None
            );
        }
        let kinds: Vec<_> = state
            .events
            .iter()
            .map(|event| event.kind.as_str())
            .collect();
        assert_eq!(kinds, ["cpu", "memory", "disk"]);
        assert_eq!(
            state.observe_sample(
                Ok(Some(process_fixture(false, true))),
                disk_fixture(),
                now + Duration::from_secs(60)
            ),
            None
        );
        assert!(!state.recovery.due(now + Duration::from_secs(3600)));
    }

    #[test]
    fn recovery_opt_in_never_treats_busy_or_failed_reads_as_crashes() {
        let mut state = MonitorState {
            config: HealthConfig {
                recovery_enabled: true,
                ..HealthConfig::default()
            },
            ..MonitorState::default()
        };
        let now = Instant::now();
        state.observe_sample(Ok(Some(process_fixture(true, true))), disk_fixture(), now);
        assert_eq!(
            state.observe_sample(Ok(None), disk_fixture(), now + Duration::from_secs(30)),
            None
        );
        assert_eq!(
            state.observe_sample(
                Err("fixture".into()),
                disk_fixture(),
                now + Duration::from_secs(60)
            ),
            None
        );
        assert!(state.events.is_empty());
        assert!(!state.recovery.due(now + Duration::from_secs(3600)));
        assert_eq!(
            state.observe_sample(
                Ok(Some(process_fixture(false, true))),
                disk_fixture(),
                now + Duration::from_secs(90)
            ),
            None
        );
        assert_eq!(
            state.observe_sample(
                Ok(Some(process_fixture(false, true))),
                disk_fixture(),
                now + Duration::from_secs(120)
            ),
            Some(1)
        );
        assert_eq!(
            state.observe_sample(Ok(None), disk_fixture(), now + Duration::from_secs(150)),
            None
        );
        assert_eq!(
            state.status().recovery_attempts,
            0,
            "Fixtures must never perform recovery"
        );
    }

    #[test]
    fn zero_disk_threshold_disables_disk_alerts_not_readings() {
        let mut state = MonitorState {
            config: HealthConfig {
                alerts_enabled: true,
                minimum_free_disk_gb: 0.0,
                ..HealthConfig::default()
            },
            ..MonitorState::default()
        };
        let now = Instant::now();
        for seconds in [0, 30] {
            state.observe_sample(
                Ok(Some(process_fixture(false, false))),
                disk_fixture(),
                now + Duration::from_secs(seconds),
            );
        }
        assert_eq!(state.sample.as_ref().unwrap().free_disk_gb, Some(42.0));
        for seconds in [60, 90] {
            state.observe_sample(
                Ok(Some(process_fixture(false, false))),
                Err("disk failure".into()),
                now + Duration::from_secs(seconds),
            );
        }
        assert!(state.events.is_empty());
    }

    #[test]
    fn automatic_disk_path_is_valid_with_every_opt_in_combination() {
        for alerts_enabled in [false, true] {
            for recovery_enabled in [false, true] {
                let mut config = HealthConfig {
                    alerts_enabled,
                    recovery_enabled,
                    disk_path: "  ".into(),
                    ..HealthConfig::default()
                };
                assert!(config.validate().is_ok());
                assert!(config.disk_path.is_empty());
                config.disk_path = "\\\\host\\share".into();
                assert!(config.validate().is_err());
                config.minimum_free_disk_gb = 0.0;
                assert!(config.validate().is_err());
            }
        }
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn vanished_disk_folder_does_not_block_disabling_either_opt_in() {
        let folder = std::env::temp_dir().join(format!(
            "fxserver-health-optout-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for (alerts_enabled, recovery_enabled) in [(true, false), (false, true), (false, false)] {
            std::fs::create_dir(&folder).unwrap();
            let monitor = HealthMonitor::default();
            let mut config = HealthConfig {
                alerts_enabled: true,
                recovery_enabled: true,
                disk_path: folder.to_string_lossy().into_owned(),
                ..HealthConfig::default()
            };
            assert!(sample_disk(&config.disk_path).is_ok());
            monitor.configure(config.clone(), "default").unwrap();
            let now = Instant::now();
            monitor.inner.state.lock().unwrap().observe_sample(
                Ok(Some(process_fixture(false, true))),
                disk_fixture(),
                now,
            );
            std::fs::remove_dir(&folder).unwrap();

            config.alerts_enabled = alerts_enabled;
            config.recovery_enabled = recovery_enabled;
            let status = monitor.configure(config.clone(), "default").unwrap();
            assert_eq!(status.config.alerts_enabled, alerts_enabled);
            assert_eq!(status.config.recovery_enabled, recovery_enabled);
            assert_eq!(status.config.disk_path, config.disk_path);
            assert_eq!(
                monitor.inner.recovery_enabled.load(Ordering::Acquire),
                recovery_enabled
            );
            if !recovery_enabled {
                assert!(!status.recovery_armed);
                assert_eq!(status.next_recovery_seconds, None);
            }
            let mut state = monitor.inner.state.lock().unwrap();
            assert_eq!(
                state.observe_sample(
                    Ok(Some(process_fixture(false, false))),
                    sample_disk(&config.disk_path),
                    now + Duration::from_secs(60)
                ),
                None
            );
            let sample = state.sample.as_ref().unwrap();
            assert!(sample.disk_error.is_some());
            assert_eq!(sample.free_disk_gb, None);
            assert_eq!(state.status().recovery_attempts, 0);
        }
    }

    #[test]
    fn saving_unavailable_disk_folders_still_rejects_unsafe_path_syntax() {
        let monitor = HealthMonitor::default();
        for path in [
            "\\\\host\\share",
            "C:relative",
            "\\\\?\\C:\\folder",
            "C:\\folder:stream",
            "C:\\folder\\..\\other",
            "C:\\NUL",
            "C:\\folder\nname",
            "C:\\*",
        ] {
            let config = HealthConfig {
                disk_path: path.into(),
                ..HealthConfig::default()
            };
            assert!(monitor.configure(config, "default").is_err(), "{path}");
        }
        let state = monitor.inner.state.lock().unwrap();
        assert_eq!(state.revision, 0);
        assert!(state.events.is_empty());
        assert!(!monitor.inner.recovery_enabled.load(Ordering::Acquire));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn local_disk_path_accepts_drive_roots_and_plain_absolute_folders() {
        for path in ["C:\\", "D:/", "C:\\folder\\nested", "D:/folder/nested"] {
            assert!(local_disk_path(path).is_ok(), "{path}");
        }
    }

    #[test]
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    fn automatic_disk_sampling_reads_the_local_app_folder() {
        let (path, value) = sample_disk("").unwrap();
        assert!(path.is_absolute());
        assert!(path.is_dir());
        assert!(value.is_finite() && value >= 0.0);
        let mut config = HealthConfig {
            disk_path: path.to_string_lossy().into_owned(),
            ..HealthConfig::default()
        };
        assert!(config.validate().is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_disk_paths_are_absolute_case_sensitive_and_nofollow() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!(
            "fxsi-health-{}",
            super::super::workspace_clone::linux_fs::token().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("Data")).unwrap();
        symlink(root.join("Data"), root.join("link")).unwrap();
        assert!(sample_disk(root.join("Data").to_str().unwrap()).unwrap().1 >= 0.0);
        assert!(sample_disk(root.join("data").to_str().unwrap()).is_err());
        assert!(sample_disk(root.join("link").to_str().unwrap()).is_err());
        for path in [
            "relative",
            "//host/share",
            "/tmp/../etc",
            "/tmp/./data",
            "/tmp/a\\b",
            "/tmp/a\0b",
        ] {
            assert!(local_disk_path(path).is_err(), "{path}");
        }
        assert!(validate_disk_path(Path::new("/proc").to_str().unwrap()).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn worker_samples_immediately_with_both_opt_ins_off_and_wakes_on_revision() {
        let manager = FxserverManager::default();
        let monitor = HealthMonitor::default();
        let worker_monitor = monitor.clone();
        let worker_manager = manager.clone();
        let worker = thread::spawn(move || worker_monitor.run(worker_manager));
        let wait_sample = || {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if let Some(sample) = monitor.inner.state.lock().unwrap().sample.clone() {
                    return Some(sample);
                }
                if Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(10));
            }
        };
        let initial = wait_sample();
        {
            let mut state = monitor.inner.state.lock().unwrap();
            state.revision += 1;
            state.sample = None;
        }
        monitor.inner.wake.notify_all();
        let updated = wait_sample();
        monitor.stop(&manager);
        worker.join().unwrap();
        assert_eq!(initial.unwrap().running, Some(false));
        assert_eq!(updated.unwrap().running, Some(false));
        let state = monitor.inner.state.lock().unwrap();
        assert!(!state.config.alerts_enabled);
        assert!(!state.config.recovery_enabled);
        assert!(state.events.is_empty());
    }

    #[test]
    fn obsolete_or_shutdown_ticks_cannot_publish_samples() {
        let manager = FxserverManager::default();
        let monitor = HealthMonitor::default();
        let mut sampler = HealthResourceSampler::default();
        monitor.tick(&manager, &mut sampler, &HealthConfig::default(), 1);
        assert!(monitor.inner.state.lock().unwrap().sample.is_none());
        monitor.stop(&manager);
        monitor.tick(&manager, &mut sampler, &HealthConfig::default(), 0);
        assert!(monitor.inner.state.lock().unwrap().sample.is_none());
    }

    #[test]
    fn stopping_monitor_does_not_wait_for_the_next_sample() {
        let manager = FxserverManager::default();
        let monitor = HealthMonitor::default();
        let worker_monitor = monitor.clone();
        let worker_manager = manager.clone();
        let (ready, waiting) = std::sync::mpsc::channel();
        let (done, finished) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            ready.send(()).unwrap();
            worker_monitor.run(worker_manager);
            done.send(()).unwrap();
        });
        waiting.recv_timeout(Duration::from_secs(1)).unwrap();
        monitor.stop(&manager);
        finished.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
        assert!(monitor.inner.stopped.load(Ordering::Acquire));
    }
}
