//! User preferences that control how the application starts, hides and automates lighting.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::layers::{MODE_COUNT, ModeSwitchButtons};
use crate::localization::Language;
use crate::storage::{application_data_directory, read_json_file, write_json_file};

const SETTINGS_FILE_NAME: &str = "settings.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub start_minimized_to_tray: bool,
    pub close_to_tray: bool,
    pub restore_lighting_on_startup: bool,
    pub dim_lighting_when_idle: bool,
    pub idle_minutes_before_dimming: u32,
    pub dimmed_brightness_percent: u8,
    pub switch_profiles_for_games: bool,
    pub language: Language,
    pub check_for_updates: bool,
    /// The buttons the throttle's M1 / M2 / M3 switch holds down.
    pub mode_switch_buttons: ModeSwitchButtons,
    /// Profile to switch to in each mode switch position; `None` keeps the current profile.
    pub profiles_by_mode: [Option<String>; MODE_COUNT],
    /// Master switch for key, mouse and virtual controller mapping.
    pub mapping_enabled: bool,
    pub active_profile_name: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            start_minimized_to_tray: true,
            close_to_tray: true,
            restore_lighting_on_startup: true,
            dim_lighting_when_idle: false,
            idle_minutes_before_dimming: 10,
            dimmed_brightness_percent: 20,
            switch_profiles_for_games: true,
            language: Language::default(),
            check_for_updates: true,
            mode_switch_buttons: [None; MODE_COUNT],
            profiles_by_mode: [None, None, None],
            mapping_enabled: false,
            active_profile_name: crate::profiles::DEFAULT_PROFILE_NAME.to_owned(),
        }
    }
}

impl AppSettings {
    pub fn load() -> Self {
        read_json_file(&settings_file_path()).unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        write_json_file(&settings_file_path(), self)
    }
}

fn settings_file_path() -> PathBuf {
    application_data_directory().join(SETTINGS_FILE_NAME)
}
