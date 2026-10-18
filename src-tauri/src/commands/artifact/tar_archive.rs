//! Alpine archives contain absolute rootfs links. Rebase them into alpine/, validate
//! the complete graph, and create links only after all regular-file writes finish.
use super::install::{
    archive_path, DirectoryPins, MAX_ARCHIVE, MAX_ENTRIES, MAX_EXPANDED, MAX_FILE,
};
use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
struct Limits {
    file: u64,
    expanded: u64,
    entries: usize,
    stream: u64,
    timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            file: MAX_FILE,
            expanded: MAX_EXPANDED,
            entries: MAX_ENTRIES,
            stream: MAX_EXPANDED + 64 * 1024 * 1024,
            timeout: Duration::from_secs(600),
        }
    }
}

struct BoundedReader<R> {
    inner: R,
    remaining: u64,
    deadline: Instant,
}

impl<R: Read> Read for BoundedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if Instant::now() >= self.deadline {
            return Err(io::Error::other("Archive extraction timed out."));
        }
        let cap = buf
            .len()
            .min(64 * 1024)
            .min(self.remaining.saturating_add(1) as usize);
        let count = self.inner.read(&mut buf[..cap])?;
        self.remaining = self
            .remaining
            .checked_sub(count as u64)
            .ok_or_else(|| io::Error::other("Expanded archive stream exceeds size limit."))?;
        if Instant::now() >= self.deadline {
            return Err(io::Error::other("Archive extraction timed out."));
        }
        Ok(count)
    }
}

#[derive(Clone, Debug)]
enum Kind {
    File,
    Directory,
    Link(PathBuf),
}

pub(super) fn extract(
    archive: File,
    destination: &Path,
    installation: &Path,
    pins: &mut DirectoryPins,
) -> Result<Vec<PathBuf>, String> {
    extract_with_limits(archive, destination, installation, pins, Limits::default())
}

fn extract_with_limits(
    archive: File,
    destination: &Path,
    installation: &Path,
    pins: &mut DirectoryPins,
    limits: Limits,
) -> Result<Vec<PathBuf>, String> {
    let manifest = read_archive(archive, limits, |relative, kind, mode, entry, size| {
        let target = destination.join(relative);
        match kind {
            Kind::Directory => pins.ensure(&target, true)?,
            Kind::File => {
                let mut output = pins.create_file(&target)?;
                let actual =
                    io::copy(&mut entry.take(size + 1), &mut output).map_err(|e| e.to_string())?;
                if actual != size {
                    return Err("Archive entry length mismatch.".into());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    // Preserve execute bits, not setuid/setgid/sticky or group/world write.
                    output
                        .set_permissions(std::fs::Permissions::from_mode(0o644 | (mode & 0o111)))
                        .map_err(|e| e.to_string())?;
                }
                #[cfg(not(unix))]
                let _ = mode;
            }
            Kind::Link(_) => {}
        }
        Ok(())
    })?;
    let mut files = Vec::new();
    for (relative, kind) in &manifest {
        match kind {
            Kind::Directory => {}
            Kind::File => files.push(relative.clone()),
            Kind::Link(target) => {
                let resolved = resolve_target(target, &manifest)?;
                // A dangling bundled link must not acquire an external target through
                // an existing installation's symlink. Never follow that path.
                for ancestor in resolved.ancestors().filter(|p| !p.as_os_str().is_empty()) {
                    if !manifest.contains_key(ancestor) {
                        super::install::regular_path(&installation.join(ancestor))?;
                    }
                }
                pins.create_link(
                    &destination.join(relative),
                    &relative_target(relative, target),
                )?;
                files.push(relative.clone());
            }
        }
    }
    Ok(files)
}

