//! Reviewed restore tests never target existing databases. Receipts contain no login.
use super::{access, backup_storage as storage, owned_restore as owned};
use crate::models::mariadb::MariaDBCredentials;
use owned::{OwnedDatabase, OwnedDatabasePurpose};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};

const TTL: u64 = 120_000;
const MAX_DUMP_BYTES: u64 = 32 * 1024 * 1024;
static PREVIEWS: OnceLock<Mutex<HashMap<String, Permit>>> = OnceLock::new();

fn previews() -> &'static Mutex<HashMap<String, Permit>> {
    PREVIEWS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RestorePreview {
    token: String,
    confirmation: String,
    expires_at: u64,
    host: String,
    port: u16,
    source: String,
    sha256: String,
    tables: Vec<String>,
    statements: usize,
    receipt: String,
    notice: &'static str,
}

struct Permit {
    preview: RestorePreview,
    target: OwnedDatabase,
    credentials: MariaDBCredentials,
    file: File,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    version: u8,
    target: OwnedDatabase,
    source_sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RestoreResult {
    database: String,
    receipt: String,
    result: owned::OwnedImportResult,
    notice: &'static str,
}

fn receipt_path(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    storage::validate_local_path(&path)?;
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
    {
        return Err(
            "Choose an absolute .json receipt path in an existing private directory.".into(),
        );
    }
    Ok(path)
}

// The source must come from open_snapshot. Windows denies concurrent writers;
// Linux needs a bounded sealed copy before either preflight or hashing.
fn bounded_dump_snapshot(source: &mut File) -> Result<File, String> {
    if source.metadata().map_err(|e| e.to_string())?.len() > MAX_DUMP_BYTES {
        return Err("Restore snapshots are limited to 32 MiB. No SQL was sent.".into());
    }
    #[cfg(target_os = "linux")]
    {
        super::linux::filesystem::sealed_snapshot(source)
    }
    #[cfg(windows)]
    {
        source.try_clone().map_err(|e| e.to_string())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Err("Immutable restore snapshots are unavailable on this platform.".into())
    }
}

fn verified_dump_snapshot(source: &mut File, expected_sha256: &str) -> Result<File, String> {
    let mut snapshot = bounded_dump_snapshot(source)?;
    if storage::sha256(&mut snapshot)? != expected_sha256 {
        return Err("Dump changed since preview; no SQL was sent.".into());
    }
    Ok(snapshot)
}

pub(crate) async fn preview(
    credentials: MariaDBCredentials,
    source: String,
    receipt: String,
) -> Result<RestorePreview, String> {
    crate::commands::run_blocking(move || {
        let path = receipt_path(&receipt)?;
        let _directories =
            storage::pin_directories(path.parent().ok_or("Missing receipt parent.")?)?;
        if path.try_exists().map_err(|e| e.to_string())? {
            return Err("Receipt already exists; choose a new path.".into());
        }
        if !Path::new(&source).is_absolute() {
            return Err("Choose an absolute SQL dump path.".into());
        }
        let mut file = storage::open_snapshot(Path::new(&source))?;
        let mut snapshot = bounded_dump_snapshot(&mut file)?;
        let plan = storage::constrained_dump(&mut snapshot)?;
        let sha256 = storage::sha256(&mut snapshot)?;
        let target = OwnedDatabase::new(&credentials, OwnedDatabasePurpose::RestoreTest)?;
        let preview = RestorePreview {
            token: storage::secure_token()?,
            confirmation: target.database.clone(),
            expires_at: storage::now_ms() + TTL,
            host: credentials.host.clone(),
            port: credentials.port,
            source,
            sha256,
            tables: plan.tables,
            statements: plan.statements,
            receipt,
            notice: "Only a NEW app-owned test database is created. The constrained importer rejects unsupported SQL, routines, triggers and cross-database statements. A recovery receipt is written before any SQL. A failed import may leave a partial test database; use its receipt for ownership-checked cleanup.",
        };
        let mut pending = previews()
            .lock()
            .map_err(|_| "Restore preview lock unavailable.")?;
        pending.retain(|_, permit| permit.preview.expires_at > storage::now_ms());
        if pending.len() >= 4 {
            return Err("Too many pending restore previews; wait for expiration.".into());
        }
        pending.insert(
            preview.token.clone(),
            Permit {
                preview: preview.clone(),
                target,
                credentials,
                file,
            },
        );
        let token = preview.token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(TTL)).await;
            if let Ok(mut pending) = previews().lock() {
                pending.remove(&token);
            }
        });
        Ok(preview)
    })
    .await
}

