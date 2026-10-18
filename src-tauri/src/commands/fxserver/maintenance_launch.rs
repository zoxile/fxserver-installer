use super::{
    process_is_running, sanitize_environment, start_fxserver_blocking, FxserverLaunchRequest,
    FxserverLaunchResult, FxserverManager, LaunchIntent, SavedLaunch,
};
use crate::commands::backup_manager::storage::secure_token;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::Ordering,
    time::{Duration, Instant, SystemTime},
};

const TICKET_LIFETIME: Duration = Duration::from_secs(60 * 60);
const CHANGED: &str = "The managed server or launch settings changed. Review maintenance again.";

#[derive(Clone, PartialEq, Eq)]
struct ProcessIdentity {
    pid: u32,
    started_at: SystemTime,
    artifact_path: PathBuf,
}

enum Phase {
    Prepared { process: ProcessIdentity },
    Stopped,
}

// Deliberately neither serializable nor Debug: the saved environment stays native-only.
pub(super) struct MaintenanceLaunch {
    token: String,
    expires_at: Instant,
    generation: u64,
    launch: SavedLaunch,
    phase: Phase,
}

impl MaintenanceLaunch {
    fn check(
        &self,
        intent: &LaunchIntent,
        process: Option<&ProcessIdentity>,
    ) -> Result<(), String> {
        if intent.generation != self.generation {
            return Err(CHANGED.into());
        }
        let matches = match &self.phase {
            Phase::Prepared { process: expected } => {
                intent.expected_running
                    && process == Some(expected)
                    && intent
                        .launch
                        .as_ref()
                        .is_some_and(|launch| exact_launch_matches(launch, &self.launch))
            }
            Phase::Stopped => {
                !intent.expected_running && intent.launch.is_none() && process.is_none()
            }
        };
        if matches {
            Ok(())
        } else {
            Err(CHANGED.into())
        }
    }
}

fn exact_launch_matches(left: &SavedLaunch, right: &SavedLaunch) -> bool {
    left.artifact_path == right.artifact_path
        && left.server_profile == right.server_profile
        && left.environment.len() == right.environment.len()
        && left
            .environment
            .iter()
            .zip(&right.environment)
            .all(|(a, b)| a.key == b.key && a.value == b.value)
}

fn sensitive_key(key: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    // Keep in parity with sensitiveTxHostKeys and publicEnvironment's name filter.
    if matches!(
        key.as_str(),
        "txhost_api_token"
            | "txhost_default_cfxkey"
            | "txhost_default_dbpass"
            | "txhost_default_dbuser"
            | "txhost_default_account"
    ) || [
        "password",
        "passwd",
        "dbpass",
        "secret",
        "token",
        "authorization",
        "cookie",
        "credential",
    ]
    .iter()
    .any(|part| key.contains(part))
    {
        return true;
    }
    [
        ("license", "key"),
        ("api", "key"),
        ("connection", "string"),
        ("private", "key"),
    ]
    .iter()
    .any(|(prefix, suffix)| {
        key.match_indices(prefix).any(|(index, _)| {
            let tail = &key[index + prefix.len()..];
            let mut chars = tail.chars();
            tail.starts_with(suffix)
                || (chars.next().is_some_and(|value| value != '\n')
                    && chars.as_str().starts_with(suffix))
        })
    })
}

pub(super) fn public_scope_matches(saved: &SavedLaunch, request: &FxserverLaunchRequest) -> bool {
    if request
        .environment
        .iter()
        .any(|entry| sensitive_key(&entry.key))
    {
        return false;
    }
    let path = |value: &str| {
        #[cfg(target_os = "windows")]
        {
            value
                .trim()
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_lowercase()
        }
        #[cfg(not(target_os = "windows"))]
        {
            value.trim().trim_end_matches('/').to_string()
        }
    };
    let public_environment = |launch: &SavedLaunch| {
        sanitize_environment(launch.environment.clone()).map(|entries| {
            entries
                .into_iter()
                .filter(|entry| !sensitive_key(&entry.key))
                .map(|entry| (entry.key, entry.value))
                .collect::<BTreeMap<_, _>>()
        })
    };
    let Ok(saved_environment) = public_environment(saved) else {
        return false;
    };
    let Ok(request_environment) = public_environment(&SavedLaunch::from_request(request)) else {
        return false;
    };
    path(&saved.artifact_path) == path(&request.artifact_path)
        && saved.server_profile.as_deref().unwrap_or("").trim()
            == request.server_profile.as_deref().unwrap_or("").trim()
        && saved_environment == request_environment
}

