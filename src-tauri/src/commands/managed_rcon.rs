use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use super::{
    backup_manager::storage::secure_token,
    fxserver::{self, FxserverManager},
};
use crate::models::fxserver::{
    FxserverCommandRequest, FxserverRconConfig, FxserverStatus, FxserverTerminalEntry,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(6);

#[tauri::command]
pub async fn verify_managed_rcon_endpoint(
    expected_pid: u32,
    rcon: FxserverRconConfig,
    app: AppHandle,
) -> Result<(), String> {
    validate_local_rcon(&rcon)?;
    let nonce = format!("fxi-rcon-probe-{}", secure_token()?);
    let mut io = ManagedEndpoint {
        app,
        rcon,
        started: Instant::now(),
    };
    verify_endpoint(&mut io, expected_pid, &nonce).await
}

pub(crate) fn validate_local_rcon(rcon: &FxserverRconConfig) -> Result<(), String> {
    if !matches!(rcon.host.trim(), "127.0.0.1" | "localhost") || rcon.port == 0 {
        return Err("Managed RCON requires a local IPv4 endpoint (127.0.0.1 or localhost).".into());
    }
    if rcon.password.is_empty()
        || rcon
            .password
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err("Set a valid RCON password in Manage Server before sending commands.".into());
    }
    Ok(())
}

trait EndpointIo {
    async fn status(&mut self) -> Result<FxserverStatus, String>;
    async fn terminal(&mut self, after: Option<u64>) -> Result<Vec<FxserverTerminalEntry>, String>;
    async fn echo(&mut self, command: String) -> Result<(), String>;
    async fn wait(&mut self);
    fn elapsed(&self) -> Duration;
}

