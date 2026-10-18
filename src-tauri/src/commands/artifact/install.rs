//! Bounded, staged installation shared by desktop and terminal callers.
use super::super::server_exe::{self, ServerEdition};
use crate::commands::backup_manager::storage::{pin_directories, validate_local_path};
use crate::models::artifact::{ArtifactInstallResult, ArtifactPlatform};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) const MAX_ARCHIVE: u64 = 2 * 1024 * 1024 * 1024;
pub(super) const MAX_EXPANDED: u64 = 8 * 1024 * 1024 * 1024;
pub(super) const MAX_FILE: u64 = 1024 * 1024 * 1024;
pub(super) const MAX_ENTRIES: usize = 30_000;

#[derive(Default)]
pub(super) struct DirectoryPins {
    handles: std::collections::BTreeMap<PathBuf, Vec<File>>,
    files: HashSet<PathBuf>,
}

impl DirectoryPins {
    pub(super) fn ensure(&mut self, path: &Path, create: bool) -> Result<(), String> {
        validate_local_path(path)?;
        regular_path(path)?;
        for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
            if self.handles.contains_key(ancestor) {
                continue;
            }
            match fs::symlink_metadata(ancestor) {
                Ok(_) => {}
                Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
                    match fs::create_dir(ancestor) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error.to_string()),
                    }
                }
                Err(error) => return Err(error.to_string()),
            }
            let mut handles = pin_directories(ancestor)?;
            // Ancestors are already pinned; retain only this directory's handle.
            let own = handles.pop().into_iter().collect();
            self.handles.insert(ancestor.to_path_buf(), own);
        }
        Ok(())
    }

    pub(super) fn create_file(&mut self, path: &Path) -> Result<File, String> {
        self.ensure(path.parent().ok_or("Missing file parent.")?, true)?;
        validate_local_path(path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let file = options.open(path).map_err(|e| e.to_string())?;
        self.files.insert(path.to_path_buf());
        Ok(file)
    }

    fn cleanup(&mut self, stage: &Path) -> Result<(), String> {
        self.ensure(stage, false)?;
        // Remove only tracked files. Never recursively follow a directory entry during cleanup.
        for path in self.files.iter().filter(|path| path.starts_with(stage)) {
            replaceable_leaf(path)?;
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        let mut directories: Vec<_> = self
            .handles
            .keys()
            .filter(|path| path.starts_with(stage))
            .cloned()
            .collect();
        directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        for path in directories {
            // The parent remains pinned when releasing and removing this empty directory.
            self.handles.remove(&path);
            fs::remove_dir(&path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub(super) fn create_link(&mut self, path: &Path, target: &Path) -> Result<(), String> {
        self.ensure(path.parent().ok_or("Missing link parent.")?, true)?;
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, path).map_err(|e| e.to_string())?;
            self.files.insert(path.to_path_buf());
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = target;
            Err("Linux archive links can only be installed on Linux.".into())
        }
    }
}

// Existing leaf links may be replaced or removed, never opened or followed.
fn replaceable_leaf(path: &Path) -> Result<bool, String> {
    regular_path(path.parent().ok_or("Missing file parent.")?)?;
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            #[cfg(unix)]
            if meta.file_type().is_symlink() {
                return Ok(true);
            }
            regular_path(path)?;
            if !meta.is_file() {
                return Err(format!("Artifact target is not a file: {}", path.display()));
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

pub(super) fn regular_path(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err(format!(
                            "Reparse points are not allowed in artifact targets: {}",
                            ancestor.display()
                        ));
                    }
                }
                if meta.file_type().is_symlink() {
                    return Err("Linked artifact targets are not allowed.".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

pub(super) fn archive_path(name: &str) -> Result<PathBuf, String> {
    if name.len() > 1024
        || name.split('/').count() > 32
        || name.contains('\\')
        || name.starts_with('/')
    {
        return Err("Unsafe archive path.".into());
    }
    let name = name.strip_suffix('/').unwrap_or(name);
    for part in name.split('/') {
        let device = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || matches!(part, "." | "..")
            || part.ends_with(['.', ' '])
            || part
                .chars()
                .any(|c| c.is_control() || "<>:\"|?*".contains(c))
            || matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (device.len() == 4
                && (device.starts_with("COM") || device.starts_with("LPT"))
                && device.as_bytes()[3].is_ascii_digit())
        {
            return Err(format!("Unsafe archive path: {name}"));
        }
    }
    if name
        .split('/')
        .any(|part| part.eq_ignore_ascii_case("txData"))
        || name.eq_ignore_ascii_case(".fxserver-artifact-version")
        || name
            .split('/')
            .any(|part| part.to_ascii_lowercase().starts_with(".fxi-install"))
    {
        return Err("Archive must not contain txData or an installer version marker.".into());
    }
    Ok(PathBuf::from(name))
}

fn extract(
    archive: File,
    destination: &Path,
    pins: &mut DirectoryPins,
) -> Result<Vec<PathBuf>, String> {
    let mut zip = zip::ZipArchive::new(archive).map_err(|e| e.to_string())?;
    if zip.len() > MAX_ENTRIES {
        return Err("Too many archive entries.".into());
    }
    let mut total = 0u64;
    let mut names = HashSet::new();
    let mut files = Vec::new();
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let relative = archive_path(&entry.name().map_err(|e| e.to_string())?)?;
        let mode = entry.unix_mode().unwrap_or(0) & 0o170000;
        if !matches!(mode, 0 | 0o100000 | 0o040000) {
            return Err("Archive links and special files are not allowed.".into());
        }
        if !names.insert(relative.to_string_lossy().to_ascii_lowercase()) {
            return Err("Duplicate archive path.".into());
        }
        total = total
            .checked_add(entry.size())
            .ok_or("Archive size overflow.")?;
        if entry.size() > MAX_FILE || total > MAX_EXPANDED {
            return Err("Expanded archive exceeds size limits.".into());
        }
    }
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let relative = archive_path(&entry.name().map_err(|e| e.to_string())?)?;
        let target = destination.join(&relative);
        if entry.is_dir() {
            pins.ensure(&target, true)?;
            continue;
        }
        let mut file = pins.create_file(&target)?;
        let expected = entry.size();
        let actual = std::io::copy(&mut Read::by_ref(&mut entry).take(expected + 1), &mut file)
            .map_err(|e| e.to_string())?;
        if actual != expected {
            return Err("Archive entry length mismatch.".into());
        }
        files.push(relative);
    }
    Ok(files)
}

#[cfg(windows)]
fn no_running_server() -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::*,
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err("Cannot verify running server processes.".into());
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snapshot, &mut entry);
        if ok == 0 {
            CloseHandle(snapshot);
            return Err("Cannot enumerate running server processes.".into());
        }
        while ok != 0 {
            let end = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            if server_exe::is_server_process_name(&String::from_utf16_lossy(
                &entry.szExeFile[..end],
            )) {
                CloseHandle(snapshot);
                return Err("Stop all FXServer.exe and cfx-server.exe processes before installing artifacts.".into());
            }
            ok = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn no_running_server() -> Result<(), String> {
    for entry in
        fs::read_dir("/proc").map_err(|e| format!("Cannot inspect server processes: {e}"))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        let path = entry.path().join("cmdline");
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(format!(
                    "Cannot verify process {} is stopped: {e}",
                    entry.file_name().to_string_lossy()
                ))
            }
        };
        let mut bytes = Vec::new();
        file.take(64 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if linux_server_cmdline(&bytes) {
            return Err(
                "Stop all Linux FXServer and cfx-server processes before installing artifacts."
                    .into(),
            );
        }
    }
    Ok(())
}
#[cfg(any(target_os = "linux", test))]
fn linux_server_cmdline(bytes: &[u8]) -> bool {
    let mut args = bytes.split(|b| *b == 0);
    let program = args
        .next()
        .unwrap_or_default()
        .rsplit(|b| *b == b'/')
        .next()
        .unwrap_or_default();
    matches!(program, b"FXServer" | b"cfx-server")
        || ((program.starts_with(b"ld-musl-") || program.starts_with(b"ld-linux-"))
            && args.any(|arg| {
                let name = arg.rsplit(|b| *b == b'/').next().unwrap_or_default();
                matches!(name, b"FXServer" | b"cfx-server")
            }))
}
#[cfg(not(any(windows, target_os = "linux")))]
fn no_running_server() -> Result<(), String> {
    Ok(())
}

fn lock_executable(path: &Path) -> Result<File, String> {
    regular_path(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .share_mode(1 | 4)
            .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).map_err(|e| {
        format!(
            "Cannot lock {} for installation. Stop the server first: {e}",
            path.display()
        )
    })?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Server executable must be a regular file.".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Linked server executables cannot be replaced.".into());
        }
    }
    Ok(file)
}

