//! Overview cards for the mouse, keyboard and audio peripherals, in the same style as the HOTAS
//! cards: category, name, product photo, a few facts and a button to the device's page.

use eframe::egui::{TextureHandle, Ui};

use crate::app::{Page, PerifeMngrApp};
use crate::audio_devices::{AudioDirection, AudioEndpoint};
use crate::localization::tr;
use crate::product_images::ProductPhoto;
use crate::ui::widgets::*;

const PERIPHERAL_CARD_MINIMUM_WIDTH: f32 = 290.0;

/// Everything one peripheral card shows.
struct PeripheralCard {
    category: &'static str,
    name: String,
    photo: ProductPhoto,
    facts: Vec<(&'static str, String)>,
    page: Option<Page>,
}

impl ProductPhoto {
    fn category(self) -> &'static str {
        match self {
            ProductPhoto::X56Stick => "Joystick",
            ProductPhoto::X56Throttle => "Throttle",
            ProductPhoto::RudderPedals => "Rudder",
            ProductPhoto::LogitechG309 => "Mouse",
            ProductPhoto::RazerKrakenV4X => "Headset",
            ProductPhoto::HyperXQuadCastS => "Microphone",
            ProductPhoto::VietaProAstra => "Speaker",
            ProductPhoto::NewskillSerikeV2 => "Keyboard",
        }
    }
}

impl PerifeMngrApp {
    pub(crate) fn show_peripheral_cards(&mut self, ui: &mut Ui) {
        let cards = self.collect_peripheral_cards();
        if cards.is_empty() {
            return;
        }
        caption(ui, "Peripherals");
        ui.add_space(6.0);
        let column_count = ((ui.available_width() / PERIPHERAL_CARD_MINIMUM_WIDTH) as usize).clamp(1, 3);
        let mut opened_page = None;
        for row in cards.chunks(column_count) {
            let row_photos: Vec<Option<TextureHandle>> = row
                .iter()
                .map(|card| self.product_photo(ui.ctx(), card.photo))
                .collect();
            ui.columns(column_count, |columns| {
                for ((column, card), photo) in columns.iter_mut().zip(row).zip(&row_photos) {
                    if show_peripheral_card(column, card, photo.as_ref()) {
                        opened_page = card.page;
                    }
                }
            });
            ui.add_space(12.0);
        }
        if let Some(page) = opened_page {
            self.open_page(page);
        }
    }

    fn collect_peripheral_cards(&self) -> Vec<PeripheralCard> {
        let mut cards = Vec::new();
        cards.extend(self.mouse_card());
        cards.extend(self.audio_cards());
        cards.extend(self.keyboard_cards());
        cards
    }

    fn mouse_card(&self) -> Option<PeripheralCard> {
        let mouse = self.services.logitech_mouse.status()?;
        let mut facts = Vec::new();
        if let Some(battery) = &mouse.battery {
            facts.push(("Battery", format!("{} %", battery.percent)));
        }
        if let Some(dpi) = &mouse.dpi {
            facts.push(("DPI", dpi.current.to_string()));
        }
        if let Some(report_rate) = &mouse.report_rate {
            facts.push(("Report rate", format!("{} Hz", report_rate.current_hz)));
        }
        Some(PeripheralCard {
            category: "Mouse",
            photo: ProductPhoto::matching_name(&mouse.name).unwrap_or(ProductPhoto::LogitechG309),
            name: mouse.name,
            facts,
            page: Some(Page::Mouse),
        })
    }

    /// One card per audio device that has a photo; virtual and built-in audio stay on the Audio page.
    fn audio_cards(&self) -> Vec<PeripheralCard> {
        let endpoints = self.services.audio.endpoints();
        let mut cards: Vec<PeripheralCard> = Vec::new();
        for endpoint in &endpoints {
            let Some(photo) = ProductPhoto::matching_name(&endpoint.device_name) else {
                continue;
            };
            if cards.iter().any(|card| card.photo == photo) {
                continue;
            }
            let device_endpoints: Vec<&AudioEndpoint> = endpoints
                .iter()
                .filter(|other| other.device_name == endpoint.device_name)
                .collect();
            cards.push(PeripheralCard {
                category: photo.category(),
                name: endpoint.device_name.clone(),
                photo,
                facts: describe_audio_endpoints(&device_endpoints),
                page: Some(Page::Audio),
            });
        }
        cards
    }

    fn keyboard_cards(&self) -> Vec<PeripheralCard> {
        self.services
            .usb_peripherals
            .connected()
            .into_iter()
            .filter(|peripheral| peripheral.photo == ProductPhoto::NewskillSerikeV2)
            .map(|keyboard| PeripheralCard {
                category: keyboard.photo.category(),
                name: keyboard.name,
                photo: keyboard.photo,
                facts: vec![(
                    "VID:PID",
                    format!("{:04X}:{:04X}", keyboard.vendor_id, keyboard.product_id),
                )],
                page: None,
            })
            .collect()
    }
}

/// Output and microphone volume, marking muted ones and the Windows default.
fn describe_audio_endpoints(endpoints: &[&AudioEndpoint]) -> Vec<(&'static str, String)> {
    endpoints
        .iter()
        .map(|endpoint| {
            let label = match endpoint.direction {
                AudioDirection::Output => "Output",
                AudioDirection::Input => "Microphone",
            };
            let mut value = if endpoint.is_muted {
                tr("Muted").to_owned()
            } else {
                format!("{:.0} %", endpoint.volume * 100.0)
            };
            if endpoint.is_default {
                value.push_str(" · ");
                value.push_str(tr("default"));
            }
            (label, value)
        })
        .collect()
}

/// Draws one card. Returns true when its button was clicked.
fn show_peripheral_card(ui: &mut Ui, peripheral: &PeripheralCard, photo: Option<&TextureHandle>) -> bool {
    let mut open_clicked = false;
    card(ui, |ui| {
        caption(ui, peripheral.category);
        card_title(ui, &peripheral.name);
        ui.add_space(8.0);
        device_preview(ui, photo, None, &peripheral.name);
        ui.add_space(10.0);
        key_value_grid(ui, &format!("peripheral-facts-{}", peripheral.name), &peripheral.facts);
        if peripheral.page.is_some() {
            ui.add_space(10.0);
            ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                open_clicked = secondary_button(ui, "Details").clicked();
            });
        }
    });
    open_clicked
}
