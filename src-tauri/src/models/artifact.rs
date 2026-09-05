use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactIssue {
    pub artifact: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactMetadata {
    pub recommended_artifact: String,
    pub windows_download_link: String,
    #[serde(default)]
    pub linux_download_link: Option<String>,
    #[serde(default)]
    pub broken_artifacts: Vec<ArtifactIssue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactInstallRequest {
    pub version: String,
    pub url: String,
    pub destination: String,
    #[serde(default)]
    pub acknowledge_risk: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactBuild {
    pub version: String,
    pub download_url: String,
    pub health: String,
    pub issues: Vec<ArtifactIssue>,
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactCatalog {
    pub builds: Vec<ArtifactBuild>,
    pub fetched_at: u64,
    pub metadata_fetched_at: Option<u64>,
    pub stale: bool,
    pub warning: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactInstallResult {
    pub version: String,
    pub destination: String,
    pub marker_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledArtifactInfo {
    pub installed: bool,
    pub version: Option<String>,
    pub destination: String,
    pub marker_path: String,
    pub citizen_server_impl_path: Option<String>,
    pub file_version: Option<String>,
    pub product_version: Option<String>,
    pub has_fxserver_executable: bool,
    pub detection_source: String,
}
