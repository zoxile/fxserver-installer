// Descriptor-relative filesystem operations shared by desktop and terminal services.
// A Unix directory descriptor does not pin its pathname as Windows sharing flags do.
use std::{
    ffi::{CString, OsStr},
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path, PathBuf},
};

fn error() -> String {
    std::io::Error::last_os_error().to_string()
}
fn name(value: &OsStr) -> Result<CString, String> {
    CString::new(value.as_bytes()).map_err(|_| "NUL in filesystem path.".into())
}
fn opened(fd: i32) -> Result<File, String> {
    if fd < 0 {
        return Err(error());
    }
    // The successful syscall transfers this new descriptor to File.
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn open_at(parent: &File, child: &OsStr, flags: i32) -> Result<File, String> {
    let child = name(child)?;
    opened(unsafe {
        libc::openat(
            parent.as_raw_fd(),
            child.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    })
}
pub(crate) fn directory(path: &Path) -> Result<File, String> {
    if !path.is_absolute() {
        return Err("An absolute local path is required.".into());
    }
    let mut file = opened(unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    })?;
    for part in path.components() {
        match part {
            Component::RootDir => (),
            Component::Normal(child) => {
                file = open_at(&file, child, libc::O_RDONLY | libc::O_DIRECTORY)?
            }
            _ => return Err("Path traversal is not allowed.".into()),
        }
    }
    Ok(file)
}
fn parent(path: &Path) -> Result<(File, &OsStr), String> {
    Ok((
        directory(path.parent().ok_or("Missing parent directory.")?)?,
        path.file_name().ok_or("Missing file name.")?,
    ))
}
pub(crate) fn read(path: &Path) -> Result<File, String> {
    let (parent, child) = parent(path)?;
    let file = open_at(&parent, child, libc::O_RDONLY)?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err("Only regular, non-hardlinked files are allowed.".into());
    }
    // Advisory locking coordinates app writers; callers still validate hashes/lengths.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } != 0 {
        return Err(error());
    }
    Ok(file)
}
pub(crate) fn unchanged(file: &File, before: &fs::Metadata) -> Result<(), String> {
    let after = file.metadata().map_err(|e| e.to_string())?;
    let stamp = |meta: &fs::Metadata| {
        (
            meta.dev(),
            meta.ino(),
            meta.len(),
            meta.mtime(),
            meta.mtime_nsec(),
            meta.ctime(),
            meta.ctime_nsec(),
            meta.nlink(),
        )
    };
    if stamp(before) != stamp(&after) {
        return Err("File changed while being read.".into());
    }
    Ok(())
}
pub(crate) fn create(path: &Path) -> Result<File, String> {
    let (parent, child) = parent(path)?;
    open_at(&parent, child, libc::O_RDWR | libc::O_CREAT | libc::O_EXCL)
}
pub(crate) fn mkdir(path: &Path) -> Result<(), String> {
    let (parent, child) = parent(path)?;
    let child = name(child)?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), child.as_ptr(), 0o700) } != 0 {
        return Err(error());
    }
    Ok(())
}
pub(crate) fn mkdir_all(path: &Path) -> Result<(), String> {
    if directory(path).is_ok() {
        return Ok(());
    }
    mkdir_all(
        path.parent()
            .filter(|p| *p != path)
            .ok_or("Invalid directory path.")?,
    )?;
    match mkdir(path) {
        Ok(()) => Ok(()),
        Err(_) => directory(path).map(|_| ()),
    }
}
pub(crate) fn available_bytes(path: &Path) -> Result<u64, String> {
    let directory = directory(path)?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::fstatvfs(directory.as_raw_fd(), stats.as_mut_ptr()) } != 0 {
        return Err(error());
    }
    let stats = unsafe { stats.assume_init() };
    (u128::from(stats.f_bavail) * u128::from(stats.f_frsize))
        .try_into()
        .map_err(|_| "Filesystem capacity overflow.".into())
}
pub(crate) fn rename(from: &Path, to: &Path, replace: bool) -> Result<(), String> {
    let (source, from) = parent(from)?;
    let (destination, to) = parent(to)?;
    let file = open_at(&source, from, libc::O_PATH)?;
    let original = file.metadata().map_err(|e| e.to_string())?;
    if !original.is_file() && !original.is_dir() {
        return Err("Only regular files and directories may be promoted.".into());
    }
    let (from_c, to_c) = (name(from)?, name(to)?);
    if unsafe {
        libc::renameat2(
            source.as_raw_fd(),
            from_c.as_ptr(),
            destination.as_raw_fd(),
            to_c.as_ptr(),
            if replace { 0 } else { libc::RENAME_NOREPLACE },
        )
    } != 0
    {
        return Err(error());
    }
    let moved = open_at(&destination, to, libc::O_PATH)?
        .metadata()
        .map_err(|e| e.to_string())?;
    if (original.dev(), original.ino()) != (moved.dev(), moved.ino()) {
        let restored = unsafe {
            libc::renameat2(
                destination.as_raw_fd(),
                to_c.as_ptr(),
                source.as_raw_fd(),
                from_c.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } == 0;
        return Err(format!(
            "Promotion source changed; {}.",
            if restored {
                "moved entry restored"
            } else {
                "moved entry preserved at the destination"
            }
        ));
    }
    Ok(())
}
pub(crate) fn token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("Secure randomness unavailable: {e}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
pub(crate) fn copy(from: &Path, to: &Path) -> Result<(), String> {
    let mut input = read(from)?;
    let temp = to.with_file_name(format!(".fxsi-copy-{}", token()?));
    let mut output = create(&temp)?;
    let metadata = input.metadata().map_err(|e| e.to_string())?;
    let size = metadata.len();
    if size > 512 * 1024 * 1024 {
        return Err("Copy exceeds the 512 MiB file limit.".into());
    }
    let copied = std::io::copy(&mut Read::by_ref(&mut input).take(size + 1), &mut output)
        .map_err(|e| e.to_string())?;
    if copied != size {
        return Err("Source changed during copy; temporary file preserved.".into());
    }
    unchanged(&input, &metadata)?;
    output
        .flush()
        .and_then(|_| output.sync_all())
        .map_err(|e| e.to_string())?;
    rename(&temp, to, true)
}
fn unlink(parent: &File, child: &OsStr, directory: bool) -> Result<(), String> {
    let child = name(child)?;
    if unsafe {
        libc::unlinkat(
            parent.as_raw_fd(),
            child.as_ptr(),
            if directory { libc::AT_REMOVEDIR } else { 0 },
        )
    } != 0
    {
        return Err(error());
    }
    Ok(())
}
pub(crate) fn remove_empty_directory(path: &Path) -> Result<(), String> {
    let (parent, child) = parent(path)?;
    open_at(&parent, child, libc::O_RDONLY | libc::O_DIRECTORY)?;
    unlink(&parent, child, true)
}
pub(crate) fn remove_file(path: &Path, expected: Option<&str>) -> Result<(), String> {
    remove_file_identity(path, expected, None)
}
pub(crate) fn remove_file_identity(
    path: &Path,
    expected: Option<&str>,
    identity: Option<(u64, u64)>,
) -> Result<(), String> {
    let (parent, child) = parent(path)?;
    // Verify after moving into a private directory. Other users with write access to
    // the original parent cannot substitute the name between verification and unlink.
    let quarantine = format!(".fxsi-remove-{}", token()?);
    let child_c = name(child)?;
    let quarantine_c = name(OsStr::new(&quarantine))?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), quarantine_c.as_ptr(), 0o700) } != 0 {
        return Err(error());
    }
    let protected = open_at(
        &parent,
        OsStr::new(&quarantine),
        libc::O_RDONLY | libc::O_DIRECTORY,
    )?;
    let protected_meta = protected.metadata().map_err(|e| e.to_string())?;
    if protected_meta.uid() != unsafe { libc::geteuid() } || protected_meta.mode() & 0o077 != 0 {
        return Err("Cleanup directory is not private; no file was removed.".into());
    }
    if unsafe {
        libc::renameat2(
            parent.as_raw_fd(),
            child_c.as_ptr(),
            protected.as_raw_fd(),
            c"entry".as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        let failure = error();
        let _ = unlink(&parent, OsStr::new(&quarantine), true);
        return Err(failure);
    }
    let result = (|| {
        let mut file = open_at(&protected, OsStr::new("entry"), libc::O_RDONLY)?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.nlink() != 1 {
            return Err("Linked or special file preserved.".into());
        }
        if identity.is_some_and(|id| id != (meta.dev(), meta.ino())) {
            return Err("Replaced file preserved.".into());
        }
        if let Some(expected) = expected {
            use sha2::{Digest, Sha256};
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 65536];
            loop {
                let size = file.read(&mut buffer).map_err(|e| e.to_string())?;
                if size == 0 {
                    break;
                }
                hash.update(&buffer[..size]);
            }
            if format!("{:x}", hash.finalize()) != expected {
                return Err("Changed file preserved.".into());
            }
        }
        unlink(&protected, OsStr::new("entry"), false)
    })();
    if let Err(failure) = result {
        let restored = unsafe {
            libc::renameat2(
                protected.as_raw_fd(),
                c"entry".as_ptr(),
                parent.as_raw_fd(),
                child_c.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } == 0;
        if restored {
            let _ = unlink(&parent, OsStr::new(&quarantine), true);
        }
        return Err(format!(
            "{failure} {}",
            if restored {
                "Original name restored.".into()
            } else {
                format!("Recovery file retained as {quarantine}/entry.")
            }
        ));
    }
    unlink(&parent, OsStr::new(&quarantine), true)
}
pub(crate) fn remove_tree(root: &Path, target: &Path) -> Result<(), String> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| "Cleanup is outside owned storage.")?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Refusing broad or traversing cleanup.".into());
    }
    let root = directory(root)?;
    let mut parent = root;
    let parts: Vec<_> = relative.components().collect();
    for part in &parts[..parts.len() - 1] {
        parent = open_at(
            &parent,
            part.as_os_str(),
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
    }
    fn remove_at(parent: &File, child: &OsStr, depth: usize) -> Result<(), String> {
        if depth > 64 {
            return Err("Cleanup tree is too deep.".into());
        }
        let dir = open_at(parent, child, libc::O_RDONLY | libc::O_DIRECTORY)?;
        let base = PathBuf::from(format!("/proc/self/fd/{}", dir.as_raw_fd()));
        for entry in fs::read_dir(base).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                remove_at(&dir, &entry.file_name(), depth + 1)?;
            } else if kind.is_file() {
                unlink(&dir, &entry.file_name(), false)?;
            } else {
                return Err("Cleanup refused a symbolic link or special file.".into());
            }
        }
        unlink(parent, child, true)
    }
    remove_at(&parent, parts.last().unwrap().as_os_str(), 0)
}
