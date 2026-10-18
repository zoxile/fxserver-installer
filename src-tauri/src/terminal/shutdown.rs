use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::{runtime::Handle, task::JoinHandle};

pub(super) struct Shutdown {
    requested: Arc<AtomicBool>,
    listener: JoinHandle<()>,
}

impl Shutdown {
    pub(super) fn listen(runtime: &Handle) -> Result<Self, String> {
        let _context = runtime.enter();
        let requested = Arc::new(AtomicBool::new(false));
        let flag = requested.clone();
        #[cfg(unix)]
        let listener = {
            use tokio::signal::unix::{signal, SignalKind};
            let mut interrupt = signal(SignalKind::interrupt()).map_err(|e| e.to_string())?;
            let mut terminate = signal(SignalKind::terminate()).map_err(|e| e.to_string())?;
            let mut hangup = signal(SignalKind::hangup()).map_err(|e| e.to_string())?;
            runtime.spawn(async move {
                tokio::select! {
                    _ = interrupt.recv() => {},
                    _ = terminate.recv() => {},
                    _ = hangup.recv() => {},
                }
                flag.store(true, Ordering::Release);
            })
        };
        #[cfg(windows)]
        let listener = {
            use tokio::signal::windows::{ctrl_break, ctrl_c};
            let mut interrupt = ctrl_c().map_err(|e| e.to_string())?;
            let mut terminate = ctrl_break().map_err(|e| e.to_string())?;
            runtime.spawn(async move {
                tokio::select! {
                    _ = interrupt.recv() => {},
                    _ = terminate.recv() => {},
                }
                flag.store(true, Ordering::Release);
            })
        };
        Ok(Self {
            requested,
            listener,
        })
    }

    pub(super) fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

impl Drop for Shutdown {
    fn drop(&mut self) {
        self.listener.abort();
    }
}
