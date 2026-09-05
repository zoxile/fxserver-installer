use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

#[path = "restore_guard.rs"]
pub mod restore_guard;

pub fn secure_token() -> Result<String, String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Security::Cryptography::{
            BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        };
        let mut bytes = [0u8; 16];
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status < 0 {
            return Err("Secure random token generation failed.".into());
        }
        Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }
    #[cfg(not(windows))]
    {
        Err("Secure restore tests require Windows.".into())
    }
}

pub fn constrained_dump(file: &mut File) -> Result<restore_guard::DumpPlan, String> {
    if file.metadata().map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
        return Err("Restore testing supports snapshots up to 32 MiB. No SQL was sent.".into());
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut sql = String::new();
    file.take(32 * 1024 * 1024 + 1)
        .read_to_string(&mut sql)
        .map_err(|e| format!("Restore test requires a UTF-8 SQL snapshot: {e}"))?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let plan = restore_guard::preflight(&sql)?;
    if plan
        .tables
        .iter()
        .any(|table| table == "__fx_restore_owner")
    {
        return Err(
            "Restore test refused a reserved ownership table name. No SQL was sent.".into(),
        );
    }
    Ok(plan)
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn client_failure(diagnostics: &str) -> String {
    let code = diagnostics.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix("ERROR ")?;
        let code = rest.split_whitespace().next()?;
        (code.len() <= 5 && !code.is_empty() && code.bytes().all(|byte| byte.is_ascii_digit()))
            .then_some(code)
    });
    format!("MariaDB client failed{}. Client diagnostics are withheld because they can contain SQL data or credentials.",
        code.map(|code| format!(" (error {code})")).unwrap_or_default())
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
    validate_local_path(Path::new(output))?;
    let mut handles = pin_directories(Path::new(output))?;
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
        handles.extend(pin_directories(&path)?);
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
    let _handles = pin_directories(directory)?;
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
    check_metadata(&fs::symlink_metadata(path).map_err(|e| e.to_string())?)
}

fn check_metadata(metadata: &fs::Metadata) -> Result<(), String> {
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

pub fn validate_local_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("Choose an absolute local path without traversal.".into());
    }
    #[cfg(windows)]
    if !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
    {
        return Err("Network and device paths are not supported.".into());
    }
    for part in path.components() {
        if let Component::Normal(value) = part {
            let value = value
                .to_str()
                .ok_or("Non-Unicode paths are not supported.")?;
            let base = value.split('.').next().unwrap_or("").to_ascii_uppercase();
            if value.ends_with(['.', ' '])
                || value
                    .chars()
                    .any(|c| c.is_control() || r#"<>:\"/\\|?*"#.contains(c))
                || matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (base.len() == 4
                    && (base.starts_with("COM") || base.starts_with("LPT"))
                    && base.as_bytes()[3].is_ascii_digit())
            {
                return Err("Reserved or unsafe filesystem path component.".into());
            }
        }
    }
    Ok(())
}

// Keep these handles alive for the entire path-based operation, not just validation.
pub fn pin_directories(path: &Path) -> Result<Vec<File>, String> {
    validate_local_path(path)?;
    let mut handles = Vec::new();
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
                FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
            };
            // Attribute-only handles do not enforce Windows delete-sharing checks.
            let handle = OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(ancestor)
                .map_err(|e| format!("Cannot pin backup directory: {e}"))?;
            let metadata = handle.metadata().map_err(|e| e.to_string())?;
            check_metadata(&metadata)?;
            if !metadata.is_dir() {
                return Err("Backup path component is not a directory.".into());
            }
            handles.push(handle);
        }
        #[cfg(not(windows))]
        reject_link(ancestor)?;
    }
    Ok(handles)
}

