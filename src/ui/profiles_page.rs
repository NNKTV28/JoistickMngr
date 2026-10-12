//! Profiles: create, edit, import, export and activate saved setups.

use std::path::{Path, PathBuf};

use eframe::egui::{self, CornerRadius, Frame, Margin, RichText, Sense, Slider, Stroke, Ui, vec2};

use crate::app::PerifeMngrApp;
use crate::app_state::AppState;
use crate::known_devices::DeviceKind;
use crate::localization::{tr, tr_format};
use crate::profiles::{Profile, RgbColor};
use crate::storage::{read_json_file, write_json_file};
use crate::system_actions::open_in_default_handler;
use crate::theme::*;
use crate::ui::widgets::*;

const LOGITECH_PROFILE_FOLDER: &str = r"C:\Users\Public\Documents\Logitech\X56";
const LOGITECH_PROFILE_EXTENSION: &str = "pr0";
const PROFILE_LIST_WIDTH: f32 = 280.0;

/// The profile currently open in the editor and the unsaved changes made to it.
pub struct ProfileEditor {
    selected_name: String,
    draft: Profile,
}

impl ProfileEditor {
    pub fn new(state: &AppState) -> Self {
        let profile = state.effective_profile();
        Self {
            selected_name: profile.name.clone(),
            draft: profile,
        }
    }

    /// Reloads the open profile from the store, dropping unsaved edits.
    pub fn reload_from(&mut self, state: &AppState) {
        let stored_profile = state.profile_store.find_by_name(&self.selected_name).cloned();
        match stored_profile {
            Some(profile) => self.draft = profile,
            None => *self = Self::new(state),
        }
    }

    fn select(&mut self, profile: &Profile) {
        self.selected_name = profile.name.clone();
        self.draft = profile.clone();
    }
}

enum ProfileAction {
    Select(Profile),
    CreateNew,
    Duplicate,
    Delete,
    Save,
    Activate,
    Import,
    Export,
}