fn validate_permit(
    permit: &Permit,
    credentials: &MariaDBCredentials,
    confirmation: &str,
    now: u64,
) -> Result<(), String> {
    if permit.preview.expires_at <= now
        || permit.preview.confirmation != confirmation
        || permit.credentials.host != credentials.host
        || permit.credentials.port != credentials.port
        || permit.credentials.username != credentials.username
        || permit.credentials.password != credentials.password
    {
        return Err("Restore preview expired, login changed, or exact confirmation did not match. Review again.".into());
    }
    permit.target.validate(credentials)
}

pub(crate) async fn apply(
    credentials: MariaDBCredentials,
    token: String,
    confirmation: String,
) -> Result<RestoreResult, String> {
    crate::commands::run_blocking(move || {
        if token.len() > 256 || confirmation.len() > 128 {
            return Err("Invalid restore confirmation.".into());
        }
        let mut permit = previews()
            .lock()
            .map_err(|_| "Restore preview lock unavailable.")?
            .remove(&token)
            .ok_or("Restore preview expired or was already used.")?;
        validate_permit(&permit, &credentials, &confirmation, storage::now_ms())?;
        let mut snapshot = verified_dump_snapshot(&mut permit.file, &permit.preview.sha256)?;
        let _lease = access::maintenance_access()?;
        let path = receipt_path(&permit.preview.receipt)?;
        let _directories =
            storage::pin_directories(path.parent().ok_or("Missing receipt parent.")?)?;
        let receipt = Receipt {
            version: 1,
            target: permit.target.clone(),
            source_sha256: permit.preview.sha256.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?;
        let mut record = crate::platform::private_options()
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("Cannot create recovery receipt; no SQL was sent: {e}"))?;
        record
            .write_all(&bytes)
            .and_then(|_| record.sync_all())
            .map_err(|e| format!("Cannot persist recovery receipt; no SQL was sent: {e}"))?;
        validate_permit(&permit, &credentials, &confirmation, storage::now_ms())?;
        let result = owned::import_dump(
            &permit.credentials,
            &mut snapshot,
            &permit.preview.sha256,
            &permit.target,
        );
        Ok(RestoreResult {
            database: permit.target.database,
            receipt: permit.preview.receipt,
            result,
            notice: "The test database is retained for inspection. Cleanup requires this receipt and the exact test database name. This is not a live-database restore.",
        })
    })
    .await
}

