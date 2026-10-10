//! Left sidebar: brand, page links, device links with connection dots and the active profile.

use eframe::egui::{self, CornerRadius, Frame, Margin, RichText, Sense, Stroke, Ui, vec2};

use crate::app::{JoystickManagerApp, Page};
use crate::known_devices::DeviceKind;
use crate::lighting::LightingAvailability;
use crate::theme::*;
use crate::ui::widgets::{caption, muted_text, navigation_item};

const ACTIVE_PROFILE_CARD_HEIGHT: f32 = 96.0;

impl JoystickManagerApp {
    pub(crate) fn show_navigation(&mut self, ui: &mut Ui) {
        show_brand(ui);
        self.show_navigation_link(ui, "Overview", Page::Overview);

        ui.add_space(14.0);
        caption(ui, "Devices");
        let connected_kinds: Vec<DeviceKind> = self
            .services
            .device_scanner
            .connected_devices()
            .iter()
            .map(|device| device.definition.kind)
            .collect();
        for device_kind in DeviceKind::ALL {
            let dot_color = if connected_kinds.contains(&device_kind) {
                SUCCESS_DOT
            } else {
                OFFLINE_DOT
            };
            let page = Page::Device(device_kind);
            if navigation_item(
                ui,
                device_kind.definition().short_name,
                self.current_page == page,
                Some(dot_color),
            )
            .clicked()
            {
                self.open_page(page);
            }
        }

        ui.add_space(14.0);
        caption(ui, "Tools");
        self.show_navigation_link(ui, "Lighting", Page::Lighting);
        self.show_navigation_link(ui, "Input tester", Page::InputTester);
        self.show_navigation_link(ui, "Profiles", Page::Profiles);
        self.show_navigation_link(ui, "Settings", Page::Settings);

        ui.add_space((ui.available_height() - ACTIVE_PROFILE_CARD_HEIGHT).max(12.0));
        self.show_active_profile_card(ui);
    }

    fn show_navigation_link(&mut self, ui: &mut Ui, label: &str, page: Page) {
        if navigation_item(ui, label, self.current_page == page, None).clicked() {
            self.open_page(page);
        }
    }

    fn show_active_profile_card(&mut self, ui: &mut Ui) {
        let (profile_name, started_by_game) = {
            let state = self.app_state.lock().unwrap();
            (state.effective_profile_name(), state.game_profile_name.is_some())
        };
        let lighting_status = describe_lighting_status(&self.services.lighting.availability());
        Frame::new()
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                show_active_profile_details(ui, &profile_name, started_by_game, lighting_status);
            });
    }
}

fn show_active_profile_details(
    ui: &mut Ui,
    profile_name: &str,
    started_by_game: bool,
    lighting_status: (&str, egui::Color32),
) {
    caption(
        ui,
        if started_by_game {
            "Game profile"
        } else {
            "Active profile"
        },
    );
    ui.label(RichText::new(profile_name).font(semibold_font(15.0)));
    ui.horizontal(|ui| {
        muted_text(ui, "Lighting ·");
        ui.label(RichText::new(lighting_status.0).size(13.5).color(lighting_status.1));
    });
}

fn show_brand(ui: &mut Ui) {
    ui.horizontal(|ui| {
        let (logo_rect, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
        paint_crosshair_logo(ui, logo_rect);
        ui.label(
            RichText::new("JOYSTICK MNGR")
                .font(condensed_font(21.0))
                .extra_letter_spacing(1.5),
        );
    });
    ui.add_space(18.0);
}

fn paint_crosshair_logo(ui: &Ui, rect: egui::Rect) {
    let painter = ui.painter();
    let stroke = Stroke::new(2.0, ACCENT_AMBER);
    let center = rect.center();
    painter.circle_stroke(center, 11.0, stroke);
    painter.circle_filled(center, 2.5, ACCENT_AMBER);
    for (start, end) in [
        ((0.0, -14.0), (0.0, -6.0)),
        ((0.0, 6.0), (0.0, 14.0)),
        ((-14.0, 0.0), (-6.0, 0.0)),
        ((6.0, 0.0), (14.0, 0.0)),
    ] {
        painter.line_segment([center + vec2(start.0, start.1), center + vec2(end.0, end.1)], stroke);
    }
}

fn describe_lighting_status(availability: &LightingAvailability) -> (&'static str, egui::Color32) {
    match availability {
        LightingAvailability::Ready { connected_units } if !connected_units.is_empty() => ("Ready", SUCCESS_TEXT),
        LightingAvailability::Ready { .. } => ("No X56 found", WARNING_TEXT),
        LightingAvailability::Starting => ("Starting", TEXT_MUTED),
        LightingAvailability::SoftwareNotInstalled => ("Needs X56 software", WARNING_TEXT),
        LightingAvailability::Failed(_) => ("Unavailable", DANGER_TEXT),
    }
}
