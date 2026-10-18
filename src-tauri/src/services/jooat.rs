use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[cfg(feature = "desktop")]
use tauri::{AppHandle, Manager};

use crate::models::jooat::{JooatResolvedHash, JooatResolverManifest, JooatResolverStatus};

const DATABASE_FOLDER: &str = "jooat-resolver";
const MANIFEST_FILE: &str = "manifest.json";
const SHARDS_FOLDER: &str = "shards";
const COMPLETE_SHARD_COUNT: usize = 256;
const MAX_MANIFEST_BYTES: u64 = 512 * 1024;
const MAX_SHARD_BYTES: u64 = 16 * 1024 * 1024;
const MAX_LOOKUP_BYTES: u64 = 64 * 1024 * 1024;

#[cfg(feature = "desktop")]
pub fn get_status(app: &AppHandle) -> JooatResolverStatus {
    match database_dir(app) {
        Ok(database_dir) => build_status(database_dir),
        Err(error) => JooatResolverStatus {
            available: false,
            database_dir: String::new(),
            manifest: None,
            installed_shards: 0,
            expected_shards: 0,
            size_bytes: 0,
            message: error,
        },
    }
}

#[cfg(feature = "desktop")]
pub fn prepare_database(
    app: &AppHandle,
    manifest: JooatResolverManifest,
) -> Result<JooatResolverStatus, String> {
    validate_manifest(&manifest)?;

    let database_dir = database_dir(app)?;
    fs::create_dir_all(shards_dir(&database_dir))
        .map_err(|error| format!("Failed to create JOOAT shard directory: {error}"))?;

    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("Failed to encode JOOAT manifest: {error}"))?;
    fs::write(manifest_path(&database_dir), manifest_json)
        .map_err(|error| format!("Failed to write JOOAT manifest: {error}"))?;

    Ok(build_status(database_dir))
}

#[cfg(feature = "desktop")]
pub fn save_shard(
    app: &AppHandle,
    prefix: String,
    content: String,
) -> Result<JooatResolverStatus, String> {
    let normalized_prefix = normalize_prefix(&prefix)?;
    let database_dir = database_dir(app)?;
    fs::create_dir_all(shards_dir(&database_dir))
        .map_err(|error| format!("Failed to create JOOAT shard directory: {error}"))?;

    fs::write(shard_path(&database_dir, &normalized_prefix), content)
        .map_err(|error| format!("Failed to write JOOAT shard {normalized_prefix}: {error}"))?;

    Ok(build_status(database_dir))
}

#[cfg(feature = "desktop")]
pub fn remove_database(app: &AppHandle) -> Result<JooatResolverStatus, String> {
    let database_dir = database_dir(app)?;

    if database_dir.exists() {
        fs::remove_dir_all(&database_dir)
            .map_err(|error| format!("Failed to remove JOOAT resolver database: {error}"))?;
    }

    Ok(build_status(database_dir))
}

#[cfg(feature = "desktop")]
pub fn resolve_hashes(
    app: &AppHandle,
    queries: Vec<String>,
) -> Result<Vec<JooatResolvedHash>, String> {
    resolve_hashes_in(database_dir(app)?, queries)
}

#[cfg(any(test, feature = "terminal"))]
pub(crate) fn get_status_in(database_dir: PathBuf) -> Result<JooatResolverStatus, String> {
    crate::commands::config_history::ensure_unlinked_path(&database_dir)?;
    Ok(build_status(database_dir))
}

#[cfg(feature = "terminal")]
pub(crate) fn core_database_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("The Windows roaming application data directory is unavailable.")?
        .join("com.fxserver.installer");
    #[cfg(not(windows))]
    let root = crate::platform::data_dir()?;
    Ok(root.join(DATABASE_FOLDER))
}

