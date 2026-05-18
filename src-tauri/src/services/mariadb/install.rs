use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::{models::mariadb::MariaDBInstallOptions, services::mariadb::detect::detect_mariadb};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(20 * 60);

pub fn install_mariadb(options: MariaDBInstallOptions) -> Result<String, String> {
    let override_args = build_msi_overrides(options)?;
    let output = run_winget_install(&override_args, INSTALL_TIMEOUT)?;

    if output.success {
        let installer_message = if output.stdout.is_empty() {
            "MariaDB installation completed.".to_string()
        } else {
            output.stdout
        };
        if let Some(detected_message) = wait_for_install_detection(Duration::from_secs(45)) {
            Ok(format!("{installer_message}\n{detected_message}"))
        } else {
            Err(format!("{installer_message}\nWinget reported success, but MariaDB was not detected after the installer exited. Windows Installer may still be running in the background; close any stuck msiexec/MariaDB installer process and try again."))
        }
    } else {
        Err(if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        })
    }
}

struct InstallOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

fn run_winget_install(override_args: &str, timeout: Duration) -> Result<InstallOutput, String> {
    let mut child = Command::new("winget")
        .args([
            "install",
            "--id",
            "MariaDB.Server",
            "-e",
            "--silent",
            "--source",
            "winget",
            "--disable-interactivity",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--override",
            override_args,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start winget: {error}"))?;

    let started = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("Failed to wait for winget: {error}"))?
        {
            let mut stdout = String::new();
            let mut stderr = String::new();
            if let Some(mut stream) = child.stdout.take() {
                let _ = stream.read_to_string(&mut stdout);
            }
            if let Some(mut stream) = child.stderr.take() {
                let _ = stream.read_to_string(&mut stderr);
            }

            return Ok(InstallOutput {
                success: status.success(),
                stdout: stdout.trim().to_string(),
                stderr: stderr.trim().to_string(),
            });
        }

        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "MariaDB installer did not finish within {} minutes. Windows Installer may still be waiting for an elevated msiexec prompt; close any MariaDB installer windows and try again.",
                timeout.as_secs() / 60
            ));
        }

        thread::sleep(Duration::from_millis(500));
    }
}

fn wait_for_install_detection(timeout: Duration) -> Option<String> {
    let started = Instant::now();

    while started.elapsed() < timeout {
        let status = detect_mariadb();
        if status.installed {
            return Some(format!(
                "MariaDB detected{}{}.",
                status
                    .version
                    .as_ref()
                    .map(|version| format!(": {version}"))
                    .unwrap_or_default(),
                status
                    .service_name
                    .as_ref()
                    .map(|service_name| format!(" ({service_name})"))
                    .unwrap_or_default()
            ));
        }

        thread::sleep(Duration::from_secs(2));
    }

    None
}

fn build_msi_overrides(options: MariaDBInstallOptions) -> Result<String, String> {
    if options.root_password.trim().is_empty() {
        return Err("Root password is required for a configured MariaDB install.".to_string());
    }

    if options.service_name.trim().is_empty() {
        return Err("Service name is required.".to_string());
    }

    let mut properties = vec![
        "/qn".to_string(),
        "/norestart".to_string(),
        property("PASSWORD", &options.root_password),
        property("SERVICENAME", &options.service_name),
        property("PORT", &options.port.to_string()),
    ];

    push_property_bool(
        &mut properties,
        "ALLOWREMOTEROOTACCESS",
        options.allow_remote_root_access,
    );
    push_property_bool(
        &mut properties,
        "DEFAULTUSER",
        options.create_anonymous_user,
    );
    push_property_bool(&mut properties, "SKIPNETWORKING", options.skip_networking);
    push_property_bool(
        &mut properties,
        "STDCONFIG",
        options.optimize_for_transactions,
    );
    push_property_bool(&mut properties, "UTF8", options.use_utf8);

    if let Some(value) = options.install_dir.filter(|value| !value.trim().is_empty()) {
        properties.push(property("INSTALLDIR", &value));
    }

    if let Some(value) = options.data_dir.filter(|value| !value.trim().is_empty()) {
        properties.push(property("DATADIR", &value));
    }

    if let Some(value) = options.page_size.filter(|value| !value.trim().is_empty()) {
        properties.push(property("PAGESIZE", &value));
    }

    if let Some(value) = options
        .buffer_pool_size
        .filter(|value| !value.trim().is_empty())
    {
        properties.push(property("BUFFERPOOLSIZE", &value));
    }

    let mut remove_features = Vec::new();
    if !options.install_heidi_sql {
        remove_features.push("HeidiSQL");
    }
    if !options.install_development_files {
        remove_features.push("DEVEL");
    }
    if !remove_features.is_empty() {
        properties.push(property("REMOVE", &remove_features.join(",")));
    }

    Ok(properties.join(" "))
}

fn property(name: &str, value: &str) -> String {
    let escaped = value.replace('"', "\\\"");
    format!("{name}=\"{escaped}\"")
}

fn push_property_bool(properties: &mut Vec<String>, name: &str, enabled: bool) {
    if enabled {
        properties.push(format!("{name}=1"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> MariaDBInstallOptions {
        MariaDBInstallOptions {
            root_password: "secret".to_string(),
            service_name: "MariaDB".to_string(),
            port: 3306,
            install_dir: None,
            data_dir: None,
            allow_remote_root_access: false,
            create_anonymous_user: false,
            skip_networking: true,
            optimize_for_transactions: true,
            use_utf8: true,
            page_size: None,
            buffer_pool_size: None,
            install_heidi_sql: false,
            install_development_files: false,
        }
    }

    #[test]
    fn msi_override_keeps_install_silent() {
        let overrides = build_msi_overrides(options()).expect("valid overrides");

        assert!(overrides.contains("/qn"));
        assert!(overrides.contains("/norestart"));
        assert!(overrides.contains("PASSWORD=\"secret\""));
        assert!(overrides.contains("SKIPNETWORKING=1"));
        assert!(overrides.contains("STDCONFIG=1"));
        assert!(overrides.contains("UTF8=1"));
        assert!(!overrides.contains("ALLOWREMOTEROOTACCESS"));
        assert!(!overrides.contains("DEFAULTUSER"));
        assert!(overrides.contains("REMOVE=\"HeidiSQL,DEVEL\""));
    }
}
