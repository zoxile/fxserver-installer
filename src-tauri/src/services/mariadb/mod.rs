pub mod access;
pub mod backup;
pub mod backup_storage;
#[cfg_attr(target_os = "linux", path = "linux/detect.rs")]
pub mod detect;
#[cfg_attr(target_os = "linux", path = "linux/install.rs")]
pub mod install;
#[cfg(any(target_os = "linux", test))]
pub(crate) mod linux;
#[cfg_attr(target_os = "linux", path = "linux/package.rs")]
mod package;
pub mod permissions;
pub mod query;
mod read_cache;
#[cfg_attr(target_os = "linux", path = "linux/service.rs")]
pub mod service;
pub mod users;