impl PerifeMngrApp {
    pub(crate) fn show_profiles_page(&mut self, ui: &mut Ui) {
        let mut requested_action = None;
        page_header(ui, "Saved setups", "Profiles", None, |ui| {
            if primary_button(ui, "New profile").clicked() {
                requested_action = Some(ProfileAction::CreateNew);
            }
            if secondary_button(ui, "Export").clicked() {
                requested_action = Some(ProfileAction::Export);
            }
            if secondary_button(ui, "Import").clicked() {
                requested_action = Some(ProfileAction::Import);
            }
        });

        let (profiles, active_name, has_unsaved_changes) = {
            let state = self.app_state.lock().unwrap();
            let stored_draft = state.profile_store.find_by_name(&self.profile_editor.selected_name);
            (
                state.profile_store.profiles.clone(),
                state.effective_profile_name(),
                stored_draft != Some(&self.profile_editor.draft),
            )
        };

        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(PROFILE_LIST_WIDTH);
                if let Some(action) = self.show_profile_list(ui, &profiles, &active_name) {
                    requested_action = Some(action);
                }
                show_logitech_profiles_card(ui);
            });
            ui.add_space(18.0);
            ui.vertical(|ui| {
                if let Some(action) = self.show_profile_editor(ui, profiles.len(), has_unsaved_changes) {
                    requested_action = Some(action);
                }
            });
        });

        if let Some(action) = requested_action {
            self.run_profile_action(action);
        }
    }

    fn show_profile_list(&self, ui: &mut Ui, profiles: &[Profile], active_name: &str) -> Option<ProfileAction> {
        let mut action = None;
        for profile in profiles {
            let is_selected = profile.name == self.profile_editor.selected_name;
            if profile_list_row(ui, profile, is_selected, profile.name == active_name).clicked() {
                action = Some(ProfileAction::Select(profile.clone()));
            }
            ui.add_space(6.0);
        }
        ui.add_space(8.0);
        action
    }

    fn show_profile_editor(
        &mut self,
        ui: &mut Ui,
        profile_count: usize,
        has_unsaved_changes: bool,
    ) -> Option<ProfileAction> {
        let mut action = None;
        card(ui, |ui| {
            self.show_profile_identity_fields(ui);
            ui.add_space(12.0);
            caption(ui, "Lighting");
            for device_kind in [DeviceKind::Joystick, DeviceKind::Throttle] {
                show_compact_lighting_editor(ui, device_kind, &mut self.profile_editor.draft);
            }
        });
        ui.add_space(14.0);
        card(ui, |ui| {
            card_title(ui, "Axis tuning");
            muted_text(
                ui,
                "Applied to axes mapped to the mouse or a virtual controller, and previewed in the Input tester.",
            );
            ui.add_space(8.0);
            show_axis_tuning_table(ui, &mut self.profile_editor.draft);
        });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if enabled_primary_button(ui, has_unsaved_changes, "Save changes").clicked() {
                action = Some(ProfileAction::Save);
            }
            if primary_button(ui, "Activate profile").clicked() {
                action = Some(ProfileAction::Activate);
            }
            if secondary_button(ui, "Duplicate").clicked() {
                action = Some(ProfileAction::Duplicate);
            }
            if enabled_secondary_button(ui, profile_count > 1, "Delete").clicked() {
                action = Some(ProfileAction::Delete);
            }
        });
        action
    }

    fn show_profile_identity_fields(&mut self, ui: &mut Ui) {
        let draft = &mut self.profile_editor.draft;
        egui::Grid::new("profile-identity")
            .num_columns(2)
            .spacing([16.0, 10.0])
            .show(ui, |ui| {
                caption(ui, "Name");
                ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(320.0));
                ui.end_row();

                caption(ui, "Activate when running");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.game_executable_name)
                            .hint_text(tr("game.exe — leave empty for manual only"))
                            .font(monospace_font(14.0))
                            .desired_width(320.0),
                    );
                    if secondary_button(ui, "Browse…").clicked()
                        && let Some(executable_name) = pick_game_executable_name()
                    {
                        draft.game_executable_name = executable_name;
                    }
                });
                ui.end_row();
            });
    }

    fn run_profile_action(&mut self, action: ProfileAction) {
        let outcome = match action {
            ProfileAction::Select(profile) => {
                self.profile_editor.select(&profile);
                Ok(None)
            }
            ProfileAction::CreateNew => self.create_profile(),
            ProfileAction::Duplicate => self.duplicate_selected_profile(),
            ProfileAction::Delete => self.delete_selected_profile(),
            ProfileAction::Save => self.save_selected_profile(),
            ProfileAction::Activate => self.activate_selected_profile(),
            ProfileAction::Import => self.import_profile(),
            ProfileAction::Export => self.export_selected_profile(),
        };
        match outcome {
            Ok(Some(message)) => self.show_status(message, Tone::Success),
            Ok(None) => {}
            Err(error) => self.show_status(error.to_string(), Tone::Danger),
        }
    }

    fn create_profile(&mut self) -> anyhow::Result<Option<String>> {
        let mut state = self.app_state.lock().unwrap();
        let new_profile = Profile::named(&state.profile_store.unused_name_like("New profile"));
        state.profile_store.profiles.push(new_profile.clone());
        state.profile_store.save()?;
        self.profile_editor.select(&new_profile);
        Ok(Some(tr_format("Created \"{}\".", &[&new_profile.name])))
    }

    fn duplicate_selected_profile(&mut self) -> anyhow::Result<Option<String>> {
        let mut state = self.app_state.lock().unwrap();
        let mut copy = self.profile_editor.draft.clone();
        copy.name = state.profile_store.unused_name_like(&format!("{} copy", copy.name));
        state.profile_store.profiles.push(copy.clone());
        state.profile_store.save()?;
        self.profile_editor.select(&copy);
        Ok(Some(tr_format("Created \"{}\".", &[&copy.name])))
    }

    fn delete_selected_profile(&mut self) -> anyhow::Result<Option<String>> {
        let deleted_name = self.profile_editor.selected_name.clone();
        {
            let mut state = self.app_state.lock().unwrap();
            anyhow::ensure!(
                state.profile_store.profiles.len() > 1,
                "The last profile cannot be deleted."
            );
            state
                .profile_store
                .profiles
                .retain(|profile| profile.name != deleted_name);
            if state.settings.active_profile_name == deleted_name {
                state.settings.active_profile_name = state.profile_store.profiles[0].name.clone();
            }
            rename_mode_profile(&mut state.settings.profiles_by_mode, &deleted_name, None);
            state.settings.save()?;
            state.profile_store.save()?;
            self.profile_editor = ProfileEditor::new(&state);
        }
        self.apply_current_lighting();
        Ok(Some(tr_format("Deleted \"{}\".", &[&deleted_name])))
    }

    fn save_selected_profile(&mut self) -> anyhow::Result<Option<String>> {
        let mut draft = self.profile_editor.draft.clone();
        draft.name = draft.name.trim().to_owned();
        draft.game_executable_name = draft.game_executable_name.trim().to_owned();
        anyhow::ensure!(!draft.name.is_empty(), "A profile needs a name.");
        {
            let mut state = self.app_state.lock().unwrap();
            let original_name = self.profile_editor.selected_name.clone();
            let name_taken_by_other = draft.name != original_name && state.profile_store.contains_name(&draft.name);
            anyhow::ensure!(
                !name_taken_by_other,
                "Another profile is already called \"{}\".",
                draft.name
            );

            if let Some(stored) = state
                .profile_store
                .profiles
                .iter_mut()
                .find(|profile| profile.name == original_name)
            {
                *stored = draft.clone();
            }
            if state.settings.active_profile_name == original_name {
                state.settings.active_profile_name = draft.name.clone();
            }
            rename_mode_profile(&mut state.settings.profiles_by_mode, &original_name, Some(&draft.name));
            state.settings.save()?;
            state.profile_store.save()?;
        }
        self.profile_editor.select(&draft);
        self.apply_current_lighting();
        Ok(Some(tr_format("Saved \"{}\".", &[&draft.name])))
    }

    fn activate_selected_profile(&mut self) -> anyhow::Result<Option<String>> {
        let selected_name = self.profile_editor.selected_name.clone();
        self.update_settings(|settings| settings.active_profile_name = selected_name.clone());
        self.apply_current_lighting();
        Ok(Some(tr_format("\"{}\" is now the active profile.", &[&selected_name])))
    }

    fn import_profile(&mut self) -> anyhow::Result<Option<String>> {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PerifeMngr profile", &["json"])
            .pick_file()
        else {
            return Ok(None);
        };
        let mut imported: Profile =
            read_json_file(&path).ok_or_else(|| anyhow::anyhow!("{} is not a valid profile file.", path.display()))?;
        let mut state = self.app_state.lock().unwrap();
        imported.name = state.profile_store.unused_name_like(&imported.name);
        state.profile_store.profiles.push(imported.clone());
        state.profile_store.save()?;
        self.profile_editor.select(&imported);
        Ok(Some(tr_format("Imported \"{}\".", &[&imported.name])))
    }

    fn export_selected_profile(&mut self) -> anyhow::Result<Option<String>> {
        let profile = self.profile_editor.draft.clone();
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PerifeMngr profile", &["json"])
            .set_file_name(format!("{}.json", profile.name))
            .save_file()
        else {
            return Ok(None);
        };
        write_json_file(&path, &profile)?;
        Ok(Some(tr_format("Exported to {}.", &[&path.display()])))
    }
}

