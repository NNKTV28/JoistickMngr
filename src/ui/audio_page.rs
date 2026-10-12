//! Audio page: one card per speaker, headset or microphone with volume, mute, a live level meter
//! and the default device switch for each of its outputs and inputs.

use eframe::egui::{self, RichText, Slider, Ui};

use crate::app::PerifeMngrApp;
use crate::audio_devices::{AudioDirection, AudioEndpoint, AudioService};
use crate::localization::tr;
use crate::theme::*;
use crate::ui::widgets::*;

const LEVEL_METER_WIDTH: f32 = 150.0;
const VOLUME_SLIDER_WIDTH: f32 = 180.0;

impl PerifeMngrApp {
    pub(crate) fn show_audio_page(&mut self, ui: &mut Ui) {
        page_header(
            ui,
            "Peripherals",
            "Audio",
            Some("Volume, mute and the default device for every speaker, headset and microphone."),
            |_| {},
        );
        let endpoints = self.services.audio.endpoints();
        if endpoints.is_empty() {
            card(ui, |ui| {
                muted_text(ui, "No speakers, headsets or microphones found.");
            });
            return;
        }
        for (device_name, device_endpoints) in group_by_device(&endpoints) {
            show_device_card(ui, device_name, &device_endpoints, &self.services.audio);
            ui.add_space(12.0);
        }
    }
}

/// Endpoints of the same hardware, in the order the service lists them.
fn group_by_device(endpoints: &[AudioEndpoint]) -> Vec<(&str, Vec<&AudioEndpoint>)> {
    let mut groups: Vec<(&str, Vec<&AudioEndpoint>)> = Vec::new();
    for endpoint in endpoints {
        match groups.iter_mut().find(|(name, _)| *name == endpoint.device_name) {
            Some((_, group)) => group.push(endpoint),
            None => groups.push((endpoint.device_name.as_str(), vec![endpoint])),
        }
    }
    groups
}

fn show_device_card(ui: &mut Ui, device_name: &str, endpoints: &[&AudioEndpoint], audio: &AudioService) {
    card(ui, |ui| {
        card_title(ui, device_name);
        ui.add_space(6.0);
        egui::Grid::new(("audio-device", device_name))
            .num_columns(6)
            .spacing([16.0, 10.0])
            .min_row_height(32.0)
            .show(ui, |ui| {
                for endpoint in endpoints {
                    show_endpoint_row(ui, endpoint, audio);
                    ui.end_row();
                }
            });
    });
}

fn show_endpoint_row(ui: &mut Ui, endpoint: &AudioEndpoint, audio: &AudioService) {
    let direction_label = match endpoint.direction {
        AudioDirection::Output => "Output",
        AudioDirection::Input => "Input",
    };
    caption(ui, direction_label);
    ui.label(RichText::new(&endpoint.name).color(TEXT_PRIMARY));
    show_volume_slider(ui, endpoint, audio);
    show_mute_switch(ui, endpoint, audio);
    let meter_color = if endpoint.is_muted { TEXT_MUTED } else { LIVE_CYAN };
    horizontal_level_bar(ui, f64::from(endpoint.peak_level), LEVEL_METER_WIDTH, meter_color);
    show_default_switch(ui, endpoint, audio);
}

fn show_volume_slider(ui: &mut Ui, endpoint: &AudioEndpoint, audio: &AudioService) {
    let mut volume_percent = (endpoint.volume * 100.0).round();
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = VOLUME_SLIDER_WIDTH;
        if ui
            .add(
                Slider::new(&mut volume_percent, 0.0..=100.0)
                    .suffix(" %")
                    .fixed_decimals(0),
            )
            .changed()
        {
            audio.set_volume(&endpoint.id, volume_percent / 100.0);
        }
    });
}

fn show_mute_switch(ui: &mut Ui, endpoint: &AudioEndpoint, audio: &AudioService) {
    let mut is_muted = endpoint.is_muted;
    if ui.checkbox(&mut is_muted, tr("Muted")).changed() {
        audio.set_mute(&endpoint.id, is_muted);
    }
}

fn show_default_switch(ui: &mut Ui, endpoint: &AudioEndpoint, audio: &AudioService) {
    if endpoint.is_default {
        status_pill(ui, "Default", Tone::Success);
    } else if secondary_button(ui, "Make default").clicked() {
        audio.make_default(&endpoint.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(device_name: &str, direction: AudioDirection) -> AudioEndpoint {
        AudioEndpoint {
            id: format!("{device_name}-{direction:?}"),
            direction,
            name: String::new(),
            device_name: device_name.to_owned(),
            volume: 0.5,
            is_muted: false,
            peak_level: 0.0,
            is_default: false,
        }
    }

    #[test]
    fn outputs_and_inputs_of_one_device_share_a_card() {
        let endpoints = [
            endpoint("Razer Kraken V4 X", AudioDirection::Output),
            endpoint("Razer Kraken V4 X", AudioDirection::Input),
            endpoint("Vieta Pro Astra", AudioDirection::Output),
        ];
        let groups = group_by_device(&endpoints);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].1.len(), 2);
    }
}
