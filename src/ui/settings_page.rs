//! Settings: start-up behaviour, tray behaviour and information about this installation.

use eframe::egui::{RichText, Ui};

use crate::app::JoystickManagerApp;
use crate::autostart::{is_autostart_registered, register_autostart, unregister_autostart};
use crate::lighting::LightingAvailability;
use crate::localization::{Language, set_current_language, tr, tr_format};
use crate::storage::application_data_directory;
use crate::system_actions::{is_running_as_administrator, open_in_default_handler};
use crate::theme::*;
use crate::ui::widgets::*;
use crate::updater::{UpdateService, UpdateState};

const PUBLISHER_WEBSITE: &str = "https://nikicoding.com";

impl JoystickManagerApp {
    pub(crate) fn show_settings_page(&mut self, ui: &mut Ui) {
        page_header(ui, "Preferences", "Settings", None, |_| {});
        self.show_startup_card(ui);
        ui.add_space(14.0);
        self.show_language_card(ui);
        ui.add_space(14.0);
        self.show_window_card(ui);
        ui.add_space(14.0);
        self.show_updates_card(ui);
        ui.add_space(14.0);
        self.show_about_card(ui);
    }

    fn show_startup_card(&mut self, ui: &mut Ui) {
        card(ui, |ui| {
            card_title(ui, "Start-up");
            ui.add_space(6.0);
            let mut start_with_windows = self.autostart_registered;
            if ui
                .checkbox(
                    &mut start_with_windows,
                    tr("Start with Windows (runs as administrator)"),
                )
                .changed()
            {
                self.change_autostart(start_with_windows);
            }
            self.setting_checkbox(
                ui,
                "When started with Windows, stay in the tray",
                |settings| settings.start_minimized_to_tray,
                |settings, value| settings.start_minimized_to_tray = value,
            );
            self.setting_checkbox(
                ui,
                "Restore lighting when the app starts",
                |settings| settings.restore_lighting_on_startup,
                |settings, value| settings.restore_lighting_on_startup = value,
            );
        });
    }

    fn change_autostart(&mut self, enable: bool) {
        let result = if enable {
            register_autostart()
        } else {
            unregister_autostart()
        };
        self.autostart_registered = is_autostart_registered();
        match result {
            Ok(()) if enable => self.show_status("Joystick Manager will start when you sign in.", Tone::Success),
            Ok(()) => self.show_status("Joystick Manager will no longer start with Windows.", Tone::Info),
            Err(error) => self.show_status(tr_format("Could not change start-up: {}", &[&error]), Tone::Danger),
        }
    }

    fn show_window_card(&mut self, ui: &mut Ui) {
        card(ui, |ui| {
            card_title(ui, "Window");
            ui.add_space(6.0);
            self.setting_checkbox(
                ui,
                "Closing the window keeps the app running in the tray",
                |settings| settings.close_to_tray,
                |settings, value| settings.close_to_tray = value,
            );
            muted_text(ui, "Use Quit in the tray icon menu to exit completely.");
        });
    }

    fn show_about_card(&mut self, ui: &mut Ui) {
        let lighting_text = match self.services.lighting.availability() {
            LightingAvailability::Ready { connected_units } => {
                tr_format("Ready · {} unit(s)", &[&connected_units.len()])
            }
            LightingAvailability::Starting => "Starting".into(),
            LightingAvailability::SoftwareNotInstalled => "Logitech X56 software not installed".into(),
            LightingAvailability::Failed(error) => format!("{}: {error}", tr("Failed")),
        };
        let data_directory = application_data_directory();
        let mut open_data_folder = false;
        let mut open_website = false;
        card(ui, |ui| {
            card_title(ui, "About");
            ui.add_space(6.0);
            key_value_grid(
                ui,
                "about",
                &[
                    ("Version", env!("CARGO_PKG_VERSION").to_owned()),
                    ("Running as administrator", yes_or_no(is_running_as_administrator())),
                    ("Lighting (DirectOutput)", lighting_text),
                    ("Settings folder", data_directory.display().to_string()),
                ],
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                open_data_folder = secondary_button(ui, "Open settings folder").clicked();
                ui.label(RichText::new(tr("Made by NikiCoding ·")).color(TEXT_MUTED));
                open_website = ui.link(PUBLISHER_WEBSITE).clicked();
            });
        });
        if open_data_folder {
            let _ = std::fs::create_dir_all(&data_directory);
            open_in_default_handler(&data_directory.to_string_lossy());
        }
        if open_website {
            open_in_default_handler(PUBLISHER_WEBSITE);
        }
    }
}

fn yes_or_no(value: bool) -> String {
    tr(if value { "Yes" } else { "No" }).to_owned()
}

impl JoystickManagerApp {
    fn show_updates_card(&mut self, ui: &mut Ui) {
        let update_state = self.services.updater.state();
        let mut check_clicked = false;
        let mut install_clicked = false;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                card_title(ui, "Updates");
                let (status_text, status_tone) = describe_update_state(&update_state);
                status_pill(ui, &status_text, status_tone);
            });
            ui.add_space(6.0);
            muted_text(
                ui,
                tr_format(
                    "Installed version {}. Updates come from the GitHub releases and are checked against their published checksum before installing.",
                    &[&UpdateService::current_version()],
                ),
            );
            self.setting_checkbox(
                ui,
                "Check for updates automatically",
                |settings| settings.check_for_updates,
                |settings, value| settings.check_for_updates = value,
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let is_busy = matches!(update_state, UpdateState::Checking | UpdateState::Downloading(_));
                if is_busy {
                    ui.spinner();
                }
                check_clicked = enabled_secondary_button(ui, !is_busy, "Check now").clicked();
                if let UpdateState::Available(update) = &update_state {
                    install_clicked = primary_button(ui, &tr_format("Install {}", &[&update.version])).clicked();
                }
            });
        });
        if check_clicked {
            self.services.updater.start_check();
        }
        if install_clicked && let UpdateState::Available(update) = update_state {
            self.services.updater.start_install(update);
        }
    }
}

fn describe_update_state(update_state: &UpdateState) -> (String, Tone) {
    match update_state {
        UpdateState::NotChecked => ("Not checked".into(), Tone::Neutral),
        UpdateState::Checking => ("Checking…".into(), Tone::Info),
        UpdateState::UpToDate => ("Up to date".into(), Tone::Success),
        UpdateState::Available(update) => (tr_format("Version {} available", &[&update.version]), Tone::Warning),
        UpdateState::Downloading(update) => (tr_format("Installing {}…", &[&update.version]), Tone::Info),
        UpdateState::Failed(message) => (message.clone(), Tone::Danger),
    }
}

impl JoystickManagerApp {
    /// The language names stay in their own language so they can be found whatever is selected.
    fn show_language_card(&mut self, ui: &mut Ui) {
        let current = self.app_state.lock().unwrap().settings.language;
        let mut chosen = current;
        card(ui, |ui| {
            card_title(ui, "Language");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for language in Language::ALL {
                    ui.selectable_value(&mut chosen, language, language.native_name());
                }
            });
        });
        if chosen != current {
            set_current_language(chosen);
            self.update_settings(|settings| settings.language = chosen);
        }
    }
}