fn profile_list_row(ui: &mut Ui, profile: &Profile, is_selected: bool, is_active: bool) -> egui::Response {
    let (background, border) = if is_selected {
        (SELECTED_BACKGROUND, ACCENT_AMBER)
    } else {
        (CARD_BACKGROUND, BORDER)
    };
    let response = Frame::new()
        .fill(background)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (color_bar, _) = ui.allocate_exact_size(vec2(10.0, 36.0), Sense::hover());
                ui.painter().rect_filled(
                    color_bar,
                    CornerRadius::same(3),
                    to_color32(profile.joystick_lighting.color),
                );
                ui.vertical(|ui| {
                    ui.label(RichText::new(&profile.name).font(semibold_font(15.0)));
                    let trigger_text = if profile.is_triggered_by_game() {
                        tr_format("When {} runs", &[&profile.game_executable_name])
                    } else {
                        tr("Manual").to_owned()
                    };
                    ui.label(RichText::new(trigger_text).size(13.0).color(TEXT_MUTED));
                });
                if is_active {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        colored_caption(ui, "Active", SUCCESS_TEXT);
                    });
                }
            });
        })
        .response;
    response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn show_compact_lighting_editor(ui: &mut Ui, device_kind: DeviceKind, profile: &mut Profile) {
    let Some(mut setting) = profile.lighting_for(device_kind) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.add_sized([110.0, 24.0], egui::Label::new(device_kind.definition().short_name));
        let mut color_channels = [setting.color.red, setting.color.green, setting.color.blue];
        if egui::color_picker::color_edit_button_srgb(ui, &mut color_channels).changed() {
            setting.color = RgbColor::new(color_channels[0], color_channels[1], color_channels[2]);
        }
        monospace_text(ui, setting.color.to_hex_string());
        ui.add(Slider::new(&mut setting.brightness_percent, 0..=100).suffix("%"));
    });
    if Some(setting) != profile.lighting_for(device_kind) {
        profile.set_lighting_for(device_kind, setting);
    }
}

