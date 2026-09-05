use super::*;

pub(super) struct Fixture {
    pub(super) root: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "fxserver-diagnostics-test-{}-{}-{}",
            std::process::id(),
            now(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("data/resources")).unwrap();
        fs::create_dir_all(root.join("artifacts")).unwrap();
        fs::create_dir_all(root.join("txData/default")).unwrap();
        fs::write(
            root.join("artifacts/FXServer.exe"),
            "test fixture, not executable",
        )
        .unwrap();
        fs::write(
            root.join("txData/default/config.json"),
            serde_json::to_vec(
                &json!({ "version": 2, "server": { "dataPath": root.join("data") } }),
            )
            .unwrap(),
        )
        .unwrap();
        fs::write(root.join("data/server.cfg"), "").unwrap();
        Self { root }
    }

    pub(super) fn resource(&self, path: &str, content: &str) {
        let path = self.root.join("data/resources").join(path);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("fxmanifest.lua"), content).unwrap();
    }

    pub(super) fn request(&self) -> PreflightRequest {
        PreflightRequest {
            artifact_path: self.root.join("artifacts").to_string_lossy().into_owned(),
            tx_data_path: self.root.join("txData").to_string_lossy().into_owned(),
            profile: "default".into(),
            credentials: None,
            check_ports: false,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        if self
            .root
            .canonicalize()
            .is_ok_and(|path| path.starts_with(&temp) && path != temp)
        {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

#[test]
fn fresh_txadmin_can_start_without_a_profile_but_invalid_configured_paths_block() {
    let fixture = Fixture::new();
    let mut request = fixture.request();
    request.profile.clear();
    request.tx_data_path.clear();
    let fresh = report(&inspect(&request));
    assert!(!fresh.blocking);
    assert!(fresh
        .checks
        .iter()
        .any(|check| check.code == "profile-not-selected" && check.severity == Severity::Warning));
    request.tx_data_path = fixture
        .root
        .join("does-not-exist")
        .to_string_lossy()
        .into_owned();
    assert!(report(&inspect(&request)).blocking);
}

#[test]
fn nested_profile_groups_execs_aliases_and_virtual_dependencies_are_supported() {
    let fixture = Fixture::new();
    fixture.resource(
        "[core]/qbx_core",
        "provide 'qb-core'\ndependencies { '/server:7290', '/onesync' }",
    );
    fixture.resource("[core]/job", "dependency 'qb-core'");
    fixture.resource("[system]/rconlog", "fx_version 'cerulean'");
    fs::write(
        fixture.root.join("data/server.cfg"),
        "exec misc.cfg\nensure [core]\nensure [system]\nrcon_password \"private-value\"",
    )
    .unwrap();
    fs::write(fixture.root.join("data/misc.cfg"), "# cfg fixture").unwrap();
    let inspection = inspect(&fixture.request());
    let report = report(&inspection);
    assert!(!report.blocking);
    assert_eq!(report.resource_count, 3);
    assert_eq!(report.config_count, 2);
    assert!(report
        .checks
        .iter()
        .any(|check| check.code == "rcon-configured"));
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("private-value"));
}

#[test]
fn missing_started_dependencies_and_duplicates_block_start() {
    let fixture = Fixture::new();
    fixture.resource("[a]/job", "dependencies { 'missing-library', '/onesync' }");
    fixture.resource("[b]/job", "fx_version 'cerulean'");
    fixture.resource("unused", "dependency 'optional-library'");
    fs::write(
        fixture.root.join("data/server.cfg"),
        "ensure job\nensure missing-resource",
    )
    .unwrap();
    let report = report(&inspect(&fixture.request()));
    assert!(report.blocking);
    assert!(report
        .checks
        .iter()
        .any(|check| check.code == "duplicate-resource"));
    assert!(report
        .checks
        .iter()
        .any(|check| check.code == "configured-resource-missing"));
    assert!(report
        .checks
        .iter()
        .any(|check| check.code == "dependency-missing" && check.severity == Severity::Error));
    assert!(report
        .checks
        .iter()
        .any(|check| check.resource.as_deref() == Some("unused")
            && check.severity == Severity::Warning));
    assert!(!report
        .checks
        .iter()
        .any(|check| check.code == "dependency-missing" && check.detail.contains("/onesync")));
}

#[cfg(windows)]
fn link_directory(target: &Path, link: &Path) {
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command",
            "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:FXSI_TEST_LINK -Target $env:FXSI_TEST_TARGET | Out-Null"])
        .env("FXSI_TEST_LINK", link)
        .env("FXSI_TEST_TARGET", target)
        .no_window()
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
fn link_directory(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[test]
fn linked_resources_and_groups_keep_their_aliases_and_support_resource_configs() {
    let fixture = Fixture::new();
    let shared = fixture.root.join("shared");
    fs::create_dir_all(shared.join("actual-job")).unwrap();
    fs::create_dir_all(shared.join("group/library")).unwrap();
    fs::write(
        shared.join("actual-job/fxmanifest.lua"),
        "dependency 'library'",
    )
    .unwrap();
    fs::write(shared.join("actual-job/settings.cfg"), "ensure library").unwrap();
    fs::write(
        shared.join("group/library/fxmanifest.lua"),
        "fx_version 'cerulean'",
    )
    .unwrap();
    fs::create_dir(fixture.root.join("data/resources/[jobs]")).unwrap();
    link_directory(
        &shared.join("actual-job"),
        &fixture.root.join("data/resources/[jobs]/job"),
    );
    link_directory(
        &shared.join("actual-job"),
        &fixture.root.join("data/resources/second-alias"),
    );
    link_directory(
        &shared.join("group"),
        &fixture.root.join("data/resources/[shared]"),
    );
    fs::write(
        fixture.root.join("data/server.cfg"),
        "exec @job/settings.cfg\nensure [jobs]\nensure [shared]\nensure second-alias",
    )
    .unwrap();

    let inspection = inspect(&fixture.request());
    assert!(!report(&inspection).blocking);
    assert_eq!(inspection.resources.len(), 3);
    assert_eq!(inspection.configs.len(), 2);
    assert!(inspection
        .resources
        .iter()
        .any(|item| item.name == "job" && item.groups == ["[jobs]"]));
    assert!(inspection
        .resources
        .iter()
        .any(|item| item.name == "second-alias"));
    assert!(config_target(
        "@job/../outside.cfg",
        &fixture.root.join("data"),
        &inspection.resources
    )
    .is_none());
    assert!(config_target(
        &shared.join("actual-job/settings.cfg").to_string_lossy(),
        &fixture.root.join("data"),
        &inspection.resources
    )
    .is_none());
}

#[test]
fn linked_resources_root_is_supported_and_cycles_are_not_followed() {
    let fixture = Fixture::new();
    let shared = fixture.root.join("shared");
    fs::create_dir_all(shared.join("job")).unwrap();
    fs::write(shared.join("job/fxmanifest.lua"), "fx_version 'cerulean'").unwrap();
    let resources = fixture.root.join("data/resources");
    fs::remove_dir(&resources).unwrap();
    link_directory(&shared, &resources);
    link_directory(&shared, &shared.join("[cycle]"));
    fs::write(fixture.root.join("data/server.cfg"), "ensure job").unwrap();
    let report = report(&inspect(&fixture.request()));
    assert!(!report.blocking);
    assert_eq!(report.resource_count, 1);
    assert_eq!(
        report
            .checks
            .iter()
            .filter(|check| check.code == "resource-link-cycle")
            .count(),
        1
    );
}

#[test]
fn linked_resource_configs_cannot_escape_their_registered_target() {
    let fixture = Fixture::new();
    let shared = fixture.root.join("shared/job");
    fs::create_dir_all(&shared).unwrap();
    fs::write(shared.join("fxmanifest.lua"), "fx_version 'cerulean'").unwrap();
    fs::create_dir(fixture.root.join("private")).unwrap();
    fs::write(
        fixture.root.join("private/secret.cfg"),
        "ensure outside-secret",
    )
    .unwrap();
    link_directory(&shared, &fixture.root.join("data/resources/job"));
    link_directory(&fixture.root.join("private"), &shared.join("external"));
    fs::write(
        fixture.root.join("data/server.cfg"),
        "ensure job\nexec @job/external/secret.cfg",
    )
    .unwrap();
    let report = report(&inspect(&fixture.request()));
    assert!(!report.blocking);
    assert_eq!(report.config_count, 1);
    assert!(report
        .checks
        .iter()
        .any(|check| check.code == "exec-unresolved"));
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("outside-secret"));
}

#[cfg(windows)]
#[test]
#[ignore = "Requires Windows Developer Mode or symbolic-link privilege."]
fn directory_symlink_is_detected_as_a_resource() {
    let fixture = Fixture::new();
    let shared = fixture.root.join("shared");
    fs::create_dir(&shared).unwrap();
    fs::write(shared.join("fxmanifest.lua"), "fx_version 'cerulean'").unwrap();
    let link = fixture.root.join("data/resources/job");
    std::os::windows::fs::symlink_dir(&shared, &link).unwrap();
    fs::write(fixture.root.join("data/server.cfg"), "ensure job").unwrap();
    let report = report(&inspect(&fixture.request()));
    assert!(!report.blocking);
    assert_eq!(report.resource_count, 1);
}

#[test]
fn rejects_profile_traversal_and_external_exec_targets() {
    let fixture = Fixture::new();
    let mut request = fixture.request();
    request.profile = "../data".into();
    assert!(resolve_data_root(&request).is_err());
    fs::write(fixture.root.join("outside.cfg"), "ensure outside-secret").unwrap();
    fs::write(
        fixture.root.join("data/server.cfg"),
        "exec ../outside.cfg\nexec server.cfg",
    )
    .unwrap();
    let report = report(&inspect(&fixture.request()));
    assert_eq!(report.config_count, 1);
    assert!(report
        .checks
        .iter()
        .any(|check| check.code == "exec-unresolved"));
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("outside-secret"));
}