pub fn open_snapshot(path: &Path) -> Result<File, String> {
    validate_local_path(path)?;
    let _handles = pin_directories(path.parent().ok_or("Missing snapshot parent.")?)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("Cannot lock backup for reading: {e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    check_metadata(&metadata)?;
    if !metadata.is_file() {
        return Err("Snapshot must be a regular file.".into());
    }
    Ok(file)
}

#[cfg(windows)]
pub fn remove_snapshot(path: &Path, expected_sha256: &str) -> Result<(), String> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FileDispositionInfo, SetFileInformationByHandle, DELETE, FILE_DISPOSITION_INFO,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_SHARE_READ,
    };
    validate_local_path(path)?;
    let _handles = pin_directories(path.parent().ok_or("Missing snapshot parent.")?)?;
    let mut file = OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | DELETE)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|e| format!("Cannot lock backup for retention: {e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    check_metadata(&metadata)?;
    if !metadata.is_file() || sha256(&mut file)? != expected_sha256 {
        return Err("Retention skipped a backup whose contents changed outside the app.".into());
    }
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // Delete exactly the object whose checksum was verified, without reopening its name.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            &disposition as *const _ as *const _,
            std::mem::size_of_val(&disposition) as u32,
        )
    } == 0
    {
        return Err(format!(
            "Could not remove verified backup: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn remove_snapshot(_path: &Path, _expected_sha256: &str) -> Result<(), String> {
    Err("Safe backup retention requires Windows.".into())
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
    validate_local_path(path)?;
    let _handles = pin_directories(path.parent().ok_or("Missing settings parent.")?)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let temp = path.with_extension(format!("{}.tmp", unique_id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        replace_file(&temp, path)
    })();
    // A failed write/rename is retained for recovery, never deleted by a stale name.
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
    fn client_errors_do_not_expose_sql_or_credentials() {
        let diagnostic = "--------------\nINSERT INTO x VALUES ('fixture-private-value')\n--------------\nERROR 1064 (42000) at line 1: syntax near fixture-private-value";
        let error = client_failure(diagnostic);
        assert!(error.contains("1064"));
        assert!(!error.contains("fixture-private-value"));
        assert!(!error.contains("INSERT"));
        assert!(!client_failure("ERROR secret-value arbitrary text").contains("secret-value"));
    }

    #[cfg(windows)]
    #[test]
    fn output_paths_reject_devices_streams_and_networks() {
        for value in [
            "relative.csv",
            "C:/out/file.csv:stream",
            "C:/out/NUL.csv",
            "C:/out/file. /x.csv",
            "C:/out/../x.csv",
            r"\\server\share\file.csv",
            r"\\.\C:\out\file.csv",
        ] {
            assert!(
                validate_local_path(Path::new(value)).is_err(),
                "accepted {value}"
            );
        }
        assert!(validate_local_path(Path::new("C:/out/fixture.csv")).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn retention_keeps_changed_files_and_deletes_only_the_verified_handle() {
        let root = std::env::temp_dir().join(format!("fx-retention-handle-{}", unique_id()));
        fs::create_dir(&root).unwrap();
        let path = root.join("fixture.sql");
        fs::write(&path, b"fixture").unwrap();
        let mut locked = open_snapshot(&path).unwrap();
        let expected = sha256(&mut locked).unwrap();
        assert!(fs::write(&path, b"replacement").is_err());
        assert!(fs::rename(&path, root.join("moved.sql")).is_err());
        assert!(remove_snapshot(&path, &expected).is_err());
        drop(locked);
        assert!(remove_snapshot(&path, "wrong").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"fixture");
        remove_snapshot(&path, &expected).unwrap();
        assert!(!path.exists());
        fs::remove_dir(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn snapshot_access_rejects_linked_ancestors_and_pins_directory_identity() {
        use std::os::windows::{fs::symlink_dir, process::CommandExt};
        let root = std::env::temp_dir().join(format!("fx-backup-links-{}", unique_id()));
        fs::create_dir(&root).unwrap();
        let original = root.join("original");
        fs::create_dir(&original).unwrap();
        fs::write(original.join("fixture.sql"), b"preserve").unwrap();
        let pins = pin_directories(&original).unwrap();
        assert!(fs::rename(&original, root.join("renamed")).is_err());
        drop(pins);
        let link = root.join("link");
        if symlink_dir(&original, &link).is_err() {
            assert!(std::process::Command::new("powershell")
                .creation_flags(0x08000000)
                .args(["-NoProfile", "-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:FX_BACKUP_LINK -Target $env:FX_BACKUP_TARGET | Out-Null"])
                .env("FX_BACKUP_LINK", &link).env("FX_BACKUP_TARGET", &original)
                .status().unwrap().success());
        }
        assert!(pin_directories(&link).is_err());
        assert!(open_snapshot(&link.join("fixture.sql")).is_err());
        assert!(remove_snapshot(&link.join("fixture.sql"), "wrong").is_err());
        assert_eq!(fs::read(original.join("fixture.sql")).unwrap(), b"preserve");
        fs::remove_dir(link).unwrap();
        fs::remove_file(original.join("fixture.sql")).unwrap();
        fs::remove_dir(original).unwrap();
        fs::remove_dir(root).unwrap();
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
