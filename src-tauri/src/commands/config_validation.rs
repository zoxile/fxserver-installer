use std::{
    collections::{HashMap, HashSet},
    fs,
    net::{IpAddr, SocketAddr},
    path::{Component, Path, PathBuf},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::config_history::{ensure_unlinked_path, read_bounded_config, ConfigFileRequest};
use super::diagnostics::parsing;

const MAX_BYTES: usize = 512 * 1024;
const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_FILES: usize = 128;
const MAX_COMMANDS: usize = 20_000;
const MAX_ISSUES: usize = 256;
const MAX_RESOURCE_ENTRIES: usize = 4096;
const UNVERIFIED_INCLUDE: &str = "This include uses a linked configuration or resource location and was not read. File existence, contents, cycles, and conflicts there are unverified; review that location before saving.";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    code: &'static str,
    severity: &'static str,
    file: String,
    line: Option<usize>,
    message: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigValidationReport {
    pub issues: Vec<ValidationIssue>,
    pub revision: String,
    pub file_count: usize,
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn validate_config_change(
    request: ConfigFileRequest,
    content: String,
) -> Result<ConfigValidationReport, String> {
    super::run_blocking(move || {
        let (_, _, _, root) =
            super::fxserver::resolve_profile_data_path(request.tx_data_path, request.profile)
                .map_err(|_| "The selected profile data directory could not be validated.")?;
        inspect(&root, Path::new(&request.path), &content)
    })
    .await
}

#[cfg(any(test, feature = "terminal"))]
pub(crate) enum ConfigHelperChange {
    Value {
        setting: String,
        value: String,
    },
    Ace {
        principal: String,
        object: String,
        access: String,
    },
    Principal {
        child: String,
        parent: String,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(any(test, feature = "terminal"))]
pub(crate) struct ConfigHelperPreview {
    pub current_revision: String,
    pub command: String,
    pub changed: bool,
    pub validation: ConfigValidationReport,
}

#[cfg(any(test, feature = "terminal"))]
pub(crate) fn preview_config_helper(
    request: &ConfigFileRequest,
    change: &ConfigHelperChange,
) -> Result<ConfigHelperPreview, String> {
    prepare_config_helper(request, change).map(|(_, preview)| preview)
}

#[cfg(any(test, feature = "terminal"))]
pub(crate) fn apply_config_helper(
    store: &Path,
    request: &ConfigFileRequest,
    change: &ConfigHelperChange,
    expected_revision: &str,
    expected_validation_revision: &str,
) -> Result<crate::models::fxserver::ServerConfigFile, String> {
    let (content, preview) = prepare_config_helper(request, change)?;
    if preview.current_revision != expected_revision
        || preview.validation.revision != expected_validation_revision
    {
        return Err("CONFIG_CHANGED: Config contents, helper values or validation evidence changed. Preview the helper again before applying it.".into());
    }
    if preview
        .validation
        .issues
        .iter()
        .any(|issue| issue.severity == "error")
    {
        return Err("The helper has blocking validation issues. Review the preview and use the cfg editor to resolve them.".into());
    }
    super::config_history::save_config_with_revision(
        store,
        request,
        expected_revision,
        &content,
        super::config_history::ConfigChangeReason::Patch,
    )
}

#[cfg(any(test, feature = "terminal"))]
fn prepare_config_helper(
    request: &ConfigFileRequest,
    change: &ConfigHelperChange,
) -> Result<(String, ConfigHelperPreview), String> {
    let (_, _, _, root) = super::fxserver::resolve_profile_data_path(
        request.tx_data_path.clone(),
        request.profile.clone(),
    )?;
    let path = safe_file(Path::new(&request.path), &root)
        .map_err(|_| "Choose an existing, unlinked cfg in the selected profile data directory.")?;
    let current = read_bounded_config(&path)?;
    let (content, command) = helper_content(&current, change)?;
    let validation = inspect(&root, &path, &content)?;
    let preview = ConfigHelperPreview {
        current_revision: super::config_history::content_revision(&current),
        command,
        changed: current != content,
        validation,
    };
    Ok((content, preview))
}

#[cfg(any(test, feature = "terminal"))]
fn helper_token(value: &str) -> Result<&str, String> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-*".contains(&byte))
    {
        return Err("Permission tokens must be 1-256 ASCII letters, digits, dots, underscores, colons, hyphens or asterisks.".into());
    }
    Ok(value)
}

#[cfg(any(test, feature = "terminal"))]
fn helper_content(source: &str, change: &ConfigHelperChange) -> Result<(String, String), String> {
    let (commands, uncertain) = parsing::config_commands_checked(source);
    if uncertain {
        return Err(
            "The config cannot be parsed completely. Use the cfg editor before applying helpers."
                .into(),
        );
    }
    let mut replace_line = None;
    let mut already_present = false;
    let command = match change {
        ConfigHelperChange::Value { setting, value } => {
            let key = match setting.as_str() {
                "sv_hostname" | "sv_maxclients" => setting.as_str(),
                "sets sv_projectName" => "sv_projectname",
                "sets sv_projectDesc" => "sv_projectdesc",
                "sets tags" => "tags",
                "sets locale" => "locale",
                _ => return Err("Choose a supported common cfg setting.".into()),
            };
            if value.is_empty()
                || value.len() > 1024
                || value
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '"' | '\\' | ';'))
            {
                return Err("Helper values must be 1-1024 bytes without control characters, quotes, backslashes or semicolons.".into());
            }
            if setting == "sv_maxclients"
                && value
                    .parse::<u16>()
                    .ok()
                    .filter(|value| *value > 0)
                    .is_none()
            {
                return Err("sv_maxclients must be a positive 16-bit integer.".into());
            }
            for (line, words) in &commands {
                let name = if ["set", "setr", "sets"]
                    .iter()
                    .any(|verb| words[0].eq_ignore_ascii_case(verb))
                {
                    words.get(1).map(String::as_str).unwrap_or("")
                } else {
                    words[0].as_str()
                };
                if name.eq_ignore_ascii_case(key) {
                    if replace_line.replace(*line).is_some()
                        || commands.iter().filter(|(other, _)| other == line).count() != 1
                    {
                        return Err("The setting is repeated or shares a line with another command. Use the cfg editor to preserve its meaning.".into());
                    }
                    let expected_words = if setting.starts_with("sets ") { 3 } else { 2 };
                    let expected_verb = if setting.starts_with("sets ") {
                        "sets"
                    } else {
                        setting
                    };
                    if words.len() != expected_words
                        || !words[0].eq_ignore_ascii_case(expected_verb)
                    {
                        return Err("This setting uses a different command form. Use the cfg editor to review it.".into());
                    }
                }
            }
            format!("{setting} \"{value}\"")
        }
        ConfigHelperChange::Ace {
            principal,
            object,
            access,
        } => {
            helper_token(principal)?;
            helper_token(object)?;
            if !matches!(access.as_str(), "allow" | "deny") {
                return Err("ACE access must be allow or deny.".into());
            }
            for (_, words) in &commands {
                if words[0].eq_ignore_ascii_case("add_ace")
                    && words.get(1) == Some(principal)
                    && words.get(2) == Some(object)
                {
                    if words.len() != 4 || words[3] != *access {
                        return Err("An ACE for this principal and object already has a different form or access. Review it in the cfg editor.".into());
                    }
                    already_present = true;
                }
            }
            format!("add_ace {principal} {object} {access}")
        }
        ConfigHelperChange::Principal { child, parent } => {
            helper_token(child)?;
            helper_token(parent)?;
            if child == parent {
                return Err("A principal cannot inherit itself.".into());
            }
            already_present = commands.iter().any(|(_, words)| {
                words.len() == 3
                    && words[0].eq_ignore_ascii_case("add_principal")
                    && words[1] == *child
                    && words[2] == *parent
            });
            format!("add_principal {child} {parent}")
        }
    };
    if already_present {
        return Ok((source.into(), command));
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let content = if let Some(line) = replace_line {
        source
            .split_inclusive('\n')
            .enumerate()
            .map(|(index, existing)| {
                if index + 1 == line {
                    let ending = if existing.ends_with("\r\n") {
                        "\r\n"
                    } else if existing.ends_with('\n') {
                        "\n"
                    } else {
                        ""
                    };
                    let bom = if existing.starts_with('\u{feff}') {
                        "\u{feff}"
                    } else {
                        ""
                    };
                    format!("{bom}{command}{ending}")
                } else {
                    existing.into()
                }
            })
            .collect::<String>()
    } else {
        format!(
            "{source}{}{command}{newline}",
            if source.is_empty() || source.ends_with('\n') {
                ""
            } else {
                newline
            }
        )
    };
    if content.len() > MAX_BYTES {
        return Err("The helper would exceed the 512 KiB cfg limit.".into());
    }
    Ok((content, command))
}

fn safe_file(path: &Path, root: &Path) -> Result<PathBuf, &'static str> {
    if !path.is_absolute() {
        return Err("path-unsafe");
    }
    ensure_unlinked_path(path).map_err(|_| "path-unsafe")?;
    let resolved = path.canonicalize().map_err(|_| "exec-missing")?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err("path-unsafe");
    }
    Ok(resolved)
}

