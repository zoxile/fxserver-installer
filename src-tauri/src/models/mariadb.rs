use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MariaDBStatus {
    pub installed: bool,
    pub running: bool,
    pub version: Option<String>,
    pub service_name: Option<String>,
    pub service_display_name: Option<String>,
    pub install_path: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MariaDBCredentials {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MariaDBInstallOptions {
    pub root_password: String,
    pub service_name: String,
    pub port: u16,
    pub install_dir: Option<String>,
    pub data_dir: Option<String>,
    pub allow_remote_root_access: bool,
    pub create_anonymous_user: bool,
    pub skip_networking: bool,
    pub optimize_for_transactions: bool,
    pub use_utf8: bool,
    pub page_size: Option<String>,
    pub buffer_pool_size: Option<String>,
    pub install_heidi_sql: bool,
    pub install_development_files: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MariaDBUserConfig {
    pub username: String,
    pub password: String,
    pub host: String,
    pub database: Option<String>,
    pub privileges: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MariaDBUserUpdateConfig {
    pub username: String,
    pub host: String,
    pub password: Option<String>,
    pub database: Option<String>,
    pub privileges: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MariaDBUser {
    pub username: String,
    pub host: String,
    pub plugin: Option<String>,
    pub password_expired: Option<String>,
    pub locked: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MariaDBUserPrivilege {
    pub database: String,
    pub table: Option<String>,
    pub privilege: String,
    pub grantable: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MariaDBUserAccess {
    pub username: String,
    pub host: String,
    pub grants: Vec<String>,
    pub schema_privileges: Vec<MariaDBUserPrivilege>,
    pub table_privileges: Vec<MariaDBUserPrivilege>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MariaDBQueryResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}
