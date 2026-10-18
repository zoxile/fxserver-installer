//! Linux descriptor-relative filesystem operations. No environment-provided temp paths.
use std::{
    ffi::{CString, OsStr},
    fs::{File, OpenOptions, Permissions},
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        },
    },
    path::{Component, Path, PathBuf},
};

pub fn secure_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    let mut offset = 0;
    while offset < bytes.len() {
        let count = unsafe {
            libc::getrandom(bytes[offset..].as_mut_ptr().cast(), bytes.len() - offset, 0)
        };
        if count < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err("Cannot obtain secure random bytes from the Linux kernel.".into());
        }
        if count == 0 {
            return Err("Linux random source returned no bytes.".into());
        }
        offset += count as usize;
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn open_at(directory: &File, name: &OsStr, flags: i32, mode: libc::mode_t) -> Result<File, String> {
    let name = CString::new(name.as_bytes()).map_err(|_| "Invalid filesystem name.")?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode,
        )
    };
    if fd < 0 {
        return Err(format!(
            "Cannot open protected filesystem object: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

pub fn pin_directories(path: &Path) -> Result<Vec<File>, String> {
    if !path.is_absolute() {
        return Err("An absolute Linux path is required.".into());
    }
    let mut handles = vec![OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open("/")
        .map_err(|e| e.to_string())?];
    for part in path.components() {
        match part {
            Component::RootDir => {}
            Component::Normal(name) => {
                let next = open_at(
                    handles.last().unwrap(),
                    name,
                    libc::O_RDONLY | libc::O_DIRECTORY,
                    0,
                )?;
                handles.push(next);
            }
            _ => return Err("Filesystem traversal is not supported.".into()),
        }
    }
    Ok(handles)
}

pub fn create_private_at(directory: &File, name: &OsStr) -> Result<File, String> {
    let file = open_at(
        directory,
        name,
        libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )?;
    // mode(0600) protects creation even under umask 000. Restore owner bits if umask is stricter.
    file.set_permissions(Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o777 != 0o600
    {
        return Err("Private file ownership or mode could not be verified.".into());
    }
    Ok(file)
}

pub struct PrivateCredential {
    pub path: PathBuf,
    _file: File,
}

impl PrivateCredential {
    pub fn create(contents: &str) -> Result<Self, String> {
        let handles = pin_directories(Path::new("/tmp"))?;
        let directory = handles.last().unwrap();
        let metadata = directory.metadata().map_err(|e| e.to_string())?;
        if metadata.uid() != 0
            || (metadata.mode() & 0o022 != 0 && metadata.mode() & libc::S_ISVTX == 0)
        {
            return Err("Credential storage requires a root-owned sticky /tmp directory.".into());
        }
        let name = CString::new(format!("fxi-client-{}.cnf", secure_token()?)).unwrap();
        let mut file = create_private_at(directory, OsStr::from_bytes(name.as_bytes()))?;
        // Unlink BEFORE writing secrets. The child opens the still-live descriptor through
        // procfs, so no attacker-controlled pathname can substitute its configuration.
        if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(
                "Cannot unlink the empty private credential file; no secrets were written.".into(),
            );
        }
        let path = PathBuf::from(format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            file.as_raw_fd()
        ));
        File::open(&path).map_err(|_| {
            "Linux procfs descriptor access is required for private MariaDB credentials."
        })?;
        file.write_all(contents.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot write private MariaDB credentials.")?;
        Ok(Self { path, _file: file })
    }
}

pub fn open_snapshot(path: &Path) -> Result<File, String> {
    let handles = pin_directories(path.parent().ok_or("Missing snapshot parent.")?)?;
    let file = open_at(
        handles.last().unwrap(),
        path.file_name().ok_or("Missing snapshot name.")?,
        libc::O_RDONLY | libc::O_NONBLOCK,
        0,
    )?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Snapshot must be a regular file.".into());
    }
    Ok(file)
}

pub fn available_bytes(path: &Path) -> Result<u64, String> {
    let handles = pin_directories(path)?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::fstatvfs(handles.last().unwrap().as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(format!(
            "Cannot inspect backup disk space: {}",
            std::io::Error::last_os_error()
        ));
    }
    let stat = unsafe { stat.assume_init() };
    (u128::from(stat.f_bavail) * u128::from(stat.f_frsize))
        .try_into()
        .map_err(|_| "Filesystem capacity overflow.".into())
}

pub fn sealed_snapshot(source: &mut File) -> Result<File, String> {
    use std::io::{Read, Seek, SeekFrom};
    let name = CString::new("fxi-restore-snapshot").unwrap();
    let fd =
        unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if fd < 0 {
        return Err(
            "Linux sealed memory files are required for immutable restore snapshots.".into(),
        );
    }
    let mut snapshot = unsafe { File::from_raw_fd(fd) };
    snapshot
        .set_permissions(Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    source.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let copied = std::io::copy(
        &mut (&mut *source).take(32 * 1024 * 1024 + 1),
        &mut snapshot,
    )
    .map_err(|e| e.to_string())?;
    source.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    if copied > 32 * 1024 * 1024 {
        return Err("Restore snapshots are limited to 32 MiB. No SQL was sent.".into());
    }
    let seals = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
    if unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, seals) } < 0 {
        return Err(
            "Cannot seal the restore snapshot against concurrent writes. No SQL was sent.".into(),
        );
    }
    snapshot
        .seek(SeekFrom::Start(0))
        .map_err(|e| e.to_string())?;
    Ok(snapshot)
}

pub fn save_json(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let handles = pin_directories(path.parent().ok_or("Missing settings parent.")?)?;
    let directory = handles.last().unwrap();
    let target = CString::new(
        path.file_name()
            .ok_or("Missing settings filename.")?
            .as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    let name = CString::new(format!(".fxi-settings-{}.tmp", secure_token()?)).unwrap();
    let mut file = create_private_at(directory, OsStr::from_bytes(name.as_bytes()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    if unsafe {
        libc::renameat(
            directory.as_raw_fd(),
            name.as_ptr(),
            directory.as_raw_fd(),
            target.as_ptr(),
        )
    } != 0
    {
        return Err(format!(
            "Cannot replace backup settings: {}",
            std::io::Error::last_os_error()
        ));
    }
    directory.sync_all().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_private_before_secrets_and_unlinked_while_alive() {
        let guard = PrivateCredential::create("").unwrap();
        let meta = guard._file.metadata().unwrap();
        assert_eq!(meta.mode() & 0o777, 0o600);
        assert_eq!(meta.nlink(), 0);
        assert_eq!(meta.len(), 0);
        assert_eq!(meta.uid(), unsafe { libc::geteuid() });
        let path = guard.path.clone();
        let identity = (meta.dev(), meta.ino());
        let witness = File::open(&path).unwrap();
        drop(guard);
        // Another parallel test may reuse the descriptor number, but not this live inode.
        assert_ne!(
            std::fs::metadata(&path).ok().map(|m| (m.dev(), m.ino())),
            Some(identity)
        );
        drop(witness);
        let guard = PrivateCredential::create("fixture-secret").unwrap();
        assert_eq!(
            std::fs::read_to_string(&guard.path).unwrap(),
            "fixture-secret"
        );
        // A real child must be able to reopen the parent descriptor despite CLOEXEC.
        let output = std::process::Command::new("/bin/cat")
            .arg(&guard.path)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"fixture-secret");
    }
    #[test]
    fn kernel_tokens_are_128_bit_hex() {
        let first = secure_token().unwrap();
        let second = secure_token().unwrap();
        assert_eq!(first.len(), 32);
        assert!(first.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }
    #[test]
    fn exclusive_creation_rejects_symlinks_without_touching_their_target() {
        use std::os::unix::fs::{symlink, DirBuilderExt};
        let root = PathBuf::from(format!(
            "/tmp/fxi-credential-test-{}",
            secure_token().unwrap()
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let target = root.join("target");
        std::fs::write(&target, "keep").unwrap();
        symlink(&target, root.join("link")).unwrap();
        let handles = pin_directories(&root).unwrap();
        assert!(create_private_at(handles.last().unwrap(), OsStr::new("link")).is_err());
        assert!(create_private_at(handles.last().unwrap(), OsStr::new("target")).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
        std::fs::remove_file(root.join("link")).unwrap();
        std::fs::remove_file(target).unwrap();
        drop(handles);
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn snapshots_reject_symlinks_at_every_component() {
        use std::os::unix::fs::{symlink, DirBuilderExt};
        let root = PathBuf::from(format!(
            "/tmp/fxi-snapshot-test-{}",
            secure_token().unwrap()
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        std::fs::write(root.join("snapshot.sql"), "fixture").unwrap();
        symlink(root.join("snapshot.sql"), root.join("link.sql")).unwrap();
        symlink(&root, root.join("directory-link")).unwrap();
        assert!(open_snapshot(&root.join("link.sql")).is_err());
        assert!(open_snapshot(&root.join("directory-link/snapshot.sql")).is_err());
        assert!(open_snapshot(&root.join("snapshot.sql")).is_ok());
        for name in ["snapshot.sql", "link.sql", "directory-link"] {
            std::fs::remove_file(root.join(name)).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn restore_snapshot_cannot_change_after_preflight() {
        let mut source = PrivateCredential::create("fixture").unwrap();
        let mut sealed = sealed_snapshot(&mut source._file).unwrap();
        assert!(sealed.write_all(b"changed").is_err());
        assert!(sealed.set_len(0).is_err());
        assert_eq!(
            unsafe { libc::fcntl(sealed.as_raw_fd(), libc::F_GET_SEALS) } & libc::F_SEAL_WRITE,
            libc::F_SEAL_WRITE
        );
    }
    #[test]
    #[ignore = "requires an installed MariaDB client; only prints fixture defaults, never connects"]
    fn mariadb_reads_unlinked_credentials_without_a_database_connection() {
        let guard = PrivateCredential::create(
            "[client]\nuser=fxi-fixture-user\npassword=fxi-fixture-password\n",
        )
        .unwrap();
        let output = std::process::Command::new("/usr/bin/mariadb")
            .arg(format!("--defaults-file={}", guard.path.display()))
            .arg("--print-defaults")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = String::from_utf8_lossy(&output.stdout);
        assert!(output.contains("fxi-fixture-user"));
        assert!(output.contains("fxi-fixture-password"));
    }
}
