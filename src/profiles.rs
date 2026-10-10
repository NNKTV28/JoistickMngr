//! Named setups combining lighting, axis tuning and an optional game that activates them.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::known_devices::DeviceKind;
use crate::storage::{application_data_directory, read_json_file, write_json_file};

pub const DEFAULT_PROFILE_NAME: &str = "Default";
const PROFILES_FILE_NAME: &str = "profiles.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl RgbColor {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    pub fn to_hex_string(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.red, self.green, self.blue)
    }

    pub fn parse_hex_string(text: &str) -> Option<Self> {
        let digits = text.trim().trim_start_matches('#');
        if digits.len() != 6 {
            return None;
        }
        let parse_channel = |start: usize| u8::from_str_radix(&digits[start..start + 2], 16).ok();
        Some(Self::new(parse_channel(0)?, parse_channel(2)?, parse_channel(4)?))
    }

    /// Windows `COLORREF` layout: 0x00BBGGRR.
    pub fn to_colorref(self) -> u32 {
        self.red as u32 | (self.green as u32) << 8 | (self.blue as u32) << 16
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LightingSetting {
    pub color: RgbColor,
    pub brightness_percent: u8,
}

impl Default for LightingSetting {
    fn default() -> Self {
        Self {
            color: RgbColor::new(0x00, 0xC8, 0xFF),
            brightness_percent: 100,
        }
    }
}

impl LightingSetting {
    pub fn with_brightness_capped_at(self, maximum_brightness_percent: u8) -> Self {
        Self {
            brightness_percent: self.brightness_percent.min(maximum_brightness_percent),
            ..self
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AxisTuning {
    pub device_kind: DeviceKind,
    pub axis_index: usize,
    pub deadzone_percent: f32,
    /// 0 is a straight line, 1 is a strong curve that softens movement near the rest position.
    pub curve_strength: f32,
    pub saturation_percent: f32,
    pub inverted: bool,
}

impl AxisTuning {
    pub fn neutral(device_kind: DeviceKind, axis_index: usize) -> Self {
        Self {
            device_kind,
            axis_index,
            deadzone_percent: 0.0,
            curve_strength: 0.0,
            saturation_percent: 100.0,
            inverted: false,
        }
    }

    /// Shapes a raw reading in 0..=1 the same way the tuning is described in the editor.
    pub fn apply_to_reading(&self, raw_reading: f64, rests_at_center: bool) -> f64 {
        let reading = if self.inverted { 1.0 - raw_reading } else { raw_reading };
        if rests_at_center {
            let signed_offset = reading * 2.0 - 1.0;
            let shaped_offset = signed_offset.signum() * self.shape_magnitude(signed_offset.abs());
            (shaped_offset + 1.0) / 2.0
        } else {
            self.shape_magnitude(reading)
        }
    }

    /// Applies deadzone, saturation and curve to a magnitude in 0..=1.
    fn shape_magnitude(&self, magnitude: f64) -> f64 {
        let deadzone = (self.deadzone_percent as f64 / 100.0).clamp(0.0, 0.5);
        let saturation = (self.saturation_percent as f64 / 100.0).clamp(deadzone + 0.01, 1.0);
        let scaled = ((magnitude - deadzone) / (saturation - deadzone)).clamp(0.0, 1.0);
        let curve_exponent = 1.0 + self.curve_strength.clamp(0.0, 1.0) as f64 * 2.0;
        scaled.powf(curve_exponent)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    /// File name such as `game.exe`; empty means the profile is only activated manually.
    pub game_executable_name: String,
    pub joystick_lighting: LightingSetting,
    pub throttle_lighting: LightingSetting,
    pub axis_tunings: Vec<AxisTuning>,
}

impl Default for Profile {
    fn default() -> Self {
        Self::named(DEFAULT_PROFILE_NAME)
    }
}

impl Profile {
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            game_executable_name: String::new(),
            joystick_lighting: LightingSetting::default(),
            throttle_lighting: LightingSetting::default(),
            axis_tunings: Vec::new(),
        }
    }

    pub fn lighting_for(&self, device_kind: DeviceKind) -> Option<LightingSetting> {
        match device_kind {
            DeviceKind::Joystick => Some(self.joystick_lighting),
            DeviceKind::Throttle => Some(self.throttle_lighting),
            DeviceKind::RudderPedals => None,
        }
    }

    pub fn set_lighting_for(&mut self, device_kind: DeviceKind, lighting: LightingSetting) {
        match device_kind {
            DeviceKind::Joystick => self.joystick_lighting = lighting,
            DeviceKind::Throttle => self.throttle_lighting = lighting,
            DeviceKind::RudderPedals => {}
        }
    }

    pub fn is_triggered_by_game(&self) -> bool {
        !self.game_executable_name.trim().is_empty()
    }

    pub fn tuning_for_axis(&self, device_kind: DeviceKind, axis_index: usize) -> Option<&AxisTuning> {
        self.axis_tunings
            .iter()
            .find(|tuning| tuning.device_kind == device_kind && tuning.axis_index == axis_index)
    }

    pub fn tuning_for_axis_mut(&mut self, device_kind: DeviceKind, axis_index: usize) -> &mut AxisTuning {
        let existing_position = self
            .axis_tunings
            .iter()
            .position(|tuning| tuning.device_kind == device_kind && tuning.axis_index == axis_index);
        let position = existing_position.unwrap_or_else(|| {
            self.axis_tunings.push(AxisTuning::neutral(device_kind, axis_index));
            self.axis_tunings.len() - 1
        });
        &mut self.axis_tunings[position]
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProfileStore {
    pub profiles: Vec<Profile>,
}

impl ProfileStore {
    pub fn load() -> Self {
        let mut store: Self = read_json_file(&profiles_file_path()).unwrap_or_default();
        store.ensure_default_profile_exists();
        store
    }

    pub fn save(&self) -> anyhow::Result<()> {
        write_json_file(&profiles_file_path(), self)
    }

    pub fn find_by_name(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.name == name)
    }

    pub fn find_for_running_game(&self, running_executable_names: &[String]) -> Option<&Profile> {
        self.profiles
            .iter()
            .filter(|profile| profile.is_triggered_by_game())
            .find(|profile| {
                let wanted_name = profile.game_executable_name.trim();
                running_executable_names
                    .iter()
                    .any(|running_name| running_name.eq_ignore_ascii_case(wanted_name))
            })
    }

    pub fn contains_name(&self, name: &str) -> bool {
        self.find_by_name(name).is_some()
    }

    /// Returns `base_name`, or `base_name 2`, `base_name 3`... whichever is not taken yet.
    pub fn unused_name_like(&self, base_name: &str) -> String {
        if !self.contains_name(base_name) {
            return base_name.to_owned();
        }
        (2..)
            .map(|counter| format!("{base_name} {counter}"))
            .find(|candidate| !self.contains_name(candidate))
            .expect("an unused name always exists")
    }

    fn ensure_default_profile_exists(&mut self) {
        if self.profiles.is_empty() {
            self.profiles.push(Profile::default());
        }
    }
}

fn profiles_file_path() -> PathBuf {
    application_data_directory().join(PROFILES_FILE_NAME)
}
