//! Reads and changes the per-device USB power saving switch
//! ("Allow the computer to turn off this device to save power" in Device Manager).
//! Power saving on HOTAS devices is a common cause of disconnects and ghost inputs.

use winreg::RegKey;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE};

const ENUM_REGISTRY_ROOT: &str = r"SYSTEM\CurrentControlSet\Enum";
const DEVICE_PARAMETERS_KEY: &str = "Device Parameters";
const ENHANCED_POWER_MANAGEMENT_VALUE: &str = "EnhancedPowerManagementEnabled";
const ALLOW_IDLE_IN_D3_VALUE: &str = "AllowIdleIrpInD3";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerSavingState {
    Enabled,
    Disabled,
    NotReported,
}

/// Power saving counts as enabled when any USB node of the device still allows it.
pub fn read_power_saving_state(usb_instance_ids: &[String]) -> PowerSavingState {
    let node_values: Vec<u32> = usb_instance_ids
        .iter()
        .filter_map(|instance_id| read_enhanced_power_management(instance_id))
        .collect();
    if node_values.is_empty() {
        PowerSavingState::NotReported
    } else if node_values.iter().any(|value| *value != 0) {
        PowerSavingState::Enabled
    } else {
        PowerSavingState::Disabled
    }
}

/// Writes the switch on every USB node that exposes it. The device must be restarted afterwards.
pub fn set_power_saving_enabled(usb_instance_ids: &[String], enabled: bool) -> anyhow::Result<usize> {
    let new_value = u32::from(enabled);
    let mut changed_node_count = 0;
    for instance_id in usb_instance_ids {
        if read_enhanced_power_management(instance_id).is_some() {
            write_power_values(instance_id, new_value)?;
            changed_node_count += 1;
        }
    }
    anyhow::ensure!(
        changed_node_count > 0,
        "this device does not expose a power saving setting"
    );
    Ok(changed_node_count)
}

fn device_parameters_path(instance_id: &str) -> String {
    format!(r"{ENUM_REGISTRY_ROOT}\{instance_id}\{DEVICE_PARAMETERS_KEY}")
}

fn read_enhanced_power_management(instance_id: &str) -> Option<u32> {
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(device_parameters_path(instance_id), KEY_READ)
        .ok()?;
    key.get_value(ENHANCED_POWER_MANAGEMENT_VALUE).ok()
}

fn write_power_values(instance_id: &str, value: u32) -> anyhow::Result<()> {
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(device_parameters_path(instance_id), KEY_READ | KEY_SET_VALUE)?;
    key.set_value(ENHANCED_POWER_MANAGEMENT_VALUE, &value)?;
    key.set_value(ALLOW_IDLE_IN_D3_VALUE, &value)?;
    Ok(())
}
