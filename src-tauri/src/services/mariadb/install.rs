use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    models::mariadb::{MariaDBCredentials, MariaDBInstallOptions, MariaDBPackageInfo},
    services::mariadb::{
        detect::{detect_mariadb, find_service_name},
        query::validate_connection,
    },
};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(20 * 60);

pub fn install_mariadb(options: MariaDBInstallOptions) -> Result<String, String> {
    let log_path = installer_log_path();
    let install_plan = build_install_plan(&options)?;
    let override_args = build_msi_overrides(&options, &install_plan, &log_path)?;
    let output = run_winget_install(&override_args, INSTALL_TIMEOUT, false)?;

    if output.success {
        let installer_message = if output.stdout.is_empty() {
            "MariaDB installation completed.".to_string()
        } else {
            output.stdout
        };
        let plan_message = match &install_plan {
            InstallPlan::Fresh => Some(initialize_fresh_database(&options)?),
            InstallPlan::Reattach {
                data_dir,
                install_dir,
            } => Some(reattach_preserved_data(&options, install_dir, data_dir)?),
        };

        if let Some(detected_message) = wait_for_install_detection(Duration::from_secs(45)) {
            let plan_message = plan_message
                .map(|message| format!("\n{message}"))
                .unwrap_or_default();
            let validation_message = install_validation_message(&options, &install_plan)?;
            Ok(format!(
                "{installer_message}{plan_message}{validation_message}\n{detected_message}\nInstaller log: {}",
                log_path.display()
            ))
        } else {
            Err(format!(
                "{installer_message}\nWinget reported success, but MariaDB was not detected after the installer exited. Windows Installer may still be running in the background; close any stuck msiexec/MariaDB installer process and try again.\nInstaller log: {}",
                log_path.display()
            ))
        }
    } else {
        let detail = if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        };
        Err(format!("{detail}\nInstaller log: {}", log_path.display()))
    }
}

pub fn get_package_info() -> MariaDBPackageInfo {
    let latest_version = winget_latest_version();
    let installed_package_version = registry_installed_package()
        .and_then(|package| package.version)
        .or_else(|| detect_mariadb().version);
    let update_available = match (&installed_package_version, &latest_version) {
        (Some(installed), Some(latest)) => compare_versions(installed, latest).is_lt(),
        _ => false,
    };

    MariaDBPackageInfo {
        latest_version,
        installed_package_version,
        update_available,
    }
}

pub fn uninstall_mariadb() -> Result<String, String> {
    let service_name = find_service_name().unwrap_or_else(|| "MariaDB".to_string());
    let package = match registry_installed_package() {
        Some(package) => package,
        None => {
            cleanup_mariadb_service(&service_name)?;
            return Ok(format!(
                "MariaDB MSI package was not installed, but the {service_name} service was removed. Data files were preserved."
            ));
        }
    };
    let heidisql_message =
        preserve_heidisql_before_uninstall(detect_heidisql(package.install_location.as_deref()))?;
    let product_code = package.product_code.ok_or_else(|| {
        "MariaDB product code was not found in Windows uninstall registry.".to_string()
    })?;
    let log_path = installer_log_path();
    let output =
        run_elevated_mariadb_uninstall(&product_code, &service_name, &log_path, INSTALL_TIMEOUT)?;

    if !output.success {
        return Err(format!(
            "{}\nInstaller log: {}",
            if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            },
            log_path.display()
        ));
    }

    if wait_for_uninstall_detection(Duration::from_secs(45)) {
        Ok(format!(
            "MariaDB uninstalled and the {service_name} service was removed. Data directory was preserved by passing CLEANUPDATA empty.{}\nInstaller log: {}",
            heidisql_message
                .map(|message| format!("\n{message}"))
                .unwrap_or_default(),
            log_path.display()
        ))
    } else {
        Err(format!(
            "MariaDB uninstall command exited, but MariaDB is still detected. Check the MSI log.\nInstaller log: {}",
            log_path.display()
        ))
    }
}

pub fn update_mariadb() -> Result<String, String> {
    let before = get_package_info().installed_package_version;
    let log_path = installer_log_path();
    let override_args = build_update_overrides(&log_path);
    let output = run_winget_install(&override_args, INSTALL_TIMEOUT, true)?;

    if !output.success {
        let detail = if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        };
        return Err(format!("{detail}\nInstaller log: {}", log_path.display()));
    }

    let after = wait_for_package_version_change(before.as_deref(), Duration::from_secs(60))
        .or_else(|| get_package_info().installed_package_version);

    Ok(format!(
        "{}\nDetected MariaDB version: {}\nInstaller log: {}",
        if output.stdout.is_empty() {
            "MariaDB update completed.".to_string()
        } else {
            output.stdout
        },
        after.unwrap_or_else(|| "Unknown".to_string()),
        log_path.display()
    ))
}

struct InstallOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

fn run_winget_install(
    override_args: &str,
    timeout: Duration,
    force: bool,
) -> Result<InstallOutput, String> {
    let mut args = vec![
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
    ];
    if force {
        args.push("--force");
    }

    run_process("winget", &args, timeout)
}

fn run_process(command: &str, args: &[&str], timeout: Duration) -> Result<InstallOutput, String> {
    let mut child = Command::new(command)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start {command}: {error}"))?;

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

fn run_elevated_mariadb_uninstall(
    product_code: &str,
    service_name: &str,
    log_path: &Path,
    timeout: Duration,
) -> Result<InstallOutput, String> {
    let escaped_product_code = product_code.replace('\'', "''");
    let escaped_service_name = service_name.replace('\'', "''");
    let escaped_log_path = log_path.to_string_lossy().replace('\'', "''");
    let script = format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
$productCode = '{escaped_product_code}'
$serviceName = '{escaped_service_name}'
$logPath = '{escaped_log_path}'
$process = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/x', $productCode, '/qn', '/norestart', 'CLEANUPDATA=""', '/l*v', $logPath) -Wait -PassThru
$exitCode = $process.ExitCode
$service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if ($service) {{
    if ($service.Status -ne 'Stopped') {{
        Stop-Service -Name $serviceName -Force -ErrorAction SilentlyContinue
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
    }}
    & sc.exe delete $serviceName | Out-Null
}}
exit $exitCode
"#
    );

    run_elevated_powershell_script("mariadb-uninstall", &script, timeout)
}

