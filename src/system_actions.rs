//! Small actions that hand work over to Windows: opening pages, tools and restarting devices.

use std::os::windows::process::CommandExt;
use std::process::{Command, Output};

use windows::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SW_RESTORE, SW_SHOWNORMAL, SetForegroundWindow, ShowWindow,
};
use windows::core::{HSTRING, w};

/// Keeps console tools such as `pnputil` from flashing a window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn open_in_default_handler(target: &str) {
    // SAFETY: both strings outlive the call; ShellExecuteW does not keep them.
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(target), None, None, SW_SHOWNORMAL);
    }
}

pub fn open_device_manager() {
    open_in_default_handler("devmgmt.msc");
}

pub fn open_game_controllers_panel() {
    let _ = run_hidden_command("control.exe", &["joy.cpl"]);
}

pub fn is_running_as_administrator() -> bool {
    // SAFETY: no arguments; reads the current process token.
    unsafe { IsUserAnAdmin().as_bool() }
}

/// Restarts a device the same way unplugging and replugging it would. Needs administrator rights.
pub fn restart_device(instance_id: &str) -> anyhow::Result<()> {
    let output = run_hidden_command("pnputil.exe", &["/restart-device", instance_id])?;
    anyhow::ensure!(
        output.status.success(),
        "pnputil: {}",
        String::from_utf8_lossy(&output.stdout).trim()
    );
    Ok(())
}

/// Restores and focuses a top-level window by its title. Used when the window was hidden to the tray.
pub fn bring_window_to_front(window_title: &str) {
    // SAFETY: the window handle comes straight from FindWindowW and is only used for these calls.
    unsafe {
        if let Ok(window) = FindWindowW(None, &HSTRING::from(window_title)) {
            let _ = ShowWindow(window, SW_RESTORE);
            let _ = SetForegroundWindow(window);
        }
    }
}

pub fn run_hidden_command(program: &str, arguments: &[&str]) -> std::io::Result<Output> {
    Command::new(program)
        .args(arguments)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}
