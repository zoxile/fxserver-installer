//! Invoked only by scripts/test-linux-mariadb.py in disposable GitHub containers.
use super::filesystem;
use crate::{
    commands::{
        backup_manager::owned_restore::{
            cleanup_owned_database, import_dump, OwnedDatabase, OwnedDatabasePurpose,
        },
        database_browser::query_json,
    },
    models::mariadb::{MariaDBBackupOptions, MariaDBCredentials},
    services::mariadb::{access, backup, backup_storage},
};
use serde::Deserialize;
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{symlink, DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    #[serde(flatten)]
    credentials: MariaDBCredentials,
    data_path: PathBuf,
}

fn load_fixture() -> Result<(Fixture, PathBuf), String> {
    for (key, value) in [
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("FXI_DISPOSABLE_MARIADB", "github-container-v1"),
    ] {
        if std::env::var(key).as_deref() != Ok(value) {
            return Err(format!("Missing disposable CI guard: {key}"));
        }
    }
    let run_id = std::env::var("GITHUB_RUN_ID").map_err(|e| e.to_string())?;
    if run_id.is_empty()
        || !run_id.bytes().all(|byte| byte.is_ascii_digit())
        || !Path::new("/.dockerenv").is_file()
        || unsafe { libc::geteuid() } != 10001
    {
        return Err("Only the dedicated disposable Linux CI user may run this test.".into());
    }
    let path = PathBuf::from(std::env::var("FXI_MARIADB_FIXTURE").map_err(|e| e.to_string())?);
    let root = path.parent().ok_or("Missing fixture directory.")?;
    if root.parent() != Some(Path::new("/tmp"))
        || !root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("fxi-mariadb-integration-"))
        || path.file_name().and_then(|name| name.to_str()) != Some("fixture.json")
    {
        return Err("Fixture descriptor must be in a test-owned /tmp directory.".into());
    }
    let file = filesystem::open_snapshot(&path)?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    let root_metadata = root.metadata().map_err(|e| e.to_string())?;
    if metadata.mode() & 0o777 != 0o600
        || metadata.uid() != 10001
        || metadata.nlink() != 1
        || metadata.len() > 16_384
        || root_metadata.mode() & 0o777 != 0o700
        || root_metadata.uid() != 10001
    {
        return Err("Fixture credentials must be private and test-owned.".into());
    }
    let mut contents = String::new();
    file.take(16_385)
        .read_to_string(&mut contents)
        .map_err(|e| e.to_string())?;
    let fixture: Fixture = serde_json::from_str(&contents).map_err(|e| e.to_string())?;
    if fixture.credentials.host != "127.0.0.1"
        || fixture.credentials.port < 1024
        || fixture.credentials.username != "fxi_ci_admin"
        || fixture.credentials.database.as_deref() != Some("fxi_ci_fixture")
        || !matches!(fixture.data_path.to_str(), Some("/var/lib/mysql"))
            && fixture.data_path != root.join("data")
    {
        return Err("Only the harness's loopback fixture is permitted.".into());
    }
    let mut marker = String::new();
    filesystem::open_snapshot(&fixture.data_path.join(".fxi-ci-owned"))?
        .take(128)
        .read_to_string(&mut marker)
        .map_err(|e| e.to_string())?;
    if marker != run_id {
        return Err("Data directory is not owned by this test run.".into());
    }
    let actual: Vec<String> = query_json(&fixture.credentials, "SELECT JSON_QUOTE(@@datadir);")?;
    if actual.len() != 1 || Path::new(&actual[0]) != fixture.data_path {
        return Err("Server data directory does not match the isolated fixture.".into());
    }
    Ok((fixture, root.to_path_buf()))
}

fn options(root: &Path) -> MariaDBBackupOptions {
    MariaDBBackupOptions {
        output_dir: root.to_string_lossy().into_owned(),
        file_name: Some("native-restore.sql".into()),
        database: Some("fxi_ci_fixture".into()),
        tables: Vec::new(),
        all_databases: false,
        schema_only: false,
        data_only: false,
        include_routines: false,
        include_triggers: false,
        include_events: false,
        single_transaction: true,
        add_drop_statements: false,
        where_clause: None,
    }
}

#[test]
#[ignore = "requires the disposable GitHub container harness and real MariaDB"]
fn live_backup_restore() -> Result<(), String> {
    let (fixture, root) = load_fixture()?;
    let credentials = &fixture.credentials;
    let _lease = access::maintenance_access()?;
    let directory = root.join("native-backup");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(|e| e.to_string())?;
    let result = backup::create_backup(credentials.clone(), options(&directory))?;
    let path = PathBuf::from(result.path);
    let mut file = filesystem::open_snapshot(&path)?;
    assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    let expected = backup_storage::sha256(&mut file)?;
    assert!(backup::create_backup(credentials.clone(), options(&directory)).is_err());
    assert_eq!(backup_storage::sha256(&mut file)?, expected);
    let alias = root.join("backup-alias");
    symlink(&directory, &alias).map_err(|e| e.to_string())?;
    assert!(backup::create_backup(credentials.clone(), options(&alias)).is_err());
    let plan = backup_storage::constrained_dump(&mut file)?;
    assert_eq!(plan.tables, ["items"]);

    let target = OwnedDatabase::new(credentials, OwnedDatabasePurpose::RestoreTest)?;
    let denied = import_dump(credentials, &mut file, &"0".repeat(64), &target);
    assert!(!denied.created);
    assert!(denied.error.is_some());
    let imported = import_dump(credentials, &mut file, &expected, &target);
    if let Some(error) = imported.error {
        return Err(error);
    }
    assert!(imported.created);
    assert_eq!(imported.tables_verified, ["items"]);
    let expected_rows: Vec<serde_json::Value> = query_json(
        credentials,
        "SELECT JSON_ARRAY(id,label) FROM fxi_ci_fixture.items ORDER BY id;",
    )?;
    let actual: Vec<serde_json::Value> = query_json(
        credentials,
        &format!(
            "SELECT JSON_ARRAY(id,label) FROM `{}`.items ORDER BY id;",
            target.database
        ),
    )?;
    assert_eq!(actual, expected_rows);
    assert!(cleanup_owned_database(credentials, &target, "wrong-confirmation").is_err());
    let mut impostor = target.clone();
    impostor.marker_token = "0".repeat(32);
    assert!(cleanup_owned_database(credentials, &impostor, &target.database).is_err());
    cleanup_owned_database(credentials, &target, &target.database)?;

    let malicious = directory.join("unsafe.sql");
    let mut malicious_file = filesystem::create_private_at(
        &fs::File::open(&directory).map_err(|e| e.to_string())?,
        malicious.file_name().unwrap(),
    )?;
    malicious_file
        .write_all(b"DROP DATABASE fxi_ci_fixture;\n")
        .map_err(|e| e.to_string())?;
    let malicious_hash = backup_storage::sha256(&mut malicious_file)?;
    let rejected = import_dump(credentials, &mut malicious_file, &malicious_hash, &target);
    assert!(!rejected.created);
    assert!(rejected.error.is_some());
    let still_present: Vec<serde_json::Value> = query_json(
        credentials,
        "SELECT JSON_ARRAY(id,label) FROM fxi_ci_fixture.items ORDER BY id;",
    )?;
    assert_eq!(still_present, expected_rows);
    println!("Verified real dump import, row equality, checksum rejection, ownership-gated cleanup and unsafe SQL rejection.");
    Ok(())
}