fn run_elevated_powershell_script(
    script_name: &str,
    script: &str,
    timeout: Duration,
) -> Result<InstallOutput, String> {
    let script_path = env::temp_dir().join(format!(
        "fxserver-installer-{script_name}-{}.ps1",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0)
    ));
    fs::write(&script_path, script).map_err(|error| {
        format!(
            "Failed to prepare elevated PowerShell script {}: {error}",
            script_path.display()
        )
    })?;

    let output = run_script_via_elevated_app(&script_path, timeout);
    let _ = fs::remove_file(&script_path);
    output
}

fn run_script_via_elevated_app(
    script_path: &Path,
    timeout: Duration,
) -> Result<InstallOutput, String> {
    let app_path = env::current_exe()
        .map_err(|error| format!("Failed to resolve app executable for elevation: {error}"))?;
    let app_path = app_path.to_string_lossy().replace('\'', "''");
    let script_path = script_path.to_string_lossy().replace('\'', "''");
    let helper_arg = crate::ELEVATED_SCRIPT_ARG.replace('\'', "''");
    let command = format!(
        "$process = Start-Process -FilePath '{app_path}' -ArgumentList @('{helper_arg}', '{script_path}') -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $process.ExitCode"
    );

    run_process("powershell", &["-NoProfile", "-Command", &command], timeout)
}

fn wait_for_install_detection(timeout: Duration) -> Option<String> {
    let started = Instant::now();

    while started.elapsed() < timeout {
        let status = detect_mariadb();
        if status.installed && status.service_name.is_some() {
            if !status.running {
                return Some(format!(
                    "MariaDB detected{}{}, but the service is not running.",
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

fn wait_for_uninstall_detection(timeout: Duration) -> bool {
    let started = Instant::now();

    while started.elapsed() < timeout {
        if !detect_mariadb().installed {
            return true;
        }

        thread::sleep(Duration::from_secs(2));
    }

    false
}

fn cleanup_mariadb_service(service_name: &str) -> Result<(), String> {
    let escaped_service_name = service_name.replace('\'', "''");
    let script = format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
$serviceName = '{escaped_service_name}'
$service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if ($service) {{
    if ($service.Status -ne 'Stopped') {{
        Stop-Service -Name $serviceName -Force -ErrorAction SilentlyContinue
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
    }}
    & sc.exe delete $serviceName | Out-Null
}}
exit 0
"#
    );

    let output = run_elevated_powershell_script(
        "mariadb-service-cleanup",
        &script,
        Duration::from_secs(90),
    )?;
    if output.success {
        Ok(())
    } else {
        Err(format!(
            "MariaDB package was removed, but the {service_name} service could not be removed: {}",
            if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            }
        ))
    }
}

fn detect_heidisql(mariadb_install_location: Option<&str>) -> bool {
    if registry_heidisql_installed() {
        return true;
    }

    mariadb_install_location
        .map(|location| {
            Path::new(location).join("heidisql.exe").exists()
                || Path::new(location)
                    .join("bin")
                    .join("heidisql.exe")
                    .exists()
        })
        .unwrap_or(false)
}

fn registry_heidisql_installed() -> bool {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "$paths = 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*','HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*','HKCU:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*'; @(Get-ItemProperty $paths -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like 'HeidiSQL*' }).Count",
        ])
        .output();

    output
        .ok()
        .and_then(|result| {
            if result.status.success() {
                String::from_utf8_lossy(&result.stdout)
                    .trim()
                    .parse::<usize>()
                    .ok()
            } else {
                None
            }
        })
        .is_some_and(|count| count > 0)
}

fn preserve_heidisql_before_uninstall(was_present: bool) -> Result<Option<String>, String> {
    if !was_present || registry_heidisql_installed() {
        return Ok(None);
    }

    let output = run_process(
        "winget",
        &[
            "install",
            "--id",
            "HeidiSQL.HeidiSQL",
            "-e",
            "--silent",
            "--source",
            "winget",
            "--disable-interactivity",
            "--accept-package-agreements",
            "--accept-source-agreements",
        ],
        INSTALL_TIMEOUT,
    )?;

    if output.success || registry_heidisql_installed() {
        Ok(Some(
            "HeidiSQL was bundled with MariaDB and was made standalone before uninstall."
                .to_string(),
        ))
    } else {
        Err(format!(
            "HeidiSQL appears to be bundled with MariaDB and could not be made standalone, so MariaDB was not uninstalled: {}",
            if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            }
        ))
    }
}

fn wait_for_package_version_change(previous: Option<&str>, timeout: Duration) -> Option<String> {
    let started = Instant::now();

    while started.elapsed() < timeout {
        let current = get_package_info().installed_package_version;
        if current
            .as_deref()
            .is_some_and(|version| previous != Some(version))
        {
            return current;
        }

        thread::sleep(Duration::from_secs(2));
    }

    None
}

fn installer_log_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    env::temp_dir().join(format!("fxserver-installer-mariadb-{timestamp}.log"))
}

enum InstallPlan {
    Fresh,
    Reattach {
        data_dir: PathBuf,
        install_dir: PathBuf,
    },
}

