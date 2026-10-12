//! Starts the application at Windows sign-in with administrator rights.
//!
//! The usual "Run" registry key cannot start elevated programs silently, so a Task Scheduler
//! task with the highest run level is used instead.

use crate::system_actions::run_hidden_command;

const TASK_NAME: &str = r"NikiCoding\PerifeMngr";
/// Task registered before the app was renamed; it points at the old program file.
const LEGACY_TASK_NAME: &str = r"NikiCoding\Joystick Manager";
pub const MINIMIZED_ARGUMENT: &str = "--minimized";

pub fn is_autostart_registered() -> bool {
    run_hidden_command("schtasks.exe", &["/Query", "/TN", TASK_NAME])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn register_autostart() -> anyhow::Result<()> {
    remove_legacy_task();
    let executable_path = std::env::current_exe()?;
    let task_definition = build_task_definition(&executable_path.to_string_lossy(), &current_user_account());
    let definition_path = std::env::temp_dir().join("perife-mngr-autostart.xml");
    std::fs::write(&definition_path, encode_as_utf16_with_bom(&task_definition))?;

    let definition_path_text = definition_path.to_string_lossy().into_owned();
    let output = run_hidden_command(
        "schtasks.exe",
        &["/Create", "/TN", TASK_NAME, "/XML", &definition_path_text, "/F"],
    );
    let _ = std::fs::remove_file(&definition_path);
    let output = output?;
    anyhow::ensure!(
        output.status.success(),
        "schtasks: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

pub fn unregister_autostart() -> anyhow::Result<()> {
    remove_legacy_task();
    if !is_autostart_registered() {
        return Ok(());
    }
    let output = run_hidden_command("schtasks.exe", &["/Delete", "/TN", TASK_NAME, "/F"])?;
    anyhow::ensure!(
        output.status.success(),
        "schtasks: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

fn remove_legacy_task() {
    let _ = run_hidden_command("schtasks.exe", &["/Delete", "/TN", LEGACY_TASK_NAME, "/F"]);
}

fn current_user_account() -> String {
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    let user_name = std::env::var("USERNAME").unwrap_or_default();
    if domain.is_empty() {
        user_name
    } else {
        format!(r"{domain}\{user_name}")
    }
}

/// Task Scheduler reads task XML most reliably as UTF-16 with a byte order mark.
fn encode_as_utf16_with_bom(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for code_unit in text.encode_utf16() {
        bytes.extend_from_slice(&code_unit.to_le_bytes());
    }
    bytes
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Runs at sign-in, elevated, with no time limit and regardless of battery state.
fn build_task_definition(executable_path: &str, user_account: &str) -> String {
    let executable_path = escape_xml(executable_path);
    let user_account = escape_xml(user_account);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>NikiCoding</Author>
    <Description>Starts PerifeMngr when you sign in.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user_account}</UserId>
      <Delay>PT5S</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user_account}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <Enabled>true</Enabled>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>"{executable_path}"</Command>
      <Arguments>{MINIMIZED_ARGUMENT}</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}