fn read_archive(
    archive: File,
    limits: Limits,
    mut visit: impl FnMut(&Path, &Kind, u32, &mut dyn Read, u64) -> Result<(), String>,
) -> Result<BTreeMap<PathBuf, Kind>, String> {
    let size = archive.metadata().map_err(|e| e.to_string())?.len();
    if size > MAX_ARCHIVE {
        return Err("Download exceeds archive size limit.".into());
    }
    // Bound the decoder dictionary as well as the decoded bytes, including TAR metadata.
    let stream =
        xz2::stream::Stream::new_stream_decoder(256 * 1024 * 1024, 0).map_err(|e| e.to_string())?;
    let decoder = xz2::read::XzDecoder::new_stream(archive, stream);
    let mut reader = BoundedReader {
        inner: decoder,
        remaining: limits.stream,
        deadline: Instant::now() + limits.timeout,
    };
    let mut manifest = BTreeMap::new();
    let mut total = 0u64;
    let mut next_path = None;
    let mut next_link = None;
    {
        let mut tar = tar::Archive::new(&mut reader);
        // Raw iteration prevents unbounded allocations for GNU/PAX extension records.
        for (index, entry) in tar
            .entries()
            .map_err(|e| e.to_string())?
            .raw(true)
            .enumerate()
        {
            if index >= limits.entries {
                return Err("Too many archive entries.".into());
            }
            let mut entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.header().entry_type();
            let size = entry.size();
            if kind.is_gnu_longname() || kind.is_gnu_longlink() {
                if size > 4096 {
                    return Err("Archive name extension exceeds size limit.".into());
                }
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
                let name = std::str::from_utf8(&bytes)
                    .map_err(|_| "Archive names must be UTF-8.")?
                    .trim_end_matches('\0')
                    .to_owned();
                let slot = if kind.is_gnu_longname() {
                    &mut next_path
                } else {
                    &mut next_link
                };
                if slot.replace(name).is_some() {
                    return Err("Duplicate archive name extension.".into());
                }
                continue;
            }
            // Official archives use regular GNU/USTAR entries. Refuse PAX/sparse records
            // rather than let them change sizes, paths, ownership, or filesystem metadata.
            if !kind.is_file() && !kind.is_dir() && !kind.is_symlink() {
                return Err(
                "Archive hard links, special files, and unsupported extensions are not allowed."
                    .into(),
            );
            }
            if size > limits.file {
                return Err("Archive file exceeds size limit.".into());
            }
            total = total.checked_add(size).ok_or("Archive size overflow.")?;
            if total > limits.expanded {
                return Err("Expanded archive exceeds size limit.".into());
            }
            if !kind.is_file() && size != 0 {
                return Err("Non-file archive entry contains data.".into());
            }
            let name = next_path.take().map(Ok).unwrap_or_else(|| {
                std::str::from_utf8(&entry.path_bytes())
                    .map(str::to_owned)
                    .map_err(|_| "Archive names must be UTF-8.")
            })?;
            if matches!(name.as_str(), "." | "./") && kind.is_dir() {
                if next_link.is_some() {
                    return Err("Unexpected archive link extension.".into());
                }
                continue;
            }
            let relative = archive_path(name.strip_prefix("./").unwrap_or(&name))?;
            let record = if kind.is_symlink() {
                let link = next_link.take().map(Ok).unwrap_or_else(|| {
                    entry
                        .link_name_bytes()
                        .ok_or("Archive link is missing its target.")
                        .and_then(|value| {
                            std::str::from_utf8(&value)
                                .map(str::to_owned)
                                .map_err(|_| "Archive link target must be UTF-8.")
                        })
                })?;
                Kind::Link(link_target(&relative, &link)?)
            } else {
                if next_link.is_some() {
                    return Err("Link extension attached to a non-link entry.".into());
                }
                if kind.is_dir() {
                    Kind::Directory
                } else {
                    Kind::File
                }
            };
            if manifest.insert(relative.clone(), record.clone()).is_some() {
                return Err("Duplicate archive path.".into());
            }
            let mode = entry.header().mode().map_err(|e| e.to_string())?;
            visit(&relative, &record, mode, &mut entry, size)?;
        }
    }
    if next_path.is_some() || next_link.is_some() {
        return Err("Incomplete archive name extension.".into());
    }
    // Consume the XZ footer even after TAR's end marker: verify checksum/truncation
    // and reject an oversized trailing decoded stream or extra compressed payload.
    io::copy(&mut reader, &mut io::sink()).map_err(|e| e.to_string())?;
    if reader.inner.total_in() != size {
        return Err("Unexpected trailing archive data.".into());
    }
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn link_target(link: &Path, target: &str) -> Result<PathBuf, String> {
    if target.is_empty()
        || target.len() > 1024
        || target.contains('\\')
        || target.split('/').count() > 32
    {
        return Err("Unsafe archive link target.".into());
    }
    let in_alpine = link.starts_with("alpine");
    let absolute = target.starts_with('/');
    if absolute && !in_alpine {
        return Err("Absolute links are allowed only inside the bundled Alpine root.".into());
    }
    let mut parts: Vec<String> = if absolute {
        vec!["alpine".into()]
    } else {
        link.parent()
            .ok_or("Missing link parent.")?
            .iter()
            .map(|p| p.to_string_lossy().into())
            .collect()
    };
    for part in target.strip_prefix('/').unwrap_or(target).split('/') {
        match part {
            "." => {}
            ".." => {
                if parts.len() <= usize::from(in_alpine) {
                    return Err("Archive link escapes its root.".into());
                }
                parts.pop();
            }
            _ => {
                archive_path(part)?;
                parts.push(part.to_owned());
            }
        }
    }
    archive_path(&parts.join("/"))
}

fn relative_target(link: &Path, target: &Path) -> PathBuf {
    let from: Vec<_> = link.parent().unwrap().iter().collect();
    let to: Vec<_> = target.iter().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut relative = PathBuf::new();
    for _ in common..from.len() {
        relative.push("..");
    }
    for component in &to[common..] {
        relative.push(component);
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    relative
}

fn validate_manifest(manifest: &BTreeMap<PathBuf, Kind>) -> Result<(), String> {
    for (path, kind) in manifest {
        for parent in path.ancestors().skip(1) {
            if matches!(manifest.get(parent), Some(Kind::File | Kind::Link(_))) {
                return Err("Archive entry traverses a file or link.".into());
            }
        }
        if let Kind::Link(target) = kind {
            resolve_target(target, manifest)?;
        }
    }
    Ok(())
}

fn resolve_target(target: &Path, manifest: &BTreeMap<PathBuf, Kind>) -> Result<PathBuf, String> {
    let mut target = target.to_path_buf();
    let mut seen = HashSet::new();
    for _ in 0..40 {
        if !seen.insert(target.clone()) {
            return Err("Cyclic archive links are not allowed.".into());
        }
        let mut replacement = None;
        for ancestor in target.ancestors().collect::<Vec<_>>().into_iter().rev() {
            match manifest.get(ancestor) {
                Some(Kind::Link(next)) => {
                    replacement = Some(next.join(target.strip_prefix(ancestor).unwrap()));
                    break;
                }
                Some(Kind::File) if ancestor != target => {
                    return Err("Archive link traverses a file.".into())
                }
                _ => {}
            }
        }
        match replacement {
            Some(next) => target = next,
            None => return Ok(target),
        }
    }
    Err("Archive link chain is too deep.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::Write,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "fxi-tar-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn archive(&self, entries: &[(&str, u8, &str, &str)]) -> PathBuf {
            let path = self.0.join("fixture.tar.xz");
            let encoder = xz2::write::XzEncoder::new(File::create(&path).unwrap(), 1);
            let mut builder = tar::Builder::new(encoder);
            for (name, kind, content, target) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::new(*kind));
                header.set_mode(0o6755);
                header.set_size(content.len() as u64);
                // Raw header bytes deliberately allow malformed paths for security fixtures.
                header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
                header.as_mut_bytes()[157..157 + target.len()].copy_from_slice(target.as_bytes());
                header.set_cksum();
                builder.append(&header, content.as_bytes()).unwrap();
            }
            builder.into_inner().unwrap().finish().unwrap();
            path
        }
        fn extract(&self, path: &Path, limits: Limits) -> Result<Vec<PathBuf>, String> {
            extract_with_limits(
                File::open(path).unwrap(),
                &self.0.join("stage"),
                &self.0.join("installation"),
                &mut DirectoryPins::default(),
                limits,
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn normalizes_only_confined_alpine_links() {
        for (link, target, expected) in [
            ("alpine/bin/sh", "/bin/busybox", "alpine/bin/busybox"),
            ("alpine/lib/libc.so", "ld-musl.so", "alpine/lib/ld-musl.so"),
            ("alpine/var/run", "../run", "alpine/run"),
            ("alpine/etc/mtab", "/proc/mounts", "alpine/proc/mounts"),
            (
                "alpine/etc/cert.pem",
                "../usr/cert.pem",
                "alpine/usr/cert.pem",
            ),
        ] {
            assert_eq!(
                link_target(Path::new(link), target).unwrap(),
                PathBuf::from(expected)
            );
        }
        for (link, target) in [
            ("outside", "/etc/passwd"),
            ("alpine/lib/link", "../../escape"),
            ("alpine/lib/link", "/../escape"),
            ("link", "../escape"),
            ("alpine/link", "C:/evil"),
            ("alpine/link", "\\evil"),
            ("alpine/link", "/txData/config.json"),
            ("link", ".fxserver-artifact-version"),
        ] {
            assert!(
                link_target(Path::new(link), target).is_err(),
                "{link} -> {target}"
            );
        }
        assert_eq!(
            relative_target(Path::new("alpine/bin/sh"), Path::new("alpine/bin/busybox")),
            PathBuf::from("busybox")
        );
        assert_eq!(
            relative_target(
                Path::new("alpine/etc/ssl/cert.pem"),
                Path::new("alpine/usr/cert.pem")
            ),
            PathBuf::from("../../usr/cert.pem")
        );
    }

    #[test]
    fn rejects_cycles_and_writes_beneath_links_regardless_of_order() {
        let link = |to: &str| Kind::Link(PathBuf::from(to));
        for entries in [
            vec![
                ("alpine/a", link("alpine/b")),
                ("alpine/b", link("alpine/a")),
            ],
            vec![("alpine/a", link("alpine/a"))],
            vec![
                ("alpine/a", link("alpine/b")),
                ("alpine/a/file", Kind::File),
            ],
            vec![
                ("alpine/a", Kind::File),
                ("alpine/link", link("alpine/a/child")),
            ],
        ] {
            let manifest = entries
                .into_iter()
                .map(|(p, k)| (PathBuf::from(p), k))
                .collect();
            assert!(validate_manifest(&manifest).is_err());
        }
        let manifest = BTreeMap::from([
            (PathBuf::from("alpine/lib/one"), link("alpine/lib/two")),
            (PathBuf::from("alpine/lib/two"), link("alpine/lib/three")),
            (PathBuf::from("alpine/lib/three"), Kind::File),
            (PathBuf::from("alpine/etc/mtab"), link("alpine/proc/mounts")),
        ]);
        validate_manifest(&manifest).unwrap();
    }

    #[test]
    fn tar_rejects_traversal_protected_paths_and_special_entries() {
        for (path, kind, content, link) in [
            ("../escape", b'0', "bad", ""),
            ("./../escape", b'0', "bad", ""),
            ("/escape", b'0', "bad", ""),
            ("alpine/txData/config.json", b'0', "bad", ""),
            (".fxi-install.lock", b'0', "bad", ""),
            (".fxserver-artifact-version", b'0', "bad", ""),
            ("alpine/link", b'2', "", "../../escape"),
            ("alpine/hard", b'1', "", "/etc/passwd"),
            ("alpine/device", b'3', "", ""),
            ("alpine/fifo", b'6', "", ""),
            ("metadata", b'x', "size=10000000000", ""),
        ] {
            let fixture = Fixture::new();
            let archive = fixture.archive(&[(path, kind, content, link)]);
            assert!(
                fixture.extract(&archive, Limits::default()).is_err(),
                "{path}"
            );
            assert!(!fixture.0.join("escape").exists());
            assert!(!fixture.0.join("installation").exists());
        }
    }

    #[test]
    fn tar_enforces_file_total_entry_stream_and_time_limits() {
        for limits in [
            Limits {
                file: 3,
                ..Limits::default()
            },
            Limits {
                expanded: 7,
                ..Limits::default()
            },
            Limits {
                entries: 1,
                ..Limits::default()
            },
            Limits {
                stream: 512,
                ..Limits::default()
            },
            Limits {
                timeout: Duration::ZERO,
                ..Limits::default()
            },
        ] {
            let fixture = Fixture::new();
            let archive = fixture.archive(&[("one", b'0', "1234", ""), ("two", b'0', "5678", "")]);
            assert!(fixture.extract(&archive, limits).is_err());
        }
    }

    #[test]
    fn tar_rejects_duplicates_and_link_parent_conflicts() {
        for entries in [
            vec![("same", b'0', "one", ""), ("./same", b'0', "two", "")],
            vec![
                ("alpine/link", b'2', "", "target"),
                ("alpine/link/file", b'0', "bad", ""),
            ],
            vec![
                ("alpine/link/file", b'0', "bad", ""),
                ("alpine/link", b'2', "", "target"),
            ],
        ] {
            let fixture = Fixture::new();
            let archive = fixture.archive(&entries);
            assert!(fixture.extract(&archive, Limits::default()).is_err());
        }
    }

    #[test]
    fn reads_gnu_long_names_and_validates_xz_footer() {
        let fixture = Fixture::new();
        let name = format!("alpine/{}.txt", "x".repeat(110));
        let archive = fixture.archive(&[
            ("././@LongLink", b'L', &format!("{name}\0"), ""),
            ("short", b'0', "content", ""),
        ]);
        let files = fixture.extract(&archive, Limits::default()).unwrap();
        assert_eq!(files, [PathBuf::from(&name)]);
        assert_eq!(
            fs::read(fixture.0.join("stage").join(name)).unwrap(),
            b"content"
        );

        let fixture = Fixture::new();
        let archive = fixture.archive(&[("file", b'0', "content", "")]);
        let size = fs::metadata(&archive).unwrap().len();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&archive)
            .unwrap()
            .set_len(size - 8)
            .unwrap();
        assert!(fixture.extract(&archive, Limits::default()).is_err());
    }

    #[test]
    fn rejects_oversized_gnu_metadata_and_trailing_payload() {
        let fixture = Fixture::new();
        let archive = fixture.archive(&[("././@LongLink", b'L', &"x".repeat(4097), "")]);
        assert!(fixture.extract(&archive, Limits::default()).is_err());
        let fixture = Fixture::new();
        let archive = fixture.archive(&[("file", b'0', "content", "")]);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&archive)
            .unwrap()
            .write_all(b"trailing garbage")
            .unwrap();
        assert!(fixture.extract(&archive, Limits::default()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn creates_relative_links_after_files_with_safe_executable_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::new();
        let archive = fixture.archive(&[
            ("./", b'5', "", ""),
            ("alpine/bin/sh", b'2', "", "/bin/busybox"),
            ("alpine/bin/busybox", b'0', "executable", ""),
            ("alpine/etc/mtab", b'2', "", "../proc/mounts"),
            ("alpine/lib/libc.so", b'2', "", "../bin/sh"),
        ]);
        let files = fixture.extract(&archive, Limits::default()).unwrap();
        assert_eq!(files.len(), 4);
        let stage = fixture.0.join("stage");
        assert_eq!(
            fs::read_link(stage.join("alpine/bin/sh")).unwrap(),
            PathBuf::from("busybox")
        );
        assert_eq!(
            fs::read(stage.join("alpine/lib/libc.so")).unwrap(),
            b"executable"
        );
        assert_eq!(
            fs::metadata(stage.join("alpine/bin/busybox"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o755
        );
        assert!(fs::symlink_metadata(stage.join("alpine/etc/mtab"))
            .unwrap()
            .is_symlink());
    }

    #[test]
    #[ignore = "read-only audit of a locally downloaded official archive; set FXI_AUDIT_ARCHIVE"]
    fn audit_official_archive_without_extraction() {
        let path = std::env::var("FXI_AUDIT_ARCHIVE").unwrap();
        let file = File::open(path).unwrap();
        let manifest = read_archive(file, Limits::default(), |_, _, _, reader, size| {
            let actual = io::copy(reader, &mut io::sink()).map_err(|e| e.to_string())?;
            if actual != size {
                return Err("Archive entry length mismatch.".into());
            }
            Ok(())
        })
        .unwrap();
        let links = manifest
            .values()
            .filter(|kind| matches!(kind, Kind::Link(_)))
            .count();
        println!(
            "Read-only official archive: {} validated paths, {links} confined links.",
            manifest.len()
        );
    }
}
