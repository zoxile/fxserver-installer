#[cfg(windows)]
use super::Operation;
use super::{
    Direction, FirewallBackend, FirewallCheck, FirewallRequest, FirewallSnapshot, Protocol,
    RuleState,
};
use serde::Deserialize;

const GROUP: &str = "FXServer Installer - FiveM v1";
const DESCRIPTION: &str = "Managed by FXServer Installer. FiveM host port rule v1.";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    enabled: bool,
    default_inbound: String,
    default_outbound: String,
    allow_inbound: String,
    allow_local: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rule {
    name: String,
    group: String,
    description: String,
    enabled: bool,
    direction: String,
    action: String,
    profile: String,
    protocol: String,
    local_port: Vec<String>,
    remote_port: Vec<String>,
    local_address: Vec<String>,
    remote_address: Vec<String>,
    program: String,
    package: String,
    service: String,
    interface_alias: Vec<String>,
    interface_type: String,
    local_user: String,
    remote_user: String,
    remote_machine: String,
    authentication: String,
    encryption: String,
    source_type: String,
}

#[derive(Deserialize)]
struct Probe {
    #[serde(default, rename = "requiresElevation")]
    requires_elevation: bool,
    profiles: Vec<Profile>,
    rules: Vec<Rule>,
    owned: Vec<Rule>,
}

fn any(values: &[String]) -> bool {
    values.len() == 1 && values[0].eq_ignore_ascii_case("Any")
}
fn unset(value: &str) -> bool {
    value.is_empty() || value.eq_ignore_ascii_case("Any")
}

fn full_scope(rule: &Rule) -> bool {
    any(&rule.remote_port)
        && any(&rule.local_address)
        && any(&rule.remote_address)
        && unset(&rule.program)
        && unset(&rule.package)
        && unset(&rule.service)
        && any(&rule.interface_alias)
        && rule.interface_type.eq_ignore_ascii_case("Any")
        && unset(&rule.local_user)
        && unset(&rule.remote_user)
        && unset(&rule.remote_machine)
        && rule.authentication.eq_ignore_ascii_case("NotRequired")
        && rule.encryption.eq_ignore_ascii_case("NotRequired")
        && (rule.profile.eq_ignore_ascii_case("Any") || rule.profile == "Domain, Private, Public")
}

fn matches_port(value: &str, port: u16) -> bool {
    value.eq_ignore_ascii_case("Any")
        || value.parse::<u16>().ok() == Some(port)
        || value.split_once('-').is_some_and(|(a, b)| {
            a.parse::<u16>()
                .ok()
                .zip(b.parse::<u16>().ok())
                .is_some_and(|(a, b)| (a..=b).contains(&port))
        })
}

fn applies(rule: &Rule, check: &FirewallCheck) -> bool {
    let direction = match check.direction {
        Direction::Inbound => "Inbound",
        Direction::Outbound => "Outbound",
    };
    let protocol = match check.protocol {
        Protocol::Tcp => "6",
        Protocol::Udp => "17",
    };
    rule.enabled
        && rule.direction == direction
        && (rule.protocol.eq_ignore_ascii_case(check.protocol.name())
            || rule.protocol == protocol
            || rule.protocol.eq_ignore_ascii_case("Any")
            || rule.protocol == "256")
        && rule
            .local_port
            .iter()
            .any(|p| matches_port(p, check.port) || p.parse::<u16>().is_err() && !p.contains('-'))
}

fn owned(rule: &Rule, check: &FirewallCheck) -> bool {
    rule.name == check.id()
        && rule.group == GROUP
        && rule.description == DESCRIPTION
        && rule.source_type.eq_ignore_ascii_case("Local")
        && rule.action == "Allow"
        && rule.direction
            == match check.direction {
                Direction::Inbound => "Inbound",
                Direction::Outbound => "Outbound",
            }
        && (rule.protocol.eq_ignore_ascii_case(check.protocol.name())
            || rule.protocol
                == match check.protocol {
                    Protocol::Tcp => "6",
                    Protocol::Udp => "17",
                })
        && rule.local_port == [check.port.to_string()]
        && full_scope(rule)
}

