//! Mouse page: battery, DPI and report rate of a Logitech mouse.

use eframe::egui::{self, RichText, Slider, TextureHandle, Ui, vec2};

use crate::app::PerifeMngrApp;
use crate::localization::{tr, tr_format};
use crate::logitech_mouse::{BatteryStatus, DpiSettings, LogitechMouseStatus, ReportRateSettings};
use crate::product_images::ProductPhoto;
use crate::theme::*;
use crate::ui::widgets::*;

const DPI_PRESETS: [u16; 6] = [400, 800, 1200, 1600, 3200, 6400];
const BATTERY_BAR_WIDTH: f32 = 260.0;
const LOW_BATTERY_PERCENT: u8 = 20;
const PHOTO_WIDTH: f32 = 300.0;

impl PerifeMngrApp {
    pub(crate) fn show_mouse_page(&mut self, ui: &mut Ui) {
        let Some(mouse) = self.services.logitech_mouse.status() else {
            page_header(ui, "Peripherals", "Mouse", None, |_| {});
            card(ui, |ui| {
                muted_text(
                    ui,
                    "No Logitech mouse answers. Make sure it is switched on and its receiver is plugged in.",
                );
            });
            return;
        };
        page_header(ui, "Logitech mouse", &mouse.name, None, |_| {});
        let photo = ProductPhoto::matching_name(&mouse.name).and_then(|photo| self.product_photo(ui.ctx(), photo));
        show_mouse_card(ui, &mouse, photo.as_ref());
        ui.add_space(12.0);
        if let Some(dpi) = &mouse.dpi {
            self.show_dpi_card(ui, dpi);
            ui.add_space(12.0);
        }
        if let Some(report_rate) = &mouse.report_rate {
            self.show_report_rate_card(ui, report_rate);
            ui.add_space(12.0);
        }
        show_onboard_profile_note(ui, &mouse);
    }

    fn show_dpi_card(&mut self, ui: &mut Ui, dpi: &DpiSettings) {
        let mut chosen_dpi = self.mouse_dpi_draft.unwrap_or(dpi.current);
        let mut apply_requested = false;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                card_title(ui, "Sensitivity (DPI)");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    status_pill(ui, &tr_format("Now {} DPI", &[&dpi.current]), Tone::Info);
                });
            });
            muted_text(ui, "Higher DPI moves the pointer further for the same hand movement.");
            ui.add_space(8.0);
            apply_requested |= show_dpi_slider(ui, &dpi.choices, &mut chosen_dpi);
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for preset in DPI_PRESETS.iter().filter(|preset| dpi.choices.contains(preset)) {
                    if ui.selectable_label(chosen_dpi == *preset, preset.to_string()).clicked() {
                        chosen_dpi = *preset;
                        apply_requested = true;
                    }
                }
            });
        });
        self.mouse_dpi_draft = Some(chosen_dpi).filter(|draft| *draft != dpi.current);
        if apply_requested && chosen_dpi != dpi.current {
            self.services.logitech_mouse.set_dpi(chosen_dpi);
            self.show_status(tr_format("DPI set to {}.", &[&chosen_dpi]), Tone::Success);
        }
    }

    fn show_report_rate_card(&mut self, ui: &mut Ui, report_rate: &ReportRateSettings) {
        let mut chosen_rate = None;
        card(ui, |ui| {
            card_title(ui, "Report rate");
            muted_text(
                ui,
                "How often the mouse tells the PC where it is. Higher is smoother and uses more battery.",
            );
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                for rate_hz in &report_rate.choices_hz {
                    let is_current = *rate_hz == report_rate.current_hz;
                    if ui.selectable_label(is_current, format!("{rate_hz} Hz")).clicked() && !is_current {
                        chosen_rate = Some(*rate_hz);
                    }
                }
            });
        });
        if let Some(rate_hz) = chosen_rate {
            self.services.logitech_mouse.set_report_rate(rate_hz);
            self.show_status(tr_format("Report rate set to {} Hz.", &[&rate_hz]), Tone::Success);
        }
    }
}

/// Product photo beside the battery level.
fn show_mouse_card(ui: &mut Ui, mouse: &LogitechMouseStatus, photo: Option<&TextureHandle>) {
    card(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.allocate_ui(vec2(PHOTO_WIDTH, 0.0), |ui| {
                ui.set_width(PHOTO_WIDTH);
                device_preview(ui, photo, None, &mouse.name);
            });
            ui.add_space(24.0);
            ui.vertical(|ui| match &mouse.battery {
                Some(battery) => show_battery(ui, battery),
                None => {
                    muted_text(ui, "The mouse does not report its battery.");
                }
            });
        });
    });
}

fn show_battery(ui: &mut Ui, battery: &BatteryStatus) {
    ui.horizontal(|ui| {
        card_title(ui, "Battery");
        if battery.is_charging {
            status_pill(ui, "Charging", Tone::Success);
        }
    });
    ui.add_space(6.0);
    ui.label(
        RichText::new(format!("{} %", battery.percent))
            .font(condensed_font(30.0))
            .color(TEXT_PRIMARY),
    );
    let bar_color = if battery.percent <= LOW_BATTERY_PERCENT {
        WARNING_TEXT
    } else {
        SUCCESS_TEXT
    };
    horizontal_level_bar(ui, f64::from(battery.percent) / 100.0, BATTERY_BAR_WIDTH, bar_color);
}

/// A slider over the supported values. Returns true when the user lets go of it.
fn show_dpi_slider(ui: &mut Ui, choices: &[u16], chosen_dpi: &mut u16) -> bool {
    if choices.is_empty() {
        return false;
    }
    let mut position = nearest_choice_position(choices, *chosen_dpi);
    let value_label = RichText::new(format!("{} DPI", choices[position])).font(monospace_font(14.0));
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().slider_width = 420.0;
            ui.add(
                Slider::new(&mut position, 0..=choices.len() - 1)
                    .show_value(false)
                    .text(value_label),
            )
        })
        .inner;
    *chosen_dpi = choices[position];
    response.drag_stopped() || (response.changed() && !response.dragged())
}

/// Position of the supported value closest to `dpi`.
fn nearest_choice_position(choices: &[u16], dpi: u16) -> usize {
    choices
        .iter()
        .enumerate()
        .min_by_key(|(_, choice)| choice.abs_diff(dpi))
        .map_or(0, |(position, _)| position)
}

fn show_onboard_profile_note(ui: &mut Ui, mouse: &LogitechMouseStatus) {
    let Some(profile) = mouse.active_onboard_profile else {
        return;
    };
    card(ui, |ui| {
        card_title(ui, "Onboard profile");
        muted_text(
            ui,
            tr_format(
                "The mouse runs from its own profile {}. Changes here apply straight away; pressing the DPI button switches back to the profile's DPI levels.",
                &[&profile],
            ),
        );
        ui.label(
            RichText::new(tr("Profiles stay stored in the mouse."))
                .size(13.0)
                .color(TEXT_MUTED),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_choice_snaps_to_a_supported_value() {
        let choices = [400, 800, 1600];
        assert_eq!(nearest_choice_position(&choices, 1000), 1);
        assert_eq!(nearest_choice_position(&choices, 5000), 2);
    }
}
