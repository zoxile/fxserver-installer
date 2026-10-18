//! A separate supervisor owns the server group and artifact lease. Its private
//! socket closes when the manager dies; the unreaped leader pins the group ID
//! until cleanup finishes. Deliberately detached descendants require a cgroup.
use super::{FxserverResources, ResourceSample};
use std::{
    collections::HashSet,
    fs,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

const SUPERVISOR_ARG: &str = "--fxserver-linux-supervisor";
const START_TIMEOUT: Duration = Duration::from_secs(10);
static SUPERVISOR_STOP: AtomicBool = AtomicBool::new(false);

pub(super) struct PendingSupervisor {
    manager: UnixStream,
    inherited: UnixStream,
}

pub(super) fn launch_command(launcher: &Path) -> Result<(Command, PendingSupervisor), String> {
    let (manager, inherited) = UnixStream::pair().map_err(|e| e.to_string())?;
    let fd = inherited.as_raw_fd();
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    #[cfg(not(test))]
    command
        .arg(SUPERVISOR_ARG)
        .arg(fd.to_string())
        .arg(launcher);
    #[cfg(test)]
    command
        .args([
            "--exact",
            "commands::fxserver::linux::tests::supervisor_entry",
            "--nocapture",
        ])
        .env("FXI_TEST_SUPERVISOR_FD", fd.to_string())
        .env("FXI_TEST_SUPERVISOR_LAUNCHER", launcher);
    // Only async-signal-safe syscalls run between fork and exec.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 || libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok((command, PendingSupervisor { manager, inherited }))
}

pub(super) fn set_profile(command: &mut Command, profile: &str) {
    #[cfg(not(test))]
    command.args(["+set", "serverProfile", profile]);
    #[cfg(test)]
    command.env("FXI_TEST_SUPERVISOR_PROFILE", profile);
}

impl PendingSupervisor {
    pub(super) fn connect(self, child: &mut Child) -> Result<Supervisor, String> {
        let Self {
            mut manager,
            inherited,
        } = self;
        drop(inherited);
        let result = (|| {
            manager
                .set_read_timeout(Some(START_TIMEOUT))
                .map_err(|e| e.to_string())?;
            manager
                .set_write_timeout(Some(Duration::from_millis(250)))
                .map_err(|e| e.to_string())?;
            let mut header = [0; 1];
            manager
                .read_exact(&mut header)
                .map_err(|e| format!("FXServer supervisor did not start: {e}"))?;
            match header[0] {
                b'R' => {
                    let mut pid = [0; 4];
                    manager.read_exact(&mut pid).map_err(|e| e.to_string())?;
                    let pid = u32::from_le_bytes(pid);
                    if pid <= 1 {
                        return Err("Invalid FXServer supervisor response.".into());
                    }
                    Ok(pid)
                }
                b'E' => {
                    let mut size = [0; 2];
                    manager.read_exact(&mut size).map_err(|e| e.to_string())?;
                    let size = usize::from(u16::from_le_bytes(size));
                    if size > 1024 {
                        return Err("Invalid FXServer supervisor error.".into());
                    }
                    let mut message = vec![0; size];
                    manager
                        .read_exact(&mut message)
                        .map_err(|e| e.to_string())?;
                    Err(String::from_utf8_lossy(&message).into_owned())
                }
                _ => Err("Invalid FXServer supervisor response.".into()),
            }
        })();
        match result {
            Ok(pid) => Ok(Supervisor {
                control: manager,
                server_pid: pid,
                cleanup_confirmed: AtomicBool::new(false),
            }),
            Err(error) => {
                // EOF requests cleanup even if startup completed after our timeout.
                drop(manager);
                let _ = super::wait_for_child_exit(
                    child,
                    super::GRACEFUL_STOP_TIMEOUT + super::FORCE_STOP_WAIT_TIMEOUT,
                );
                Err(error)
            }
        }
    }
}

pub(super) struct Supervisor {
    control: UnixStream,
    pub(super) server_pid: u32,
    cleanup_confirmed: AtomicBool,
}

impl Supervisor {
    pub(super) fn graceful_stop(&self, _: Option<&mut ChildStdin>) -> Result<(), String> {
        self.send(b'Q')
    }

    fn send(&self, byte: u8) -> Result<(), String> {
        let mut socket = &self.control;
        socket
            .write_all(&[byte])
            .map_err(|e| format!("FXServer supervisor is unavailable: {e}"))
    }

    pub(super) fn is_running(&self, child: &mut Child) -> Result<bool, String> {
        if child.try_wait().map_err(|e| e.to_string())?.is_none() {
            return Ok(true);
        }
        if !self.cleanup_confirmed.load(Ordering::Acquire) {
            let mut message = 0u8;
            let received = unsafe {
                libc::recv(
                    self.control.as_raw_fd(),
                    &mut message as *mut _ as *mut _,
                    1,
                    libc::MSG_DONTWAIT,
                )
            };
            if received == 1 && message == b'C' {
                self.cleanup_confirmed.store(true, Ordering::Release);
            }
        }
        if self.cleanup_confirmed.load(Ordering::Acquire) {
            Ok(false)
        } else {
            Err("The FXServer supervisor exited without confirming cleanup. The server may still be running; automatic recovery and operations requiring a stopped server are blocked.".into())
        }
    }

    pub(super) fn wait_for_exit(
        &self,
        child: &mut Child,
        timeout: Duration,
    ) -> Result<bool, String> {
        let deadline = Instant::now() + timeout;
        while self.is_running(child)? {
            if Instant::now() >= deadline {
                return Ok(false);
            }
            thread::sleep(Duration::from_millis(50));
        }
        Ok(true)
    }

    pub(super) fn force_stop(&self, child: &mut Child, timeout: Duration) -> Result<(), String> {
        if !self.is_running(child)? {
            return Ok(());
        }
        // Never kill the supervisor: it must keep the lease until cleanup finishes.
        let _ = self.send(b'K');
        if self.wait_for_exit(child, timeout)? {
            Ok(())
        } else {
            Err("FXServer cleanup is still running. Its supervisor retains the artifact lock; retry Stop.".into())
        }
    }
}

pub(crate) fn run_supervisor_from_args() -> bool {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(SUPERVISOR_ARG)) {
        return false;
    }
    let fd = args.next().and_then(|s| s.to_str()?.parse::<i32>().ok());
    let launcher = args.next();
    let (Some(fd), Some(launcher)) = (fd, launcher) else {
        std::process::exit(2);
    };
    let code = supervisor_main(fd, Path::new(&launcher), args.collect());
    std::process::exit(code);
}

