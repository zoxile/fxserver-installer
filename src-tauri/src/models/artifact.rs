use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactIssue {
    pub artifact: String,
    pub reason: String,
}

#[derive(Debug, Deserialize, Serialize)]
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
    pub has_fxserver_executable: bool,
    pub detection_source: String,
}
