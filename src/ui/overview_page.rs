//! Overview: one card per device plus a strip of live axis meters.

use eframe::egui::Ui;

use crate::app::{JoystickManagerApp, Page};
use crate::device_scanner::{ConnectedDevice, ScanState};
use crate::driver_status::{DriverHealth, evaluate_driver_health};
use crate::known_devices::{DeviceKind, KnownDevice};
use crate::profiles::LightingSetting;
use crate::ui::widgets::*;

const DEVICE_CARD_MINIMUM_WIDTH: f32 = 290.0;
const AXIS_METER_WIDTH: f32 = 230.0;

impl JoystickManagerApp {
    pub(crate) fn show_overview_page(&mut self, ui: &mut Ui) {
        let scan_state = self.services.device_scanner.scan_state();
        let connected_devices = self.services.device_scanner.connected_devices();
        let subtitle = describe_scan_state(&scan_state, &connected_devices);

        let mut rescan_clicked = false;
        let mut check_updates_clicked = false;
        page_header(ui, "Hangar", "Overview", Some(&subtitle), |ui| {
            check_updates_clicked = primary_button(ui, "Check for driver updates").clicked();
            rescan_clicked = secondary_button(ui, "Rescan").clicked();
        });
        if rescan_clicked {
            self.services.device_scanner.request_rescan();
            self.show_status("Rescanning devices…", Tone::Info);
        }
        if check_updates_clicked {
            self.services.windows_update.start_search();
            self.open_page(Page::Device(DeviceKind::Joystick));
            self.device_page_tab = crate::app::DevicePageTab::Drivers;
        }

        self.show_device_cards(ui, &connected_devices);
        ui.add_space(18.0);
        self.show_live_axes_card(ui, &connected_devices);
    }

    fn show_device_cards(&mut self, ui: &mut Ui, connected_devices: &[ConnectedDevice]) {
        let column_count = ((ui.available_width() / DEVICE_CARD_MINIMUM_WIDTH) as usize).clamp(1, 3);
        let mut opened_page = None;
        for row_kinds in DeviceKind::ALL.chunks(column_count) {
            ui.columns(column_count, |columns| {
                for (column, device_kind) in columns.iter_mut().zip(row_kinds) {
                    let connected_device = connected_devices
                        .iter()
                        .find(|device| device.definition.kind == *device_kind);
                    let lighting = self.displayed_lighting(*device_kind);
                    if show_device_card(column, device_kind.definition(), connected_device, lighting) {
                        opened_page = Some(Page::Device(*device_kind));
                    }
                }
            });
            ui.add_space(12.0);
        }
        if let Some(page) = opened_page {
            self.open_page(page);
        }
    }

    /// The colour last sent to the unit, or the colour the current profile asks for.
    pub(crate) fn displayed_lighting(&self, device_kind: DeviceKind) -> Option<LightingSetting> {
        self.services.lighting.applied_setting(device_kind).or_else(|| {
            self.app_state
                .lock()
                .unwrap()
                .effective_profile()
                .lighting_for(device_kind)
        })
    }

    fn show_live_axes_card(&mut self, ui: &mut Ui, connected_devices: &[ConnectedDevice]) {
        let mut open_tester = false;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                card_title(ui, "Live axes");
                ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                    open_tester = ui.link("Open input tester").clicked();
                });
            });
            ui.add_space(8.0);
            if connected_devices.is_empty() {
                muted_text(ui, "Connect a device to see its axes move here.");
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 26.0;
                for device in connected_devices {
                    self.show_axis_meters_for(ui, device.definition);
                }
            });
        });
        if open_tester {
            self.open_page(Page::InputTester);
        }
    }

    fn show_axis_meters_for(&self, ui: &mut Ui, definition: &KnownDevice) {
        let Some(snapshot) = self.services.input_reader.snapshot(definition.kind) else {
            return;
        };
        for (axis_index, value) in snapshot.axis_values.iter().enumerate() {
            let label = format!(
                "{} · {}",
                definition.kind.category_label(),
                definition.axis_name(axis_index)
            );
            let value_text = format_axis_value(*value, definition.axis_rests_at_center(axis_index));
            labeled_axis_meter(ui, &label, &value_text, *value, AXIS_METER_WIDTH);
        }
    }
}

