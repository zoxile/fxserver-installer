pub use super::package::{list_releases, list_series};
use super::{
    detect,
    linux::{
        plans::{self, Action, Distro},
        runtime,
    },
    package,
};
use crate::models::mariadb::{MariaDBInstallOptions, MariaDBPackageInfo};

pub fn install_mariadb(
    options: MariaDBInstallOptions,
    report: &dyn Fn(&str),
) -> Result<String, String> {
    validate_options(&options)?;
    report("Reading signed distro package metadata; repository configuration will not be changed.");
    detect::clear_detection_cache();
    let catalog = package::catalog()?;
    if catalog.installed.is_some() || detect::detect_mariadb().installed {
        return Err("MariaDB is already installed. Use Update; installation will not replace an existing server.".into());
    }
    let version = catalog.select(options.version.as_deref())?;
    // Do not reconnect unknown preserved data to a potentially incompatible server version.
    match std::fs::read_dir("/var/lib/mysql") {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err("Existing /var/lib/mysql data was found. Reinstall through your distro administrator after verifying data-version compatibility; authentication will not be reset.".into());
            }
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err("Cannot inspect /var/lib/mysql for preserved data. Installation refused; use your distro administrator.".into()),
        Err(_) => {},
    }
    report("Installing distro MariaDB packages. Approve only this package action when prompted.");
    mutate(&catalog, Action::Install, Some(&version))?;
    verify_version(catalog.distro, &version)?;
    let note = if catalog.distro == Distro::Arch {
        " Arch requires an administrator to initialize a NEW database with mariadb-install-db before starting the service; this app does not initialize or overwrite data directories."
    } else {
        " The distro package scripts may initialize or start the service."
    };
    Ok(format!("MariaDB package {version} installed.{note} Authentication and configuration remain under distro control. No root password was set or validated. Configure a dedicated database login before connecting; socket-only root authentication is not a TCP password login."))
}

fn validate_options(options: &MariaDBInstallOptions) -> Result<(), String> {
    if !options.root_password.is_empty() {
        return Err("Linux distro packages preserve database authentication. Leave root_password empty; configure a dedicated login separately. An installer password is never applied to existing data.".into());
    }
    let has_value = |value: &Option<String>| value.as_deref().is_some_and(|s| !s.trim().is_empty());
    if !options.service_name.eq_ignore_ascii_case("mariadb")
        && options.service_name != "mariadb.service"
        || options.port != 3306
        || has_value(&options.install_dir)
        || has_value(&options.data_dir)
        || options.allow_remote_root_access
        || options.create_anonymous_user
        || options.skip_networking
        || options.optimize_for_transactions
        || options.use_utf8
        || has_value(&options.page_size)
        || has_value(&options.buffer_pool_size)
        || options.install_heidi_sql
        || options.install_development_files
    {
        return Err("Linux installation supports distro defaults only: mariadb service, port 3306, no custom directories, tuning, account changes, or optional Windows components. Configure these externally; no settings were applied.".into());
    }
    Ok(())
}

fn mutate(catalog: &package::Catalog, action: Action, version: Option<&str>) -> Result<(), String> {
    let plan = plans::package_plan(catalog.distro, action, version, &catalog.repos)?;
    detect::clear_detection_cache();
    let result = runtime::administer(&plan);
    detect::clear_detection_cache();
    result
}

fn verify_version(distro: Distro, expected: &str) -> Result<(), String> {
    let actual = package::installed_version(distro)?;
    if actual.as_deref().map(normalized_epoch) != Some(normalized_epoch(expected)) {
        return Err("The package command completed but the requested installed version was not observed. Inspect your package manager before retrying; no rollback was attempted.".into());
    }
    Ok(())
}

fn normalized_epoch(version: &str) -> &str {
    version.strip_prefix("0:").unwrap_or(version)
}