fn build_install_plan(options: &MariaDBInstallOptions) -> Result<InstallPlan, String> {
    let Some(data_dir) = install_data_dir_candidates(options)
        .into_iter()
        .find(|path| path.exists() && !is_directory_empty(path).unwrap_or(false))
    else {
        return Ok(InstallPlan::Fresh);
    };

    let install_dir = preserved_install_dir(options, &data_dir)?;
    Ok(InstallPlan::Reattach {
        data_dir,
        install_dir,
    })
}

fn build_msi_overrides(
    options: &MariaDBInstallOptions,
    install_plan: &InstallPlan,
    log_path: &Path,
) -> Result<String, String> {
    if options.root_password.trim().is_empty() {
        return Err("Root password is required for a configured MariaDB install.".to_string());
    }

    if options.service_name.trim().is_empty() {
        return Err("Service name is required.".to_string());
    }

    let mut properties = vec![
        "/qn".to_string(),
        "/norestart".to_string(),
        "/l*v".to_string(),
        quote_arg(&log_path.to_string_lossy()),
        format!(
            "ADDLOCAL={}",
            selected_features(options, install_plan).join(",")
        ),
    ];

    if matches!(install_plan, InstallPlan::Fresh) {
        push_property_bool(
            &mut properties,
            "STDCONFIG",
            options.optimize_for_transactions,
        );
        push_property_bool(&mut properties, "UTF8", options.use_utf8);
    } else {
        push_property_bool(
            &mut properties,
            "STDCONFIG",
            options.optimize_for_transactions,
        );
        push_property_bool(&mut properties, "UTF8", options.use_utf8);
    }

    match install_plan {
        InstallPlan::Fresh => {
            if let Some(value) = options
                .install_dir
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                properties.push(property("INSTALLDIR", value));
            }
        }
        InstallPlan::Reattach { install_dir, .. } => {
            properties.push(property("INSTALLDIR", &install_dir.to_string_lossy()));
        }
    }

    if matches!(install_plan, InstallPlan::Fresh) {
        if let Some(value) = options
            .data_dir
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            properties.push(property("DATADIR", value));
        }
    }

    if let Some(value) = options
        .page_size
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        properties.push(property("PAGESIZE", value));
    }

    if let Some(value) = options
        .buffer_pool_size
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        properties.push(property("BUFFERPOOLSIZE", value));
    }

    Ok(properties.join(" "))
}

fn initialize_fresh_database(options: &MariaDBInstallOptions) -> Result<String, String> {
    let install_dir = options
        .install_dir
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            registry_installed_package()
                .and_then(|package| package.install_location)
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\MariaDB"));
    let install_dir = resolve_actual_install_dir(&install_dir)?;
    let install_db_path = install_dir.join("bin").join("mariadb-install-db.exe");
    if !install_db_path.exists() {
        return Err(format!(
            "MariaDB binaries installed, but mariadb-install-db.exe was not found at {}.",
            install_db_path.display()
        ));
    }

    let data_dir = options
        .data_dir
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| install_dir.join("data"));
    let config_template = write_fresh_config_template(options, &install_dir, &data_dir)?;
    let output =
        run_elevated_fresh_database_init(options, &install_db_path, &data_dir, &config_template)?;
    let _ = fs::remove_file(&config_template);

    if !output.success {
        return Err(format!(
            "MariaDB binaries installed, but database initialization failed: {}",
            if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            }
        ));
    }
    if !wait_for_service_running(&options.service_name, Duration::from_secs(45)) {
        return Err(format!(
            "MariaDB database was initialized, but {service_name} did not reach Running state.",
            service_name = options.service_name
        ));
    }

    Ok(format!(
        "Fresh database was initialized at {} with service {}.",
        data_dir.display(),
        options.service_name
    ))
}

fn write_fresh_config_template(
    options: &MariaDBInstallOptions,
    install_dir: &Path,
    data_dir: &Path,
) -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("System clock error: {error}"))?
        .as_millis();
    let path = env::temp_dir().join(format!("fxserver-mariadb-template-{timestamp}.ini"));
    let content = rewrite_my_ini("", options, install_dir, data_dir);
    fs::write(&path, content)
        .map_err(|error| format!("Failed to write MariaDB config template: {error}"))?;
    Ok(path)
}

fn run_elevated_fresh_database_init(
    options: &MariaDBInstallOptions,
    install_db_path: &Path,
    data_dir: &Path,
    config_template: &Path,
) -> Result<InstallOutput, String> {
    let escaped_install_db_path = install_db_path.to_string_lossy().replace('\'', "''");
    let escaped_data_dir = data_dir.to_string_lossy().replace('\'', "''");
    let escaped_config_template = config_template.to_string_lossy().replace('\'', "''");
    let escaped_service_name = options.service_name.replace('\'', "''");
    let escaped_password = options.root_password.replace('\'', "''");
    let mut flags = Vec::new();
    if options.allow_remote_root_access {
        flags.push("'--allow-remote-root-access'".to_string());
    }
    if options.create_anonymous_user {
        flags.push("'--default-user'".to_string());
    }
    if options.skip_networking {
        flags.push("'--skip-networking'".to_string());
    }
    if let Some(page_size) = options
        .page_size
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        flags.push(format!(
            "'--innodb-page-size={}'",
            page_size.replace('\'', "''")
        ));
    }
    let flags = if flags.is_empty() {
        String::new()
    } else {
        format!(", {}", flags.join(", "))
    };
    let script = format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
