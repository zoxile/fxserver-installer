use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};

use crate::{models::mariadb::MariaDBInstallOptions, services::mariadb::detect::detect_mariadb};

pub fn install_mariadb(options: MariaDBInstallOptions) -> Result<String, String> {
    let override_args = build_msi_overrides(options)?;
    let output = Command::new("winget")
        .args([
            "install",
            "--id",
            "MariaDB.Server",
            "-e",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--override",
            &override_args,
        ])
        .output()
        .map_err(|error| format!("Failed to start winget: {error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        let installer_message = if stdout.is_empty() {
            "MariaDB installation completed.".to_string()
        } else {
            stdout
        };
        Ok(match wait_for_install_detection(Duration::from_secs(45)) {
            Some(detected_message) => format!("{installer_message}\n{detected_message}"),
            None => format!("{installer_message}\nMariaDB installer exited successfully, but the app could not detect the service yet. Refresh status after Windows finishes registering the service."),
        })
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr })
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
        property("PASSWORD", &options.root_password),
        property("SERVICENAME", &options.service_name),
        property("PORT", &options.port.to_string()),
        property_bool("ALLOWREMOTEROOTACCESS", options.allow_remote_root_access),
        property_bool("DEFAULTUSER", options.create_anonymous_user),
        property_bool("SKIPNETWORKING", options.skip_networking),
        property_bool("STDCONFIG", options.optimize_for_transactions),
        property_bool("UTF8", options.use_utf8),
    ];

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

fn property_bool(name: &str, enabled: bool) -> String {
    if enabled {
        format!("{name}=1")
    } else {
        format!("{name}=\"\"")
    }
}
