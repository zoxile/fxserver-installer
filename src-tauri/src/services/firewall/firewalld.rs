use super::super::{
    Direction, FirewallBackend, FirewallCheck, FirewallRequest, FirewallSnapshot, Operation,
    RuleState,
};
use std::collections::BTreeMap;

const DESCRIPTION: &str = "FXServer Installer owned FiveM policy v1";

#[derive(Clone, Debug)]
struct Object {
    name: String,
    active: bool,
    fields: BTreeMap<String, String>,
}
impl Object {
    fn field(&self, name: &str) -> &str {
        self.fields.get(name).map(String::as_str).unwrap_or("")
    }
}

fn name_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn parse(text: &str) -> Result<Vec<Object>, String> {
    let mut objects: Vec<Object> = Vec::new();
    let mut rich = false;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        if !line.starts_with(char::is_whitespace) {
            let mut parts = line.split_whitespace();
            let name = parts.next().unwrap_or_default();
            if !name_valid(name) || objects.iter().any(|o| o.name == name) {
                return Err("Unrecognized or duplicate firewalld object name.".into());
            }
            objects.push(Object {
                name: name.into(),
                active: line.contains("(active)"),
                fields: BTreeMap::new(),
            });
            rich = false;
        } else {
            let object = objects.last_mut().ok_or("Unexpected firewalld output.")?;
            let line = line.trim();
            if rich && line.starts_with("rule ") {
                object
                    .fields
                    .entry("rich rules".into())
                    .or_default()
                    .push_str(&format!("\n{line}"));
            } else {
                let (key, value) = line
                    .split_once(':')
                    .ok_or("Unrecognized firewalld field.")?;
                if object
                    .fields
                    .insert(key.into(), value.trim().into())
                    .is_some()
                {
                    return Err("Duplicate firewalld field.".into());
                }
                rich = key == "rich rules";
            }
        }
        if objects.len() > 256 {
            return Err("Too many firewalld objects to inspect safely.".into());
        }
    }
    Ok(objects)
}

fn policy_name(c: &FirewallCheck) -> String {
    format!("fxi1{}{}{}", c.direction.short(), c.protocol.name(), c.port)
}

fn scope(c: &FirewallCheck) -> (&'static str, &'static str) {
    match c.direction {
        Direction::Inbound => ("ANY", "HOST"),
        Direction::Outbound => ("HOST", "ANY"),
    }
}

fn port_field(c: &FirewallCheck) -> &'static str {
    match c.direction {
        Direction::Inbound => "ports",
        Direction::Outbound => "source-ports",
    }
}
fn port_value(c: &FirewallCheck) -> String {
    format!("{}/{}", c.port, c.protocol.name())
}

fn owned_shape(object: &Object, check: &FirewallCheck) -> bool {
    let (ingress, egress) = scope(check);
    if object.name != policy_name(check)
        || object.field("target") != "CONTINUE"
        || object.field("priority") != "-1"
        || object.field("ingress-zones") != ingress
        || object.field("egress-zones") != egress
    {
        return false;
    }
    object.fields.iter().all(|(key, value)| match key.as_str() {
        "target" | "priority" | "ingress-zones" | "egress-zones" => true,
        "ports" | "source-ports" if key == port_field(check) => {
            value.is_empty() || value == &port_value(check)
        }
        "masquerade" | "forward" => value == "no",
        _ => value.is_empty(),
    })
}

fn includes_port(text: &str, check: &FirewallCheck) -> bool {
    text.split_whitespace().any(|value| {
        value.split_once('/').is_some_and(|(ports, protocol)| {
            protocol == check.protocol.name()
                && (ports.parse::<u16>().ok() == Some(check.port)
                    || ports.split_once('-').is_some_and(|(a, b)| {
                        a.parse::<u16>()
                            .ok()
                            .zip(b.parse::<u16>().ok())
                            .is_some_and(|(a, b)| (a..=b).contains(&check.port))
                    }))
        })
    })
}

struct Probe {
    runtime: Vec<Object>,
    permanent: Vec<Object>,
    zones: Vec<Object>,
    permanent_zones: Vec<Object>,
    descriptions: BTreeMap<String, String>,
    default_zone: String,
    direct: bool,
    panic: bool,
}