$installDbPath = '{escaped_install_db_path}'
$dataDir = '{escaped_data_dir}'
$serviceName = '{escaped_service_name}'
$configTemplate = '{escaped_config_template}'
$service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if ($service) {{
    if ($service.Status -ne 'Stopped') {{
        Stop-Service -Name $serviceName -Force -ErrorAction SilentlyContinue
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
    }}
    & sc.exe delete $serviceName | Out-Null
    Start-Sleep -Seconds 2
}}
if ((Test-Path -LiteralPath $dataDir) -and @(Get-ChildItem -LiteralPath $dataDir -Force -ErrorAction SilentlyContinue).Count -gt 0) {{
    Write-Error "Data directory is not empty: $dataDir"
    exit 71
}}
New-Item -ItemType Directory -Path $dataDir -Force | Out-Null
$installArgs = @('--datadir=' + $dataDir, '--service=' + $serviceName, '--password={escaped_password}', '--port={port}', '--socket=' + $serviceName, '--config=' + $configTemplate, '--silent'{flags})
$init = Start-Process -FilePath $installDbPath -ArgumentList $installArgs -Wait -PassThru
if ($init.ExitCode -ne 0) {{
    exit $init.ExitCode
}}
$start = Start-Process -FilePath 'sc.exe' -ArgumentList @('start', $serviceName) -Wait -PassThru
if ($start.ExitCode -ne 0) {{
    exit $start.ExitCode
}}
$deadline = (Get-Date).AddSeconds(45)
do {{
    $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
    if ($service -and $service.Status -eq 'Running') {{
        exit 0
    }}
    Start-Sleep -Seconds 1
}} while ((Get-Date) -lt $deadline)
exit 72
"#,
        port = options.port
    );

    run_elevated_powershell_script("mariadb-fresh-database", &script, Duration::from_secs(180))
}

fn install_validation_message(
    options: &MariaDBInstallOptions,
    install_plan: &InstallPlan,
) -> Result<String, String> {
    match install_plan {
        InstallPlan::Fresh if options.skip_networking => Ok(
            "\nFresh install completed, but TCP validation was skipped because Skip networking is enabled. Disable Skip networking if this app should connect to MariaDB through localhost.".to_string(),
        ),
        InstallPlan::Fresh => validate_fresh_root_password(options),
        InstallPlan::Reattach { data_dir, .. } => Ok(format!(
            "\nExisting MariaDB data was reattached from {}. Local root accounts were reset to the installer password.",
            data_dir.display()
        )),
    }
}

fn validate_fresh_root_password(options: &MariaDBInstallOptions) -> Result<String, String> {
    let credentials = MariaDBCredentials {
        host: "localhost".to_string(),
        port: options.port,
        username: "root".to_string(),
        password: options.root_password.clone(),
        database: None,
    };
    let started = Instant::now();
    let mut last_error = String::new();

    while started.elapsed() < Duration::from_secs(45) {
        match validate_connection(credentials.clone()) {
            Ok(()) => {
                cleanup_default_install_artifacts(credentials)?;
                return Ok(format!(
                    "\nValidated root login over localhost:{} with the installer password.",
                    options.port
                ));
            }
            Err(error) => {
                last_error = error;
                thread::sleep(Duration::from_secs(2));
            }
        }
    }

    Err(format!(
        "MariaDB installed, but root login with the supplied password was not accepted over localhost:{}. Last error: {}",
        options.port,
        if last_error.trim().is_empty() {
            "MariaDB client returned no error details.".to_string()
        } else {
            last_error
        }
    ))
}

fn cleanup_default_install_artifacts(credentials: MariaDBCredentials) -> Result<(), String> {
    let cleanup_sql = "DROP DATABASE IF EXISTS test;\
        DELETE FROM mysql.global_priv WHERE User = 'PUBLIC';\
        FLUSH PRIVILEGES;"
        .to_string();
    crate::services::mariadb::query::run_admin_query(credentials, cleanup_sql)
        .map_err(|error| format!("MariaDB installed, but default cleanup failed: {error}"))
}

fn build_update_overrides(log_path: &std::path::Path) -> String {
    [
        "/qn".to_string(),
        "/norestart".to_string(),
        "/l*v".to_string(),
        quote_arg(&log_path.to_string_lossy()),
        "ADDLOCAL=DBInstance,Client,MYSQLSERVER,SharedLibraries".to_string(),
    ]
    .join(" ")
}

fn install_data_dir_candidates(options: &MariaDBInstallOptions) -> Vec<PathBuf> {
    if let Some(data_dir) = options
        .data_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return vec![PathBuf::from(data_dir)];
    }

    let mut candidates = Vec::new();
    if let Some(install_dir) = options
        .install_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        candidates.push(PathBuf::from(install_dir).join("data"));
    }

    candidates.extend(default_mariadb_data_dirs());
    candidates
}

fn preserved_install_dir(
    options: &MariaDBInstallOptions,
    data_dir: &Path,
) -> Result<PathBuf, String> {
    if let Some(install_dir) = options
        .install_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let path = PathBuf::from(install_dir);
        if !path.exists() || is_directory_empty(&path)? {
            return Ok(path);
        }
    }

    let preserved_parent = data_dir.parent().ok_or_else(|| {
        format!(
            "Could not resolve parent install directory for {}",
            data_dir.display()
        )
    })?;
    let parent = preserved_parent
        .parent()
        .unwrap_or_else(|| Path::new("C:\\"));
    let base_name = preserved_parent
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("MariaDB");

    for index in 0..50 {
        let suffix = if index == 0 {
            "Reattached".to_string()
        } else {
            format!("Reattached {index}")
        };
        let candidate = parent.join(format!("{base_name} {suffix}"));
        if !candidate.exists() || is_directory_empty(&candidate)? {
            return Ok(candidate);
        }
    }

    Err(format!(
        "Could not find an empty MariaDB install directory next to {}. Clear an old reattach folder or choose an empty Install Directory.",
        preserved_parent.display()
    ))
}

