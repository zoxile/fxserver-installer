use super::*;
use std::collections::VecDeque;

fn cmd_response(response: &str) -> String {
    response
        .lines()
        .map(|line| {
            if line.starts_with('[') {
                line.to_string()
            } else {
                format!("[cmd] {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cfx_write(path: &Path, content: &str) {
    use std::io::Write;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
        options.share_mode(FILE_SHARE_READ);
    }
    options
        .open(path)
        .unwrap()
        .write_all(content.as_bytes())
        .unwrap();
}

const TRACE: &str =
    r#"{"traceEvents":[{"name":"Resource Manager Tick","ph":"X","ts":0,"dur":1200}]}"#;

#[cfg(target_os = "linux")]
#[test]
fn linux_capture_observes_writer_close_not_just_nonempty_bytes() {
    use std::io::Write;
    let fixture = Fixture::new();
    let mut file = CaptureFile::reserve(&fixture.0, &capture_id()).unwrap();
    let mut writer = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(file.path())
        .unwrap();
    writer.write_all(TRACE.as_bytes()).unwrap();
    assert!(!file.writer_finished());
    drop(writer);
    assert!(file.writer_finished());
    assert_eq!(file.read().unwrap(), TRACE);
    assert!(file.writer_finished());
    file.remove().unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn linux_export_targets_reserved_inode_and_preserves_replacement() {
    use std::os::unix::fs::symlink;
    for hardlink in [false, true] {
        let fixture = Fixture::new();
        let mut file = CaptureFile::reserve(&fixture.0, &capture_id()).unwrap();
        let command = file.save_command().unwrap();
        let descriptor = Path::new(
            command
                .strip_prefix("profiler saveJSON \"")
                .unwrap()
                .strip_suffix('"')
                .unwrap(),
        );
        fs::rename(file.path(), fixture.0.join("reserved.json")).unwrap();
        let unrelated = fixture.0.join("unrelated.json");
        fs::write(&unrelated, "keep").unwrap();
        if hardlink {
            fs::hard_link(&unrelated, file.path()).unwrap();
        } else {
            symlink(&unrelated, file.path()).unwrap();
        }
        cfx_write(descriptor, TRACE);
        assert_eq!(fs::read_to_string(&unrelated).unwrap(), "keep");
        assert_eq!(
            fs::read_to_string(fixture.0.join("reserved.json")).unwrap(),
            TRACE
        );
        assert!(file.read().is_err());
        assert!(file.remove().is_err());
        assert!(file.reserved.is_some());
        assert!(file.path().symlink_metadata().is_ok());
    }
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(capture_id());
        fs::create_dir(&root).unwrap();
        Self(fs::canonicalize(root).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("fxi-profiler-"));
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct FakeIo {
    target: ServerIdentity,
    steps: VecDeque<(&'static str, &'static str)>,
    commands: Vec<String>,
    clock: Duration,
    wait_step: Duration,
    cancel: Arc<AtomicBool>,
    cancel_after: Option<usize>,
    restart_after: Option<usize>,
    output: &'static str,
    endpoint_verified: bool,
    writer_delay: Duration,
    pending_write: Option<(PathBuf, Duration)>,
}
impl FakeIo {
    fn new(root: &Path, steps: &[(&'static str, &'static str)]) -> Self {
        Self {
            target: ServerIdentity {
                pid: 17,
                started_at: "fixture".into(),
                root: root.into(),
            },
            steps: steps.iter().copied().collect(),
            commands: Vec::new(),
            clock: Duration::ZERO,
            wait_step: Duration::from_millis(250),
            cancel: Arc::new(AtomicBool::new(false)),
            cancel_after: None,
            restart_after: None,
            output: TRACE,
            endpoint_verified: true,
            writer_delay: Duration::ZERO,
            pending_write: None,
        }
    }
}
impl CaptureIo for FakeIo {
    async fn verify_endpoint(&mut self, _: &ServerIdentity) -> Result<(), String> {
        if self.endpoint_verified {
            Ok(())
        } else {
            Err("RCON endpoint was not verified.".into())
        }
    }
    async fn identity(&mut self) -> Result<ServerIdentity, String> {
        let mut target = self.target.clone();
        if self
            .restart_after
            .is_some_and(|count| self.commands.len() >= count)
        {
            target.pid += 1;
        }
        Ok(target)
    }
    async fn command(&mut self, command: String) -> Result<String, String> {
        let (expected, response) = self
            .steps
            .pop_front()
            .expect("Unexpected command; tests never contact a server");
        if expected == "save" {
            let path = PathBuf::from(
                command
                    .strip_prefix("profiler saveJSON \"")
                    .unwrap()
                    .strip_suffix('"')
                    .unwrap(),
            );
            assert!(path.is_absolute());
            // Linux exports through the app's reserved descriptor, never a replaceable path.
            let output = fs::canonicalize(&path).unwrap();
            assert_eq!(
                fs::canonicalize(output.parent().unwrap()).unwrap(),
                self.target.root
            );
            assert!(output
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("fxi-profiler-"));
            if self.writer_delay.is_zero() {
                cfx_write(&path, self.output);
            } else {
                self.pending_write = Some((path, self.clock + self.writer_delay));
            }
        } else {
            assert_eq!(command, expected);
        }
        self.commands.push(command);
        if self.cancel_after == Some(self.commands.len()) {
            self.cancel.store(true, Ordering::SeqCst);
        }
        Ok(cmd_response(response))
    }
    async fn wait(&mut self) {
        self.clock += self.wait_step;
        if self
            .pending_write
            .as_ref()
            .is_some_and(|(_, ready)| self.clock >= *ready)
        {
            let (path, _) = self.pending_write.take().unwrap();
            cfx_write(&path, self.output);
        }
    }
    fn elapsed(&self) -> Duration {
        self.clock
    }
}

async fn run(io: &mut FakeIo) -> Result<ProfilerCaptureResult, String> {
    run_with_manager(io, &ProfilerCaptureManager::default()).await
}

async fn run_with_manager(
    io: &mut FakeIo,
    manager: &ProfilerCaptureManager,
) -> Result<ProfilerCaptureResult, String> {
    let cancel = io.cancel.clone();
    run_capture(io, manager, &capture_id(), 100, &cancel).await
}

fn status_for(io: &FakeIo, running: bool) -> FxserverStatus {
    FxserverStatus {
        running,
        pid: running.then_some(io.target.pid),
        started_at: running.then(|| io.target.started_at.clone()),
        artifact_path: Some(io.target.root.to_string_lossy().into_owned()),
        uptime_seconds: None,
        resources: None,
    }
}

fn export_steps() -> Vec<(&'static str, &'static str)> {
    vec![
        ("profiler status", "Recording: No"),
        ("profiler record 100", "Started recording"),
        ("profiler status", "Recording: No"),
        ("profiler status", "Recording: No"),
        ("save", ""),
    ]
}

#[test]
fn forged_channels_stderr_multiline_and_body_substrings_are_not_acknowledgements() {
    for body in ["Recording: No", "Started recording", "Save complete"] {
        for (stream, line) in [
            ("stdout", format!("[script:untrusted] {body}")),
            ("stdout", format!("[rcon/script:untrusted] {body}")),
            ("stderr", format!("[cmd] {body}")),
            ("rcon", format!("[cmd] {body}")),
            ("stdout", body.to_string()),
            (
                "stdout",
                format!("[script:untrusted] message\n[cmd] {body}"),
            ),
        ] {
            let mut collected = String::new();
            append_profiler_response(&mut collected, stream, &line).unwrap();
            assert!(collected.is_empty(), "{stream}: {line}");
        }
        assert!(!has_profiler_line(
            &format!("[cmd] resource says {body}"),
            body
        ));
        assert!(has_profiler_line(
            &format!("[                 cmd] {body}"),
            body
        ));
        assert!(has_profiler_line(
            &format!("[            rcon/cmd] {body}"),
            body
        ));
    }
    assert!(parse_recording_status("[script:other] Recording: No").is_err());
    assert_eq!(
        parse_recording_status("[script:other] Recording: No\n[cmd] Recording: Yes (20)").unwrap(),
        RecordingStatus::Recording(Some(20))
    );
}

#[test]
fn response_collection_has_byte_and_line_limits() {
    let mut response = String::new();
    append_profiler_response(
        &mut response,
        "stdout",
        &format!("[cmd] {}", "x".repeat(MAX_RESPONSE_LINE_BYTES)),
    )
    .unwrap();
    assert!(response.is_empty());
    let line = format!("[cmd] {}", "x".repeat(900));
    for _ in 0..4 {
        append_profiler_response(&mut response, "stdout", &line).unwrap();
    }
    assert!(append_profiler_response(&mut response, "stdout", &line).is_err());
    assert!(response.len() <= MAX_RESPONSE_BYTES);
    assert!(parse_recording_status(&"x".repeat(MAX_RESPONSE_BYTES + 1)).is_err());
}

#[test]
fn absolute_save_path_quotes_spaces_and_rejects_command_characters() {
    #[cfg(windows)]
    assert_eq!(
        quoted_capture_path(Path::new(
            r"\\?\C:\FX Server\artifact\fxi-profiler-test.json"
        ))
        .unwrap(),
        "\"C:/FX Server/artifact/fxi-profiler-test.json\""
    );
    #[cfg(target_os = "linux")]
    {
        assert_eq!(
            quoted_capture_path(Path::new("/home/user/FX Server/capture.json")).unwrap(),
            "\"/home/user/FX Server/capture.json\""
        );
        for path in [
            "/tmp/../capture.json",
            "/tmp/a;quit/x.json",
            "/tmp/a\\b/x.json",
            "/tmp/a\"b/x.json",
            "//host/share/x.json",
        ] {
            assert!(quoted_capture_path(Path::new(path)).is_err());
        }
    }
    for path in [
        "relative.json",
        "C:/safe;quit/x.json",
        "C:/bad\nquit/x.json",
        "C:/bad\"quit/x.json",
        "C:/bad&quit/x.json",
        "C:/bad|quit/x.json",
        "C:/bad`quit/x.json",
        "C:/bad$quit/x.json",
        "C:/capture.json:stream",
        "C:/../capture.json",
        r"\\host\share\capture.json",
    ] {
        assert!(quoted_capture_path(Path::new(path)).is_err(), "{path}");
    }
}

#[tokio::test]
async fn capture_with_spaces_exports_to_absolute_artifact_root_not_child_cwd() {
    let fixture = Fixture::new();
    let root = fixture.0.join("Artifact With Spaces");
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let mut io = FakeIo::new(&root, &export_steps());
    let result = run(&mut io).await.unwrap();
    assert_eq!(result.json, TRACE);
    let command = io.commands.last().unwrap();
    assert!(command.starts_with("profiler saveJSON \""));
    #[cfg(windows)]
    assert!(command.contains("Artifact With Spaces/"));
    #[cfg(target_os = "linux")]
    assert!(command.contains(&format!("/proc/{}/fd/", std::process::id())));
    assert!(!root.join(result.file_name).exists());
}

#[tokio::test]
async fn cancellation_during_delayed_export_waits_and_removes_closed_output() {
    let fixture = Fixture::new();
    let mut io = FakeIo::new(&fixture.0, &export_steps());
    io.writer_delay = Duration::from_secs(3);
    io.cancel_after = Some(5);
    let error = run(&mut io).await.err().unwrap();
    assert!(error.starts_with("Profiler capture cancelled."));
    assert!(io.clock >= Duration::from_secs(3));
    assert!(io.pending_write.is_none());
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
    assert_eq!(io.commands.len(), 5);
}

#[tokio::test]
async fn unknown_writer_retains_one_protected_file_and_retry_cleans_after_late_write() {
    let fixture = Fixture::new();
    let manager = ProfilerCaptureManager::default();
    let mut io = FakeIo::new(&fixture.0, &export_steps());
    io.writer_delay = SAVE_TIMEOUT + Duration::from_secs(2);
    io.cancel_after = Some(5);
    let error = run_with_manager(&mut io, &manager).await.err().unwrap();
    assert!(error.starts_with("Profiler capture cancelled."));
    assert!(error.contains("remains protected"));
    assert!(error.contains("No background cleanup"));
    assert_eq!(io.clock, SAVE_TIMEOUT);
    let path = fs::canonicalize(&io.pending_write.as_ref().unwrap().0).unwrap();
    assert!(path.exists());
    #[cfg(windows)]
    assert!(fs::remove_file(&path).is_err());
    manager
        .retry_retained_cleanup(&status_for(&io, true))
        .unwrap();
    assert!(manager.begin(100).is_err());
    while io.pending_write.is_some() {
        io.wait().await;
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), TRACE);
    manager
        .retry_retained_cleanup(&status_for(&io, true))
        .unwrap();
    assert!(!path.exists());
    assert!(manager.begin(100).is_ok());
}

#[tokio::test]
async fn unknown_writer_cleanup_is_allowed_after_original_managed_process_exit() {
    let fixture = Fixture::new();
    let manager = ProfilerCaptureManager::default();
    let mut io = FakeIo::new(&fixture.0, &export_steps());
    io.writer_delay = Duration::from_secs(100);
    let error = run_with_manager(&mut io, &manager).await.err().unwrap();
    assert!(error.contains("Writer completion is unknown"));
    manager
        .retry_retained_cleanup(&status_for(&io, false))
        .unwrap();
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
    assert!(manager.1.lock().unwrap().is_none());
}

#[cfg(windows)]
#[test]
fn cfx_writer_sharing_is_compatible_but_snapshot_and_cleanup_wait_for_close() {
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
    let fixture = Fixture::new();
    let mut file = CaptureFile::reserve(&fixture.0, &capture_id()).unwrap();
    let mut writer = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .share_mode(FILE_SHARE_READ)
        .open(file.path())
        .unwrap();
    writer.write_all(TRACE.as_bytes()).unwrap();
    assert!(!file.writer_finished());
    assert!(file.read().is_err());
    assert!(file.remove().is_err());
    assert!(file.reserved.is_some());
    drop(writer);
    assert!(file.writer_finished());
    assert_eq!(file.read().unwrap(), TRACE);
    file.remove().unwrap();
}

#[cfg(windows)]
#[test]
fn identity_check_still_rejects_replacement_during_cleanup_handoff() {
    let fixture = Fixture::new();
    let mut file = CaptureFile::reserve(&fixture.0, &capture_id()).unwrap();
    file.reserved.take();
    fs::rename(file.path(), fixture.0.join("original.json")).unwrap();
    let unrelated = fixture.0.join("unrelated.json");
    fs::write(&unrelated, TRACE).unwrap();
    fs::hard_link(&unrelated, file.path()).unwrap();
    assert!(file.read().is_err());
    assert!(file.remove().is_err());
    assert_eq!(fs::read_to_string(unrelated).unwrap(), TRACE);
}

#[test]
fn only_short_local_requests_are_allowed() {
    let mut request = ProfilerCaptureRequest {
        frames: 100,
        rcon: FxserverRconConfig {
            host: "127.0.0.1".into(),
            port: 30120,
            password: "secret".into(),
        },
    };
    for frames in [100, 250, 500] {
        request.frames = frames;
        assert!(validate_request(&request).is_ok());
    }
    for frames in [0, 1, 99, 501, u32::MAX] {
        request.frames = frames;
        assert!(validate_request(&request).is_err());
    }
    request.frames = 100;
    for host in ["example.com", "127.0.0.2", "127.0.0.1; quit", "::1"] {
        request.rcon.host = host.into();
        assert!(validate_request(&request).is_err());
    }
    request.rcon.host = "localhost".into();
    request.rcon.password = "secret\nquit".into();
    assert!(validate_request(&request).is_err());
    assert!(serde_json::from_str::<ProfilerCaptureRequest>(r#"{"frames":100,"rcon":{"host":"localhost","port":30120,"password":"secret"},"path":"C:/private.json"}"#).is_err());
}

#[test]
fn cfx_status_is_fail_closed_and_understands_frame_counts() {
    assert_eq!(
        parse_recording_status(
            "[                 cmd] Recording: Yes (123)\nBuffer: 70 events over 40 frames"
        )
        .unwrap(),
        RecordingStatus::Recording(Some(123))
    );
    assert_eq!(
        parse_recording_status("[cmd] Recording: Yes").unwrap(),
        RecordingStatus::Recording(None)
    );
    assert_eq!(
        parse_recording_status("[cmd] Recording: No").unwrap(),
        RecordingStatus::Idle
    );
    for response in [
        "",
        "No such command profiler",
        "Recording: Maybe",
        "Recording: Yes (-1)",
        "Recording: Yes\nRecording: No",
        "rcon> echo Recording: No",
        "[script:other] Recording: No",
        "[cmd-fake] Recording: No",
        "Recording: No",
        "[cmd] Recording: Yes\n[cmd] Recording: No",
    ] {
        assert!(parse_recording_status(response).is_err(), "{response}");
    }
}

#[test]
fn only_one_capture_can_hold_the_lease() {
    let manager = ProfilerCaptureManager::default();
    let (first, _) = manager.begin(100).unwrap();
    assert!(manager.begin(250).is_err());
    drop(CaptureLease(manager.clone()));
    let (second, _) = manager.begin(250).unwrap();
    assert_ne!(first, second);
}

#[tokio::test]
async fn capture_exports_real_trace_then_cleans_only_generated_file() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("manual.json"), "keep this").unwrap();
    let mut io = FakeIo::new(
        &fixture.0,
        &[
            ("profiler status", "Recording: No"),
            ("profiler record 100", "Started recording"),
            ("profiler status", "Recording: Yes (98)"),
            ("profiler status", "Recording: No"),
            ("profiler status", "Recording: No"),
            ("save", "Save complete"),
        ],
    );
    let result = run(&mut io).await.unwrap();
    assert_eq!(result.json, TRACE);
    assert_eq!(result.frames, 100);
    assert!(!fixture.0.join(result.file_name).exists());
    assert_eq!(
        fs::read_to_string(fixture.0.join("manual.json")).unwrap(),
        "keep this"
    );
    assert!(io.steps.is_empty());
}

#[tokio::test]
async fn active_manual_or_unknown_profiler_status_never_records_or_stops() {
    for status in [
        "Recording: Yes",
        "Recording: Yes (200)",
        "No such command profiler",
        "[script:other] Recording: No",
        "[rcon/script:other] Recording: No",
        "",
    ] {
        let fixture = Fixture::new();
        let mut io = FakeIo::new(&fixture.0, &[("profiler status", status)]);
        assert!(run(&mut io).await.is_err());
        assert_eq!(io.commands, ["profiler status"]);
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn manual_recording_race_and_unconfirmed_start_are_never_stopped() {
    for response in [
        "A recording is already taking place",
        "",
        "No such command profiler",
        "[script:other] Started recording",
        "[rcon/script:other] Started recording",
        "[cmd] unrelated message: Started recording",
    ] {
        let fixture = Fixture::new();
        let mut io = FakeIo::new(
            &fixture.0,
            &[
                ("profiler status", "Recording: No"),
                ("profiler record 100", response),
            ],
        );
        assert!(run(&mut io).await.is_err());
        assert_eq!(io.commands.len(), 2);
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn cancellation_never_stops_a_started_recording_and_cleans_reservation() {
    let fixture = Fixture::new();
    let mut io = FakeIo::new(
        &fixture.0,
        &[
            ("profiler status", "Recording: No"),
            ("profiler record 100", "Started recording"),
        ],
    );
    io.cancel_after = Some(2);
    assert!(run(&mut io)
        .await
        .err()
        .unwrap()
        .starts_with("Profiler capture cancelled."));
    assert!(io.steps.is_empty());
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[tokio::test]
async fn cancellation_before_start_does_not_touch_profiler() {
    let fixture = Fixture::new();
    let mut io = FakeIo::new(&fixture.0, &[]);
    io.cancel.store(true, Ordering::SeqCst);
    assert!(run(&mut io).await.is_err());
    assert!(io.commands.is_empty());
}

#[tokio::test]
async fn stalled_capture_times_out_without_sending_stop() {
    let fixture = Fixture::new();
    let mut io = FakeIo::new(
        &fixture.0,
        &[
            ("profiler status", "Recording: No"),
            ("profiler record 100", "Started recording"),
            ("profiler status", "Recording: Yes (98)"),
        ],
    );
    io.wait_step = Duration::from_secs(46);
    assert!(run(&mut io).await.err().unwrap().contains("45-second"));
    assert!(io.steps.is_empty());
}

#[tokio::test]
async fn server_restart_prevents_commands_to_replacement_process() {
    let fixture = Fixture::new();
    let mut io = FakeIo::new(
        &fixture.0,
        &[
            ("profiler status", "Recording: No"),
            ("profiler record 100", "Started recording"),
        ],
    );
    io.restart_after = Some(2);
    assert!(run(&mut io).await.err().unwrap().contains("server changed"));
    assert_eq!(io.commands.len(), 2);
}

#[tokio::test]
async fn replacement_recording_is_not_saved_or_stopped() {
    let fixture = Fixture::new();
    let mut io = FakeIo::new(
        &fixture.0,
        &[
            ("profiler status", "Recording: No"),
            ("profiler record 100", "Started recording"),
            ("profiler status", "Recording: Yes (500)"),
        ],
    );
    assert!(run(&mut io)
        .await
        .err()
        .unwrap()
        .contains("outside the app"));
    assert_eq!(io.commands.len(), 3);
}

#[tokio::test]
async fn new_manual_recording_before_save_is_left_alone() {
    let fixture = Fixture::new();
    let mut io = FakeIo::new(
        &fixture.0,
        &[
            ("profiler status", "Recording: No"),
            ("profiler record 100", "Started recording"),
            ("profiler status", "Recording: No"),
            ("profiler status", "Recording: Yes"),
        ],
    );
    assert!(run(&mut io).await.err().unwrap().contains("before export"));
    assert_eq!(io.commands.len(), 4);
}

#[tokio::test]
async fn partial_export_has_bounded_wait_and_cleanup() {
    let fixture = Fixture::new();
    let mut io = FakeIo::new(
        &fixture.0,
        &[
            ("profiler status", "Recording: No"),
            ("profiler record 100", "Started recording"),
            ("profiler status", "Recording: No"),
            ("profiler status", "Recording: No"),
            ("save", ""),
        ],
    );
    io.output = "{\"traceEvents\":[";
    assert!(run(&mut io)
        .await
        .err()
        .unwrap()
        .contains("incomplete or invalid"));
    assert!(io.clock <= SAVE_TIMEOUT + Duration::from_secs(1));
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[tokio::test]
async fn cancellation_during_export_removes_output_without_stopping_a_new_recording() {
    let fixture = Fixture::new();
    let mut io = FakeIo::new(
        &fixture.0,
        &[
            ("profiler status", "Recording: No"),
            ("profiler record 100", "Started recording"),
            ("profiler status", "Recording: No"),
            ("profiler status", "Recording: No"),
            ("save", "Save complete"),
        ],
    );
    io.cancel_after = Some(5);
    assert!(run(&mut io)
        .await
        .err()
        .unwrap()
        .starts_with("Profiler capture cancelled."));
    assert_eq!(io.commands.len(), 5);
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[tokio::test]
async fn cancelling_a_smaller_replacement_never_sends_stop_or_export() {
    let fixture = Fixture::new();
    let mut io = FakeIo::new(
        &fixture.0,
        &[
            ("profiler status", "Recording: No"),
            ("profiler record 100", "Started recording"),
            ("profiler status", "Recording: Yes (98)"),
            ("profiler status", "Recording: Yes (10)"),
        ],
    );
    io.cancel_after = Some(4);
    let error = run(&mut io).await.err().unwrap();
    assert!(error.starts_with("Profiler capture cancelled."));
    assert!(error.contains("No profiler stop was sent"));
    assert_eq!(io.commands.len(), 4);
    assert!(io.steps.is_empty());
}

#[tokio::test]
async fn asynchronous_save_without_rcon_ack_waits_for_writer_completion() {
    let fixture = Fixture::new();
    let mut io = FakeIo::new(
        &fixture.0,
        &[
            ("profiler status", "Recording: No"),
            ("profiler record 100", "Started recording"),
            ("profiler status", "Recording: No"),
            ("profiler status", "Recording: No"),
            ("save", ""),
        ],
    );
    io.writer_delay = Duration::from_secs(2);
    assert_eq!(run(&mut io).await.unwrap().json, TRACE);
    assert!(io.clock >= Duration::from_secs(2));
}

#[test]
fn generated_files_reject_traversal_collisions_and_oversize_reads() {
    let fixture = Fixture::new();
    for id in [
        "../secret",
        "fxi-profiler-../secret",
        "fxi-profiler-x:secret",
        "C:\\private",
        "fxi-profiler-x\nquit",
        "fxi-profiler-x;quit",
    ] {
        assert!(CaptureFile::reserve(&fixture.0, id).is_err());
    }
    let id = capture_id();
    let mut file = CaptureFile::reserve(&fixture.0, &id).unwrap();
    assert!(CaptureFile::reserve(&fixture.0, &id).is_err());
    OpenOptions::new()
        .write(true)
        .open(file.path())
        .unwrap()
        .set_len(MAX_CAPTURE_BYTES + 1)
        .unwrap();
    assert!(file.read().unwrap_err().contains("16 MiB"));
    file.remove().unwrap();
    assert!(!file.path().exists());
}

#[test]
#[cfg(windows)]
fn reservation_blocks_replacement_with_a_hardlink_before_cfx_write() {
    let fixture = Fixture::new();
    let unrelated = fixture.0.join("unrelated.json");
    fs::write(&unrelated, TRACE).unwrap();
    let mut file = CaptureFile::reserve(&fixture.0, &capture_id()).unwrap();
    assert!(fs::remove_file(file.path()).is_err());
    assert!(fs::rename(file.path(), fixture.0.join("replaced.json")).is_err());
    assert!(fs::hard_link(&unrelated, file.path()).is_err());
    cfx_write(&file.path(), TRACE);
    assert_eq!(file.read().unwrap(), TRACE);
    file.remove().unwrap();
    assert_eq!(fs::read_to_string(unrelated).unwrap(), TRACE);
}

#[test]
fn malformed_empty_and_non_trace_outputs_are_rejected() {
    let fixture = Fixture::new();
    let file = CaptureFile::reserve(&fixture.0, &capture_id()).unwrap();
    for content in ["", "{", "[]", "{}", "{\"traceEvents\":[]}"] {
        fs::write(file.path(), content).unwrap();
        assert!(file.read().is_err());
    }
    fs::write(file.path(), TRACE).unwrap();
    assert_eq!(file.read().unwrap(), TRACE);
}

#[tokio::test]
async fn unverified_endpoint_never_receives_profiler_commands() {
    let fixture = Fixture::new();
    let mut io = FakeIo::new(&fixture.0, &[]);
    io.endpoint_verified = false;
    assert!(run(&mut io).await.err().unwrap().contains("not verified"));
    assert!(io.commands.is_empty());
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn capture_directory_and_reserved_file_stay_pinned_until_cleanup() {
    let fixture = Fixture::new();
    let directory = fixture.0.join("managed");
    fs::create_dir(&directory).unwrap();
    let mut file = CaptureFile::reserve(&directory, &capture_id()).unwrap();
    assert!(fs::rename(&directory, fixture.0.join("swapped")).is_err());
    assert!(fs::rename(file.path(), directory.join("original.json")).is_err());
    cfx_write(&file.path(), TRACE);
    assert_eq!(file.read().unwrap(), TRACE);
    file.remove().unwrap();
}
