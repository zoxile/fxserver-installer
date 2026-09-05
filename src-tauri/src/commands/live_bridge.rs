use std::{
    io::Read,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

#[path = "live_bridge/install.rs"]
mod install;

pub const RESOURCE_NAME: &str = "fxserver_installer_bridge";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTarget {
    pub workspace_id: String,
    pub tx_data_path: String,
    pub profile: String,
    pub port: u16,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStatus {
    workspace_id: String,
    enabled: bool,
    connected: bool,
    received_at: Option<u64>,
    error: Option<String>,
    snapshot: Option<BridgeSnapshot>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSnapshot {
    protocol: u32,
    version: String,
    instance_id: String,
    timestamp: u64,
    uptime_seconds: u64,
    scheduler_delay_ms: f64,
    hostname: String,
    game_build: String,
    onesync: String,
    max_players: u32,
    player_count: u32,
    resource_count: u32,
    resources: Vec<BridgeResource>,
    players: Vec<BridgePlayer>,
    events: Vec<BridgeEvent>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct BridgeResource {
    name: String,
    state: String,
    version: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct BridgePlayer {
    id: String,
    name: String,
    ping: i32,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct BridgeEvent {
    id: u64,
    timestamp: u64,
    kind: String,
    resource: String,
}

struct Connection {
    target: BridgeTarget,
    token: String,
}

#[derive(Default)]
struct BridgeState {
    connection: Option<Connection>,
    revision: u64,
    status: BridgeStatus,
}

#[derive(Clone, Default)]
pub struct LiveBridge {
    state: Arc<Mutex<BridgeState>>,
    transport: Arc<Mutex<()>>,
    stopped: Arc<AtomicBool>,
}

impl BridgeState {
    fn reset(&mut self, workspace_id: String, enabled: bool) -> u64 {
        self.connection = None;
        self.revision = self.revision.wrapping_add(1);
        self.status = BridgeStatus {
            workspace_id,
            enabled,
            ..BridgeStatus::default()
        };
        self.revision
    }

    fn connect(
        &mut self,
        revision: u64,
        target: BridgeTarget,
        token: Result<String, String>,
    ) -> Result<(), String> {
        self.check_revision(revision)?;
        match token {
            Ok(token) => self.connection = Some(Connection { target, token }),
            Err(error) => {
                self.status.error = Some(error.clone());
                return Err(error);
            }
        }
        Ok(())
    }

    fn check_revision(&self, revision: u64) -> Result<(), String> {
        if self.revision != revision {
            return Err("Bridge configuration was superseded.".into());
        }
        Ok(())
    }

    fn action_connection(
        &self,
        revision: u64,
        workspace_id: &str,
    ) -> Result<(u16, String), String> {
        self.check_revision(revision)?;
        if self.status.workspace_id != workspace_id || !self.status.connected {
            return Err("Connect the live bridge for this workspace first.".into());
        }
        let connection = self
            .connection
            .as_ref()
            .ok_or("Live bridge is disconnected.")?;
        Ok((connection.target.port, connection.token.clone()))
    }

    fn update_snapshot(&mut self, revision: u64, result: Result<BridgeSnapshot, String>) -> bool {
        if self.check_revision(revision).is_err() {
            return false;
        }
        match result {
            Ok(snapshot) => {
                self.status.connected = true;
                self.status.received_at = Some(timestamp());
                self.status.error = None;
                self.status.snapshot = Some(snapshot);
            }
            Err(error) => {
                self.status.connected = false;
                self.status.snapshot = None;
                self.status.error = Some(error);
            }
        }
        true
    }
}

impl LiveBridge {
    pub fn start(&self, app: AppHandle) {
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(3));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if manager.stopped.load(Ordering::Acquire) {
                    break;
                }
                let worker = manager.clone();
                let app = app.clone();
                let _ = tauri::async_runtime::spawn_blocking(move || worker.poll(&app, None)).await;
            }
        });
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }

    fn poll(&self, app: &AppHandle, expected_revision: Option<u64>) -> Result<(), String> {
        let Ok(_transport) = self.transport.try_lock() else {
            return Ok(());
        };
        if self.stopped.load(Ordering::Acquire) {
            return Ok(());
        }
        let (revision, port, token) = {
            let state = self
                .state
                .lock()
                .map_err(|_| "Live bridge is unavailable.")?;
            if let Some(revision) = expected_revision {
                state.check_revision(revision)?;
            }
            let Some(connection) = &state.connection else {
                return Ok(());
            };
            (
                state.revision,
                connection.target.port,
                connection.token.clone(),
            )
        };
        let result = fetch_snapshot(port, &token);
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Live bridge is unavailable.")?;
        if self.stopped.load(Ordering::Acquire) || !state.update_snapshot(revision, result) {
            return Ok(());
        }
        let _ = app.emit("live-bridge-update", state.status.clone());
        Ok(())
    }

    fn disconnect(&self) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Live bridge is unavailable.")?;
        let workspace_id = state.status.workspace_id.clone();
        state.reset(workspace_id, false);
        Ok(())
    }
}

#[tauri::command]
pub async fn get_live_bridge_installation(
    app: AppHandle,
    target: BridgeTarget,
) -> Result<install::Installation, String> {
    super::run_blocking(move || install::inspect(&app, &target)).await
}

#[tauri::command]
pub async fn preview_live_bridge_change(
    app: AppHandle,
    target: BridgeTarget,
    remove: bool,
) -> Result<install::ChangePreview, String> {
    super::run_blocking(move || install::preview(&app, target, remove)).await
}

#[tauri::command]
pub async fn apply_live_bridge_change(
    app: AppHandle,
    preview_id: String,
    manager: tauri::State<'_, super::fxserver::FxserverManager>,
    bridge: tauri::State<'_, LiveBridge>,
) -> Result<install::Installation, String> {
    let manager = manager.inner().clone();
    let bridge = bridge.inner().clone();
    super::run_blocking(move || {
        manager.with_stopped_server(|| {
            install::with_operation(|| {
                bridge.disconnect()?;
                let result = install::apply(&app, &preview_id)?;
                super::logs::append_background_log(
                    &app,
                    "success",
                    "fxserver.bridge",
                    if result.installed {
                        "Live bridge installed. Start FXServer to connect."
                    } else {
                        "Live bridge removed."
                    },
                );
                Ok(result)
            })
        })
    })
    .await
}

#[tauri::command]
pub async fn configure_live_bridge(
    app: AppHandle,
    target: BridgeTarget,
    enabled: bool,
    bridge: tauri::State<'_, LiveBridge>,
) -> Result<BridgeStatus, String> {
    let bridge = bridge.inner().clone();
    if bridge.stopped.load(Ordering::Acquire) {
        return Err("Live bridge is shutting down.".into());
    }
    let revision = {
        let mut state = bridge
            .state
            .lock()
            .map_err(|_| "Live bridge is unavailable.")?;
        let revision = state.reset(target.workspace_id.clone(), enabled);
        if !enabled {
            return Ok(state.status.clone());
        }
        revision
    };
    super::run_blocking(move || {
        let result = install::with_operation(|| {
            let token = install::read_token(&app, &target);
            let mut state = bridge
                .state
                .lock()
                .map_err(|_| "Live bridge is unavailable.")?;
            state.connect(revision, target, token)
        });
        if let Err(error) = result {
            let mut state = bridge
                .state
                .lock()
                .map_err(|_| "Live bridge is unavailable.")?;
            state.check_revision(revision)?;
            state.status.error = Some(error.clone());
            return Err(error);
        }
        bridge.poll(&app, Some(revision))?;
        let state = bridge
            .state
            .lock()
            .map_err(|_| "Live bridge is unavailable.")?;
        state.check_revision(revision)?;
        Ok(state.status.clone())
    })
    .await
}

#[tauri::command]
pub async fn get_live_bridge_status(
    bridge: tauri::State<'_, LiveBridge>,
) -> Result<BridgeStatus, String> {
    Ok(bridge
        .state
        .lock()
        .map_err(|_| "Live bridge is unavailable.")?
        .status
        .clone())
}

#[tauri::command]
pub async fn send_live_bridge_action(
    app: AppHandle,
    workspace_id: String,
    action: String,
    resource: String,
    bridge: tauri::State<'_, LiveBridge>,
) -> Result<(), String> {
    validate_action(&action, &resource)?;
    let bridge = bridge.inner().clone();
    let revision = {
        let state = bridge
            .state
            .lock()
            .map_err(|_| "Live bridge is unavailable.")?;
        state.action_connection(state.revision, &workspace_id)?;
        state.revision
    };
    super::run_blocking(move || {
        let _transport = bridge
            .transport
            .try_lock()
            .map_err(|_| "Bridge is busy. Try again shortly.")?;
        if bridge.stopped.load(Ordering::Acquire) {
            return Err("Live bridge is shutting down.".into());
        }
        let (port, token) = {
            let state = bridge
                .state
                .lock()
                .map_err(|_| "Live bridge is unavailable.")?;
            state.action_connection(revision, &workspace_id)?
        };
        let body = serde_json::json!({ "action": action, "resource": resource }).to_string();
        let response = request(port, &token, Some(body))?;
        if response.get("accepted").and_then(|v| v.as_bool()) != Some(true) {
            return Err("The bridge did not acknowledge the action.".into());
        }
        super::logs::append_background_log(
            &app,
            "info",
            "fxserver.bridge",
            &format!("Bridge accepted {action} for {resource}."),
        );
        Ok(())
    })
    .await
}

fn validate_action(action: &str, resource: &str) -> Result<(), String> {
    if !matches!(action, "start" | "stop" | "restart" | "ensure")
        || resource.is_empty()
        || resource.len() > 96
        || resource.eq_ignore_ascii_case(RESOURCE_NAME)
        || !resource
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
    {
        return Err("Unsupported bridge resource action.".into());
    }
    Ok(())
}

fn request(port: u16, token: &str, body: Option<String>) -> Result<serde_json::Value, String> {
    if port == 0 {
        return Err("Enter the FXServer HTTP port.".into());
    }
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_millis(800))
        .timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not create bridge client.")?;
    let endpoint = if body.is_some() {
        "resource"
    } else {
        "snapshot"
    };
    let url = format!("http://127.0.0.1:{port}/{RESOURCE_NAME}/{endpoint}");
    let builder = if let Some(body) = body {
        client
            .post(url)
            .header("Content-Type", "application/json")
            .body(body)
    } else {
        client.get(url)
    };
    let response = builder
        .bearer_auth(token)
        .send()
        .map_err(|_| "Bridge not reachable. Check the server, HTTP port, and ensured resource.")?;
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            403 => "Bridge authentication rejected. Reinstall the bridge to repair pairing.".into(),
            429 => "Bridge is busy. Try again shortly.".into(),
            code => format!("Live bridge returned HTTP {code}. Check the server console."),
        });
    }
    let mut bytes = Vec::new();
    response
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read bridge response.")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Bridge response exceeded the size limit.".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Bridge returned invalid JSON.".into())
}