fn default_mariadb_data_dirs() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(program_files) = env::var("ProgramFiles") {
        let root = PathBuf::from(program_files);
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                    continue;
                };
                if name.to_ascii_lowercase().starts_with("mariadb ") {
                    paths.push(path.join("data"));
                }
            }
        }

        paths.push(root.join("MariaDB 12.2").join("data"));
    }

    paths.sort_by(|left, right| {
        user_schema_count(right)
            .cmp(&user_schema_count(left))
            .then_with(|| mariadb_data_dir_version(right).cmp(&mariadb_data_dir_version(left)))
            .then_with(|| right.cmp(left))
    });
    paths.dedup();
    paths
}

fn user_schema_count(path: &Path) -> usize {
    std::fs::read_dir(path)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_ascii_lowercase))
        .filter(|name| {
            !matches!(
                name.as_str(),
                "mysql" | "performance_schema" | "sys" | "test"
            )
        })
        .count()
}

fn mariadb_data_dir_version(path: &Path) -> Vec<u32> {
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .map(numeric_version_parts)
        .unwrap_or_default()
}

fn is_directory_empty(path: &Path) -> Result<bool, String> {
    if !path.is_dir() {
        return Ok(true);
    }

    let mut entries = std::fs::read_dir(path).map_err(|error| {
        format!(
            "Failed to inspect MariaDB data directory {}: {error}",
            path.display()
        )
    })?;
    Ok(entries.next().is_none())
}

fn reattach_preserved_data(
    options: &MariaDBInstallOptions,
    install_dir: &Path,
    data_dir: &Path,
) -> Result<String, String> {
    let install_dir = resolve_actual_install_dir(install_dir)?;
    let mysqld_path = install_dir.join("bin").join("mysqld.exe");
    if !mysqld_path.exists() {
        return Err(format!(
            "MariaDB binaries installed, but mysqld.exe was not found at {}.",
            mysqld_path.display()
        ));
    }

    let my_ini = prepare_preserved_my_ini(options, &install_dir, data_dir)?;
    let service_output =
        run_elevated_reattach_service(&mysqld_path, &options.service_name, &my_ini)?;
    if !service_output.success {
        return Err(format!(
            "MariaDB binaries installed, but the preserved data service could not be registered or started: {}",
            if service_output.stderr.is_empty() {
                service_output.stdout
            } else {
                service_output.stderr
            }
        ));
    }
    if !wait_for_service_running(&options.service_name, Duration::from_secs(45)) {
        return Err(format!(
            "MariaDB service was registered against preserved data, but {service_name} did not reach Running state.",
            service_name = options.service_name
        ));
    }
    let reset_output = run_elevated_reset_preserved_root_password(options, &my_ini)?;
    if !reset_output.success {
        return Err(format!(
            "MariaDB preserved data was reattached, but the root password could not be reset: {}",
            if reset_output.stderr.is_empty() {
                reset_output.stdout
            } else {
                reset_output.stderr
            }
        ));
    }

    Ok(format!(
        "Preserved data was reattached from {} using binaries in {}, and local root password was reset.",
        data_dir.display(),
        install_dir.display()
    ))
}

fn resolve_actual_install_dir(requested_install_dir: &Path) -> Result<PathBuf, String> {
    if requested_install_dir
        .join("bin")
        .join("mysqld.exe")
        .exists()
    {
        return Ok(requested_install_dir.to_path_buf());
    }

    if let Some(install_location) = registry_installed_package()
        .and_then(|package| package.install_location)
        .map(PathBuf::from)
        .filter(|path| path.join("bin").join("mysqld.exe").exists())
    {
        return Ok(install_location);
    }

    Ok(requested_install_dir.to_path_buf())
}

