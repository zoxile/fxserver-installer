use super::{json, Action, Context, Field, Outcome, Submission};
use crate::{
    commands::{diagnostics::firewall_ports::get_fivem_firewall_ports, firewall},
    models::fxserver::ServerConfigRequest,
    services::firewall::{FirewallBackend, FirewallRequest},
};

pub(crate) fn backend(value: &str) -> Result<FirewallBackend, String> {
    match value {
        "auto" => Ok(FirewallBackend::Auto),
        "windows" => Ok(FirewallBackend::Windows),
        "ufw" => Ok(FirewallBackend::Ufw),
        "firewalld" => Ok(FirewallBackend::Firewalld),
        _ => Err("Choose auto, windows, ufw or firewalld.".into()),
    }
}

fn fields() -> Vec<Field> {
    vec![
        Field::text("tcp_port", "TCP game port", "30120"),
        Field::text("udp_port", "UDP game port", "30120"),
        Field::text("backend", "Firewall", "auto").choice(&["auto", "windows", "ufw", "firewalld"]),
    ]
}

pub(super) fn list(context: &Context) -> Vec<Action> {
    let mut check_fields = fields();
    check_fields.push(
        Field::text("elevated", "Allow administrator inspection", "no").choice(&["no", "yes"]),
    );
    vec![
        Action { id: "firewall.ports", group: "Firewall", title: "Read profile game ports",
            description: "Read static game endpoints from executed cfg files. No firewall changes.",
            fields: vec![Field::text("tx_data", "txData folder", &context.settings.tx_data_path), Field::text("profile", "Profile", &context.settings.profile)], confirmation: None },
        Action { id: "firewall.check", group: "Firewall", title: "Check FiveM firewall",
            description: "Inspect inbound/outbound TCP and UDP. This does not prove Internet reachability. On Linux, exit the TUI and run sudo -v first if administrator inspection is required.",
            fields: check_fields, confirmation: None },
        Action { id: "firewall.setup", group: "Firewall", title: "Set up FiveM firewall rules",
            description: "Add missing game-port rules. Existing rules and policies are preserved. On Linux, run sudo -v before opening the TUI.",
            fields: fields(), confirmation: Some("Allow FiveM traffic on these game ports? Approve the operating system permission prompt. Router and provider firewalls are not changed.") },
        Action { id: "firewall.remove", group: "Firewall", title: "Remove app firewall rules",
            description: "Remove only exact app-owned game-port rules. External rules and policies remain unchanged.",
            fields: fields(), confirmation: Some("Remove the app's firewall rules for these game ports? Players may lose access if no other rules allow traffic.") },
    ]
}

pub(super) async fn execute(_: &Context, form: &Submission) -> Result<Outcome, String> {
    if form.id == "firewall.ports" {
        return json(
            &get_fivem_firewall_ports(ServerConfigRequest {
                tx_data_path: form.required("tx_data")?.into(),
                profile: form.required("profile")?.into(),
            })
            .await?,
        );
    }
    let request = FirewallRequest {
        tcp_port: form.number("tcp_port")?,
        udp_port: form.number("udp_port")?,
        backend: backend(form.required("backend")?)?,
    };
    let result = match form.id.as_str() {
        "firewall.check" => {
            firewall::inspect_fivem_firewall(request, form.get("elevated") == "yes").await?
        }
        "firewall.setup" => firewall::configure_fivem_firewall(request).await?,
        "firewall.remove" => firewall::remove_fivem_firewall(request).await?,
        _ => return Err("Unknown firewall action.".into()),
    };
    json(&result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_selection_does_not_accept_programs_or_shell_arguments() {
        for name in ["auto", "windows", "ufw", "firewalld"] {
            assert!(backend(name).is_ok());
        }
        for name in ["nft", "ufw disable", ";whoami", "../ufw", ""] {
            assert!(backend(name).is_err());
        }
    }
}
