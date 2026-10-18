//! Linux children own a fresh process group. Keep its leader waitable until cleanup
//! finishes: reaping it first would allow its PID (and our group ID) to be reused.
//! This is not a cgroup or external watchdog: SIGKILL/abort of the manager bypasses
//! Drop, and descendants that deliberately leave the group are not contained.
//! Explicit application shutdown must call the shared manager stop path.
use super::{FxserverResources, ResourceSample};
use std::{
    collections::HashSet,
    fs, io,
    os::{fd::AsRawFd, unix::process::CommandExt},
    path::Path,
    process::{Child, ChildStdin, Command},
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

pub(super) fn launch_command(launcher: &Path) -> Command {
    let mut command = Command::new("/bin/bash");
    command.arg(launcher).process_group(0);
    command
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

    pub(super) fn is_running(&self, child: &mut Child) -> Result<bool, String> {
        if !self.leader_exited()? {
            return Ok(true);
        }
        self.force_stop(child, super::FORCE_STOP_WAIT_TIMEOUT)?;
        Ok(false)
    }

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
        process::Stdio,
    };

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
