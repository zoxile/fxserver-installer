use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::{
    backup_manager::storage,
    config_history::ensure_unlinked_path,
    fxserver::{decrypt_secret, encrypt_secret},
};
use crate::models::mariadb::MariaDBCredentials;

static CREDENTIAL_ACCESS: Mutex<()> = Mutex::new(());
const MAX_FILE_BYTES: u64 = 256 * 1024;
const STORAGE_ERROR: &str = "Could not access the Windows-protected database login. Forget it and validate your credentials again.";

#[derive(Serialize, Deserialize)]
struct SavedLogin {
    version: u8,
    workspace_id: String,
    credentials: MariaDBCredentials,
}

fn credential_path(root: &Path, workspace_id: &str) -> Result<PathBuf, String> {
    storage::validate_id(workspace_id)?;
    Ok(root.join(format!(
        "database-{}.json",
        workspace_id.to_ascii_lowercase()
    )))
}

fn root(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app
        .path()
        .app_local_data_dir()
        .map_err(|_| STORAGE_ERROR)?
        .join("database-logins");
    ensure_unlinked_path(&path).map_err(|_| STORAGE_ERROR)?;
    fs::create_dir_all(&path).map_err(|_| STORAGE_ERROR)?;
    Ok(path)
}

fn validate_size(credentials: &MariaDBCredentials) -> Result<(), String> {
    if credentials.host.is_empty()
        || credentials.host.len() > 255
        || credentials.port == 0
        || credentials.username.is_empty()
        || credentials.username.len() > 256
        || credentials.password.len() > 8192
        || credentials
            .database
            .as_ref()
            .is_some_and(|value| value.len() > 256)
        || credentials.host.chars().any(char::is_control)
    {
        return Err("Database login fields exceed the supported limits.".into());
    }
    Ok(())
}

fn save(root: &Path, workspace_id: String, credentials: MariaDBCredentials) -> Result<(), String> {
    validate_size(&credentials)?;
    let path = credential_path(root, &workspace_id)?;
    let plaintext = serde_json::to_vec(&SavedLogin {
        version: 1,
        workspace_id,
        credentials,
    })
    .map_err(|_| STORAGE_ERROR)?;
    let ciphertext = encrypt_secret(&plaintext).map_err(|_| STORAGE_ERROR)?;
    storage::save_json(&path, &ciphertext).map_err(|_| STORAGE_ERROR.to_string())
}

fn load(root: &Path, workspace_id: &str) -> Result<Option<MariaDBCredentials>, String> {
    let path = credential_path(root, workspace_id)?;
    let _directories = storage::pin_directories(root).map_err(|_| STORAGE_ERROR)?;
    if !path.try_exists().map_err(|_| STORAGE_ERROR)? {
        return Ok(None);
    }
    let file = storage::open_snapshot(&path).map_err(|_| STORAGE_ERROR)?;
    if file.metadata().map_err(|_| STORAGE_ERROR)?.len() > MAX_FILE_BYTES {
        return Err(STORAGE_ERROR.into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| STORAGE_ERROR)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(STORAGE_ERROR.into());
    }
    let ciphertext: Vec<u8> = serde_json::from_slice(&bytes).map_err(|_| STORAGE_ERROR)?;
    let plaintext = decrypt_secret(&ciphertext).map_err(|_| STORAGE_ERROR)?;
    let saved: SavedLogin = serde_json::from_slice(&plaintext).map_err(|_| STORAGE_ERROR)?;
    if saved.version != 1 || saved.workspace_id != workspace_id {
        return Err(STORAGE_ERROR.into());
    }
    validate_size(&saved.credentials)?;
    Ok(Some(saved.credentials))
}

fn clear(root: &Path, workspace_id: &str) -> Result<(), String> {
    let path = credential_path(root, workspace_id)?;
    let _directories = storage::pin_directories(root).map_err(|_| STORAGE_ERROR)?;
    if !path.try_exists().map_err(|_| STORAGE_ERROR)? {
        return Ok(());
    }
    let mut file = storage::open_snapshot(&path).map_err(|_| STORAGE_ERROR)?;
    let hash = storage::sha256(&mut file).map_err(|_| STORAGE_ERROR)?;
    drop(file);
    storage::remove_snapshot(&path, &hash).map_err(|_| STORAGE_ERROR.to_string())
}

#[tauri::command]
pub async fn save_database_login(
    app: AppHandle,
    workspace_id: String,
    credentials: MariaDBCredentials,
) -> Result<(), String> {
    super::run_blocking(move || {
        let _access = CREDENTIAL_ACCESS.lock().map_err(|_| STORAGE_ERROR)?;
        let _database = super::mariadb::database_access()?;
        validate_size(&credentials)?;
        crate::services::mariadb::query::validate_connection(credentials.clone())?;
        save(&root(&app)?, workspace_id, credentials)
    })
    .await
}

#[tauri::command]
pub async fn load_database_login(
    app: AppHandle,
    workspace_id: String,
) -> Result<Option<MariaDBCredentials>, String> {
    super::run_blocking(move || {
        let _access = CREDENTIAL_ACCESS.lock().map_err(|_| STORAGE_ERROR)?;
        load(&root(&app)?, &workspace_id)
    })
    .await
}

#[tauri::command]
pub async fn clear_database_login(app: AppHandle, workspace_id: String) -> Result<(), String> {
    super::run_blocking(move || {
        let _access = CREDENTIAL_ACCESS.lock().map_err(|_| STORAGE_ERROR)?;
        clear(&root(&app)?, &workspace_id)
    })
    .await
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn protected_login_round_trips_replaces_and_binds_workspace() {
        let root = std::env::temp_dir().join(format!("fxi-login-{}", storage::unique_id()));
        fs::create_dir(&root).unwrap();
        let mut credentials = MariaDBCredentials {
            host: "localhost".into(),
            port: 3306,
            username: "fixture".into(),
            password: "private-test-value".into(),
            database: None,
        };
        save(&root, "one".into(), credentials.clone()).unwrap();
        let path = credential_path(&root, "one").unwrap();
        assert!(!fs::read_to_string(&path)
            .unwrap()
            .contains(&credentials.password));
        assert_eq!(
            load(&root, "one").unwrap().unwrap().password,
            credentials.password
        );
        fs::copy(&path, credential_path(&root, "two").unwrap()).unwrap();
        assert!(load(&root, "two").is_err());
        credentials.password = "replacement".into();
        save(&root, "one".into(), credentials).unwrap();
        assert_eq!(load(&root, "one").unwrap().unwrap().password, "replacement");
        clear(&root, "one").unwrap();
        clear(&root, "one").unwrap();
        assert!(load(&root, "one").unwrap().is_none());
        fs::write(&path, b"not a protected login").unwrap();
        assert!(load(&root, "one").is_err());
        clear(&root, "one").unwrap();
        clear(&root, "two").unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn rejects_path_escape_and_oversized_credentials() {
        assert!(credential_path(Path::new("C:/temp"), "../other").is_err());
        let credentials = MariaDBCredentials {
            host: "localhost".into(),
            port: 3306,
            username: "fixture".into(),
            password: "x".repeat(8193),
            database: None,
        };
        assert!(validate_size(&credentials).is_err());
    }
}
