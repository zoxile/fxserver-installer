use super::{execute_privileged, FirewallBackend, FirewallRequest, FirewallSnapshot, Operation};
use std::{
    ffi::OsString,
    io::{Read, Write},
    time::{Duration, Instant},
};

#[cfg(not(target_os = "linux"))]
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener as Listener, TcpStream as Stream};
#[cfg(target_os = "linux")]
use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};

const HELPER: &str = "--fxins-firewall-helper";
const ACTION_TIMEOUT: Duration = Duration::from_secs(180);
const RESPONSE_LIMIT: usize = 1024 * 1024;

struct HelperArgs {
    request: FirewallRequest,
    operation: Operation,
    reply_port: u16,
    nonce: String,
}

fn word_backend(backend: FirewallBackend) -> &'static str {
    match backend {
        FirewallBackend::Auto => "auto",
        FirewallBackend::Windows => "windows",
        FirewallBackend::Ufw => "ufw",
        FirewallBackend::Firewalld => "firewalld",
        FirewallBackend::Unsupported => "unsupported",
    }
}

fn word_operation(operation: Operation) -> &'static str {
    match operation {
        Operation::Inspect => "inspect",
        Operation::Configure => "configure",
        Operation::Remove => "remove",
    }
}

fn port(value: &str) -> Result<u16, String> {
    if value.is_empty() || value.len() > 5 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Invalid firewall port.".into());
    }
    value
        .parse::<u16>()
        .ok()
        .filter(|n| *n != 0)
        .ok_or_else(|| "Invalid firewall port.".into())
}

fn parse(args: &[String]) -> Result<HelperArgs, String> {
    if args.len() != 6 {
        return Err("Invalid firewall helper arguments.".into());
    }
    let operation = match args[0].as_str() {
        "inspect" => Operation::Inspect,
        "configure" => Operation::Configure,
        "remove" => Operation::Remove,
        _ => return Err("Invalid firewall operation.".into()),
    };
    let backend = match args[1].as_str() {
        "auto" => FirewallBackend::Auto,
        "windows" => FirewallBackend::Windows,
        "ufw" => FirewallBackend::Ufw,
        "firewalld" => FirewallBackend::Firewalld,
        _ => return Err("Invalid firewall backend.".into()),
    };
    if args[5].len() != 64 || !args[5].bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid firewall reply token.".into());
    }
    Ok(HelperArgs {
        operation,
        request: FirewallRequest {
            backend,
            tcp_port: port(&args[2])?,
            udp_port: port(&args[3])?,
        },
        reply_port: port(&args[4])?,
        nonce: args[5].clone(),
    })
}

fn arguments(
    request: FirewallRequest,
    operation: Operation,
    reply_port: u16,
    nonce: String,
) -> Vec<String> {
    vec![
        HELPER.into(),
        word_operation(operation).into(),
        word_backend(request.backend).into(),
        request.tcp_port.to_string(),
        request.udp_port.to_string(),
        reply_port.to_string(),
        nonce,
    ]
}

/// Call before single-instance, desktop initialization, or CLI argument parsing in every binary.
pub fn run_firewall_helper_from_args() -> bool {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(HELPER)) {
        return false;
    }
    let values: Result<Vec<String>, OsString> = args.take(7).map(OsString::into_string).collect();
    let result = values
        .map_err(|_| "Invalid firewall helper encoding.".to_string())
        .and_then(|values| parse(&values))
        .and_then(helper);
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
    std::process::exit(0);
}

