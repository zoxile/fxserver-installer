#![cfg_attr(not(feature = "desktop"), allow(dead_code))]

mod commands;
mod models;
mod platform;
mod process;
mod services;

#[cfg(feature = "desktop")]
mod desktop;
#[cfg(feature = "desktop")]
pub use desktop::run;

mod helpers;
pub use helpers::{run_elevated_helper_from_args, run_fxserver_watchdog_from_args};

#[cfg(windows)]
pub(crate) const ELEVATED_SCRIPT_ARG: &str = "--fxi-elevated-script";
pub(crate) const FXSERVER_WATCHDOG_ARG: &str = "--fxi-watch-fxserver";

#[cfg(feature = "terminal")]
pub mod terminal;
