use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    net::UdpSocket,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::fxserver::{self, FxserverManager};
use super::{backup_manager::storage, managed_rcon};
use crate::models::fxserver::{FxserverRconConfig, FxserverStatus};

const MAX_CAPTURE_BYTES: u64 = 16 * 1024 * 1024;
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(45);
const SAVE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 4096;
const MAX_RESPONSE_LINE_BYTES: usize = 1024;
const CANCEL_MESSAGE: &str = "Profiler capture cancelled. No profiler stop was sent; a started frame-bounded recording ends as the server advances.";
static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfilerCaptureRequest {
    pub frames: u32,
    pub rcon: FxserverRconConfig,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilerCaptureStatus {
    pub id: String,
    pub frames: u32,
    pub phase: String,
    pub captured_frames: u32,
    pub cancel_requested: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilerCaptureResult {
    pub id: String,
    pub file_name: String,
    pub frames: u32,
    pub json: String,
}

struct ActiveCapture {
    status: ProfilerCaptureStatus,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone, Default)]
pub struct ProfilerCaptureManager(
    Arc<Mutex<Option<ActiveCapture>>>,
    Arc<Mutex<Option<RetainedExport>>>,
);

struct RetainedExport {
    file: CaptureFile,
    target: ServerIdentity,
}

impl ProfilerCaptureManager {
    fn begin(&self, frames: u32) -> Result<(String, Arc<AtomicBool>), String> {
        if self
            .1
            .lock()
            .map_err(|_| "Profiler cleanup state is unavailable.")?
            .is_some()
        {
            return Err("The previous profiler export is still protected. Wait for its writer to finish or stop the managed server, then retry capture.".into());
        }
        let mut active = self
            .0
            .lock()
            .map_err(|_| "Profiler state is unavailable.")?;
        if active.is_some() {
            return Err("A guided profiler capture is already running.".into());
        }
        let id = capture_id();
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(ActiveCapture {
            status: ProfilerCaptureStatus {
                id: id.clone(),
                frames,
                phase: "checking".into(),
                captured_frames: 0,
                cancel_requested: false,
            },
            cancel: cancel.clone(),
        });
        Ok((id, cancel))
    }

    fn progress(&self, phase: &str, captured_frames: u32) {
        if let Ok(mut active) = self.0.lock() {
            if let Some(active) = active.as_mut() {
                active.status.phase = phase.into();
                active.status.captured_frames = captured_frames.min(active.status.frames);
            }
        }
    }

    fn retry_retained_cleanup(&self, status: &FxserverStatus) -> Result<(), String> {
        let mut pending = self
            .1
            .lock()
            .map_err(|_| "Profiler cleanup state is unavailable.")?;
        if let Some(retained) = pending.as_mut() {
            let exited = !status.running
                || status.pid.is_some_and(|pid| pid != retained.target.pid)
                || status
                    .started_at
                    .as_ref()
                    .is_some_and(|start| start != &retained.target.started_at);
            if !exited && !retained.file.writer_finished() {
                return Ok(());
            }
            retained.file.remove()?;
            *pending = None;
        }
        Ok(())
    }
}

// Clear the single-capture lease even when an async task or blocking worker fails.
struct CaptureLease(ProfilerCaptureManager);
impl Drop for CaptureLease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.0 .0.lock() {
            *active = None;
        }
    }
}

#[tauri::command]
pub async fn capture_fxserver_profile(
    request: ProfilerCaptureRequest,
    app: AppHandle,
    manager: tauri::State<'_, ProfilerCaptureManager>,
) -> Result<ProfilerCaptureResult, String> {
    validate_request(&request)?;
    let state = manager.inner().clone();
    let status = fxserver::get_fxserver_status(app.state::<FxserverManager>()).await?;
    state.retry_retained_cleanup(&status)?;
    let (id, cancel) = state.begin(request.frames)?;
    let lease = CaptureLease(state.clone());
    // Use the shared work permit so native workspace switching/shutdown sees this job.
    super::run_blocking(move || {
        let _lease = lease;
        tauri::async_runtime::block_on(async move {
            let mut io = ManagedCaptureIo {
                app,
                rcon: request.rcon,
                started: Instant::now(),
            };
            run_capture(&mut io, &state, &id, request.frames, &cancel).await
        })
    })
    .await
}

