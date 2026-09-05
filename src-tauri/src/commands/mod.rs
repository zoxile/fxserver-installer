pub mod artifact;
pub mod fxserver;
pub mod jooat;
pub mod logs;
pub mod mariadb;
pub mod system;

pub(crate) async fn run_blocking<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(task)
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