fn fetch_snapshot(port: u16, token: &str) -> Result<BridgeSnapshot, String> {
    let snapshot: BridgeSnapshot = serde_json::from_value(request(port, token, None)?)
        .map_err(|_| "Bridge protocol is incompatible. Reinstall its current version.")?;
    validate_snapshot(&snapshot)?;
    Ok(snapshot)
}

fn validate_snapshot(snapshot: &BridgeSnapshot) -> Result<(), String> {
    let states = [
        "missing",
        "started",
        "starting",
        "stopped",
        "stopping",
        "uninitialized",
        "unknown",
    ];
    if snapshot.protocol != 1
        || snapshot.instance_id.len() > 64
        || snapshot.version.len() > 64
        || snapshot.hostname.len() > 1024
        || snapshot.game_build.len() > 128
        || snapshot.onesync.len() > 64
        || snapshot.resources.len() > 5000
        || snapshot.players.len() > 512
        || snapshot.events.len() > 100
        || !snapshot.scheduler_delay_ms.is_finite()
        || snapshot.scheduler_delay_ms < 0.0
        || snapshot.resources.iter().any(|r| {
            r.name.len() > 512 || r.version.len() > 320 || !states.contains(&r.state.as_str())
        })
        || snapshot
            .players
            .iter()
            .any(|p| p.name.len() > 512 || p.id.len() > 32)
        || snapshot.events.iter().any(|e| {
            e.resource.len() > 512
                || !matches!(
                    e.kind.as_str(),
                    "resource-started" | "resource-stopped" | "resource-command"
                )
        })
    {
        return Err("Bridge status exceeded protocol limits.".into());
    }
    Ok(())
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(workspace_id: &str, port: u16) -> BridgeTarget {
        BridgeTarget {
            workspace_id: workspace_id.into(),
            tx_data_path: String::new(),
            profile: "fixture".into(),
            port,
        }
    }

    fn snapshot() -> BridgeSnapshot {
        serde_json::from_value(serde_json::json!({
            "protocol": 1, "version": "1.0.0", "instanceId": "fixture", "timestamp": 1,
            "uptimeSeconds": 1, "schedulerDelayMs": 0, "hostname": "Fixture", "gameBuild": "default",
            "onesync": "on", "maxPlayers": 48, "playerCount": 0, "resourceCount": 0,
            "resources": [], "players": [], "events": []
        })).unwrap()
    }

    fn serve_once(response: Vec<u8>) -> (u16, std::thread::JoinHandle<String>) {
        use std::{io::Write, net::TcpListener, thread, time::Instant};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("Fixture connection failed: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut input = Vec::new();
            while !input.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                input.push(byte[0]);
                assert!(input.len() < 8192);
            }
            let _ = stream.write_all(&response);
            String::from_utf8(input).unwrap()
        });
        (port, server)
    }

    #[test]
    fn actions_are_narrow_and_cannot_execute_arbitrary_console_input() {
        assert!(validate_action("ensure", "qbx_core").is_ok());
        for (action, resource) in [
            ("quit", "chat"),
            ("start", "a;quit"),
            ("stop", RESOURCE_NAME),
            ("stop", "FXSERVER_INSTALLER_BRIDGE"),
            ("ensure", ""),
            ("start", "a b"),
        ] {
            assert!(validate_action(action, resource).is_err());
        }
    }

    #[test]
    fn request_uses_loopback_bearer_auth_and_rejects_redirects() {
        let (port, server) = serve_once(
            b"HTTP/1.1 302 Found\r\nLocation: http://example.invalid/\r\nContent-Length: 0\r\n\r\n"
                .to_vec(),
        );
        assert!(request(port, "fixture", None).unwrap_err().contains("302"));
        let input = server.join().unwrap().to_lowercase();
        assert!(input.contains("authorization: bearer fixture\r\n"));
        assert!(input.starts_with("get /fxserver_installer_bridge/snapshot"));
        assert!(!input.lines().next().unwrap().contains("fixture"));
    }

    #[test]
    fn stale_configuration_poll_and_action_cannot_rebind_to_a_new_connection() {
        let mut state = BridgeState::default();
        let old = state.reset("same-workspace".into(), true);
        state
            .connect(old, target("same-workspace", 30120), Ok("old-key".into()))
            .unwrap();
        assert!(state.update_snapshot(old, Ok(snapshot())));
        assert!(state.action_connection(old, "same-workspace").is_ok());
        let new = state.reset("same-workspace".into(), true);
        state
            .connect(new, target("same-workspace", 30121), Ok("new-key".into()))
            .unwrap();
        state.update_snapshot(new, Ok(snapshot()));
        assert!(state
            .connect(old, target("old-workspace", 30120), Ok("old-key".into()))
            .is_err());
        assert!(state
            .connect(
                old,
                target("old-workspace", 30120),
                Err("old failure".into())
            )
            .is_err());
        assert!(!state.update_snapshot(old, Err("old poll failure".into())));
        assert!(state.action_connection(old, "same-workspace").is_err());
        assert!(state.action_connection(new, "different-workspace").is_err());
        assert_eq!(
            state.action_connection(new, "same-workspace").unwrap(),
            (30121, "new-key".into())
        );
        assert!(state.status.error.is_none() && state.status.connected);
    }

    #[test]
    fn disconnect_invalidates_pending_configuration_and_hides_old_snapshot() {
        let mut state = BridgeState::default();
        let revision = state.reset("fixture".into(), true);
        state.update_snapshot(revision, Ok(snapshot()));
        state.reset("fixture".into(), false);
        assert!(state
            .connect(revision, target("fixture", 30120), Ok("key".into()))
            .is_err());
        assert!(!state.update_snapshot(revision, Ok(snapshot())));
        assert!(!state.status.enabled && !state.status.connected);
        assert!(state.connection.is_none() && state.status.snapshot.is_none());
    }

    #[test]
    fn pairing_failure_remains_visible_without_a_connection() {
        let mut state = BridgeState::default();
        let revision = state.reset("fixture".into(), true);
        assert!(state
            .connect(
                revision,
                target("fixture", 30120),
                Err("Pairing unavailable".into())
            )
            .is_err());
        assert_eq!(state.status.error.as_deref(), Some("Pairing unavailable"));
        assert!(state.connection.is_none() && !state.status.connected);
    }

    #[test]
    fn responses_are_bounded_and_errors_do_not_echo_secrets() {
        for body in [
            "fixture-secret".to_string(),
            "a".repeat(2 * 1024 * 1024 + 1),
        ] {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let (port, server) = serve_once(response.into_bytes());
            let error = request(port, "fixture-secret", None).unwrap_err();
            assert!(!error.contains("fixture-secret"));
            assert!(error.contains(if body.len() > 2 * 1024 * 1024 {
                "size limit"
            } else {
                "invalid JSON"
            }));
            server.join().unwrap();
        }
        let (port, server) = serve_once(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 14\r\n\r\nfixture-secret".to_vec(),
        );
        assert!(!request(port, "fixture-secret", None)
            .unwrap_err()
            .contains("fixture-secret"));
        server.join().unwrap();
        assert!(request(0, "fixture-secret", None).is_err());
    }

    #[test]
    fn snapshots_reject_unbounded_fields_and_only_serialize_protocol_data() {
        let valid = snapshot();
        assert!(validate_snapshot(&valid).is_ok());
        let mut value = serde_json::to_value(&valid).unwrap();
        value["token"] = "fixture-secret".into();
        let typed: BridgeSnapshot = serde_json::from_value(value).unwrap();
        assert!(!serde_json::to_string(&typed)
            .unwrap()
            .contains("fixture-secret"));
        let mut invalid = valid.clone();
        invalid.protocol = 2;
        assert!(validate_snapshot(&invalid).is_err());
        invalid = valid.clone();
        invalid.hostname = "x".repeat(1025);
        assert!(validate_snapshot(&invalid).is_err());
        invalid = valid.clone();
        invalid.scheduler_delay_ms = f64::NAN;
        assert!(validate_snapshot(&invalid).is_err());
        invalid = valid.clone();
        invalid.players = vec![
            BridgePlayer {
                id: "1".into(),
                name: "Fixture".into(),
                ping: 10
            };
            513
        ];
        assert!(validate_snapshot(&invalid).is_err());
    }
}
