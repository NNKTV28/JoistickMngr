#![windows_subsystem = "windows"]

mod app;
mod app_icon;
mod app_state;
mod audio_devices;
mod automation;
mod autostart;
mod bindings;
mod calibration;
mod device_scanner;
mod driver_repair;
mod driver_status;
mod hall_effect;
mod hid_controller;
mod input_injection;
mod input_reader;
mod keyboard_keys;
mod known_devices;
mod layers;
mod lighting;
mod lighting_effects;
mod localization;
mod mapping_engine;
mod mapping_presets;
mod press_styles;
mod product_images;
mod profiles;
mod settings;
mod single_instance;
mod storage;
mod system_actions;
mod theme;
mod thread_priority;
mod tray;
mod ui;
mod updater;
mod usb_power;
mod version_numbers;
mod virtual_gamepad;
mod windows_update;

use std::sync::{Arc, Mutex};

use eframe::egui;

use app::{BackgroundServices, PerifeMngrApp, WINDOW_TITLE};
use app_state::{AppState, SharedAppState};
use audio_devices::AudioService;
use autostart::MINIMIZED_ARGUMENT;
use device_scanner::DeviceScannerService;
use input_reader::InputReaderService;
use lighting::LightingService;
use mapping_engine::MappingEngine;
use updater::UpdateService;
use windows_update::WindowsUpdateService;

const REGISTER_AUTOSTART_ARGUMENT: &str = "--register-autostart";
const UNREGISTER_AUTOSTART_ARGUMENT: &str = "--unregister-autostart";
const WINDOW_ICON_SIZE: u32 = 64;

fn main() -> eframe::Result {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if let Some(exit_code) = run_command_line_task(&arguments) {
        std::process::exit(exit_code);
    }
    let Some(single_instance) = single_instance::acquire_single_instance() else {
        return Ok(());
    };

    storage::move_legacy_application_data();
    let app_state: SharedAppState = Arc::new(Mutex::new(AppState::load()));
    localization::set_current_language(app_state.lock().unwrap().settings.language);
    let services = start_background_services(&app_state);
    restore_lighting_if_enabled(&app_state, &services.lighting);
    start_update_checks_if_enabled(&app_state, &services.updater);
    automation::start_automation(Arc::clone(&app_state), services.lighting.clone());

    let starts_visible = !should_start_hidden(&arguments, &app_state);
    eframe::run_native(
        WINDOW_TITLE,
        build_native_options(starts_visible),
        Box::new(move |creation_context| {
            let app = PerifeMngrApp::new(creation_context, app_state, services, single_instance, starts_visible);
            Ok(Box::new(app))
        }),
    )
}

/// Handles the switches the installer uses. Returns an exit code when the program should stop afterwards.
fn run_command_line_task(arguments: &[String]) -> Option<i32> {
    let task_result = if arguments.iter().any(|argument| argument == REGISTER_AUTOSTART_ARGUMENT) {
        autostart::register_autostart()
    } else if arguments
        .iter()
        .any(|argument| argument == UNREGISTER_AUTOSTART_ARGUMENT)
    {
        autostart::unregister_autostart()
    } else {
        return None;
    };
    Some(if task_result.is_ok() { 0 } else { 1 })
}

fn start_background_services(app_state: &SharedAppState) -> BackgroundServices {
    let input_reader = InputReaderService::start();
    let lighting = LightingService::start();
    BackgroundServices {
        audio: AudioService::start(),
        mapping_engine: MappingEngine::start(Arc::clone(app_state), &input_reader, lighting.clone()),
        device_scanner: DeviceScannerService::start(),
        input_reader,
        lighting,
        windows_update: WindowsUpdateService::default(),
        updater: UpdateService::default(),
    }
}

fn start_update_checks_if_enabled(app_state: &SharedAppState, updater: &UpdateService) {
    if app_state.lock().unwrap().settings.check_for_updates {
        updater.start_automatic_checks();
    }
}

fn restore_lighting_if_enabled(app_state: &SharedAppState, lighting: &LightingService) {
    let state = app_state.lock().unwrap();
    if state.settings.restore_lighting_on_startup {
        lighting.apply_profile(&state.effective_profile());
    }
}

/// The sign-in task passes `--minimized`; the user decides whether that really means "tray only".
fn should_start_hidden(arguments: &[String], app_state: &SharedAppState) -> bool {
    let launched_by_autostart = arguments.iter().any(|argument| argument == MINIMIZED_ARGUMENT);
    launched_by_autostart && app_state.lock().unwrap().settings.start_minimized_to_tray
}

fn build_native_options(starts_visible: bool) -> eframe::NativeOptions {
    let window_icon = egui::IconData {
        rgba: app_icon::render_logo_rgba(WINDOW_ICON_SIZE),
        width: WINDOW_ICON_SIZE,
        height: WINDOW_ICON_SIZE,
    };
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([960.0, 620.0])
            .with_icon(window_icon)
            .with_visible(starts_visible),
        ..Default::default()
    }
}
