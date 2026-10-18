use crate::{
    models::fxserver::{FxserverEnvironmentVariable, FxserverLaunchRequest},
    platform,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Settings {
    pub artifact_path: String,
    pub tx_data_path: String,
    pub profile: String,
    pub rcon_host: String,
    pub rcon_port: u16,
    pub database_host: String,
    pub database_port: u16,
    pub database_user: String,
    pub database: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            artifact_path: String::new(),
            tx_data_path: String::new(),
            profile: "default".into(),
            rcon_host: "127.0.0.1".into(),
            rcon_port: 30120,
            database_host: "localhost".into(),
            database_port: 3306,
            database_user: "root".into(),
            database: String::new(),
        }
    }
}

impl Settings {
    pub fn launch(&self) -> Result<FxserverLaunchRequest, String> {
        self.validate_server()?;
        Ok(FxserverLaunchRequest {
            artifact_path: self.artifact_path.clone(),
            server_profile: Some(self.profile.clone()),
            environment: vec![FxserverEnvironmentVariable {
                key: "TXHOST_DATA_PATH".into(),
                value: self.tx_data_path.clone(),
            }],
        })
    }

    pub fn validate_server(&self) -> Result<(), String> {
        for (label, value) in [
            ("Artifact folder", &self.artifact_path),
            ("txData folder", &self.tx_data_path),
        ] {
            if !Path::new(value).is_absolute() {
                return Err(format!("{label} must be an absolute path. Set it in Settings or with a command-line option."));
            }
        }
        if self.profile.is_empty()
            || self.profile.len() > 100
            || !self
                .profile
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err("Choose a valid txAdmin profile name.".into());
        }
        Ok(())
    }
}

pub(super) fn default_path() -> Result<PathBuf, String> {
    Ok(platform::data_dir()?.join("terminal.json"))
}

pub(super) fn load(path: &Path) -> Result<Settings, String> {
    crate::commands::config_history::ensure_unlinked_path(path)?;
    let file = match super::open_regular_file(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => return Err(format!("Cannot read terminal settings: {e}")),
    };
    if file.metadata().map_err(|e| e.to_string())?.len() > 64 * 1024 {
        return Err("Terminal settings exceed 64 KiB.".into());
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 {
        return Err("Terminal settings exceed 64 KiB.".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid terminal settings: {e}"))
}

pub(super) fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("Settings require an absolute path.")?;
    if !path.is_absolute() {
        return Err("Settings require an absolute path.".into());
    }
    prepare_parent(parent)?;
    crate::commands::backup_manager::storage::save_json(path, settings)
}

pub(super) fn write_example(path: &Path) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(&Settings::default()).map_err(|e| e.to_string())?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("Choose an absolute config path.")?;
    if !path.is_absolute() {
        return Err("Choose an absolute config path.".into());
    }
    prepare_parent(parent)?;
    let mut file = platform::private_options()
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Cannot create config (existing files are never overwritten): {e}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}

fn prepare_parent(parent: &Path) -> Result<(), String> {
    if parent == platform::data_dir()? {
        platform::private_directory(parent)
    } else {
        crate::commands::config_history::ensure_unlinked_path(parent)?;
        if parent.is_dir() {
            Ok(())
        } else {
            Err("Create the selected configuration directory first.".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_rejects_secret_fields_and_relative_server_paths() {
        assert!(serde_json::from_str::<Settings>(r#"{"password":"secret"}"#).is_err());
        assert!(Settings::default().launch().is_err());
        assert!(!serde_json::to_string(&Settings::default())
            .unwrap()
            .contains("password"));
    }
}
