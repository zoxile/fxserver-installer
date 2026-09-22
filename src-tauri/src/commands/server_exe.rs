//! Edition detection adapted from Huntercorlett's fork, commit 53f1834.
//! Mixed installations are rejected instead of preferring either executable.

use std::path::{Path, PathBuf};

pub(crate) const LEGACY_EXECUTABLE: &str = "FXServer.exe";
pub(crate) const ENHANCED_EXECUTABLE: &str = "cfx-server.exe";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ServerEdition {
    Legacy,
    Enhanced,
}

impl ServerEdition {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Enhanced => "enhanced",
        }
    }
    pub(crate) fn executable(self) -> &'static str {
        match self {
            Self::Legacy => LEGACY_EXECUTABLE,
            Self::Enhanced => ENHANCED_EXECUTABLE,
        }
    }
}

pub(crate) fn find_server_executable(
    dir: &Path,
) -> Result<Option<(PathBuf, ServerEdition)>, String> {
    let legacy = dir.join(LEGACY_EXECUTABLE);
    let enhanced = dir.join(ENHANCED_EXECUTABLE);
    match (legacy.is_file(), enhanced.is_file()) {
        (true, true) => Err("This folder contains both FXServer.exe and cfx-server.exe. Use separate Legacy and Enhanced artifact folders.".into()),
        (true, false) => Ok(Some((legacy, ServerEdition::Legacy))),
        (false, true) => Ok(Some((enhanced, ServerEdition::Enhanced))),
        _ => Ok(None),
    }
}

pub(crate) fn require_server_executable(dir: &Path) -> Result<PathBuf, String> {
    find_server_executable(dir)?.map(|(path, _)| path).ok_or_else(||
        "No server executable found. Choose a folder containing FXServer.exe (Legacy) or cfx-server.exe (Enhanced).".into())
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn is_server_process_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(LEGACY_EXECUTABLE) || name.eq_ignore_ascii_case(ENHANCED_EXECUTABLE)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editions_and_conflicts() {
        let dir = std::env::temp_dir().join(format!(
            "fxi-editions-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        assert!(find_server_executable(&dir).unwrap().is_none());
        for edition in [ServerEdition::Legacy, ServerEdition::Enhanced] {
            std::fs::write(dir.join(edition.executable()), "fixture").unwrap();
            assert_eq!(find_server_executable(&dir).unwrap().unwrap().1, edition);
            std::fs::remove_file(dir.join(edition.executable())).unwrap();
        }
        std::fs::write(dir.join(LEGACY_EXECUTABLE), "fixture").unwrap();
        std::fs::write(dir.join(ENHANCED_EXECUTABLE), "fixture").unwrap();
        assert!(require_server_executable(&dir)
            .unwrap_err()
            .contains("both"));
        std::fs::remove_dir_all(dir).unwrap();
        assert!(is_server_process_name("CFX-SERVER.EXE"));
        assert!(is_server_process_name("fxserver.exe"));
        assert!(!is_server_process_name("node.exe"));
    }
}