fn ticket<'a>(
    slot: &'a mut Option<MaintenanceLaunch>,
    token: &str,
) -> Result<&'a mut MaintenanceLaunch, String> {
    if slot
        .as_ref()
        .is_some_and(|ticket| Instant::now() >= ticket.expires_at)
    {
        *slot = None;
        return Err("The maintenance launch ticket expired. Review maintenance again.".into());
    }
    slot.as_mut()
        .filter(|ticket| ticket.token == token)
        .ok_or_else(|| "The maintenance launch ticket is invalid or already used.".into())
}

trait ProcessIo {
    fn identity(&self, manager: &FxserverManager) -> Result<Option<ProcessIdentity>, String>;
    fn stop(&self, manager: &FxserverManager) -> Result<(), String>;
    fn start(
        &self,
        manager: &FxserverManager,
        request: FxserverLaunchRequest,
    ) -> Result<FxserverLaunchResult, String>;
}

struct NativeProcessIo;
impl ProcessIo for NativeProcessIo {
    fn identity(&self, manager: &FxserverManager) -> Result<Option<ProcessIdentity>, String> {
        let mut process = manager
            .process
            .lock()
            .map_err(|_| "FXServer process state is unavailable.")?;
        let Some(process) = process.as_mut() else {
            return Ok(None);
        };
        if !process_is_running(process)? {
            return Ok(None);
        }
        Ok(Some(ProcessIdentity {
            pid: process.child.id(),
            started_at: process.started_at,
            artifact_path: process.artifact_path.clone(),
        }))
    }

    fn stop(&self, manager: &FxserverManager) -> Result<(), String> {
        manager.stop_process()
    }

    fn start(
        &self,
        manager: &FxserverManager,
        request: FxserverLaunchRequest,
    ) -> Result<FxserverLaunchResult, String> {
        start_fxserver_blocking(request, manager)
    }
}

pub(super) fn prepare(
    manager: &FxserverManager,
    request: FxserverLaunchRequest,
    expected_pid: u32,
) -> Result<String, String> {
    prepare_with(manager, request, expected_pid, &NativeProcessIo)
}

fn prepare_with(
    manager: &FxserverManager,
    request: FxserverLaunchRequest,
    expected_pid: u32,
    io: &impl ProcessIo,
) -> Result<String, String> {
    let _operation = manager
        .lifecycle
        .try_lock()
        .map_err(|_| "Another FXServer action is in progress.")?;
    let mut slot = manager
        .maintenance_launch
        .lock()
        .map_err(|_| "Maintenance launch state is unavailable.")?;
    if slot
        .as_ref()
        .is_some_and(|ticket| Instant::now() >= ticket.expires_at)
    {
        *slot = None;
    }
    if slot
        .as_ref()
        .is_some_and(|ticket| matches!(ticket.phase, Phase::Stopped))
    {
        return Err(
            "Finish or release the active maintenance launch before preparing another.".into(),
        );
    }
    if manager.shutting_down.load(Ordering::Acquire) {
        return Err(CHANGED.into());
    }
    let process = io
        .identity(manager)?
        .filter(|process| process.pid == expected_pid && expected_pid != 0)
        .ok_or(CHANGED)?;
    let intent = manager
        .launch_intent
        .lock()
        .map_err(|_| "FXServer launch settings are unavailable.")?;
    let launch = intent
        .launch
        .as_ref()
        .filter(|_| intent.expected_running)
        .ok_or("Start the server through this app before planning maintenance.")?;
    if !public_scope_matches(launch, &request) {
        return Err("The public launch scope differs from the running server or contains secret overrides. Review maintenance again.".into());
    }
    let token = secure_token()?;
    *slot = Some(MaintenanceLaunch {
        token: token.clone(),
        expires_at: Instant::now() + TICKET_LIFETIME,
        generation: intent.generation,
        launch: launch.clone(),
        phase: Phase::Prepared { process },
    });
    Ok(token)
}

pub(super) fn validate(manager: &FxserverManager, token: &str) -> Result<(), String> {
    validate_with(manager, token, &NativeProcessIo)
}

