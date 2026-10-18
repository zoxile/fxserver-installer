use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
#[test]
fn unix_include_names_do_not_use_windows_stream_or_trailing_dot_rules() {
    let fixture = Fixture::new();
    fixture.file("local:settings.cfg", "set first value\n");
    fixture.file("settings./extra.cfg", "set second value\n");
    let result = fixture.report("exec local:settings.cfg\nexec settings./extra.cfg\n");
    assert_eq!(result.file_count, 3);
    assert!(result.issues.is_empty());
    assert!(fixture
        .report("exec ../outside.cfg\n")
        .issues
        .iter()
        .any(|issue| issue.code == "path-unsafe"));
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fxi-validation-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn file(&self, name: &str, content: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }
    fn report(&self, content: &str) -> ConfigValidationReport {
        let selected = self.file("server.cfg", "# disk baseline\r\n");
        inspect(&self.0, &selected, content).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct DirectoryLink(PathBuf);
impl DirectoryLink {
    fn new(path: PathBuf, target: &Path) -> Self {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        #[cfg(windows)]
        {
            use crate::process::CommandNoWindowExt;
            let status = std::process::Command::new("cmd")
                .no_window()
                .args(["/C", "mklink", "/J"])
                .arg(&path)
                .arg(target)
                .output()
                .unwrap();
            assert!(status.status.success());
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, &path).unwrap();
        Self(path)
    }
}
impl Drop for DirectoryLink {
    fn drop(&mut self) {
        #[cfg(windows)]
        fs::remove_dir(&self.0).unwrap();
        #[cfg(unix)]
        fs::remove_file(&self.0).unwrap();
    }
}

fn has(report: &ConfigValidationReport, code: &str, severity: &str) -> bool {
    report
        .issues
        .iter()
        .any(|issue| issue.code == code && issue.severity == severity)
}

#[test]
fn quotes_comments_semicolons_and_empty_values_use_preflight_parser() {
    let f = Fixture::new();
    let report = f.report("\u{feff}# set fixture ignored\r\nset fixture \"one;#//\"; set fixture \"two\\\"three\" // comment\r\nset empty \"\"\nensure one; ensure two\n");
    assert_eq!(report.issues.len(), 1);
    assert!(has(&report, "setting-duplicate", "warning"));
    assert_eq!(report.issues[0].line, Some(2));
}

#[test]
fn malformed_cfg_and_missing_arguments_block() {
    let f = Fixture::new();
    for cfg in ["set value \"unterminated", "set x '\nunclosed", "set x y\0"] {
        assert!(has(&f.report(cfg), "cfg-malformed", "error"));
    }
    for cfg in ["set value", "setr \"\" value"] {
        assert!(has(&f.report(cfg), "setting-arguments", "error"));
    }
    for cfg in ["exec", "exec \"\"", "exec first.cfg second.cfg"] {
        assert!(has(&f.report(cfg), "exec-arguments", "error"));
    }
}

#[test]
fn includes_are_data_relative_even_from_nested_configs_and_cycles_block() {
    let f = Fixture::new();
    f.file("nested/first.cfg", "exec shared.cfg\n");
    f.file("shared.cfg", "set shared okay\n");
    let report = f.report("exec nested/first.cfg");
    assert!(report.issues.is_empty());
    assert_eq!(report.file_count, 3);
    f.file("shared.cfg", "exec nested/first.cfg");
    assert!(has(
        &f.report("exec nested/first.cfg"),
        "exec-cycle",
        "error"
    ));
    assert!(has(&f.report("exec missing.cfg"), "exec-missing", "error"));
}

#[test]
fn traversals_absolute_paths_streams_and_devices_are_rejected_without_echoing_input() {
    let f = Fixture::new();
    for path in [
        "../private.cfg",
        "nested/../../private.cfg",
        "C:/private.cfg",
        "/private.cfg",
        "file.cfg:secret",
        "\\\\server\\private.cfg",
        "@resource/../private.cfg",
        "@resource//private.cfg",
    ] {
        let report = f.report(&format!("exec \"{path}\""));
        assert!(has(&report, "path-unsafe", "error"), "{path}");
        assert!(!serde_json::to_string(&report).unwrap().contains(path));
    }
    let outside = Fixture::new();
    let path = outside.file("outside.cfg", "secret");
    assert!(inspect(&f.0, &path, "set x y").is_err());
    assert!(inspect(&f.0, &f.0.join("../outside.cfg"), "set x y").is_err());
}

#[test]
fn resource_includes_allow_real_grouped_resources_without_interpreting_lua() {
    let f = Fixture::new();
    f.file(
        "resources/[core]/fixture/fxmanifest.lua",
        "error('must never run')",
    );
    f.file(
        "resources/[core]/fixture/config/permissions.cfg",
        "add_ace group.admin command allow",
    );
    let report = f.report("exec @fixture/config/permissions.cfg");
    assert!(report.issues.is_empty());
    assert_eq!(report.file_count, 2);
    f.file("resources/[other]/fixture/fxmanifest.lua", "");
    assert!(has(
        &f.report("exec @fixture/config/permissions.cfg"),
        "resource-ambiguous",
        "error"
    ));
}

#[test]
fn tcp_udp_pair_is_valid_but_wildcard_same_protocol_overlap_blocks() {
    let f = Fixture::new();
    assert!(f
        .report("endpoint_add_tcp \"0.0.0.0:30120\"; endpoint_add_udp \"0.0.0.0:30120\"")
        .issues
        .is_empty());
    assert!(f
        .report("endpoint_add_tcp 127.0.0.1:30120\nendpoint_add_tcp 127.0.0.2:30120")
        .issues
        .is_empty());
    for cfg in [
        "endpoint_add_tcp 0.0.0.0:30120\nendpoint_add_tcp 127.0.0.1:30120",
        "endpoint_add_udp 127.0.0.1:30120\nendpoint_add_udp 127.0.0.1:30120",
        "endpoint_add_tcp [::1]:30120\nendpoint_add_tcp [::]:30120",
        "endpoint_add_tcp 127.0.0.1:30120\nendpoint_add_tcp [::ffff:127.0.0.1]:30120",
    ] {
        assert!(has(&f.report(cfg), "endpoint-conflict", "error"));
    }
    assert!(has(
        &f.report("endpoint_add_tcp [::]:30120\nendpoint_add_tcp 0.0.0.0:30120"),
        "endpoint-dual-stack",
        "warning"
    ));
    for endpoint in [
        "0.0.0.0:0",
        "0.0.0.0:65536",
        "0.0.0.0:-1",
        "0.0.0.0",
        "[::1]:oops",
    ] {
        assert!(has(
            &f.report(&format!("endpoint_add_tcp {endpoint}")),
            "endpoint-invalid",
            "error"
        ));
    }
}

#[test]
fn includes_detect_cross_file_duplicates_and_use_only_selected_draft() {
    let f = Fixture::new();
    f.file("server.cfg", "set example first; exec child.cfg");
    let child = f.file("child.cfg", "set other baseline");
    let report = inspect(&f.0, &child, "set example draft").unwrap();
    assert!(has(&report, "setting-duplicate", "warning"));
    assert_eq!(report.issues[0].file, "Edited file");
    assert_eq!(fs::read_to_string(child).unwrap(), "set other baseline");
}

#[test]
fn repeated_exec_is_not_a_cycle_but_repeated_endpoint_still_conflicts() {
    let f = Fixture::new();
    f.file("child.cfg", "endpoint_add_tcp 0.0.0.0:30120");
    let report = f.report("exec child.cfg; exec child.cfg");
    assert!(has(&report, "exec-repeated", "warning"));
    assert!(has(&report, "endpoint-conflict", "error"));
    assert!(!has(&report, "exec-cycle", "error"));
}

#[test]
fn inactive_file_does_not_create_false_cross_file_duplicates() {
    let f = Fixture::new();
    f.file("server.cfg", "set example live");
    let inactive = f.file("inactive.cfg", "");
    assert!(inspect(&f.0, &inactive, "set example inactive")
        .unwrap()
        .issues
        .is_empty());
}

#[test]
fn secrets_are_advisories_and_never_serialized_in_findings_or_errors() {
    let f = Fixture::new();
    let report = f.report("sets api_token \"never-display-this;#//\"\nsetr mysql_connection_string \"mysql://root:private@localhost/db\"\nsv_licenseKey cfxk_hidden\nset private_token before; set private_token after");
    assert!(has(&report, "secret-exposed", "warning"));
    assert!(!report.issues.iter().any(|issue| issue.severity == "error"));
    let json = serde_json::to_string(&report).unwrap();
    for secret in [
        "never-display",
        "root:private",
        "cfxk_hidden",
        "api_token",
        "private_token",
    ] {
        assert!(!json.contains(secret));
    }
    assert_eq!(
        fs::read_to_string(f.0.join("server.cfg")).unwrap(),
        "# disk baseline\r\n"
    );
}

#[test]
fn revisions_change_with_included_contents_and_drafts() {
    let f = Fixture::new();
    f.file("child.cfg", "set x one");
    let first = f.report("exec child.cfg");
    assert_eq!(first.revision, f.report("exec child.cfg").revision);
    f.file("child.cfg", "set x two");
    assert_ne!(first.revision, f.report("exec child.cfg").revision);
    assert_ne!(
        first.revision,
        f.report("exec child.cfg\n# draft changed").revision
    );
}

#[test]
fn parser_and_read_limits_block_instead_of_claiming_success() {
    let f = Fixture::new();
    assert!(has(
        &f.report(&"echo x;".repeat(5000)),
        "cfg-malformed",
        "error"
    ));
    f.file("large.cfg", &"x".repeat(MAX_BYTES + 1));
    assert!(has(
        &f.report("exec large.cfg"),
        "config-unreadable",
        "error"
    ));
    let path = f.file("server.cfg", "");
    assert!(inspect(&f.0, &path, &"x".repeat(MAX_BYTES + 1)).is_err());
    fs::write(f.0.join("bad.cfg"), [0xff, 0xfe]).unwrap();
    assert!(has(&f.report("exec bad.cfg"), "config-unreadable", "error"));
}

#[test]
fn failed_reads_count_toward_the_aggregate_budget() {
    let f = Fixture::new();
    f.file("large.cfg", &"x".repeat(MAX_BYTES + 1));
    let report = f.report(&"exec large.cfg;".repeat(100));
    assert!(has(&report, "scan-limit", "error"));
    assert!(
        report
            .issues
            .iter()
            .filter(|issue| issue.code == "config-unreadable")
            .count()
            < 8
    );
}

#[test]
fn dynamic_references_are_advisory_but_traversal_is_never_downgraded() {
    let f = Fixture::new();
    assert!(has(
        &f.report("exec ${config}.cfg"),
        "exec-dynamic",
        "warning"
    ));
    assert!(has(
        &f.report("exec ../${config}.cfg"),
        "path-unsafe",
        "error"
    ));
    assert!(has(
        &f.report("endpoint_add_tcp ${bind}:${port}"),
        "endpoint-dynamic",
        "warning"
    ));
}

#[cfg(windows)]
#[test]
fn normal_windows_absolute_paths_match_canonical_verbatim_roots() {
    let f = Fixture::new();
    let selected = f.file("server.cfg", "");
    let standard = selected
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();
    assert!(inspect(&f.0, Path::new(&standard), "set x y")
        .unwrap()
        .issues
        .is_empty());
}

#[test]
fn linked_include_is_advisory_without_reading_outside_or_weakening_write_guards() {
    let f = Fixture::new();
    let outside = Fixture::new();
    outside.file("secret.cfg", "set token do-not-read");
    let link = DirectoryLink::new(f.0.join("linked"), &outside.0);
    let report = f.report("exec linked/secret.cfg");
    assert!(has(&report, "exec-unverified", "warning"));
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.file_count, 1);
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("do-not-read"));
    assert!(inspect(&f.0, &link.0.join("secret.cfg"), "set x edited").is_err());
    assert!(has(
        &f.report("exec linked/../secret.cfg"),
        "path-unsafe",
        "error"
    ));
}

