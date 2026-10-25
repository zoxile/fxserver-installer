#[cfg(target_os = "linux")]
use super::{FirewallBackend, FirewallRequest, FirewallSnapshot, Operation};

#[path = "firewalld.rs"]
mod firewalld;
#[path = "ufw.rs"]
mod ufw;

#[cfg(target_os = "linux")]
pub(super) fn inspect(request: FirewallRequest) -> Result<FirewallSnapshot, String> {
    if request.backend == FirewallBackend::Windows {
        return Err("Windows firewall is unavailable on Linux.".into());
    }
    if super::elevation::is_elevated()? {
        return inspect_root(request);
    }
    let ufw = super::runtime::system_program("ufw").is_ok();
    let firewalld = super::runtime::system_program("firewall-cmd").is_ok();
    let backend = match (request.backend, ufw, firewalld) {
        (FirewallBackend::Auto, true, false) => FirewallBackend::Ufw,
        (FirewallBackend::Auto, false, true) => FirewallBackend::Firewalld,
        (FirewallBackend::Auto, true, true) => FirewallBackend::Auto,
        (FirewallBackend::Ufw, true, _) => FirewallBackend::Ufw,
        (FirewallBackend::Firewalld, _, true) => FirewallBackend::Firewalld,
        _ => FirewallBackend::Unsupported,
    };
    let mut result = FirewallSnapshot::new(request, backend);
    result.requires_elevation = backend != FirewallBackend::Unsupported;
    result.notices.push(if result.requires_elevation {
        "Check with administrator approval to inspect Linux firewall rules. No rule changes are made by inspection."
    } else { UNSUPPORTED }.into());
    Ok(result)
}

#[cfg(target_os = "linux")]
const UNSUPPORTED: &str = "No active supported firewall manager was found. Custom nftables/iptables rules are not modified. Ask the administrator to allow the selected TCP/UDP server ports with the existing firewall manager; the app will not install, enable, disable or reset a firewall.";

#[cfg(target_os = "linux")]
fn inspect_root(request: FirewallRequest) -> Result<FirewallSnapshot, String> {
    use std::process::Command;
    let ufw_path = super::runtime::system_program("ufw").ok();
    let fw_path = super::runtime::system_program("firewall-cmd").ok();
    let ufw_status = ufw_path
        .as_ref()
        .map(|path| super::runtime::checked(Command::new(path).args(["status", "verbose"])))
        .transpose()?;
    let ufw_active = ufw_status
        .as_ref()
        .is_some_and(|s| s.lines().any(|l| l.trim() == "Status: active"));
    let fw_active = fw_path
        .as_ref()
        .map(|path| {
            super::runtime::run(
                Command::new(path).arg("--state"),
                super::runtime::COMMAND_TIMEOUT,
            )
            .map(|out| out.success && out.stdout == "running")
        })
        .transpose()?
        .unwrap_or(false);
    if ufw_active && fw_active {
        return Err("Both UFW and firewalld are active. Resolve the overlapping firewall managers before using automatic rules.".into());
    }
    let backend = if fw_active {
        FirewallBackend::Firewalld
    } else if ufw_active
        || ufw_path.is_some()
            && matches!(
                request.backend,
                FirewallBackend::Auto | FirewallBackend::Ufw
            )
    {
        FirewallBackend::Ufw
    } else {
        FirewallBackend::Unsupported
    };
    if backend != FirewallBackend::Unsupported
        && request.backend != FirewallBackend::Auto
        && request.backend != backend
    {
        return Err("The selected backend is not the active firewall manager. Check again using automatic detection.".into());
    }
    match backend {
        FirewallBackend::Ufw => ufw::inspect(request, ufw_status.as_deref().unwrap_or_default()),
        FirewallBackend::Firewalld => firewalld::inspect(request),
        _ => {
            let mut result = FirewallSnapshot::new(request, backend);
            result.active = Some(false);
            result.notices.push(UNSUPPORTED.into());
            Ok(result)
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn execute(
    request: FirewallRequest,
    operation: Operation,
) -> Result<FirewallSnapshot, String> {
    let before = inspect_root(request)?;
    if operation == Operation::Inspect {
        return Ok(before);
    }
    if before.backend == FirewallBackend::Unsupported {
        return Err(UNSUPPORTED.into());
    }
    log::info!(
        "Firewall {:?}: {:?} TCP {} UDP {}",
        operation,
        before.backend,
        request.tcp_port,
        request.udp_port
    );
    match before.backend {
        FirewallBackend::Ufw => ufw::execute(request, operation, before),
        FirewallBackend::Firewalld => firewalld::execute(request, operation, before),
        _ => Err(UNSUPPORTED.into()),
    }
}
