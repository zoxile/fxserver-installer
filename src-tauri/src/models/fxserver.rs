use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FxserverLaunchRequest {
    pub artifact_path: String,
    #[serde(default)]
    pub environment: Vec<FxserverEnvironmentVariable>,
    #[serde(default)]
    pub server_profile: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FxserverEnvironmentVariable {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FxserverLaunchResult {
    pub pid: u32,
    pub artifact_path: String,
    pub started_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FxserverStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub artifact_path: Option<String>,
    pub started_at: Option<String>,
    pub uptime_seconds: Option<u64>,
    pub resources: Option<FxserverResources>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FxserverResources {
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub total_memory_bytes: u64,
    pub memory_percent: f64,
    pub thread_count: u32,
    pub handle_count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxDataLogRequest {
    pub data_path: String,
    #[serde(default)]
    pub profile: Option<String>,
    pub log_name: String,
    #[serde(default)]
    pub max_lines: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TxDataLogResult {
    pub path: String,
    pub log_name: String,
    pub content: String,
    pub line_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TxDataProfilesResult {
    pub data_path: String,
    pub profiles: Vec<String>,
    pub has_root_logs: bool,
}
