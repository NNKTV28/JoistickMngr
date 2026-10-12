//! Lighting: live colour and brightness editing for the X56 stick and throttle.

use eframe::egui::{self, ComboBox, RichText, Slider, TextureHandle, Ui};

use crate::app::PerifeMngrApp;
use crate::driver_status::vendor_release_for;
use crate::known_devices::DeviceKind;
use crate::lighting::LightingAvailability;
use crate::lighting_effects::{
    LightingEffectKind, MAXIMUM_CYCLE_SECONDS, MINIMUM_CYCLE_SECONDS, setting_showing_current_frame,
};
use crate::localization::{tr, tr_format};
use crate::profiles::{LightingSetting, Profile, RgbColor};
use crate::system_actions::open_in_default_handler;
use crate::theme::*;
use crate::ui::widgets::*;

const LIGHTING_UNITS: [DeviceKind; 2] = [DeviceKind::Joystick, DeviceKind::Throttle];

pub const LIGHTING_PRESETS: [(&str, RgbColor); 7] = [
    ("Ice cyan", RgbColor::new(0x00, 0xC8, 0xFF)),
    ("Night red", RgbColor::new(0xFF, 0x2A, 0x1F)),
    ("Amber", RgbColor::new(0xFF, 0x9A, 0x1F)),
    ("Green", RgbColor::new(0x27, 0xE3, 0x6A)),
    ("Violet", RgbColor::new(0x8A, 0x3C, 0xFF)),
    ("White", RgbColor::new(0xFF, 0xFF, 0xFF)),
    ("Off", RgbColor::new(0x00, 0x00, 0x00)),
];

/// Colours being edited on the lighting page. They are sent to the hardware live
/// and only written to a profile when the user saves.
pub struct LightingEditor {
    pub linked: bool,
    joystick: LightingSetting,
    throttle: LightingSetting,
    joystick_hex_input: String,
    throttle_hex_input: String,
}

impl LightingEditor {
    pub fn from_profile(profile: &Profile) -> Self {
        Self {
            linked: profile.joystick_lighting == profile.throttle_lighting,
            joystick: profile.joystick_lighting,
            throttle: profile.throttle_lighting,
            joystick_hex_input: profile.joystick_lighting.color.to_hex_string(),
            throttle_hex_input: profile.throttle_lighting.color.to_hex_string(),
        }
    }

    fn setting(&self, device_kind: DeviceKind) -> LightingSetting {
        match device_kind {
            DeviceKind::Throttle => self.throttle,
            _ => self.joystick,
        }
    }

    fn set_setting(&mut self, device_kind: DeviceKind, setting: LightingSetting) {
        let hex_text = setting.color.to_hex_string();
        match device_kind {
            DeviceKind::Throttle => {
                self.throttle = setting;
                self.throttle_hex_input = hex_text;
            }
            _ => {
                self.joystick = setting;
                self.joystick_hex_input = hex_text;
            }
        }
    }

    fn hex_input_mut(&mut self, device_kind: DeviceKind) -> &mut String {
        match device_kind {
            DeviceKind::Throttle => &mut self.throttle_hex_input,
            _ => &mut self.joystick_hex_input,
        }
    }
}

impl PerifeMngrApp {
    pub(crate) fn show_lighting_page(&mut self, ui: &mut Ui) {
        let mut save_clicked = false;
        let mut revert_clicked = false;
        let mut linked = self.lighting_editor.linked;
        page_header(
            ui,
            "RGB · via Logitech DirectOutput",
            "Lighting",
            Some(
                "Each X56 unit has one colour zone. Buttons and axes share it. Changes show on the device right away.",
            ),
            |ui| {
                save_clicked = primary_button(ui, "Save to profile").clicked();
                revert_clicked = secondary_button(ui, "Revert").clicked();
                ui.checkbox(&mut linked, tr("Link stick & throttle"));
            },
        );
        self.lighting_editor.linked = linked;
        if save_clicked {
            self.save_lighting_to_profile();
        }
        if revert_clicked {
            self.apply_current_lighting();
            self.show_status("Lighting restored from the profile.", Tone::Info);
        }

        self.show_lighting_availability_notice(ui);
        self.show_unit_editors(ui);
        ui.add_space(18.0);
        self.show_lighting_automation_card(ui);
    }