extern "C" fn supervisor_signal(_: libc::c_int) {
    SUPERVISOR_STOP.store(true, Ordering::Release);
}

fn supervisor_socket(fd: i32) -> Result<UnixStream, String> {
    if fd <= 2 {
        return Err("Missing private supervisor socket.".into());
    }
    let mut peer: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of_val(&peer) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut peer as *mut _ as *mut _,
            &mut size,
        )
    } != 0
        || peer.pid != unsafe { libc::getppid() }
        || peer.uid != unsafe { libc::geteuid() }
        || unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0
    {
        return Err("Invalid private supervisor socket.".into());
    }
    // The descriptor is validated, exclusively inherited, and closed on server exec.
    let socket = unsafe { UnixStream::from_raw_fd(fd) };
    socket
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|e| e.to_string())?;
    socket
        .set_write_timeout(Some(Duration::from_millis(250)))
        .map_err(|e| e.to_string())?;
    Ok(socket)
}

fn supervisor_main(fd: i32, launcher: &Path, args: Vec<std::ffi::OsString>) -> i32 {
    let Ok(mut socket) = supervisor_socket(fd) else {
        return 2;
    };
    match supervise(&mut socket, launcher, args) {
        Ok(code) => code,
        Err(error) => {
            let message = error.as_bytes();
            let message = &message[..message.len().min(1024)];
            let _ = socket.write_all(b"E");
            let _ = socket.write_all(&(message.len() as u16).to_le_bytes());
            let _ = socket.write_all(message);
            1
        }
    }
}