fn snapshot(request: FirewallRequest, probe: &Probe) -> FirewallSnapshot {
    let mut result = FirewallSnapshot::new(request, FirewallBackend::Windows);
    if probe.requires_elevation {
        result.requires_elevation = true;
        for check in &mut result.checks {
            check.detail = "Windows requires administrator approval to read complete firewall filter collections.".into();
        }
        result.notices.push("Use Check with administrator approval. This inspection does not change any firewall rules.".into());
        return result;
    }
    result.active = Some(!probe.profiles.is_empty() && probe.profiles.iter().all(|p| p.enabled));
    for check in &mut result.checks {
        check.owned_rule_present = probe.owned.iter().any(|r| owned(r, check));
        let collision = probe
            .owned
            .iter()
            .any(|r| r.name == check.id() && !owned(r, check));
        let blocked = probe
            .rules
            .iter()
            .any(|r| applies(r, check) && r.action == "Block");
        let policy_block = probe.profiles.iter().any(|p| {
            p.allow_local.eq_ignore_ascii_case("False")
                || check.direction == Direction::Inbound
                    && p.allow_inbound.eq_ignore_ascii_case("False")
        });
        let active_owned = probe.rules.iter().any(|r| r.enabled && owned(r, check));
        let external = probe.rules.iter().any(|r| {
            applies(r, check)
                && r.action == "Allow"
                && full_scope(r)
                && r.local_port.iter().any(|v| matches_port(v, check.port))
        });
        let default_allow = !probe.profiles.is_empty()
            && probe.profiles.iter().all(|p| {
                let action = match check.direction {
                    Direction::Inbound => &p.default_inbound,
                    Direction::Outbound => &p.default_outbound,
                };
                action.eq_ignore_ascii_case("Allow")
            });
        let (state, detail) = if collision {
            (RuleState::Unknown, "A rule uses the app's reserved name but has different settings. It will not be changed or removed.")
        } else if blocked || policy_block {
            (RuleState::Blocked, "An applicable block rule or policy restriction exists. Adding an allow rule cannot override it; ask the firewall administrator.")
        } else if result.active != Some(true) {
            (RuleState::Unknown, "One or more network profiles have a disabled firewall, or their status could not be verified. The app will not enable or disable it.")
        } else if active_owned {
            (RuleState::Owned, "An exact app-owned allow rule is active.")
        } else if external {
            (RuleState::External, "An existing unrestricted allow rule covers this port. It is not removable by this app.")
        } else if default_allow {
            (
                RuleState::DefaultAllowed,
                "The default policy allows this direction; no extra rule is necessary.",
            )
        } else {
            (
                RuleState::Missing,
                "No unrestricted allow rule was found for this local port across all profiles.",
            )
        };
        check.state = state;
        check.detail = detail.into();
    }
    result.can_configure = result.active == Some(true)
        && result
            .checks
            .iter()
            .all(|c| !matches!(c.state, RuleState::Unknown | RuleState::Blocked))
        && result.checks.iter().any(|c| c.state == RuleState::Missing);
    result.can_remove = result.checks.iter().any(|c| c.owned_rule_present);
    result.notices.push("Rules restricted to particular programs, addresses, users or profiles are not treated as universal access. Matching block rules are reported conservatively even if scoped more narrowly.".into());
    result
}

#[cfg(windows)]
fn query(
    request: FirewallRequest,
    operation: Operation,
    indexes: &[usize],
) -> Result<Probe, String> {
    let selected = indexes
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let operation = match operation {
        Operation::Inspect => "inspect",
        Operation::Configure => "configure",
        Operation::Remove => "remove",
    };
    let script = format!(
        "$operation='{operation}'; $tcpPort={}; $udpPort={}; $selected=@({selected});\n{}",
        request.tcp_port,
        request.udp_port,
        include_str!("windows.ps1")
    );
    let output = super::runtime::checked(&mut powershell_command(&script)?)?;
    let probe: Probe = serde_json::from_str(&output)
        .map_err(|e| format!("Cannot interpret Windows firewall state: {e}"))?;
    if probe.requires_elevation && super::elevation::is_elevated()? {
        return Err("Windows denied firewall inspection even with administrator access. Check system policy; no automatic rule changes were attempted.".into());
    }
    Ok(probe)
}

#[cfg(windows)]
fn powershell_command(script: &str) -> Result<std::process::Command, String> {
    let executable = super::runtime::powershell()?;
    let system_dir = executable
        .ancestors()
        .nth(3)
        .ok_or("Cannot locate Windows system directory.")?;
    let windows_dir = system_dir
        .parent()
        .ok_or("Cannot locate Windows directory.")?;
    let mut command = std::process::Command::new(&executable);
    command
        .env_clear()
        .env("SystemRoot", windows_dir)
        .env("WINDIR", windows_dir)
        .env("PATH", system_dir)
        .current_dir(system_dir)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ]);
    Ok(command)
}

#[cfg(windows)]
pub(super) fn inspect(request: FirewallRequest) -> Result<FirewallSnapshot, String> {
    if !matches!(
        request.backend,
        FirewallBackend::Auto | FirewallBackend::Windows
    ) {
        return Err("Select the Windows firewall backend on Windows.".into());
    }
    Ok(snapshot(request, &query(request, Operation::Inspect, &[])?))
}

