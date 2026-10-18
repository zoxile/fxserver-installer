use super::linux::{
    plans::{service_unit, Plan},
    runtime,
};
use crate::models::mariadb::MariaDBStatus;
use std::{
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

pub fn clear_detection_cache() {
    super::query::clear_client_cache();
}

pub(super) fn find_binary(names: &[&str]) -> Option<PathBuf> {
    let mut directories: Vec<_> = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|p| p.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    directories.extend(
        [
            "/usr/bin",
            "/usr/sbin",
            "/bin",
            "/sbin",
            "/usr/local/bin",
            "/usr/local/sbin",
        ]
        .map(PathBuf::from),
    );
    for name in names {
        for directory in &directories {
            let path = directory.join(name);
            if !path
                .metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            {
                continue;
            }
            let Ok(path) = path.canonicalize() else {
                continue;
            };
            if binary_version(&path).is_some() {
                return Some(path);
            }
        }
    }
    None
}

pub(super) fn binary_version(path: &std::path::Path) -> Option<String> {
    let output = runtime::run(
        Command::new(path)
            .args(["--no-defaults", "--version"])
            .stdin(Stdio::null()),
        Some(Duration::from_secs(2)),
    )
    .ok()?;
    (output.success && output.stdout.to_ascii_lowercase().contains("mariadb"))
        .then_some(output.stdout)
}

pub fn get_install_path() -> Option<String> {
    let binary = find_binary(&["mariadbd", "mysqld", "mariadb", "mysql"])?;
    let directory = binary.parent()?;
    let prefix = if matches!(directory.file_name()?.to_str(), Some("bin" | "sbin")) {
        directory.parent()?
    } else {
        directory
    };
    Some(prefix.to_string_lossy().into_owned())
}

pub(super) fn server_version() -> Option<String> {
    binary_version(&find_binary(&["mariadbd", "mysqld"])?)
}

pub(super) fn identified_service(name: &str) -> bool {
    let Ok(unit) = service_unit(name) else {
        return false;
    };
    let Ok(output) = runtime::read(&Plan::new(
        "systemctl",
        &[
            "--system",
            "--no-pager",
            "show",
            "--property=LoadState,Id,ExecStart",
            "--",
            &unit,
        ],
    )) else {
        return false;
    };
    if !output.success || !output.stdout.lines().any(|l| l == "LoadState=loaded") {
        return false;
    }
    // mysql.service may be a MariaDB alias, but never control an unrelated MySQL server.
    output
        .stdout
        .lines()
        .filter_map(|line| line.strip_prefix("ExecStart="))
        .filter_map(|line| line.split("path=").nth(1))
        .filter_map(|line| line.split_whitespace().next())
        .map(|path| std::path::Path::new(path.trim_end_matches(';')))
        .any(|path| {
            path.is_absolute()
                && matches!(
                    path.file_name().and_then(|n| n.to_str()),
                    Some("mariadbd" | "mysqld")
                )
                && binary_version(path).is_some()
        })
}

pub fn find_service_name() -> Option<String> {
    ["mariadb.service", "mysql.service", "mysqld.service"]
        .into_iter()
        .find(|name| identified_service(name))
        .map(str::to_string)
}

pub fn is_service_running(name: &str) -> bool {
    let Ok(unit) = service_unit(name) else {
        return false;
    };
    runtime::read(&Plan::new(
        "systemctl",
        &[
            "--system",
            "--no-pager",
            "is-active",
            "--quiet",
            "--",
            &unit,
        ],
    ))
    .is_ok_and(|o| o.success)
}

pub fn detect_mariadb() -> MariaDBStatus {
    let service_name = find_service_name();
    let version = server_version();
    MariaDBStatus {
        installed: service_name.is_some() || version.is_some(),
        running: service_name.as_deref().is_some_and(is_service_running),
        version,
        service_display_name: service_name.as_ref().map(|_| "MariaDB (systemd)".into()),
        service_name,
        install_path: get_install_path(),
    }
}
