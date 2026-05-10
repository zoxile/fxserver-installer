use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use tauri::{AppHandle, Manager};

use crate::models::jooat::{JooatResolvedHash, JooatResolverManifest, JooatResolverStatus};

const DATABASE_FOLDER: &str = "jooat-resolver";
const MANIFEST_FILE: &str = "manifest.json";
const SHARDS_FOLDER: &str = "shards";

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

pub fn prepare_database(app: &AppHandle, manifest: JooatResolverManifest) -> Result<JooatResolverStatus, String> {
    validate_manifest(&manifest)?;

    let database_dir = database_dir(app)?;
    fs::create_dir_all(shards_dir(&database_dir)).map_err(|error| format!("Failed to create JOOAT shard directory: {error}"))?;

    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|error| format!("Failed to encode JOOAT manifest: {error}"))?;
    fs::write(manifest_path(&database_dir), manifest_json).map_err(|error| format!("Failed to write JOOAT manifest: {error}"))?;

    Ok(build_status(database_dir))
}

pub fn save_shard(app: &AppHandle, prefix: String, content: String) -> Result<JooatResolverStatus, String> {
    let normalized_prefix = normalize_prefix(&prefix)?;
    let database_dir = database_dir(app)?;
    fs::create_dir_all(shards_dir(&database_dir)).map_err(|error| format!("Failed to create JOOAT shard directory: {error}"))?;

    fs::write(shard_path(&database_dir, &normalized_prefix), content).map_err(|error| format!("Failed to write JOOAT shard {normalized_prefix}: {error}"))?;

    Ok(build_status(database_dir))
}

pub fn remove_database(app: &AppHandle) -> Result<JooatResolverStatus, String> {
    let database_dir = database_dir(app)?;

    if database_dir.exists() {
        fs::remove_dir_all(&database_dir).map_err(|error| format!("Failed to remove JOOAT resolver database: {error}"))?;
    }

    Ok(build_status(database_dir))
}

pub fn resolve_hashes(app: &AppHandle, queries: Vec<String>) -> Result<Vec<JooatResolvedHash>, String> {
    let database_dir = database_dir(app)?;
    let status = build_status(database_dir.clone());
    if !status.available {
        return Err("Install the optional JOOAT resolver database before using offline resolver lookups.".to_string());
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
                hashes_by_prefix.entry(hash_prefix(value)).or_default().insert(value);
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
    let expected_shards = manifest.as_ref().map(|manifest| manifest.shards.len()).unwrap_or(0);
    let installed_shards = manifest
        .as_ref()
        .map(|manifest| installed_manifest_shards(&database_dir, manifest))
        .unwrap_or_else(|| installed_shards(&database_dir));
    let size_bytes = directory_size(&database_dir);
    let available = manifest.is_some() && expected_shards > 0 && installed_shards >= expected_shards;
    let message = if available {
        "Offline resolver database is installed.".to_string()
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

fn database_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data = app.path().app_data_dir().map_err(|error| format!("Failed to resolve app data directory: {error}"))?;
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
    let content = fs::read_to_string(manifest_path(database_dir)).map_err(|error| format!("Failed to read JOOAT manifest: {error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("Failed to parse JOOAT manifest: {error}"))
}

fn validate_manifest(manifest: &JooatResolverManifest) -> Result<(), String> {
    if manifest.version.trim().is_empty() {
        return Err("JOOAT manifest version is required.".to_string());
    }

    if manifest.shards.is_empty() {
        return Err("JOOAT manifest must include at least one shard.".to_string());
    }

    for shard in &manifest.shards {
        normalize_prefix(&shard.prefix)?;
        let shard_path = Path::new(&shard.path);
        if shard.path.contains("..") || shard_path.is_absolute() || shard.path.contains(':') {
            return Err(format!("Invalid JOOAT shard path for prefix {}.", shard.prefix));
        }
    }

    Ok(())
}

fn installed_shards(database_dir: &Path) -> usize {
    fs::read_dir(shards_dir(database_dir))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "tsv"))
                .count()
        })
        .unwrap_or(0)
}

fn installed_manifest_shards(database_dir: &Path, manifest: &JooatResolverManifest) -> usize {
    manifest
        .shards
        .iter()
        .filter(|shard| database_dir.join(&shard.path).exists())
        .count()
}

fn directory_size(path: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];

    while let Some(current) = stack.pop() {
        let Ok(metadata) = fs::metadata(&current) else {
            continue;
        };

        if metadata.is_file() {
            total += metadata.len();
            continue;
        }

        if let Ok(entries) = fs::read_dir(current) {
            for entry in entries.flatten() {
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
        .filter_map(|shard| normalize_prefix(&shard.prefix).ok().map(|prefix| (prefix, shard.path.clone())))
        .collect()
}

fn load_matches(
    database_dir: &Path,
    manifest_lookup: Option<&HashMap<String, String>>,
    hashes_by_prefix: HashMap<String, HashSet<u32>>,
) -> Result<HashMap<u32, Vec<String>>, String> {
    let mut matches_by_hash: HashMap<u32, Vec<String>> = HashMap::new();

    for (prefix, targets) in hashes_by_prefix {
        let path = manifest_lookup
            .and_then(|lookup| lookup.get(&prefix))
            .map(|relative_path| database_dir.join(relative_path))
            .unwrap_or_else(|| shard_path(database_dir, &prefix));

        if !path.exists() {
            continue;
        }

        let content = fs::read_to_string(&path).map_err(|error| format!("Failed to read JOOAT shard {prefix}: {error}"))?;
        for line in content.lines() {
            if let Some((hash, name)) = parse_shard_line(line) {
                if targets.contains(&hash) {
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

    let mut parts = trimmed.splitn(2, |character: char| character == '\t' || character == ',' || character.is_whitespace());
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

    if let Some(hex) = lowercase.strip_prefix("0x").or_else(|| lowercase.strip_prefix("hash_")) {
        return u32::from_str_radix(hex, 16).map_err(|_| "Invalid hex JOOAT hash.".to_string());
    }

    if lowercase.len() == 8 && lowercase.chars().all(|character| character.is_ascii_hexdigit()) {
        return u32::from_str_radix(&lowercase, 16).map_err(|_| "Invalid hex JOOAT hash.".to_string());
    }

    let value = cleaned.parse::<i64>().map_err(|_| "Enter a hex, unsigned, or signed 32-bit hash.".to_string())?;
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

    if normalized.len() == 2 && normalized.chars().all(|character| character.is_ascii_hexdigit()) {
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