fn validate_with(
    manager: &FxserverManager,
    token: &str,
    io: &impl ProcessIo,
) -> Result<(), String> {
    let _operation = manager
        .lifecycle
        .try_lock()
        .map_err(|_| "Another FXServer action is in progress.")?;
    let mut slot = manager
        .maintenance_launch
        .lock()
        .map_err(|_| "Maintenance launch state is unavailable.")?;
    let ticket = ticket(&mut slot, token)?;
    if manager.shutting_down.load(Ordering::Acquire) {
        return Err(CHANGED.into());
    }
    let process = io.identity(manager)?;
    let intent = manager
        .launch_intent
        .lock()
        .map_err(|_| "FXServer launch settings are unavailable.")?;
    ticket.check(&intent, process.as_ref())
}

pub(super) fn stop(manager: &FxserverManager, token: &str) -> Result<(), String> {
    stop_with(manager, token, &NativeProcessIo)
}

fn stop_with(manager: &FxserverManager, token: &str, io: &impl ProcessIo) -> Result<(), String> {
    let _operation = manager
        .lifecycle
        .try_lock()
        .map_err(|_| "Another FXServer action is in progress.")?;
    let mut slot = manager
        .maintenance_launch
        .lock()
        .map_err(|_| "Maintenance launch state is unavailable.")?;
    let ticket = ticket(&mut slot, token)?;
    if !matches!(ticket.phase, Phase::Prepared { .. }) {
        return Err("The maintenance stop was already used.".into());
    }
    if manager.shutting_down.load(Ordering::Acquire) {
        return Err(CHANGED.into());
    }
    let process = io.identity(manager)?;
    let mut intent = manager
        .launch_intent
        .lock()
        .map_err(|_| "FXServer launch settings are unavailable.")?;
    ticket.check(&intent, process.as_ref())?;
    // Only the ticket retains restart settings after stop; normal lifecycle actions
    // advance the generation and invalidate this authorization.
    intent.expected_running = false;
    intent.launch = None;
    intent.generation = intent.generation.wrapping_add(1);
    ticket.generation = intent.generation;
    ticket.phase = Phase::Stopped;
    drop(intent);
    if let Err(error) = io.stop(manager) {
        *slot = None;
        return Err(error);
    }
    Ok(())
}

pub(super) fn start(
    manager: &FxserverManager,
    token: &str,
) -> Result<FxserverLaunchResult, String> {
    start_with(manager, token, &NativeProcessIo)
}

fn start_with(
    manager: &FxserverManager,
    token: &str,
    io: &impl ProcessIo,
) -> Result<FxserverLaunchResult, String> {
    let _operation = manager
        .lifecycle
        .try_lock()
        .map_err(|_| "Another FXServer action is in progress.")?;
    let mut slot = manager
        .maintenance_launch
        .lock()
        .map_err(|_| "Maintenance launch state is unavailable.")?;
    let ticket = ticket(&mut slot, token)?;
    if !matches!(ticket.phase, Phase::Stopped) {
        return Err("Stop the reviewed server for maintenance before starting it.".into());
    }
    if manager.shutting_down.load(Ordering::Acquire) {
        return Err(CHANGED.into());
    }
    let process = io.identity(manager)?;
    let intent = manager
        .launch_intent
        .lock()
        .map_err(|_| "FXServer launch settings are unavailable.")?;
    ticket.check(&intent, process.as_ref())?;
    drop(intent);
    let ticket = slot
        .take()
        .ok_or("The maintenance launch ticket was already used.")?;
    // Consume before starting, including failed starts. The existing generation check
    // also catches manual stop/shutdown requests that arrive during process launch.
    let result = io.start(manager, ticket.launch.clone().into_request())?;
    if !manager.remember_launch(ticket.launch, ticket.generation)? {
        io.stop(manager)?;
        return Err(
            "FXServer maintenance start was cancelled by a stop or shutdown request.".into(),
        );
    }
    Ok(result)
}