pub(crate) fn resolve_hashes_in(
    database_dir: PathBuf,
    queries: Vec<String>,
) -> Result<Vec<JooatResolvedHash>, String> {
    if queries.len() > 100 || queries.iter().any(|query| query.len() > 128) {
        return Err("Resolve at most 100 hashes of at most 128 bytes each.".into());
    }
    crate::commands::config_history::ensure_unlinked_path(&database_dir)?;
    let status = build_status(database_dir.clone());
    if !status.available {
        return Err(
            "Install the optional JOOAT resolver database before using offline resolver lookups."
                .to_string(),
        );
    }

    let manifest_lookup = status.manifest.as_ref().map(shard_lookup);
    let mut parsed_queries = Vec::new();
    let mut hashes_by_prefix: HashMap<String, HashSet<u32>> = HashMap::new();

    for query in queries {
        let trimmed = query.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        match parse_hash(&trimmed) {
            Ok(value) => {
                hashes_by_prefix
                    .entry(hash_prefix(value))
                    .or_default()
                    .insert(value);
                parsed_queries.push((trimmed, Some(value), None));
            }
            Err(error) => parsed_queries.push((trimmed, None, Some(error))),
        }
    }

    let matches_by_hash = load_matches(&database_dir, manifest_lookup.as_ref(), hashes_by_prefix)?;

    Ok(parsed_queries
        .into_iter()
        .map(|(query, value, error)| match value {
            Some(value) => JooatResolvedHash {
                query,
                value: Some(value),
                hex: Some(format_hash(value)),
                unsigned: Some(value.to_string()),
                signed: Some(signed_hash(value).to_string()),
                matches: matches_by_hash.get(&value).cloned().unwrap_or_default(),
                error: None,
            },
            None => JooatResolvedHash {
                query,
                value: None,
                hex: None,
                unsigned: None,
                signed: None,
                matches: Vec::new(),
                error,
            },
        })
        .collect())
}

fn build_status(database_dir: PathBuf) -> JooatResolverStatus {
    let manifest = read_manifest(&database_dir).ok();
    let expected_shards = manifest
        .as_ref()
        .map(|manifest| manifest.shards.len())
        .unwrap_or(0);
    let installed_shards = manifest
        .as_ref()
        .map(|manifest| installed_manifest_shards(&database_dir, manifest))
        .unwrap_or_else(|| installed_shards(&database_dir));
    let size_bytes = directory_size(&database_dir);
    let complete_manifest = manifest.as_ref().is_some_and(manifest_is_complete);
    let available = complete_manifest && installed_shards >= expected_shards;
    let message = if available {
        "Complete offline resolver database is installed.".to_string()
    } else if manifest
        .as_ref()
        .is_some_and(|manifest| !manifest_is_complete(manifest))
    {
        format!("Resolver manifest is incomplete. Expected {COMPLETE_SHARD_COUNT} prefix shards from 00 through ff.")
    } else if manifest.is_some() {
        format!("Resolver manifest is installed, but only {installed_shards} of {expected_shards} shards are present.")
    } else {
        "Resolver database is not installed. The hasher still works without it.".to_string()
    };

    JooatResolverStatus {
        available,
        database_dir: database_dir.to_string_lossy().into_owned(),
        manifest,
        installed_shards,
        expected_shards,
        size_bytes,
        message,
    }
}

#[cfg(feature = "desktop")]
fn database_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve app data directory: {error}"))?;
    Ok(app_data.join(DATABASE_FOLDER))
}

fn manifest_path(database_dir: &Path) -> PathBuf {
    database_dir.join(MANIFEST_FILE)
}

fn shards_dir(database_dir: &Path) -> PathBuf {
    database_dir.join(SHARDS_FOLDER)
}

fn shard_path(database_dir: &Path, prefix: &str) -> PathBuf {
    shards_dir(database_dir).join(format!("{prefix}.tsv"))
}

fn read_manifest(database_dir: &Path) -> Result<JooatResolverManifest, String> {
    let content = read_resolver_file(&manifest_path(database_dir), MAX_MANIFEST_BYTES)?;
    let manifest = serde_json::from_str(&content)
        .map_err(|_| "The JOOAT resolver manifest is invalid.".to_string())?;
    validate_manifest_paths(&manifest)?;
    Ok(manifest)
}

fn read_resolver_file(path: &Path, limit: u64) -> Result<String, String> {
    crate::commands::config_history::ensure_unlinked_path(path)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| "Cannot open the JOOAT resolver file.")?;
    if !file
        .metadata()
        .map_err(|_| "Cannot inspect the JOOAT resolver file.")?
        .is_file()
    {
        return Err("JOOAT resolver data requires regular files.".into());
    }
    let mut content = String::new();
    file.take(limit + 1)
        .read_to_string(&mut content)
        .map_err(|_| "Cannot read the JOOAT resolver file as UTF-8.")?;
    if content.len() as u64 > limit {
        return Err("The JOOAT resolver file exceeds the read limit.".into());
    }
    Ok(content)
}

#[cfg(feature = "desktop")]
fn validate_manifest(manifest: &JooatResolverManifest) -> Result<(), String> {
    if manifest.version.trim().is_empty() {
        return Err("JOOAT manifest version is required.".to_string());
    }

    if !manifest_is_complete(manifest) {
        return Err(format!(
            "JOOAT manifest must include the complete {COMPLETE_SHARD_COUNT}-shard database from 00 through ff."
        ));
    }

    validate_manifest_paths(manifest)
}