struct ManagedEndpoint {
    app: AppHandle,
    rcon: FxserverRconConfig,
    started: Instant,
}
impl EndpointIo for ManagedEndpoint {
    async fn status(&mut self) -> Result<FxserverStatus, String> {
        fxserver::get_fxserver_status(self.app.state::<FxserverManager>()).await
    }
    async fn terminal(&mut self, after: Option<u64>) -> Result<Vec<FxserverTerminalEntry>, String> {
        Ok(
            fxserver::get_fxserver_terminal(Some(5000), after, self.app.state::<FxserverManager>())
                .await?
                .entries,
        )
    }
    async fn echo(&mut self, command: String) -> Result<(), String> {
        // RCON response text is not evidence of managed-process ownership.
        fxserver::send_fxserver_rcon_command(FxserverCommandRequest {
            command,
            rcon: self.rcon.clone(),
        })
        .await?;
        Ok(())
    }
    async fn wait(&mut self) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

fn identity(status: FxserverStatus, expected_pid: u32) -> Result<(String, String), String> {
    if !status.running || expected_pid == 0 || status.pid != Some(expected_pid) {
        return Err(
            "The expected managed FXServer is no longer running. No server action was sent.".into(),
        );
    }
    Ok((
        status
            .started_at
            .ok_or("Managed server start time is unavailable.")?,
        status
            .artifact_path
            .ok_or("Managed server working directory is unavailable.")?,
    ))
}

fn is_probe_echo(entry: &FxserverTerminalEntry, after: u64, nonce: &str) -> bool {
    if entry.id <= after || entry.stream != "stdout" {
        return false;
    }
    let line = entry.plain_line.trim();
    if line == nonce {
        return true;
    }
    line.strip_prefix('[')
        .and_then(|line| line.split_once(']'))
        .is_some_and(|(channel, message)| {
            matches!(channel.trim(), "cmd" | "rcon/cmd") && message.trim() == nonce
        })
}

async fn verify_endpoint(
    io: &mut impl EndpointIo,
    expected_pid: u32,
    nonce: &str,
) -> Result<(), String> {
    let expected = identity(io.status().await?, expected_pid)?;
    let after = io
        .terminal(None)
        .await?
        .last()
        .map(|entry| entry.id)
        .unwrap_or(0);
    if identity(io.status().await?, expected_pid)? != expected {
        return Err("The managed server changed before RCON verification.".into());
    }
    io.echo(format!("echo {nonce}")).await?;
    while io.elapsed() < PROBE_TIMEOUT {
        if identity(io.status().await?, expected_pid)? != expected {
            return Err("The managed server changed during RCON verification.".into());
        }
        if io
            .terminal(Some(after))
            .await?
            .iter()
            .any(|entry| is_probe_echo(entry, after, nonce))
        {
            if identity(io.status().await?, expected_pid)? == expected {
                return Ok(());
            }
            return Err("The managed server changed during RCON verification.".into());
        }
        io.wait().await;
    }
    Err("RCON endpoint did not echo the verification token in the managed server's stdout. Check the local RCON port; no server action was sent.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(id: u64, stream: &str, line: &str) -> FxserverTerminalEntry {
        FxserverTerminalEntry {
            id,
            stream: stream.into(),
            line: line.into(),
            plain_line: line.into(),
            segments: vec![],
            timestamp: String::new(),
        }
    }
    struct Fake {
        entries: Vec<FxserverTerminalEntry>,
        clock: Duration,
        sent: Vec<String>,
        changed: bool,
    }
    impl EndpointIo for Fake {
        async fn status(&mut self) -> Result<FxserverStatus, String> {
            Ok(FxserverStatus {
                running: true,
                pid: Some(if self.changed && !self.sent.is_empty() {
                    99
                } else {
                    7
                }),
                artifact_path: Some("fixture".into()),
                started_at: Some("fixture-start".into()),
                uptime_seconds: None,
                resources: None,
            })
        }
        async fn terminal(
            &mut self,
            after: Option<u64>,
        ) -> Result<Vec<FxserverTerminalEntry>, String> {
            Ok(if after.is_none() {
                vec![entry(10, "stdout", "old")]
            } else {
                self.entries.clone()
            })
        }
        async fn echo(&mut self, command: String) -> Result<(), String> {
            self.sent.push(command);
            Ok(())
        }
        async fn wait(&mut self) {
            self.clock += Duration::from_secs(1);
        }
        fn elapsed(&self) -> Duration {
            self.clock
        }
    }
    #[tokio::test]
    async fn only_fresh_exact_managed_stdout_proves_endpoint() {
        for (stream, line, id, valid) in [
            ("stdout", "nonce", 11, true),
            ("stdout", "[                 cmd] nonce", 11, true),
            ("stdout", "[            rcon/cmd] nonce", 11, true),
            ("rcon", "nonce", 11, false),
            ("command", "nonce", 11, false),
            ("stderr", "nonce", 11, false),
            ("stdout", "echo nonce", 11, false),
            ("stdout", "[script:other] nonce", 11, false),
            ("stdout", "[rcon/script:other] nonce", 11, false),
            ("rcon", "[rcon/cmd] nonce", 11, false),
            ("stdout", "[rcon/cmd] echo nonce", 11, false),
            ("stdout", "nonce", 10, false),
        ] {
            let mut io = Fake {
                entries: vec![entry(id, stream, line)],
                clock: Duration::ZERO,
                sent: vec![],
                changed: false,
            };
            assert_eq!(verify_endpoint(&mut io, 7, "nonce").await.is_ok(), valid);
            assert_eq!(io.sent, ["echo nonce"]);
            assert!(io.clock <= PROBE_TIMEOUT);
        }
    }
    #[tokio::test]
    async fn wrong_endpoint_and_server_change_fail_without_actions() {
        let mut io = Fake {
            entries: vec![],
            clock: Duration::ZERO,
            sent: vec![],
            changed: false,
        };
        assert!(verify_endpoint(&mut io, 8, "nonce").await.is_err());
        assert!(io.sent.is_empty());
        assert!(verify_endpoint(&mut io, 7, "nonce").await.is_err());
        assert_eq!(io.sent, ["echo nonce"]);
        io.sent.clear();
        io.clock = Duration::ZERO;
        io.changed = true;
        io.entries = vec![entry(11, "stdout", "nonce")];
        assert!(verify_endpoint(&mut io, 7, "nonce").await.is_err());
    }
    #[test]
    fn only_local_endpoints_and_single_token_passwords_are_accepted() {
        for host in ["remote.example", "127.0.0.2", "127.0.0.1; quit"] {
            assert!(validate_local_rcon(&FxserverRconConfig {
                host: host.into(),
                port: 30120,
                password: "secret".into()
            })
            .is_err());
        }
        assert!(validate_local_rcon(&FxserverRconConfig {
            host: "127.0.0.1".into(),
            port: 30120,
            password: "secret\nquit".into()
        })
        .is_err());
    }
    #[cfg(windows)]
    #[test]
    fn probe_tokens_use_unpredictable_128_bit_randomness() {
        let first = secure_token().unwrap();
        let second = secure_token().unwrap();
        assert_eq!(first.len(), 32);
        assert!(first.bytes().all(|ch| ch.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }
}