#[test]
fn dynamic_resource_references_warn_instead_of_reporting_definite_missing_resources() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("data/server.cfg"),
        "ensure ${resource_name}",
    )
    .unwrap();
    let report = report(&inspect(&fixture.request()));
    assert!(!report.blocking);
    assert!(report
        .checks
        .iter()
        .any(|check| check.code == "dynamic-resource-reference"));
}

#[test]
fn occupied_tcp_port_is_reported_without_changing_configuration() {
    let fixture = Fixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = format!("endpoint_add_tcp \"127.0.0.1:{port}\"");
    fs::write(fixture.root.join("data/server.cfg"), &config).unwrap();
    let mut request = fixture.request();
    request.check_ports = true;
    let report = report(&inspect(&request));
    assert!(report
        .checks
        .iter()
        .any(|check| check.code == "port-in-use" && check.severity == Severity::Error));
    assert_eq!(
        fs::read_to_string(fixture.root.join("data/server.cfg")).unwrap(),
        config
    );
}

#[test]
fn preview_contains_only_redacted_summaries_and_bounded_logs() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("data/server.cfg"), "rcon_password \"do-not-export-this\"\nset mysql_connection_string \"mysql://root:secret@localhost/test\"").unwrap();
    let log = fixture.root.join("application.log");
    fs::write(
        &log,
        format!(
            "{}\nstandalone do-not-export-this\nAuthorization: Bearer hidden-token\nall good",
            "normal line\n".repeat(250)
        ),
    )
    .unwrap();
    let request = DiagnosticPreviewRequest {
        preflight: fixture.request(),
        include_application_log: true,
        include_server_log: false,
    };
    let preview = prepare_preview(&request, Some(&log), "0.3.2").unwrap();
    for entry in &preview.entries {
        assert!(!entry.content.contains("do-not-export-this"));
        assert!(!entry.content.contains("mysql://"));
        assert!(!entry.content.contains("hidden-token"));
        assert!(!entry.name.contains("default"));
        if entry.name.ends_with(".json") {
            assert!(serde_json::from_str::<serde_json::Value>(&entry.content).is_ok());
        }
    }
    let log = preview
        .entries
        .iter()
        .find(|entry| entry.name == "logs/application.log")
        .unwrap();
    assert!(log.content.lines().count() <= 200);
    assert!(log.content.ends_with("all good"));
}

