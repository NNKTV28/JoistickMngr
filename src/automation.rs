//! Background rules that keep running while the window is hidden:
//! switching profiles when a game starts and dimming the lights when the PC is idle.

use std::time::Duration;

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

use crate::app_state::SharedAppState;
use crate::lighting::LightingService;

const AUTOMATION_INTERVAL: Duration = Duration::from_secs(2);

pub fn start_automation(app_state: SharedAppState, lighting: LightingService) {
    std::thread::Builder::new()
        .name("automation".into())
        .spawn(move || run_automation_loop(app_state, lighting))
        .expect("automation thread starts");
}

fn run_automation_loop(app_state: SharedAppState, lighting: LightingService) {
    let mut system = System::new_with_specifics(RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()));
    loop {
        let running_executable_names = list_running_executable_names(&mut system);
        let profile_changed = update_game_profile(&app_state, &running_executable_names);
        let dimming_changed = update_idle_dimming(&app_state);
        if profile_changed || dimming_changed {
            apply_current_lighting(&app_state, &lighting);
        }
        std::thread::sleep(AUTOMATION_INTERVAL);
    }
}

/// Applies the effective profile's lighting, dimmed if the PC is idle.
pub fn apply_current_lighting(app_state: &SharedAppState, lighting: &LightingService) {
    let state = app_state.lock().unwrap();
    let profile = state.effective_profile();
    if state.lighting_is_dimmed {
        lighting.apply_profile_dimmed(&profile, state.settings.dimmed_brightness_percent);
    } else {
        lighting.apply_profile(&profile);
    }
}

fn list_running_executable_names(system: &mut System) -> Vec<String> {
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    system
        .processes()
        .values()
        .map(|process| process.name().to_string_lossy().into_owned())
        .collect()
}

/// Returns true when the game profile in use changed.
fn update_game_profile(app_state: &SharedAppState, running_executable_names: &[String]) -> bool {
    let mut state = app_state.lock().unwrap();
    let detected_game_profile = if state.settings.switch_profiles_for_games {
        state
            .profile_store
            .find_for_running_game(running_executable_names)
            .map(|profile| profile.name.clone())
    } else {
        None
    };
    let has_changed = detected_game_profile != state.game_profile_name;
    state.game_profile_name = detected_game_profile;
    has_changed
}

/// Returns true when the lights should switch between dimmed and normal.
fn update_idle_dimming(app_state: &SharedAppState) -> bool {
    let mut state = app_state.lock().unwrap();
    let idle_limit = Duration::from_secs(u64::from(state.settings.idle_minutes_before_dimming) * 60);
    let should_be_dimmed = state.settings.dim_lighting_when_idle && time_since_last_user_input() >= idle_limit;
    let has_changed = should_be_dimmed != state.lighting_is_dimmed;
    state.lighting_is_dimmed = should_be_dimmed;
    has_changed
}

/// Time since the last keyboard or mouse input anywhere in the session.
fn time_since_last_user_input() -> Duration {
    let mut last_input = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: last_input is a correctly sized LASTINPUTINFO.
    let has_last_input = unsafe { GetLastInputInfo(&mut last_input).as_bool() };
    if !has_last_input {
        return Duration::ZERO;
    }
    // SAFETY: no arguments.
    let milliseconds_now = unsafe { GetTickCount() };
    Duration::from_millis(u64::from(milliseconds_now.wrapping_sub(last_input.dwTime)))
}