#[test]
fn resource_junction_cfg_is_unverified_not_missing_and_external_contents_stay_unread() {
    let f = Fixture::new();
    let outside = Fixture::new();
    outside.file("fxmanifest.lua", "error('never execute')");
    outside.file(
        "config/permissions.cfg",
        "set token \"external-secret-unterminated",
    );
    let link = DirectoryLink::new(f.0.join("resources/[core]/fixture"), &outside.0);
    let draft = "exec @fixture/config/permissions.cfg\nsv_hostname edited\n";
    let report = f.report(draft);
    assert!(has(&report, "exec-unverified", "warning"));
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.file_count, 1);
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("external-secret"));
    outside.file(
        "config/permissions.cfg",
        "exec missing.cfg; endpoint_add_tcp invalid",
    );
    assert_eq!(report.revision, f.report(draft).revision);
    assert!(inspect(&f.0, &link.0.join("config/permissions.cfg"), "set x y").is_err());
    for target in [
        "@fixture/../secret.cfg",
        "@fixture/config/../../secret.cfg",
        "@fixture/config/permissions.cfg:stream",
        "@fixture//secret.cfg",
    ] {
        assert!(has(
            &f.report(&format!("exec \"{target}\"")),
            "path-unsafe",
            "error"
        ));
    }
    assert!(has(
        &f.report("exec @unrelated/missing.cfg"),
        "exec-missing",
        "error"
    ));
    f.file("resources/ordinary/fxmanifest.lua", "");
    f.file("resources/ordinary/config.cfg", "set ordinary value");
    assert!(f.report("exec @ordinary/config.cfg").issues.is_empty());
}

