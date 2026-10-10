//! State shared between the window and the background automation: settings, profiles and
//! which profile is currently driving the lighting.

use std::sync::{Arc, Mutex};

use crate::profiles::{Profile, ProfileStore};
use crate::settings::AppSettings;

pub struct AppState {
    pub settings: AppSettings,
    pub profile_store: ProfileStore,
    /// Set while a game with its own profile is running; it overrides the manually chosen profile.
    pub game_profile_name: Option<String>,
    pub lighting_is_dimmed: bool,
}

pub type SharedAppState = Arc<Mutex<AppState>>;

impl AppState {
    pub fn load() -> Self {
        Self {
            settings: AppSettings::load(),
            profile_store: ProfileStore::load(),
            game_profile_name: None,
            lighting_is_dimmed: false,
        }
    }

    /// The profile that should be applied right now.
    pub fn effective_profile(&self) -> Profile {
        let wanted_name = self
            .game_profile_name
            .as_deref()
            .unwrap_or(&self.settings.active_profile_name);
        self.profile_store
            .find_by_name(wanted_name)
            .or_else(|| self.profile_store.profiles.first())
            .cloned()
            .unwrap_or_default()
    }

    pub fn effective_profile_name(&self) -> String {
        self.effective_profile().name
    }
}
