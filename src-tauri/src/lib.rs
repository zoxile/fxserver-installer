mod commands;
mod models;
mod services;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(commands::fxserver::FxserverManager::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::artifact::get_windows_artifact_metadata,
            commands::artifact::get_installed_windows_artifact_info,
            commands::artifact::install_windows_artifact,
            commands::fxserver::get_fxserver_status,
            commands::fxserver::read_txdata_log,
            commands::fxserver::start_fxserver,
            commands::fxserver::stop_fxserver,
            commands::jooat::get_jooat_resolver_status,
            commands::jooat::prepare_jooat_resolver_database,
            commands::jooat::save_jooat_resolver_shard,
            commands::jooat::remove_jooat_resolver_database,
            commands::jooat::resolve_jooat_hashes,
            commands::logs::read_app_logs,
            commands::logs::append_app_log,
            commands::logs::clear_app_logs,
            commands::system::open_external_url,
            commands::mariadb::get_mariadb_status,
            commands::mariadb::install_mariadb,
            commands::mariadb::start_mariadb_service,
            commands::mariadb::stop_mariadb_service,
            commands::mariadb::restart_mariadb_service,
            commands::mariadb::execute_mariadb_query,
            commands::mariadb::save_mariadb_user,
            commands::mariadb::list_mariadb_users,
            commands::mariadb::update_mariadb_user,
            commands::mariadb::get_mariadb_user_access,
            commands::mariadb::grant_mariadb_permissions,
            commands::mariadb::delete_mariadb_user,
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
