mod commands;
mod models;
mod services;

use std::{env, process::Command};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, Runtime, WindowEvent,
};

pub(crate) const ELEVATED_SCRIPT_ARG: &str = "--fxi-elevated-script";
const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_SHOW_ID: &str = "show-main-window";
const TRAY_QUIT_ID: &str = "quit-app";

pub fn run_elevated_helper_from_args() -> bool {
    let mut args = env::args().skip(1);
    let Some(first) = args.next() else {
        return false;
    };

    if first != ELEVATED_SCRIPT_ARG {
        return false;
    }

    let Some(script_path) = args.next() else {
        std::process::exit(2);
    };

    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script_path,
        ])
        .status();

    std::process::exit(status.ok().and_then(|status| status.code()).unwrap_or(1));
}

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
            commands::fxserver::get_fxserver_terminal,
            commands::fxserver::get_fxserver_rcon_password,
            commands::fxserver::list_txdata_profiles,
            commands::fxserver::read_server_config,
            commands::fxserver::read_txdata_log,
            commands::fxserver::scan_fxserver_resources,
            commands::fxserver::save_server_config,
            commands::fxserver::save_fxserver_rcon_password,
            commands::fxserver::send_fxserver_rcon_command,
            commands::fxserver::send_fxserver_command,
            commands::fxserver::start_fxserver,
            commands::fxserver::stop_fxserver,
            commands::fxserver::update_github_resource,
            commands::fxserver::clear_fxserver_rcon_password,
            commands::jooat::get_jooat_resolver_status,
            commands::jooat::prepare_jooat_resolver_database,
            commands::jooat::save_jooat_resolver_shard,
            commands::jooat::remove_jooat_resolver_database,
            commands::jooat::resolve_jooat_hashes,
            commands::logs::read_app_logs,
            commands::logs::append_app_log,
            commands::logs::clear_app_logs,
            commands::logs::read_client_logs,
            commands::system::open_external_url,
            commands::system::read_text_file,
            commands::mariadb::get_mariadb_status,
            commands::mariadb::get_mariadb_package_info,
            commands::mariadb::install_mariadb,
            commands::mariadb::uninstall_mariadb,
            commands::mariadb::update_mariadb,
            commands::mariadb::start_mariadb_service,
            commands::mariadb::stop_mariadb_service,
            commands::mariadb::restart_mariadb_service,
            commands::mariadb::execute_mariadb_query,
            commands::mariadb::validate_mariadb_credentials,
            commands::mariadb::list_mariadb_databases,
            commands::mariadb::list_mariadb_tables,
            commands::mariadb::backup_mariadb,
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
            setup_system_tray(app)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::WindowEvent {
                label,
                event: WindowEvent::CloseRequested { api, .. },
                ..
            } = event
            {
                if label == MAIN_WINDOW_LABEL {
                    api.prevent_close();
                    hide_main_window(app_handle);
                }
            }
        });
}

fn setup_system_tray<R: Runtime>(app: &mut tauri::App<R>) -> tauri::Result<()> {
    let open_item = MenuItem::with_id(
        app,
        TRAY_SHOW_ID,
        "Open FXServer Installer",
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, TRAY_QUIT_ID, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_item, &separator, &quit_item])?;

    let mut tray = TrayIconBuilder::with_id("fxserver-installer-tray")
        .menu(&menu)
        .tooltip("FXServer Installer")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_SHOW_ID => show_main_window(app),
            TRAY_QUIT_ID => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => show_main_window(tray.app_handle()),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;
    Ok(())
}

fn hide_main_window<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.hide();
    }
}

fn show_main_window<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
