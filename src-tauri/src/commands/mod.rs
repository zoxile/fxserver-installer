pub mod artifact;
pub mod backup_manager;
pub mod config_history;
pub mod diagnostics;
pub mod fxserver;
pub mod health;
pub mod jooat;
pub mod logs;
pub mod mariadb;
pub mod resource_updates;
pub mod system;

use std::sync::{Condvar, Mutex};

struct BackgroundWork {
    closing: bool,
    active: usize,
}

static BACKGROUND_WORK: Mutex<BackgroundWork> = Mutex::new(BackgroundWork {
    closing: false,
    active: 0,
});
static BACKGROUND_IDLE: Condvar = Condvar::new();

struct WorkPermit;

impl Drop for WorkPermit {
    fn drop(&mut self) {
        let mut work = BACKGROUND_WORK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        work.active = work.active.saturating_sub(1);
        BACKGROUND_IDLE.notify_all();
    }
}

pub(crate) fn begin_shutdown() {
    BACKGROUND_WORK
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .closing = true;
}

pub(crate) fn require_other_work_idle() -> Result<(), String> {
    let work = BACKGROUND_WORK
        .lock()
        .map_err(|_| "Background work lock is unavailable.".to_string())?;
    // The workspace-switch command itself owns one permit.
    if work.active > 1 {
        return Err("Background operations are still running. Try switching workspaces again when they finish.".into());
    }
    Ok(())
}

pub(crate) fn wait_for_background_work() {
    let mut work = BACKGROUND_WORK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    while work.active > 0 {
        work = BACKGROUND_IDLE
            .wait(work)
            .unwrap_or_else(|error| error.into_inner());
    }
}

pub(crate) async fn run_blocking<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    {
        let mut work = BACKGROUND_WORK
            .lock()
            .map_err(|_| "Background work lock is unavailable.".to_string())?;
        if work.closing {
            return Err("The application is shutting down.".into());
        }
        work.active += 1;
    }
    let permit = WorkPermit;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        task()
    })
    .await
    .map_err(|error| format!("Background task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, thread, time::Duration};

    #[tokio::test(flavor = "current_thread")]
    async fn blocking_work_leaves_the_async_executor_responsive() {
        let caller = thread::current().id();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = mpsc::channel();
        let task = tokio::spawn(run_blocking(move || {
            started.send(thread::current().id()).unwrap();
            wait.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(42)
        }));
        let worker = tokio::time::timeout(Duration::from_millis(500), ready)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(caller, worker);
        assert!(!task.is_finished());
        release.send(()).unwrap();
        assert_eq!(task.await.unwrap().unwrap(), 42);
        assert_eq!(
            run_blocking(|| Err::<(), _>("failure".to_string()))
                .await
                .unwrap_err(),
            "failure"
        );
    }
}
