mod download;

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const EVENT: &str = "app-update-progress";
type PrepareInstall = fn(&AppHandle) -> Result<(), String>;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdatePhase {
    Disabled,
    Idle,
    Checking,
    Available,
    Downloading,
    Ready,
    Stopping,
    Installing,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateStatus {
    pub phase: UpdatePhase,
    pub current_version: String,
    pub available_version: Option<String>,
    pub release_url: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub message: Option<String>,
    pub supported: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelPolicy {
    channel: String,
}

fn channel() -> Result<String, String> {
    serde_json::from_str::<ChannelPolicy>(include_str!("../../../update-channel.json"))
        .map(|policy| policy.channel)
        .map_err(|_| "The updater release channel is invalid.".into())
}

fn disabled_reason(channel: &str, os: &str, arch: &str, debug: bool) -> Option<&'static str> {
    if channel == "development" || debug {
        Some("In-app updates are disabled in development builds. Install a tested development build manually.")
    } else if !matches!(channel, "beta" | "stable") {
        Some("This build does not have a supported update channel.")
    } else if os != "windows" || arch != "x86_64" {
        Some("Signed in-app updates are currently available only for released Windows x64 builds. Update this platform manually.")
    } else {
        None
    }
}

struct UpdateState {
    status: AppUpdateStatus,
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct AppUpdateManager {
    state: Arc<Mutex<UpdateState>>,
    operation: Arc<Semaphore>,
    prepare_install: PrepareInstall,
}

impl AppUpdateManager {
    /// The hook must exclusively quiesce app work and gracefully stop FXServer, without exiting.
    pub fn new(prepare_install: PrepareInstall) -> Self {
        let channel = channel().unwrap_or_default();
        let reason = disabled_reason(
            &channel,
            std::env::consts::OS,
            std::env::consts::ARCH,
            cfg!(debug_assertions),
        );
        Self {
            state: Arc::new(Mutex::new(UpdateState {
                status: AppUpdateStatus {
                    phase: if reason.is_some() {
                        UpdatePhase::Disabled
                    } else {
                        UpdatePhase::Idle
                    },
                    current_version: env!("CARGO_PKG_VERSION").into(),
                    available_version: None,
                    release_url: None,
                    downloaded_bytes: 0,
                    total_bytes: None,
                    message: reason.map(str::to_string),
                    supported: reason.is_none(),
                },
                update: None,
                bytes: None,
            })),
            operation: Arc::new(Semaphore::new(1)),
            prepare_install,
        }
    }

    fn snapshot(&self) -> AppUpdateStatus {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .clone()
    }

    fn change(&self, app: &AppHandle, change: impl FnOnce(&mut UpdateState)) -> AppUpdateStatus {
        let status = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            change(&mut state);
            state.status.clone()
        };
        let _ = app.emit(EVENT, &status);
        status
    }

    fn phase(&self, app: &AppHandle, phase: UpdatePhase) {
        self.change(app, |state| {
            state.status.phase = phase;
            state.status.message = None;
        });
    }

    fn fail(&self, app: &AppHandle, error: String) -> String {
        log::warn!("Application update: {error}");
        self.change(app, |state| {
            state.status.phase = UpdatePhase::Error;
            state.status.message = Some(error.clone());
        });
        error
    }

    fn require_supported(&self) -> Result<(), String> {
        let status = self.snapshot();
        if status.supported {
            Ok(())
        } else {
            Err(status
                .message
                .unwrap_or_else(|| "In-app updates are disabled.".into()))
        }
    }

    fn begin_operation(&self) -> Result<OwnedSemaphorePermit, String> {
        let permit = self
            .operation
            .clone()
            .try_acquire_owned()
            .map_err(|_| "An update operation is already running.")?;
        self.require_supported()?;
        Ok(permit)
    }

    fn fail_install(&self, app: &AppHandle, error: String) -> String {
        let error = if super::background_work_closing() {
            self.change(app, |state| state.status.supported = false);
            format!("{error} The application has stopped its background work. Restart it before trying again.")
        } else {
            error
        };
        self.fail(app, error)
    }
}

#[tauri::command]
pub fn get_app_update_status(state: State<'_, AppUpdateManager>) -> AppUpdateStatus {
    state.snapshot()
}

#[tauri::command]
pub async fn check_app_update(
    app: AppHandle,
    state: State<'_, AppUpdateManager>,
    force: bool,
) -> Result<AppUpdateStatus, String> {
    let _permit = state.begin_operation()?;
    state.change(&app, |state| {
        state.update = None;
        state.bytes = None;
        state.status.phase = UpdatePhase::Checking;
        state.status.available_version = None;
        state.status.release_url = None;
        state.status.downloaded_bytes = 0;
        state.status.total_bytes = None;
        state.status.message = None;
    });
    let result = async {
        let release = super::app_release::fetch_latest_app_release(app.clone(), force).await?;
        let current = state.snapshot().current_version;
        if !download::newer_release(&release, &current, &channel()?)? {
            state.phase(&app, UpdatePhase::Idle);
            return Ok(state.snapshot());
        }
        let metadata = download::manifest(&release).await?;
        let update = app
            .updater_builder()
            .endpoints(vec![download::manifest_url(&release)?])
            .map_err(|error| format!("Invalid update endpoint: {error}"))?
            .target(download::TARGET)
            .version_comparator(|current, remote| remote.version > current)
            .clear_installer_args()
            .no_proxy()
            .build()
            .map_err(|error| format!("Cannot initialize the signed updater: {error}"))?
            .restore_update(metadata)
            .map_err(|error| format!("Invalid signed update metadata: {error}"))?
            .ok_or("The selected release is not newer than this app.")?;
        Ok(state.change(&app, |state| {
            state.status.phase = UpdatePhase::Available;
            state.status.available_version = Some(release.version);
            state.status.release_url = Some(release.html_url);
            state.update = Some(update);
        }))
    }
    .await;
    result.map_err(|error| state.fail(&app, error))
}

#[tauri::command]
pub async fn download_app_update(
    app: AppHandle,
    state: State<'_, AppUpdateManager>,
) -> Result<AppUpdateStatus, String> {
    let _permit = state.begin_operation()?;
    let update = state
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .update
        .clone()
        .ok_or("Check for an update before downloading.")?;
    state.change(&app, |state| {
        state.bytes = None;
        state.status.phase = UpdatePhase::Downloading;
        state.status.downloaded_bytes = 0;
        state.status.total_bytes = None;
        state.status.message = None;
    });
    let result = async {
        let bytes = download::installer(&update.download_url, |downloaded, total| {
            state.change(&app, |state| {
                state.status.downloaded_bytes = downloaded;
                state.status.total_bytes = total;
            });
        })
        .await?;
        let bytes = tokio::task::spawn_blocking(move || {
            update
                .verify(&bytes)
                .map_err(|error| format!("Update signature verification failed: {error}"))?;
            Ok::<_, String>(bytes)
        })
        .await
        .map_err(|_| "The update verification worker failed.")??;
        log::info!("Application update downloaded and signature verified.");
        Ok(state.change(&app, |state| {
            state.status.phase = UpdatePhase::Ready;
            state.status.total_bytes = Some(bytes.len() as u64);
            state.status.downloaded_bytes = bytes.len() as u64;
            state.bytes = Some(bytes);
        }))
    }
    .await;
    result.map_err(|error| state.fail(&app, error))
}

#[tauri::command]
pub async fn install_app_update(
    app: AppHandle,
    state: State<'_, AppUpdateManager>,
) -> Result<AppUpdateStatus, String> {
    let permit = state.begin_operation()?;
    let (update, bytes) = {
        let mut state = state.state.lock().unwrap_or_else(|e| e.into_inner());
        let update = state
            .update
            .clone()
            .ok_or("Check for an update before installing.")?;
        let bytes = state
            .bytes
            .take()
            .ok_or("Download and verify the update before installing.")?;
        (update, bytes)
    };
    let manager = state.inner().clone();
    let worker_app = app.clone();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let app = worker_app;
        let result = (|| {
            update
                .verify(&bytes)
                .map_err(|error| format!("Update signature verification failed: {error}"))?;
            manager.phase(&app, UpdatePhase::Stopping);
            (manager.prepare_install)(&app)?;
            let server = app.try_state::<super::fxserver::FxserverManager>().ok_or(
                "Cannot verify that the managed server stopped. The update was not installed.",
            )?;
            server.with_stopped_server(|| Ok(())).map_err(|error| {
                format!("FXServer must be stopped before installing the update: {error}")
            })?;
            manager.phase(&app, UpdatePhase::Installing);
            log::info!("Starting the verified application update installer.");
            update
                .install(&bytes)
                .map_err(|error| format!("The update installer could not start: {error}."))?;
            Ok(manager.snapshot())
        })();
        if result.is_err() {
            manager
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .bytes = Some(bytes);
        }
        result.map_err(|error| manager.fail_install(&app, error))
    })
    .await
    .map_err(|_| state.fail_install(&app, "The update installation worker failed.".into()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_channel_never_installs_a_main_release_on_any_os() {
        for os in ["windows", "linux", "macos"] {
            assert!(disabled_reason("development", os, "x86_64", false).is_some());
        }
        for channel in ["beta", "stable"] {
            assert!(disabled_reason(channel, "windows", "x86_64", false).is_none());
            assert!(disabled_reason(channel, "windows", "x86_64", true).is_some());
            assert!(disabled_reason(channel, "linux", "x86_64", false).is_some());
            assert!(disabled_reason(channel, "windows", "aarch64", false).is_some());
        }
        assert!(disabled_reason("", "windows", "x86_64", false).is_some());
    }

    #[test]
    fn updater_operations_cannot_overlap_and_release_their_slot_on_failure() {
        let manager = AppUpdateManager::new(|_| Err("not integrated".into()));
        manager.state.lock().unwrap().status.supported = true;
        let permit = manager.begin_operation().unwrap();
        assert!(manager.begin_operation().is_err());
        assert!(manager.clone().begin_operation().is_err());
        drop(permit);
        assert!(manager.begin_operation().is_ok());
        manager.state.lock().unwrap().status.supported = false;
        assert!(manager.begin_operation().is_err());
    }

    #[test]
    fn frontend_status_does_not_expose_installer_inputs() {
        let manager = AppUpdateManager::new(|_| Err("not integrated".into()));
        let value = serde_json::to_value(manager.snapshot()).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 8);
        for key in ["signature", "url", "path", "bytes", "pubkey"] {
            assert!(value.get(key).is_none());
        }
    }
}
