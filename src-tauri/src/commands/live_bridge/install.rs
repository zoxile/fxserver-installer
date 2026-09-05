use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use super::{BridgeTarget, RESOURCE_NAME};
use crate::commands::{
    config_history::{self, ConfigChangeReason, ConfigFileRequest},
    fxserver,
};

const MANIFEST: &str = include_str!("../../../resources/live-bridge/fxmanifest.lua");
const SCRIPT: &str = include_str!("../../../resources/live-bridge/server.js");
const OWNER: &str = "fxserver-installer-live-bridge";
const VERSION: &str = "1.0.0";
const MARKER: &str = ".fxsi-bridge.json";
const BEGIN: &str = "# BEGIN FXSERVER INSTALLER LIVE BRIDGE";
const END: &str = "# END FXSERVER INSTALLER LIVE BRIDGE";
const CONFIG_LINES: [&str; 7] = [
    BEGIN,
    "add_ace resource.fxserver_installer_bridge command.start allow",
    "add_ace resource.fxserver_installer_bridge command.stop allow",
    "add_ace resource.fxserver_installer_bridge command.restart allow",
    "add_ace resource.fxserver_installer_bridge command.ensure allow",
    "ensure fxserver_installer_bridge",
    END,
];
static OPERATION: Mutex<()> = Mutex::new(());
static PREVIEWS: Mutex<BTreeMap<String, Preview>> = Mutex::new(BTreeMap::new());

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
    pub workspace_id: String,
    pub installed: bool,
    managed: bool,
    resource_path: String,
    version: Option<String>,
    cfg_enabled: bool,
    key_available: bool,
    warning: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePreview {
    id: String,
    remove: bool,
    resource_path: String,
    files: Vec<String>,
    config_lines: Vec<String>,
    expires_in_seconds: u32,
}

struct Preview {
    target: BridgeTarget,
    root: PathBuf,
    remove: bool,
    config_revision: String,
    inventory: BTreeMap<String, String>,
    created: Instant,
}

#[derive(Serialize, Deserialize)]
struct Marker {
    owner: String,
    version: String,
    key_id: String,
    files: BTreeMap<String, String>,
}

struct ResourceSwap {
    root: PathBuf,
    resource: PathBuf,
    stage: PathBuf,
    old: PathBuf,
    previous: BTreeMap<String, String>,
    staged: Option<BTreeMap<String, String>>,
    previous_moved: bool,
    promoted: bool,
}

impl ResourceSwap {
    fn promote(&mut self) -> Result<(), String> {
        if inventory(&self.resource)? != self.previous {
            return Err(
                "Bridge files changed before replacement. Review the changes again.".into(),
            );
        }
        if !self.previous.is_empty() {
            rename_new(&self.resource, &self.old)?;
            self.previous_moved = true;
            if inventory(&self.old)? != self.previous {
                return Err("Bridge files changed during replacement. Nothing was deleted.".into());
            }
        }
        if let Some(expected) = &self.staged {
            if inventory(&self.stage)? != *expected {
                return Err("Bridge staging files changed. Nothing was deleted.".into());
            }
            rename_new(&self.stage, &self.resource)?;
            self.promoted = true;
        }
        Ok(())
    }

    fn rollback(&self) -> Result<(), String> {
        if self.promoted {
            remove_verified_files(
                &self.resource,
                self.staged
                    .as_ref()
                    .ok_or("Bridge staging is unavailable.")?,
            )?;
        }
        if self.previous_moved {
            rename_new(&self.old, &self.resource)?;
        }
        if let Some(expected) = &self.staged {
            remove_owned_stage(&self.stage, &self.root, expected)?;
        }
        Ok(())
    }

    fn finish(&self) -> Result<(), String> {
        if self.previous_moved {
            remove_owned_stage(&self.old, &self.root, &self.previous)?;
        }
        Ok(())
    }
}

pub(super) fn with_operation<T>(action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let _operation = OPERATION
        .try_lock()
        .map_err(|_| "Another bridge file operation is in progress.")?;
    action()
}

