//! Cooperative desktop/terminal admission, held at whole-operation boundaries.
//! Helpers called within a lease must not acquire another lease: a restore owns
//! exclusive access while it runs queries and captures its recovery backup.
//! A crashed parent can leave a child running after its lease is released.
use std::{
    fs::File,
    path::Path,
    sync::{RwLock, RwLockReadGuard, RwLockWriteGuard},
};

static DATABASE_ACCESS: RwLock<()> = RwLock::new(());
const LOCK_NAME: &str = "mariadb-access.lock";
const DATABASE_BUSY: &str = "MariaDB maintenance or a restore is in progress in another operation or desktop/terminal session. Try again after it finishes.";
const MAINTENANCE_BUSY: &str = "A MariaDB operation is in progress in another operation or desktop/terminal session. Wait for backups, restores, and queries to finish before changing the service.";

#[must_use = "Keep the lease alive until the entire database operation finishes."]
pub struct DatabaseLease {
    _file: FileLease,
    _local: RwLockReadGuard<'static, ()>,
}

#[must_use = "Keep the lease alive until maintenance and its verification finish."]
pub struct MaintenanceLease {
    _file: FileLease,
    _local: RwLockWriteGuard<'static, ()>,
}

/// Shared admission, not a guarantee that the SQL itself is read-only.
pub fn database_access() -> Result<DatabaseLease, String> {
    database_access_at(&DATABASE_ACCESS, &crate::platform::coordination_dir()?)
}

pub fn maintenance_access() -> Result<MaintenanceLease, String> {
    maintenance_access_at(&DATABASE_ACCESS, &crate::platform::coordination_dir()?)
}

fn database_access_at(
    local: &'static RwLock<()>,
    directory: &Path,
) -> Result<DatabaseLease, String> {
    let guard = local.try_read().map_err(|_| DATABASE_BUSY.to_owned())?;
    let file = open_lock(directory)?;
    fs2::FileExt::try_lock_shared(&file).map_err(|error| lock_error(error, DATABASE_BUSY))?;
    Ok(DatabaseLease {
        _file: FileLease(file),
        _local: guard,
    })
}

fn maintenance_access_at(
    local: &'static RwLock<()>,
    directory: &Path,
) -> Result<MaintenanceLease, String> {
    let guard = local.try_write().map_err(|_| MAINTENANCE_BUSY.to_owned())?;
    let file = open_lock(directory)?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|error| lock_error(error, MAINTENANCE_BUSY))?;
    Ok(MaintenanceLease {
        _file: FileLease(file),
        _local: guard,
    })
}

fn lock_error(error: std::io::Error, busy: &str) -> String {
    if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() {
        busy.to_owned()
    } else {
        format!("Cannot coordinate MariaDB operations: {error}. No operation was started.")
    }
}

struct FileLease(File);

impl Drop for FileLease {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
        // Never delete or replace the lock file: existing holders lock its inode.
    }
}

#[cfg(target_os = "linux")]
fn open_lock(directory: &Path) -> Result<File, String> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    };

    let directories = super::linux::filesystem::pin_directories(directory)?;
    let directory = directories
        .last()
        .ok_or("Missing coordination directory.")?;
    let metadata = directory.metadata().map_err(|e| e.to_string())?;
    let uid = unsafe { libc::geteuid() };
    if metadata.uid() != uid || metadata.mode() & 0o7777 != 0o700 {
        return Err(
            "MariaDB coordination storage must be owned by this user with mode 0700.".into(),
        );
    }
    let name = std::ffi::CString::new(LOCK_NAME).unwrap();
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(format!(
            "Cannot open the MariaDB coordination lock: {}",
            std::io::Error::last_os_error()
        ));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file()
        || metadata.uid() != uid
        || metadata.nlink() != 1
        || metadata.mode() & 0o7777 != 0o600
    {
        return Err(
            "MariaDB coordination lock must be an owned, single-link regular file with mode 0600."
                .into(),
        );
    }
    Ok(file)
}

