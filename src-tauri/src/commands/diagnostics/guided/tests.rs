use super::super::tests::Fixture;
use super::*;

fn installed() -> Fixture {
    let fixture = Fixture::new();
    fixture.resource("[system]/rconlog", "fx_version 'cerulean'\ngame 'gta5'");
    fixture
}

#[test]
fn guidance_points_to_evidence_without_exposing_credentials_or_writing_files() {
    let fixture = Fixture::new();
    fixture.resource("job", "dependency 'missing-lib'");
    let source = "rcon_password \"fixture-secret\"\nexec missing.cfg\nensure job";
    fs::write(fixture.root.join("data/server.cfg"), source).unwrap();
    let report = report(&inspect(&fixture.request()));
    let missing = report
        .checks
        .iter()
        .find(|check| check.code == "dependency-missing")
        .unwrap();
    assert_eq!(missing.resource.as_deref(), Some("job"));
    assert_eq!(missing.guidance.as_ref().unwrap().page, "resource-manager");
    let exec = report
        .checks
        .iter()
        .find(|check| check.code == "exec-unresolved")
        .unwrap();
    assert_eq!(exec.file.as_deref(), Some("server.cfg"));
    assert_eq!(exec.line, Some(2));
    assert_eq!(exec.guidance.as_ref().unwrap().page, "server-configure");
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("fixture-secret"));
    assert_eq!(
        read_bounded(&fixture.root.join("data/server.cfg")).unwrap(),
        source
    );
    assert!(prepare_patch(fixture.request()).is_err());
}

#[test]
fn patch_is_offered_only_for_complete_unambiguous_static_evidence() {
    let fixture = installed();
    assert!(patch_target(&inspect(&fixture.request())).is_ok());
    for source in [
        "exec missing.cfg",
        "exec",
        "exec server.cfg",
        "ensure ${group}",
        "stop rconlog",
        "ensure rconlog",
        "set name \"unclosed",
    ] {
        fs::write(fixture.root.join("data/server.cfg"), source).unwrap();
        assert!(
            prepare_patch(fixture.request()).is_err(),
            "unexpected repair for {source}"
        );
    }
    fs::write(fixture.root.join("data/server.cfg"), "").unwrap();
    fixture.resource("[system]/rconlog", "dependency 'unavailable'");
    assert!(prepare_patch(fixture.request()).is_err());
    fixture.resource("[system]/rconlog", "dependencies(get_dependencies())");
    assert!(prepare_patch(fixture.request()).is_err());
    fixture.resource("[system]/rconlog", "fx_version 'cerulean'");
    fixture.resource("[duplicate]/rconlog", "fx_version 'cerulean'");
    assert!(prepare_patch(fixture.request()).is_err());
}

#[test]
fn effective_rcon_respects_exec_order_empty_overrides_and_stop() {
    let fixture = installed();
    fs::write(
        fixture.root.join("data/server.cfg"),
        "rcon_password original\nensure rconlog\nexec override.cfg",
    )
    .unwrap();
    fs::write(
        fixture.root.join("data/override.cfg"),
        "rcon_password \"\"\nstop rconlog",
    )
    .unwrap();
    let report = report(&inspect(&fixture.request()));
    assert!(report
        .checks
        .iter()
        .any(|check| check.code == "rcon-not-configured"));
    assert!(report
        .checks
        .iter()
        .any(|check| check.code == "rconlog-not-started"));
    assert!(!report
        .checks
        .iter()
        .any(|check| check.code == "rcon-configured"));
    assert!(prepare_patch(fixture.request()).is_err());
}

