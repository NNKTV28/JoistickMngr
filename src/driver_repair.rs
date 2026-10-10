//! Repairs a device whose driver Windows refuses to load, for example an old vendor driver blocked by
//! Memory Integrity (Core isolation). Removing the driver package lets Windows fall back to its own driver.

use crate::system_actions::run_hidden_command;

const DRIVER_NAME_LABEL: &str = "Driver Name:";
const THIRD_PARTY_PACKAGE_PREFIX: &str = "oem";

/// The third-party driver package (such as `oem54.inf`) installed on a device node, if any.
/// Windows' own inbox drivers (such as `input.inf`) are never returned, because they cannot be removed.
pub fn find_installed_third_party_package(instance_id: &str) -> Option<String> {
    let output = run_hidden_command("pnputil.exe", &["/enum-devices", "/instanceid", instance_id]).ok()?;
    parse_third_party_package(&String::from_utf8_lossy(&output.stdout))
}

/// Reads the first `Driver Name:` line of a `pnputil /enum-devices` report, which is the installed driver.
fn parse_third_party_package(device_report: &str) -> Option<String> {
    let installed_package = device_report
        .lines()
        .find_map(|line| line.trim().strip_prefix(DRIVER_NAME_LABEL))?
        .trim()
        .to_owned();
    installed_package
        .to_lowercase()
        .starts_with(THIRD_PARTY_PACKAGE_PREFIX)
        .then_some(installed_package)
}

/// Removes the driver package from every device using it and from the driver store,
/// then asks Windows to pick the next best driver. Needs administrator rights.
pub fn remove_driver_package(published_package_name: &str) -> anyhow::Result<()> {
    let output = run_hidden_command(
        "pnputil.exe",
        &["/delete-driver", published_package_name, "/uninstall", "/force"],
    )?;
    anyhow::ensure!(
        output.status.success(),
        "pnputil: {}",
        String::from_utf8_lossy(&output.stdout).trim()
    );
    run_hidden_command("pnputil.exe", &["/scan-devices"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_third_party_package;

    #[test]
    fn finds_vendor_package_installed_on_device() {
        let report = "Instance ID:                HID\\VID_06A3&PID_0763\\8&2a715df8&1&0000\n\
                      Status:                     Problem\n\
                      Driver Name:                oem54.inf\n\
                      Matching Drivers:\n    Driver Name:            input.inf\n";
        assert_eq!(parse_third_party_package(report), Some("oem54.inf".to_owned()));
    }

    #[test]
    fn ignores_windows_inbox_driver() {
        let report = "Status:                     Started\nDriver Name:                input.inf\n";
        assert_eq!(parse_third_party_package(report), None);
    }
}