fn root(target: &BridgeTarget) -> Result<PathBuf, String> {
    if target.workspace_id.is_empty()
        || target.workspace_id.len() > 64
        || target.port == 0
        || !target
            .workspace_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err("Invalid workspace or bridge port.".into());
    }
    if target.profile.is_empty()
        || target.profile.contains(['/', '\\', ':'])
        || matches!(target.profile.as_str(), "." | "..")
    {
        return Err("Select a valid txAdmin profile.".into());
    }
    let (_, _, _, data) =
        fxserver::resolve_profile_data_path(target.tx_data_path.clone(), target.profile.clone())?;
    no_links(&data)?;
    let data = data.canonicalize().map_err(io_error)?;
    no_links(&data.join("resources"))?;
    if !data.join("resources").is_dir() {
        return Err("Server resources folder was not found.".into());
    }
    no_links(&data.join("server.cfg"))?;
    Ok(data)
}

pub(super) fn inspect(app: &AppHandle, target: &BridgeTarget) -> Result<Installation, String> {
    let root = root(target)?;
    let path = root.join("resources").join(RESOURCE_NAME);
    let cfg = config_history::read_bounded_config(&root.join("server.cfg"))?;
    let mut result = Installation {
        workspace_id: target.workspace_id.clone(),
        installed: path.exists(),
        managed: false,
        resource_path: path.to_string_lossy().into(),
        version: None,
        cfg_enabled: cfg.replace("\r\n", "\n").contains(&CONFIG_LINES.join("\n")),
        key_available: false,
        warning: None,
    };
    if path.exists() {
        match owned_marker(&path) {
            Ok(marker) => {
                result.managed = true;
                result.version = Some(marker.version);
                result.key_available = key_path(app, &marker.key_id)?.is_file();
            }
            Err(error) => result.warning = Some(error),
        }
    }
    Ok(result)
}

pub(super) fn preview(
    app: &AppHandle,
    target: BridgeTarget,
    remove: bool,
) -> Result<ChangePreview, String> {
    let root = root(&target)?;
    let resource = root.join("resources").join(RESOURCE_NAME);
    let config = config_history::read_bounded_config(&root.join("server.cfg"))?;
    edit_config(&config, remove)?;
    if remove && !resource.exists() {
        return Err("The bridge is not installed.".into());
    }
    if resource.exists() {
        owned_marker(&resource)?;
    }
    let inventory = inventory(&resource)?;
    let _ = app.path().app_local_data_dir().map_err(io_error)?;
    let id = random_id()?;
    let mut previews = PREVIEWS
        .lock()
        .map_err(|_| "Bridge previews are unavailable.")?;
    previews.retain(|_, item| item.created.elapsed() < Duration::from_secs(600));
    if previews.len() >= 8 {
        return Err("Too many active bridge previews. Retry after a preview expires.".into());
    }
    previews.insert(
        id.clone(),
        Preview {
            target,
            root,
            remove,
            config_revision: digest(config.as_bytes()),
            inventory,
            created: Instant::now(),
        },
    );
    Ok(ChangePreview {
        id,
        remove,
        resource_path: resource.to_string_lossy().into(),
        files: vec![
            "fxmanifest.lua".into(),
            "server.js".into(),
            "bridge-token.txt (secret)".into(),
            MARKER.into(),
        ],
        config_lines: CONFIG_LINES.iter().map(|s| s.to_string()).collect(),
        expires_in_seconds: 600,
    })
}