fn snapshot(request: FirewallRequest, probe: &Probe) -> FirewallSnapshot {
    let mut result = FirewallSnapshot::new(request, FirewallBackend::Firewalld);
    result.active = Some(true);
    let zones: Vec<_> = probe
        .zones
        .iter()
        .filter(|z| z.active || z.name == probe.default_zone)
        .collect();
    let block = probe.panic
        || zones
            .iter()
            .copied()
            .chain(probe.permanent_zones.iter().filter(|z| {
                z.name == probe.default_zone
                    || !z.field("interfaces").is_empty()
                    || !z.field("sources").is_empty()
                    || zones.iter().any(|active| active.name == z.name)
            }))
            .any(|z| {
                matches!(z.field("target"), "DROP" | "REJECT") || rich_blocks(z.field("rich rules"))
            })
        || probe
            .runtime
            .iter()
            .filter(|p| p.active)
            .chain(probe.permanent.iter().filter(|p| {
                !p.field("ingress-zones").is_empty() && !p.field("egress-zones").is_empty()
            }))
            .any(|p| {
                matches!(p.field("target"), "DROP" | "REJECT") || rich_blocks(p.field("rich rules"))
            });
    for check in &mut result.checks {
        let name = policy_name(check);
        let permanent = probe.permanent.iter().find(|p| p.name == name);
        let runtime = probe.runtime.iter().find(|p| p.name == name);
        let owns_permanent = permanent.is_some_and(|p| owned_shape(p, check))
            && probe
                .descriptions
                .get(&name)
                .is_some_and(|d| d == DESCRIPTION);
        let owns_runtime = owns_permanent && runtime.is_some_and(|p| owned_shape(p, check));
        let has_permanent_rule = owns_permanent
            && permanent.is_some_and(|p| p.field(port_field(check)) == port_value(check));
        let has_runtime_rule = owns_runtime
            && runtime.is_some_and(|p| p.field(port_field(check)) == port_value(check));
        let collision =
            permanent.is_some() && !owns_permanent || runtime.is_some() && !owns_runtime;
        check.owned_rule_present = !collision && (has_permanent_rule || has_runtime_rule);
        let pending = has_permanent_rule && !has_runtime_rule;
        result.reload_required |= pending;
        let external = check.direction == Direction::Inbound
            && !zones.is_empty()
            && zones.iter().all(|z| {
                z.field("target") == "ACCEPT"
                    || includes_port(z.field("ports"), check)
                    || z.field("protocols")
                        .split_whitespace()
                        .any(|p| p == check.protocol.name())
            })
            || probe.runtime.iter().filter(|p| p.active).any(|p| {
                let (ingress, egress) = scope(check);
                p.name != name
                    && p.field("ingress-zones") == ingress
                    && p.field("egress-zones") == egress
                    && (p.field("target") == "ACCEPT"
                        || includes_port(p.field(port_field(check)), check))
            });
        let unknown = probe.direct
            || zones.is_empty()
            || probe.runtime.iter().filter(|p| p.active).any(|p| {
                !p.field("rich rules").is_empty()
                    && !p
                        .field("rich rules")
                        .lines()
                        .all(|l| l.is_empty() || l.contains("icmp") && l.ends_with("accept"))
            })
            || zones.iter().any(|z| !z.field("rich rules").is_empty());
        let (state, detail) = if collision {
            (RuleState::Unknown,"An app policy name has unexpected settings or no ownership marker. It will not be changed or removed.")
        } else if block {
            (RuleState::Blocked,"A zone or active policy has a deny target or explicit blocking rule. Automatic rules will not override it.")
        } else if unknown {
            (RuleState::Unknown,"Custom rich/direct rules require administrator review; effective access cannot be safely inferred.")
        } else if has_runtime_rule && runtime.is_some_and(|p| p.active) {
            (
                RuleState::Owned,
                "An exact app-owned policy allows this local port.",
            )
        } else if external {
            (RuleState::External,"Existing firewalld zone or policy permissions cover this port and will remain untouched.")
        } else if pending {
            (RuleState::Missing,"The app rule is saved permanently but is not active. Ask the administrator to reload firewalld after reviewing runtime-only changes, then check again.")
        } else if check.direction == Direction::Outbound {
            (RuleState::DefaultAllowed,"No output restriction was found in firewalld policies; host output uses the default allow behavior.")
        } else {
            (
                RuleState::Missing,
                "No unrestricted firewalld port permission was found across active/default zones.",
            )
        };
        check.state = state;
        check.detail = detail.into();
    }
    result.can_configure = !result.reload_required
        && result
            .checks
            .iter()
            .all(|c| !matches!(c.state, RuleState::Blocked | RuleState::Unknown))
        && result.checks.iter().any(|c| c.state == RuleState::Missing);
    result.can_remove = result.checks.iter().any(|c| c.owned_rule_present);
    result.notices.push("The app never reloads firewalld or saves unrelated runtime configuration. New policies need an administrator-reviewed reload. Removing rules leaves empty app-owned policy containers, which grant no access.".into());
    result.notices.push("Custom services, rich rules, third-party nftables chains and provider firewalls may affect access. Outbound default allowance is a firewalld policy observation, not a reachability test.".into());
    result
}

fn rich_blocks(text: &str) -> bool {
    text.split_whitespace()
        .any(|w| matches!(w, "drop" | "reject") || w.starts_with("limit"))
}

