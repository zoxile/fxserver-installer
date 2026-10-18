use super::plans::Plan;
use std::{
    cell::Cell,
    io::{IsTerminal, Read},
    os::unix::{fs::MetadataExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
#[cfg(any(feature = "terminal", test))]
use std::{marker::PhantomData, rc::Rc};

thread_local! {
    static TERMINAL_INPUT_RESERVATIONS: Cell<usize> = const { Cell::new(0) };
}

fn terminal_input_reserved() -> bool {
    TERMINAL_INPUT_RESERVATIONS.with(|depth| depth.get() != 0)
}

#[cfg(any(feature = "terminal", test))]
pub(crate) struct TerminalInputReservation(PhantomData<Rc<()>>);

#[cfg(any(feature = "terminal", test))]
impl Drop for TerminalInputReservation {
    fn drop(&mut self) {
        TERMINAL_INPUT_RESERVATIONS.with(|depth| depth.set(depth.get() - 1));
    }
}

/// Reserve stdin on this blocking worker while the TUI owns terminal input.
#[cfg(any(feature = "terminal", test))]
#[must_use]
pub(crate) fn reserve_terminal_input() -> TerminalInputReservation {
    TERMINAL_INPUT_RESERVATIONS.with(|depth| {
        depth.set(
            depth
                .get()
                .checked_add(1)
                .expect("Terminal input reservation overflow"),
        );
    });
    // A reservation must be dropped on the same thread, including during unwinding.
    TerminalInputReservation(PhantomData)
}

pub struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn system_program(name: &str) -> Result<PathBuf, String> {
    for directory in ["/usr/bin", "/usr/sbin", "/bin", "/sbin"] {
        let Ok(path) = Path::new(directory).join(name).canonicalize() else {
            continue;
        };
        let Ok(meta) = path.metadata() else { continue };
        if !meta.is_file() || meta.mode() & 0o111 == 0 {
            continue;
        }
        if path.ancestors().all(|p| {
            p.metadata()
                .is_ok_and(|m| m.uid() == 0 && m.mode() & 0o022 == 0)
        }) {
            return Ok(path);
        }
    }
    Err(format!(
        "A root-owned system {name} executable is required."
    ))
}

fn drain(mut input: impl Read + Send + 'static) -> thread::JoinHandle<Result<String, String>> {
    thread::spawn(move || {
        let mut result = Vec::new();
        let mut buffer = [0; 8192];
        let mut overflow = false;
        loop {
            let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            let keep = count.min((1024 * 1024usize).saturating_sub(result.len()));
            overflow |= keep != count;
            result.extend_from_slice(&buffer[..keep]);
        }
        if overflow {
            return Err("System command output exceeded 1 MiB.".into());
        }
        Ok(String::from_utf8_lossy(&result).trim().into())
    })
}

pub fn run(command: &mut Command, timeout: Option<Duration>) -> Result<Output, String> {
    command
        .env("LC_ALL", "C")
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin");
    command
        .env_remove("APT_CONFIG")
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME");
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    // sudo must remain in the terminal's foreground process group to prompt safely.
    if timeout.is_some() {
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Cannot start system command: {e}"))?;
    let stdout = drain(child.stdout.take().unwrap());
    let stderr = drain(child.stderr.take().unwrap());
    let start = Instant::now();
    let status = loop {
        match exited_without_reaping(child.id()) {
            Ok(true) if stdout.is_finished() && stderr.is_finished() => {
                break child
                    .wait()
                    .map_err(|e| format!("Cannot reap system command: {e}"))?;
            }
            Ok(_) => {}
            Err(error) => {
                if timeout.is_some() {
                    terminate_group(&mut child);
                }
                return Err(format!("Cannot wait for system command: {error}"));
            }
        }
        if timeout.is_some_and(|limit| start.elapsed() >= limit) {
            terminate_group(&mut child);
            return Err("Read-only system probe timed out.".into());
        }
        thread::sleep(Duration::from_millis(25));
    };
    Ok(Output {
        success: status.success(),
        stdout: stdout
            .join()
            .map_err(|_| "System output worker failed.")??,
        stderr: stderr.join().map_err(|_| "System error worker failed.")??,
    })
}

fn exited_without_reaping(pid: u32) -> std::io::Result<bool> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // Keep the process-group leader reserved until pipe readers finish or timeout cleanup runs.
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(error);
    }
    Ok(unsafe { info.si_pid() } != 0)
}

pub fn terminate_group(child: &mut std::process::Child) {
    // Every caller creates a new process group before spawning.
    if let Ok(pid) = i32::try_from(child.id()) {
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub fn read(plan: &Plan) -> Result<Output, String> {
    run(
        Command::new(system_program(plan.program)?)
            .args(&plan.args)
            .stdin(Stdio::null()),
        Some(Duration::from_secs(20)),
    )
}

pub fn checked_read(plan: &Plan) -> Result<String, String> {
    let output = read(plan)?;
    if output.success {
        Ok(output.stdout)
    } else {
        Err(format!("{} query failed: {}", plan.program, output.stderr))
    }
}

fn sudo_command(sudo: &Path, program: &Path, args: &[String], reserved: bool) -> Command {
    let mut command = Command::new(sudo);
    if reserved {
        command.arg("-n");
    }
    command
        .arg("--")
        .arg(program)
        .args(args)
        .stdin(if reserved {
            Stdio::null()
        } else {
            Stdio::inherit()
        });
    command
}

pub fn administer(plan: &Plan) -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        return Err("Run the app as your normal user. Only the requested package or service action may be elevated.".into());
    }
    if !matches!(plan.program, "apt-get" | "dnf" | "pacman" | "systemctl") {
        return Err("Unsupported privileged program.".into());
    }
    let program = system_program(plan.program)?;
    let reserved = terminal_input_reserved();
    let terminal = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let mut command = if reserved || terminal {
        sudo_command(&system_program("sudo")?, &program, &plan.args, reserved)
    } else if std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
    {
        let mut cmd = Command::new(system_program("pkexec")?);
        cmd.arg("--disable-internal-agent")
            .arg(program)
            .args(&plan.args)
            .stdin(Stdio::null());
        cmd
    } else {
        return Err("This action needs a desktop polkit agent (pkexec) or an interactive terminal (sudo). No changes were made.".into());
    };
    // Do not kill a package manager mid-transaction or retry an uncertain mutation.
    let output = run(&mut command, None)?;
    if output.success {
        Ok(())
    } else {
        let guidance = if reserved {
            " The terminal UI cannot prompt for administrator credentials. If sudo authentication is required, quit the TUI, run `sudo -v` in the same terminal, then reopen it as your normal user. Inspect the package or service error before retrying. Do not run the app as root."
        } else {
            ""
        };
        Err(format!(
            "{} action failed or administrator approval was declined: {}{}",
            plan.program,
            if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            },
            guidance
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sudo_builder_preserves_noninteractive_flag_and_argument_boundary() {
        let args = vec!["restart".into(), "mariadb.service".into()];
        for reserved in [false, true] {
            let command = sudo_command(
                Path::new("/usr/bin/sudo"),
                Path::new("/usr/bin/systemctl"),
                &args,
                reserved,
            );
            assert_eq!(command.get_program(), "/usr/bin/sudo");
            let actual: Vec<_> = command
                .get_args()
                .map(|arg| arg.to_str().unwrap())
                .collect();
            let mut expected = vec!["--", "/usr/bin/systemctl", "restart", "mariadb.service"];
            if reserved {
                expected.insert(0, "-n");
            }
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn terminal_input_reservations_are_nested_and_thread_local() {
        assert!(!terminal_input_reserved());
        let outer = reserve_terminal_input();
        assert!(terminal_input_reserved());
        {
            let _inner = reserve_terminal_input();
            assert!(terminal_input_reserved());
        }
        assert!(terminal_input_reserved());
        thread::spawn(|| {
            assert!(!terminal_input_reserved());
            let _other = reserve_terminal_input();
            assert!(terminal_input_reserved());
        })
        .join()
        .unwrap();
        assert!(terminal_input_reserved());
        drop(outer);
        assert!(!terminal_input_reserved());
    }

    #[test]
    fn terminal_input_reservation_resets_after_panics() {
        assert!(!terminal_input_reserved());
        let outer = reserve_terminal_input();
        let result = std::panic::catch_unwind(|| {
            let _inner = reserve_terminal_input();
            panic!("fixture panic while input is reserved");
        });
        assert!(result.is_err());
        assert!(terminal_input_reserved());
        drop(outer);
        assert!(!terminal_input_reserved());
        let result = std::panic::catch_unwind(|| {
            let _reservation = reserve_terminal_input();
            panic!("fixture panic without an outer reservation");
        });
        assert!(result.is_err());
        assert!(!terminal_input_reserved());
    }

    #[test]
    fn timeout_reaps_the_leader_and_closes_descendant_pipes() {
        let started = Instant::now();
        let result = run(
            Command::new("/bin/sh").args(["-c", "sleep 30 & exit 0"]),
            Some(Duration::from_millis(150)),
        );
        assert!(result.err().unwrap().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn completed_readers_keep_exit_status_and_output() {
        let output = run(
            Command::new("/bin/sh").args(["-c", "printf fixture; printf diagnostic >&2; exit 7"]),
            Some(Duration::from_secs(2)),
        )
        .unwrap();
        assert!(!output.success);
        assert_eq!(output.stdout, "fixture");
        assert_eq!(output.stderr, "diagnostic");
    }
}