fn run_elevated_reattach_service(
    mysqld_path: &Path,
    service_name: &str,
    my_ini: &Path,
) -> Result<InstallOutput, String> {
    let escaped_mysqld_path = mysqld_path.to_string_lossy().replace('\'', "''");
    let escaped_service_name = service_name.replace('\'', "''");
    let escaped_my_ini = my_ini.to_string_lossy().replace('\'', "''");
    let script = format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
$mysqldPath = '{escaped_mysqld_path}'
$serviceName = '{escaped_service_name}'
$myIni = '{escaped_my_ini}'
$service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if ($service) {{
    if ($service.Status -ne 'Stopped') {{
        Stop-Service -Name $serviceName -Force -ErrorAction SilentlyContinue
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
    }}
    & sc.exe delete $serviceName | Out-Null
    Start-Sleep -Seconds 2
}}
$install = Start-Process -FilePath $mysqldPath -ArgumentList @('--install', $serviceName, "--defaults-file=`"$myIni`"") -Wait -PassThru
if ($install.ExitCode -ne 0) {{
    exit $install.ExitCode
}}
$start = Start-Process -FilePath 'sc.exe' -ArgumentList @('start', $serviceName) -Wait -PassThru
if ($start.ExitCode -ne 0) {{
    exit $start.ExitCode
}}
$deadline = (Get-Date).AddSeconds(45)
do {{
    $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
    if ($service -and $service.Status -eq 'Running') {{
        exit 0
    }}
    Start-Sleep -Seconds 1
}} while ((Get-Date) -lt $deadline)
exit 1
"#
    );

    run_elevated_powershell_script(
        "mariadb-reattach-service",
        &script,
        Duration::from_secs(180),
    )
}

fn run_elevated_reset_preserved_root_password(
    options: &MariaDBInstallOptions,
    my_ini: &Path,
) -> Result<InstallOutput, String> {
    let escaped_service_name = options.service_name.replace('\'', "''");
    let escaped_my_ini = my_ini.to_string_lossy().replace('\'', "''");
    let root_password = sql_string_literal(&options.root_password);
    let script = format!(
        r#"$ErrorActionPreference = 'Stop'
$serviceName = '{escaped_service_name}'
$myIni = '{escaped_my_ini}'
$machineHost = [System.Net.Dns]::GetHostName().ToLowerInvariant().Replace("'", "''")
$initFile = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), "fxserver-mariadb-reset-root-$([System.Guid]::NewGuid().ToString('N')).sql")
$originalConfig = [System.IO.File]::ReadAllText($myIni)
$initPathForIni = $initFile.Replace('\', '/')
$sql = @"
CREATE USER IF NOT EXISTS 'root'@'localhost';
GRANT ALL PRIVILEGES ON *.* TO 'root'@'localhost' WITH GRANT OPTION;
ALTER USER 'root'@'localhost' IDENTIFIED BY {root_password};
CREATE USER IF NOT EXISTS 'root'@'127.0.0.1';
GRANT ALL PRIVILEGES ON *.* TO 'root'@'127.0.0.1' WITH GRANT OPTION;
ALTER USER 'root'@'127.0.0.1' IDENTIFIED BY {root_password};
CREATE USER IF NOT EXISTS 'root'@'::1';
GRANT ALL PRIVILEGES ON *.* TO 'root'@'::1' WITH GRANT OPTION;
ALTER USER 'root'@'::1' IDENTIFIED BY {root_password};
CREATE USER IF NOT EXISTS 'root'@'$machineHost';
GRANT ALL PRIVILEGES ON *.* TO 'root'@'$machineHost' WITH GRANT OPTION;
ALTER USER 'root'@'$machineHost' IDENTIFIED BY {root_password};
DROP DATABASE IF EXISTS test;
DELETE FROM mysql.global_priv WHERE User = 'PUBLIC';
FLUSH PRIVILEGES;
"@
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText($initFile, $sql, $utf8NoBom)
try {{
    $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
    if ($service -and $service.Status -ne 'Stopped') {{
        Stop-Service -Name $serviceName -Force -ErrorAction SilentlyContinue
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
    }}
    [System.IO.File]::WriteAllText($myIni, $originalConfig.TrimEnd() + "`r`n`r`n[mysqld]`r`ninit-file=$initPathForIni`r`n", $utf8NoBom)
    $start = Start-Process -FilePath 'sc.exe' -ArgumentList @('start', $serviceName) -Wait -PassThru
    if ($start.ExitCode -ne 0) {{
        exit $start.ExitCode
    }}
    $deadline = (Get-Date).AddSeconds(45)
    do {{
        $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
        if ($service -and $service.Status -eq 'Running') {{
            break
        }}
        Start-Sleep -Seconds 1
    }} while ((Get-Date) -lt $deadline)
    $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
    if (-not $service -or $service.Status -ne 'Running') {{
        Write-Error "MariaDB service did not start while resetting root password."
        exit 73
    }}
    Stop-Service -Name $serviceName -Force -ErrorAction SilentlyContinue
    $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
    [System.IO.File]::WriteAllText($myIni, $originalConfig, $utf8NoBom)
    $restart = Start-Process -FilePath 'sc.exe' -ArgumentList @('start', $serviceName) -Wait -PassThru
    if ($restart.ExitCode -ne 0) {{
        exit $restart.ExitCode
    }}
    $deadline = (Get-Date).AddSeconds(45)
    do {{
        $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
        if ($service -and $service.Status -eq 'Running') {{
            exit 0
        }}
        Start-Sleep -Seconds 1
    }} while ((Get-Date) -lt $deadline)
    exit 74
}} finally {{
    [System.IO.File]::WriteAllText($myIni, $originalConfig, $utf8NoBom)
    Remove-Item -LiteralPath $initFile -Force -ErrorAction SilentlyContinue
}}
"#
    );

    run_elevated_powershell_script(
        "mariadb-reset-preserved-root",
        &script,
        Duration::from_secs(180),
    )
}

fn sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}

fn wait_for_service_running(service_name: &str, timeout: Duration) -> bool {
    let started = Instant::now();

    while started.elapsed() < timeout {
        let status = detect_mariadb();
        if status.service_name.as_deref() == Some(service_name) && status.running {
            return true;
        }

        thread::sleep(Duration::from_secs(2));
    }

    false
}

fn prepare_preserved_my_ini(
    options: &MariaDBInstallOptions,
    install_dir: &Path,
    data_dir: &Path,
) -> Result<PathBuf, String> {
    let my_ini = data_dir.join("my.ini");
    let existing = fs::read_to_string(&my_ini).unwrap_or_default();
    let content = rewrite_my_ini(&existing, options, install_dir, data_dir);

    write_text_allowing_elevation(&my_ini, &content)?;

    Ok(my_ini)
}

fn write_text_allowing_elevation(path: &Path, content: &str) -> Result<(), String> {
    if fs::write(path, content).is_ok() {
        return Ok(());
    }

    let temp_path = env::temp_dir().join(format!(
        "fxserver-installer-mariadb-my-{}.ini",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0)
    ));
    fs::write(&temp_path, content).map_err(|error| {
        format!(
            "Failed to prepare temporary MariaDB config {}: {error}",
            temp_path.display()
        )
    })?;

    let command = format!(
        "Copy-Item -LiteralPath '{}' -Destination '{}' -Force",
        temp_path.to_string_lossy().replace('\'', "''"),
        path.to_string_lossy().replace('\'', "''")
    );
    let output =
        run_elevated_powershell_script("mariadb-copy-config", &command, Duration::from_secs(120))?;
    let _ = fs::remove_file(&temp_path);

    if output.success {
        Ok(())
    } else {
        Err(format!(
            "Failed to update preserved MariaDB config {}: {}",
            path.display(),
            if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            }
        ))
    }
}

fn rewrite_my_ini(
    existing: &str,
    options: &MariaDBInstallOptions,
    install_dir: &Path,
    data_dir: &Path,
) -> String {
    let normalized_data_dir = data_dir.to_string_lossy().replace('\\', "/");
    let normalized_plugin_dir = install_dir
        .join("lib")
        .join("plugin")
        .to_string_lossy()
        .replace('\\', "/");
    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();
    if lines.is_empty() {
        lines.push("[mysqld]".to_string());
    }

    upsert_ini_value(&mut lines, "mysqld", "datadir", &normalized_data_dir);
    upsert_ini_value(&mut lines, "mysqld", "plugin-dir", &normalized_plugin_dir);
    upsert_ini_value(&mut lines, "mysqld", "port", &options.port.to_string());
    if options.use_utf8 {
        upsert_ini_value(&mut lines, "mysqld", "character-set-server", "utf8mb4");
        upsert_ini_value(
            &mut lines,
            "mysqld",
            "collation-server",
            "utf8mb4_unicode_ci",
        );
    }
    if options.skip_networking {
        upsert_ini_value(&mut lines, "mysqld", "skip-networking", "ON");
        remove_ini_value(&mut lines, "mysqld", "bind-address");
    } else {
        remove_ini_value(&mut lines, "mysqld", "skip-networking");
        upsert_ini_value(&mut lines, "mysqld", "bind-address", "127.0.0.1");
    }

    upsert_ini_value(&mut lines, "client", "plugin-dir", &normalized_plugin_dir);

    let mut output = lines.join("\r\n");
    output.push_str("\r\n");
    output
}

fn upsert_ini_value(lines: &mut Vec<String>, section: &str, key: &str, value: &str) {
    let section_header = format!("[{section}]");
    let mut section_start = None;
    let mut next_section = lines.len();

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case(&section_header) {
            section_start = Some(index);
            continue;
        }
        if section_start.is_some() && trimmed.starts_with('[') && trimmed.ends_with(']') {
            next_section = index;
            break;
        }
    }

    let section_start = match section_start {
        Some(index) => index,
        None => {
            if !lines.last().is_some_and(|line| line.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(section_header);
            lines.push(format!("{key}={value}"));
            return;
        }
    };

    for line in lines.iter_mut().take(next_section).skip(section_start + 1) {
        let trimmed = line.trim_start();
        if trimmed
            .split_once('=')
            .is_some_and(|(candidate, _)| candidate.trim().eq_ignore_ascii_case(key))
        {
            *line = format!("{key}={value}");
            return;
        }
    }

    lines.insert(next_section, format!("{key}={value}"));
}

fn remove_ini_value(lines: &mut Vec<String>, section: &str, key: &str) {
    let Some((section_start, next_section)) = ini_section_range(lines, section) else {
        return;
    };

    let mut index = next_section;
    while index > section_start + 1 {
        index -= 1;
        let trimmed = lines[index].trim_start();
        if trimmed
            .split_once('=')
            .is_some_and(|(candidate, _)| candidate.trim().eq_ignore_ascii_case(key))
            || trimmed.eq_ignore_ascii_case(key)
        {
            lines.remove(index);
        }
    }
}

fn ini_section_range(lines: &[String], section: &str) -> Option<(usize, usize)> {
    let section_header = format!("[{section}]");
    let mut section_start = None;
    let mut next_section = lines.len();

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case(&section_header) {
            section_start = Some(index);
            continue;
        }
        if section_start.is_some() && trimmed.starts_with('[') && trimmed.ends_with(']') {
            next_section = index;
            break;
        }
    }

    section_start.map(|start| (start, next_section))
}

fn selected_features(
    options: &MariaDBInstallOptions,
    install_plan: &InstallPlan,
) -> Vec<&'static str> {
    let mut features = match install_plan {
        InstallPlan::Fresh => vec!["Client", "MYSQLSERVER", "SharedLibraries"],
        InstallPlan::Reattach { .. } => vec!["Client", "MYSQLSERVER", "SharedLibraries"],
    };
    if options.install_heidi_sql {
        features.push("HeidiSQL");
    }
    if options.install_development_files {
        features.push("DEVEL");
    }
    features
}

fn property(name: &str, value: &str) -> String {
    format!("{name}={}", quote_arg(value))
}

fn quote_arg(value: &str) -> String {
    let escaped = value.replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn push_property_bool(properties: &mut Vec<String>, name: &str, enabled: bool) {
    if enabled {
        properties.push(format!("{name}=1"));
    }
}

struct RegistryPackage {
    product_code: Option<String>,
    version: Option<String>,
    install_location: Option<String>,
}

fn registry_installed_package() -> Option<RegistryPackage> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "$paths = 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*','HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*'; Get-ItemProperty $paths -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like 'MariaDB*' } | Sort-Object DisplayVersion -Descending | Select-Object -First 1 DisplayVersion,PSChildName,InstallLocation | ConvertTo-Json -Compress",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return None;
    }

    Some(RegistryPackage {
        product_code: extract_json_string(&stdout, "PSChildName"),
        version: extract_json_string(&stdout, "DisplayVersion"),
        install_location: extract_json_string(&stdout, "InstallLocation"),
    })
}

fn winget_latest_version() -> Option<String> {
    let output = Command::new("winget")
        .args(["show", "--id", "MariaDB.Server", "-e", "--source", "winget"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("Version:")
                .map(|value| value.trim().to_string())
        })
        .filter(|value| !value.is_empty())
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts = numeric_version_parts(left);
    let right_parts = numeric_version_parts(right);
    let max_len = left_parts.len().max(right_parts.len());

    for index in 0..max_len {
        let left_value = *left_parts.get(index).unwrap_or(&0);
        let right_value = *right_parts.get(index).unwrap_or(&0);
        match left_value.cmp(&right_value) {
            std::cmp::Ordering::Equal => continue,
            ordering => return ordering,
        }
    }

    std::cmp::Ordering::Equal
}

fn numeric_version_parts(value: &str) -> Vec<u32> {
    let version = value
        .split_once("Distrib")
        .map(|(_, version)| version)
        .unwrap_or(value);

    version
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\":\"");
    let start = json.find(&marker)? + marker.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].replace("\\\"", "\"").replace("\\\\", "\\"))
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
        let log_path = std::path::PathBuf::from("C:\\Temp\\mariadb-install.log");
        let options = options();
        let overrides =
            build_msi_overrides(&options, &InstallPlan::Fresh, &log_path).expect("valid overrides");

        assert!(overrides.contains("/qn"));
        assert!(overrides.contains("/norestart"));
        assert!(overrides.contains("/l*v"));
        assert!(overrides.contains("\"C:\\Temp\\mariadb-install.log\""));
        assert!(overrides.contains("STDCONFIG=1"));
        assert!(overrides.contains("UTF8=1"));
        assert!(overrides.contains("ADDLOCAL=Client,MYSQLSERVER,SharedLibraries"));
        assert!(!overrides.contains("PASSWORD="));
        assert!(!overrides.contains("SERVICENAME="));
        assert!(!overrides.contains("PORT="));
        assert!(!overrides.contains("SKIPNETWORKING="));
        assert!(!overrides.contains("ALLOWREMOTEROOTACCESS"));
        assert!(!overrides.contains("DEFAULTUSER"));
        assert!(!overrides.contains("REMOVE="));
    }

    #[test]
    fn version_compare_handles_mariadb_version_strings() {
        assert!(compare_versions("mariadb  Ver 15.1 Distrib 12.1.2-MariaDB", "12.2.2.0").is_lt());
        assert!(compare_versions("12.2.2.0", "12.2.2.0").is_eq());
        assert!(compare_versions("12.3.0", "12.2.2.0").is_gt());
    }

    #[test]
    fn preserved_data_uses_binaries_only_install() {
        let root = std::env::temp_dir().join(format!(
            "fxserver-installer-mariadb-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let data_dir = root.join("data");
        std::fs::create_dir_all(&data_dir).expect("data dir");
        std::fs::write(data_dir.join("ibdata1"), b"preserved").expect("data marker");

        let mut options = options();
        options.data_dir = Some(data_dir.to_string_lossy().to_string());
        let plan = build_install_plan(&options).expect("preserved data plan");
        let log_path = std::path::PathBuf::from("C:\\Temp\\mariadb-reattach.log");
        let overrides =
            build_msi_overrides(&options, &plan, &log_path).expect("reattach overrides");

        assert!(matches!(plan, InstallPlan::Reattach { .. }));
        assert!(overrides.contains("ADDLOCAL=Client,MYSQLSERVER,SharedLibraries"));
        assert!(!overrides.contains("ADDLOCAL=DBInstance"));
        assert!(overrides.contains("INSTALLDIR="));
        assert!(!overrides.contains("DATADIR="));
        assert!(!overrides.contains("PASSWORD="));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rewrites_preserved_my_ini_paths() {
        let content = "[mysqld]\r\ndatadir=C:/old/data\r\nskip-networking=ON\r\n\r\n[client]\r\nsocket=MariaDB\r\n";
        let rewritten = rewrite_my_ini(
            content,
            &options(),
            Path::new("C:\\Program Files\\MariaDB 12.2 Reattached"),
            Path::new("C:\\Program Files\\MariaDB 12.2\\data"),
        );

        assert!(rewritten.contains("datadir=C:/Program Files/MariaDB 12.2/data"));
        assert!(
            rewritten.contains("plugin-dir=C:/Program Files/MariaDB 12.2 Reattached/lib/plugin")
        );
        assert!(rewritten.contains("port=3306"));
    }

    #[test]
    fn preserved_my_ini_enables_local_tcp_when_networking_is_not_skipped() {
        let content = "[mysqld]\r\ndatadir=C:/old/data\r\nskip-networking=ON\r\nbind-address=0.0.0.0\r\n\r\n[client]\r\nsocket=MariaDB\r\n";
        let mut options = options();
        options.skip_networking = false;
        let rewritten = rewrite_my_ini(
            content,
            &options,
            Path::new("C:\\Program Files\\MariaDB 12.2 Reattached"),
            Path::new("C:\\Program Files\\MariaDB 12.2\\data"),
        );

        assert!(!rewritten.contains("skip-networking"));
        assert!(rewritten.contains("bind-address=127.0.0.1"));
        assert!(rewritten.contains("port=3306"));
    }

    #[test]
    fn mariadb_data_dir_version_reads_parent_directory() {
        let version = mariadb_data_dir_version(Path::new("C:\\Program Files\\MariaDB 12.2\\data"));

        assert_eq!(version, vec![12, 2]);
    }

    #[test]
    fn user_schema_count_ignores_system_databases() {
        let root = std::env::temp_dir().join(format!(
            "fxserver-installer-schema-count-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        for schema in ["mysql", "performance_schema", "sys", "test"] {
            std::fs::create_dir_all(root.join(schema)).expect("schema dir");
        }

        assert_eq!(user_schema_count(&root), 1);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn json_string_extraction_unescapes_windows_paths() {
        let value = extract_json_string(
            r#"{"InstallLocation":"C:\\Program Files\\MariaDB 12.2 Reattached\\"}"#,
            "InstallLocation",
        )
        .expect("install location");

        assert_eq!(value, "C:\\Program Files\\MariaDB 12.2 Reattached\\");
    }
}