fn linked_component(path: &Path, root: &Path) -> Result<bool, &'static str> {
    let relative = path.strip_prefix(root).map_err(|_| "path-unsafe")?;
    let mut current = root.to_path_buf();
    // Inspect from the trusted root outward; never query descendants through a link.
    for component in relative.components() {
        match component {
            Component::CurDir => continue,
            Component::Normal(name) if !cfg!(windows) || !name.to_string_lossy().contains(':') => {
                current.push(name)
            }
            _ => return Err("path-unsafe"),
        }
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err("config-unreadable"),
        };
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Ok(true);
            }
        }
        if metadata.file_type().is_symlink() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn include_file(path: &Path, root: &Path) -> Result<PathBuf, &'static str> {
    if linked_component(path, root)? {
        return Err("exec-unverified");
    }
    safe_file(path, root)
}

fn inspect(root: &Path, selected: &Path, content: &str) -> Result<ConfigValidationReport, String> {
    if content.len() > MAX_BYTES {
        return Err("The draft exceeds the 512 KiB configuration limit.".into());
    }
    let root = root
        .canonicalize()
        .map_err(|_| "The data directory is unavailable.")?;
    let selected = safe_file(selected, &root).map_err(|_| {
        "Choose an existing, unlinked cfg inside the selected profile data directory."
    })?;
    if !selected
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cfg"))
    {
        return Err("Only cfg files can be validated for saving.".into());
    }
    let mut scan = Scan {
        root: &root,
        selected: &selected,
        draft: content,
        issues: Vec::new(),
        visited: HashSet::new(),
        ancestors: HashSet::new(),
        settings: HashSet::new(),
        endpoints: Vec::new(),
        resources: None,
        bytes: 0,
        commands: 0,
        revision: Sha256::new(),
    };
    scan.hash(root.to_string_lossy().as_bytes());
    scan.hash(selected.to_string_lossy().as_bytes());
    // Include the startup graph so an edited include is compared with its callers.
    let entry = root.join("server.cfg");
    if entry.exists() && entry != selected {
        scan.visit(&entry, "server.cfg", None);
    }
    if !scan.visited.contains(&selected) {
        // An inactive cfg is a separate execution context, not another startup include.
        scan.settings.clear();
        scan.endpoints.clear();
        scan.visit(&selected, "Edited file", None);
    }
    scan.hash(&serde_json::to_vec(&scan.issues).unwrap_or_default());
    Ok(ConfigValidationReport {
        file_count: scan.visited.len(),
        revision: format!("{:x}", scan.revision.finalize()),
        issues: scan.issues,
    })
}