#[cfg(windows)]
fn open_lock(directory: &Path) -> Result<File, String> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    // The shared platform helper validates the directory; do not follow a
    // reparse point at the final file or permit its removal while locked.
    let metadata = std::fs::symlink_metadata(directory).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
        return Err("MariaDB coordination storage must be an unlinked directory.".into());
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0x1 | 0x2)
        .custom_flags(0x0020_0000) // FILE_FLAG_OPEN_REPARSE_POINT
        .open(directory.join(LOCK_NAME))
        .map_err(|e| format!("Cannot open the MariaDB coordination lock: {e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
        return Err("MariaDB coordination lock must be an unlinked regular file.".into());
    }
    Ok(file)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn open_lock(_directory: &Path) -> Result<File, String> {
    Err("MariaDB operation coordination is unsupported on this platform.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            for _ in 0..100 {
                let path = std::env::temp_dir().join(format!(
                    "fxi-mariadb-access-test-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                let builder = fs::DirBuilder::new();
                #[cfg(unix)]
                let builder = {
                    use std::os::unix::fs::DirBuilderExt;
                    let mut builder = builder;
                    builder.mode(0o700);
                    builder
                };
                match builder.create(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("Cannot create isolated lock test directory: {error}"),
                }
            }
            panic!("Cannot allocate an isolated lock test directory.");
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            // Only test-owned files are removed, after every test lease is dropped.
            let _ = fs::remove_file(self.0.join(LOCK_NAME));
            let _ = fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn shared_operations_exclude_maintenance_until_last_reader_finishes() {
        static LOCAL: RwLock<()> = RwLock::new(());
        let directory = TestDirectory::new();
        let first = database_access_at(&LOCAL, &directory.0).unwrap();
        let second = database_access_at(&LOCAL, &directory.0).unwrap();
        assert!(maintenance_access_at(&LOCAL, &directory.0).is_err());
        drop(first);
        assert!(maintenance_access_at(&LOCAL, &directory.0).is_err());
        drop(second);
        drop(maintenance_access_at(&LOCAL, &directory.0).unwrap());
        assert!(directory.0.join(LOCK_NAME).is_file());
        drop(database_access_at(&LOCAL, &directory.0).unwrap());
    }

    #[test]
    fn exclusive_work_keeps_one_lease_across_nested_helpers() {
        static LOCAL: RwLock<()> = RwLock::new(());
        static OTHER_SESSION: RwLock<()> = RwLock::new(());
        let directory = TestDirectory::new();
        let lease = maintenance_access_at(&LOCAL, &directory.0).unwrap();
        let nested_query_or_backup = || {
            // Nested helpers use the caller's lease, never reacquire admission.
            let _borrow = &lease;
            assert!(database_access_at(&OTHER_SESSION, &directory.0).is_err());
            assert!(maintenance_access_at(&OTHER_SESSION, &directory.0).is_err());
        };
        nested_query_or_backup();
        nested_query_or_backup();
        assert!(database_access_at(&LOCAL, &directory.0).is_err());
        assert!(maintenance_access_at(&LOCAL, &directory.0).is_err());
        drop(lease);
        drop(database_access_at(&OTHER_SESSION, &directory.0).unwrap());
    }

    #[test]
    fn failed_file_acquisition_releases_local_admission() {
        static LOCAL: RwLock<()> = RwLock::new(());
        let directory = TestDirectory::new();
        assert!(maintenance_access_at(&LOCAL, &directory.0.join("missing")).is_err());
        drop(database_access_at(&LOCAL, &directory.0).unwrap());
        assert!(database_access_at(&LOCAL, &directory.0.join("missing")).is_err());
        drop(maintenance_access_at(&LOCAL, &directory.0).unwrap());
    }

    #[test]
    fn second_process_observes_contention_and_release() {
        static LOCAL: RwLock<()> = RwLock::new(());
        let directory = TestDirectory::new();
        let probe = |mode: &str, expected: &str| {
            let module = module_path!().split_once("::").unwrap().1;
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    &format!("{module}::lease_subprocess_probe"),
                    "--ignored",
                    "--nocapture",
                ])
                .env("FXI_TEST_MARIADB_LOCK_DIRECTORY", &directory.0)
                .env("FXI_TEST_MARIADB_LOCK_MODE", mode)
                .env("FXI_TEST_MARIADB_LOCK_EXPECTED", expected)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("lease probe completed"),
                "Subprocess test did not run."
            );
        };
        let shared = database_access_at(&LOCAL, &directory.0).unwrap();
        probe("shared", "acquired");
        probe("exclusive", "busy");
        drop(shared);
        let exclusive = maintenance_access_at(&LOCAL, &directory.0).unwrap();
        probe("shared", "busy");
        probe("exclusive", "busy");
        drop(exclusive);
        probe("exclusive", "acquired");
        drop(maintenance_access_at(&LOCAL, &directory.0).unwrap());
    }

    #[test]
    #[ignore = "Helper executed in a separate process by second_process_observes_contention_and_release"]
    fn lease_subprocess_probe() {
        static LOCAL: RwLock<()> = RwLock::new(());
        let Some(directory) = std::env::var_os("FXI_TEST_MARIADB_LOCK_DIRECTORY") else {
            return;
        };
        let directory = PathBuf::from(directory);
        let mode = std::env::var("FXI_TEST_MARIADB_LOCK_MODE").unwrap();
        let result = match mode.as_str() {
            "shared" => database_access_at(&LOCAL, &directory).map(drop),
            "exclusive" => maintenance_access_at(&LOCAL, &directory).map(drop),
            _ => panic!("Unexpected lease probe mode."),
        };
        match std::env::var("FXI_TEST_MARIADB_LOCK_EXPECTED")
            .unwrap()
            .as_str()
        {
            "acquired" => result.unwrap(),
            "busy" => assert_eq!(
                result.unwrap_err(),
                if mode == "shared" {
                    DATABASE_BUSY
                } else {
                    MAINTENANCE_BUSY
                }
            ),
            _ => panic!("Unexpected lease probe expectation."),
        }
        println!("lease probe completed");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn lock_is_private_and_rejects_links_and_permissive_modes() {
        use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
        static LOCAL: RwLock<()> = RwLock::new(());
        let directory = TestDirectory::new();
        let path = directory.0.join(LOCK_NAME);
        drop(database_access_at(&LOCAL, &directory.0).unwrap());
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o7777, 0o600);
        let target = directory.0.join("target");
        fs::rename(&path, &target).unwrap();
        symlink(&target, &path).unwrap();
        assert!(database_access_at(&LOCAL, &directory.0).is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&target, &path).unwrap();
        assert!(database_access_at(&LOCAL, &directory.0).is_err());
        fs::remove_file(&path).unwrap();
        fs::rename(&target, &path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(database_access_at(&LOCAL, &directory.0).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(database_access_at(&LOCAL, &directory.0).is_err());
        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700)).unwrap();
        drop(maintenance_access_at(&LOCAL, &directory.0).unwrap());
    }
}