fn plan(
    check: &FirewallCheck,
    operation: Operation,
    exists: bool,
    runtime_exists: bool,
) -> Vec<Vec<String>> {
    let name = policy_name(check);
    let policy = format!("--policy={name}");
    let (ingress, egress) = scope(check);
    let mut steps = Vec::new();
    if operation == Operation::Configure && !exists {
        steps.push(vec!["--permanent".into(), format!("--new-policy={name}")]);
        for option in [
            format!("--set-description={DESCRIPTION}"),
            "--set-priority=-1".into(),
            "--set-target=CONTINUE".into(),
            format!("--add-ingress-zone={ingress}"),
            format!("--add-egress-zone={egress}"),
        ] {
            steps.push(vec!["--permanent".into(), policy.clone(), option]);
        }
    }
    let verb = if operation == Operation::Remove {
        "remove"
    } else {
        "add"
    };
    let kind = if check.direction == Direction::Inbound {
        "port"
    } else {
        "source-port"
    };
    let option = format!("--{verb}-{kind}={}", port_value(check));
    steps.push(vec!["--permanent".into(), policy.clone(), option.clone()]);
    if runtime_exists {
        steps.push(vec![policy, option]);
    }
    steps
}

#[cfg(target_os = "linux")]
fn command(args: &[String]) -> Result<String, String> {
    super::super::runtime::checked(
        std::process::Command::new(super::super::runtime::system_program("firewall-cmd")?)
            .args(args),
    )
}

#[cfg(target_os = "linux")]
fn probe(request: FirewallRequest) -> Result<Probe, String> {
    let runtime = parse(&command(&["--list-all-policies".into()])?)?;
    let permanent = parse(&command(&[
        "--permanent".into(),
        "--list-all-policies".into(),
    ])?)?;
    let zones = parse(&command(&["--list-all-zones".into()])?)?;
    let permanent_zones = parse(&command(&[
        "--permanent".into(),
        "--list-all-zones".into(),
    ])?)?;
    let default_zone = command(&["--get-default-zone".into()])?;
    if !name_valid(&default_zone) {
        return Err("Invalid firewalld default zone.".into());
    }
    let mut direct = false;
    for permanent in [false, true] {
        for query in ["--get-all-rules", "--get-all-passthroughs"] {
            let mut args = vec!["--direct".into(), query.into()];
            if permanent {
                args.insert(0, "--permanent".into());
            }
            direct |= !command(&args)?.is_empty();
        }
    }
    let panic_output = super::super::runtime::run(
        std::process::Command::new(super::super::runtime::system_program("firewall-cmd")?)
            .arg("--query-panic"),
        super::super::runtime::COMMAND_TIMEOUT,
    )?;
    let panic = match panic_output.stdout.as_str() {
        "yes" if panic_output.success => true,
        "no" if !panic_output.success => false,
        _ => {
            return Err("Cannot determine firewalld panic state; refusing automatic rules.".into())
        }
    };
    let mut descriptions = BTreeMap::new();
    for c in request.checks() {
        let name = policy_name(&c);
        if permanent.iter().any(|p| p.name == name) {
            descriptions.insert(
                name.clone(),
                command(&[
                    "--permanent".into(),
                    format!("--policy={name}"),
                    "--get-description".into(),
                ])?,
            );
        }
    }
    Ok(Probe {
        runtime,
        permanent,
        zones,
        permanent_zones,
        descriptions,
        default_zone,
        direct,
        panic,
    })
}

