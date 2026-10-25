use crate::services::firewall::{self, FirewallRequest, FirewallSnapshot};

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn inspect_fivem_firewall(
    request: FirewallRequest,
    elevated: bool,
) -> Result<FirewallSnapshot, String> {
    super::run_blocking(move || {
        if elevated {
            firewall::inspect_elevated(request)
        } else {
            firewall::inspect(request)
        }
    })
    .await
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn configure_fivem_firewall(
    request: FirewallRequest,
) -> Result<FirewallSnapshot, String> {
    super::run_blocking(move || firewall::configure(request)).await
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn remove_fivem_firewall(request: FirewallRequest) -> Result<FirewallSnapshot, String> {
    super::run_blocking(move || firewall::remove(request)).await
}
