//! Reuses diagnostics' bounded, link-aware scanner and literal metadata parser.
use serde::{Deserialize, Serialize};

use super::{inspect, PreflightRequest, ResourceOrigin};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyRequest {
    pub artifact_path: String,
    pub tx_data_path: String,
    pub profile: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyResource {
    pub name: String,
    pub path: String,
    pub origin: &'static str,
    pub dependencies: Vec<String>,
    pub provides: Vec<String>,
    pub groups: Vec<String>,
    pub dynamic: bool,
}

#[derive(Serialize)]
pub struct StartupCommand {
    pub command: String,
    pub target: String,
}

#[derive(Serialize)]
pub struct ScanWarning {
    pub code: String,
    pub detail: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyInventory {
    pub resources: Vec<DependencyResource>,
    pub resources_incomplete: bool,
    pub configs_incomplete: bool,
    pub startup: Vec<StartupCommand>,
    pub warnings: Vec<ScanWarning>,
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn scan_resource_dependencies(
    request: DependencyRequest,
) -> Result<DependencyInventory, String> {
    crate::commands::run_blocking(move || scan(request)).await
}

fn scan(request: DependencyRequest) -> Result<DependencyInventory, String> {
    let inspection = inspect(&PreflightRequest {
        artifact_path: request.artifact_path,
        tx_data_path: request.tx_data_path,
        profile: request.profile,
        credentials: None,
        check_ports: false,
    });
    if inspection.data_root.is_none() {
        return Err("Select an accessible txAdmin profile before inspecting dependencies.".into());
    }
    let artifact_unavailable = inspection.checks.iter().any(|check| {
        matches!(
            check.code.as_str(),
            "artifact-missing" | "artifact-conflict"
        )
    });
    // Return only dependency metadata, never config contents, credentials or commands' other arguments.
    let startup = inspection
        .executed_commands
        .iter()
        .filter_map(|words| {
            let command = words.first()?.to_ascii_lowercase();
            let target = words.get(1)?;
            (matches!(command.as_str(), "start" | "ensure" | "stop" | "restart")
                && target.len() <= 255)
                .then(|| StartupCommand {
                    command,
                    target: super::redact_known(target, &inspection.secrets),
                })
        })
        .collect();
    let warnings = inspection
        .checks
        .into_iter()
        .filter(|check| {
            matches!(
                check.severity,
                super::Severity::Warning | super::Severity::Error
            ) && matches!(
                check.category.as_str(),
                "Resources" | "Dependencies" | "Paths" | "Configuration"
            )
        })
        .map(|check| ScanWarning {
            code: check.code,
            detail: super::redact_known(&check.detail, &inspection.secrets),
        })
        .collect();
    Ok(DependencyInventory {
        resources_incomplete: inspection.resources_incomplete || artifact_unavailable,
        configs_incomplete: inspection.configs_incomplete,
        resources: inspection
            .resources
            .into_iter()
            .map(|resource| DependencyResource {
                name: resource.name,
                path: resource.path.to_string_lossy().into_owned(),
                origin: if resource.origin == ResourceOrigin::Artifact {
                    "artifact"
                } else {
                    "data"
                },
                dependencies: resource.manifest.dependencies,
                provides: resource.manifest.provides,
                groups: resource.groups,
                dynamic: resource.manifest.dynamic,
            })
            .collect(),
        startup,
        warnings,
    })
}

#[cfg(test)]
#[path = "resource_dependencies_tests.rs"]
mod tests;