pub(super) fn apply(app: &AppHandle, id: &str) -> Result<Installation, String> {
    let preview = PREVIEWS
        .lock()
        .map_err(|_| "Bridge previews are unavailable.")?
        .remove(id)
        .ok_or("Bridge preview expired. Review the changes again.")?;
    if preview.created.elapsed() >= Duration::from_secs(600)
        || root(&preview.target)? != preview.root
    {
        return Err("Server paths changed or preview expired. Review the changes again.".into());
    }
    let resource = preview.root.join("resources").join(RESOURCE_NAME);
    let config_path = preview.root.join("server.cfg");
    let current = config_history::read_bounded_config(&config_path)?;
    if digest(current.as_bytes()) != preview.config_revision
        || inventory(&resource)? != preview.inventory
    {
        return Err(
            "Bridge files or server.cfg changed since preview. Review the changes again.".into(),
        );
    }
    let updated = edit_config(&current, preview.remove)?;
    let previous = if resource.exists() {
        Some(owned_marker(&resource)?)
    } else {
        None
    };
    let stage = preview
        .root
        .join(format!(".fxsi-bridge-stage-{}", random_id()?));
    let old = preview
        .root
        .join(format!(".fxsi-bridge-old-{}", random_id()?));
    let new_key = if preview.remove {
        None
    } else {
        Some(random_id()?)
    };
    let staged = if let Some(key_id) = &new_key {
        let token = random_id()?;
        let key = key_path(app, key_id)?;
        fs::create_dir_all(key.parent().ok_or("Bridge key folder is unavailable.")?)
            .map_err(io_error)?;
        write_new(&key, &fxserver::encrypt_secret(token.as_bytes())?)?;
        match stage_resource(&stage, key_id, &token) {
            Ok(files) => Some(files),
            Err(error) => {
                let _ = fs::remove_file(key);
                return Err(error);
            }
        }
    } else {
        None
    };
    let mut swap = ResourceSwap {
        root: preview.root.clone(),
        resource,
        stage,
        old,
        previous: preview.inventory.clone(),
        staged,
        previous_moved: false,
        promoted: false,
    };
    let mut config_attempted = false;
    let replace_result = (|| {
        swap.promote()?;
        no_links(&config_path)?;
        let request = ConfigFileRequest {
            tx_data_path: preview.target.tx_data_path.clone(),
            profile: preview.target.profile.clone(),
            path: config_path.to_string_lossy().into(),
        };
        config_attempted = true;
        config_history::save_config_with_revision(
            &config_history::history_root(app)?,
            &request,
            &preview.config_revision,
            &updated,
            ConfigChangeReason::Patch,
        )?;
        Ok::<_, String>(())
    })();
    if let Err(error) = replace_result {
        let config_committed = if config_attempted {
            config_write_committed(&config_path, &current, &updated).map_err(|_| format!("{error} Configuration changed or cannot be verified. Bridge recovery files were preserved at {}.", swap.root.display()))?
        } else {
            false
        };
        if !config_committed {
            swap.rollback().map_err(|rollback| {
                format!(
                    "{error} Rollback incomplete: {rollback} Recovery files were preserved at {}.",
                    swap.root.display()
                )
            })?;
            if let Some(key_id) = new_key {
                let _ = fs::remove_file(key_path(app, &key_id)?);
            }
            return Err(error);
        }
        crate::commands::logs::append_background_log(app, "warn", "fxserver.bridge", "Bridge files changed, but final configuration history could not be recorded. Review server.cfg before continuing.");
    }
    swap.finish().map_err(|error| {
        format!(
            "{error} Bridge changes were applied; recovery files remain at {}.",
            swap.old.display()
        )
    })?;
    if let Some(marker) = previous {
        let _ = fs::remove_file(key_path(app, &marker.key_id)?);
    }
    inspect(app, &preview.target)
}

pub(super) fn read_token(app: &AppHandle, target: &BridgeTarget) -> Result<String, String> {
    let resource = root(target)?.join("resources").join(RESOURCE_NAME);
    let marker = owned_marker(&resource)?;
    let path = key_path(app, &marker.key_id)?;
    no_links(&path)?;
    let bytes = read_bounded(&path, 8192)?;
    let token = String::from_utf8(fxserver::decrypt_secret(&bytes)?)
        .map_err(|_| "Bridge pairing is invalid. Reinstall the bridge.")?;
    if !valid_id(&token) || marker.files.get("bridge-token.txt") != Some(&digest(token.as_bytes()))
    {
        return Err("Bridge pairing is invalid. Reinstall the bridge.".into());
    }
    Ok(token)
}

fn config_write_committed(path: &Path, current: &str, updated: &str) -> Result<bool, String> {
    no_links(path)?;
    let observed = config_history::read_bounded_config(path)?;
    if observed == current {
        return Ok(false);
    }
    if observed == updated {
        return Ok(true);
    }
    Err("Configuration changed during the bridge operation.".into())
}