#[test]
fn preview_preserves_newlines_and_changes_no_files() {
    let fixture = installed();
    for source in ["# fixture\r\n", "# fixture", "", "# fixture\n"] {
        fs::write(fixture.root.join("data/server.cfg"), source).unwrap();
        let preview = prepare_patch(fixture.request()).unwrap();
        assert_eq!(preview.before, source);
        assert!(preview.after.starts_with(source));
        assert_eq!(
            parsing::config_commands(&preview.after),
            vec![(
                if source.is_empty() { 1 } else { 2 },
                vec!["ensure".into(), "rconlog".into()]
            )]
        );
        if source.contains("\r\n") {
            assert_eq!(preview.after, "# fixture\r\nensure rconlog\r\n");
        }
        assert_eq!(
            read_bounded(&fixture.root.join("data/server.cfg")).unwrap(),
            source
        );
        pending().lock().unwrap().remove(&preview.id);
    }
}

#[test]
fn config_include_and_manifest_changes_invalidate_the_reviewed_patch() {
    for change in ["server", "include", "manifest", "profile"] {
        let fixture = installed();
        let cfg = fixture.root.join("data/server.cfg");
        fs::write(&cfg, "exec included.cfg\n").unwrap();
        fs::write(fixture.root.join("data/included.cfg"), "# original").unwrap();
        let preview = prepare_patch(fixture.request()).unwrap();
        match change {
            "server" => fs::write(&cfg, "# changed\nexec included.cfg\n").unwrap(),
            "include" => fs::write(fixture.root.join("data/included.cfg"), "# changed").unwrap(),
            "manifest" => fixture.resource(
                "[system]/rconlog",
                "fx_version 'cerulean'\nversion 'changed'",
            ),
            _ => fs::write(fixture.root.join("txData/default/config.json"), "{}").unwrap(),
        }
        let current = read_bounded(&cfg).unwrap();
        let store = fixture.root.join("history");
        assert!(apply_patch(&store, &preview.id).is_err());
        assert_eq!(read_bounded(&cfg).unwrap(), current);
        assert!(!store.exists());
    }
}

#[test]
fn expired_and_unknown_reviews_never_write() {
    let fixture = installed();
    let preview = prepare_patch(fixture.request()).unwrap();
    pending()
        .lock()
        .unwrap()
        .get_mut(&preview.id)
        .unwrap()
        .created -= PREVIEW_TTL + Duration::from_secs(1);
    let store = fixture.root.join("history");
    assert!(apply_patch(&store, &preview.id).is_err());
    assert!(apply_patch(&store, "unknown").is_err());
    assert!(!store.exists());
    assert_eq!(
        read_bounded(&fixture.root.join("data/server.cfg")).unwrap(),
        ""
    );
}

#[cfg(windows)]
#[test]
fn reviewed_patch_is_single_use_and_preserves_encrypted_previous_content() {
    let fixture = installed();
    let cfg = fixture.root.join("data/server.cfg");
    let original = "rcon_password fixture-sensitive\r\n";
    fs::write(&cfg, original).unwrap();
    let preview = prepare_patch(fixture.request()).unwrap();
    let store = fixture.root.join("history");
    let saved = apply_patch(&store, &preview.id).unwrap();
    assert_eq!(saved.content, preview.after);
    assert_eq!(saved.content, format!("{original}ensure rconlog\r\n"));
    let entry = fs::read_dir(&store).unwrap().next().unwrap().unwrap();
    let bytes = fs::read(entry.path()).unwrap();
    assert!(!bytes
        .windows(b"fixture-sensitive".len())
        .any(|chunk| chunk == b"fixture-sensitive"));
    let journal: serde_json::Value =
        serde_json::from_slice(&crate::commands::fxserver::decrypt_secret(&bytes).unwrap())
            .unwrap();
    assert_eq!(
        journal["snapshots"][0]["metadata"]["reason"],
        "before-patch"
    );
    assert_eq!(journal["snapshots"][0]["content"], original);
    assert_eq!(journal["snapshots"][1]["metadata"]["reason"], "patch");
    assert!(apply_patch(&store, &preview.id).is_err());
    assert!(prepare_patch(fixture.request()).is_err());
}