pub(super) fn discard(manager: &FxserverManager, token: &str) -> Result<(), String> {
    let mut slot = manager
        .maintenance_launch
        .lock()
        .map_err(|_| "Maintenance launch state is unavailable.")?;
    if slot.as_ref().is_some_and(|ticket| ticket.token == token) {
        *slot = None;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::fxserver::FxserverEnvironmentVariable;
    use std::cell::{Cell, RefCell};

    struct FakeIo {
        process: RefCell<Option<ProcessIdentity>>,
        stopped: Cell<usize>,
        starts: RefCell<Vec<SavedLaunch>>,
        fail_stop: Cell<bool>,
        fail_start: Cell<bool>,
        disarm_during_start: Cell<bool>,
    }

    impl ProcessIo for FakeIo {
        fn identity(&self, _: &FxserverManager) -> Result<Option<ProcessIdentity>, String> {
            Ok(self.process.borrow().clone())
        }

        fn stop(&self, manager: &FxserverManager) -> Result<(), String> {
            assert!(manager.lifecycle.try_lock().is_err());
            assert!(!manager.launch_intent.lock().unwrap().expected_running);
            assert!(manager.launch_intent.lock().unwrap().launch.is_none());
            self.stopped.set(self.stopped.get() + 1);
            if self.fail_stop.get() {
                return Err("fixture stop failed".into());
            }
            *self.process.borrow_mut() = None;
            Ok(())
        }

        fn start(
            &self,
            manager: &FxserverManager,
            request: FxserverLaunchRequest,
        ) -> Result<FxserverLaunchResult, String> {
            assert!(manager.lifecycle.try_lock().is_err());
            assert!(self.process.borrow().is_none());
            self.starts
                .borrow_mut()
                .push(SavedLaunch::from_request(&request));
            if self.fail_start.get() {
                return Err("fixture start failed".into());
            }
            if self.disarm_during_start.get() {
                manager.disarm_recovery();
            }
            *self.process.borrow_mut() = Some(ProcessIdentity {
                pid: 43,
                started_at: SystemTime::UNIX_EPOCH + Duration::from_secs(2),
                artifact_path: PathBuf::from(&request.artifact_path),
            });
            Ok(FxserverLaunchResult {
                pid: 43,
                artifact_path: request.artifact_path,
                started_at: "2".into(),
            })
        }
    }

    fn variable(key: &str, value: &str) -> FxserverEnvironmentVariable {
        FxserverEnvironmentVariable {
            key: key.into(),
            value: value.into(),
        }
    }

    fn public_request() -> FxserverLaunchRequest {
        FxserverLaunchRequest {
            artifact_path: "C:/fixture/artifact".into(),
            server_profile: Some("production".into()),
            environment: vec![
                variable("TXHOST_DATA_PATH", "C:/fixture/txData"),
                variable("TXHOST_RCON_PORT", "30120"),
            ],
        }
    }

    fn fixture() -> (FxserverManager, FakeIo) {
        let manager = FxserverManager::default();
        let mut launch = SavedLaunch::from_request(&public_request());
        launch.environment.extend([
            variable("TXHOST_DEFAULT_DBPASS", "  native db password  "),
            variable("TXHOST_API_TOKEN", "native-api-token"),
            variable("TXHOST_DEFAULT_CFXKEY", "native-cfx-key"),
            variable("TXHOST_DEFAULT_ACCOUNT", "native-account"),
            variable("TXHOST_DEFAULT_DBUSER", "native-dbuser"),
        ]);
        manager.remember_launch(launch, 0).unwrap();
        let io = FakeIo {
            process: RefCell::new(Some(ProcessIdentity {
                pid: 42,
                started_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1),
                artifact_path: PathBuf::from("C:/fixture/artifact"),
            })),
            stopped: Cell::new(0),
            starts: RefCell::new(Vec::new()),
            fail_stop: Cell::new(false),
            fail_start: Cell::new(false),
            disarm_during_start: Cell::new(false),
        };
        (manager, io)
    }

    fn prepare_fixture(manager: &FxserverManager, io: &FakeIo) -> String {
        prepare_with(manager, public_request(), 42, io).unwrap()
    }

    #[test]
    fn maintenance_sensitive_fields_match_public_environment_filter() {
        for key in [
            "TXHOST_API_TOKEN",
            "TXHOST_DEFAULT_CFXKEY",
            "TXHOST_DEFAULT_DBPASS",
            "TXHOST_DEFAULT_DBUSER",
            "TXHOST_DEFAULT_ACCOUNT",
            "TXHOST_PASSWORD",
            "TXHOST_PASSWD",
            "TXHOST_DBPASS",
            "TXHOST_SECRET",
            "TXHOST_TOKEN",
            "TXHOST_AUTHORIZATION",
            "TXHOST_COOKIE",
            "TXHOST_CREDENTIAL",
            "TXHOST_LICENSEKEY",
            "TXHOST_LICENSE_KEY",
            "TXHOST_LICENSE1KEY",
            "TXHOST_APIKEY",
            "TXHOST_API_KEY",
            "TXHOST_CONNECTIONSTRING",
            "TXHOST_CONNECTION_STRING",
            "TXHOST_PRIVATEKEY",
            "TXHOST_PRIVATE_KEY",
            "TXHOST_LICENSE_OTHER_LICENSE_KEY",
        ] {
            assert!(sensitive_key(key), "{key}");
            assert!(sensitive_key(&key.to_lowercase()), "{key}");
            let (manager, io) = fixture();
            let mut request = public_request();
            request.environment.push(variable(key, ""));
            assert!(
                prepare_with(&manager, request, 42, &io).is_err(),
                "even empty overrides must be rejected: {key}"
            );
            assert!(manager.maintenance_launch.lock().unwrap().is_none());
        }
        for key in [
            "TXHOST_DATA_PATH",
            "TXHOST_RCON_HOST",
            "TXHOST_RCON_PORT",
            "TXHOST_API_URL",
            "TXHOST_PRIVATE__KEY",
        ] {
            assert!(!sensitive_key(key), "{key}");
        }
    }

    #[test]
    fn maintenance_public_scope_is_complete_and_secrets_remain_native() {
        let (manager, io) = fixture();
        let saved = manager
            .launch_intent
            .lock()
            .unwrap()
            .launch
            .clone()
            .unwrap();
        assert!(public_scope_matches(&saved, &public_request()));
        for mutation in 0..4 {
            let mut request = public_request();
            match mutation {
                0 => {
                    request.environment.pop();
                }
                1 => request.environment[0].value = "C:/different".into(),
                2 => request
                    .environment
                    .push(variable("TXHOST_PUBLIC_EXTRA", "different")),
                _ => request
                    .environment
                    .push(variable("INVALID_KEY", "different")),
            }
            assert!(prepare_with(&manager, request, 42, &io).is_err());
        }
        let token = prepare_fixture(&manager, &io);
        assert_eq!(token.len(), 32);
        assert!(token.bytes().all(|value| value.is_ascii_hexdigit()));
        let slot = manager.maintenance_launch.lock().unwrap();
        let ticket = slot.as_ref().unwrap();
        assert!(exact_launch_matches(&ticket.launch, &saved));
        assert!(ticket.expires_at.duration_since(Instant::now()) <= TICKET_LIFETIME);
        assert!(ticket.expires_at.duration_since(Instant::now()) > Duration::from_secs(3590));
    }

    #[test]
    fn maintenance_wrong_tokens_and_wrong_pid_never_change_intent() {
        let (manager, io) = fixture();
        assert!(prepare_with(&manager, public_request(), 99, &io).is_err());
        let token = prepare_fixture(&manager, &io);
        let generation = manager.launch_generation().unwrap();
        assert!(validate_with(&manager, "wrong", &io).is_err());
        assert!(stop_with(&manager, "wrong", &io).is_err());
        assert!(start_with(&manager, "wrong", &io).is_err());
        discard(&manager, "wrong").unwrap();
        validate_with(&manager, &token, &io).unwrap();
        assert_eq!(manager.launch_generation().unwrap(), generation);
        assert!(manager.launch_intent.lock().unwrap().expected_running);
        assert_eq!(io.stopped.get(), 0);
        assert!(io.starts.borrow().is_empty());
    }

    #[test]
    fn maintenance_stale_process_generation_and_private_launch_are_rejected() {
        for mutation in 0..7 {
            let (manager, io) = fixture();
            let token = prepare_fixture(&manager, &io);
            match mutation {
                0 => io.process.borrow_mut().as_mut().unwrap().pid += 1,
                1 => io.process.borrow_mut().as_mut().unwrap().started_at += Duration::from_secs(1),
                2 => io.process.borrow_mut().as_mut().unwrap().artifact_path = "other".into(),
                3 => manager.launch_intent.lock().unwrap().generation += 1,
                4 => {
                    manager
                        .launch_intent
                        .lock()
                        .unwrap()
                        .launch
                        .as_mut()
                        .unwrap()
                        .environment[2]
                        .value = "changed-native-password".into()
                }
                5 => manager.disarm_recovery(),
                _ => *io.process.borrow_mut() = None,
            }
            assert!(validate_with(&manager, &token, &io).is_err());
            assert!(stop_with(&manager, &token, &io).is_err());
            assert_eq!(io.stopped.get(), 0);
        }
    }

    #[test]
    fn maintenance_restart_consumes_ticket_and_preserves_exact_saved_secrets() {
        let (manager, io) = fixture();
        let saved = manager
            .launch_intent
            .lock()
            .unwrap()
            .launch
            .clone()
            .unwrap();
        let token = prepare_fixture(&manager, &io);
        let generation = manager.launch_generation().unwrap();
        assert!(start_with(&manager, &token, &io).is_err());
        stop_with(&manager, &token, &io).unwrap();
        assert_eq!(manager.launch_generation().unwrap(), generation + 1);
        assert!(!manager.launch_intent.lock().unwrap().expected_running);
        assert!(manager.launch_intent.lock().unwrap().launch.is_none());
        assert!(exact_launch_matches(
            &manager
                .maintenance_launch
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .launch,
            &saved
        ));
        assert_eq!(
            manager
                .maintenance_launch
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .generation,
            generation + 1
        );
        validate_with(&manager, &token, &io).unwrap();
        assert!(stop_with(&manager, &token, &io).is_err());
        assert_eq!(start_with(&manager, &token, &io).unwrap().pid, 43);
        assert!(exact_launch_matches(&io.starts.borrow()[0], &saved));
        assert!(exact_launch_matches(
            manager
                .launch_intent
                .lock()
                .unwrap()
                .launch
                .as_ref()
                .unwrap(),
            &saved
        ));
        assert!(manager.launch_intent.lock().unwrap().expected_running);
        assert_eq!(manager.launch_generation().unwrap(), generation + 2);
        assert!(manager.maintenance_launch.lock().unwrap().is_none());
        assert!(validate_with(&manager, &token, &io).is_err());
        assert!(start_with(&manager, &token, &io).is_err());
        assert_eq!(io.starts.borrow().len(), 1);
        assert_eq!(io.stopped.get(), 1);
    }

    #[test]
    fn maintenance_intervening_actions_prevent_restart() {
        for mutation in 0..5 {
            let (manager, io) = fixture();
            let token = prepare_fixture(&manager, &io);
            let old_process = io.process.borrow().clone();
            let old_launch = manager.launch_intent.lock().unwrap().launch.clone();
            stop_with(&manager, &token, &io).unwrap();
            match mutation {
                0 => manager.disarm_recovery(),
                1 => manager.launch_intent.lock().unwrap().launch = old_launch,
                2 => *io.process.borrow_mut() = old_process,
                3 => manager.launch_intent.lock().unwrap().expected_running = true,
                _ => manager.begin_shutdown(),
            }
            assert!(start_with(&manager, &token, &io).is_err());
            assert!(io.starts.borrow().is_empty());
        }
    }

    #[test]
    fn maintenance_stop_or_start_failure_consumes_authorization_safely() {
        let (manager, io) = fixture();
        let token = prepare_fixture(&manager, &io);
        io.fail_stop.set(true);
        assert!(stop_with(&manager, &token, &io).is_err());
        assert!(manager.maintenance_launch.lock().unwrap().is_none());
        assert!(!manager.launch_intent.lock().unwrap().expected_running);
        assert!(manager.launch_intent.lock().unwrap().launch.is_none());
        assert!(start_with(&manager, &token, &io).is_err());

        let (manager, io) = fixture();
        let token = prepare_fixture(&manager, &io);
        stop_with(&manager, &token, &io).unwrap();
        io.fail_start.set(true);
        assert!(start_with(&manager, &token, &io).is_err());
        assert!(start_with(&manager, &token, &io).is_err());
        assert_eq!(io.starts.borrow().len(), 1);
        assert!(!manager.launch_intent.lock().unwrap().expected_running);
        assert!(manager.launch_intent.lock().unwrap().launch.is_none());
        assert!(manager.maintenance_launch.lock().unwrap().is_none());
    }

    #[test]
    fn maintenance_manual_stop_during_launch_cleans_up_new_process() {
        let (manager, io) = fixture();
        let token = prepare_fixture(&manager, &io);
        stop_with(&manager, &token, &io).unwrap();
        io.disarm_during_start.set(true);
        assert!(start_with(&manager, &token, &io).is_err());
        assert_eq!(io.stopped.get(), 2);
        assert!(io.process.borrow().is_none());
        assert!(!manager.launch_intent.lock().unwrap().expected_running);
        assert!(manager.launch_intent.lock().unwrap().launch.is_none());
        assert!(manager.maintenance_launch.lock().unwrap().is_none());
    }

    #[test]
    fn maintenance_release_is_idempotent_and_cannot_discard_a_new_ticket() {
        let (manager, io) = fixture();
        let generation = manager.launch_generation().unwrap();
        let first = prepare_fixture(&manager, &io);
        let second = prepare_fixture(&manager, &io);
        assert_ne!(first, second);
        discard(&manager, &first).unwrap();
        discard(&manager, &first).unwrap();
        validate_with(&manager.clone(), &second, &io).unwrap();
        assert_eq!(manager.launch_generation().unwrap(), generation);
        assert!(manager.launch_intent.lock().unwrap().expected_running);
        stop_with(&manager, &second, &io).unwrap();
        assert!(prepare_with(&manager, public_request(), 42, &io)
            .unwrap_err()
            .contains("active maintenance"));
        validate_with(&manager, &second, &io).unwrap();
        discard(&manager, &second).unwrap();
        assert!(!manager.launch_intent.lock().unwrap().expected_running);
        assert!(manager.launch_intent.lock().unwrap().launch.is_none());
        assert!(manager.maintenance_launch.lock().unwrap().is_none());
        assert!(start_with(&manager, &second, &io).is_err());
        assert!(io.starts.borrow().is_empty());
    }

    #[test]
    fn maintenance_failed_replacement_preserves_review_and_discard_only_matches_token() {
        let (manager, io) = fixture();
        let token = prepare_fixture(&manager, &io);
        assert!(prepare_with(&manager, public_request(), 99, &io).is_err());
        validate_with(&manager, &token, &io).unwrap();
        manager
            .maintenance_launch
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .expires_at = Instant::now() - Duration::from_secs(1);
        discard(&manager, "old-unrelated-token").unwrap();
        assert!(manager.maintenance_launch.lock().unwrap().is_some());
        discard(&manager, &token).unwrap();
        assert!(manager.maintenance_launch.lock().unwrap().is_none());
        assert!(manager.launch_intent.lock().unwrap().expected_running);
    }

    #[test]
    fn maintenance_expiry_never_extends_on_validation_and_releases_secrets() {
        for stopped in [false, true] {
            let (manager, io) = fixture();
            let token = prepare_fixture(&manager, &io);
            if stopped {
                stop_with(&manager, &token, &io).unwrap();
            }
            let deadline = manager
                .maintenance_launch
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .expires_at;
            validate_with(&manager, &token, &io).unwrap();
            assert_eq!(
                manager
                    .maintenance_launch
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .expires_at,
                deadline
            );
            manager
                .maintenance_launch
                .lock()
                .unwrap()
                .as_mut()
                .unwrap()
                .expires_at = Instant::now() - Duration::from_secs(1);
            assert!(validate_with(&manager, &token, &io)
                .unwrap_err()
                .contains("expired"));
            assert!(manager.maintenance_launch.lock().unwrap().is_none());
            assert_eq!(
                manager.launch_intent.lock().unwrap().launch.is_none(),
                stopped
            );
            assert!(start_with(&manager, &token, &io).is_err());
            assert!(io.starts.borrow().is_empty());
        }
    }

    #[test]
    fn maintenance_commands_share_lifecycle_gate_without_side_effects_when_busy() {
        let (manager, io) = fixture();
        let token = prepare_fixture(&manager, &io);
        let generation = manager.launch_generation().unwrap();
        let guard = manager.lifecycle.lock().unwrap();
        assert!(prepare_with(&manager, public_request(), 42, &io).is_err());
        assert!(validate_with(&manager, &token, &io).is_err());
        assert!(stop_with(&manager, &token, &io).is_err());
        assert!(start_with(&manager, &token, &io).is_err());
        assert_eq!(manager.launch_generation().unwrap(), generation);
        assert_eq!(io.stopped.get(), 0);
        assert!(io.starts.borrow().is_empty());
        drop(guard);
        validate_with(&manager, &token, &io).unwrap();
    }
}