fn owned_marker(resource: &Path) -> Result<Marker, String> {
    no_links(resource)?;
    let marker: Marker = serde_json::from_slice(&read_bounded(&resource.join(MARKER), 8192)?)
        .map_err(|_| {
            "This resource was not installed by this app. Its files will not be changed."
        })?;
    if marker.owner != OWNER || !valid_id(&marker.key_id) || marker.files.len() != 3 {
        return Err(
            "Bridge ownership could not be verified. Its files will not be changed.".into(),
        );
    }
    let actual = inventory(resource)?;
    if actual.len() != 4
        || ["fxmanifest.lua", "server.js", "bridge-token.txt"]
            .iter()
            .any(|name| actual.get(*name) != marker.files.get(*name))
    {
        return Err("Bridge files were modified or contain extra files. Back up your changes and review the resource manually.".into());
    }
    Ok(marker)
}

fn stage_resource(
    path: &Path,
    key_id: &str,
    token: &str,
) -> Result<BTreeMap<String, String>, String> {
    no_links(path)?;
    fs::create_dir(path).map_err(io_error)?;
    let mut files = BTreeMap::new();
    let result: Result<(), String> = (|| {
        for (name, content) in [
            ("fxmanifest.lua", MANIFEST),
            ("server.js", SCRIPT),
            ("bridge-token.txt", token),
        ] {
            write_new(&path.join(name), content.as_bytes())?;
            files.insert(name.into(), digest(content.as_bytes()));
        }
        let marker = Marker {
            owner: OWNER.into(),
            version: VERSION.into(),
            key_id: key_id.into(),
            files: files.clone(),
        };
        let content = serde_json::to_vec(&marker).map_err(io_error)?;
        write_new(&path.join(MARKER), &content)?;
        files.insert(MARKER.into(), digest(&content));
        Ok(())
    })();
    if let Err(error) = result {
        if remove_verified_files(path, &files).is_err() {
            return Err(format!(
                "{error} Incomplete staging files were preserved at {}.",
                path.display()
            ));
        }
        return Err(error);
    }
    Ok(files)
}

fn edit_config(content: &str, remove: bool) -> Result<String, String> {
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let block = CONFIG_LINES.join(newline);
    if content.matches(BEGIN).count() > 1 || content.matches(END).count() > 1 {
        return Err("Duplicate bridge configuration blocks. Review server.cfg manually.".into());
    }
    if content.contains(BEGIN) || content.contains(END) {
        let start = content
            .find(&block)
            .filter(|start| *start == 0 || content[..*start].ends_with('\n'));
        let Some(start) = start.filter(|start| {
            content[*start + block.len()..].is_empty()
                || content[*start + block.len()..].starts_with(newline)
        }) else {
            return Err("Bridge configuration was edited. Review its marked block in server.cfg before proceeding.".into());
        };
        let end = start + block.len();
        let end = end
            + if content[end..].starts_with(newline) {
                newline.len()
            } else {
                0
            };
        return Ok(if remove {
            format!("{}{}", &content[..start], &content[end..])
        } else {
            content.to_string()
        });
    }
    if remove {
        return Ok(content.to_string());
    }
    if content
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .any(|line| line.contains(RESOURCE_NAME))
    {
        return Err(
            "An unmanaged bridge entry already exists in server.cfg. Review it before installing."
                .into(),
        );
    }
    Ok(format!(
        "{content}{}{block}{newline}",
        if content.is_empty() || content.ends_with('\n') {
            ""
        } else {
            newline
        }
    ))
}

fn inventory(path: &Path) -> Result<BTreeMap<String, String>, String> {
    no_links(path)?;
    let mut result = BTreeMap::new();
    if !path.exists() {
        return Ok(result);
    }
    for entry in fs::read_dir(path).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        no_links(&entry.path())?;
        if !entry.file_type().map_err(io_error)?.is_file() || result.len() >= 4 {
            return Err("Bridge folder contains unexpected files. Nothing was removed.".into());
        }
        result.insert(
            entry.file_name().to_string_lossy().into(),
            digest(&read_bounded(&entry.path(), 256 * 1024)?),
        );
    }
    Ok(result)
}

#[cfg(test)]
fn remove_owned_resource(path: &Path) -> Result<(), String> {
    owned_marker(path)?;
    remove_verified_files(path, &inventory(path)?)
}

