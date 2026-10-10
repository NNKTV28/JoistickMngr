//! Locations and helpers for the JSON files the application keeps in the user's AppData folder.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

const PUBLISHER_FOLDER_NAME: &str = "NikiCoding";
const APPLICATION_FOLDER_NAME: &str = "JoystickManager";

pub fn application_data_directory() -> PathBuf {
    let roaming_app_data = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    roaming_app_data
        .join(PUBLISHER_FOLDER_NAME)
        .join(APPLICATION_FOLDER_NAME)
}

pub fn read_json_file<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let file_contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&file_contents).ok()
}

pub fn write_json_file<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(parent_directory) = path.parent() {
        std::fs::create_dir_all(parent_directory)?;
    }
    let file_contents = serde_json::to_string_pretty(value)?;
    std::fs::write(path, file_contents)?;
    Ok(())
}
