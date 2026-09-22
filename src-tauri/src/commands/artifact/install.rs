//! Bounded, staged ZIP installation shared by Legacy and Enhanced artifacts.
use super::super::server_exe::{self, ServerEdition};
use crate::commands::backup_manager::storage::{pin_directories, validate_local_path};
use crate::models::artifact::ArtifactInstallResult;
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_ARCHIVE: u64 = 2 * 1024 * 1024 * 1024;
const MAX_EXPANDED: u64 = 8 * 1024 * 1024 * 1024;
const MAX_FILE: u64 = 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 30_000;

#[derive(Default)]
struct DirectoryPins {
    handles: std::collections::BTreeMap<PathBuf, Vec<File>>,
    files: HashSet<PathBuf>,
}

impl DirectoryPins {
    fn ensure(&mut self, path: &Path, create: bool) -> Result<(), String> {
        validate_local_path(path)?;
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

    fn create_file(&mut self, path: &Path) -> Result<File, String> {
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
            regular_path(path)?;
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
}

fn regular_path(path: &Path) -> Result<(), String> {
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

fn archive_path(name: &str) -> Result<PathBuf, String> {
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
        let relative = archive_path(entry.name())?;
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
        let relative = archive_path(entry.name())?;
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
#[cfg(not(windows))]
fn no_running_server() -> Result<(), String> {
    Ok(())
}

fn lock_executable(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
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
        regular_path(&stage.join("files").join(relative))?;
        regular_path(&target)?;
        if target.exists() && !target.is_file() {
            return Err(format!(
                "Artifact target is not a file: {}",
                target.display()
            ));
        }
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
            regular_path(&target)?;
            let backup = stage.join("backup").join(relative);
            let existed = target.exists();
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
            if target.exists() && fs::remove_file(&target).is_err() {
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
) -> Result<ArtifactInstallResult, String> {
    if !cfg!(windows) {
        return Err("Artifact installation is only supported on Windows.".into());
    }
    let destination = PathBuf::from(destination.trim());
    validate_local_path(&destination)?;
    let mut pins = DirectoryPins::default();
    pins.ensure(&destination, true)?;
    regular_path(&destination)?;
    if let Some((_, found)) = server_exe::find_server_executable(&destination)? {
        if found != edition {
            return Err("Use separate folders for Legacy and Enhanced artifacts.".into());
        }
    }
    no_running_server()?;
    let stage = destination.join(format!(
        ".fxi-install-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
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
        let archive = stage.join("download.zip");
        let mut file = pins.create_file(&archive)?;
        let size = std::io::copy(&mut response.take(MAX_ARCHIVE + 1), &mut file)
            .map_err(|e| e.to_string())?;
        if size > MAX_ARCHIVE {
            return Err("Download exceeds archive size limit.".into());
        }
        file.flush().map_err(|e| e.to_string())?;
        let mut files = extract(file, &stage.join("files"), &mut pins)?;
        let detected = server_exe::find_server_executable(&stage.join("files"))?;
        if detected.map(|(_, found)| found) != Some(edition) {
            return Err(
                "Archive does not contain the expected server executable at its root.".into(),
            );
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
    fn staged_install_preserves_custom_data_and_rolls_back_failure() {
        let fixture = Fixture::new();
        let destination = fixture.0.join("destination");
        let stage = fixture.0.join("stage");
        fs::create_dir_all(destination.join("txData/default")).unwrap();
        fs::create_dir_all(stage.join("files")).unwrap();
        fs::write(destination.join("txData/default/config.json"), "custom").unwrap();
        fs::write(destination.join("cfx-server.exe"), "old executable").unwrap();
        fs::write(destination.join("first.dll"), "old dll").unwrap();
        fs::write(destination.join("missing.dll"), "old missing").unwrap();
        fs::write(stage.join("files/cfx-server.exe"), "new executable").unwrap();
        fs::write(stage.join("files/first.dll"), "new dll").unwrap();
        let mut files = vec![
            PathBuf::from("first.dll"),
            PathBuf::from("missing.dll"),
            PathBuf::from("cfx-server.exe"),
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
            fs::read_to_string(destination.join("cfx-server.exe")).unwrap(),
            "old executable"
        );
        fs::write(stage.join("files/first.dll"), "new dll").unwrap();
        fs::write(stage.join("files/missing.dll"), "new missing").unwrap();
        publish(
            &stage,
            &destination,
            &mut files,
            ServerEdition::Enhanced,
            &mut pins,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(destination.join("cfx-server.exe")).unwrap(),
            "new executable"
        );
        assert_eq!(
            fs::read_to_string(destination.join("txData/default/config.json")).unwrap(),
            "custom"
        );
    }

    #[test]
    fn extraction_of_regular_nested_files() {
        let fixture = Fixture::new();
        let archive = fixture.archive(
            &[
                ("cfx-server.exe", "fixture"),
                ("citizen/system/file.dll", "content"),
            ],
            false,
        );
        assert_eq!(
            extract_fixture(&archive, &fixture.0.join("out"))
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            fs::read_to_string(fixture.0.join("out/citizen/system/file.dll")).unwrap(),
            "content"
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