fn validate_manifest_paths(manifest: &JooatResolverManifest) -> Result<(), String> {
    if manifest.shards.len() > COMPLETE_SHARD_COUNT {
        return Err("JOOAT manifests are limited to 256 prefix shards.".into());
    }
    for shard in &manifest.shards {
        normalize_prefix(&shard.prefix)?;
        let shard_path = Path::new(&shard.path);
        if shard.path.contains("..") || shard_path.is_absolute() || shard.path.contains(':') {
            return Err(format!(
                "Invalid JOOAT shard path for prefix {}.",
                shard.prefix
            ));
        }
    }

    Ok(())
}

fn manifest_is_complete(manifest: &JooatResolverManifest) -> bool {
    if manifest.shards.len() != COMPLETE_SHARD_COUNT {
        return false;
    }

    let prefixes = manifest
        .shards
        .iter()
        .filter_map(|shard| normalize_prefix(&shard.prefix).ok())
        .collect::<HashSet<_>>();

    (0..=u8::MAX).all(|value| prefixes.contains(&format!("{value:02x}")))
}

fn installed_shards(database_dir: &Path) -> usize {
    if crate::commands::config_history::ensure_unlinked_path(&shards_dir(database_dir)).is_err() {
        return 0;
    }
    fs::read_dir(shards_dir(database_dir))
        .map(|entries| {
            entries
                .take(4096)
                .flatten()
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "tsv")
                })
                .count()
        })
        .unwrap_or(0)
}

fn installed_manifest_shards(database_dir: &Path, manifest: &JooatResolverManifest) -> usize {
    manifest
        .shards
        .iter()
        .filter(|shard| {
            let path = database_dir.join(&shard.path);
            crate::commands::config_history::ensure_unlinked_path(&path).is_ok() && path.is_file()
        })
        .count()
}

fn directory_size(path: &Path) -> u64 {
    let mut total: u64 = 0;
    let mut remaining = 4096;
    let mut stack = vec![path.to_path_buf()];

    while let Some(current) = stack.pop() {
        let Ok(metadata) = fs::symlink_metadata(&current) else {
            continue;
        };

        if crate::commands::config_history::ensure_unlinked_path(&current).is_err() {
            continue;
        }

        if metadata.is_file() {
            total = total.saturating_add(metadata.len());
            continue;
        }

        if let Ok(entries) = fs::read_dir(current) {
            for entry in entries.take(remaining).flatten() {
                remaining -= 1;
                stack.push(entry.path());
            }
        }
    }

    total
}

fn shard_lookup(manifest: &JooatResolverManifest) -> HashMap<String, String> {
    manifest
        .shards
        .iter()
        .filter_map(|shard| {
            normalize_prefix(&shard.prefix)
                .ok()
                .map(|prefix| (prefix, shard.path.clone()))
        })
        .collect()
}

fn load_matches(
    database_dir: &Path,
    manifest_lookup: Option<&HashMap<String, String>>,
    hashes_by_prefix: HashMap<String, HashSet<u32>>,
) -> Result<HashMap<u32, Vec<String>>, String> {
    let mut matches_by_hash: HashMap<u32, Vec<String>> = HashMap::new();
    let mut total_bytes = 0;
    let mut total_matches = 0;

    for (prefix, targets) in hashes_by_prefix {
        let path = manifest_lookup
            .and_then(|lookup| lookup.get(&prefix))
            .map(|relative_path| database_dir.join(relative_path))
            .unwrap_or_else(|| shard_path(database_dir, &prefix));

        if !path.exists() {
            continue;
        }

        let content =
            read_resolver_file(&path, MAX_SHARD_BYTES.min(MAX_LOOKUP_BYTES - total_bytes))?;
        total_bytes += content.len() as u64;
        for line in content.lines() {
            if let Some((hash, name)) = parse_shard_line(line) {
                if targets.contains(&hash) {
                    total_matches += 1;
                    if total_matches > 1000 || name.len() > 4096 {
                        return Err("JOOAT resolver matches exceed the output limit.".into());
                    }
                    matches_by_hash.entry(hash).or_default().push(name);
                }
            }
        }
    }

    for matches in matches_by_hash.values_mut() {
        matches.sort();
        matches.dedup();
    }

    Ok(matches_by_hash)
}

fn parse_shard_line(line: &str) -> Option<(u32, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let mut parts = trimmed.splitn(2, |character: char| {
        character == '\t' || character == ',' || character.is_whitespace()
    });
    let hash = parse_hash(parts.next()?.trim()).ok()?;
    let name = parts.next()?.trim();
    if name.is_empty() {
        return None;
    }

    Some((hash, name.to_string()))
}