fn supervise(
    socket: &mut UnixStream,
    launcher: &Path,
    args: Vec<std::ffi::OsString>,
) -> Result<i32, String> {
    let artifact = std::env::current_dir().map_err(|e| e.to_string())?;
    let _lease = crate::platform::ServerLease::acquire(&artifact)?;
    if super::server_exe::require_server_executable(&artifact)? != launcher {
        return Err("FXServer launcher does not match the locked artifact folder.".into());
    }
    for signal in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = supervisor_signal as *const () as usize;
        action.sa_flags = libc::SA_RESTART;
        if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
    }
    let mut child = Command::new("/bin/bash")
        .arg(launcher)
        .args(args)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Failed to start FXServer: {e}"))?;
    let group = match OwnedProcessGroup::new(&child) {
        Ok(group) => group,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let mut stop_at = None;
    let mut force = false;
    let mut stopping = false;
    let mut connected = socket
        .write_all(b"R")
        .and_then(|_| socket.write_all(&child.id().to_le_bytes()))
        .is_ok();
    loop {
        if group.leader_exited().unwrap_or(true) || force {
            if group
                .force_stop(&mut child, super::FORCE_STOP_WAIT_TIMEOUT)
                .is_ok()
            {
                let _ = socket.write_all(b"C");
                return Ok(child
                    .try_wait()
                    .ok()
                    .flatten()
                    .and_then(|s| s.code())
                    .unwrap_or(1));
            }
            // An uninterruptible child must not release the lease or its pinned PID.
            force = true;
            thread::sleep(Duration::from_millis(100));
            continue;
        }
        let mut command = [0];
        if connected {
            match socket.read(&mut command) {
                Ok(1) if command[0] == b'K' => force = true,
                Ok(1) if command[0] == b'Q' => stopping = true,
                Ok(_) => connected = false,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => connected = false,
            }
        } else {
            thread::sleep(Duration::from_millis(50));
        }
        if (!connected || stopping || SUPERVISOR_STOP.load(Ordering::Acquire)) && stop_at.is_none()
        {
            let _ = group.graceful_stop(child.stdin.as_mut());
            child.stdin.take();
            stop_at = Some(Instant::now() + super::GRACEFUL_STOP_TIMEOUT);
        }
        if stop_at.is_some_and(|deadline| Instant::now() >= deadline) {
            force = true;
        }
    }
}

pub(super) struct OwnedProcessGroup {
    pid: u32,
    start_ticks: u64,
    retired: AtomicBool,
}

impl OwnedProcessGroup {
    // Only called for our Child after Command::process_group(0) successfully spawned.
    pub(super) fn new(child: &Child) -> Result<Self, String> {
        let pid = child.id();
        let identity = stat(pid)
            .filter(|identity| pid > 1 && identity.group == pid)
            .ok_or("FXServer child does not own the expected process group.")?;
        let group = Self {
            pid,
            start_ticks: identity.start_ticks,
            retired: AtomicBool::new(false),
        };
        group.leader_exited()?;
        Ok(group)
    }

    fn leader_exited(&self) -> Result<bool, String> {
        if self.retired.load(Ordering::Acquire) {
            return Ok(true);
        }
        loop {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.pid,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == 0 {
                return Ok(unsafe { info.si_pid() } != 0);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(format!(
                    "Cannot verify ownership of FXServer child: {error}"
                ));
            }
        }
    }

