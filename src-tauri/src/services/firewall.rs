//! FiveM host firewall rules. Router forwarding and provider firewalls are out of scope.
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[path = "firewall/elevation.rs"]
mod elevation;
#[cfg(any(target_os = "linux", test))]
#[path = "firewall/linux.rs"]
mod linux;
#[path = "firewall/runtime.rs"]
mod runtime;
#[cfg(any(windows, test))]
#[path = "firewall/windows.rs"]
mod windows;

pub use elevation::run_firewall_helper_from_args;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FirewallBackend {
    #[default]
    Auto,
    Windows,
    Ufw,
    Firewalld,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FirewallRequest {
    #[serde(default)]
    pub backend: FirewallBackend,
    pub tcp_port: u16,
    pub udp_port: u16,
}

impl FirewallRequest {
    fn validate(self) -> Result<Self, String> {
        if self.tcp_port == 0 || self.udp_port == 0 {
            return Err("TCP and UDP ports must be between 1 and 65535.".into());
        }
        if self.backend == FirewallBackend::Unsupported {
            return Err("This firewall backend cannot be managed by the app.".into());
        }
        Ok(self)
    }

    fn checks(self) -> Vec<FirewallCheck> {
        [Direction::Inbound, Direction::Outbound]
            .into_iter()
            .flat_map(|direction| {
                [
                    (Protocol::Tcp, self.tcp_port),
                    (Protocol::Udp, self.udp_port),
                ]
                .map(|(protocol, port)| FirewallCheck {
                    direction,
                    protocol,
                    port,
                    state: RuleState::Unknown,
                    owned_rule_present: false,
                    detail: "Not inspected.".into(),
                })
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Inbound,
    Outbound,
}

impl Direction {
    fn short(self) -> &'static str {
        match self {
            Self::Inbound => "in",
            Self::Outbound => "out",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    fn name(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuleState {
    Owned,
    External,
    DefaultAllowed,
    Missing,
    Blocked,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallCheck {
    pub direction: Direction,
    pub protocol: Protocol,
    pub port: u16,
    pub state: RuleState,
    pub owned_rule_present: bool,
    pub detail: String,
}

impl FirewallCheck {
    fn id(&self) -> String {
        format!(
            "fxins-fw-v1-{}-{}-{}",
            self.direction.short(),
            self.protocol.name(),
            self.port
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallSnapshot {
    pub backend: FirewallBackend,
    pub active: Option<bool>,
    pub requires_elevation: bool,
    pub can_configure: bool,
    pub can_remove: bool,
    pub reload_required: bool,
    pub checks: Vec<FirewallCheck>,
    pub notices: Vec<String>,
}

impl FirewallSnapshot {
    fn new(request: FirewallRequest, backend: FirewallBackend) -> Self {
        Self { backend, active: None, requires_elevation: false, can_configure: false,
            can_remove: false, reload_required: false, checks: request.checks(), notices: vec![
                "Only exact app-owned rules can be removed. Existing external rules and default policies are never changed.".into(),
                "This checks host firewall configuration, not Internet reachability. Router/NAT forwarding, hosting-provider rules, bind addresses and the running server must also be correct.".into(),
                "Outbound checks concern the server's local/source port, not unrestricted Internet access for resources.".into(),
            ] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Inspect,
    Configure,
    Remove,
}

static ACTION: Mutex<()> = Mutex::new(());

/// No automatic elevation. Linux returns `requiresElevation` for an explicit UI action.
pub fn inspect(request: FirewallRequest) -> Result<FirewallSnapshot, String> {
    inspect_local(request.validate()?)
}

pub fn inspect_elevated(request: FirewallRequest) -> Result<FirewallSnapshot, String> {
    act(request, Operation::Inspect)
}

pub fn configure(request: FirewallRequest) -> Result<FirewallSnapshot, String> {
    act(request, Operation::Configure)
}

pub fn remove(request: FirewallRequest) -> Result<FirewallSnapshot, String> {
    act(request, Operation::Remove)
}

fn act(request: FirewallRequest, operation: Operation) -> Result<FirewallSnapshot, String> {
    let request = request.validate()?;
    let _guard = ACTION
        .try_lock()
        .map_err(|_| "A firewall action is already running.")?;
    log::info!(
        "Firewall {:?} requested: {:?}, TCP {}, UDP {}",
        operation,
        request.backend,
        request.tcp_port,
        request.udp_port
    );
    let result = elevation::launch(request, operation);
    match &result {
        Ok(snapshot) => log::info!(
            "Firewall {:?} finished: {:?}, reload required: {}",
            operation,
            snapshot.backend,
            snapshot.reload_required
        ),
        Err(error) => log::warn!("Firewall {:?} failed: {}", operation, error),
    }
    result
}

fn inspect_local(request: FirewallRequest) -> Result<FirewallSnapshot, String> {
    #[cfg(windows)]
    {
        windows::inspect(request)
    }
    #[cfg(target_os = "linux")]
    {
        linux::inspect(request)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Ok(FirewallSnapshot::new(request, FirewallBackend::Unsupported))
    }
}

fn execute_privileged(
    request: FirewallRequest,
    operation: Operation,
) -> Result<FirewallSnapshot, String> {
    let request = request.validate()?;
    if !elevation::is_elevated()? {
        return Err("The firewall helper requires administrator approval.".into());
    }
    #[cfg(windows)]
    {
        windows::execute(request, operation)
    }
    #[cfg(target_os = "linux")]
    {
        linux::execute(request, operation)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = operation;
        Err("Unsupported operating system.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_reject_out_of_range_ports_and_unknown_fields() {
        for json in [
            r#"{"tcpPort":-1,"udpPort":30120}"#,
            r#"{"tcpPort":65536,"udpPort":30120}"#,
            r#"{"tcpPort":30120,"udpPort":30120,"script":"anything"}"#,
            r#"{"tcpPort":"80;id","udpPort":30120}"#,
            r#"{"tcpPort":30120,"udpPort":30120,"backend":"nft"}"#,
        ] {
            assert!(serde_json::from_str::<FirewallRequest>(json).is_err());
        }
        let zero = FirewallRequest {
            backend: FirewallBackend::Auto,
            tcp_port: 0,
            udp_port: 30120,
        };
        assert!(zero.validate().is_err());
    }

    #[test]
    fn every_check_is_a_direction_protocol_and_local_port() {
        let request = FirewallRequest {
            backend: FirewallBackend::Auto,
            tcp_port: 30120,
            udp_port: 30121,
        };
        let checks = request.checks();
        assert_eq!(checks.len(), 4);
        let ids: std::collections::BTreeSet<_> = checks.iter().map(FirewallCheck::id).collect();
        assert_eq!(ids.len(), 4);
        for c in checks {
            assert_eq!(
                c.port,
                match c.protocol {
                    Protocol::Tcp => 30120,
                    Protocol::Udp => 30121,
                }
            );
        }
    }
}
