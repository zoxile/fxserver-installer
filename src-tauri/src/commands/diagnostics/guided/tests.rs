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
fn retargeted_resource_links_invalidate_identical_manifest_evidence() {
    let fixture = Fixture::new();
    for name in ["first", "second"] {
        fs::create_dir(fixture.root.join(name)).unwrap();
        fs::write(
            fixture.root.join(name).join("fxmanifest.lua"),
            "fx_version 'cerulean'",
        )
        .unwrap();
    }
    let link = fixture.root.join("data/resources/rconlog");
    super::super::tests::link_directory(&fixture.root.join("first"), &link);
    let preview = prepare_patch(fixture.request()).unwrap();
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
    super::super::tests::link_directory(&fixture.root.join("second"), &link);
    let store = fixture.root.join("history");
    assert!(apply_patch(&store, &preview.id)
        .unwrap_err()
        .contains("CONFIG_CHANGED"));
    assert!(!store.exists());
    assert_eq!(
        read_bounded(&fixture.root.join("data/server.cfg")).unwrap(),
        ""
    );
    #[cfg(windows)]
    fs::remove_dir(link).unwrap();
    #[cfg(unix)]
    fs::remove_file(link).unwrap();
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
fn artifact_resources_and_included_configs_are_evidence_never_patch_targets() {
    let fixture = Fixture::new();
    fixture.artifact_resource("rconlog", "fx_version 'cerulean'");
    let artifact = fixture
        .root
        .join("artifacts/citizen/system_resources/rconlog");
    let manifest = read_bounded(&artifact.join("fxmanifest.lua")).unwrap();
    fs::write(artifact.join("server.cfg"), "# artifact configuration\n").unwrap();
    let cfg = fixture.root.join("data/server.cfg");
    fs::write(&cfg, "exec @rconlog/server.cfg\n").unwrap();
    let preview = prepare_patch(fixture.request()).unwrap();
    assert_eq!(Path::new(&preview.path), cfg.canonicalize().unwrap());
    assert_ne!(
        Path::new(&preview.path),
        artifact.join("server.cfg").canonicalize().unwrap()
    );
    let store = fixture.root.join("history");
    apply_patch(&store, &preview.id).unwrap();
    assert_eq!(read_bounded(&cfg).unwrap(), preview.after);
    assert_eq!(
        read_bounded(&artifact.join("server.cfg")).unwrap(),
        "# artifact configuration\n"
    );
    assert_eq!(
        read_bounded(&artifact.join("fxmanifest.lua")).unwrap(),
        manifest
    );

    fs::remove_file(&cfg).unwrap();
    assert!(prepare_patch(fixture.request()).is_err());
    assert!(!cfg.exists());
    assert_eq!(
        read_bounded(&artifact.join("server.cfg")).unwrap(),
        "# artifact configuration\n"
    );
}

#[test]
fn artifact_dependency_guidance_and_changed_evidence_cannot_repair_artifact_files() {
    let fixture = Fixture::new();
    fixture.artifact_resource("rconlog", "fx_version 'cerulean'");
    let preview = prepare_patch(fixture.request()).unwrap();
    fixture.artifact_resource("rconlog", "dependency 'missing-lib'");
    let result = report(&inspect(&fixture.request()));
    let guidance = result
        .checks
        .iter()
        .find(|item| item.code == "dependency-missing")
        .unwrap()
        .guidance
        .as_ref()
        .unwrap();
    assert_eq!(guidance.page, "artifact-install");
    assert!(!guidance.patch_available);
    let store = fixture.root.join("history");
    assert!(apply_patch(&store, &preview.id).is_err());
    assert!(!store.exists());
    assert_eq!(
        read_bounded(&fixture.root.join("data/server.cfg")).unwrap(),
        ""
    );
}

#[test]
fn resource_shadowing_or_provider_ambiguity_prevents_automatic_config_repairs() {
    let fixture = Fixture::new();
    fixture.artifact_resource("rconlog", "fx_version 'cerulean'");
    fixture.resource("alternative", "provide 'rconlog'");
    assert!(prepare_patch(fixture.request()).is_err());
    fixture.resource("alternative", "fx_version 'cerulean'");
    fixture.resource("rconlog", "fx_version 'cerulean'");
    assert!(prepare_patch(fixture.request()).is_err());
}

#[test]
fn bundled_resource_configs_cannot_be_patch_targets_even_as_direct_or_linked_data_paths() {
    let fixture = Fixture::new();
    fixture.artifact_resource("chat", "fx_version 'cerulean'");
    fixture.artifact_resource("rconlog", "fx_version 'cerulean'");
    let chat = fixture.artifact_resources().join("chat");
    fs::create_dir(chat.join("resources")).unwrap();
    fs::write(chat.join("server.cfg"), "# bundled configuration\n").unwrap();
    let linked = fixture.root.join("linked-data");
    super::super::tests::link_directory(&chat, &linked);
    for data_path in [&chat, &linked] {
        fs::write(
            fixture.root.join("txData/default/config.json"),
            serde_json::to_vec(&json!({"server": {"dataPath": data_path}})).unwrap(),
        )
        .unwrap();
        let inspection = inspect(&fixture.request());
        assert!(!inspection.resources_incomplete);
        assert!(!inspection.configs_incomplete);
        assert!(!report(&inspection).blocking);
        assert!(patch_target(&inspection)
            .err()
            .unwrap()
            .contains("Bundled artifact resource"));
        assert!(prepare_patch(fixture.request()).is_err());
        assert_eq!(
            read_bounded(&chat.join("server.cfg")).unwrap(),
            "# bundled configuration\n"
        );
    }
}

#[cfg(windows)]
#[test]
fn user_configs_at_the_artifact_root_or_sibling_data_folder_remain_repairable() {
    let fixture = Fixture::new();
    fixture.artifact_resource("rconlog", "fx_version 'cerulean'");
    let artifact = fixture.root.join("artifacts");
    let bundled_manifest = artifact.join("citizen/system_resources/rconlog/fxmanifest.lua");
    let original_manifest = read_bounded(&bundled_manifest).unwrap();
    for data_path in [artifact.clone(), artifact.join("user-data")] {
        fs::create_dir_all(data_path.join("resources")).unwrap();
        let cfg = data_path.join("server.cfg");
        fs::write(&cfg, "# user configuration\n").unwrap();
        fs::write(
            fixture.root.join("txData/default/config.json"),
            serde_json::to_vec(&json!({"server": {"dataPath": data_path}})).unwrap(),
        )
        .unwrap();
        let preview = prepare_patch(fixture.request()).unwrap();
        assert_eq!(Path::new(&preview.path), cfg.canonicalize().unwrap());
        apply_patch(&fixture.root.join("history"), &preview.id).unwrap();
        assert_eq!(
            read_bounded(&cfg).unwrap(),
            "# user configuration\nensure rconlog\n"
        );
        assert_eq!(read_bounded(&bundled_manifest).unwrap(), original_manifest);
    }
}

#[test]
fn incomplete_scans_disable_repairs_even_when_their_warning_was_omitted() {
    let fixture = installed();
    for incomplete_resources in [true, false] {
        let mut inspection = inspect(&fixture.request());
        assert!(patch_target(&inspection).is_ok());
        if incomplete_resources {
            inspection.resources_incomplete = true;
        } else {
            inspection.configs_incomplete = true;
        }
        assert!(patch_target(&inspection).is_err());
    }
    let mut inspection = inspect(&fixture.request());
    inspection.checks_limited = true;
    assert!(patch_target(&inspection).is_err());
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
