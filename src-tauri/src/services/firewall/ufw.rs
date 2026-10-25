#[cfg(target_os = "linux")]
use super::super::Operation;
use super::super::{
    Direction, FirewallBackend, FirewallCheck, FirewallRequest, FirewallSnapshot, Protocol,
    RuleState,
};

#[derive(Debug)]
struct Rule {
    action: String,
    direction: Direction,
    protocol: Option<Protocol>,
    local_port: Option<String>,
    remote_port: Option<String>,
    scoped: bool,
    comment: Option<String>,
    known: bool,
}

fn tokens(line: &str) -> Option<Vec<String>> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for ch in line.chars() {
        match (quote, ch) {
            (Some(q), c) if q == c => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, c) if c.is_whitespace() => {
                if !word.is_empty() {
                    result.push(std::mem::take(&mut word));
                }
            }
            (_, c) => word.push(c),
        }
    }
    if quote.is_some() {
        return None;
    }
    if !word.is_empty() {
        result.push(word);
    }
    Some(result)
}

fn parse(line: &str) -> Option<Rule> {
    let mut t = tokens(line)?;
    if t.first()?.as_str() != "ufw" || t.len() < 2 {
        return None;
    }
    let comment = if let Some(i) = t.iter().position(|w| w == "comment") {
        if i + 2 != t.len() {
            return None;
        }
        let c = t[i + 1].clone();
        t.truncate(i);
        Some(c)
    } else {
        None
    };
    let mut r = Rule {
        action: t[1].clone(),
        direction: Direction::Inbound,
        protocol: None,
        local_port: None,
        remote_port: None,
        scoped: false,
        comment,
        known: true,
    };
    let mut i = 2;
    if t.get(i).is_some_and(|s| s == "in" || s == "out") {
        if t[i] == "out" {
            r.direction = Direction::Outbound;
        }
        i += 1;
    }
    if let Some(value) = t
        .get(i)
        .filter(|v| v.bytes().next().is_some_and(|b| b.is_ascii_digit()))
    {
        let (port, proto) = value
            .split_once('/')
            .map_or((value.as_str(), None), |(p, s)| (p, Some(s)));
        r.protocol = match proto {
            Some("tcp") => Some(Protocol::Tcp),
            Some("udp") => Some(Protocol::Udp),
            None => None,
            _ => {
                r.known = false;
                None
            }
        };
        if r.direction == Direction::Inbound {
            r.local_port = Some(port.into());
        } else {
            r.remote_port = Some(port.into());
        }
        i += 1;
    }
    while i < t.len() {
        match t[i].as_str() {
            "proto" if i + 1 < t.len() => {
                r.protocol = match t[i + 1].as_str() {
                    "tcp" => Some(Protocol::Tcp),
                    "udp" => Some(Protocol::Udp),
                    "any" => None,
                    _ => {
                        r.known = false;
                        None
                    }
                };
                i += 2;
            }
            "from" | "to" if i + 1 < t.len() => {
                let local = (t[i] == "to") == (r.direction == Direction::Inbound);
                r.scoped |= !matches!(t[i + 1].as_str(), "any" | "Anywhere");
                i += 2;
                if t.get(i).is_some_and(|w| w == "port") && i + 1 < t.len() {
                    if local {
                        r.local_port = Some(t[i + 1].clone());
                    } else {
                        r.remote_port = Some(t[i + 1].clone());
                    }
                    i += 2;
                }
            }
            _ => {
                r.known = false;
                i += 1;
            }
        }
    }
    r.known &= [&r.local_port, &r.remote_port]
        .into_iter()
        .flatten()
        .all(|v| valid_ports(v));
    Some(r)
}

fn valid_ports(value: &str) -> bool {
    value.split(',').all(|v| {
        let number = |s: &str| s.parse::<u16>().ok().filter(|n| *n != 0);
        match v.split_once(':') {
            Some((a, b)) => number(a).zip(number(b)).is_some_and(|(a, b)| a <= b),
            None => number(v).is_some(),
        }
    })
}

fn covers(value: &str, port: u16) -> bool {
    value.split(',').any(|s| {
        s.parse::<u16>().ok() == Some(port)
            || s.split_once(':').is_some_and(|(a, b)| {
                a.parse::<u16>()
                    .ok()
                    .zip(b.parse::<u16>().ok())
                    .is_some_and(|(a, b)| (a..=b).contains(&port))
            })
    })
}

fn applies(r: &Rule, c: &FirewallCheck) -> bool {
    r.direction == c.direction
        && r.protocol.is_none_or(|p| p == c.protocol)
        && r.local_port.as_deref().is_none_or(|v| covers(v, c.port))
}

fn owned(r: &Rule, c: &FirewallCheck) -> bool {
    r.comment.as_deref() == Some(c.id().as_str())
        && r.action == "allow"
        && r.known
        && !r.scoped
        && r.direction == c.direction
        && r.protocol == Some(c.protocol)
        && r.local_port.as_deref() == Some(c.port.to_string().as_str())
        && r.remote_port.is_none()
}

