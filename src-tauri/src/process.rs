use std::process::Command;

pub(crate) trait CommandNoWindowExt {
    fn no_window(&mut self) -> &mut Command;
}

impl CommandNoWindowExt for Command {
    fn no_window(&mut self) -> &mut Command {
        hide_child_console(self);
        self
    }
}

#[cfg(target_os = "windows")]
fn hide_child_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn hide_child_console(_: &mut Command) {}
