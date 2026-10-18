use super::{
    detect,
    linux::{
        plans::{service_plan, service_unit},
        runtime,
    },
};

pub fn start_service(name: Option<String>) -> Result<(), String> {
    control("start", name)
}
pub fn stop_service(name: Option<String>) -> Result<(), String> {
    control("stop", name)
}
pub fn restart_service(name: Option<String>) -> Result<(), String> {
    control("restart", name)
}

pub(super) fn validate_service_name(name: &str) -> Result<(), String> {
    service_unit(name).map(|_| ())
}

fn control(action: &str, name: Option<String>) -> Result<(), String> {
    let name = name.or_else(detect::find_service_name).ok_or("No MariaDB systemd service was found. Containers and non-systemd hosts must manage MariaDB externally.")?;
    validate_service_name(&name)?;
    if !detect::identified_service(&name) {
        return Err("The selected systemd unit is not an identifiable MariaDB service.".into());
    }
    detect::clear_detection_cache();
    let result = runtime::administer(&service_plan(action, &name)?);
    detect::clear_detection_cache();
    result?;
    if detect::is_service_running(&name) != (action != "stop") {
        return Err(
            "systemctl completed but the expected MariaDB service state was not observed.".into(),
        );
    }
    Ok(())
}