fn snapshot(request: FirewallRequest, verbose: &str, added: &str) -> FirewallSnapshot {
    let mut result = FirewallSnapshot::new(request, FirewallBackend::Ufw);
    result.active = Some(verbose.lines().any(|l| l.trim() == "Status: active"));
    let lines: Vec<&str> = added
        .lines()
        .filter(|l| l.trim_start().starts_with("ufw "))
        .collect();
    let rules: Vec<_> = lines.iter().filter_map(|l| parse(l.trim())).collect();
    let unknown = lines.len() != rules.len()
        || rules.iter().any(|r| {
            !r.known || !matches!(r.action.as_str(), "allow" | "deny" | "reject" | "limit")
        });
    for check in &mut result.checks {
        let owned_count = rules.iter().filter(|r| owned(r, check)).count();
        let same_shape = rules
            .iter()
            .filter(|r| {
                r.action == "allow"
                    && r.known
                    && !r.scoped
                    && r.direction == check.direction
                    && r.protocol == Some(check.protocol)
                    && r.local_port.as_deref() == Some(check.port.to_string().as_str())
                    && r.remote_port.is_none()
            })
            .count();
        // UFW ignores comments when comparing rule identity. Never delete an ambiguous duplicate.
        check.owned_rule_present = owned_count == 1 && same_shape == 1;
        let collision = rules
            .iter()
            .any(|r| r.comment.as_deref() == Some(check.id().as_str()) && !owned(r, check))
            || owned_count > 0 && !check.owned_rule_present;
        let blocker = rules
            .iter()
            .any(|r| applies(r, check) && matches!(r.action.as_str(), "deny" | "reject" | "limit"));
        let external = rules.iter().any(|r| {
            applies(r, check) && r.action == "allow" && !r.scoped && r.remote_port.is_none()
        });
        let policy = match check.direction {
            Direction::Inbound => "allow (incoming)",
            Direction::Outbound => "allow (outgoing)",
        };
        let default_allow = verbose
            .lines()
            .any(|l| l.starts_with("Default:") && l.contains(policy));
        let (state, detail) = if collision {
            (RuleState::Unknown, "A reserved comment is attached to modified or duplicate rules. They will not be changed or removed.")
        } else if blocker {
            (RuleState::Blocked, "An applicable deny, reject or limit rule exists. The app will not reorder or override it.")
        } else if unknown || result.active != Some(true) {
            (
                RuleState::Unknown,
                "UFW is inactive or contains rules this app cannot safely interpret.",
            )
        } else if check.owned_rule_present {
            (
                RuleState::Owned,
                "An exact app-owned allow rule covers this local port.",
            )
        } else if external {
            (
                RuleState::External,
                "An existing external allow rule covers this port and will remain untouched.",
            )
        } else if default_allow {
            (
                RuleState::DefaultAllowed,
                "UFW's default policy permits this direction; no additional rule is needed.",
            )
        } else {
            (
                RuleState::Missing,
                "No unrestricted UFW allow rule covers this local port.",
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
    result.notices.push("UFW's custom before/after rules and independently managed nftables/iptables rules can still restrict traffic. This is a UFW configuration check, not a packet-path guarantee. Rules apply to the address families enabled by UFW.".into());
    result
}

fn arguments(check: &FirewallCheck, remove: bool) -> Vec<String> {
    let mut args = Vec::new();
    if remove {
        args.extend(["--force".into(), "delete".into()]);
    }
    args.extend([
        "allow".into(),
        check.direction.short().into(),
        "proto".into(),
        check.protocol.name().into(),
        "from".into(),
        "any".into(),
    ]);
    if check.direction == Direction::Outbound {
        args.extend(["port".into(), check.port.to_string()]);
    }
    args.extend(["to".into(), "any".into()]);
    if check.direction == Direction::Inbound {
        args.extend(["port".into(), check.port.to_string()]);
    }
    args.extend(["comment".into(), check.id()]);
    args
}

#[cfg(target_os = "linux")]
fn command(args: &[String]) -> Result<String, String> {
    super::super::runtime::checked(
        std::process::Command::new(super::super::runtime::system_program("ufw")?).args(args),
    )
}

#[cfg(target_os = "linux")]
pub(super) fn inspect(request: FirewallRequest, verbose: &str) -> Result<FirewallSnapshot, String> {
    Ok(snapshot(
        request,
        verbose,
        &command(&["show".into(), "added".into()])?,
    ))
}

#[cfg(target_os = "linux")]
pub(super) fn execute(
    request: FirewallRequest,
    operation: Operation,
    before: FirewallSnapshot,
) -> Result<FirewallSnapshot, String> {
    if operation == Operation::Configure && !before.can_configure {
        if before.checks.iter().all(|c| {
            matches!(
                c.state,
                RuleState::Owned | RuleState::External | RuleState::DefaultAllowed
            )
        }) {
            return Ok(before);
        }
        return Err("Resolve blocking or unknown UFW rules before configuring ports.".into());
    }
    for check in &before.checks {
        let needed = match operation {
            Operation::Configure => check.state == RuleState::Missing,
            Operation::Remove => check.owned_rule_present,
            _ => false,
        };
        if !needed {
            continue;
        }
        let fresh = inspect(request, &command(&["status".into(), "verbose".into()])?)?;
        let current = fresh
            .checks
            .iter()
            .find(|c| c.id() == check.id())
            .ok_or("Firewall inspection changed unexpectedly.")?;
        if operation == Operation::Remove && !current.owned_rule_present
            || operation == Operation::Configure
                && (!fresh.can_configure || current.state != RuleState::Missing)
        {
            return Err(
                "UFW rules changed during the operation; inspect again before retrying.".into(),
            );
        }
        command(&arguments(check, operation == Operation::Remove))?;
    }
    inspect(request, &command(&["status".into(), "verbose".into()])?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> FirewallRequest {
        FirewallRequest {
            backend: FirewallBackend::Ufw,
            tcp_port: 30120,
            udp_port: 30120,
        }
    }
    const STATUS: &str =
        "Status: active\nDefault: deny (incoming), allow (outgoing), disabled (routed)\n";
    #[test]
    fn recognizes_external_owned_and_implicit_rules() {
        let s = snapshot(request(),STATUS,"Added user rules (see 'ufw status' for running firewall):\nufw allow 30120/tcp\nufw allow 30120/udp comment 'fxins-fw-v1-in-udp-30120'");
        assert_eq!(s.checks[0].state, RuleState::External);
        assert_eq!(s.checks[1].state, RuleState::Owned);
        assert_eq!(s.checks[2].state, RuleState::DefaultAllowed);
        assert!(s.can_remove);
    }
    #[test]
    fn outbound_source_port_is_not_destination_port() {
        let s = snapshot(request(),"Status: active\nDefault: deny (incoming), deny (outgoing)","ufw allow out 30120/tcp\nufw allow out proto udp from any port 30120 to any comment 'fxins-fw-v1-out-udp-30120'");
        assert_eq!(s.checks[2].state, RuleState::Missing);
        assert_eq!(s.checks[3].state, RuleState::Owned);
    }
    #[test]
    fn block_and_unknown_syntax_are_not_allowed() {
        let s = snapshot(
            request(),
            STATUS,
            "ufw allow 30120/tcp\nufw deny from 192.0.2.1 to any port 30120 proto tcp",
        );
        assert_eq!(s.checks[0].state, RuleState::Blocked);
        assert!(!s.can_configure);
        assert_eq!(
            snapshot(request(), STATUS, "ufw allow in on eth0 to any port 30120").checks[0].state,
            RuleState::Unknown
        );
    }
    #[test]
    fn cannot_remove_external_modified_or_duplicate_rules() {
        for text in [
            "ufw allow 30120/tcp",
            "ufw allow 22/tcp comment 'fxins-fw-v1-in-tcp-30120'",
            "ufw allow 30120/tcp\nufw allow 30120/tcp comment 'fxins-fw-v1-in-tcp-30120'",
        ] {
            assert!(!snapshot(request(), STATUS, text).can_remove);
        }
    }
    #[test]
    fn generated_rules_round_trip_and_never_change_policy() {
        for check in request().checks() {
            let args = arguments(&check, false);
            let parsed = parse(&format!("ufw {}", args.join(" "))).unwrap();
            assert!(owned(&parsed, &check));
            let delete = arguments(&check, true);
            assert_eq!(&delete[..2], &["--force", "delete"]);
            assert!(!args.iter().any(|a| matches!(
                a.as_str(),
                "enable" | "disable" | "reset" | "default" | "prepend"
            )));
        }
    }

    #[test]
    fn inactive_ufw_only_allows_removing_owned_rules() {
        let s = snapshot(
            request(),
            "Status: inactive",
            "ufw allow 30120/tcp comment 'fxins-fw-v1-in-tcp-30120'",
        );
        assert_eq!(s.active, Some(false));
        assert!(!s.can_configure);
        assert!(s.can_remove);
        assert!(s.checks.iter().all(|c| c.state == RuleState::Unknown));
    }

    #[test]
    fn named_or_invalid_ports_fail_closed() {
        for port in ["fivem", "0", "65536", "90:10"] {
            let s = snapshot(
                request(),
                STATUS,
                &format!("ufw deny proto tcp from any to any port {port}"),
            );
            assert!(!s.can_configure);
            assert_ne!(s.checks[0].state, RuleState::DefaultAllowed);
            assert_ne!(s.checks[2].state, RuleState::DefaultAllowed);
        }
    }
}