#[tauri::command]
pub fn get_profiler_capture_status(
    manager: tauri::State<'_, ProfilerCaptureManager>,
) -> Result<Option<ProfilerCaptureStatus>, String> {
    Ok(manager
        .0
        .lock()
        .map_err(|_| "Profiler state is unavailable.")?
        .as_ref()
        .map(|active| active.status.clone()))
}

#[tauri::command]
pub fn cancel_profiler_capture(
    id: String,
    manager: tauri::State<'_, ProfilerCaptureManager>,
) -> Result<(), String> {
    let mut active = manager
        .0
        .lock()
        .map_err(|_| "Profiler state is unavailable.")?;
    let active = active
        .as_mut()
        .filter(|active| active.status.id == id)
        .ok_or("That profiler capture is no longer active.")?;
    active.cancel.store(true, Ordering::SeqCst);
    active.status.cancel_requested = true;
    Ok(())
}

fn validate_request(request: &ProfilerCaptureRequest) -> Result<(), String> {
    if !matches!(request.frames, 100 | 250 | 500) {
        return Err("Choose a bounded capture of 100, 250, or 500 frames.".into());
    }
    managed_rcon::validate_local_rcon(&request.rcon)
}

fn capture_id() -> String {
    format!(
        "fxi-profiler-{:x}-{:x}-{:x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ServerIdentity {
    pid: u32,
    started_at: String,
    root: PathBuf,
}

impl ServerIdentity {
    fn from_status(status: FxserverStatus) -> Result<Self, String> {
        if !status.running {
            return Err(
                "FXServer must already be running from this app. No server was started.".into(),
            );
        }
        Ok(Self {
            pid: status.pid.ok_or("Managed server PID is unavailable.")?,
            started_at: status
                .started_at
                .ok_or("Managed server start time is unavailable.")?,
            root: fs::canonicalize(
                status
                    .artifact_path
                    .ok_or("Managed server working directory is unavailable.")?,
            )
            .map_err(|_| "Managed server working directory is unavailable.")?,
        })
    }
}

trait CaptureIo {
    async fn identity(&mut self) -> Result<ServerIdentity, String>;
    async fn verify_endpoint(&mut self, target: &ServerIdentity) -> Result<(), String>;
    async fn command(&mut self, command: String) -> Result<String, String>;
    async fn wait(&mut self);
    fn elapsed(&self) -> Duration;
}

struct ManagedCaptureIo {
    app: AppHandle,
    rcon: FxserverRconConfig,
    started: Instant,
}

impl CaptureIo for ManagedCaptureIo {
    async fn verify_endpoint(&mut self, target: &ServerIdentity) -> Result<(), String> {
        managed_rcon::verify_managed_rcon_endpoint(target.pid, self.rcon.clone(), self.app.clone())
            .await
    }
    async fn identity(&mut self) -> Result<ServerIdentity, String> {
        ServerIdentity::from_status(
            fxserver::get_fxserver_status(self.app.state::<FxserverManager>()).await?,
        )
    }

    async fn command(&mut self, command: String) -> Result<String, String> {
        let before =
            fxserver::get_fxserver_terminal(Some(50), None, self.app.state::<FxserverManager>())
                .await?
                .entries
                .last()
                .map(|entry| entry.id)
                .unwrap_or(0);
        // Cfx RconOutOfBand prefixes synchronous output with rcon/; detached saves use cmd.
        // Only those managed stdout channels are evidence. Do not collect UDP replies.
        send_profiler_command(&self.rcon, &command)?;
        let mut response = String::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut cursor = before;
        loop {
            let terminal = fxserver::get_fxserver_terminal(
                Some(500),
                Some(cursor),
                self.app.state::<FxserverManager>(),
            )
            .await?;
            for entry in terminal.entries {
                cursor = cursor.max(entry.id);
                append_profiler_response(&mut response, &entry.stream, &entry.plain_line)?;
            }
            if command_response_ready(&command, &response) || Instant::now() >= deadline {
                return Ok(response);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn wait(&mut self) {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

fn send_profiler_command(rcon: &FxserverRconConfig, command: &str) -> Result<(), String> {
    managed_rcon::validate_local_rcon(rcon)?;
    if command.len() + rcon.password.len() > 2048 {
        return Err("Profiler command exceeds the local RCON packet limit.".into());
    }
    let socket =
        UdpSocket::bind("127.0.0.1:0").map_err(|_| "Could not open local profiler RCON socket.")?;
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| "Could not configure profiler RCON socket.")?;
    socket
        .connect((std::net::Ipv4Addr::LOCALHOST, rcon.port))
        .map_err(|_| "Could not connect local profiler RCON socket.")?;
    let mut packet = vec![0xff; 4];
    packet.extend_from_slice(format!("rcon {} {command}", rcon.password).as_bytes());
    socket
        .send(&packet)
        .map_err(|_| "Could not send profiler command.")?;
    Ok(())
}

fn profiler_body(line: &str) -> Option<&str> {
    if line.len() > MAX_RESPONSE_LINE_BYTES || line.contains(['\r', '\n']) {
        return None;
    }
    let (channel, body) = line.trim().strip_prefix('[')?.split_once(']')?;
    matches!(channel.trim(), "cmd" | "rcon/cmd").then(|| body.trim())
}

fn append_profiler_response(response: &mut String, stream: &str, line: &str) -> Result<(), String> {
    if stream != "stdout" || profiler_body(line).is_none() {
        return Ok(());
    }
    if response.len() + line.len() + 1 > MAX_RESPONSE_BYTES {
        return Err("Profiler command response exceeded the 4 KiB limit.".into());
    }
    response.push_str(line);
    response.push('\n');
    Ok(())
}

fn has_profiler_line(response: &str, expected: &str) -> bool {
    response
        .lines()
        .filter_map(profiler_body)
        .any(|body| body == expected)
}

fn command_response_ready(command: &str, response: &str) -> bool {
    response.lines().filter_map(profiler_body).any(|body| {
        body == "No such command profiler."
            || body == "Access denied for command profiler."
            || if command == "profiler status" {
                body.starts_with("Recording: ")
            } else if command.starts_with("profiler record ") {
                matches!(
                    body,
                    "Started recording" | "A recording is already taking place"
                )
            } else {
                body == "Save complete" || body.starts_with("Saving the recording as JSON to: ")
            }
    })
}

#[derive(Debug, PartialEq, Eq)]
enum RecordingStatus {
    Idle,
    Recording(Option<u32>),
}

fn parse_recording_status(response: &str) -> Result<RecordingStatus, String> {
    if response.len() > MAX_RESPONSE_BYTES {
        return Err("Profiler status response is too large.".into());
    }
    let mut status = None;
    for line in response.lines() {
        let Some(line) = profiler_body(line) else {
            continue;
        };
        let Some(value) = line.strip_prefix("Recording: ") else {
            continue;
        };
        let parsed = if value == "No" {
            RecordingStatus::Idle
        } else if value == "Yes" {
            RecordingStatus::Recording(None)
        } else if let Some(number) = value
            .strip_prefix("Yes (")
            .and_then(|s| s.strip_suffix(')'))
        {
            RecordingStatus::Recording(Some(
                number
                    .parse()
                    .map_err(|_| "Unrecognized profiler status; no recording was changed.")?,
            ))
        } else {
            return Err("Unrecognized profiler status; no recording was changed.".into());
        };
        if status.as_ref().is_some_and(|previous| previous != &parsed) {
            return Err("Conflicting profiler status responses. Check the server console.".into());
        }
        status = Some(parsed);
    }
    status.ok_or("FXServer did not confirm profiler status. Check RCON, permissions, and profiler support in this artifact.".into())
}

async fn checked_command(
    io: &mut impl CaptureIo,
    target: &ServerIdentity,
    command: String,
) -> Result<String, String> {
    verify_target(io, target).await?;
    io.command(command).await
}

async fn verify_target(io: &mut impl CaptureIo, target: &ServerIdentity) -> Result<(), String> {
    if io.identity().await? != *target {
        return Err(
            "The managed server changed during capture. No further profiler commands were sent."
                .into(),
        );
    }
    io.verify_endpoint(target).await?;
    if io.identity().await? != *target {
        return Err("The managed server changed during RCON verification.".into());
    }
    Ok(())
}

async fn recording_status(
    io: &mut impl CaptureIo,
    target: &ServerIdentity,
) -> Result<RecordingStatus, String> {
    parse_recording_status(&checked_command(io, target, "profiler status".into()).await?)
}

fn check_abort(io: &impl CaptureIo, cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        return Err(CANCEL_MESSAGE.into());
    }
    if io.elapsed() >= CAPTURE_TIMEOUT {
        return Err("Profiler capture exceeded the 45-second wait limit. No stop was sent; the frame-bounded recording ends as the server advances.".into());
    }
    Ok(())
}

async fn run_capture(
    io: &mut impl CaptureIo,
    manager: &ProfilerCaptureManager,
    id: &str,
    frames: u32,
    cancel: &AtomicBool,
) -> Result<ProfilerCaptureResult, String> {
    let target = io.identity().await?;
    check_abort(io, cancel)?;
    if recording_status(io, &target).await? != RecordingStatus::Idle {
        return Err("A manual or other profiler recording is already active. Finish and save it before starting a guided capture.".into());
    }
    check_abort(io, cancel)?;
    let mut file = CaptureFile::reserve(&target.root, id)?;
    let save_command = file.save_command()?;
    let mut remaining = frames;
    let mut export_pending = false;
    let result = async {
        verify_target(io, &target).await?;
        check_abort(io, cancel)?;
        let response = io.command(format!("profiler record {frames}")).await?;
        if has_profiler_line(&response, "A recording is already taking place") {
            return Err("Another profiler recording started first. It was left untouched.".into());
        }
        if !has_profiler_line(&response, "Started recording") {
            return Err("FXServer did not confirm the capture started. It may still finish its bounded frame request; no stop was sent because ownership is uncertain.".into());
        }
        manager.progress("recording", 0);
        loop {
            check_abort(io, cancel)?;
            match recording_status(io, &target).await? {
                RecordingStatus::Idle => break,
                RecordingStatus::Recording(Some(left)) if left > 0 && left <= remaining => {
                    remaining = left;
                    manager.progress("recording", frames.saturating_sub(left));
                }
                _ => {
                    return Err("Profiler recording changed outside the app. It was left untouched.".into());
                }
            }
            io.wait().await;
        }
        check_abort(io, cancel)?;
        manager.progress("saving", frames);
        if recording_status(io, &target).await? != RecordingStatus::Idle {
            return Err("Another recording began before export. It was left untouched.".into());
        }
        check_abort(io, cancel)?;
        // Sending can succeed even if acknowledgement fails. Always finish the bounded export wait.
        verify_target(io, &target).await?;
        check_abort(io, cancel)?;
        export_pending = true;
        let response = io.command(save_command).await;
        let deadline = io.elapsed() + SAVE_TIMEOUT;
        let last_error = "Waiting for a complete profiler JSON export.".to_string();
        loop {
            if cancel.load(Ordering::SeqCst) { manager.progress("cleaning", frames); }
            if file.writer_finished() {
                // Cfx opens/truncates before conversion, writes once, then closes. Denying write
                // sharing and observing nonempty output proves that writer has relinquished it.
                export_pending = false;
                if cancel.load(Ordering::SeqCst) { return Err(CANCEL_MESSAGE.into()); }
                if io.identity().await? != target { return Err("The managed server changed before the capture could be read.".into()); }
                let json = file.read()?;
                return Ok(ProfilerCaptureResult { id: id.into(), file_name: file.name.clone(), frames, json });
            }
            if io.elapsed() >= deadline {
                if cancel.load(Ordering::SeqCst) { return Err(CANCEL_MESSAGE.into()); }
                return Err(format!("Profiler export failed: {} The artifact must support profiler saveJSON.", response.as_ref().err().unwrap_or(&last_error)));
            }
            io.wait().await;
        }
    }.await;

    manager.progress("cleaning", frames.saturating_sub(remaining));
    let mut warnings = Vec::new();
    // Cfx exposes no recording identity. Cancellation/timeout must never stop a replacement.
    if export_pending {
        warnings.push(format!("Writer completion is unknown. {} remains protected in the managed artifact directory; cleanup will be retried on the next capture request after the writer finishes or the managed server exits. No background cleanup is running.", file.name));
        *manager
            .1
            .lock()
            .map_err(|_| "Profiler cleanup state is unavailable.")? =
            Some(RetainedExport { file, target });
        return Err(format!(
            "{} {}",
            result
                .err()
                .unwrap_or_else(|| "Profiler export did not complete.".into()),
            warnings.join(" ")
        ));
    }
    let cleanup_deadline = io.elapsed() + Duration::from_secs(2);
    loop {
        match file.remove() {
            Ok(()) => break,
            Err(error) if io.elapsed() >= cleanup_deadline => {
                warnings.push(format!("Capture cleanup failed for {}: {error}", file.name));
                break;
            }
            Err(_) => io.wait().await,
        }
    }
    match (result, warnings.is_empty()) {
        (result, true) => result,
        (Ok(_), false) => Err(warnings.join(" ")),
        (Err(error), false) => Err(format!("{error} {}", warnings.join(" "))),
    }
}

struct CaptureFile {
    root: PathBuf,
    name: String,
    identity: (u64, u64, u64),
    reserved: Option<File>,
    _directory_pins: Vec<File>,
}

impl CaptureFile {
    fn reserve(root: &Path, id: &str) -> Result<Self, String> {
        if !id.starts_with("fxi-profiler-")
            || !id
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-')
        {
            return Err("Invalid generated profiler capture identifier.".into());
        }
        let directory_pins = storage::pin_directories(root)?;
        let root =
            fs::canonicalize(root).map_err(|_| "Managed capture directory is unavailable.")?;
        let name = format!("{id}.json");
        let path = root.join(&name);
        quoted_capture_path(&path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
            options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
        }
        let created = options.open(&path).map_err(|_| {
            "Could not reserve a new capture file in the managed server working directory."
        })?;
        let identity = verify_capture_handle(&created, &path)?;
        // Cfx Create requests READ|WRITE but shares only READ. Keep a read-only reservation.
        let reserved = open_capture_file(&path, true)?;
        if verify_capture_handle(&reserved, &path)? != identity {
            return Err("Generated capture file changed while reserving.".into());
        }
        drop(created);
        Ok(Self {
            root,
            name,
            identity,
            reserved: Some(reserved),
            _directory_pins: directory_pins,
        })
    }

    fn path(&self) -> PathBuf {
        self.root.join(&self.name)
    }

    fn save_command(&self) -> Result<String, String> {
        Ok(format!(
            "profiler saveJSON {}",
            quoted_capture_path(&self.path())?
        ))
    }

    fn open(&self) -> Result<File, String> {
        let path = self.path();
        let file = open_capture_file(&path, false)?;
        if verify_capture_handle(&file, &path)? != self.identity {
            return Err("Generated capture file was replaced; reading refused.".into());
        }
        Ok(file)
    }

    fn writer_finished(&self) -> bool {
        self.open()
            .and_then(|file| {
                file.metadata()
                    .map_err(|_| "Capture metadata is unavailable.".into())
            })
            .is_ok_and(|metadata| metadata.len() > 0)
    }

    fn read(&self) -> Result<String, String> {
        let file = self.open()?;
        if file
            .metadata()
            .map_err(|_| "Capture metadata is unavailable.")?
            .len()
            > MAX_CAPTURE_BYTES
        {
            return Err("Capture exceeds the 16 MiB limit. Choose fewer frames.".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_CAPTURE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Capture could not be read.")?;
        if bytes.len() as u64 > MAX_CAPTURE_BYTES {
            return Err("Capture exceeds the 16 MiB limit. Choose fewer frames.".into());
        }
        let json = String::from_utf8(bytes).map_err(|_| "Capture is not UTF-8 JSON.")?;
        let parsed: serde_json::Value =
            serde_json::from_str(&json).map_err(|_| "Capture JSON is incomplete or invalid.")?;
        if !parsed.get("traceEvents").is_some_and(|events| {
            events.is_array() && events.as_array().is_some_and(|events| !events.is_empty())
        }) {
            return Err(
                "Capture has no traceEvents; this artifact may not support profiler saveJSON."
                    .into(),
            );
        }
        Ok(json)
    }

    fn remove(&mut self) -> Result<(), String> {
        if self.reserved.is_some() {
            // Do not release the non-delete-sharing guard while a writer still owns the file.
            let snapshot = self.open()?;
            self.reserved.take();
            drop(snapshot);
        }
        if self
            .path()
            .try_exists()
            .map_err(|_| "Capture path is unavailable.")?
        {
            self.remove_verified()?;
        }
        Ok(())
    }

    #[cfg(windows)]
    fn remove_verified(&self) -> Result<(), String> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
        };
        // The shared directory pins stay alive; deletion targets this verified handle, not its name.
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | DELETE)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(self.path())
            .map_err(|_| "Could not lock generated capture for cleanup.")?;
        if verify_capture_handle(&file, &self.path())? != self.identity {
            return Err("Generated capture file was replaced; cleanup refused.".into());
        }
        delete_capture_handle(&file)
    }

    #[cfg(not(windows))]
    fn remove_verified(&self) -> Result<(), String> {
        Err("Safe profiler capture cleanup requires Windows.".into())
    }
}

fn quoted_capture_path(path: &Path) -> Result<String, String> {
    let value = path
        .to_str()
        .ok_or("Profiler export path must be valid Unicode.")?;
    let value = value
        .strip_prefix(r"\\?\")
        .unwrap_or(value)
        .replace('\\', "/");
    let bytes = value.as_bytes();
    if bytes.len() > 900
        || bytes.len() < 4
        || !bytes[0].is_ascii_alphabetic()
        || &bytes[1..3] != b":/"
        || value[2..].contains(':')
        || value.chars().any(|ch| {
            ch.is_control()
                || matches!(
                    ch,
                    '"' | '\''
                        | ';'
                        | '`'
                        | '$'
                        | '|'
                        | '&'
                        | '<'
                        | '>'
                        | '*'
                        | '?'
                        | '#'
                        | '{'
                        | '}'
                )
        })
        || value.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(
            "Profiler export requires a safe absolute local drive path (no command characters)."
                .into(),
        );
    }
    // Cfx Console::Tokenize supports double-quoted strings; VFS falls back to LocalDevice.
    Ok(format!("\"{value}\""))
}

fn open_capture_file(path: &Path, allow_writer: bool) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        options
            .share_mode(FILE_SHARE_READ | if allow_writer { FILE_SHARE_WRITE } else { 0 })
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(not(windows))]
    let _ = allow_writer;
    let file = options
        .open(path)
        .map_err(|_| "Capture is unavailable or its writer is still active.")?;
    if !file
        .metadata()
        .map_err(|_| "Capture metadata is unavailable.")?
        .is_file()
    {
        return Err("Capture must be a regular file.".into());
    }
    Ok(file)
}

#[cfg(windows)]
fn delete_capture_handle(file: &File) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
    };
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            &disposition as *const _ as *const _,
            std::mem::size_of_val(&disposition) as u32,
        )
    } == 0
    {
        return Err(
            "Could not remove verified profiler output; its writer may still be active.".into(),
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn delete_capture_handle(_: &File) -> Result<(), String> {
    Err("Safe profiler capture cleanup requires Windows.".into())
}

#[cfg(windows)]
fn verify_capture_handle(file: &File, expected: &Path) -> Result<(u64, u64, u64), String> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, GetFinalPathNameByHandleW, BY_HANDLE_FILE_INFORMATION,
        FILE_ATTRIBUTE_REPARSE_POINT,
    };
    let handle = file.as_raw_handle();
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0
        || info.nNumberOfLinks != 1
        || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err("Capture links and reparse points are not allowed.".into());
    }
    let mut buffer = vec![0u16; 32768];
    let length =
        unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, 0) }
            as usize;
    if length == 0
        || length >= buffer.len()
        || std::ffi::OsString::from_wide(&buffer[..length]) != expected.as_os_str()
    {
        return Err("Capture handle is outside the managed output path.".into());
    }
    Ok((
        info.dwVolumeSerialNumber as u64,
        info.nFileIndexHigh as u64,
        info.nFileIndexLow as u64,
    ))
}

#[cfg(not(windows))]
fn verify_capture_handle(file: &File, expected: &Path) -> Result<(u64, u64, u64), String> {
    use std::os::unix::fs::MetadataExt;
    let actual = file
        .metadata()
        .map_err(|_| "Capture metadata is unavailable.")?;
    let named = fs::symlink_metadata(expected).map_err(|_| "Capture path is unavailable.")?;
    if !actual.is_file()
        || actual.nlink() != 1
        || named.file_type().is_symlink()
        || actual.ino() != named.ino()
        || actual.dev() != named.dev()
    {
        return Err("Capture links and replaced files are not allowed.".into());
    }
    Ok((actual.dev(), actual.ino(), 0))
}

#[cfg(test)]
#[path = "profiler_capture_tests.rs"]
mod tests;