#[cfg(windows)]
pub(super) fn execute(
    request: FirewallRequest,
    operation: Operation,
) -> Result<FirewallSnapshot, String> {
    let before = inspect(request)?;
    if operation == Operation::Inspect {
        return Ok(before);
    }
    if operation == Operation::Configure && !before.can_configure {
        if before.checks.iter().all(|c| {
            matches!(
                c.state,
                RuleState::Owned | RuleState::External | RuleState::DefaultAllowed
            )
        }) {
            return Ok(before);
        }
        return Err("Firewall configuration has blockers or unknown settings. Inspect and resolve them before adding rules.".into());
    }
    let selected: Vec<usize> = before
        .checks
        .iter()
        .enumerate()
        .filter(|(_, c)| match operation {
            Operation::Configure => c.state == RuleState::Missing,
            Operation::Remove => c.owned_rule_present,
            _ => false,
        })
        .map(|(i, _)| i)
        .collect();
    if selected.is_empty() {
        return Ok(before);
    }
    log::info!(
        "Firewall {:?}: Windows TCP {} UDP {}",
        operation,
        request.tcp_port,
        request.udp_port
    );
    Ok(snapshot(request, &query(request, operation, &selected)?))
}

#[cfg(all(windows, test))]
#[path = "windows_integration.rs"]
mod integration;

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    #[ignore = "Opt-in read-only Windows Defender Firewall probe; never changes rules or elevates"]
    fn readonly_windows_firewall_schema_probe() {
        let request = FirewallRequest {
            backend: FirewallBackend::Windows,
            tcp_port: 30120,
            udp_port: 30120,
        };
        let result = inspect(request)
            .expect("Read-only Windows firewall inspection must match the fixture schema");
        assert_eq!(result.backend, FirewallBackend::Windows);
        assert_eq!(result.checks.len(), 4);
        if result.requires_elevation {
            assert!(result.checks.iter().all(|c| c.state == RuleState::Unknown));
            assert!(!result.can_configure && !result.can_remove);
        }
    }
    fn fixture() -> (FirewallRequest, Probe) {
        (
            FirewallRequest {
                backend: FirewallBackend::Windows,
                tcp_port: 30120,
                udp_port: 30120,
            },
            serde_json::from_str(include_str!("fixtures/windows.json")).unwrap(),
        )
    }
    #[test]
    fn block_overrides_owned_and_external_allow() {
        let (req, mut p) = fixture();
        let c = req.checks().remove(0);
        p.rules[0].name = c.id();
        p.rules[0].group = GROUP.into();
        p.rules[0].description = DESCRIPTION.into();
        p.owned.clear();
        let mut block: Rule =
            serde_json::from_str(include_str!("fixtures/windows_rule.json")).unwrap();
        block.action = "Block".into();
        p.rules.push(block);
        assert_eq!(snapshot(req, &p).checks[0].state, RuleState::Blocked);
        assert!(!snapshot(req, &p).can_configure);
    }
    #[test]
    fn external_and_default_allow_are_distinct() {
        let (req, p) = fixture();
        let s = snapshot(req, &p);
        assert_eq!(s.checks[0].state, RuleState::External);
        assert_eq!(s.checks[1].state, RuleState::Missing);
        assert_eq!(s.checks[2].state, RuleState::DefaultAllowed);
        assert!(!s.can_remove);
    }

    #[test]
    fn permission_required_is_not_reported_as_an_empty_allowed_firewall() {
        let (req, mut p) = fixture();
        p.requires_elevation = true;
        let result = snapshot(req, &p);
        assert!(result.requires_elevation);
        assert_eq!(result.active, None);
        assert!(result.checks.iter().all(|c| c.state == RuleState::Unknown));
        assert!(!result.can_configure && !result.can_remove);
    }
    #[test]
    fn changed_owned_rules_are_never_removed() {
        let (req, mut p) = fixture();
        let c = req.checks().remove(0);
        let mut r: Rule = serde_json::from_str(include_str!("fixtures/windows_rule.json")).unwrap();
        r.name = c.id();
        r.group = GROUP.into();
        r.description = DESCRIPTION.into();
        assert!(owned(&r, &c));
        r.remote_address = vec!["10.0.0.1".into()];
        assert!(!owned(&r, &c));
        p.owned.push(r);
        let s = snapshot(req, &p);
        assert_eq!(s.checks[0].state, RuleState::Unknown);
        assert!(!s.can_remove);
    }
    #[test]
    fn scoped_allow_and_disabled_profile_are_not_green() {
        let (req, mut p) = fixture();
        p.rules[0].program = "C:\\server.exe".into();
        assert_eq!(snapshot(req, &p).checks[0].state, RuleState::Missing);
        p.profiles[0].enabled = false;
        assert_eq!(snapshot(req, &p).checks[0].state, RuleState::Unknown);
    }
}