    fn show_lighting_availability_notice(&mut self, ui: &mut Ui) {
        let message = match self.services.lighting.availability() {
            LightingAvailability::SoftwareNotInstalled => {
                "Lighting needs the Logitech X56 software (it provides DirectOutput). Install it, then restart this app."
            }
            LightingAvailability::Ready { connected_units } if connected_units.is_empty() => {
                "DirectOutput is running but no X56 unit answered. Check the USB connection."
            }
            LightingAvailability::Failed(_) => {
                "DirectOutput could not be started. Reinstall the Logitech X56 software."
            }
            _ => return,
        };
        let mut open_download_page = false;
        warning_card(ui, |ui| {
            ui.label(RichText::new(message).color(WARNING_BODY_TEXT));
            open_download_page = ui.link(tr("Open the Logitech download page")).clicked();
        });
        if open_download_page {
            open_in_default_handler(vendor_release_for(DeviceKind::Joystick).download_page_url);
        }
        ui.add_space(14.0);
    }

    fn show_unit_editors(&mut self, ui: &mut Ui) {
        let column_count = if ui.available_width() > 760.0 { 2 } else { 1 };
        let mut changed_unit = None;
        for row_units in LIGHTING_UNITS.chunks(column_count) {
            let row_photos: Vec<Option<TextureHandle>> = row_units
                .iter()
                .map(|device_kind| self.product_photo(ui.ctx(), *device_kind))
                .collect();
            ui.columns(column_count, |columns| {
                for ((column, device_kind), photo) in columns.iter_mut().zip(row_units).zip(&row_photos) {
                    let mut setting = self.lighting_editor.setting(*device_kind);
                    let hex_input = self.lighting_editor.hex_input_mut(*device_kind);
                    if show_unit_editor(column, *device_kind, photo.as_ref(), &mut setting, hex_input) {
                        changed_unit = Some((*device_kind, setting));
                    }
                }
            });
            ui.add_space(12.0);
        }
        if let Some((device_kind, setting)) = changed_unit {
            self.apply_edited_lighting(device_kind, setting);
        }
    }

    fn apply_edited_lighting(&mut self, device_kind: DeviceKind, setting: LightingSetting) {
        let affected_units: Vec<DeviceKind> = if self.lighting_editor.linked {
            LIGHTING_UNITS.to_vec()
        } else {
            vec![device_kind]
        };
        for unit in affected_units {
            self.lighting_editor.set_setting(unit, setting);
            self.services.lighting.apply(unit, setting);
        }
    }

    fn save_lighting_to_profile(&mut self) {
        let save_result = {
            let mut state = self.app_state.lock().unwrap();
            let profile_name = state.effective_profile_name();
            if let Some(profile) = state
                .profile_store
                .profiles
                .iter_mut()
                .find(|profile| profile.name == profile_name)
            {
                profile.joystick_lighting = self.lighting_editor.joystick;
                profile.throttle_lighting = self.lighting_editor.throttle;
            }
            state.profile_store.save().map(|_| profile_name)
        };
        match save_result {
            Ok(profile_name) => {
                self.profile_editor.reload_from(&self.app_state.lock().unwrap());
                self.show_status(tr_format("Lighting saved to \"{}\".", &[&profile_name]), Tone::Success);
            }
            Err(error) => self.show_status(tr_format("Could not save the profile: {}", &[&error]), Tone::Danger),
        }
    }

    fn show_lighting_automation_card(&mut self, ui: &mut Ui) {
        card(ui, |ui| {
            card_title(ui, "Automatic lighting");
            ui.add_space(6.0);
            self.setting_checkbox(
                ui,
                "Restore lighting when the app starts",
                |settings| settings.restore_lighting_on_startup,
                |settings, value| settings.restore_lighting_on_startup = value,
            );
            self.setting_checkbox(
                ui,
                "Follow the game profile when a game starts",
                |settings| settings.switch_profiles_for_games,
                |settings, value| settings.switch_profiles_for_games = value,
            );
            self.setting_checkbox(
                ui,
                "Dim the lights when the PC is idle",
                |settings| settings.dim_lighting_when_idle,
                |settings, value| settings.dim_lighting_when_idle = value,
            );
            self.show_idle_dimming_sliders(ui);
        });
    }

    fn show_idle_dimming_sliders(&mut self, ui: &mut Ui) {
        let (mut idle_minutes, mut dimmed_brightness) = {
            let settings = &self.app_state.lock().unwrap().settings;
            (settings.idle_minutes_before_dimming, settings.dimmed_brightness_percent)
        };
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.add_space(28.0);
            muted_text(ui, "After");
            changed |= ui.add(Slider::new(&mut idle_minutes, 1..=60).suffix(" min")).changed();
            muted_text(ui, "dim to");
            changed |= ui
                .add(Slider::new(&mut dimmed_brightness, 0..=100).suffix("%"))
                .changed();
        });
        if changed {
            self.update_settings(|settings| {
                settings.idle_minutes_before_dimming = idle_minutes;
                settings.dimmed_brightness_percent = dimmed_brightness;
            });
        }
    }
}