    fn signal(&self, signal: i32) -> Result<(), String> {
        if self.retired.load(Ordering::Acquire) {
            return Ok(());
        }
        // waitid also rejects externally reaped/non-child PIDs. Never signal those.
        self.leader_exited()?;
        if !stat(self.pid).is_some_and(|identity| {
            identity.group == self.pid && identity.start_ticks == self.start_ticks
        }) {
            return Err("FXServer process-group identity changed; refusing to signal it.".into());
        }
        let pid = i32::try_from(self.pid)
            .ok()
            .filter(|pid| *pid > 1)
            .ok_or("Invalid owned FXServer process group.")?;
        if unsafe { libc::kill(-pid, signal) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        Err(format!("Failed to signal owned FXServer group: {error}"))
    }

    pub(super) fn graceful_stop(&self, stdin: Option<&mut ChildStdin>) -> Result<(), String> {
        use std::io::Write;
        if let Some(stdin) = stdin {
            let fd = stdin.as_raw_fd();
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            // A full/unread pipe must not hang the lifecycle worker indefinitely.
            if flags >= 0
                && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0
                && stdin.write_all(b"quit\n").is_ok()
            {
                return Ok(());
            }
        }
        self.signal(libc::SIGTERM)
    }

    #[cfg(test)]
    pub(super) fn wait_for_exit(
        &self,
        child: &mut Child,
        timeout: Duration,
    ) -> Result<bool, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.leader_exited()? {
                self.force_stop(child, super::FORCE_STOP_WAIT_TIMEOUT)?;
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub(super) fn force_stop(&self, child: &mut Child, timeout: Duration) -> Result<(), String> {
        if self.retired.load(Ordering::Acquire) {
            return Ok(());
        }
        self.signal(libc::SIGKILL)?;
        let deadline = Instant::now() + timeout;
        loop {
            // Only shutdown scans all of /proc, to catch orphaned group members too.
            if self.leader_exited()? && !group_has_live_members(self.pid)? {
                child
                    .try_wait()
                    .map_err(|error| format!("Failed to reap FXServer: {error}"))?
                    .ok_or("FXServer exit status is unavailable.")?;
                self.retired.store(true, Ordering::Release);
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("FXServer process group has not exited. Ownership and the artifact lock are retained; retry Stop.".into());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}

#[derive(Debug)]
struct ProcStat {
    state: u8,
    group: u32,
    ticks: u64,
    threads: u32,
    start_ticks: u64,
    rss_pages: u64,
}

fn parse_stat(value: &str) -> Option<ProcStat> {
    // comm may include spaces and ')' characters. Fields after its final ')' are fixed.
    let fields: Vec<_> = value
        .get(value.rfind(')')? + 1..)?
        .split_whitespace()
        .take(22)
        .collect();
    Some(ProcStat {
        state: *fields.first()?.as_bytes().first()?,
        group: fields.get(2)?.parse().ok()?,
        ticks: fields
            .get(11)?
            .parse::<u64>()
            .ok()?
            .saturating_add(fields.get(12)?.parse().ok()?),
        threads: fields.get(17)?.parse().ok()?,
        start_ticks: fields.get(19)?.parse().ok()?,
        rss_pages: fields.get(21)?.parse::<i64>().ok()?.max(0) as u64,
    })
}

fn stat(pid: u32) -> Option<ProcStat> {
    parse_stat(&fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

fn group_has_live_members(group: u32) -> Result<bool, String> {
    let entries =
        fs::read_dir("/proc").map_err(|error| format!("Cannot inspect FXServer group: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("Cannot inspect FXServer group: {error}"))?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if stat(pid)
            .is_some_and(|value| value.group == group && !matches!(value.state, b'Z' | b'X'))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

struct SystemInfo {
    ticks_per_second: f64,
    page_size: u64,
    processors: f64,
}

fn system_info() -> &'static SystemInfo {
    static INFO: OnceLock<SystemInfo> = OnceLock::new();
    INFO.get_or_init(|| SystemInfo {
        ticks_per_second: unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64,
        page_size: unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64,
        processors: thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1) as f64,
    })
}

fn total_memory() -> Option<u64> {
    fs::read_to_string("/proc/meminfo")
        .ok()?
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next()? == "MemTotal:")
                .then(|| fields.next()?.parse::<u64>().ok()?.checked_mul(1024))?
        })
}

pub(super) fn read_resources(
    pid: u32,
    previous: &mut Option<ResourceSample>,
) -> Option<FxserverResources> {
    let root = stat(pid)?;
    if root.group != pid || matches!(root.state, b'Z' | b'X') {
        return None;
    }
    let info = system_info();
    let mut pending = vec![pid];
    let mut seen = HashSet::new();
    let (mut ticks, mut memory_bytes, mut thread_count, mut fd_count) = (0u64, 0u64, 0u32, 0u32);
    // Traverse just the owned tree, including children forked by non-main threads.
    while let Some(current) = pending.pop() {
        if !seen.insert(current) {
            continue;
        }
        let Some(value) = stat(current).filter(|value| value.group == pid) else {
            continue;
        };
        ticks = ticks.saturating_add(value.ticks);
        memory_bytes = memory_bytes.saturating_add(value.rss_pages.saturating_mul(info.page_size));
        thread_count = thread_count.saturating_add(value.threads);
        if let Ok(fds) = fs::read_dir(format!("/proc/{current}/fd")) {
            fd_count = fd_count
                .saturating_add(fds.filter_map(Result::ok).count().min(u32::MAX as usize) as u32);
        }
        if let Ok(tasks) = fs::read_dir(format!("/proc/{current}/task")) {
            for task in tasks.filter_map(Result::ok) {
                if let Ok(children) = fs::read_to_string(task.path().join("children")) {
                    pending.extend(
                        children
                            .split_whitespace()
                            .filter_map(|id| id.parse::<u32>().ok()),
                    );
                }
            }
        }
    }
    if stat(pid)?.start_ticks != root.start_ticks {
        return None;
    }
    let sample = ResourceSample {
        cpu_seconds: ticks as f64 / info.ticks_per_second,
        sampled_at: Instant::now(),
        start_ticks: root.start_ticks,
    };
    let cpu_percent = previous
        .filter(|old| old.start_ticks == sample.start_ticks)
        .map(|old| {
            let elapsed = sample
                .sampled_at
                .duration_since(old.sampled_at)
                .as_secs_f64();
            let delta = (sample.cpu_seconds - old.cpu_seconds).max(0.0);
            if elapsed > 0.0 {
                (delta / elapsed * 100.0 / info.processors).clamp(0.0, 100.0)
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);
    *previous = Some(sample);
    let total_memory_bytes = total_memory().unwrap_or(0);
    Some(FxserverResources {
        cpu_percent: (cpu_percent * 100.0).round() / 100.0,
        memory_bytes,
        total_memory_bytes,
        memory_percent: if total_memory_bytes > 0 {
            (memory_bytes as f64 / total_memory_bytes as f64 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        },
        thread_count,
        handle_count: fd_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        path::PathBuf,
        process::Stdio,
    };

    #[test]
    fn supervisor_entry() {
        let Ok(fd) = std::env::var("FXI_TEST_SUPERVISOR_FD") else {
            return;
        };
        let launcher = std::env::var_os("FXI_TEST_SUPERVISOR_LAUNCHER").unwrap();
        let args = std::env::var_os("FXI_TEST_SUPERVISOR_PROFILE")
            .map(|profile| vec!["+set".into(), "serverProfile".into(), profile])
            .unwrap_or_default();
        std::process::exit(supervisor_main(
            fd.parse().unwrap(),
            Path::new(&launcher),
            args,
        ));
    }

    #[test]
    fn manager_entry() {
        let Some(root) = std::env::var_os("FXI_TEST_MANAGER_ARTIFACT") else {
            return;
        };
        let root = PathBuf::from(root);
        let manager = super::super::FxserverManager::default();
        let result = manager
            .start(super::super::FxserverLaunchRequest {
                artifact_path: root.to_string_lossy().into(),
                environment: vec![],
                server_profile: None,
            })
            .unwrap();
        fs::write(root.join("manager-ready"), result.pid.to_string()).unwrap();
        thread::sleep(Duration::from_secs(30));
        manager.stop().unwrap();
    }

    struct ManagedFixture {
        root: PathBuf,
        manager: Child,
    }

    impl ManagedFixture {
        fn new(script: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "fxi-supervisor-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(root.join("alpine/opt/cfx-server")).unwrap();
            fs::write(root.join("alpine/opt/cfx-server/FXServer"), "fixture").unwrap();
            fs::write(root.join("run.sh"), script).unwrap();
            let manager = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "commands::fxserver::linux::tests::manager_entry",
                    "--nocapture",
                ])
                .env("FXI_TEST_MANAGER_ARTIFACT", &root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            let fixture = Self { root, manager };
            fixture.wait_file("manager-ready");
            fixture
        }

        fn wait_file(&self, name: &str) -> String {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Ok(value) = fs::read_to_string(self.root.join(name)) {
                    if !value.is_empty() {
                        return value.trim().to_string();
                    }
                }
                assert!(Instant::now() < deadline, "timed out waiting for {name}");
                thread::sleep(Duration::from_millis(20));
            }
        }

        fn kill_manager(&mut self) {
            self.manager.kill().unwrap();
            self.manager.wait().unwrap();
        }

        fn wait_cleanup(&self) -> bool {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if let Ok(lease) = crate::platform::ServerLease::acquire(&self.root) {
                    drop(lease);
                    return true;
                }
                thread::sleep(Duration::from_millis(20));
            }
            false
        }
    }

    impl Drop for ManagedFixture {
        fn drop(&mut self) {
            let _ = self.manager.kill();
            let _ = self.manager.wait();
            if self.wait_cleanup() {
                let _ = fs::remove_dir_all(&self.root);
            }
        }
    }

    #[test]
    fn manager_death_requests_quit_before_releasing_artifact_lease() {
        let mut fixture = ManagedFixture::new("#!/bin/bash\necho $$ > server-pid\nread -r answer\nprintf '%s\\n' \"$answer\" > stopped\n");
        let pid: u32 = fixture.wait_file("server-pid").parse().unwrap();
        assert!(crate::platform::ServerLease::acquire(&fixture.root).is_err());
        for entry in fs::read_dir(format!("/proc/{pid}/fd")).unwrap().flatten() {
            if let Ok(target) = fs::read_link(entry.path()) {
                assert!(
                    !target.to_string_lossy().starts_with("socket:"),
                    "server inherited a supervisor control socket"
                );
            }
        }
        fixture.kill_manager();
        assert_eq!(fixture.wait_file("stopped"), "quit");
        assert!(fixture.wait_cleanup());
        assert!(!group_has_live_members(pid).unwrap());
    }

    #[test]
    fn manager_sigkill_cleans_stubborn_group_without_touching_unrelated_process() {
        let mut unrelated = Fixture::new("echo ready; exec sleep 30");
        unrelated.line();
        let mut fixture = ManagedFixture::new("#!/bin/bash\ntrap '' TERM\necho $$ > server-pid\nsleep 30 &\necho $! > descendant\nread -r answer\nprintf '%s\\n' \"$answer\" > stopped\nwait\n");
        let pid: u32 = fixture.wait_file("server-pid").parse().unwrap();
        let descendant: u32 = fixture.wait_file("descendant").parse().unwrap();
        let started = Instant::now();
        fixture.kill_manager();
        assert_eq!(fixture.wait_file("stopped"), "quit");
        assert!(
            crate::platform::ServerLease::acquire(&fixture.root).is_err(),
            "lease must cover graceful shutdown"
        );
        assert!(fixture.wait_cleanup());
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(!group_has_live_members(pid).unwrap());
        assert!(stat(descendant).is_none_or(|s| matches!(s.state, b'Z' | b'X')));
        assert!(unrelated.child.try_wait().unwrap().is_none());
    }

    #[test]
    fn supervisor_rejects_untrusted_descriptor_before_launch() {
        assert!(supervisor_socket(0).is_err());
        let (left, _right) = UnixStream::pair().unwrap();
        // A socket from this process is not an inherited parent connection.
        assert!(supervisor_socket(left.as_raw_fd()).is_err());
    }

    #[test]
    fn unconfirmed_supervisor_exit_blocks_recovery_and_file_changes() {
        use super::super::{
            FxserverManager, HealthResourceSampler, ManagedFxserverProcess, RecoveryOutcome,
            SavedLaunch,
        };
        let mut unrelated = Fixture::new("echo ready; exec sleep 30");
        unrelated.line();
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 1"])
            .spawn()
            .unwrap();
        child.wait().unwrap();
        let (control, peer) = UnixStream::pair().unwrap();
        drop(peer);
        let manager = FxserverManager::default();
        *manager.process.lock().unwrap() = Some(ManagedFxserverProcess {
            child,
            stdin: None,
            artifact_path: PathBuf::from("unused"),
            started_at: std::time::SystemTime::now(),
            resource_sample: None,
            _cleanup_job: Some(std::sync::Arc::new(Supervisor {
                control,
                server_pid: unrelated.child.id(),
                cleanup_confirmed: AtomicBool::new(false),
            })),
        });
        assert!(manager
            .status()
            .unwrap_err()
            .contains("without confirming cleanup"));
        assert!(manager
            .sample_health(&mut HealthResourceSampler::default(), true)
            .is_err());
        assert!(manager
            .with_stopped_server(|| -> Result<(), String> {
                panic!("unconfirmed cleanup must block file changes")
            })
            .is_err());
        let generation = manager.arm_start().unwrap();
        manager
            .remember_launch(
                SavedLaunch {
                    artifact_path: "unused".into(),
                    environment: vec![],
                    server_profile: None,
                },
                generation,
            )
            .unwrap();
        assert!(matches!(
            manager
                .recover_last_launch(manager.launch_generation().unwrap(), &AtomicBool::new(true)),
            RecoveryOutcome::Cancelled
        ));
        assert!(manager.stop().is_err());
        assert!(unrelated.child.try_wait().unwrap().is_none());
        drop(manager);
        assert!(unrelated.child.try_wait().unwrap().is_none());
    }

    #[test]
    fn confirmed_cleanup_accepts_server_failure_exit_status() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 7"])
            .spawn()
            .unwrap();
        child.wait().unwrap();
        let (control, mut peer) = UnixStream::pair().unwrap();
        peer.write_all(b"C").unwrap();
        drop(peer);
        let supervisor = Supervisor {
            control,
            server_pid: child.id(),
            cleanup_confirmed: AtomicBool::new(false),
        };
        assert!(!supervisor.is_running(&mut child).unwrap());
        assert!(!supervisor.is_running(&mut child).unwrap());
    }

    struct Fixture {
        child: Child,
        group: OwnedProcessGroup,
    }
    impl Fixture {
        fn new(script: &str) -> Self {
            let child = Command::new("/bin/sh")
                .arg("-c")
                .arg(script)
                .process_group(0)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let group = OwnedProcessGroup::new(&child).unwrap();
            Self { child, group }
        }
        fn line(&mut self) -> String {
            let mut line = String::new();
            BufReader::new(self.child.stdout.as_mut().unwrap())
                .read_line(&mut line)
                .unwrap();
            line.trim().to_string()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self
                .group
                .force_stop(&mut self.child, Duration::from_secs(2));
        }
    }

    #[test]
    fn stat_parser_handles_parentheses_and_negative_rss() {
        let parsed =
            parse_stat("42 (a ) tricky name) S 1 42 0 0 0 0 0 0 0 0 120 30 0 0 20 0 4 0 5678 0 -1")
                .unwrap();
        assert_eq!(parsed.group, 42);
        assert_eq!(parsed.ticks, 150);
        assert_eq!(parsed.threads, 4);
        assert_eq!(parsed.start_ticks, 5678);
        assert_eq!(parsed.rss_pages, 0);
        assert!(parse_stat("malformed").is_none());
    }

    #[test]
    fn unrelated_group_and_changed_identity_are_never_signalled() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exec sleep 30"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let rejected = OwnedProcessGroup::new(&child).is_err();
        // Kill just our direct fixture child, never its inherited process group.
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(rejected);

        let mut fixture = Fixture::new("echo ready; exec sleep 30");
        fixture.line();
        fixture.group.start_ticks = fixture.group.start_ticks.wrapping_add(1);
        let rejected = fixture.group.signal(libc::SIGKILL).is_err();
        let alive = fixture.child.try_wait().unwrap().is_none();
        fixture.group.start_ticks = fixture.group.start_ticks.wrapping_sub(1);
        assert!(rejected);
        assert!(alive);
    }

    #[test]
    fn graceful_console_exit_is_reaped() {
        let mut fixture = Fixture::new("echo ready; read answer; test \"$answer\" = quit");
        assert_eq!(fixture.line(), "ready");
        fixture
            .group
            .graceful_stop(fixture.child.stdin.as_mut())
            .unwrap();
        assert!(fixture
            .group
            .wait_for_exit(&mut fixture.child, Duration::from_secs(2))
            .unwrap());
        assert!(fixture.child.try_wait().unwrap().unwrap().success());
    }

    #[test]
    fn unread_full_console_pipe_falls_back_to_sigterm_without_blocking() {
        use std::io::Write;
        let mut fixture = Fixture::new("echo ready; exec sleep 30");
        fixture.line();
        let stdin = fixture.child.stdin.as_mut().unwrap();
        let fd = stdin.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        assert!(flags >= 0);
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) },
            0
        );
        while stdin.write(&[0; 4096]).is_ok() {}
        let started = Instant::now();
        fixture
            .group
            .graceful_stop(fixture.child.stdin.as_mut())
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(fixture
            .group
            .wait_for_exit(&mut fixture.child, Duration::from_secs(2))
            .unwrap());
    }

    #[test]
    fn forced_group_stop_preserves_unrelated_process_and_reaps_leader() {
        let mut unrelated = Fixture::new("echo ready; exec sleep 30");
        unrelated.line();
        let mut fixture = Fixture::new("trap '' TERM; sleep 30 & echo $!; wait");
        let descendant = fixture.line().parse::<u32>().unwrap();
        assert_eq!(stat(descendant).unwrap().group, fixture.child.id());
        fixture.group.signal(libc::SIGTERM).unwrap();
        assert!(!fixture
            .group
            .wait_for_exit(&mut fixture.child, Duration::from_millis(100))
            .unwrap());
        fixture
            .group
            .force_stop(&mut fixture.child, Duration::from_secs(2))
            .unwrap();
        assert!(!group_has_live_members(fixture.child.id()).unwrap());
        assert!(fixture.child.try_wait().unwrap().is_some());
        assert!(unrelated.child.try_wait().unwrap().is_none());
        fixture
            .group
            .force_stop(&mut fixture.child, Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn leader_crash_cleans_orphan_and_rejects_reaped_child() {
        let mut fixture = Fixture::new("sleep 30 & echo $!; exit 7");
        fixture.line();
        assert!(fixture
            .group
            .wait_for_exit(&mut fixture.child, Duration::from_secs(2))
            .unwrap());
        assert_eq!(fixture.child.try_wait().unwrap().unwrap().code(), Some(7));
        assert!(!group_has_live_members(fixture.child.id()).unwrap());
        let mut reaped = Fixture::new("exit 0");
        reaped.child.wait().unwrap();
        assert!(reaped.group.signal(libc::SIGKILL).is_err());
    }

    #[test]
    fn proc_sampling_includes_group_children() {
        let mut fixture = Fixture::new("sleep 30 & echo ready; wait");
        fixture.line();
        let resources = read_resources(fixture.child.id(), &mut None).unwrap();
        assert!(resources.thread_count >= 2);
        assert!(resources.handle_count >= 3);
        assert!(resources.memory_bytes > 0);
        assert!(resources.total_memory_bytes > 0);
        assert_eq!(resources.cpu_percent, 0.0);
    }
}
