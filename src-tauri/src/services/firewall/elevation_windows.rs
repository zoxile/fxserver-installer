use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
    Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
    System::Threading::{GetCurrentProcess, OpenProcessToken, WaitForSingleObject},
};

#[repr(C)]
struct ShellExecuteInfo {
    size: u32,
    mask: u32,
    window: HANDLE,
    verb: *const u16,
    file: *const u16,
    parameters: *const u16,
    directory: *const u16,
    show: i32,
    instance: HANDLE,
    id_list: *mut c_void,
    class: *const u16,
    class_key: HANDLE,
    hot_key: u32,
    icon_or_monitor: HANDLE,
    process: HANDLE,
}

#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteExW(info: *mut ShellExecuteInfo) -> i32;
}
#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
    fn CoUninitialize();
}

pub(super) fn is_elevated() -> Result<bool, String> {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
        let mut length = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        );
        CloseHandle(token);
        if ok == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(elevation.TokenIsElevated != 0)
    }
}

pub(super) struct ElevatedProcess(HANDLE);
impl ElevatedProcess {
    pub(super) fn finished(&mut self) -> Result<bool, String> {
        let status = unsafe { WaitForSingleObject(self.0, 0) };
        if status == u32::MAX {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(status == WAIT_OBJECT_0)
    }
}
impl Drop for ElevatedProcess {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub(super) fn start(executable: &Path, args: &[String]) -> Result<ElevatedProcess, String> {
    let file: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // These arguments are produced internally and contain only fixed words, digits and a hex nonce.
    if args
        .iter()
        .any(|a| !a.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    {
        return Err("Invalid Windows firewall helper arguments.".into());
    }
    let parameters: Vec<u16> = args.join(" ").encode_utf16().chain(Some(0)).collect();
    let verb: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();
    unsafe {
        let initialized = CoInitializeEx(ptr::null_mut(), 0x2 | 0x4) >= 0;
        let mut info: ShellExecuteInfo = std::mem::zeroed();
        info.size = std::mem::size_of::<ShellExecuteInfo>() as u32;
        info.mask = 0x0000_0040 | 0x0000_0100 | 0x0000_0400; // Process handle, synchronous launch, no error UI.
        info.verb = verb.as_ptr();
        info.file = file.as_ptr();
        info.parameters = parameters.as_ptr();
        info.show = 0;
        let ok = ShellExecuteExW(&mut info);
        let error = std::io::Error::last_os_error();
        if initialized {
            CoUninitialize();
        }
        if ok == 0 || info.process.is_null() {
            return Err(format!(
                "Administrator approval failed or was cancelled: {error}"
            ));
        }
        Ok(ElevatedProcess(info.process))
    }
}
