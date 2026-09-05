mod commands;
mod models;
mod process;
mod services;

use crate::process::CommandNoWindowExt;
use std::{
    env,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicBool, AtomicIsize, Ordering},
    thread,
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, Runtime, WindowEvent,
};

pub(crate) const ELEVATED_SCRIPT_ARG: &str = "--fxi-elevated-script";
pub(crate) const FXSERVER_WATCHDOG_ARG: &str = "--fxi-watch-fxserver";
const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "fxserver-installer-tray";
const TRAY_SHOW_ID: &str = "show-main-window";
const TRAY_QUIT_ID: &str = "quit-app";
#[cfg(target_os = "windows")]
static SINGLE_INSTANCE_MUTEX: AtomicIsize = AtomicIsize::new(0);
#[cfg(target_os = "windows")]
static SINGLE_INSTANCE_LOCK_FILE: AtomicIsize = AtomicIsize::new(0);
static SHUTDOWN_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

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
        .no_window()
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

pub fn run_fxserver_watchdog_from_args() -> bool {
    let mut args = env::args().skip(1);
    let Some(first) = args.next() else {
        return false;
    };

    if first != FXSERVER_WATCHDOG_ARG {
        return false;
    }

    let Some(parent_pid) = args.next().and_then(|value| value.parse::<u32>().ok()) else {
        std::process::exit(2);
    };
    let Some(fxserver_pid) = args.next().and_then(|value| value.parse::<u32>().ok()) else {
        std::process::exit(2);
    };

    watch_fxserver_parent(parent_pid, fxserver_pid);
    std::process::exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    exit_if_secondary_instance();

    tauri::Builder::default()
        .manage(commands::fxserver::FxserverManager::default())
        .manage(commands::backup_manager::BackupManager::default())
        .manage(commands::health::HealthMonitor::default())
        .manage(commands::live_bridge::LiveBridge::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::artifact::get_windows_artifact_metadata,
            commands::artifact::get_windows_artifact_catalog,
            commands::artifact::get_installed_windows_artifact_info,
            commands::artifact::install_windows_artifact,
            commands::backup_manager::get_backup_manager,
            commands::backup_manager::save_backup_schedule,
            commands::backup_manager::remove_backup_schedule,
            commands::backup_manager::run_scheduled_backup_now,
            commands::backup_manager::preview_backup_restore,
            commands::backup_manager::restore_backup_snapshot,
            commands::backup_manager::preview_backup_restore_test,
            commands::backup_manager::test_backup_restore,
            commands::backup_manager::cleanup_backup_restore_test,
            commands::config_history::read_config_history_file,
            commands::config_history::list_config_history,
            commands::config_history::read_config_history_version,
            commands::config_history::restore_config_history_version,
            commands::database_browser::get_database_browser_metadata,
            commands::database_browser::get_database_browser_rows,
            commands::database_browser::export_database_browser_csv,
            commands::database_browser::preview_database_browser_change,
            commands::database_browser::apply_database_browser_change,
            commands::workspace_clone::list_workspace_clone_choices,
            commands::workspace_clone::preview_workspace_clone,
            commands::workspace_clone::discard_workspace_clone_preview,
            commands::workspace_clone::execute_workspace_clone,
            commands::live_bridge::get_live_bridge_installation,
            commands::live_bridge::preview_live_bridge_change,
            commands::live_bridge::apply_live_bridge_change,
            commands::live_bridge::configure_live_bridge,
            commands::live_bridge::get_live_bridge_status,
            commands::live_bridge::send_live_bridge_action,
            commands::diagnostics::run_fxserver_preflight,
            commands::diagnostics::guided::preview_diagnostic_config_patch,
            commands::diagnostics::guided::apply_diagnostic_config_patch,
            commands::diagnostics::preview_diagnostic_export,
            commands::diagnostics::export_diagnostic_zip,
            commands::health::get_health_status,
            commands::health::configure_health,
            commands::health::clear_health_events,
            commands::health::prepare_workspace_switch,
            commands::health::initialize_health_workspace,
            commands::resource_updates::preview_resource_update,
            commands::resource_updates::apply_resource_update,
            commands::resource_updates::discard_resource_preview,
            commands::resource_updates::list_resource_snapshots,
            commands::resource_updates::rollback_resource_update,
            commands::resource_updates::delete_resource_snapshot,
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
            commands::fxserver::restart_fxserver,
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
            app.state::<commands::fxserver::FxserverManager>()
                .start_incident_events(app.handle().clone());
            app.state::<commands::live_bridge::LiveBridge>()
                .start(app.handle().clone());
            app.state::<commands::backup_manager::BackupManager>()
                .start(app.handle().clone());
            app.state::<commands::health::HealthMonitor>()
                .start(
                    app.state::<commands::fxserver::FxserverManager>()
                        .inner()
                        .clone(),
                    app.handle().clone(),
                )
                .map_err(std::io::Error::other)?;
            spawn_activation_signal_watcher(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            tauri::RunEvent::WindowEvent {
                label,
                event: WindowEvent::CloseRequested { api, .. },
                ..
            } if label == MAIN_WINDOW_LABEL => {
                api.prevent_close();
                hide_main_window(app_handle);
            }
            tauri::RunEvent::ExitRequested { api, .. }
                if !SHUTDOWN_IN_PROGRESS.load(Ordering::SeqCst) =>
            {
                api.prevent_exit();
                request_app_quit(app_handle);
            }
            _ => {}
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

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("FXServer Installer")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_SHOW_ID => show_main_window(app),
            TRAY_QUIT_ID => request_app_quit(app),
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

fn request_app_quit<R: Runtime>(app: &tauri::AppHandle<R>) {
    if SHUTDOWN_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return;
    }

    commands::begin_shutdown();
    app.state::<commands::live_bridge::LiveBridge>().stop();
    app.state::<commands::fxserver::FxserverManager>()
        .begin_shutdown();

    hide_main_window(app);
    hide_tray_icon(app);

    let app = app.clone();
    thread::spawn(move || {
        let health = app.state::<commands::health::HealthMonitor>();
        let backups = app.state::<commands::backup_manager::BackupManager>();
        health.stop(&app.state::<commands::fxserver::FxserverManager>());
        backups.stop();
        health.wait_stopped();
        stop_managed_fxserver(&app);
        commands::wait_for_background_work();
        if let Err(error) = backups.stop_and_wait() {
            eprintln!("Backup shutdown failed: {error}");
        }
        app.exit(0);
    });
}

fn hide_tray_icon<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(false);
    }

    let _ = app.remove_tray_by_id(TRAY_ID);
}

