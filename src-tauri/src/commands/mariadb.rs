use crate::{
    models::mariadb::{
        MariaDBCredentials, MariaDBInstallOptions, MariaDBQueryResult, MariaDBStatus, MariaDBUser,
        MariaDBUserAccess, MariaDBUserConfig, MariaDBUserUpdateConfig,
    },
    services::mariadb::{
        detect::detect_mariadb,
        install::install_mariadb as install_mariadb_service,
        query::execute_query,
        service::{restart_service, start_service, stop_service},
        users::{
            create_or_update_user, drop_user, get_user_access, grant_permissions, list_users,
            update_user,
        },
    },
};

#[tauri::command]
pub fn get_mariadb_status() -> MariaDBStatus {
    detect_mariadb()
}

#[tauri::command]
pub fn install_mariadb(options: MariaDBInstallOptions) -> Result<String, String> {
    install_mariadb_service(options)
}

#[tauri::command]
pub fn start_mariadb_service(service_name: Option<String>) -> Result<MariaDBStatus, String> {
    start_service(service_name)?;
    Ok(detect_mariadb())
}

#[tauri::command]
pub fn stop_mariadb_service(service_name: Option<String>) -> Result<MariaDBStatus, String> {
    stop_service(service_name)?;
    Ok(detect_mariadb())
}

#[tauri::command]
pub fn restart_mariadb_service(service_name: Option<String>) -> Result<MariaDBStatus, String> {
    restart_service(service_name)?;
    Ok(detect_mariadb())
}

#[tauri::command]
pub fn execute_mariadb_query(
    credentials: MariaDBCredentials,
    query: String,
) -> Result<MariaDBQueryResult, String> {
    execute_query(credentials, query)
}

#[tauri::command]
pub fn save_mariadb_user(
    credentials: MariaDBCredentials,
    config: MariaDBUserConfig,
) -> Result<(), String> {
    create_or_update_user(credentials, config)
}

#[tauri::command]
pub fn list_mariadb_users(credentials: MariaDBCredentials) -> Result<Vec<MariaDBUser>, String> {
    list_users(credentials)
}

#[tauri::command]
pub fn update_mariadb_user(
    credentials: MariaDBCredentials,
    config: MariaDBUserUpdateConfig,
) -> Result<(), String> {
    update_user(credentials, config)
}

#[tauri::command]
pub fn get_mariadb_user_access(
    credentials: MariaDBCredentials,
    username: String,
    host: String,
) -> Result<MariaDBUserAccess, String> {
    get_user_access(credentials, username, host)
}

#[tauri::command]
pub fn grant_mariadb_permissions(
    credentials: MariaDBCredentials,
    username: String,
    host: String,
    database: String,
    privileges: Vec<String>,
) -> Result<(), String> {
    grant_permissions(credentials, username, host, database, privileges)
}

#[tauri::command]
pub fn delete_mariadb_user(
    credentials: MariaDBCredentials,
    username: String,
    host: String,
) -> Result<(), String> {
    drop_user(credentials, username, host)
}
