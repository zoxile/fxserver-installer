use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn unique_id() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}-{}",
        now_ms(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

pub fn validate_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 100
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Invalid workspace or backup identifier.".into());
    }
    Ok(())
}

pub fn validate_database(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || value.trim() != value
        || value.starts_with('-')
        || value.chars().any(char::is_control)
        || ["mysql", "sys", "information_schema", "performance_schema"]
            .contains(&value.to_ascii_lowercase().as_str())
    {
        return Err("Choose a non-system database with a valid name.".into());
    }
    Ok(())
}

pub fn owned_directory(output: &str, workspace: &str, schedule: &str) -> Result<PathBuf, String> {
    validate_id(workspace)?;
    validate_id(schedule)?;
    let root = Path::new(output)
        .canonicalize()
        .map_err(|e| format!("Backup folder is unavailable: {e}"))?;
    if !root.is_dir() {
        return Err("Choose an existing backup folder.".into());
    }
    let mut path = root.clone();
    for part in ["fxserver-managed-backups", workspace, schedule] {
        path.push(part);
        if !path.exists() {
            fs::create_dir(&path).map_err(|e| e.to_string())?;
        }
        reject_link(&path)?;
        let resolved = path.canonicalize().map_err(|e| e.to_string())?;
        if !resolved.starts_with(&root) {
            return Err("Backup folder leaves the selected output directory.".into());
        }
    }
    path.canonicalize().map_err(|e| e.to_string())
}

pub fn snapshot_path(directory: &str, id: &str) -> Result<PathBuf, String> {
    validate_id(id)?;
    let directory = Path::new(directory);
    reject_link(directory)?;
    let root = directory.canonicalize().map_err(|e| e.to_string())?;
    let path = root.join(format!("{id}.sql"));
    reject_link(&path)?;
    if path.canonicalize().map_err(|e| e.to_string())?.parent() != Some(root.as_path()) {
        return Err("Backup file is outside its recorded folder.".into());
    }
    Ok(path)
}

fn reject_link(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Linked backup files and directories are not supported.".into());
        }
    }
    if metadata.file_type().is_symlink() {
        return Err("Linked backup files are not supported.".into());
    }
    Ok(())
}

pub fn open_snapshot(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    options
        .open(path)
        .map_err(|e| format!("Cannot lock backup for reading: {e}"))
}

pub fn sha256(file: &mut File) -> Result<String, String> {
    let mut hash = Sha256::new();
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(windows)]
pub fn available_bytes(path: &Path) -> Result<u64, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0;
    if unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(format!(
            "Could not check backup disk space: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(available)
}

#[cfg(not(windows))]
pub fn available_bytes(_path: &Path) -> Result<u64, String> {
    Err("Managed database backups require Windows.".into())
}

pub fn require_headroom(available: u64, estimate: u64) -> Result<(), String> {
    let required = estimate.saturating_mul(2).saturating_add(256 * 1024 * 1024);
    if available < required {
        return Err(format!(
            "Insufficient backup disk space: {} MiB available; at least {} MiB required.",
            available / 1048576,
            required / 1048576
        ));
    }
    Ok(())
}

pub fn save_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let temp = path.with_extension(format!("{}.tmp", unique_id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        replace_file(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(format!(
            "Cannot save backup settings: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> Result<(), String> {
    fs::rename(from, to).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_cannot_escape_owned_folders() {
        for invalid in ["../x", "a/b", "C:\\data", "..", "", "a b"] {
            assert!(validate_id(invalid).is_err());
        }
        assert!(validate_id("workspace-123_default").is_ok());
    }

    #[test]
    fn backups_exclude_system_databases_and_command_options() {
        for name in ["mysql", "SYS", "information_schema", "-x", "", "bad\nname"] {
            assert!(validate_database(name).is_err());
        }
        assert!(validate_database("qbx_main").is_ok());
    }

    #[test]
    fn disk_headroom_includes_growth_and_overflow_guard() {
        assert!(require_headroom(255 * 1048576, 0).is_err());
        assert!(require_headroom(512 * 1048576, 128 * 1048576).is_ok());
        assert!(require_headroom(u64::MAX - 1, u64::MAX).is_err());
    }

    #[test]
    fn atomic_settings_replace_is_readable() {
        let root = std::env::temp_dir().join(format!("fx-backup-test-{}", unique_id()));
        fs::create_dir(&root).unwrap();
        let path = root.join("settings.json");
        save_json(&path, &vec![1]).unwrap();
        save_json(&path, &vec![2, 3]).unwrap();
        assert_eq!(
            serde_json::from_slice::<Vec<u8>>(&fs::read(&path).unwrap()).unwrap(),
            vec![2, 3]
        );
        fs::remove_file(path).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn sha256_matches_known_vector_and_rewinds() {
        let path = std::env::temp_dir().join(format!("fx-backup-test-{}.sql", unique_id()));
        fs::write(&path, b"abc").unwrap();
        let mut file = open_snapshot(&path).unwrap();
        assert_eq!(
            sha256(&mut file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(file.stream_position().unwrap(), 0);
        drop(file);
        fs::remove_file(path).unwrap();
    }
}