fn publish(
    stage: &Path,
    destination: &Path,
    files: &mut [PathBuf],
    edition: ServerEdition,
    pins: &mut DirectoryPins,
) -> Result<(), String> {
    pins.ensure(destination, false)?;
    pins.ensure(stage, false)?;
    regular_path(destination)?;
    if let Some((_, found)) = server_exe::find_server_executable(destination)? {
        if found != edition {
            return Err("Use separate folders for Legacy and Enhanced artifacts.".into());
        }
    }
    // Include empty archive directories (Alpine has runtime directories with no files).
    let directories: Vec<_> = pins
        .handles
        .keys()
        .filter_map(|path| path.strip_prefix(stage.join("files")).ok())
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect();
    for relative in directories {
        pins.ensure(&destination.join(relative), true)?;
    }
    for relative in files.iter() {
        let target = destination.join(relative);
        pins.ensure(target.parent().ok_or("Missing target parent.")?, true)?;
        pins.ensure(
            stage
                .join("files")
                .join(relative)
                .parent()
                .ok_or("Missing staged parent.")?,
            false,
        )?;
        pins.ensure(
            stage
                .join("backup")
                .join(relative)
                .parent()
                .ok_or("Missing backup parent.")?,
            true,
        )?;
        replaceable_leaf(&stage.join("files").join(relative))?;
        replaceable_leaf(&target)?;
    }
    let old_exe = destination.join(edition.executable());
    let _old_lock = old_exe
        .exists()
        .then(|| lock_executable(&old_exe))
        .transpose()?;
    let _new_lock = lock_executable(&stage.join("files").join(edition.executable()))?;
    // Publish the executable last; keep write handles open until all files and marker are in place.
    files.sort_by_key(|path| {
        path.to_string_lossy()
            .eq_ignore_ascii_case(edition.executable())
    });
    let mut changed: Vec<(PathBuf, bool)> = Vec::new();
    let result: Result<(), String> = (|| {
        for relative in files.iter() {
            let target = destination.join(relative);
            let existed = replaceable_leaf(&target)?;
            let backup = stage.join("backup").join(relative);
            if existed {
                fs::rename(&target, &backup)
                    .map_err(|e| format!("Could not replace {}: {e}", target.display()))?;
                pins.files.insert(backup);
            }
            changed.push((relative.clone(), existed));
            fs::rename(stage.join("files").join(relative), &target).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut rollback_failed = false;
        for (relative, existed) in changed.into_iter().rev() {
            let target = destination.join(&relative);
            if replaceable_leaf(&target)
                .and_then(|exists| {
                    if exists {
                        fs::remove_file(&target).map_err(|e| e.to_string())
                    } else {
                        Ok(())
                    }
                })
                .is_err()
            {
                rollback_failed = true;
                continue;
            }
            if existed && fs::rename(stage.join("backup").join(&relative), &target).is_err() {
                rollback_failed = true;
            }
        }
        return Err(format!(
            "{error}{}",
            if rollback_failed {
                " Rollback incomplete; recovery files remain in the staging folder."
            } else {
                " Previous files restored."
            }
        ));
    }
    Ok(())
}

pub(super) fn install(
    url: &str,
    destination: &str,
    version: &str,
    edition: ServerEdition,
    platform: ArtifactPlatform,
) -> Result<ArtifactInstallResult, String> {
    if ArtifactPlatform::host()? != platform {
        return Err("The artifact platform does not match this host.".into());
    }
    let destination = PathBuf::from(destination.trim());
    validate_local_path(&destination)?;
    let mut pins = DirectoryPins::default();
    pins.ensure(&destination, true)?;
    regular_path(&destination)?;
    // Same canonical-directory lease used by server launch in every UI/process.
    // The manager guard is already held; keep this lease until cleanup completes.
    let _server_lease = crate::platform::ServerLease::acquire(&destination)?;
    if let Some((_, found)) = server_exe::find_server_executable(&destination)? {
        if found != edition {
            return Err("Use separate folders for Legacy and Enhanced artifacts.".into());
        }
    }
    no_running_server()?;
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let nonce = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let stage = destination.join(format!(".fxi-install-{}-{nonce}", std::process::id()));
    #[allow(unused_mut)]
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&stage).map_err(|e| e.to_string())?;
    pins.ensure(&stage, false)?;
    let preparation = (|| {
        let response = super::enhanced::client(Duration::from_secs(600))?
            .get(url)
            .send()
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "Download returned {}. Redirects are not accepted.",
                response.status()
            ));
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_ARCHIVE)
        {
            return Err("Download exceeds archive size limit.".into());
        }
        let archive = stage.join("download.archive");
        let mut file = pins.create_file(&archive)?;
        let size = std::io::copy(&mut response.take(MAX_ARCHIVE + 1), &mut file)
            .map_err(|e| e.to_string())?;
        if size > MAX_ARCHIVE {
            return Err("Download exceeds archive size limit.".into());
        }
        file.flush().map_err(|e| e.to_string())?;
        file.rewind().map_err(|e| e.to_string())?;
        let mut files = match platform {
            ArtifactPlatform::Windows => extract(file, &stage.join("files"), &mut pins)?,
            ArtifactPlatform::Linux => {
                super::tar_archive::extract(file, &stage.join("files"), &destination, &mut pins)?
            }
        };
        let detected = server_exe::find_server_executable(&stage.join("files"))?;
        if detected.map(|(_, found)| found) != Some(edition) {
            return Err("Archive does not contain the expected complete server layout.".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for relative in ["run.sh", edition.executable()] {
                let path = stage.join("files").join(relative);
                regular_path(&path)?;
                let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
                if !metadata.is_file() {
                    return Err("Server launch files must be regular files.".into());
                }
                fs::set_permissions(path, fs::Permissions::from_mode(0o755))
                    .map_err(|e| e.to_string())?;
            }
        }
        pins.create_file(&stage.join("files/.fxserver-artifact-version"))?
            .write_all(version.as_bytes())
            .map_err(|e| e.to_string())?;
        files.push(PathBuf::from(".fxserver-artifact-version"));
        Ok(files)
    })();
    let mut files = match preparation {
        Ok(files) => files,
        Err(error) => {
            return match pins.cleanup(&stage) {
                Ok(()) => Err(error),
                Err(cleanup) => Err(format!(
                    "{error} Staging files retained at {}: {cleanup}",
                    stage.display()
                )),
            };
        }
    };
    no_running_server()?;
    // Failed publication retains recovery files, never deleting the previous installation backup.
    publish(&stage, &destination, &mut files, edition, &mut pins)
        .map_err(|e| format!("{e} Staging folder: {}", stage.display()))?;
    pins.cleanup(&stage).map_err(|e| {
        format!(
            "Artifacts installed, but staging cleanup was incomplete at {}: {e}",
            stage.display()
        )
    })?;
    Ok(ArtifactInstallResult {
        version: version.into(),
        marker_path: destination
            .join(".fxserver-artifact-version")
            .to_string_lossy()
            .into(),
        destination: destination.to_string_lossy().into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "fxi-zip-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn archive(&self, entries: &[(&str, &str)], link: bool) -> PathBuf {
            let path = self.0.join("fixture.zip");
            let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            for (name, content) in entries {
                if link {
                    zip.add_symlink(*name, *content, options).unwrap();
                } else {
                    zip.start_file(*name, options).unwrap();
                    zip.write_all(content.as_bytes()).unwrap();
                }
            }
            zip.finish().unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn extract_fixture(archive: &Path, destination: &Path) -> Result<Vec<PathBuf>, String> {
        extract(
            File::open(archive).map_err(|e| e.to_string())?,
            destination,
            &mut DirectoryPins::default(),
        )
    }

    #[test]
    fn extraction_rejects_links_traversal_and_case_collisions_before_writing() {
        for (entries, link) in [
            (vec![("../escape", "bad")], false),
            (vec![("linked", "../escape")], true),
            (vec![("same.dll", "one"), ("SAME.dll", "two")], false),
        ] {
            let fixture = Fixture::new();
            let archive = fixture.archive(&entries, link);
            assert!(extract_fixture(&archive, &fixture.0.join("out")).is_err());
            assert!(!fixture.0.join("out").exists());
        }
    }

    #[test]
    fn extraction_rejects_nul_names_before_writing() {
        let fixture = Fixture::new();
        let archive = fixture.archive(&[("file.dll", "content")], false);
        let (local, central) = {
            let mut zip = zip::ZipArchive::new(File::open(&archive).unwrap()).unwrap();
            let entry = zip.by_index(0).unwrap();
            (
                entry.header_start() as usize,
                entry.central_header_start() as usize,
            )
        };
        let mut bytes = fs::read(&archive).unwrap();
        // Filenames follow the fixed local (30-byte) and central (46-byte) headers.
        bytes[local + 30] = 0;
        bytes[central + 46] = 0;
        fs::write(&archive, bytes).unwrap();
        let output = fixture.0.join("out");
        assert!(extract_fixture(&archive, &output)
            .unwrap_err()
            .contains("Unsafe archive path"));
        assert!(!output.exists());
    }

    #[test]
    fn extraction_rejects_crc_mismatch() {
        let fixture = Fixture::new();
        let archive = fixture.archive(&[("file.dll", "content")], false);
        let (local, central) = {
            let mut zip = zip::ZipArchive::new(File::open(&archive).unwrap()).unwrap();
            let entry = zip.by_index(0).unwrap();
            (
                entry.header_start() as usize,
                entry.central_header_start() as usize,
            )
        };
        let mut bytes = fs::read(&archive).unwrap();
        // Keep the headers consistent, but make their expected CRC differ from the payload.
        bytes[local + 14] ^= 1;
        bytes[central + 16] ^= 1;
        fs::write(&archive, bytes).unwrap();
        assert!(extract_fixture(&archive, &fixture.0.join("out"))
            .unwrap_err()
            .contains("checksum"));
    }

    #[test]
    fn staged_install_preserves_custom_data_and_rolls_back_failure() {
        let fixture = Fixture::new();
        let destination = fixture.0.join("destination");
        let stage = fixture.0.join("stage");
        let executable = ServerEdition::Enhanced.executable();
        fs::create_dir_all(destination.join("txData/default")).unwrap();
        fs::create_dir_all(stage.join("files")).unwrap();
        fs::create_dir_all(destination.join(executable).parent().unwrap()).unwrap();
        fs::create_dir_all(stage.join("files").join(executable).parent().unwrap()).unwrap();
        #[cfg(target_os = "linux")]
        {
            fs::write(destination.join("run.sh"), "launcher").unwrap();
            fs::write(stage.join("files/run.sh"), "launcher").unwrap();
        }
        fs::write(destination.join("txData/default/config.json"), "custom").unwrap();
        fs::write(destination.join(executable), "old executable").unwrap();
        fs::write(destination.join("first.dll"), "old dll").unwrap();
        fs::write(destination.join("missing.dll"), "old missing").unwrap();
        fs::write(
            destination.join(".fxserver-artifact-version"),
            "old version",
        )
        .unwrap();
        fs::write(stage.join("files").join(executable), "new executable").unwrap();
        fs::write(stage.join("files/first.dll"), "new dll").unwrap();
        fs::write(
            stage.join("files/.fxserver-artifact-version"),
            "new version",
        )
        .unwrap();
        let mut files = vec![
            PathBuf::from(".fxserver-artifact-version"),
            PathBuf::from("first.dll"),
            PathBuf::from("missing.dll"),
            PathBuf::from(executable),
        ];
        let mut pins = DirectoryPins::default();
        assert!(publish(
            &stage,
            &destination,
            &mut files,
            ServerEdition::Enhanced,
            &mut pins
        )
        .is_err());
        assert_eq!(
            fs::read_to_string(destination.join("first.dll")).unwrap(),
            "old dll"
        );
        assert_eq!(
            fs::read_to_string(destination.join("missing.dll")).unwrap(),
            "old missing"
        );
        assert_eq!(
            fs::read_to_string(destination.join(executable)).unwrap(),
            "old executable"
        );
        assert_eq!(
            fs::read_to_string(destination.join(".fxserver-artifact-version")).unwrap(),
            "old version"
        );
        fs::write(stage.join("files/first.dll"), "new dll").unwrap();
        fs::write(stage.join("files/missing.dll"), "new missing").unwrap();
        fs::write(
            stage.join("files/.fxserver-artifact-version"),
            "new version",
        )
        .unwrap();
        publish(
            &stage,
            &destination,
            &mut files,
            ServerEdition::Enhanced,
            &mut pins,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(destination.join(executable)).unwrap(),
            "new executable"
        );
        assert_eq!(
            fs::read_to_string(destination.join("txData/default/config.json")).unwrap(),
            "custom"
        );
        assert_eq!(
            fs::read_to_string(destination.join(".fxserver-artifact-version")).unwrap(),
            "new version"
        );
    }

    #[cfg(unix)]
    #[test]
    fn publication_replaces_link_leaves_without_touching_targets_and_rolls_back() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let destination = fixture.0.join("destination");
        let stage = fixture.0.join("stage");
        let executable = ServerEdition::Legacy.executable();
        fs::create_dir_all(destination.join(executable).parent().unwrap()).unwrap();
        fs::create_dir_all(stage.join("files").join(executable).parent().unwrap()).unwrap();
        fs::write(destination.join(executable), "old exe").unwrap();
        fs::write(destination.join("run.sh"), "launcher").unwrap();
        fs::write(stage.join("files").join(executable), "new exe").unwrap();
        let outside = fixture.0.join("outside");
        fs::write(&outside, "untouched").unwrap();
        symlink(&outside, destination.join("link")).unwrap();
        symlink("new-target", stage.join("files/link")).unwrap();
        let mut files = vec![
            PathBuf::from("link"),
            PathBuf::from("missing"),
            PathBuf::from(executable),
        ];
        let mut pins = DirectoryPins::default();
        assert!(publish(
            &stage,
            &destination,
            &mut files,
            ServerEdition::Legacy,
            &mut pins
        )
        .is_err());
        assert_eq!(fs::read_link(destination.join("link")).unwrap(), outside);
        assert_eq!(fs::read_to_string(&outside).unwrap(), "untouched");
        symlink("new-target", stage.join("files/link")).unwrap();
        fs::write(stage.join("files/missing"), "content").unwrap();
        publish(
            &stage,
            &destination,
            &mut files,
            ServerEdition::Legacy,
            &mut pins,
        )
        .unwrap();
        assert_eq!(
            fs::read_link(destination.join("link")).unwrap(),
            PathBuf::from("new-target")
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "untouched");
        pins.cleanup(&stage).unwrap();
    }

    #[test]
    fn recognizes_linux_servers_but_not_cli_destination_arguments() {
        assert!(linux_server_cmdline(
            b"/server/alpine/opt/cfx-server/FXServer\0+exec\0server.cfg\0"
        ));
        assert!(linux_server_cmdline(b"/server/alpine/lib/ld-musl-x86_64.so.1\0--library-path\0lib\0--\0/server/alpine/opt/cfx-server/cfx-server\0"));
        assert!(!linux_server_cmdline(
            b"fxserver-manager\0install\0--destination\0/home/FXServer\0"
        ));
        assert!(!linux_server_cmdline(b"bash\0-c\0FXServer\0"));
    }

    #[test]
    fn installation_cannot_start_while_another_session_holds_the_server_lease() {
        let fixture = Fixture::new();
        let destination = fixture.0.join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("keep.txt"), "unchanged").unwrap();
        let _lease = crate::platform::ServerLease::acquire(&destination).unwrap();
        // A deliberately unusable URL proves that exclusion occurs before download.
        let error = install(
            "not-a-url",
            destination.to_str().unwrap(),
            "123",
            ServerEdition::Legacy,
            ArtifactPlatform::host().unwrap(),
        )
        .unwrap_err();
        assert!(error.contains("already managed"), "{error}");
        assert_eq!(
            fs::read_to_string(destination.join("keep.txt")).unwrap(),
            "unchanged"
        );
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
    }

    #[test]
    fn extraction_of_regular_nested_files() {
        let fixture = Fixture::new();
        let archive = fixture.archive(
            &[
                ("cfx-server.exe", "fixture"),
                ("citizen/system/file.dll", "content"),
                ("citizen/caf\u{e9}.txt", "utf-8 filename"),
            ],
            false,
        );
        assert_eq!(
            extract_fixture(&archive, &fixture.0.join("out"))
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            fs::read_to_string(fixture.0.join("out/citizen/system/file.dll")).unwrap(),
            "content"
        );
        assert_eq!(
            fs::read_to_string(fixture.0.join("out/citizen/caf\u{e9}.txt")).unwrap(),
            "utf-8 filename"
        );
    }
    #[test]
    fn rejects_windows_traversal_aliases_and_data() {
        for path in [
            "../escape",
            "/escape",
            "C:/escape",
            "a\\..\\escape",
            "a:stream",
            "a/./b",
            "a//b",
            "NUL.txt",
            "a/COM1",
            "a./b",
            "txData/default/config.json",
            ".fxserver-artifact-version",
        ] {
            assert!(archive_path(path).is_err(), "{path}");
        }
        assert!(archive_path("citizen/system_resources/chat/fxmanifest.lua").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn destination_validation_rejects_network_device_ads_and_alias_paths() {
        for path in [
            r"\\server\share\artifacts",
            r"\\.\C:\artifacts",
            r"C:\artifacts:stream",
            r"C:\NUL\artifacts",
            r"C:\folder.\artifacts",
            r"C:\a\..\artifacts",
        ] {
            assert!(validate_local_path(Path::new(path)).is_err(), "{path}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn pins_block_directory_swaps_and_cleanup_never_recurses_into_untracked_junctions() {
        use crate::process::CommandNoWindowExt;
        let fixture = Fixture::new();
        let stage = fixture.0.join("stage");
        let outside = fixture.0.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep.txt"), "untouched").unwrap();
        let mut pins = DirectoryPins::default();
        pins.ensure(&stage.join("files"), true).unwrap();
        pins.create_file(&stage.join("files/owned.txt")).unwrap();
        assert!(fs::rename(stage.join("files"), stage.join("swapped")).is_err());
        let junction = stage.join("untracked");
        let output = std::process::Command::new("cmd")
            .no_window()
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(pins.ensure(&junction, false).is_err());
        assert!(pins.cleanup(&stage).is_err());
        assert_eq!(
            fs::read_to_string(outside.join("keep.txt")).unwrap(),
            "untouched"
        );
        fs::remove_dir(&junction).unwrap();
    }
}