pub fn uninstall_mariadb(report: &dyn Fn(&str)) -> Result<String, String> {
    let catalog = package::catalog()?;
    if catalog.installed.is_none() {
        return Err("No distro-managed MariaDB server package is installed. Unmanaged installations are not removed.".into());
    }
    report("Removing the distro server package without purge, autoremove, or data-directory deletion. Package scripts may stop MariaDB.");
    mutate(&catalog, Action::Remove, None)?;
    if package::installed_version(catalog.distro)?.is_some() {
        return Err("The package manager exited but the server package is still installed. Inspect it before retrying.".into());
    }
    if detect::detect_mariadb().installed {
        return Err("The selected package was removed, but a MariaDB server is still detected. Older distro metapackages or another installation may retain server binaries; manage those explicitly with your distro administrator. No data deletion or autoremove was attempted.".into());
    }
    Ok("MariaDB server package removed. No database files or database accounts were deleted; client packages and dependencies were retained unless the distro transaction required otherwise.".into())
}

pub fn update_mariadb(report: &dyn Fn(&str)) -> Result<String, String> {
    let catalog = package::catalog()?;
    let installed = catalog
        .installed
        .as_deref()
        .ok_or("A distro-managed MariaDB server is required for updates.")?;
    let target = catalog.select(None)?;
    if normalized_epoch(installed) == normalized_epoch(&target) {
        return Ok(format!(
            "MariaDB {installed} is current in the cached distro repository."
        ));
    }
    if plans::series(installed) != plans::series(&target) {
        return Err("The distro candidate changes MariaDB series. Back up and perform the distro's documented migration externally; no automatic major upgrade was attempted.".into());
    }
    if !package::is_upgrade(&catalog, &target)? {
        return Err("The distro candidate is not a verified upgrade. No downgrade or reinstall was attempted.".into());
    }
    report("Updating MariaDB within its installed series. Package scripts may restart the service; database authentication is preserved.");
    mutate(&catalog, Action::Update, Some(&target))?;
    verify_version(catalog.distro, &target)?;
    Ok(format!(
        "MariaDB updated to distro package {target}. Authentication was not changed or validated."
    ))
}

pub fn get_package_info() -> MariaDBPackageInfo {
    let installed_fallback = package::distro()
        .and_then(package::installed_version)
        .ok()
        .flatten();
    match package::catalog() {
        Ok(catalog) => {
            let selected = catalog.select(None);
            let latest_version = selected.as_ref().ok().cloned();
            let compatible = catalog
                .installed
                .as_ref()
                .zip(latest_version.as_ref())
                .is_some_and(|(old, new)| plans::series(old) == plans::series(new));
            let upgrade = if compatible {
                package::is_upgrade(&catalog, latest_version.as_deref().unwrap())
            } else {
                Ok(false)
            };
            let error = selected.err().or_else(|| upgrade.as_ref().err().cloned()).or_else(|| {
                (catalog.installed.is_some() && latest_version.is_some() && !compatible).then(|| "Distro candidate changes MariaDB series; an external migration is required.".into())
            });
            MariaDBPackageInfo {
                latest_version,
                installed_package_version: catalog.installed,
                update_available: upgrade.unwrap_or(false),
                error,
            }
        }
        Err(error) => MariaDBPackageInfo {
            latest_version: None,
            installed_package_version: installed_fallback,
            update_available: false,
            error: Some(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options() -> MariaDBInstallOptions {
        MariaDBInstallOptions {
            version: None,
            root_password: String::new(),
            service_name: "mariadb".into(),
            port: 3306,
            install_dir: None,
            data_dir: None,
            allow_remote_root_access: false,
            create_anonymous_user: false,
            skip_networking: false,
            optimize_for_transactions: false,
            use_utf8: false,
            page_size: None,
            buffer_pool_size: None,
            install_heidi_sql: false,
            install_development_files: false,
        }
    }
    #[test]
    fn installer_never_promises_to_apply_a_password_or_windows_settings() {
        assert!(validate_options(&options()).is_ok());
        let mut opts = options();
        opts.root_password = "secret".into();
        assert!(validate_options(&opts).is_err());
        opts = options();
        opts.data_dir = Some("/existing/data".into());
        assert!(validate_options(&opts).is_err());
        opts = options();
        opts.allow_remote_root_access = true;
        assert!(validate_options(&opts).is_err());
    }
}
