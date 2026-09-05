use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

use crate::{
    models::mariadb::{
        MariaDBBackupOptions, MariaDBBackupResult, MariaDBCredentials, MariaDBInstallOptions,
        MariaDBPackageInfo, MariaDBQueryResult, MariaDBStatus, MariaDBUser, MariaDBUserAccess,
        MariaDBUserConfig, MariaDBUserUpdateConfig,
    },
    services::mariadb::{
        backup::create_backup,
        detect::detect_mariadb,
        install::{
            get_package_info, install_mariadb as install_mariadb_service,
            uninstall_mariadb as uninstall_mariadb_service,
            update_mariadb as update_mariadb_service,
        },
        query::{execute_query, list_databases, list_tables, validate_connection},
        service::{restart_service, start_service, stop_service},
        users::{
            create_or_update_user, drop_user, get_user_access, grant_permissions, list_users,
            update_user,
        },
    },
};

static INSTALLER: Mutex<()> = Mutex::new(());

async fn run_installer(
    app: AppHandle,
    task: impl FnOnce(&dyn Fn(&str)) -> Result<String, String> + Send + 'static,
) -> Result<String, String> {
    super::run_blocking(move || {
        let _guard = INSTALLER
            .try_lock()
            .map_err(|_| "A MariaDB installation action is already running.".to_string())?;
        task(&|stage| {
            let _ = app.emit("mariadb-progress", stage);
        })
    })
    .await
}

#[tauri::command]
pub async fn get_mariadb_status() -> Result<MariaDBStatus, String> {
    super::run_blocking(|| Ok(detect_mariadb())).await
}

#[tauri::command]
pub async fn install_mariadb(
    app: AppHandle,
    options: MariaDBInstallOptions,
) -> Result<String, String> {
    run_installer(app, move |report| install_mariadb_service(options, report)).await
}

#[tauri::command]
pub async fn get_mariadb_package_info() -> Result<MariaDBPackageInfo, String> {
    tauri::async_runtime::spawn_blocking(get_package_info)
        .await
        .map_err(|error| format!("MariaDB package info task failed: {error}"))
}

#[tauri::command]
pub async fn uninstall_mariadb(app: AppHandle) -> Result<String, String> {
    run_installer(app, |report| uninstall_mariadb_service(report)).await
}

#[tauri::command]
pub async fn update_mariadb(app: AppHandle) -> Result<String, String> {
    run_installer(app, |report| update_mariadb_service(report)).await
}

#[tauri::command]
pub async fn start_mariadb_service(service_name: Option<String>) -> Result<MariaDBStatus, String> {
    super::run_blocking(move || {
        start_service(service_name)?;
        Ok(detect_mariadb())
    })
    .await
}

#[tauri::command]
pub async fn stop_mariadb_service(service_name: Option<String>) -> Result<MariaDBStatus, String> {
    super::run_blocking(move || {
        stop_service(service_name)?;
        Ok(detect_mariadb())
    })
    .await
}

#[tauri::command]
pub async fn restart_mariadb_service(
    service_name: Option<String>,
) -> Result<MariaDBStatus, String> {
    super::run_blocking(move || {
        restart_service(service_name)?;
        Ok(detect_mariadb())
    })
    .await
}

#[tauri::command]
pub async fn execute_mariadb_query(
    credentials: MariaDBCredentials,
    query: String,
) -> Result<MariaDBQueryResult, String> {
    super::run_blocking(move || execute_query(credentials, query)).await
}

#[tauri::command]
pub async fn validate_mariadb_credentials(credentials: MariaDBCredentials) -> Result<(), String> {
    super::run_blocking(move || validate_connection(credentials)).await
}

#[tauri::command]
pub async fn list_mariadb_databases(
    credentials: MariaDBCredentials,
) -> Result<Vec<String>, String> {
    super::run_blocking(move || list_databases(credentials)).await
}

#[tauri::command]
pub async fn list_mariadb_tables(
    credentials: MariaDBCredentials,
    database: String,
) -> Result<Vec<String>, String> {
    super::run_blocking(move || list_tables(credentials, database)).await
}

#[tauri::command]
pub async fn backup_mariadb(
    credentials: MariaDBCredentials,
    options: MariaDBBackupOptions,
) -> Result<MariaDBBackupResult, String> {
    super::run_blocking(move || create_backup(credentials, options)).await
}

#[tauri::command]
pub async fn save_mariadb_user(
    credentials: MariaDBCredentials,
    config: MariaDBUserConfig,
) -> Result<(), String> {
    super::run_blocking(move || create_or_update_user(credentials, config)).await
}

#[tauri::command]
pub async fn list_mariadb_users(
    credentials: MariaDBCredentials,
) -> Result<Vec<MariaDBUser>, String> {
    super::run_blocking(move || list_users(credentials)).await
}

#[tauri::command]
pub async fn update_mariadb_user(
    credentials: MariaDBCredentials,
    config: MariaDBUserUpdateConfig,
) -> Result<(), String> {
    super::run_blocking(move || update_user(credentials, config)).await
}

#[tauri::command]
pub async fn get_mariadb_user_access(
    credentials: MariaDBCredentials,
    username: String,
    host: String,
) -> Result<MariaDBUserAccess, String> {
    super::run_blocking(move || get_user_access(credentials, username, host)).await
}

#[tauri::command]
pub async fn grant_mariadb_permissions(
    credentials: MariaDBCredentials,
    username: String,
    host: String,
    database: String,
    privileges: Vec<String>,
) -> Result<(), String> {
    super::run_blocking(move || {
        grant_permissions(credentials, username, host, database, privileges)
    })
    .await
}

#[tauri::command]
pub async fn delete_mariadb_user(
    credentials: MariaDBCredentials,
    username: String,
    host: String,
) -> Result<(), String> {
    super::run_blocking(move || drop_user(credentials, username, host)).await
}