#[test]
fn linked_resource_collections_are_unverified_without_scanning_them() {
    let f = Fixture::new();
    let outside = Fixture::new();
    outside.file("fixture/fxmanifest.lua", "");
    outside.file("fixture/config.cfg", "set x \"not parsed");
    for directory in ["resources/[linked]", "resources"] {
        // Remove only the empty directory left by the previous category-link fixture.
        if directory == "resources" {
            fs::remove_dir(f.0.join("resources")).unwrap();
        }
        let _link = DirectoryLink::new(f.0.join(directory), &outside.0);
        let report = f.report("exec @fixture/config.cfg");
        assert!(has(&report, "exec-unverified", "warning"));
        assert_eq!(report.issues.len(), 1);
        assert_eq!(report.file_count, 1);
    }
}

#[test]
fn linked_cfg_subdirectory_in_an_ordinary_resource_is_advisory() {
    let f = Fixture::new();
    let outside = Fixture::new();
    f.file("resources/fixture/fxmanifest.lua", "");
    outside.file("permissions.cfg", "set x \"not parsed");
    let _link = DirectoryLink::new(f.0.join("resources/fixture/config"), &outside.0);
    let report = f.report("exec @fixture/config/permissions.cfg");
    assert!(has(&report, "exec-unverified", "warning"));
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.file_count, 1);
}