struct Scan<'a> {
    root: &'a Path,
    selected: &'a Path,
    draft: &'a str,
    issues: Vec<ValidationIssue>,
    visited: HashSet<PathBuf>,
    ancestors: HashSet<PathBuf>,
    settings: HashSet<String>,
    endpoints: Vec<(bool, SocketAddr)>,
    resources: Option<Result<ResourceRoots, &'static str>>,
    bytes: usize,
    commands: usize,
    revision: Sha256,
}

impl Scan<'_> {
    fn hash(&mut self, bytes: &[u8]) {
        self.revision.update((bytes.len() as u64).to_le_bytes());
        self.revision.update(bytes);
    }

    fn issue(
        &mut self,
        code: &'static str,
        severity: &'static str,
        file: &str,
        line: Option<usize>,
        message: &'static str,
    ) {
        if self.issues.len() >= MAX_ISSUES {
            return;
        }
        let issue = if self.issues.len() == MAX_ISSUES - 1 {
            ValidationIssue {
                code: "issue-limit",
                severity: "error",
                file: "Configuration".into(),
                line: None,
                message:
                    "The issue limit was reached. Resolve the reported issues and validate again.",
            }
        } else {
            ValidationIssue {
                code,
                severity,
                file: file.into(),
                line,
                message,
            }
        };
        self.issues.push(issue);
    }

    fn visit(&mut self, path: &Path, caller: &str, line: Option<usize>) {
        let path = match include_file(path, self.root) {
            Ok(path) => path,
            Err("exec-unverified") => {
                self.issue(
                    "exec-unverified",
                    "warning",
                    caller,
                    line,
                    UNVERIFIED_INCLUDE,
                );
                return;
            }
            Err(code) => {
                self.issue(code, "error", caller, line, "An exec target is missing, unreadable, linked, or outside the selected data directory.");
                return;
            }
        };
        if self.ancestors.contains(&path) {
            self.issue(
                "exec-cycle",
                "error",
                caller,
                line,
                "This exec creates a configuration include cycle.",
            );
            return;
        }
        if self.visited.contains(&path) {
            self.issue("exec-repeated", "warning", caller, line, "This file is included more than once. Repeated execution and endpoint additions need review.");
        }
        if self.visited.len() >= MAX_FILES
            || self.ancestors.len() >= 32
            || self.commands >= MAX_COMMANDS
        {
            self.issue("scan-limit", "error", caller, line, "The bounded configuration scan could not finish. Reduce include depth or configuration size.");
            return;
        }
        let file = if path == self.selected {
            "Edited file".into()
        } else if path == self.root.join("server.cfg") {
            "server.cfg".into()
        } else {
            format!("Included config {}", self.visited.len() + 1)
        };
        let source = if path == self.selected {
            self.draft.to_string()
        } else {
            // Reserve the reader's worst case, including its overflow-detection byte.
            // Failed/oversized reads must count too, even for repeated includes.
            if self.bytes > MAX_TOTAL_BYTES - MAX_BYTES - 1 {
                self.issue("scan-limit", "error", caller, line, "The remaining configuration read budget is too small for another bounded file read.");
                return;
            }
            match read_bounded_config(&path) {
                Ok(source) => source,
                Err(_) => {
                    self.bytes += MAX_BYTES + 1;
                    self.issue(
                        "config-unreadable",
                        "error",
                        caller,
                        line,
                        "An included file is not readable UTF-8 or exceeds 512 KiB.",
                    );
                    return;
                }
            }
        };
        self.bytes += source.len();
        if self.bytes > MAX_TOTAL_BYTES {
            self.issue(
                "scan-limit",
                "error",
                caller,
                line,
                "The configuration scan exceeds the 4 MiB total read limit.",
            );
            return;
        }
        self.hash(path.to_string_lossy().as_bytes());
        self.hash(source.as_bytes());
        self.visited.insert(path.clone());
        self.ancestors.insert(path.clone());
        let (commands, uncertain) = parsing::config_commands_checked(&source);
        if source.contains('\0') || uncertain {
            self.issue("cfg-malformed", "error", &file, None, "Unterminated quotes, NUL characters, or parser limits prevent a complete configuration review.");
            self.ancestors.remove(&path);
            return;
        }
        for (line, words) in commands {
            self.commands += 1;
            if self.commands > MAX_COMMANDS {
                self.issue(
                    "scan-limit",
                    "error",
                    &file,
                    Some(line),
                    "The scan exceeds 20,000 configuration commands.",
                );
                break;
            }
            self.command(&words, &file, line);
        }
        self.ancestors.remove(&path);
    }

    fn command(&mut self, words: &[String], file: &str, line: usize) {
        let command = words[0].to_ascii_lowercase();
        if command == "exec" {
            if words.len() != 2 || words[1].is_empty() {
                self.issue("exec-arguments", "error", file, Some(line), "Exec requires exactly one non-empty file argument; quote paths containing spaces.");
                return;
            }
            match self.exec_target(&words[1]) {
                Ok(target) => self.visit(&target, file, Some(line)),
                Err("exec-dynamic") => self.issue("exec-dynamic", "warning", file, Some(line), "A dynamic exec target cannot be resolved statically and was not read."),
                Err("exec-unverified") => self.issue("exec-unverified", "warning", file, Some(line), UNVERIFIED_INCLUDE),
                Err(code) => self.issue(code, "error", file, Some(line), "The exec target is missing, ambiguous, unsafe, or outside the bounded resource scan."),
            }
            return;
        }
        if matches!(command.as_str(), "endpoint_add_tcp" | "endpoint_add_udp") {
            self.endpoint(words, file, line, command == "endpoint_add_tcp");
            return;
        }
        let setter = matches!(command.as_str(), "set" | "setr" | "sets" | "seta");
        if setter && (words.len() < 3 || words[1].is_empty()) {
            self.issue("setting-arguments", "error", file, Some(line), "A setting command requires a name and a value (an explicitly quoted empty value is allowed).");
            return;
        }
        let offset = usize::from(setter);
        if words.len() <= offset + 1 {
            return;
        }
        let name = words[offset].to_ascii_lowercase();
        // Only classify known direct convars; resource/control commands are repeatable.
        let direct = name.starts_with("sv_")
            || name.starts_with("net_")
            || name.starts_with("onesync")
            || matches!(
                name.as_str(),
                "rcon_password" | "steam_webapikey" | "mysql_connection_string" | "gamename"
            );
        if setter || direct {
            if !self.settings.insert(name.clone()) {
                self.issue("setting-duplicate", "warning", file, Some(line), "This setting was assigned earlier in this execution context. The later value may intentionally override it.");
            }
            if (secret_name(&name) || words[offset + 1..].iter().any(|value| secret_value(value)))
                && words[offset + 1..].iter().any(|value| !value.is_empty())
            {
                self.issue("secret-exposed", "warning", file, Some(line), if matches!(command.as_str(), "sets" | "setr") {
                    "A potentially sensitive setting is public or replicated to clients. Keep credentials in server-only settings. Values are omitted."
                } else {
                    "A potentially sensitive value is stored in plaintext configuration. Protect this file and avoid sharing it. Values are omitted."
                });
            }
        }
    }

    fn exec_target(&mut self, reference: &str) -> Result<PathBuf, &'static str> {
        // FXServer resolves plain exec paths from dataPath, not the including cfg's folder.
        let normalized = if cfg!(windows) {
            reference.replace('\\', "/")
        } else {
            reference.to_string()
        };
        let drive_prefix = normalized
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
            && normalized.as_bytes().get(1) == Some(&b':');
        if normalized.contains('\0')
            || drive_prefix
            || cfg!(windows) && normalized.contains(':')
            || !cfg!(windows) && normalized.contains('\\')
            || normalized.starts_with('/')
            || normalized.split('/').any(|part| {
                part == ".." || cfg!(windows) && part.ends_with([' ', '.']) && part != "."
            })
        {
            return Err("path-unsafe");
        }
        if normalized.contains(['$', '*', '?', '%']) {
            return Err("exec-dynamic");
        }
        if let Some(resource) = normalized.strip_prefix('@') {
            let (name, file) = resource.split_once('/').ok_or("exec-missing")?;
            if name.is_empty() || file.is_empty() || file.starts_with('/') {
                return Err("path-unsafe");
            }
            if self.resources.is_none() {
                self.resources = Some(resource_roots(self.root));
            }
            let resources = self
                .resources
                .as_ref()
                .unwrap()
                .as_ref()
                .map_err(|error| *error)?;
            if resources.linked_groups {
                return Err("exec-unverified");
            }
            let root = resources
                .roots
                .get(&name.to_ascii_lowercase())
                .ok_or("exec-missing")?
                .as_ref()
                .map_err(|error| *error)?;
            return include_file(&root.join(file), root);
        }
        let relative = Path::new(&normalized);
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
        {
            return Err("path-unsafe");
        }
        include_file(&self.root.join(relative), self.root)
    }

    fn endpoint(&mut self, words: &[String], file: &str, line: usize, tcp: bool) {
        if self.endpoints.len() >= 128 {
            self.issue(
                "endpoint-limit",
                "error",
                file,
                Some(line),
                "More than 128 endpoints prevent a complete bounded conflict check.",
            );
            return;
        }
        if words.len() != 2 {
            self.issue(
                "endpoint-invalid",
                "error",
                file,
                Some(line),
                "An endpoint requires one IP address and port argument.",
            );
            return;
        }
        if words[1].contains(['$', '%']) {
            self.issue(
                "endpoint-dynamic",
                "warning",
                file,
                Some(line),
                "A dynamic endpoint cannot be checked for port conflicts statically.",
            );
            return;
        }
        let Ok(address) = words[1].parse::<SocketAddr>() else {
            self.issue("endpoint-invalid", "error", file, Some(line), "An endpoint must be an IPv4 address or bracketed IPv6 address with a port from 1 to 65535.");
            return;
        };
        if address.port() == 0 {
            self.issue(
                "endpoint-invalid",
                "error",
                file,
                Some(line),
                "Endpoint port zero is not a fixed server listening port.",
            );
            return;
        }
        let ip = normalize_ip(address.ip());
        for &(other_tcp, other) in &self.endpoints {
            if tcp != other_tcp || address.port() != other.port() {
                continue;
            }
            let other_ip = normalize_ip(other.ip());
            if ip == other_ip
                || (ip.is_ipv4() == other_ip.is_ipv4()
                    && (ip.is_unspecified() || other_ip.is_unspecified()))
            {
                self.issue("endpoint-conflict", "error", file, Some(line), "Two endpoints overlap on the same protocol and port. TCP and UDP may intentionally share a port.");
                break;
            }
            if ip.is_ipv4() != other_ip.is_ipv4()
                && (ip.is_unspecified() || other_ip.is_unspecified())
            {
                self.issue("endpoint-dual-stack", "warning", file, Some(line), "IPv4 and IPv6 wildcard endpoints share a protocol and port; dual-stack binding may conflict on this host.");
                break;
            }
        }
        self.endpoints.push((tcp, address));
    }
}

fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(ip)),
        _ => ip,
    }
}

fn secret_name(name: &str) -> bool {
    let compact: String = name.chars().filter(char::is_ascii_alphanumeric).collect();
    [
        "password",
        "passwd",
        "secret",
        "token",
        "authorization",
        "apikey",
        "licensekey",
        "licencekey",
        "connectionstring",
        "webhook",
        "privatekey",
        "credential",
        "cookie",
    ]
    .iter()
    .any(|key| compact.contains(key))
        || compact == "pwd"
}

fn secret_value(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("cfxk_")
        || value.contains("ghp_")
        || value.contains("bearer ")
        || value.contains("private key-----")
        || (value.contains("://") && value.contains('@'))
}

#[derive(Default)]
struct ResourceRoots {
    roots: HashMap<String, Result<PathBuf, &'static str>>,
    linked_groups: bool,
}

impl ResourceRoots {
    fn insert(&mut self, name: String, path: Result<PathBuf, &'static str>) {
        self.roots
            .entry(name)
            .and_modify(|existing| *existing = Err("resource-ambiguous"))
            .or_insert(path);
    }
}

fn resource_roots(root: &Path) -> Result<ResourceRoots, &'static str> {
    let mut resources = ResourceRoots::default();
    let mut pending = vec![(root.join("resources"), 0)];
    let mut count = 0;
    while let Some((directory, depth)) = pending.pop() {
        if linked_component(&directory, root)? {
            resources.linked_groups = true;
            continue;
        }
        ensure_unlinked_path(&directory).map_err(|_| "resource-unsafe")?;
        let directory = directory.canonicalize().map_err(|_| "exec-missing")?;
        if !directory.starts_with(root) {
            return Err("resource-unsafe");
        }
        for entry in fs::read_dir(directory).map_err(|_| "resource-unreadable")? {
            count += 1;
            if count > MAX_RESOURCE_ENTRIES {
                return Err("resource-limit");
            }
            let entry = entry.map_err(|_| "resource-unreadable")?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let group = name.starts_with('[') && name.ends_with(']');
            if linked_component(&path, root)? {
                if group {
                    resources.linked_groups = true;
                } else {
                    resources.insert(name.to_ascii_lowercase(), Err("exec-unverified"));
                }
                continue;
            }
            ensure_unlinked_path(&path).map_err(|_| "resource-unsafe")?;
            if !entry
                .file_type()
                .map_err(|_| "resource-unreadable")?
                .is_dir()
            {
                continue;
            }
            if group {
                if depth >= 8 {
                    return Err("resource-limit");
                }
                pending.push((path, depth + 1));
            } else if ["fxmanifest.lua", "__resource.lua"]
                .iter()
                .any(|file| safe_file(&path.join(file), root).is_ok())
            {
                resources.insert(name.to_ascii_lowercase(), Ok(path));
            }
        }
    }
    Ok(resources)
}

