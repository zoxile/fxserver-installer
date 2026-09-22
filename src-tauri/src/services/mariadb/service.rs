use super::{
    detect::find_service_name,
    install::{run_elevated_powershell_script, run_process},
};
use std::time::Duration;

pub fn start_service(service_name: Option<String>) -> Result<(), String> {
    control_service("start", service_name)
}

pub fn stop_service(service_name: Option<String>) -> Result<(), String> {
    control_service("stop", service_name)
}

pub fn restart_service(service_name: Option<String>) -> Result<(), String> {
    control_service("restart", service_name)
}

fn control_service(action: &str, service_name: Option<String>) -> Result<(), String> {
    super::detect::clear_detection_cache();
    let service_name = service_name
        .or_else(find_service_name)
        .ok_or_else(missing_service_message)?;
    validate_service_name(&service_name)?;
    let script = service_script(action, &service_name)?;
    let output = run_process(
        "powershell",
        &["-NoProfile", "-Command", &script],
        Duration::from_secs(150),
    )?;
    // Only a Win32 access-denied code emitted by our exception handler permits UAC retry.
    let output = if !output.success
        && output
            .stderr
            .lines()
            .any(|line| line == "FXI_SERVICE_ACCESS_DENIED")
    {
        run_elevated_powershell_script(
            "mariadb-service-control",
            &script,
            Duration::from_secs(180),
        )?
    } else {
        output
    };
    super::detect::clear_detection_cache();
    if output.success {
        Ok(())
    } else {
        Err(if output.stderr.is_empty() {
            "MariaDB service action failed or administrator approval was declined.".into()
        } else {
            output.stderr
        })
    }
}

pub(super) fn validate_service_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err("Service name must contain only letters, digits, hyphens, or underscores (1-64 characters).".into());
    }
    Ok(())
}

fn service_script(action: &str, service_name: &str) -> Result<String, String> {
    validate_service_name(service_name)?;
    if !matches!(action, "start" | "stop" | "restart") {
        return Err("Unsupported service action.".into());
    }
    Ok(format!(
        r#"$ErrorActionPreference = 'Stop'
try {{
    $serviceName = '{service_name}'
    $action = '{action}'
    $identity = @(Get-CimInstance Win32_Service -Filter "Name='$serviceName'")
    if ($identity.Count -ne 1 -or $identity[0].PathName -notmatch '(?i)(?:^|[\\/])(?:mariadbd|mysqld)\.exe(?:"|\s|$)' -or $identity[0].PathName -notmatch '(?i)mariadb') {{
        throw 'The selected service is not an identifiable MariaDB server.'
    }}
    $service = New-Object System.ServiceProcess.ServiceController($serviceName)
    $service.Refresh()
    if ($service.Status -eq 'StopPending') {{ $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(60)) }}
    if ($service.Status -eq 'StartPending') {{ $service.WaitForStatus('Running', [TimeSpan]::FromSeconds(60)) }}
    if ($action -in @('stop', 'restart') -and $service.Status -ne 'Stopped') {{
        $service.Stop()
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(60))
    }}
    if ($action -in @('start', 'restart') -and $service.Status -ne 'Running') {{
        $service.Start()
        $service.WaitForStatus('Running', [TimeSpan]::FromSeconds(60))
    }}
    $service.Dispose()
    exit 0
}} catch {{
    $cause = $_.Exception
    while ($cause) {{
        if ($cause -is [ComponentModel.Win32Exception] -and $cause.NativeErrorCode -eq 5) {{
            [Console]::Error.WriteLine('FXI_SERVICE_ACCESS_DENIED')
            exit 5
        }}
        $cause = $cause.InnerException
    }}
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}}
"#
    ))
}

fn missing_service_message() -> String {
    "No MariaDB Windows service was found.".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_wildcards_and_script_injection() {
        for name in [
            "",
            "*",
            "MariaDB?",
            "MariaDB[12]",
            "a\nb",
            "Maria'DB",
            "$(calc)",
            "a b",
            "a;exit",
        ] {
            assert!(service_script("restart", name).is_err());
        }
        assert!(service_script("delete", "MariaDB").is_err());
        assert!(service_script("start", "MariaDB_11-4").is_ok());
    }
    #[test]
    fn restart_waits_and_never_suppresses_stop_errors() {
        let script = service_script("restart", "MariaDB").unwrap();
        let stop = script.find("$service.Stop()").unwrap();
        let wait = script[stop..].find("WaitForStatus('Stopped'").unwrap() + stop;
        let start = script.find("$service.Start()").unwrap();
        assert!(stop < wait && wait < start);
        assert!(!script.contains("SilentlyContinue"));
        assert!(!script.contains("-Force"));
        assert!(script.contains("NativeErrorCode -eq 5"));
    }

    #[cfg(windows)]
    #[test]
    fn simulated_stop_failure_prevents_start_and_does_not_request_elevation() {
        let fixture = r#"
function Get-CimInstance { [pscustomobject]@{ PathName = '"C:\MariaDB\bin\mysqld.exe"' } }
function New-Object {
    $mock = [pscustomobject]@{ Status = 'Running' }
    $mock | Add-Member ScriptMethod Refresh {}
    $mock | Add-Member ScriptMethod Stop { [Console]::Out.WriteLine('STOP'); $this.Status = 'StopPending' }
    $mock | Add-Member ScriptMethod WaitForStatus { throw 'fixture stop timeout' }
    $mock | Add-Member ScriptMethod Start { [Console]::Out.WriteLine('UNEXPECTED_START') }
    $mock | Add-Member ScriptMethod Dispose {}
    $mock
}
"#;
        let script = format!(
            "{fixture}\n{}",
            service_script("restart", "MariaDB").unwrap()
        );
        let output = run_process(
            "powershell",
            &["-NoProfile", "-Command", &script],
            Duration::from_secs(15),
        )
        .unwrap();
        assert!(!output.success);
        assert!(output.stdout.contains("STOP"), "{}", output.stderr);
        assert!(!output.stdout.contains("UNEXPECTED_START"));
        assert!(output.stderr.contains("fixture stop timeout"));
        assert!(!output.stderr.contains("FXI_SERVICE_ACCESS_DENIED"));
    }
}