pub(crate) async fn cleanup(
    credentials: MariaDBCredentials,
    receipt: String,
    confirmation: String,
) -> Result<(), String> {
    crate::commands::run_blocking(move || {
        let path = receipt_path(&receipt)?;
        let mut file = storage::open_snapshot(&path)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(16 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 16 * 1024 {
            return Err("Restore receipt exceeds 16 KiB.".into());
        }
        let receipt: Receipt =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid restore receipt.")?;
        if receipt.version != 1
            || !matches!(receipt.target.purpose, OwnedDatabasePurpose::RestoreTest)
        {
            return Err("Not a supported restore-test receipt.".into());
        }
        receipt.target.validate(&credentials)?;
        if receipt.target.database != confirmation {
            return Err("Exact test database confirmation does not match.".into());
        }
        let _lease = access::maintenance_access()?;
        owned::cleanup_owned_database(&credentials, &receipt.target, &confirmation)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAFE_DUMP: &[u8] = include_bytes!("../../../tests/fixtures/restore-safe.sql");

    struct DumpFixture(PathBuf);

    impl DumpFixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "fxi-safe-restore-{}-{}",
                std::process::id(),
                storage::unique_id()
            ));
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("source.sql"), SAFE_DUMP).unwrap();
            Self(root)
        }

        fn path(&self) -> PathBuf {
            self.0.join("source.sql")
        }

        fn open(&self) -> File {
            storage::open_snapshot(&self.path()).unwrap()
        }

        fn resize(&self, size: u64) {
            File::options()
                .write(true)
                .open(self.path())
                .unwrap()
                .set_len(size)
                .unwrap();
        }
    }

    impl Drop for DumpFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dump_sha256() -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(SAFE_DUMP))
    }

    #[test]
    fn verified_snapshot_is_rewound_and_preserves_reviewed_metadata() {
        use std::io::Seek;
        let fixture = DumpFixture::new();
        let mut source = fixture.open();
        let mut snapshot = verified_dump_snapshot(&mut source, &dump_sha256()).unwrap();
        assert_eq!(snapshot.stream_position().unwrap(), 0);
        let plan = storage::constrained_dump(&mut snapshot).unwrap();
        assert_eq!(plan.tables, ["players"]);
        assert_eq!(storage::sha256(&mut snapshot).unwrap(), dump_sha256());
        assert!(verified_dump_snapshot(&mut source, &"0".repeat(64))
            .unwrap_err()
            .contains("Dump changed"));
        assert_eq!(std::fs::read(fixture.path()).unwrap(), SAFE_DUMP);
    }

    #[test]
    fn dump_snapshot_size_limit_is_exact() {
        let fixture = DumpFixture::new();
        fixture.resize(MAX_DUMP_BYTES);
        {
            let mut source = fixture.open();
            let snapshot = bounded_dump_snapshot(&mut source).unwrap();
            assert_eq!(snapshot.metadata().unwrap().len(), MAX_DUMP_BYTES);
        }
        fixture.resize(MAX_DUMP_BYTES + 1);
        let mut source = fixture.open();
        assert!(bounded_dump_snapshot(&mut source)
            .unwrap_err()
            .contains("32 MiB"));
        assert!(verified_dump_snapshot(&mut source, &dump_sha256())
            .unwrap_err()
            .contains("32 MiB"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn preview_plan_and_hash_use_the_same_sealed_bytes() {
        let fixture = DumpFixture::new();
        let mut source = fixture.open();
        let mut snapshot = bounded_dump_snapshot(&mut source).unwrap();
        let plan = storage::constrained_dump(&mut snapshot).unwrap();
        let changed = String::from_utf8_lossy(SAFE_DUMP).replace("players", "other_table");
        std::fs::write(fixture.path(), changed).unwrap();
        assert_eq!(plan.tables, ["players"]);
        assert_eq!(storage::sha256(&mut snapshot).unwrap(), dump_sha256());
        assert!(verified_dump_snapshot(&mut source, &dump_sha256())
            .unwrap_err()
            .contains("Dump changed"));
        assert!(snapshot.set_len(0).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn apply_snapshot_stays_immutable_after_source_changes_or_grows() {
        let fixture = DumpFixture::new();
        let mut source = fixture.open();
        let mut snapshot = verified_dump_snapshot(&mut source, &dump_sha256()).unwrap();
        std::fs::write(fixture.path(), b"not the reviewed dump").unwrap();
        fixture.resize(MAX_DUMP_BYTES + 1);
        assert_eq!(storage::sha256(&mut snapshot).unwrap(), dump_sha256());
        assert_eq!(
            storage::constrained_dump(&mut snapshot).unwrap().tables,
            ["players"]
        );
        assert!(verified_dump_snapshot(&mut source, &dump_sha256())
            .unwrap_err()
            .contains("32 MiB"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_snapshot_keeps_the_source_write_denial() {
        let fixture = DumpFixture::new();
        let mut source = fixture.open();
        let mut snapshot = verified_dump_snapshot(&mut source, &dump_sha256()).unwrap();
        drop(source);
        assert!(File::options().write(true).open(fixture.path()).is_err());
        assert_eq!(storage::sha256(&mut snapshot).unwrap(), dump_sha256());
        drop(snapshot);
        assert!(File::options().write(true).open(fixture.path()).is_ok());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn grown_dump_consumes_preview_before_receipt_or_database_access() {
        let fixture_dir = DumpFixture::new();
        let mut permit = fixture();
        permit.file = fixture_dir.open();
        permit.preview.sha256 = dump_sha256();
        permit.preview.expires_at = storage::now_ms() + TTL;
        let receipt = fixture_dir.0.join("receipt.json");
        permit.preview.receipt = receipt.to_string_lossy().into_owned();
        let token = permit.preview.token.clone();
        let confirmation = permit.preview.confirmation.clone();
        let credentials = permit.credentials.clone();
        previews().lock().unwrap().insert(token.clone(), permit);
        fixture_dir.resize(MAX_DUMP_BYTES + 1);
        let result = apply(credentials, token.clone(), confirmation).await;
        assert!(matches!(result, Err(error) if error.contains("32 MiB")));
        assert!(!previews().lock().unwrap().contains_key(&token));
        assert!(!receipt.exists());
    }

    fn fixture() -> Permit {
        let credentials = MariaDBCredentials {
            host: "fixture.invalid".into(),
            port: 3307,
            username: "fixture".into(),
            password: "private-password".into(),
            database: None,
        };
        let target = OwnedDatabase::new(&credentials, OwnedDatabasePurpose::RestoreTest).unwrap();
        let preview = RestorePreview {
            token: storage::secure_token().unwrap(),
            confirmation: target.database.clone(),
            expires_at: 100,
            host: credentials.host.clone(),
            port: credentials.port,
            source: "unused".into(),
            sha256: "a".repeat(64),
            tables: vec!["fixture".into()],
            statements: 2,
            receipt: "unused".into(),
            notice: "fixture",
        };
        Permit {
            preview,
            target,
            credentials,
            file: File::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap(),
        }
    }

    #[test]
    fn preview_is_bound_to_exact_target_login_and_deadline() {
        let permit = fixture();
        let confirmation = &permit.preview.confirmation;
        assert!(validate_permit(&permit, &permit.credentials, confirmation, 99).is_ok());
        assert!(validate_permit(&permit, &permit.credentials, confirmation, 100).is_err());
        assert!(validate_permit(&permit, &permit.credentials, "production", 99).is_err());
        let mut changed = permit.credentials.clone();
        changed.username = "other".into();
        assert!(validate_permit(&permit, &changed, confirmation, 99).is_err());
        changed = permit.credentials.clone();
        changed.host = "other.invalid".into();
        assert!(validate_permit(&permit, &changed, confirmation, 99).is_err());
        changed = permit.credentials.clone();
        changed.port = 3308;
        assert!(validate_permit(&permit, &changed, confirmation, 99).is_err());
        changed = permit.credentials.clone();
        changed.password = "replacement-password".into();
        assert!(validate_permit(&permit, &changed, confirmation, 99).is_err());
        changed.password.clear();
        assert!(validate_permit(&permit, &changed, confirmation, 99).is_err());
    }

    #[tokio::test]
    async fn changed_password_consumes_preview_before_database_access() {
        let mut permit = fixture();
        permit.preview.expires_at = storage::now_ms() + TTL;
        let mut credentials = permit.credentials.clone();
        credentials.password = "replacement-password".into();
        let token = permit.preview.token.clone();
        let confirmation = permit.preview.confirmation.clone();
        previews().lock().unwrap().insert(token.clone(), permit);
        let result = apply(credentials, token.clone(), confirmation).await;
        assert!(matches!(result, Err(error) if error.contains("login changed")));
        assert!(!previews().lock().unwrap().contains_key(&token));
    }

    #[tokio::test]
    async fn invalid_confirmation_consumes_preview_without_database_access() {
        let mut permit = fixture();
        permit.preview.expires_at = storage::now_ms() + TTL;
        let credentials = permit.credentials.clone();
        let token = permit.preview.token.clone();
        previews().lock().unwrap().insert(token.clone(), permit);
        assert!(
            apply(credentials.clone(), token.clone(), "wrong-target".into())
                .await
                .is_err()
        );
        let error = match apply(credentials, token, "wrong-target".into()).await {
            Err(error) => error,
            Ok(_) => panic!("Consumed token was reused"),
        };
        assert!(error.contains("already used"));
    }

    #[test]
    fn receipts_reject_relative_paths_and_wrong_extensions() {
        assert!(receipt_path("receipt.json").is_err());
        assert!(receipt_path("/tmp/receipt.sql").is_err());
    }

    #[test]
    fn receipt_serialization_never_contains_login_credentials() {
        let credentials = MariaDBCredentials {
            host: "fixture.invalid".into(),
            port: 3307,
            username: "private-user".into(),
            password: "private-password".into(),
            database: None,
        };
        let receipt = Receipt {
            version: 1,
            target: OwnedDatabase::new(&credentials, OwnedDatabasePurpose::RestoreTest).unwrap(),
            source_sha256: "a".repeat(64),
        };
        let encoded = serde_json::to_string(&receipt).unwrap();
        assert!(!encoded.contains(&credentials.password));
        assert!(!encoded.contains(&credentials.username));
        assert!(!encoded.contains("credentials"));
        assert!(receipt.target.validate(&credentials).is_ok());
    }
}