#[test]
fn redacting_quoted_secret_values_does_not_corrupt_json() {
    let entry = json_entry(
        "test.json",
        json!({ "message": "one \" quote" }),
        &["\"".into()],
    )
    .unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&entry.content).is_ok());
}

#[test]
fn zip_exports_the_reviewed_snapshot_not_live_files_and_never_overwrites() {
    let fixture = Fixture::new();
    let log = fixture.root.join("app.log");
    fs::write(&log, "reviewed content").unwrap();
    let request = DiagnosticPreviewRequest {
        preflight: fixture.request(),
        include_application_log: true,
        include_server_log: false,
    };
    let preview = prepare_preview(&request, Some(&log), "0.3.2").unwrap();
    fs::write(&log, "changed after preview").unwrap();
    let path = fixture.root.join("diagnostics.zip");
    let result = export_preview(&preview.id, &path).unwrap();
    assert!(result.size_bytes > 0);
    let mut archive = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
    for entry in &preview.entries {
        let mut extracted = String::new();
        archive
            .by_name(&entry.name)
            .unwrap()
            .read_to_string(&mut extracted)
            .unwrap();
        assert_eq!(extracted, entry.content);
    }
    drop(archive);
    let previous = fs::read(&path).unwrap();
    assert!(write_archive(&path, &preview.entries).is_err());
    assert_eq!(fs::read(&path).unwrap(), previous);
    assert!(export_preview(&preview.id, &fixture.root.join("again.zip")).is_err());
}
