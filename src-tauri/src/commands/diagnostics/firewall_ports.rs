use super::{read_config, Inspection, Severity};
use crate::models::fxserver::ServerConfigRequest;
use serde::Serialize;
use std::{
    collections::{BTreeSet, HashSet},
    net::SocketAddr,
    path::Path,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallPorts {
    pub tcp_ports: Vec<u16>,
    pub udp_ports: Vec<u16>,
    pub warnings: Vec<String>,
}

#[tauri::command]
pub async fn get_fivem_firewall_ports(
    request: ServerConfigRequest,
) -> Result<FirewallPorts, String> {
    crate::commands::run_blocking(move || {
        let (_, _, _, root) = crate::commands::fxserver::resolve_profile_data_path(
            request.tx_data_path,
            request.profile,
        )?;
        inspect_ports(&root)
    })
    .await
}

fn inspect_ports(root: &Path) -> Result<FirewallPorts, String> {
    let mut inspection = Inspection::default();
    read_config(
        &root.join("server.cfg"),
        root,
        &mut HashSet::new(),
        &mut inspection,
        true,
    );
    if inspection.configs.is_empty() {
        return Err(
            "Could not read server.cfg. Select a valid profile or enter the game ports manually."
                .into(),
        );
    }
    let mut tcp = BTreeSet::new();
    let mut udp = BTreeSet::new();
    let mut warnings = BTreeSet::new();
    if inspection.configs_incomplete
        || inspection
            .checks
            .iter()
            .any(|item| item.severity == Severity::Error)
    {
        warnings.insert("Some config includes could not be resolved. Confirm the ports before changing firewall rules.".to_owned());
    }
    for words in &inspection.executed_commands {
        let target = match words.first().map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("endpoint_add_tcp") => &mut tcp,
            Some("endpoint_add_udp") => &mut udp,
            _ => continue,
        };
        let Some(address) = words
            .get(1)
            .and_then(|value| value.parse::<SocketAddr>().ok())
        else {
            warnings.insert(
                "A game endpoint is dynamic or invalid. Its port was not guessed.".to_owned(),
            );
            continue;
        };
        if address.port() == 0 {
            warnings.insert("A game endpoint uses port zero. Set a fixed game port before configuring the firewall.".to_owned());
            continue;
        }
        if address.ip().is_loopback() {
            warnings.insert("A game endpoint is bound to loopback. Firewall rules alone will not let remote players connect.".to_owned());
        }
        target.insert(address.port());
    }
    if tcp.is_empty() || udp.is_empty() {
        warnings.insert("No static TCP or UDP game port was found for one or both protocols. Confirm the manually entered ports.".to_owned());
    }
    if tcp.len() > 1 || udp.len() > 1 {
        warnings.insert("Multiple game ports were found. Check and configure each required TCP/UDP pair separately.".to_owned());
    }
    Ok(FirewallPorts {
        tcp_ports: tcp.into_iter().collect(),
        udp_ports: udp.into_iter().collect(),
        warnings: warnings.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::diagnostics::tests::Fixture;
    use std::fs;

    #[test]
    fn reads_only_executed_cfg_endpoints_and_never_returns_secrets() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("data/server.cfg"), "# endpoint_add_tcp \"0.0.0.0:2222\"\nexec network.cfg\nrcon_password private-value\nset txAdminPort 40120\n").unwrap();
        fs::write(fixture.root.join("data/network.cfg"), "endpoint_add_tcp \"0.0.0.0:30122\"\nendpoint_add_udp \"[::]:30123\"\nendpoint_add_udp \"0.0.0.0:30123\"\n").unwrap();
        fs::write(
            fixture.root.join("data/unused.cfg"),
            "endpoint_add_tcp \"0.0.0.0:3333\"",
        )
        .unwrap();
        let result = inspect_ports(&fixture.root.join("data").canonicalize().unwrap()).unwrap();
        assert_eq!(result.tcp_ports, [30122]);
        assert_eq!(result.udp_ports, [30123]);
        assert!(result.warnings.is_empty());
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("private-value"));
    }

    #[test]
    fn unresolved_dynamic_and_loopback_endpoints_require_review() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("data/server.cfg"), "exec @unknown/network.cfg\nendpoint_add_tcp \"127.0.0.1:30120\"\nendpoint_add_udp \"${SERVER_ENDPOINT}\"\nendpoint_add_udp \"0.0.0.0:0\"\n").unwrap();
        let result = inspect_ports(&fixture.root.join("data").canonicalize().unwrap()).unwrap();
        assert_eq!(result.tcp_ports, [30120]);
        assert!(result.udp_ports.is_empty());
        assert!(result.warnings.iter().any(|v| v.contains("loopback")));
        assert!(result.warnings.iter().any(|v| v.contains("includes")));
        assert!(result.warnings.iter().any(|v| v.contains("dynamic")));
    }

    #[test]
    fn unreadable_root_config_does_not_fall_back_to_guessed_ports() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.root.join("data/server.cfg")).unwrap();
        assert!(inspect_ports(&fixture.root.join("data").canonicalize().unwrap()).is_err());
    }
}
