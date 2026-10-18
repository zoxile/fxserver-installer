use super::plans::Plan;
use std::{
    io::{IsTerminal, Read},
    os::unix::{fs::MetadataExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

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
        match child.try_wait() {
            Ok(Some(status)) if stdout.is_finished() && stderr.is_finished() => break status,
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

pub fn administer(plan: &Plan) -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        return Err("Run the app as your normal user. Only the requested package or service action may be elevated.".into());
    }
    if !matches!(plan.program, "apt-get" | "dnf" | "pacman" | "systemctl") {
        return Err("Unsupported privileged program.".into());
    }
    let program = system_program(plan.program)?;
    let terminal = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let mut command = if terminal {
        let mut cmd = Command::new(system_program("sudo")?);
        cmd.arg("--")
            .arg(program)
            .args(&plan.args)
            .stdin(Stdio::inherit());
        cmd
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
        Err(format!(
            "{} action failed or administrator approval was declined: {}",
            plan.program,
            if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            }
        ))
    }
}
