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

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FxserverRconConfig {
    pub host: String,
    pub port: u16,
    pub password: String,
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

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FxserverTerminalEntry {
    pub id: u64,
    pub stream: String,
    pub line: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FxserverTerminalResult {
    pub entries: Vec<FxserverTerminalEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FxserverCommandRequest {
    pub command: String,
    pub rcon: FxserverRconConfig,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfigRequest {
    pub tx_data_path: String,
    pub profile: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfigFile {
    pub name: String,
    pub path: String,
    pub content: String,
    pub size: u64,
    pub modified: Option<u64>,
    pub has_rcon_password: bool,
    pub has_rconlog: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfigResult {
    pub tx_data_path: String,
    pub profile: String,
    pub profile_config_path: String,
    pub data_path: String,
    pub files: Vec<ServerConfigFile>,
    pub rcon_password_found: bool,
    pub rcon_password_file: Option<String>,
    pub rcon_password_line: Option<usize>,
    pub rconlog_found: bool,
    pub rconlog_line: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveServerConfigRequest {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceScanRequest {
    pub tx_data_path: String,
    pub profile: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceScanResult {
    pub tx_data_path: String,
    pub profile: String,
    pub data_path: String,
    pub resource_root: String,
    pub resources: Vec<FxserverResourceInfo>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FxserverResourceInfo {
    pub name: String,
    pub path: String,
    pub manifest_path: String,
    pub manifest_name: String,
    pub version: Option<String>,
    pub repository: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUpdateRequest {
    pub resource_path: String,
    pub repository: String,
    pub branch: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUpdateResult {
    pub resource_path: String,
    pub repository: String,
    pub branch: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStatesRequest {
    pub rcon: FxserverRconConfig,
    #[serde(default)]
    pub resource_names: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStatesResult {
    pub raw_output: String,
    pub states: Vec<ResourceRuntimeState>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRuntimeState {
    pub name: String,
    pub state: String,
}