#[cfg(target_os = "linux")]
pub(super) fn inspect(request: FirewallRequest) -> Result<FirewallSnapshot, String> {
    Ok(snapshot(request, &probe(request)?))
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
        return Err("Review blocking/unknown rules or pending policy activation before configuring firewalld.".into());
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
        let current = probe(request)?;
        let fresh = snapshot(request, &current);
        let row = fresh
            .checks
            .iter()
            .find(|c| c.id() == check.id())
            .ok_or("Firewall inspection changed unexpectedly.")?;
        if operation == Operation::Remove && !row.owned_rule_present
            || operation == Operation::Configure
                && matches!(row.state, RuleState::Unknown | RuleState::Blocked)
        {
            return Err(
                "Firewalld configuration changed during the action; inspect before retrying."
                    .into(),
            );
        }
        let name = policy_name(check);
        for args in plan(
            check,
            operation,
            current.permanent.iter().any(|p| p.name == name),
            current.runtime.iter().any(|p| p.name == name),
        ) {
            command(&args)?;
        }
    }
    inspect(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> FirewallRequest {
        FirewallRequest {
            backend: FirewallBackend::Firewalld,
            tcp_port: 30120,
            udp_port: 30120,
        }
    }
    fn fixture() -> Probe {
        Probe{runtime:vec![],permanent:vec![],permanent_zones:vec![],zones:parse("public (active)\n  target: default\n  interfaces: eth0\n  ports: 30120/tcp\n  rich rules:\n").unwrap(), descriptions:BTreeMap::new(),default_zone:"public".into(),direct:false,panic:false}
    }
    fn policy(c: &FirewallCheck) -> Object {
        let (ingress, egress) = scope(c);
        parse(&format!("{} (active)\n  target: CONTINUE\n  priority: -1\n  ingress-zones: {ingress}\n  egress-zones: {egress}\n  {}: {}\n  masquerade: no\n",policy_name(c),port_field(c),port_value(c))).unwrap().remove(0)
    }
    #[test]
    fn distinguishes_external_default_and_missing() {
        let s = snapshot(request(), &fixture());
        assert_eq!(s.checks[0].state, RuleState::External);
        assert_eq!(s.checks[1].state, RuleState::Missing);
        assert_eq!(s.checks[2].state, RuleState::DefaultAllowed);
    }
    #[test]
    fn pending_policy_is_not_reported_active() {
        let mut p = fixture();
        let c = request().checks().remove(1);
        let policy = policy(&c);
        p.descriptions
            .insert(policy.name.clone(), DESCRIPTION.into());
        p.permanent.push(policy.clone());
        let s = snapshot(request(), &p);
        assert!(s.reload_required);
        assert_eq!(s.checks[1].state, RuleState::Missing);
        assert!(s.can_remove);
        p.runtime.push(policy);
        assert_eq!(snapshot(request(), &p).checks[1].state, RuleState::Owned);
    }
    #[test]
    fn block_wins_and_modified_owned_policy_is_not_removed() {
        let mut p = fixture();
        let c = request().checks().remove(1);
        let mut policy = policy(&c);
        p.descriptions
            .insert(policy.name.clone(), DESCRIPTION.into());
        policy.fields.insert("services".into(), "ssh".into());
        p.permanent.push(policy);
        assert!(!snapshot(request(), &p).can_remove);
        p.zones[0].fields.insert(
            "rich rules".into(),
            "rule port port=30120 protocol=tcp drop".into(),
        );
        assert_eq!(snapshot(request(), &p).checks[0].state, RuleState::Blocked);
    }
    #[test]
    fn plans_do_not_reload_reset_or_touch_external_objects() {
        for c in request().checks() {
            assert!(policy_name(&c).len() <= 17);
            for op in [Operation::Configure, Operation::Remove] {
                let steps = plan(&c, op, false, false);
                assert!(steps.iter().flatten().all(|a| !matches!(
                    a.as_str(),
                    "--reload"
                        | "--complete-reload"
                        | "--runtime-to-permanent"
                        | "--panic-off"
                        | "--panic-on"
                )));
                assert!(steps.iter().all(|s| s
                    .iter()
                    .any(|a| a == &format!("--policy={}", policy_name(&c))
                        || a == &format!("--new-policy={}", policy_name(&c)))));
            }
        }
    }
    #[test]
    fn untrusted_names_and_duplicate_fields_fail_closed() {
        assert!(parse("--reload\n  ports: 80/tcp\n  ports: 443/tcp").is_err());
        assert!(parse("bad;name\n  ports: 80/tcp").is_err());
    }

    #[test]
    fn panic_and_pending_permanent_blocks_are_never_green() {
        let mut p = fixture();
        p.panic = true;
        assert!(snapshot(request(), &p)
            .checks
            .iter()
            .all(|c| c.state == RuleState::Blocked));
        p.panic = false;
        p.permanent_zones=parse("public\n  target: default\n  rich rules:\n    rule port port=30120 protocol=tcp drop\n").unwrap();
        assert_eq!(snapshot(request(), &p).checks[0].state, RuleState::Blocked);
    }

    #[test]
    fn unused_drop_zone_does_not_block_other_zones() {
        let mut p = fixture();
        p.permanent_zones = parse("drop\n  target: DROP\n  interfaces:\n  sources:\n").unwrap();
        assert_eq!(snapshot(request(), &p).checks[0].state, RuleState::External);
    }

    #[test]
    fn empty_owned_containers_do_not_grant_access() {
        let mut p = fixture();
        let c = request().checks().remove(1);
        let mut policy = policy(&c);
        policy.fields.insert("ports".into(), String::new());
        p.descriptions
            .insert(policy.name.clone(), DESCRIPTION.into());
        p.permanent.push(policy.clone());
        p.runtime.push(policy);
        let s = snapshot(request(), &p);
        assert_eq!(s.checks[1].state, RuleState::Missing);
        assert!(!s.can_remove);
        assert!(!s.reload_required);
        assert!(s.can_configure);
    }
}
