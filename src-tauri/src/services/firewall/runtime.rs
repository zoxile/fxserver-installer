use crate::process::CommandNoWindowExt;
use std::{
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: usize = 1024 * 1024;
pub(super) const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);

thread_local! { static DEADLINE: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) }; }

pub(super) fn within_action_deadline<T>(work: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            DEADLINE.set(None);
        }
    }
    DEADLINE.set(Some(Instant::now() + Duration::from_secs(120)));
    let _reset = Reset;
    work()
}

pub(super) struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

fn drain(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<Result<String, String>> {
    thread::spawn(move || {
        let mut result = Vec::new();
        let mut bytes = [0; 8192];
        let mut overflow = false;
        loop {
            let count = pipe.read(&mut bytes).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            let keep = count.min(OUTPUT_LIMIT.saturating_sub(result.len()));
            overflow |= keep != count;
            result.extend_from_slice(&bytes[..keep]);
        }
        if overflow {
            return Err("Firewall command output exceeded 1 MiB.".into());
        }
        String::from_utf8(result)
            .map(|s| s.trim_start_matches('\u{feff}').trim().to_owned())
            .map_err(|_| "Firewall command returned invalid UTF-8.".into())
    })
}

pub(super) fn run(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    let timeout = DEADLINE.get().map_or(timeout, |deadline| {
        timeout.min(deadline.saturating_duration_since(Instant::now()))
    });
    if timeout.is_zero() {
        return Err("Firewall action deadline exceeded; inspect before retrying.".into());
    }
    command
        .no_window()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        command
            .env_clear()
            .env("LC_ALL", "C")
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .current_dir("/");
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Cannot start firewall command: {e}"))?;
    #[cfg(windows)]
    let _job = match WindowsJob::attach(&child) {
        Ok(job) => job,
        Err(error) => {
            terminate(&mut child);
            return Err(error);
        }
    };
    let stdout = drain(child.stdout.take().ok_or("Missing firewall output pipe.")?);
    let stderr = drain(child.stderr.take().ok_or("Missing firewall error pipe.")?);
    let start = Instant::now();
    let status = loop {
        if stdout.is_finished() && stderr.is_finished() {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(e) => {
                    terminate(&mut child);
                    return Err(format!("Cannot wait for firewall command: {e}"));
                }
            }
        }
        if start.elapsed() >= timeout {
            terminate(&mut child);
            return Err(
                "Firewall command timed out. Changes may be partial; inspect before retrying."
                    .into(),
            );
        }
        thread::sleep(Duration::from_millis(20));
    };
    Ok(Output {
        success: status.success(),
        stdout: stdout
            .join()
            .map_err(|_| "Firewall output worker failed.")??,
        stderr: stderr
            .join()
            .map_err(|_| "Firewall error worker failed.")??,
    })
}

fn terminate(child: &mut Child) {
    #[cfg(target_os = "linux")]
    if let Ok(pid) = i32::try_from(child.id()) {
        // The group leader is not reaped until its output readers complete.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub(super) fn checked(command: &mut Command) -> Result<String, String> {
    let output = run(command, COMMAND_TIMEOUT)?;
    if output.success {
        Ok(output.stdout)
    } else {
        Err(format!(
            "Firewall command failed: {}. Inspect before retrying; changes may be partial.",
            if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            }
        ))
    }
}

#[cfg(target_os = "linux")]
pub(super) fn system_program(name: &str) -> Result<PathBuf, String> {
    use std::os::unix::fs::MetadataExt;
    if !matches!(name, "ufw" | "firewall-cmd" | "sudo" | "pkexec") {
        return Err("Unsupported firewall executable.".into());
    }
    for root in ["/usr/sbin", "/usr/bin", "/sbin", "/bin"] {
        let Ok(path) = std::path::Path::new(root).join(name).canonicalize() else {
            continue;
        };
        if path
            .metadata()
            .is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0)
            && path.ancestors().all(|p| {
                p.metadata()
                    .is_ok_and(|m| m.uid() == 0 && m.mode() & 0o022 == 0)
            })
        {
            return Ok(path);
        }
    }
    Err(format!(
        "A trusted root-owned {name} executable is required."
    ))
}

#[cfg(windows)]
pub(super) fn powershell() -> Result<PathBuf, String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    let mut buffer = vec![0u16; 32768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err("Cannot locate the Windows system directory.".into());
    }
    use std::os::windows::ffi::OsStringExt;
    Ok(
        PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length]))
            .join("WindowsPowerShell/v1.0/powershell.exe"),
    )
}

#[cfg(windows)]
struct WindowsJob(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl WindowsJob {
    fn attach(child: &Child) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let job = Self(handle);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(handle, child.as_raw_handle()) == 0
            {
                return Err(format!(
                    "Cannot bound firewall child processes: {}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(job)
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
