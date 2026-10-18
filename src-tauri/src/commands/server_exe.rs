//! Edition detection adapted from Huntercorlett's fork, commit 53f1834.
//! Mixed installations are rejected instead of preferring either executable.

use std::path::{Path, PathBuf};

#[cfg(not(target_os = "linux"))]
pub(crate) const LEGACY_EXECUTABLE: &str = "FXServer.exe";
#[cfg(not(target_os = "linux"))]
pub(crate) const ENHANCED_EXECUTABLE: &str = "cfx-server.exe";
#[cfg(target_os = "linux")]
pub(crate) const LEGACY_EXECUTABLE: &str = "alpine/opt/cfx-server/FXServer";
#[cfg(target_os = "linux")]
pub(crate) const ENHANCED_EXECUTABLE: &str = "alpine/opt/cfx-server/cfx-server";
#[cfg(any(target_os = "linux", test))]
pub(crate) const LINUX_LAUNCHER: &str = "run.sh";

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
    #[cfg(target_os = "linux")]
    return find_linux_launcher(dir);
    #[cfg(not(target_os = "linux"))]
    find_windows_executable(dir)
}

#[cfg(any(not(target_os = "linux"), test))]
fn find_windows_executable(dir: &Path) -> Result<Option<(PathBuf, ServerEdition)>, String> {
    let legacy = dir.join("FXServer.exe");
    let enhanced = dir.join("cfx-server.exe");
    match (legacy.is_file(), enhanced.is_file()) {
        (true, true) => Err("This folder contains both FXServer.exe and cfx-server.exe. Use separate Legacy and Enhanced artifact folders.".into()),
        (true, false) => Ok(Some((legacy, ServerEdition::Legacy))),
        (false, true) => Ok(Some((enhanced, ServerEdition::Enhanced))),
        _ => Ok(None),
    }
}

// Cfx documents Linux Legacy and Enhanced in its vanilla-server installation guide.
// Always launch the packaged script: executing the ELF directly skips its bundled loader.
#[cfg(any(target_os = "linux", test))]
fn find_linux_launcher(dir: &Path) -> Result<Option<(PathBuf, ServerEdition)>, String> {
    let legacy = dir.join("alpine/opt/cfx-server/FXServer").is_file();
    let enhanced = dir.join("alpine/opt/cfx-server/cfx-server").is_file();
    let edition = match (legacy, enhanced) {
        (true, true) => return Err("This folder contains both Linux Legacy and Enhanced server binaries. Use separate artifact folders.".into()),
        (true, false) => ServerEdition::Legacy,
        (false, true) => ServerEdition::Enhanced,
        _ => return Ok(None),
    };
    let launcher = dir.join(LINUX_LAUNCHER);
    if !launcher.is_file() {
        return Err("The Linux artifact is incomplete: run.sh is missing. Extract the complete official archive.".into());
    }
    Ok(Some((launcher, edition)))
}

pub(crate) fn require_server_executable(dir: &Path) -> Result<PathBuf, String> {
    find_server_executable(dir)?.map(|(path, _)| path).ok_or_else(|| {
        if cfg!(target_os = "linux") {
            "No Linux server found. Choose a folder containing run.sh and alpine/opt/cfx-server/FXServer (Legacy) or cfx-server (Enhanced)."
        } else {
            "No server executable found. Choose a folder containing FXServer.exe (Legacy) or cfx-server.exe (Enhanced)."
        }.into()
    })
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn is_server_process_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("FXServer.exe") || name.eq_ignore_ascii_case("cfx-server.exe")
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
        assert!(find_windows_executable(&dir).unwrap().is_none());
        for edition in [ServerEdition::Legacy, ServerEdition::Enhanced] {
            let name = match edition {
                ServerEdition::Legacy => "FXServer.exe",
                ServerEdition::Enhanced => "cfx-server.exe",
            };
            std::fs::write(dir.join(name), "fixture").unwrap();
            assert_eq!(find_windows_executable(&dir).unwrap().unwrap().1, edition);
            std::fs::remove_file(dir.join(name)).unwrap();
        }
        std::fs::write(dir.join("FXServer.exe"), "fixture").unwrap();
        std::fs::write(dir.join("cfx-server.exe"), "fixture").unwrap();
        assert!(find_windows_executable(&dir).unwrap_err().contains("both"));
        assert!(find_linux_launcher(&dir).unwrap().is_none());
        std::fs::create_dir_all(dir.join("alpine/opt/cfx-server")).unwrap();
        let legacy = dir.join("alpine/opt/cfx-server/FXServer");
        let enhanced = dir.join("alpine/opt/cfx-server/cfx-server");
        std::fs::write(&legacy, "fixture").unwrap();
        assert!(find_linux_launcher(&dir).unwrap_err().contains("run.sh"));
        std::fs::write(dir.join(LINUX_LAUNCHER), "fixture").unwrap();
        assert_eq!(
            find_linux_launcher(&dir).unwrap(),
            Some((dir.join(LINUX_LAUNCHER), ServerEdition::Legacy))
        );
        std::fs::write(&enhanced, "fixture").unwrap();
        assert!(find_linux_launcher(&dir).unwrap_err().contains("both"));
        std::fs::remove_file(legacy).unwrap();
        assert_eq!(
            find_linux_launcher(&dir).unwrap().unwrap().1,
            ServerEdition::Enhanced
        );
        std::fs::remove_dir_all(dir).unwrap();
        assert!(is_server_process_name("CFX-SERVER.EXE"));
        assert!(is_server_process_name("fxserver.exe"));
        assert!(!is_server_process_name("node.exe"));
    }
}