#[cfg(test)]
#[path = "config_validation/tests.rs"]
mod tests;

#[cfg(test)]
mod helper_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn hostname(value: &str) -> ConfigHelperChange {
        ConfigHelperChange::Value {
            setting: "sv_hostname".into(),
            value: value.into(),
        }
    }

    struct Fixture {
        root: PathBuf,
        request: ConfigFileRequest,
    }

    impl Fixture {
        fn new(source: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cfg-helper-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let data = root.join("data");
            let tx = root.join("txData");
            fs::create_dir_all(tx.join("default")).unwrap();
            fs::create_dir(&data).unwrap();
            fs::write(
                tx.join("default/config.json"),
                serde_json::json!({"dataPath": data}).to_string(),
            )
            .unwrap();
            let path = data.join("server.cfg");
            fs::write(&path, source).unwrap();
            Self {
                request: ConfigFileRequest {
                    tx_data_path: tx.to_string_lossy().into(),
                    profile: "default".into(),
                    path: path.to_string_lossy().into(),
                },
                root,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn common_value_helper_uses_parser_lines_and_preserves_other_lines_and_endings() {
        let source =
            "\u{feff}sv_hostname \"old\" # previous label\r\nrcon_password \"fixture-secret\"\r\n";
        let (updated, command) = helper_content(source, &hostname("new name")).unwrap();
        assert_eq!(command, "sv_hostname \"new name\"");
        assert_eq!(
            updated,
            "\u{feff}sv_hostname \"new name\"\r\nrcon_password \"fixture-secret\"\r\n"
        );
        assert_eq!(
            helper_content("# empty", &hostname("new")).unwrap().0,
            "# empty\nsv_hostname \"new\"\n"
        );
    }

    #[test]
    fn helpers_refuse_ambiguous_commands_and_value_injection() {
        for source in [
            "sv_hostname old; ensure other",
            "sv_hostname old\nsv_hostname again",
            "set sv_hostname old",
            "sv_hostname \"unterminated",
        ] {
            assert!(helper_content(source, &hostname("new")).is_err());
        }
        for value in ["", "a\nb", "a;b", "a\"b", "a\\b", "\u{1b}"] {
            assert!(helper_content("", &hostname(value)).is_err());
        }
        assert!(helper_content(
            "",
            &ConfigHelperChange::Value {
                setting: "rcon_password".into(),
                value: "not-allowed".into()
            }
        )
        .is_err());
        assert!(helper_content(
            "",
            &ConfigHelperChange::Value {
                setting: "sv_maxclients".into(),
                value: "0".into()
            }
        )
        .is_err());
    }

    #[test]
    fn permission_helpers_are_idempotent_and_reject_conflicts_and_injection() {
        let ace = ConfigHelperChange::Ace {
            principal: "group.admin".into(),
            object: "command.quit".into(),
            access: "deny".into(),
        };
        let command = "add_ace group.admin command.quit deny\n";
        assert_eq!(helper_content("", &ace).unwrap().0, command);
        assert_eq!(helper_content(command, &ace).unwrap().0, command);
        assert!(helper_content("add_ace group.admin command.quit allow", &ace).is_err());
        assert!(helper_token("group.admin; quit").is_err());
        let principal = ConfigHelperChange::Principal {
            child: "group.admin".into(),
            parent: "group.user".into(),
        };
        assert_eq!(
            helper_content("", &principal).unwrap().0,
            "add_principal group.admin group.user\n"
        );
        assert!(helper_content(
            "",
            &ConfigHelperChange::Principal {
                child: "group.admin".into(),
                parent: "group.admin".into()
            }
        )
        .is_err());
    }

    #[test]
    fn helper_previews_hide_current_secrets_and_bind_values_and_include_evidence() {
        let source = "rcon_password fixture-secret\nexec included.cfg\n";
        let fixture = Fixture::new(source);
        fs::write(fixture.root.join("data/included.cfg"), "set marker old\n").unwrap();
        let preview = preview_config_helper(&fixture.request, &hostname("new")).unwrap();
        assert!(!serde_json::to_string(&preview)
            .unwrap()
            .contains("fixture-secret"));
        assert!(preview.changed);
        let store = fixture.root.join("history");
        assert!(apply_config_helper(
            &store,
            &fixture.request,
            &hostname("different"),
            &preview.current_revision,
            &preview.validation.revision
        )
        .unwrap_err()
        .contains("CONFIG_CHANGED"));
        fs::write(
            fixture.root.join("data/included.cfg"),
            "set marker changed\n",
        )
        .unwrap();
        assert!(apply_config_helper(
            &store,
            &fixture.request,
            &hostname("new"),
            &preview.current_revision,
            &preview.validation.revision
        )
        .unwrap_err()
        .contains("CONFIG_CHANGED"));
        assert_eq!(fs::read_to_string(&fixture.request.path).unwrap(), source);
        assert!(!store.exists());
    }

    #[test]
    fn helper_apply_rejects_changed_current_file_and_blocking_validation() {
        let fixture = Fixture::new("endpoint_add_tcp invalid\n");
        let change = hostname("new");
        let preview = preview_config_helper(&fixture.request, &change).unwrap();
        let store = fixture.root.join("history");
        assert!(apply_config_helper(
            &store,
            &fixture.request,
            &change,
            &preview.current_revision,
            &preview.validation.revision
        )
        .unwrap_err()
        .contains("blocking validation"));
        fs::write(&fixture.request.path, "external change").unwrap();
        assert!(apply_config_helper(
            &store,
            &fixture.request,
            &change,
            &preview.current_revision,
            &preview.validation.revision
        )
        .unwrap_err()
        .contains("CONFIG_CHANGED"));
        assert!(!store.exists());
    }

    #[cfg(windows)]
    #[test]
    fn helper_apply_persists_with_shared_encrypted_history() {
        let fixture = Fixture::new("sv_hostname old\n");
        let change = hostname("new");
        let preview = preview_config_helper(&fixture.request, &change).unwrap();
        let store = fixture.root.join("history");
        let file = apply_config_helper(
            &store,
            &fixture.request,
            &change,
            &preview.current_revision,
            &preview.validation.revision,
        )
        .unwrap();
        assert_eq!(file.content, "sv_hostname \"new\"\n");
        let versions =
            super::super::config_history::list_history_in(&store, &fixture.request).unwrap();
        assert_eq!(versions[0].reason, "patch");
        assert_eq!(versions[1].reason, "before-patch");
    }
}
