use std::process::Command;

use crate::models::mariadb::MariaDBInstallOptions;

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
        Ok(if stdout.is_empty() {
            "MariaDB installation completed.".to_string()
        } else {
            stdout
        })
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
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
