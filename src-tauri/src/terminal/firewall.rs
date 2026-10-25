use crate::{commands::firewall, services::firewall::FirewallRequest};
use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub(super) enum FirewallAction {
    /// Inspect game-port rules without modifying them.
    Check {
        #[command(flatten)]
        ports: Ports,
        /// Approve administrator access if ordinary inspection is restricted.
        #[arg(long)]
        elevated: bool,
    },
    /// Add missing managed rules; never disable a firewall or replace its policy.
    Setup {
        #[command(flatten)]
        ports: Ports,
        #[arg(long)]
        yes: bool,
    },
    /// Remove only this app's exact game-port rules, not existing external rules.
    Remove {
        #[command(flatten)]
        ports: Ports,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Args)]
pub(super) struct Ports {
    #[arg(long, default_value_t = 30120, value_parser = clap::value_parser!(u16).range(1..))]
    tcp_port: u16,
    #[arg(long, default_value_t = 30120, value_parser = clap::value_parser!(u16).range(1..))]
    udp_port: u16,
    #[arg(long, default_value = "auto", value_parser = ["auto", "windows", "ufw", "firewalld"])]
    backend: String,
}

impl Ports {
    fn request(self) -> Result<FirewallRequest, String> {
        Ok(FirewallRequest {
            tcp_port: self.tcp_port,
            udp_port: self.udp_port,
            backend: super::actions::firewall::backend(&self.backend)?,
        })
    }
}

pub(super) async fn run(action: FirewallAction) -> Result<(), String> {
    let snapshot = match action {
        FirewallAction::Check { ports, elevated } => {
            firewall::inspect_fivem_firewall(ports.request()?, elevated).await?
        }
        FirewallAction::Setup { ports, yes } => {
            if !yes {
                return Err("Check these game ports first, then pass --yes to allow FiveM traffic. No changes were made.".into());
            }
            firewall::configure_fivem_firewall(ports.request()?).await?
        }
        FirewallAction::Remove { ports, yes } => {
            if !yes {
                return Err("Pass --yes to remove the app's game-port rules. Players may lose access. No changes were made.".into());
            }
            firewall::remove_fivem_firewall(ports.request()?).await?
        }
    };
    super::print_json(&snapshot)
}

#[cfg(test)]
mod tests {
    use crate::terminal::Cli;
    use clap::Parser;

    #[test]
    fn firewall_cli_rejects_zero_overflow_and_unknown_backends() {
        for args in [
            vec!["fxins", "firewall", "check", "--tcp-port", "0"],
            vec!["fxins", "firewall", "check", "--udp-port", "65536"],
            vec!["fxins", "firewall", "setup", "--backend", "bash"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
        assert!(Cli::try_parse_from([
            "fxins",
            "firewall",
            "check",
            "--tcp-port",
            "30121",
            "--elevated"
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["fxins", "firewall", "remove", "--yes"]).is_ok());
    }

    #[tokio::test]
    async fn mutations_require_explicit_confirmation_before_any_system_access() {
        let ports = || super::Ports {
            tcp_port: 30120,
            udp_port: 30120,
            backend: "auto".into(),
        };
        assert!(super::run(super::FirewallAction::Setup {
            ports: ports(),
            yes: false
        })
        .await
        .unwrap_err()
        .contains("No changes"));
        assert!(super::run(super::FirewallAction::Remove {
            ports: ports(),
            yes: false
        })
        .await
        .unwrap_err()
        .contains("No changes"));
    }
}
