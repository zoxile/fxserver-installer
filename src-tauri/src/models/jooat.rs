use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JooatShardManifest {
    pub prefix: String,
    pub path: String,
    pub hashes: Option<u64>,
    pub bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JooatResolverManifest {
    pub version: String,
    pub source: Option<String>,
    pub generated_at: Option<String>,
    pub total_hashes: Option<u64>,
    pub total_names: Option<u64>,
    pub size_bytes: Option<u64>,
    pub shards: Vec<JooatShardManifest>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JooatResolverStatus {
    pub available: bool,
    pub database_dir: String,
    pub manifest: Option<JooatResolverManifest>,
    pub installed_shards: usize,
    pub expected_shards: usize,
    pub size_bytes: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JooatResolvedHash {
    pub query: String,
    pub value: Option<u32>,
    pub hex: Option<String>,
    pub unsigned: Option<String>,
    pub signed: Option<String>,
    pub matches: Vec<String>,
    pub error: Option<String>,
}