fn parse_hash(input: &str) -> Result<u32, String> {
    let cleaned = input.trim();
    let lowercase = cleaned.to_lowercase();

    if let Some(hex) = lowercase
        .strip_prefix("0x")
        .or_else(|| lowercase.strip_prefix("hash_"))
    {
        return u32::from_str_radix(hex, 16).map_err(|_| "Invalid hex JOOAT hash.".to_string());
    }

    if lowercase.len() == 8
        && lowercase
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return u32::from_str_radix(&lowercase, 16)
            .map_err(|_| "Invalid hex JOOAT hash.".to_string());
    }

    let value = cleaned
        .parse::<i64>()
        .map_err(|_| "Enter a hex, unsigned, or signed 32-bit hash.".to_string())?;
    if value < i32::MIN as i64 || value > u32::MAX as i64 {
        return Err("JOOAT hash must fit in a signed or unsigned 32-bit value.".to_string());
    }

    if value < 0 {
        Ok((value as i32) as u32)
    } else {
        Ok(value as u32)
    }
}

fn normalize_prefix(prefix: &str) -> Result<String, String> {
    let normalized = prefix.trim().to_lowercase();

    if normalized.len() == 2
        && normalized
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        Ok(normalized)
    } else {
        Err(format!("Invalid JOOAT shard prefix: {prefix}"))
    }
}

fn hash_prefix(value: u32) -> String {
    format!("{value:08x}")[..2].to_string()
}

fn format_hash(value: u32) -> String {
    format!("0x{value:08X}")
}

fn signed_hash(value: u32) -> i32 {
    value as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::jooat::JooatShardManifest;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "fxi-jooat-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn install(&self) -> JooatResolverManifest {
            fs::create_dir(shards_dir(&self.0)).unwrap();
            let manifest = JooatResolverManifest {
                version: "fixture".into(),
                source: None,
                generated_at: None,
                total_hashes: None,
                total_names: None,
                size_bytes: None,
                shards: (0..=u8::MAX)
                    .map(|prefix| {
                        let prefix = format!("{prefix:02x}");
                        let path = format!("shards/{prefix}.tsv");
                        fs::write(self.0.join(&path), "").unwrap();
                        JooatShardManifest {
                            prefix,
                            path,
                            hashes: None,
                            bytes: None,
                        }
                    })
                    .collect(),
            };
            fs::write(
                manifest_path(&self.0),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            manifest
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_database_status_is_read_only() {
        let fixture = Fixture::new();
        let path = fixture.0.join("not-installed");
        assert!(!get_status_in(path.clone()).unwrap().available);
        assert!(resolve_hashes_in(path.clone(), vec!["1".into()]).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn core_resolver_reuses_signed_unsigned_hex_parsing_and_deduplicates_matches() {
        let fixture = Fixture::new();
        fixture.install();
        fs::write(
            shard_path(&fixture.0, "ff"),
            "0xFFFFFFFF\tfixture_name\n-1 fixture_name\n",
        )
        .unwrap();
        let result = resolve_hashes_in(
            fixture.0.clone(),
            vec![
                "-1".into(),
                "4294967295".into(),
                "hash_ffffffff".into(),
                "invalid".into(),
            ],
        )
        .unwrap();
        for item in &result[..3] {
            assert_eq!(item.value, Some(u32::MAX));
            assert_eq!(item.signed.as_deref(), Some("-1"));
            assert_eq!(item.matches, ["fixture_name"]);
        }
        assert!(result[3].error.is_some());
        assert!(resolve_hashes_in(fixture.0.clone(), vec!["1".into(); 101]).is_err());
    }

    #[test]
    fn resolver_rejects_unsafe_manifests_and_bounded_file_reads() {
        let fixture = Fixture::new();
        let mut manifest = fixture.install();
        manifest.shards[0].path = "../outside.tsv".into();
        fs::write(
            manifest_path(&fixture.0),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(read_manifest(&fixture.0).is_err());
        assert!(!get_status_in(fixture.0.clone()).unwrap().available);
        let path = fixture.0.join("bounded.tsv");
        fs::write(&path, "12345").unwrap();
        assert_eq!(read_resolver_file(&path, 5).unwrap(), "12345");
        assert!(read_resolver_file(&path, 4).is_err());
        assert!(read_resolver_file(&fixture.0, 10).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resolver_rejects_linked_files_and_does_not_follow_directory_cycles() {
        let fixture = Fixture::new();
        let path = fixture.0.join("data.tsv");
        fs::write(&path, "fixture").unwrap();
        let link = fixture.0.join("linked.tsv");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(read_resolver_file(&link, 100).is_err());
        std::os::unix::fs::symlink(&fixture.0, fixture.0.join("cycle")).unwrap();
        assert_eq!(directory_size(&fixture.0), 7);
    }
}