fn remove_owned_stage(
    path: &Path,
    root: &Path,
    expected: &BTreeMap<String, String>,
) -> Result<(), String> {
    no_links(path)?;
    if !path.try_exists().map_err(io_error)? {
        return Ok(());
    }
    if path.parent() != Some(root)
        || !path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".fxsi-bridge-"))
    {
        return Err("Refusing to remove a folder outside bridge staging.".into());
    }
    remove_verified_files(path, expected)
}

fn remove_verified_files(path: &Path, expected: &BTreeMap<String, String>) -> Result<(), String> {
    if inventory(path)? != *expected
        || expected.keys().any(|name| {
            !["fxmanifest.lua", "server.js", "bridge-token.txt", MARKER].contains(&name.as_str())
        })
    {
        return Err("Changed or unexpected bridge files were preserved.".into());
    }
    for name in expected.keys() {
        fs::remove_file(path.join(name)).map_err(io_error)?;
    }
    fs::remove_dir(path).map_err(io_error)
}

fn rename_new(source: &Path, destination: &Path) -> Result<(), String> {
    no_links(source)?;
    no_links(destination)?;
    if destination.try_exists().map_err(io_error)? {
        return Err("Bridge destination already exists. Its files were preserved.".into());
    }
    fs::rename(source, destination).map_err(io_error)
}

fn key_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("Invalid bridge pairing identifier.".into());
    }
    let path = app
        .path()
        .app_local_data_dir()
        .map_err(io_error)?
        .join("live-bridge")
        .join(format!("{id}.dpapi"));
    no_links(&path)?;
    Ok(path)
}

fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn io_error(error: impl std::fmt::Display) -> String {
    format!("Live bridge: {error}")
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    no_links(path)?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io_error)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > limit {
        return Err("Bridge file exceeds its size limit.".into());
    }
    Ok(bytes)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    no_links(path)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(io_error)?;
    let result = file
        .write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(io_error);
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn no_links(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err("Bridge files cannot use junctions or symbolic links.".into());
                    }
                }
                if meta.file_type().is_symlink() {
                    return Err("Bridge files cannot use symbolic links.".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn random_id() -> Result<String, String> {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };
    let mut bytes = [0u8; 32];
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err("Could not generate a secure bridge pairing token.".into());
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(not(windows))]
fn random_id() -> Result<String, String> {
    Err("Live bridge pairing requires Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static SEQUENCE: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "fxsi-bridge-test-{}-{}-{}",
                std::process::id(),
                super::super::timestamp(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn swap(&self, installed: bool, remove: bool) -> ResourceSwap {
            let root = self.0.clone();
            fs::create_dir(root.join("resources")).unwrap();
            let resource = root.join("resources").join(RESOURCE_NAME);
            let previous = if installed {
                stage_resource(&resource, &"a".repeat(64), &"b".repeat(64)).unwrap()
            } else {
                BTreeMap::new()
            };
            let stage = root.join(".fxsi-bridge-stage-fixture");
            let old = root.join(".fxsi-bridge-old-fixture");
            let staged = if remove {
                None
            } else {
                Some(stage_resource(&stage, &"c".repeat(64), &"d".repeat(64)).unwrap())
            };
            ResourceSwap {
                root,
                resource,
                stage,
                old,
                previous,
                staged,
                previous_moved: false,
                promoted: false,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let temp = std::env::temp_dir().canonicalize().unwrap();
            if let Ok(path) = self.0.canonicalize() {
                if path.parent() == Some(temp.as_path())
                    && path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("fxsi-bridge-test-")
                {
                    let _ = fs::remove_dir_all(path);
                }
            }
        }
    }

    #[test]
    fn config_block_is_idempotent_and_preserves_other_settings() {
        for text in [
            "sv_hostname Test\n",
            "sv_hostname Test\r\n",
            "sv_hostname Test",
        ] {
            let installed = edit_config(text, false).unwrap();
            assert_eq!(edit_config(&installed, false).unwrap(), installed);
            assert_eq!(
                edit_config(&installed, true).unwrap().trim_end(),
                text.trim_end()
            );
        }
        assert!(edit_config("ensure fxserver_installer_bridge\n", false).is_err());
        assert!(edit_config(&format!("{BEGIN}\ncustom\n{END}"), true).is_err());
    }

    #[test]
    fn owned_files_are_verified_before_removal() {
        let fixture = Fixture::new();
        let root = fixture.0.join("resource");
        stage_resource(&root, &"a".repeat(64), &"b".repeat(64)).unwrap();
        assert!(owned_marker(&root).is_ok());
        fs::write(root.join("server.js"), "user edits").unwrap();
        assert!(owned_marker(&root).is_err());
        assert!(remove_owned_resource(&root).is_err());
        fs::write(root.join("server.js"), SCRIPT).unwrap();
        fs::write(root.join("user.txt"), "keep").unwrap();
        assert!(owned_marker(&root).is_err());
        fs::remove_file(root.join("user.txt")).unwrap();
        remove_owned_resource(&root).unwrap();
        assert!(!root.exists());
    }

    #[test]
    #[cfg(windows)]
    fn pairing_tokens_have_sufficient_entropy_and_valid_shape() {
        let a = random_id().unwrap();
        let b = random_id().unwrap();
        assert!(valid_id(&a));
        assert_ne!(a, b);
        assert!(!valid_id("../../secret"));
    }

    #[test]
    fn configuration_markers_must_occupy_whole_lines() {
        let block = CONFIG_LINES.join("\n");
        for content in [
            format!("# user prefix {block}\n"),
            format!("{block} user suffix\n"),
            format!("{block}\n{block}\n"),
        ] {
            assert!(edit_config(&content, true).is_err());
            assert!(edit_config(&content, false).is_err());
        }
        let content = format!("# before\n{block}\n# after\n");
        assert_eq!(edit_config(&content, true).unwrap(), "# before\n# after\n");
    }

    #[test]
    fn failed_previous_move_never_deletes_existing_resource_or_destination() {
        let fixture = Fixture::new();
        let mut swap = fixture.swap(true, false);
        fs::create_dir(&swap.old).unwrap();
        fs::write(swap.old.join("user.txt"), "keep").unwrap();
        assert!(swap.promote().is_err());
        assert!(!swap.previous_moved && !swap.promoted);
        swap.rollback().unwrap();
        assert_eq!(inventory(&swap.resource).unwrap(), swap.previous);
        assert_eq!(
            fs::read_to_string(swap.old.join("user.txt")).unwrap(),
            "keep"
        );
        assert!(!swap.stage.exists());
    }

    #[test]
    fn failed_stage_promotion_restores_previous_resource() {
        let fixture = Fixture::new();
        let mut swap = fixture.swap(true, false);
        let displaced = fixture.0.join("displaced-stage");
        fs::rename(&swap.stage, &displaced).unwrap();
        assert!(swap.promote().is_err());
        assert!(swap.previous_moved && !swap.promoted);
        swap.rollback().unwrap();
        assert_eq!(inventory(&swap.resource).unwrap(), swap.previous);
        assert!(!swap.old.exists());
        assert_eq!(
            inventory(&displaced).unwrap(),
            *swap.staged.as_ref().unwrap()
        );
    }

    #[test]
    fn config_failure_rolls_back_install_upgrade_and_removal() {
        for (installed, remove) in [(false, false), (true, false), (true, true)] {
            let fixture = Fixture::new();
            let mut swap = fixture.swap(installed, remove);
            swap.promote().unwrap();
            swap.rollback().unwrap();
            assert_eq!(inventory(&swap.resource).unwrap(), swap.previous);
            assert_eq!(swap.resource.exists(), installed);
            assert!(!swap.old.exists());
            assert!(!swap.stage.exists());
        }
    }

    #[test]
    fn cleanup_preserves_modified_backups_and_promoted_resources() {
        for edit_backup in [false, true] {
            let fixture = Fixture::new();
            let mut swap = fixture.swap(true, false);
            swap.promote().unwrap();
            let edited = if edit_backup {
                &swap.old
            } else {
                &swap.resource
            };
            fs::write(edited.join("server.js"), "user changes").unwrap();
            let before = inventory(edited).unwrap();
            assert!(if edit_backup {
                swap.finish()
            } else {
                swap.rollback()
            }
            .is_err());
            assert_eq!(inventory(edited).unwrap(), before);
            assert!(swap.old.exists() && swap.resource.exists());
        }
    }

    #[test]
    fn rollback_preserves_a_resource_created_during_removal() {
        let fixture = Fixture::new();
        let mut swap = fixture.swap(true, true);
        swap.promote().unwrap();
        fs::create_dir(&swap.resource).unwrap();
        fs::write(swap.resource.join("user.txt"), "keep").unwrap();
        assert!(swap.rollback().is_err());
        assert_eq!(
            fs::read_to_string(swap.resource.join("user.txt")).unwrap(),
            "keep"
        );
        assert_eq!(inventory(&swap.old).unwrap(), swap.previous);
    }

    #[test]
    fn successful_removal_only_cleans_the_verified_backup() {
        let fixture = Fixture::new();
        let mut swap = fixture.swap(true, true);
        fs::write(fixture.0.join("server.cfg"), "user settings").unwrap();
        swap.promote().unwrap();
        swap.finish().unwrap();
        assert!(!swap.resource.exists() && !swap.old.exists());
        assert_eq!(
            fs::read_to_string(fixture.0.join("server.cfg")).unwrap(),
            "user settings"
        );
    }

    #[test]
    fn failed_history_save_preserves_config_and_restores_the_resource() {
        let fixture = Fixture::new();
        let mut swap = fixture.swap(true, false);
        let tx = fixture.0.join("txData");
        fs::create_dir_all(tx.join("profile")).unwrap();
        fs::write(
            tx.join("profile/config.json"),
            serde_json::to_vec(&serde_json::json!({ "dataPath": fixture.0 })).unwrap(),
        )
        .unwrap();
        let config = fixture.0.join("server.cfg");
        let original = "sv_hostname Fixture\n";
        let updated = edit_config(original, false).unwrap();
        fs::write(&config, original).unwrap();
        let history = fixture.0.join("history");
        fs::write(&history, "not a directory").unwrap();
        let request = ConfigFileRequest {
            tx_data_path: tx.to_string_lossy().into(),
            profile: "profile".into(),
            path: config.to_string_lossy().into(),
        };
        swap.promote().unwrap();
        assert!(config_history::save_config_with_revision(
            &history,
            &request,
            &digest(original.as_bytes()),
            &updated,
            ConfigChangeReason::Patch
        )
        .is_err());
        assert!(!config_write_committed(&config, original, &updated).unwrap());
        swap.rollback().unwrap();
        assert_eq!(fs::read_to_string(config).unwrap(), original);
        assert_eq!(inventory(&swap.resource).unwrap(), swap.previous);
        assert!(!swap.old.exists() && !swap.stage.exists());
    }

    #[test]
    fn ambiguous_config_failure_preserves_both_recovery_versions() {
        let fixture = Fixture::new();
        let mut swap = fixture.swap(true, false);
        let config = fixture.0.join("server.cfg");
        swap.promote().unwrap();
        fs::write(&config, "after").unwrap();
        assert!(config_write_committed(&config, "before", "after").unwrap());
        assert!(!config_write_committed(&config, "after", "after").unwrap());
        fs::write(&config, "external edit").unwrap();
        assert!(config_write_committed(&config, "before", "after").is_err());
        fs::remove_file(&config).unwrap();
        assert!(config_write_committed(&config, "before", "after").is_err());
        assert_eq!(inventory(&swap.old).unwrap(), swap.previous);
        assert_eq!(
            inventory(&swap.resource).unwrap(),
            *swap.staged.as_ref().unwrap()
        );
    }

    #[test]
    fn staging_collision_and_outside_cleanup_preserve_existing_files() {
        let fixture = Fixture::new();
        let path = fixture.0.join(".fxsi-bridge-stage-fixture");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("server.js"), "keep").unwrap();
        assert!(stage_resource(&path, &"a".repeat(64), &"b".repeat(64)).is_err());
        let expected = inventory(&path).unwrap();
        assert!(remove_owned_stage(&path, &fixture.0.join("elsewhere"), &expected).is_err());
        assert_eq!(inventory(&path).unwrap(), expected);
    }
}