fn describe_scan_state(scan_state: &ScanState, connected_devices: &[ConnectedDevice]) -> String {
    match scan_state {
        ScanState::NotScannedYet => "Looking for devices…".to_owned(),
        ScanState::Failed(error) => format!("Device scan failed: {error}"),
        ScanState::Scanned(_) => {
            let signed_note = if connected_devices.iter().all(ConnectedDevice::all_drivers_signed) {
                "all drivers signed"
            } else {
                "some drivers are not signed"
            };
            format!(
                "{} of {} devices connected · {signed_note}",
                connected_devices.len(),
                DeviceKind::ALL.len()
            )
        }
    }
}

/// Draws one device card. Returns true when "Details" was clicked.
fn show_device_card(
    ui: &mut Ui,
    definition: &KnownDevice,
    connected_device: Option<&ConnectedDevice>,
    lighting: Option<LightingSetting>,
) -> bool {
    let mut details_clicked = false;
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                caption(ui, definition.kind.category_label());
                card_title(ui, definition.product_name);
            });
            ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                match connected_device {
                    Some(_) => status_pill(ui, "Connected", Tone::Success),
                    None => status_pill(ui, "Not connected", Tone::Neutral),
                };
            });
        });
        ui.add_space(6.0);

        let glow_color = lighting.filter(|_| definition.supports_rgb_lighting);
        match glow_color {
            Some(setting) => {
                lighting_glow_preview(ui, setting.color, setting.brightness_percent, definition.short_name)
            }
            None => lighting_glow_preview(
                ui,
                crate::profiles::RgbColor::new(0x2A, 0x32, 0x3A),
                40,
                definition.short_name,
            ),
        }
        ui.add_space(6.0);

        key_value_grid(
            ui,
            definition.short_name,
            &build_device_facts(definition, connected_device, lighting),
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if let Some(device) = connected_device {
                let health = evaluate_driver_health(device);
                status_pill(ui, &health.summary(), tone_for_health(&health));
            }
            ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                details_clicked = secondary_button(ui, "Details").clicked();
            });
        });
    });
    details_clicked
}

fn build_device_facts(
    definition: &KnownDevice,
    connected_device: Option<&ConnectedDevice>,
    lighting: Option<LightingSetting>,
) -> Vec<(&'static str, String)> {
    let serial_number = connected_device
        .and_then(|device| device.serial_number.clone())
        .unwrap_or_else(|| "Not reported".into());
    let driver = connected_device
        .and_then(|device| device.game_controller_interface())
        .map(|interface| format!("{} {}", interface.driver_provider, interface.driver_version))
        .unwrap_or_else(|| "—".into());
    let lighting_text = match (definition.supports_rgb_lighting, lighting) {
        (false, _) => "None".to_owned(),
        (true, Some(setting)) => format!("{} · {}%", setting.color.to_hex_string(), setting.brightness_percent),
        (true, None) => "Not set".to_owned(),
    };
    vec![
        ("VID:PID", definition.vendor_and_product_label()),
        ("Serial", serial_number),
        ("Driver", driver),
        ("Lighting", lighting_text),
    ]
}

pub fn tone_for_health(health: &DriverHealth) -> Tone {
    match health {
        DriverHealth::UpToDate { .. } => Tone::Success,
        DriverHealth::UsingWindowsDriver => Tone::Info,
        DriverHealth::Outdated { .. } | DriverHealth::VendorDriverMissing => Tone::Warning,
        DriverHealth::DeviceProblem => Tone::Danger,
    }
}
