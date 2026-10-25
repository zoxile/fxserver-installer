use crate::FXSERVER_WATCHDOG_ARG;
#[cfg(windows)]
use crate::{commands, process::CommandNoWindowExt, ELEVATED_SCRIPT_ARG};
use std::env;
#[cfg(windows)]
use std::process::Command;
#[cfg(windows)]
use std::{thread, time::Duration};

#[cfg(windows)]
pub fn run_elevated_helper_from_args() -> bool {
    if crate::services::firewall::run_firewall_helper_from_args() {
        return true;
    }
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

#[cfg(not(windows))]
pub fn run_elevated_helper_from_args() -> bool {
    crate::services::firewall::run_firewall_helper_from_args()
}

pub fn run_fxserver_watchdog_from_args() -> bool {
    #[cfg(target_os = "linux")]
    if crate::commands::fxserver::run_supervisor_from_args() {
        return true;
    }
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