fn show_axis_tuning_table(ui: &mut Ui, profile: &mut Profile) {
    egui::ScrollArea::horizontal().show(ui, |ui| {
        egui::Grid::new("axis-tuning")
            .num_columns(7)
            .spacing([20.0, 8.0])
            .show(ui, |ui| {
                for heading in [
                    "Axis",
                    "Device",
                    "Deadzone",
                    "Curve",
                    "Saturation",
                    "Invert",
                    "Response",
                ] {
                    caption(ui, heading);
                }
                ui.end_row();
                for device_kind in DeviceKind::ALL {
                    let definition = device_kind.definition();
                    for axis_index in 0..definition.axes.len() {
                        let tuning = profile.tuning_for_axis_mut(device_kind, axis_index);
                        ui.label(definition.axis_name(axis_index));
                        muted_text(ui, definition.short_name);
                        ui.add(
                            egui::DragValue::new(&mut tuning.deadzone_percent)
                                .range(0.0..=30.0)
                                .speed(0.2)
                                .suffix("%"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut tuning.curve_strength)
                                .range(0.0..=1.0)
                                .speed(0.01),
                        );
                        ui.add(
                            egui::DragValue::new(&mut tuning.saturation_percent)
                                .range(50.0..=100.0)
                                .speed(0.2)
                                .suffix("%"),
                        );
                        ui.checkbox(&mut tuning.inverted, "");
                        tuning_curve_preview(ui, tuning);
                        ui.end_row();
                    }
                }
            });
    });
    remove_neutral_tunings(profile);
}

/// Keeps saved profiles small: untouched axes are not stored.
fn remove_neutral_tunings(profile: &mut Profile) {
    profile.axis_tunings.retain(|tuning| {
        let neutral = crate::profiles::AxisTuning::neutral(tuning.device_kind, tuning.axis_index);
        *tuning != neutral
    });
}

fn show_logitech_profiles_card(ui: &mut Ui) {
    let logitech_profiles = find_logitech_profiles(Path::new(LOGITECH_PROFILE_FOLDER));
    if logitech_profiles.is_empty() {
        return;
    }
    card_frame().inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        caption(ui, "Logitech X56 profiles found");
        for profile_path in &logitech_profiles {
            ui.label(
                RichText::new(profile_path.display().to_string())
                    .font(monospace_font(12.0))
                    .color(TEXT_SECONDARY),
            );
            if ui.link(tr("Open with Logitech profiler")).clicked() {
                open_in_default_handler(&profile_path.to_string_lossy());
            }
        }
    });
}

fn find_logitech_profiles(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case(LOGITECH_PROFILE_EXTENSION))
        })
        .collect()
}

fn pick_game_executable_name() -> Option<String> {
    let path = rfd::FileDialog::new().add_filter("Programs", &["exe"]).pick_file()?;
    Some(path.file_name()?.to_string_lossy().into_owned())
}

/// Keeps the mode switch pointing at a profile after it is renamed (`Some(new name)`) or deleted (`None`).
fn rename_mode_profile(profiles_by_mode: &mut [Option<String>], old_name: &str, new_name: Option<&str>) {
    for chosen_profile in profiles_by_mode.iter_mut() {
        if chosen_profile.as_deref() == Some(old_name) {
            *chosen_profile = new_name.map(str::to_owned);
        }
    }
}