/// Editor card for one unit. Returns true when the setting changed.
fn show_unit_editor(
    ui: &mut Ui,
    device_kind: DeviceKind,
    photo: Option<&TextureHandle>,
    setting: &mut LightingSetting,
    hex_input: &mut String,
) -> bool {
    let mut changed = false;
    card(ui, |ui| {
        ui.horizontal(|ui| {
            card_title(ui, device_kind.definition().short_name);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                monospace_text(ui, setting.color.to_hex_string());
            });
        });
        ui.add_space(6.0);
        let current_frame = setting_showing_current_frame(*setting, ui.input(|input| input.time));
        device_preview(ui, photo, Some(current_frame), device_kind.definition().short_name);
        ui.add_space(10.0);

        caption(ui, "Effect");
        changed |= show_effect_controls(ui, device_kind, setting);
        ui.add_space(8.0);

        caption(ui, "Presets");
        changed |= show_preset_swatches(ui, setting);
        ui.add_space(8.0);

        caption(ui, "Brightness");
        changed |= ui
            .add(Slider::new(&mut setting.brightness_percent, 0..=100).suffix("%"))
            .changed();
        ui.add_space(8.0);

        caption(ui, "Custom colour");
        changed |= show_custom_color_controls(ui, setting, hex_input);
    });
    changed
}

/// Effect choice, its speed, and the second colour when the effect fades between two.
fn show_effect_controls(ui: &mut Ui, device_kind: DeviceKind, setting: &mut LightingSetting) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        let mut chosen_kind = setting.effect.kind();
        ComboBox::from_id_salt(("lighting-effect", device_kind))
            .selected_text(chosen_kind.label())
            .width(220.0)
            .show_ui(ui, |ui| {
                for kind in LightingEffectKind::ALL {
                    ui.selectable_value(&mut chosen_kind, kind, kind.label());
                }
            });
        if chosen_kind != setting.effect.kind() {
            setting.effect = chosen_kind.build(setting.effect);
            changed = true;
        }
        if let Some(cycle_seconds) = setting.effect.cycle_seconds_mut() {
            muted_text(ui, "Seconds per cycle");
            let speed_slider = Slider::new(cycle_seconds, MINIMUM_CYCLE_SECONDS..=MAXIMUM_CYCLE_SECONDS)
                .logarithmic(true)
                .suffix(" s");
            changed |= ui.add(speed_slider).changed();
        }
        if let Some(second_color) = setting.effect.second_color_mut() {
            muted_text(ui, "Second colour");
            let mut channels = [second_color.red, second_color.green, second_color.blue];
            if egui::color_picker::color_edit_button_srgb(ui, &mut channels).changed() {
                *second_color = RgbColor::new(channels[0], channels[1], channels[2]);
                changed = true;
            }
        }
    });
    changed
}

fn show_preset_swatches(ui: &mut Ui, setting: &mut LightingSetting) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for (preset_name, preset_color) in LIGHTING_PRESETS {
            if color_swatch(ui, preset_color, setting.color == preset_color, preset_name).clicked() {
                setting.color = preset_color;
                changed = true;
            }
        }
    });
    changed
}

fn show_custom_color_controls(ui: &mut Ui, setting: &mut LightingSetting, hex_input: &mut String) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        let mut color_channels = [setting.color.red, setting.color.green, setting.color.blue];
        if egui::color_picker::color_edit_button_srgb(ui, &mut color_channels).changed() {
            setting.color = RgbColor::new(color_channels[0], color_channels[1], color_channels[2]);
            changed = true;
        }
        let hex_response = ui.add(
            egui::TextEdit::singleline(hex_input)
                .desired_width(90.0)
                .font(monospace_font(14.0)),
        );
        if hex_response.changed()
            && let Some(parsed_color) = RgbColor::parse_hex_string(hex_input)
        {
            setting.color = parsed_color;
            changed = true;
        }
        for (channel_label, channel_value) in [
            ("R", &mut setting.color.red),
            ("G", &mut setting.color.green),
            ("B", &mut setting.color.blue),
        ] {
            muted_text(ui, channel_label);
            changed |= ui.add(egui::DragValue::new(channel_value).range(0..=255)).changed();
        }
    });
    if changed {
        *hex_input = setting.color.to_hex_string();
    }
    changed
}
