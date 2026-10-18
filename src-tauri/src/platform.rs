use sha2::{Digest, Sha256};
#[cfg(all(test, target_os = "linux", feature = "desktop"))]
mod glib_regression;

use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

pub(crate) fn data_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or("LOCALAPPDATA is unavailable.")?;
    #[cfg(not(windows))]
    let root = std::env::var_os("XDG_DATA_HOME")
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|p| Path::new(p).is_absolute())
                .map(|p| PathBuf::from(p).join(".local/share"))
        })
        .ok_or("Set HOME or an absolute XDG_DATA_HOME before starting the application.")?;
    Ok(root.join("com.fxserver.installer"))
}

pub(crate) fn private_directory(path: &Path) -> Result<(), String> {
    crate::commands::config_history::ensure_unlinked_path(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|e| e.to_string())?;
        let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(
                "Application storage must be owned by the current user and not linked.".into(),
            );
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(path).map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    options
}

pub(crate) fn coordination_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    let root = PathBuf::from(format!("/tmp/fxserver-installer-locks-{}", unsafe {
        libc::geteuid()
    }));
    #[cfg(not(target_os = "linux"))]
    let root = data_dir()?.join("locks");
    private_directory(&root)?;
    Ok(root)
}

pub(crate) struct ServerLease(File);

impl ServerLease {
    pub(crate) fn acquire(artifact: &Path) -> Result<Self, String> {
        let canonical = artifact
            .canonicalize()
            .map_err(|e| format!("Artifact folder is unavailable: {e}"))?;
        let mut identity = canonical.to_string_lossy().into_owned();
        if cfg!(windows) {
            identity.make_ascii_lowercase();
        }
        let root = coordination_dir()?.join("servers");
        private_directory(&root)?;
        let path = root.join(format!("{:x}.lock", Sha256::digest(identity.as_bytes())));
        crate::commands::config_history::ensure_unlinked_path(&path)?;
        let file = private_options()
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| e.to_string())?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|_| {
            "This artifact folder is already managed by another desktop or terminal session."
        })?;
        Ok(Self(file))
    }
}

impl Drop for ServerLease {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

pub(crate) fn secure_storage_supported() -> bool {
    #[cfg(windows)]
    {
        true
    }
    #[cfg(target_os = "linux")]
    {
        Path::new("/usr/bin/secret-tool").is_file()
            && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}

#[cfg(target_os = "linux")]
mod secrets;
#[cfg(target_os = "linux")]
pub(crate) use secrets::{decrypt_secret, encrypt_secret};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lease_is_exclusive_and_released() {
        let dir = std::env::temp_dir().join(format!(
            "fxi-lease-{}",
            crate::commands::backup_manager::storage::unique_id()
        ));
        fs::create_dir(&dir).unwrap();
        let first = ServerLease::acquire(&dir).unwrap();
        assert!(ServerLease::acquire(&dir).is_err());
        drop(first);
        drop(ServerLease::acquire(&dir).unwrap());
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn lease_coordinates_processes_with_different_data_roots() {
        let dir = std::env::temp_dir().join(format!(
            "fxi-process-lease-{}",
            crate::commands::backup_manager::storage::unique_id()
        ));
        fs::create_dir(&dir).unwrap();
        let probe = |blocked: bool| {
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "platform::tests::lease_child_probe",
                    "--nocapture",
                ])
                .env("FXI_TEST_LEASE_PATH", &dir)
                .env("FXI_TEST_LEASE_BLOCKED", if blocked { "1" } else { "0" })
                .env("XDG_DATA_HOME", dir.join("separate-app-data"))
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stdout)
            );
        };
        let lease = ServerLease::acquire(&dir).unwrap();
        probe(true);
        drop(lease);
        probe(false);
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn lease_child_probe() {
        if let Some(path) = std::env::var_os("FXI_TEST_LEASE_PATH") {
            let blocked = std::env::var("FXI_TEST_LEASE_BLOCKED").unwrap() == "1";
            assert_eq!(ServerLease::acquire(Path::new(&path)).is_err(), blocked);
        }
    }
}
