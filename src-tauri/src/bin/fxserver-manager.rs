fn main() {
    if app_lib::run_elevated_helper_from_args() || app_lib::run_fxserver_watchdog_from_args() {
        return;
    }
    if let Err(error) = app_lib::terminal::run() {
        eprintln!("{}", app_lib::terminal::safe_text(&error));
        std::process::exit(1);
    }
}
