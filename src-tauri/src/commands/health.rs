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

use super::fxserver::{FxserverManager, HealthResourceSampler, RecoveryOutcome};

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
        if self.alerts_enabled && self.minimum_free_disk_gb > 0.0 {
            self.disk_path = validate_disk_path(&self.disk_path)?
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
    running: bool,
    pid: Option<u32>,
    cpu_percent: Option<f64>,
    memory_percent: Option<f64>,
    free_disk_gb: Option<f64>,
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
            let Ok((state, _)) = self.inner.wake.wait_timeout(state, SAMPLE_INTERVAL) else {
                break;
            };
            if self.inner.stopped.load(Ordering::Acquire) {
                break;
            }
            let config = state.config.clone();
            let revision = state.revision;
            drop(state);
            if !config.alerts_enabled && !config.recovery_enabled {
                continue;
            }
            self.tick(&manager, &mut sampler, &config, revision);
            self.publish_events();
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
        let now = Instant::now();
        let process = match manager.sample_health(sampler, config.alerts_enabled) {
            Ok(Some(process)) => process,
            Ok(None) => return,
            Err(error) => {
                self.record_read_error(revision, error);
                return;
            }
        };
        let free_disk = if config.alerts_enabled && config.minimum_free_disk_gb > 0.0 {
            Some(free_disk_gb(Path::new(&config.disk_path)))
        } else {
            None
        };
        let Ok(mut state) = self.inner.state.lock() else {
            return;
        };
        if state.revision != revision || self.inner.stopped.load(Ordering::Acquire) {
            return;
        }
        let sustain = Duration::from_secs(config.sustained_seconds);
        let cooldown = Duration::from_secs(config.alert_cooldown_seconds);
        let backoff = Duration::from_secs(config.recovery_backoff_seconds);
        let cpu_percent = process
            .resources
            .as_ref()
            .map(|resource| resource.cpu_percent);
        let memory_percent = process
            .resources
            .as_ref()
            .map(|resource| resource.memory_percent);
        let disk_gb = free_disk
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .copied();
        state.sample = Some(HealthSample {
            timestamp: timestamp(),
            running: process.running,
            pid: process.pid,
            cpu_percent,
            memory_percent,
            free_disk_gb: disk_gb,
        });
        if config.alerts_enabled {
            if state.cpu.observe(
                cpu_percent.is_some_and(|value| value >= config.cpu_threshold_percent),
                now,
                sustain,
                cooldown,
            ) {
                state.event(
                    "warn",
                    "cpu",
                    format!(
                        "FXServer CPU usage has remained above {:.0}% for {} seconds.",
                        config.cpu_threshold_percent, config.sustained_seconds
                    ),
                );
            }
            if state.memory.observe(
                memory_percent.is_some_and(|value| value >= config.memory_threshold_percent),
                now,
                sustain,
                cooldown,
            ) {
                state.event("warn", "memory", format!("FXServer RAM usage has remained above {:.0}% of physical memory for {} seconds.", config.memory_threshold_percent, config.sustained_seconds));
            }
            if state.disk.observe(
                disk_gb.is_some_and(|value| value < config.minimum_free_disk_gb),
                now,
                sustain,
                cooldown,
            ) {
                state.event(
                    "warn",
                    "disk",
                    format!(
                        "Free disk space is {:.1} GiB, below the {:.1} GiB threshold.",
                        disk_gb.unwrap_or_default(),
                        config.minimum_free_disk_gb
                    ),
                );
            }
            let read_failed = free_disk.as_ref().is_some_and(Result::is_err)
                || (process.running && process.resources.is_none());
            if state
                .read_error
                .observe(read_failed, now, sustain, cooldown)
            {
                state.event("warn", "sampling", "Some native health metrics are unavailable. Check the selected disk folder and process access.");
            }
        }
        state.recovery_armed = process.expected_running;
        let crashed = state.recovery.observe(
            process.generation,
            process.expected_running,
            process.running,
            now,
            backoff,
        );
        if crashed {
            state.event("warn", "crash", "FXServer exited unexpectedly.");
        }
        if !config.recovery_enabled {
            state.recovery.disable();
            return;
        }
        if !state.recovery.due(now) {
            return;
        }
        drop(state);
        let outcome = manager.recover_last_launch(process.generation, &self.inner.recovery_enabled);
        let Ok(mut state) = self.inner.state.lock() else {
            return;
        };
        if state.revision != revision
            || !state.recovery.matches_generation(process.generation)
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

    fn record_read_error(&self, revision: u64, error: String) {
        if let Ok(mut state) = self.inner.state.lock() {
            if state.revision == revision
                && state.read_error.observe(
                    true,
                    Instant::now(),
                    Duration::from_secs(15),
                    Duration::from_secs(300),
                )
            {
                state.event("warn", "sampling", error);
            }
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
    mut config: HealthConfig,
    workspace_id: String,
    monitor: tauri::State<'_, HealthMonitor>,
) -> Result<HealthStatus, String> {
    let monitor = monitor.inner().clone();
    super::run_blocking(move || {
        config.validate()?;
        let mut state = monitor
            .inner
            .state
            .lock()
            .map_err(|_| "Health monitor is unavailable.".to_string())?;
        if state.workspace_id != workspace_id {
            return Err("The active workspace changed. Reload health settings.".to_string());
        }
        if monitor.inner.stopped.load(Ordering::Acquire) {
            return Err("FXServer Installer is shutting down.".to_string());
        }
        monitor
            .inner
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
        monitor.inner.wake.notify_all();
        monitor.publish_events();
        Ok(status)
    })
    .await
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
    let bytes = path.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
        || path.contains('\0')
    {
        return Err("Choose an existing folder on a local drive for disk monitoring, or set the disk threshold to 0 to disable it.".to_string());
    }
    let path = Path::new(path);
    if !path.is_dir() {
        return Err("The disk monitoring folder does not exist.".to_string());
    }
    free_disk_gb(path)?;
    Ok(path.to_path_buf())
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

#[cfg(not(target_os = "windows"))]
fn free_disk_gb(_: &Path) -> Result<f64, String> {
    Err("Native disk monitoring is only supported on Windows.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitoring_and_recovery_default_to_disabled() {
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
        ] {
            assert!(validate_disk_path(path).is_err());
        }
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