fn stop_managed_fxserver<R: Runtime>(app: &tauri::AppHandle<R>) {
    let manager = app.state::<commands::fxserver::FxserverManager>();
    if let Err(error) = manager.stop_running_process() {
        eprintln!("Failed to stop FXServer during app shutdown: {error}");
    }
}

#[cfg(target_os = "windows")]
fn exit_if_secondary_instance() {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
    };

    if existing_instance_process_id().is_some() {
        request_existing_instance_activation();
        std::process::exit(0);
    }

    let name = wide_null("Local\\com.fxserver.installer.single-instance");
    let handle = unsafe { CreateMutexW(ptr::null(), 1, name.as_ptr()) };
    if handle.is_null() {
        return;
    }

    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            CloseHandle(handle);
        }

        request_existing_instance_activation();
        std::process::exit(0);
    }

    if !acquire_single_instance_file_lock() {
        request_existing_instance_activation();
        std::process::exit(0);
    }

    SINGLE_INSTANCE_MUTEX.store(handle as isize, Ordering::Relaxed);
}

#[cfg(not(target_os = "windows"))]
fn exit_if_secondary_instance() {}

#[cfg(target_os = "windows")]
fn acquire_single_instance_file_lock() -> bool {
    use std::{fs, ptr};
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_ALWAYS},
    };

    let path = app_local_data_file("single-instance.lock");

    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return true;
        }
    }

    let path = wide_null(&path.to_string_lossy());
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            ptr::null(),
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        return false;
    }

    SINGLE_INSTANCE_LOCK_FILE.store(handle as isize, Ordering::Relaxed);
    true
}

