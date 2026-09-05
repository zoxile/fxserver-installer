use super::*;

struct Fixture {
    root: PathBuf,
    source: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("fxclone-fixture-{}", unique_id()));
        let source = root.join("source");
        fs::create_dir_all(source.join("resources/[local]/demo")).unwrap();
        fs::write(
            source.join("resources/[local]/demo/fxmanifest.lua"),
            "fx_version 'cerulean'\ngame 'gta5'\nserver_script 'server.lua'\n",
        )
        .unwrap();
        fs::write(
            source.join("resources/[local]/demo/server.lua"),
            "print('fixture only')\n",
        )
        .unwrap();
        fs::write(
            source.join("resources/[local]/demo/LICENSE"),
            "MIT License\nCopyright Fixture\nhttps://opensource.org/license/mit\n",
        )
        .unwrap();
        fs::write(source.join("server.cfg"), "sv_hostname \"Fixture\"\nsv_licenseKey \"cfxk_fixture_secret\"\nset mysql_connection_string \"mysql://fixture:pass@localhost/source\"\nrcon_password \"fixture-rcon\"\nexec \"../external.cfg\"\nendpoint_add_tcp \"0.0.0.0:30120\"\n").unwrap();
        Self { root, source }
    }
    fn request(&self) -> CloneRequest {
        CloneRequest {
            source_path: self.source.to_string_lossy().into(),
            destination_path: self.root.join("target").to_string_lossy().into(),
            mode: CloneMode::Clone,
            resources: vec!["[local]/demo".into()],
            configs: vec!["server.cfg".into()],
            server_port: 30121,
            tx_admin_port: 40121,
            source_server_port: 30120,
            source_tx_admin_port: 40120,
            database: None,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let root = self.root.canonicalize().unwrap();
        let temp = std::env::temp_dir().canonicalize().unwrap();
        assert!(
            root.starts_with(temp)
                && root
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("fxclone-fixture-")
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn rejects_traversal_devices_ads_and_reserved_paths() {
    for path in [
        "../outside",
        "/absolute",
        "C:/absolute",
        "server-data/../out",
        "a\\b",
        "a//b",
        "a:stream",
        "NUL.txt",
        "COM1.cfg",
        "foo. ",
        "a/./b",
    ] {
        assert!(relative(path).is_err(), "accepted {path}");
    }
    assert!(relative("server-data/resources/[local]/demo/server.lua").is_ok());
    let fixture = Fixture::new();
    assert!(destination_path(
        fixture.source.join("nested").to_str().unwrap(),
        &fixture.source.canonicalize().unwrap()
    )
    .is_err());
    assert!(destination_path(
        fixture.source.to_str().unwrap(),
        &fixture.source.canonicalize().unwrap()
    )
    .is_err());
    fs::create_dir(fixture.root.join("target")).unwrap();
    assert!(build_plan(&fixture.request()).is_err());
}

#[test]
fn excludes_secrets_external_references_and_preserves_license_notices() {
    let fixture = Fixture::new();
    let resource = fixture.source.join("resources/[local]/demo");
    for (name, contents) in [
        (".env", "PASSWORD=fixture-secret"),
        ("dump.sql", "INSERT fixture"),
        ("config.lua", "Config.Token = 'fixture-secret'"),
        ("external.lua", "dofile('C:/outside/private.lua')"),
    ] {
        fs::write(resource.join(name), contents).unwrap();
    }
    let plan = build_plan(&fixture.request()).unwrap();
    assert_eq!(plan.files.len(), 4);
    assert!(plan
        .files
        .iter()
        .any(|item| item.file.path.ends_with("/LICENSE")));
    for name in [".env", "dump.sql", "config.lua", "external.lua"] {
        assert!(plan.excluded.iter().any(|item| item.path.ends_with(name)));
    }
    let cfg = String::from_utf8(
        plan.files
            .iter()
            .find(|item| item.file.path == "server-data/server.cfg")
            .unwrap()
            .generated
            .clone()
            .unwrap(),
    )
    .unwrap();
    for secret in [
        "fixture_secret",
        "fixture-rcon",
        "mysql://",
        "../external",
        "30120",
    ] {
        assert!(!cfg.contains(secret), "leaked {secret}");
    }
    assert!(cfg.contains("30121") && cfg.contains("Fixture"));
}

#[test]
fn removes_owned_bridge_blocks_and_exact_unmanaged_start_references() {
    let cfg = format!(
        "ensure demo\n{LIVE_BRIDGE_BEGIN}\nset bridge_instance fixture-pairing-value\nensure {LIVE_BRIDGE_RESOURCE}\n{LIVE_BRIDGE_END}\nstart '{LIVE_BRIDGE_RESOURCE}' # unmanaged\nENSURE \"FXSERVER_INSTALLER_BRIDGE\"\nensure {LIVE_BRIDGE_RESOURCE}_extension\n# BEGIN FXSERVER INSTALLER LIVE BRIDGE OTHER\nset unrelated_setting 1\n"
    );
    let clean = String::from_utf8(sanitize_cfg(&cfg)).unwrap();
    assert!(clean.contains("ensure demo"));
    assert!(clean.contains("ensure fxserver_installer_bridge_extension"));
    assert!(clean.contains("set unrelated_setting 1"));
    for excluded in [
        "fixture-pairing-value",
        "bridge_instance",
        "ensure fxserver_installer_bridge\n",
        "start 'fxserver_installer_bridge'",
        "ENSURE \"FXSERVER_INSTALLER_BRIDGE\"",
        LIVE_BRIDGE_BEGIN,
        LIVE_BRIDGE_END,
    ] {
        assert!(!clean.contains(excluded), "retained {excluded}");
    }
    let unterminated =
        format!("ensure demo\n{LIVE_BRIDGE_BEGIN}\nset bridge_instance fixture-value\n");
    assert_eq!(sanitize_cfg(&unterminated), b"ensure demo\n");
}

#[cfg(windows)]
#[test]
fn clone_and_export_exclude_the_entire_machine_paired_bridge() {
    for mode in [CloneMode::Clone, CloneMode::Export] {
        let fixture = Fixture::new();
        let bridge = fixture.source.join("resources").join(LIVE_BRIDGE_RESOURCE);
        fs::create_dir(&bridge).unwrap();
        for (name, content) in [
            ("fxmanifest.lua", "fx_version 'cerulean'\n"),
            ("server.lua", "print('fixture bridge')\n"),
            ("pairing.json", "fixture-pairing-value"),
            (".owned-marker", "fixture-owner-value"),
        ] {
            fs::write(bridge.join(name), content).unwrap();
        }
        fs::write(
            fixture.source.join("server.cfg"),
            format!("ensure demo\n{LIVE_BRIDGE_BEGIN}\nset bridge_instance fixture-pairing-value\nensure {LIVE_BRIDGE_RESOURCE}\n{LIVE_BRIDGE_END}\nstart {LIVE_BRIDGE_RESOURCE}\n"),
        ).unwrap();
        let mut request = fixture.request();
        request.mode = mode;
        request.resources.push(LIVE_BRIDGE_RESOURCE.into());
        let plan = build_plan(&request).unwrap();
        assert!(!plan
            .files
            .iter()
            .any(|item| item.file.path.contains(LIVE_BRIDGE_RESOURCE)));
        assert!(plan.excluded.iter().any(|item| item.path
            == format!("server-data/resources/{LIVE_BRIDGE_RESOURCE}")
            && item.reason.contains("reinstall and pair")));
        let result = execute_plan(&request, &plan, || Ok(())).unwrap();
        let data = Path::new(&result.server_data_path);
        assert!(!data.join("resources").join(LIVE_BRIDGE_RESOURCE).exists());
        assert!(data.join("resources/[local]/demo/server.lua").is_file());
        let cfg = fs::read_to_string(data.join("server.cfg")).unwrap();
        assert!(cfg.contains("ensure demo"));
        assert!(!cfg.contains(LIVE_BRIDGE_RESOURCE) && !cfg.contains("fixture-pairing-value"));
        assert_eq!(
            fs::read_to_string(bridge.join("pairing.json")).unwrap(),
            "fixture-pairing-value"
        );
    }
}

#[cfg(windows)]
#[test]
fn imported_package_omits_all_bridge_files_with_one_folder_exclusion() {
    let fixture = Fixture::new();
    let package = fixture.root.join("package");
    let bridge_path = format!(
        "server-data/resources/[system]/{}",
        LIVE_BRIDGE_RESOURCE.to_ascii_uppercase()
    );
    fs::create_dir_all(package.join(&bridge_path)).unwrap();
    let mut files = Vec::new();
    for (path, contents) in [
        (format!("{bridge_path}/fxmanifest.lua"), "fx_version 'cerulean'\n".to_string()),
        (format!("{bridge_path}/server.lua"), "print('fixture bridge')\n".to_string()),
        (format!("{bridge_path}/pairing.json"), "fixture-pairing-value".to_string()),
        ("server-data/server.cfg".to_string(), format!("ensure demo\n{LIVE_BRIDGE_BEGIN}\nset bridge_instance fixture-value\nensure {LIVE_BRIDGE_RESOURCE}\n{LIVE_BRIDGE_END}\nstart {LIVE_BRIDGE_RESOURCE}\n")),
    ] {
        fs::write(package.join(&path), &contents).unwrap();
        files.push(PackageFile { path, size: contents.len() as u64, sha256: digest(contents.as_bytes()) });
    }
    let manifest = PackageManifest {
        schema_version: 1,
        usage: "private-user-copy".into(),
        server_port: 30120,
        tx_admin_port: 40120,
        files,
        database: None,
    };
    fs::write(
        package.join(MANIFEST),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let mut request = fixture.request();
    request.mode = CloneMode::Import;
    request.source_path = package.to_string_lossy().into();
    let plan = build_plan(&request).unwrap();
    assert_eq!(
        plan.excluded
            .iter()
            .filter(|item| item.path == bridge_path)
            .count(),
        1
    );
    assert_eq!(plan.files.len(), 1);
    let result = execute_plan(&request, &plan, || Ok(())).unwrap();
    let data = Path::new(&result.server_data_path);
    assert!(!data.join("resources").exists());
    let cfg = fs::read_to_string(data.join("server.cfg")).unwrap();
    assert!(cfg.contains("ensure demo"));
    assert!(!cfg.contains(LIVE_BRIDGE_RESOURCE) && !cfg.contains("bridge_instance"));
}

#[test]
fn rejects_source_port_collisions_and_unknown_selections() {
    let fixture = Fixture::new();
    let mut request = fixture.request();
    request.tx_admin_port = request.source_server_port;
    assert!(build_plan(&request).is_err());
    request = fixture.request();
    request.resources = vec!["../../outside".into()];
    assert!(build_plan(&request).is_err());
    request = fixture.request();
    request.resources.push(request.resources[0].clone());
    assert!(build_plan(&request).is_err());
}

#[test]
fn package_rejects_external_manifest_paths_and_hash_tampering() {
    let fixture = Fixture::new();
    let package = fixture.root.join("package");
    fs::create_dir(&package).unwrap();
    let mut manifest = PackageManifest {
        schema_version: 1,
        usage: "private-user-copy".into(),
        server_port: 30120,
        tx_admin_port: 40120,
        files: vec![PackageFile {
            path: "../outside".into(),
            size: 4,
            sha256: digest(b"test"),
        }],
        database: None,
    };
    fs::write(
        package.join(MANIFEST),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let mut request = fixture.request();
    request.mode = CloneMode::Import;
    request.source_path = package.to_string_lossy().into();
    assert!(build_plan(&request).is_err());
    fs::create_dir(package.join("server-data")).unwrap();
    fs::write(package.join("server-data/server.cfg"), "test").unwrap();
    manifest.files[0].path = "server-data/server.cfg".into();
    manifest.files[0].sha256 = digest(b"fake");
    fs::write(
        package.join(MANIFEST),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(build_plan(&request).is_err());
    manifest.files[0].sha256 = digest(b"test");
    manifest.files.push(manifest.files[0].clone());
    fs::write(
        package.join(MANIFEST),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(build_plan(&request).is_err());
}

fn database_selection(fixture: &Fixture) -> database::DatabaseSelection {
    database::DatabaseSelection {
        dump_path: fixture.root.join("fixture.sql").to_string_lossy().into(),
        source_database: "fixture_source".into(),
        host: "fixture.invalid".into(),
        port: 3306,
        username: "fixture".into(),
    }
}

#[test]
fn database_preflight_rejects_unsupported_or_secret_sql_without_promoting() {
    let fixture = Fixture::new();
    let mut request = fixture.request();
    let selected = database_selection(&fixture);
    request.database = Some(selected.clone());
    for sql in [
        "USE production; DROP TABLE players;",
        "CREATE TABLE `x` (`password` varchar(32)) ENGINE=InnoDB;",
        "CREATE TABLE `x` (`id` int) ENGINE=CONNECT;",
    ] {
        fs::write(&selected.dump_path, sql).unwrap();
        assert!(build_plan(&request).is_err());
        assert!(!fixture.root.join("target").exists());
    }
}

#[cfg(windows)]
#[test]
fn opted_in_database_package_is_validated_without_connecting_to_any_database() {
    let fixture = Fixture::new();
    let mut request = fixture.request();
    let selected = database_selection(&fixture);
    let sql = "CREATE TABLE `players` (`id` int NOT NULL, `name` varchar(64), PRIMARY KEY (`id`)) ENGINE=InnoDB; INSERT INTO `players` VALUES (1,'Fixture');";
    fs::write(&selected.dump_path, sql).unwrap();
    request.mode = CloneMode::Export;
    request.database = Some(selected.clone());
    let plan = build_plan(&request).unwrap();
    assert_eq!(plan.database.as_ref().unwrap().table_count, 1);
    let preview = database::preview(plan.database.as_ref().unwrap(), &request).unwrap();
    assert!(preview.target.is_none());
    let result = execute_plan(&request, &plan, || Ok(())).unwrap();
    assert_eq!(
        fs::read_to_string(Path::new(&result.destination_path).join("database.sql")).unwrap(),
        sql
    );
    let mut import = request.clone();
    import.mode = CloneMode::Import;
    import.source_path = result.destination_path;
    import.destination_path = fixture.root.join("imported").to_string_lossy().into();
    import.server_port = 30122;
    import.tx_admin_port = 40122;
    let imported_plan = build_plan(&import).unwrap();
    let preview = database::preview(imported_plan.database.as_ref().unwrap(), &import).unwrap();
    let public_preview = serde_json::to_value(&preview).unwrap();
    let internal_target = serde_json::to_value(preview.target.as_ref().unwrap()).unwrap();
    assert!(public_preview["target"].get("markerToken").is_none());
    assert_eq!(public_preview["target"].as_object().unwrap().len(), 3);
    assert_eq!(
        internal_target["markerToken"],
        preview.target.as_ref().unwrap().marker_token
    );
    assert!(preview
        .target
        .as_ref()
        .unwrap()
        .database
        .starts_with("fxsi_clone_"));
    assert_ne!(preview.target.unwrap().database, selected.source_database);
    import.database = None;
    let without_database = build_plan(&import).unwrap();
    assert!(without_database.database.is_none());
    assert!(without_database
        .excluded
        .iter()
        .any(|item| item.path == "database.sql"));
}

#[cfg(windows)]
#[test]
fn private_clone_and_local_package_round_trip_never_launch_or_restore() {
    let fixture = Fixture::new();
    let request = fixture.request();
    let plan = build_plan(&request).unwrap();
    let result = execute_plan(&request, &plan, || Ok(())).unwrap();
    assert!(Path::new(&result.tx_data_path).is_dir());
    assert!(Path::new(&result.artifact_path).is_dir());
    assert_eq!(fs::read_dir(&result.tx_data_path).unwrap().count(), 0);
    let manifest: PackageManifest = serde_json::from_slice(
        &fs::read(Path::new(&result.destination_path).join(MANIFEST)).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.usage, "private-user-copy");
    assert!(!String::from_utf8(
        fs::read(Path::new(&result.server_data_path).join("server.cfg")).unwrap()
    )
    .unwrap()
    .contains("fixture-rcon"));
    let mut import = request.clone();
    import.mode = CloneMode::Import;
    import.source_path = result.destination_path;
    import.destination_path = fixture.root.join("imported").to_string_lossy().into();
    import.server_port = 30122;
    import.tx_admin_port = 40122;
    let imported_plan = build_plan(&import).unwrap();
    let imported = execute_plan(&import, &imported_plan, || Ok(())).unwrap();
    assert!(
        fs::read_to_string(Path::new(&imported.server_data_path).join("server.cfg"))
            .unwrap()
            .contains("30122")
    );
    assert!(fs::read_to_string(fixture.source.join("server.cfg"))
        .unwrap()
        .contains("fixture-rcon"));
}

#[cfg(windows)]
#[test]
fn failed_copy_cleans_stage_and_never_promotes_partial_files() {
    let fixture = Fixture::new();
    let request = fixture.request();
    let plan = build_plan(&request).unwrap();
    fs::write(
        fixture.source.join("resources/[local]/demo/server.lua"),
        "changed fixture",
    )
    .unwrap();
    assert!(execute_plan(&request, &plan, || Ok(())).is_err());
    assert!(!fixture.root.join("target").exists());
    assert!(!fs::read_dir(&fixture.root).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".fxclone-stage-")));
    let plan = build_plan(&request).unwrap();
    assert!(execute_plan(&request, &plan, || Err("injected copy failure".into())).is_err());
    assert!(!fixture.root.join("target").exists());
}

#[cfg(windows)]
#[test]
fn late_destination_collision_is_not_overwritten() {
    let fixture = Fixture::new();
    let request = fixture.request();
    let plan = build_plan(&request).unwrap();
    assert!(execute_plan(&request, &plan, || {
        fs::create_dir(&plan.destination).unwrap();
        fs::write(plan.destination.join("keep.txt"), "keep").unwrap();
        Ok(())
    })
    .is_err());
    assert_eq!(
        fs::read_to_string(plan.destination.join("keep.txt")).unwrap(),
        "keep"
    );
    let stage = fixture.root.join("move-source");
    fs::create_dir(&stage).unwrap();
    assert!(promote(&stage, &plan.destination).is_err());
    assert!(stage.exists());
}

#[cfg(windows)]
#[test]
fn junctions_are_never_followed() {
    use std::os::windows::fs::symlink_dir;
    let fixture = Fixture::new();
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    let link = fixture.source.join("resources/[local]/demo/link");
    // Symlink creation needs Developer Mode; a directory junction does not.
    let linked = symlink_dir(&outside, &link).is_ok();
    if !linked {
        let status = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:FXCLONE_TEST_LINK -Target $env:FXCLONE_TEST_TARGET | Out-Null"])
            .env("FXCLONE_TEST_LINK", &link).env("FXCLONE_TEST_TARGET", &outside).status().unwrap();
        assert!(status.success());
    }
    assert!(build_plan(&fixture.request()).is_err());
    fs::remove_dir(link).unwrap();
}