fn helper(args: HelperArgs) -> Result<(), String> {
    if !is_elevated()? {
        return Err("The firewall helper requires administrator approval.".into());
    }
    let mut stream = connect(&args)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(args.nonce.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut ack = [0];
    stream.read_exact(&mut ack).map_err(|e| e.to_string())?;
    if ack != [1] {
        return Err("Requesting app did not authorize this helper.".into());
    }
    let result =
        super::runtime::within_action_deadline(|| execute_privileged(args.request, args.operation));
    let bytes = serde_json::to_vec(&result).map_err(|e| e.to_string())?;
    if bytes.len() > RESPONSE_LIMIT {
        return Err("Firewall response is too large.".into());
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(&bytes).map_err(|e| e.to_string())
}

pub(super) fn launch(
    request: FirewallRequest,
    operation: Operation,
) -> Result<FirewallSnapshot, String> {
    #[cfg(target_os = "linux")]
    if is_elevated()? {
        return Err(
            "Run the app as your normal user; only the firewall helper is elevated.".into(),
        );
    }
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|e| format!("Cannot create firewall reply token: {e}"))?;
    let nonce: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let (listener, reply_port) = listen(&nonce)?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let args = arguments(request, operation, reply_port, nonce.clone());
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut process = start(&executable, &args)?;
    let start = Instant::now();
    loop {
        if start.elapsed() >= ACTION_TIMEOUT {
            return Err("Firewall approval/action timed out. Inspect the current configuration before retrying.".into());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if !trusted_peer(&stream)? {
                    continue;
                }
                stream
                    .set_read_timeout(Some(Duration::from_millis(300)))
                    .map_err(|e| e.to_string())?;
                stream
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .map_err(|e| e.to_string())?;
                let mut supplied = [0; 64];
                if read_before(
                    &mut stream,
                    &mut supplied,
                    Instant::now() + Duration::from_millis(300),
                )
                .is_err()
                    || supplied != nonce.as_bytes()
                {
                    continue;
                }
                stream.write_all(&[1]).map_err(|e| e.to_string())?;
                stream
                    .set_read_timeout(Some(ACTION_TIMEOUT.saturating_sub(start.elapsed())))
                    .map_err(|e| e.to_string())?;
                let mut size = [0; 4];
                read_before(&mut stream, &mut size, start + ACTION_TIMEOUT).map_err(|e| {
                    format!(
                        "Firewall helper did not return a result: {e}. Inspect before retrying."
                    )
                })?;
                let size = u32::from_be_bytes(size) as usize;
                if size > RESPONSE_LIMIT {
                    return Err("Firewall helper response exceeded its limit.".into());
                }
                let mut bytes = vec![0; size];
                read_before(&mut stream, &mut bytes, start + ACTION_TIMEOUT)
                    .map_err(|e| e.to_string())?;
                return serde_json::from_slice::<Result<FirewallSnapshot, String>>(&bytes)
                    .map_err(|e| format!("Invalid firewall helper response: {e}"))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.to_string()),
        }
        if process.finished()? {
            return Err("Firewall helper exited without a result or approval was declined. On headless Linux, leave the TUI, run `sudo -v`, then reopen it as your normal user. A desktop requires a running polkit authentication agent.".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn read_before(
    stream: &mut Stream,
    mut buffer: &mut [u8],
    deadline: Instant,
) -> std::io::Result<()> {
    while !buffer.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        stream.set_read_timeout(Some(remaining))?;
        match stream.read(buffer) {
            Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            Ok(count) => buffer = &mut buffer[count..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn listen(_: &str) -> Result<(Listener, u16), String> {
    let listener = Listener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    Ok((listener, port))
}

#[cfg(not(target_os = "linux"))]
fn connect(args: &HelperArgs) -> Result<Stream, String> {
    Stream::connect_timeout(
        &SocketAddrV4::new(Ipv4Addr::LOCALHOST, args.reply_port).into(),
        Duration::from_secs(3),
    )
    .map_err(|e| format!("Cannot connect to requesting app: {e}"))
}

#[cfg(not(target_os = "linux"))]
fn trusted_peer(_: &Stream) -> Result<bool, String> {
    Ok(true)
}

#[cfg(target_os = "linux")]
fn listen(nonce: &str) -> Result<(Listener, u16), String> {
    use std::os::linux::net::SocketAddrExt;
    let address = std::os::unix::net::SocketAddr::from_abstract_name(format!("fxins-fw-{nonce}"))
        .map_err(|e| e.to_string())?;
    // No user-controlled paths or socket files are passed to the elevated process.
    Ok((Listener::bind_addr(&address).map_err(|e| e.to_string())?, 1))
}

#[cfg(target_os = "linux")]
fn connect(args: &HelperArgs) -> Result<Stream, String> {
    use std::os::fd::{AsRawFd, FromRawFd};
    if args.reply_port != 1 {
        return Err("Invalid Linux firewall reply transport.".into());
    }
    let name = format!("fxins-fw-{}", args.nonce);
    unsafe {
        let fd = libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        );
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let stream = Stream::from_raw_fd(fd);
        let mut address: libc::sockaddr_un = std::mem::zeroed();
        address.sun_family = libc::AF_UNIX as libc::sa_family_t;
        if name.len() + 1 > address.sun_path.len() {
            return Err("Firewall socket name is too long.".into());
        }
        for (slot, byte) in address.sun_path[1..].iter_mut().zip(name.bytes()) {
            *slot = byte as libc::c_char;
        }
        let length =
            (std::mem::offset_of!(libc::sockaddr_un, sun_path) + 1 + name.len()) as libc::socklen_t;
        if libc::connect(
            stream.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            length,
        ) != 0
        {
            return Err(format!(
                "Cannot connect to requesting app: {}",
                std::io::Error::last_os_error()
            ));
        }
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        Ok(stream)
    }
}

#[cfg(target_os = "linux")]
fn trusted_peer(stream: &Stream) -> Result<bool, String> {
    use std::os::fd::AsRawFd;
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of_val(&credentials) as libc::socklen_t;
    let ok = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut size,
        )
    };
    if ok != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // Linux exposes argv to other users; the nonce alone is not an authentication boundary.
    Ok(size as usize == std::mem::size_of_val(&credentials) && credentials.uid == 0)
}

#[cfg(target_os = "linux")]
pub(super) fn is_elevated() -> Result<bool, String> {
    Ok(unsafe { libc::geteuid() } == 0)
}

#[cfg(target_os = "linux")]
fn start(executable: &std::path::Path, args: &[String]) -> Result<ElevatedProcess, String> {
    use std::process::{Command, Stdio};
    let graphical =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    let mut command = if graphical {
        let mut c = Command::new(super::runtime::system_program("pkexec")?);
        c.arg("--disable-internal-agent");
        c
    } else {
        let mut c = Command::new(super::runtime::system_program("sudo")?);
        c.args(["-n", "--"]);
        c
    };
    command
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C");
    for key in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_RUNTIME_DIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let child = command
        .arg(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Cannot request firewall administrator approval: {e}"))?;
    Ok(ElevatedProcess(Some(child)))
}

#[cfg(target_os = "linux")]
struct ElevatedProcess(Option<std::process::Child>);
#[cfg(target_os = "linux")]
impl ElevatedProcess {
    fn finished(&mut self) -> Result<bool, String> {
        self.0
            .as_mut()
            .ok_or("Missing firewall helper process.")?
            .try_wait()
            .map(|s| s.is_some())
            .map_err(|e| e.to_string())
    }
}
#[cfg(target_os = "linux")]
impl Drop for ElevatedProcess {
    fn drop(&mut self) {
        // Reap without holding the UI worker; the helper has its own action deadline.
        if let Some(mut child) = self.0.take() {
            if child.try_wait().ok().flatten().is_none() {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
        }
    }
}

#[cfg(windows)]
#[path = "elevation_windows.rs"]
mod platform;
#[cfg(windows)]
use platform::start;
#[cfg(windows)]
pub(super) fn is_elevated() -> Result<bool, String> {
    platform::is_elevated()
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(super) fn is_elevated() -> Result<bool, String> {
    Err("Unsupported operating system.".into())
}
#[cfg(not(any(windows, target_os = "linux")))]
fn start(_: &std::path::Path, _: &[String]) -> Result<ElevatedProcess, String> {
    Err("Unsupported operating system.".into())
}
#[cfg(not(any(windows, target_os = "linux")))]
struct ElevatedProcess;
#[cfg(not(any(windows, target_os = "linux")))]
impl ElevatedProcess {
    fn finished(&mut self) -> Result<bool, String> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_reply_authentication_uses_kernel_credentials() {
        let (peer, _) = Stream::pair().unwrap();
        assert_eq!(
            trusted_peer(&peer).unwrap(),
            unsafe { libc::geteuid() } == 0
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_reply_transport_has_no_files_or_terminal_input() {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).unwrap();
        let nonce: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let (listener, reply_port) = listen(&nonce).unwrap();
        let args = HelperArgs {
            request: FirewallRequest {
                backend: FirewallBackend::Ufw,
                tcp_port: 30120,
                udp_port: 30120,
            },
            operation: Operation::Inspect,
            reply_port,
            nonce,
        };
        let mut client = connect(&args).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        client.write_all(b"test").unwrap();
        let mut response = [0; 4];
        read_before(
            &mut server,
            &mut response,
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(&response, b"test");
        assert!(read_before(&mut server, &mut response, Instant::now()).is_err());
    }
    #[test]
    fn helper_rejects_untyped_arguments() {
        let valid = vec![
            "configure".into(),
            "ufw".into(),
            "30120".into(),
            "30121".into(),
            "12000".into(),
            "a".repeat(64),
        ];
        assert!(parse(&valid).is_ok());
        for (index, value) in [
            (0, "enable"),
            (0, "inspect; id"),
            (1, "nft"),
            (2, "0"),
            (2, "65536"),
            (2, "+80"),
            (3, "80;id"),
            (4, "host:80"),
            (5, "../../file"),
        ] {
            let mut args = valid.clone();
            args[index] = value.into();
            assert!(parse(&args).is_err());
        }
        let mut extra = valid.clone();
        extra.push("--file".into());
        assert!(parse(&extra).is_err());
        assert!(parse(&valid[..5]).is_err());
    }
    #[test]
    fn arguments_round_trip_without_paths_or_scripts() {
        let request = FirewallRequest {
            backend: FirewallBackend::Windows,
            tcp_port: 1,
            udp_port: 65535,
        };
        for operation in [Operation::Inspect, Operation::Configure, Operation::Remove] {
            let args = arguments(request, operation, 23456, "b".repeat(64));
            let parsed = parse(&args[1..]).unwrap();
            assert_eq!(parsed.operation, operation);
            assert_eq!(parsed.request.udp_port, 65535);
            assert_eq!(args.len(), 7);
        }
    }
}