#[cfg(target_os = "windows")]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn app_local_data_file(file_name: &str) -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("FXServer Installer")
        .join(file_name)
}

#[cfg(target_os = "windows")]
fn activation_signal_path() -> PathBuf {
    app_local_data_file("activate.signal")
}

#[cfg(target_os = "windows")]
fn request_existing_instance_activation() {
    use std::{fs, time::SystemTime};

    let path = activation_signal_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let _ = fs::write(
        path,
        format!("{:?}:{}", SystemTime::now(), std::process::id()),
    );
}

#[cfg(target_os = "windows")]
fn spawn_activation_signal_watcher<R: Runtime>(app: tauri::AppHandle<R>) {
    use std::fs;

    let path = activation_signal_path();
    let mut last_signal = fs::read_to_string(&path).ok();

    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(200));

        let signal = fs::read_to_string(&path).ok();
        if signal.is_some() && signal != last_signal && !SHUTDOWN_IN_PROGRESS.load(Ordering::SeqCst)
        {
            last_signal = signal;
            let app = app.clone();
            let _ = app
                .clone()
                .run_on_main_thread(move || show_main_window(&app));
        }
    });
}

#[cfg(not(target_os = "windows"))]
fn spawn_activation_signal_watcher<R: Runtime>(_: tauri::AppHandle<R>) {}

#[cfg(target_os = "windows")]
fn existing_instance_process_id() -> Option<u32> {
    use std::mem;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
    };

    let current_pid = std::process::id();
    let current_exe = env::current_exe().ok().and_then(normalize_path);
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }

    let mut entry: PROCESSENTRY32W = unsafe { mem::zeroed() };
    entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut found = None;
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while has_entry {
        let pid = entry.th32ProcessID;
        if pid != current_pid {
            if let Some(path) = process_image_path(pid).and_then(normalize_path) {
                if Some(path) == current_exe {
                    found = Some(pid);
                    break;
                }
            }
        }
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }

    unsafe {
        CloseHandle(snapshot);
    }

    found
}

#[cfg(target_os = "windows")]
fn process_image_path(pid: u32) -> Option<PathBuf> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }

    let mut buffer = vec![0_u16; 32768];
    let mut size = buffer.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut size) };
    unsafe {
        CloseHandle(process);
    }

    if ok == 0 || size == 0 {
        return None;
    }

    buffer.truncate(size as usize);
    Some(PathBuf::from(String::from_utf16_lossy(&buffer)))
}

#[cfg(target_os = "windows")]
fn normalize_path(path: PathBuf) -> Option<PathBuf> {
    path.canonicalize()
        .ok()
        .map(|path| PathBuf::from(path.to_string_lossy().to_ascii_lowercase()))
}

fn watch_fxserver_parent(parent_pid: u32, fxserver_pid: u32) {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, WAIT_OBJECT_0},
            System::Threading::{OpenProcess, WaitForSingleObject},
        };

        const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;

        let parent = unsafe { OpenProcess(SYNCHRONIZE_ACCESS, 0, parent_pid) };
        let fxserver = unsafe { OpenProcess(SYNCHRONIZE_ACCESS, 0, fxserver_pid) };

        if fxserver.is_null() {
            if !parent.is_null() {
                unsafe {
                    CloseHandle(parent);
                }
            }
            return;
        }

        if parent.is_null() {
            let _ = commands::fxserver::terminate_process_tree(fxserver_pid);
            unsafe {
                CloseHandle(fxserver);
            }
            return;
        }

        loop {
            if unsafe { WaitForSingleObject(fxserver, 0) } == WAIT_OBJECT_0 {
                break;
            }

            if unsafe { WaitForSingleObject(parent, 1000) } == WAIT_OBJECT_0 {
                thread::sleep(Duration::from_secs(2));
                if unsafe { WaitForSingleObject(fxserver, 0) } != WAIT_OBJECT_0 {
                    let _ = commands::fxserver::terminate_process_tree(fxserver_pid);
                }
                break;
            }
        }

        unsafe {
            CloseHandle(parent);
            CloseHandle(fxserver);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (parent_pid, fxserver_pid);
    }
}
