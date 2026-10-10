//! Device details: general information, drivers with update actions, and USB power saving.

use eframe::egui::{self, Align2, RichText, ScrollArea, Sense, Stroke, Ui, vec2};

use crate::app::{DevicePageTab, JoystickManagerApp};
use crate::device_scanner::ConnectedDevice;
use crate::driver_repair::{find_installed_third_party_package, remove_driver_package};
use crate::driver_status::{evaluate_driver_health, installed_package_version, vendor_release_for};
use crate::known_devices::{DeviceKind, KnownDevice};
use crate::localization::{tr, tr_format};
use crate::system_actions::{open_device_manager, open_in_default_handler, restart_device};
use crate::theme::*;
use crate::ui::overview_page::tone_for_health;
use crate::ui::widgets::*;
use crate::usb_power::{PowerSavingState, read_power_saving_state, set_power_saving_enabled};
use crate::windows_update::WindowsUpdateState;

const INFORMATION_CARD_MINIMUM_WIDTH: f32 = 420.0;

impl JoystickManagerApp {
    pub(crate) fn show_device_page(&mut self, ui: &mut Ui, device_kind: DeviceKind) {
        let definition = device_kind.definition();
        let connected_device = self
            .services
            .device_scanner
            .connected_devices()
            .into_iter()
            .find(|device| device.definition.kind == device_kind);

        let header_caption = build_header_caption(definition, connected_device.as_ref());
        let mut restart_clicked = false;
        let mut device_manager_clicked = false;
        page_header(ui, &header_caption, definition.product_name, None, |ui| {
            restart_clicked = enabled_secondary_button(ui, connected_device.is_some(), "Restart device").clicked();
            device_manager_clicked = secondary_button(ui, "Open in Device Manager").clicked();
        });
        if device_manager_clicked {
            open_device_manager();
        }
        if restart_clicked && let Some(device) = &connected_device {
            self.restart_connected_device(device);
        }

        self.show_device_tabs(ui);
        ui.add_space(16.0);

        let Some(device) = connected_device else {
            show_not_connected_card(ui, definition);
            return;
        };
        match self.device_page_tab {
            DevicePageTab::Information => self.show_information_tab(ui, &device),
            DevicePageTab::Drivers => self.show_drivers_tab(ui, &device),
            DevicePageTab::Power => self.show_power_tab(ui, &device),
        }
    }

    fn show_device_tabs(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for (tab, label) in [
                (DevicePageTab::Information, "Info"),
                (DevicePageTab::Drivers, "Drivers"),
                (DevicePageTab::Power, "Power"),
            ] {
                if tab_button(ui, label, self.device_page_tab == tab).clicked() {
                    self.device_page_tab = tab;
                }
            }
        });
        let separator_y = ui.cursor().top();
        let full_width = ui.max_rect().x_range();
        ui.painter().hline(full_width, separator_y, Stroke::new(1.0, BORDER));
    }

    fn restart_connected_device(&mut self, device: &ConnectedDevice) {
        let Some(parent_instance_id) = device.usb_instance_ids().into_iter().min_by_key(String::len) else {
            self.show_status("This device has no USB node to restart.", Tone::Warning);
            return;
        };
        match restart_device(&parent_instance_id) {
            Ok(()) => {
                self.services.device_scanner.request_rescan();
                self.show_status(
                    tr_format("{} restarted.", &[&device.definition.short_name]),
                    Tone::Success,
                );
            }
            Err(error) => self.show_status(tr_format("Restart failed: {}", &[&error]), Tone::Danger),
        }
    }

    fn show_information_tab(&mut self, ui: &mut Ui, device: &ConnectedDevice) {
        let definition = device.definition;
        let snapshot = self.services.input_reader.snapshot(definition.kind).unwrap_or_default();
        let facts = vec![
            ("Product", definition.product_name.to_owned()),
            ("Vendor:Product", definition.vendor_and_product_label()),
            (
                "Serial number",
                device.serial_number.clone().unwrap_or_else(|| "Not reported".into()),
            ),
            ("USB interfaces", device.interfaces.len().to_string()),
            ("Axes", snapshot.axis_values.len().to_string()),
            ("Buttons", snapshot.button_states.len().to_string()),
            ("Hat switches", snapshot.hat_directions.len().to_string()),
            (
                "RGB lighting",
                if definition.supports_rgb_lighting {
                    "Yes, one zone".into()
                } else {
                    "No".into()
                },
            ),
            (
                "Drivers signed",
                if device.all_drivers_signed() {
                    "All".into()
                } else {
                    "Not all".into()
                },
            ),
        ];
        card(ui, |ui| {
            card_title(ui, "Device information");
            ui.add_space(8.0);
            key_value_grid(ui, "device-information", &facts);
        });
    }

    fn show_drivers_tab(&mut self, ui: &mut Ui, device: &ConnectedDevice) {
        if !device.all_interfaces_working() {
            self.show_failed_driver_repair_card(ui, device);
        }
        let column_count = ((ui.available_width() / INFORMATION_CARD_MINIMUM_WIDTH) as usize).clamp(1, 3);
        let power_saving = read_power_saving_state(&device.usb_instance_ids());
        let mut disable_power_saving_clicked = false;
        ui.columns(column_count, |columns| {
            show_driver_package_card(&mut columns[0], device);
            self.show_windows_update_card(&mut columns[1 % column_count], device);
            if power_saving == PowerSavingState::Enabled {
                disable_power_saving_clicked = show_power_recommendation_card(&mut columns[2 % column_count]);
            }
        });
        if disable_power_saving_clicked {
            self.change_power_saving(device, false);
        }
        ui.add_space(18.0);
        show_interfaces_table(ui, device);
    }

    fn show_windows_update_card(&mut self, ui: &mut Ui, device: &ConnectedDevice) {
        let update_state = self.services.windows_update.state();
        let offers = self
            .services
            .windows_update
            .offers_for_hardware_id(&device.definition.hardware_id_fragment());
        let (pill_text, pill_tone) = describe_update_state(&update_state, offers.len());
        let installing_is_allowed = vendor_release_for(device.definition.kind).vendor_driver_required;
        let mut search_clicked = false;
        let mut install_clicked = false;

        card(ui, |ui| {
            ui.horizontal(|ui| {
                caption(ui, "Windows Update");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    status_pill(ui, &pill_text, pill_tone);
                });
            });
            ui.add_space(6.0);
            let explanation = if installing_is_allowed {
                "Searches Windows Update for newer signed drivers that match this device. \
                 Installing runs with the administrator rights this app already has."
            } else {
                "Windows' own driver already supports every axis and button of this device. \
                 The vendor drivers Windows Update offers for it are old and are blocked by Memory \
                 Integrity, so installing them from here is turned off."
            };
            ui.label(RichText::new(tr(explanation)).color(TEXT_SECONDARY));
            for offer in &offers {
                monospace_text(ui, format!("{} · {}", offer.provider, offer.title));
            }
            if let WindowsUpdateState::InstallFinished { message } | WindowsUpdateState::Failed { message } =
                &update_state
            {
                muted_text(ui, message.as_str());
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if update_state.is_busy() {
                    ui.spinner();
                }
                search_clicked =
                    enabled_secondary_button(ui, !update_state.is_busy(), "Search Windows Update").clicked();
                if installing_is_allowed {
                    let can_install = !offers.is_empty() && !update_state.is_busy();
                    install_clicked = enabled_primary_button(ui, can_install, "Install update").clicked();
                }
            });
        });
        if search_clicked {
            self.services.windows_update.start_search();
        }
        if install_clicked {
            let hardware_id_fragment = device.definition.hardware_id_fragment();
            self.services.windows_update.start_install(hardware_id_fragment);
        }
    }

    /// Offered when a driver on the device failed to load, which is what Memory Integrity does to old drivers.
    fn show_failed_driver_repair_card(&mut self, ui: &mut Ui, device: &ConnectedDevice) {
        let blocked_package = device
            .interfaces
            .iter()
            .filter(|interface| !interface.is_working)
            .find_map(|interface| find_installed_third_party_package(&interface.instance_id));
        let mut repair_clicked = false;
        card(ui, |ui| {
            colored_caption(ui, "Driver failed to load", DANGER_TEXT);
            card_title(ui, "Windows blocked this device's driver");
            let explanation = match &blocked_package {
                Some(package) => tr_format(
                    "The installed vendor driver ({}) could not start. Old drivers are usually blocked \
                     by Memory Integrity (Core isolation). Removing it switches the device back to the Windows \
                     driver, which supports every axis and button.",
                    &[package],
                ),
                None => tr(
                    "A driver on this device could not start. Restarting the device or reconnecting it \
                         may help; Device Manager shows the exact error.",
                )
                .to_owned(),
            };
            ui.label(RichText::new(explanation).color(TEXT_SECONDARY));
            ui.add_space(8.0);
            if blocked_package.is_some() {
                repair_clicked = primary_button(ui, "Remove blocked driver and use the Windows driver").clicked();
            }
        });
        if let (true, Some(package)) = (repair_clicked, blocked_package) {
            self.remove_blocked_driver(device, &package);
        }
        ui.add_space(14.0);
    }

    fn remove_blocked_driver(&mut self, device: &ConnectedDevice, package: &str) {
        match remove_driver_package(package) {
            Ok(()) => {
                self.services.device_scanner.request_rescan();
                self.show_status(
                    tr_format(
                        "Removed {}. {} now uses the Windows driver.",
                        &[&package, &device.definition.short_name],
                    ),
                    Tone::Success,
                );
            }
            Err(error) => self.show_status(tr_format("Could not remove {}: {}", &[&package, &error]), Tone::Danger),
        }
    }

    fn show_power_tab(&mut self, ui: &mut Ui, device: &ConnectedDevice) {
        let power_saving = read_power_saving_state(&device.usb_instance_ids());
        let (state_text, state_tone) = match power_saving {
            PowerSavingState::Enabled => ("Power saving on", Tone::Warning),
            PowerSavingState::Disabled => ("Power saving off", Tone::Success),
            PowerSavingState::NotReported => ("Not reported", Tone::Neutral),
        };
        let mut requested_change = None;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                card_title(ui, "USB power saving");
                status_pill(ui, state_text, state_tone);
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(tr(
                    "Windows can switch idle USB devices off to save power. Flight controllers then drop out \
                     or send ghost inputs when they wake up. Turning it off is recommended for HOTAS setups. \
                     The device restarts to apply the change.",
                ))
                .color(TEXT_SECONDARY),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let can_change = power_saving != PowerSavingState::NotReported;
                if enabled_primary_button(
                    ui,
                    can_change && power_saving == PowerSavingState::Enabled,
                    "Turn off power saving",
                )
                .clicked()
                {
                    requested_change = Some(false);
                }
                if enabled_secondary_button(
                    ui,
                    can_change && power_saving == PowerSavingState::Disabled,
                    "Allow power saving",
                )
                .clicked()
                {
                    requested_change = Some(true);
                }
            });
        });
        if let Some(enable) = requested_change {
            self.change_power_saving(device, enable);
        }
    }

    fn change_power_saving(&mut self, device: &ConnectedDevice, enable: bool) {
        let usb_instance_ids = device.usb_instance_ids();
        if let Err(error) = set_power_saving_enabled(&usb_instance_ids, enable) {
            self.show_status(tr_format("Could not change power saving: {}", &[&error]), Tone::Danger);
            return;
        }
        self.restart_connected_device(device);
        let template = if enable {
            "Power saving allowed for {}."
        } else {
            "Power saving turned off for {}."
        };
        self.show_status(tr_format(template, &[&device.definition.short_name]), Tone::Success);
    }
}

fn build_header_caption(definition: &KnownDevice, connected_device: Option<&ConnectedDevice>) -> String {
    let serial_part = connected_device
        .and_then(|device| device.serial_number.as_deref())
        .map(|serial| format!(" · {} {serial}", tr("Serial")))
        .unwrap_or_default();
    format!(
        "{} · {}{serial_part}",
        definition.kind.category_label(),
        definition.vendor_and_product_label()
    )
}

fn tab_button(ui: &mut Ui, label: &str, is_selected: bool) -> egui::Response {
    let text_color = if is_selected { TEXT_PRIMARY } else { TEXT_SECONDARY };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), egui::FontId::proportional(15.0), text_color);
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 32.0, 42.0), Sense::click());
    if is_selected {
        ui.painter()
            .hline(rect.x_range(), rect.max.y - 1.0, Stroke::new(2.0, ACCENT_AMBER));
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(15.0),
        text_color,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn show_not_connected_card(ui: &mut Ui, definition: &KnownDevice) {
    card(ui, |ui| {
        card_title(ui, "Not connected");
        muted_text(
            ui,
            format!(
                "Plug in the {} and it will show up here within a few seconds.",
                definition.product_name
            ),
        );
    });
}

fn show_driver_package_card(ui: &mut Ui, device: &ConnectedDevice) {
    let release = vendor_release_for(device.definition.kind);
    let health = evaluate_driver_health(device);
    let installed_package = installed_package_version(device.definition.kind).unwrap_or_else(|| "Not installed".into());
    card(ui, |ui| {
        ui.horizontal(|ui| {
            caption(ui, "Driver package");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                status_pill(ui, &health.summary(), tone_for_health(&health));
            });
        });
        ui.add_space(4.0);
        ui.label(RichText::new(release.package_name).font(semibold_font(21.0)));
        ui.add_space(4.0);
        let latest_text = match (release.latest_package_version, release.release_date) {
            (Some(version), Some(date)) => format!("{version} · {date}"),
            _ => "Not published".to_owned(),
        };
        key_value_grid(
            ui,
            "driver-package",
            &[
                ("Installed package", installed_package),
                ("Latest known", latest_text),
                (
                    "Game controller driver",
                    device
                        .game_controller_interface()
                        .map(|i| i.driver_version.clone())
                        .unwrap_or_default(),
                ),
            ],
        );
        ui.add_space(8.0);
        let button_label = if health.needs_attention() {
            "Get update from Logitech"
        } else {
            "Open Logitech download page"
        };
        let clicked = if health.needs_attention() {
            primary_button(ui, button_label).clicked()
        } else {
            secondary_button(ui, button_label).clicked()
        };
        if clicked {
            open_in_default_handler(release.download_page_url);
        }
    });
}

/// Returns true when the user asks to turn power saving off.
fn show_power_recommendation_card(ui: &mut Ui) -> bool {
    let mut disable_clicked = false;
    warning_card(ui, |ui| {
        colored_caption(ui, "Power · recommended", WARNING_TEXT);
        card_title(ui, "USB power saving is on");
        ui.label(
            RichText::new(tr(
                "Windows may switch this device off when idle, a frequent cause of X56 disconnects and ghost inputs.",
            ))
            .color(WARNING_BODY_TEXT),
        );
        ui.add_space(8.0);
        disable_clicked = primary_button(ui, "Turn off for this device").clicked();
    });
    disable_clicked
}

fn describe_update_state(update_state: &WindowsUpdateState, offer_count: usize) -> (String, Tone) {
    match update_state {
        WindowsUpdateState::NotChecked => ("Not checked".into(), Tone::Neutral),
        WindowsUpdateState::Searching => ("Searching…".into(), Tone::Info),
        WindowsUpdateState::Installing => ("Installing…".into(), Tone::Info),
        WindowsUpdateState::SearchFinished { .. } if offer_count > 0 => {
            (tr_format("{} update(s) offered", &[&offer_count]), Tone::Warning)
        }
        WindowsUpdateState::SearchFinished { .. } => ("No driver offered".into(), Tone::Success),
        WindowsUpdateState::InstallFinished { .. } => ("Installed".into(), Tone::Success),
        WindowsUpdateState::Failed { .. } => ("Failed".into(), Tone::Danger),
    }
}

fn show_interfaces_table(ui: &mut Ui, device: &ConnectedDevice) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            card_title(ui, "USB interfaces");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let signed_text = if device.all_drivers_signed() {
                    "All drivers signed"
                } else {
                    "Unsigned driver present"
                };
                muted_text(ui, signed_text);
            });
        });
        ui.add_space(8.0);
        ScrollArea::horizontal().show(ui, |ui| {
            egui::Grid::new("interfaces-table")
                .num_columns(7)
                .spacing([26.0, 12.0])
                .striped(false)
                .show(ui, |ui| {
                    for heading in ["Interface", "Role", "Provider", "Version", "Date", "INF", "Status"] {
                        caption(ui, heading);
                    }
                    ui.end_row();
                    for interface in &device.interfaces {
                        monospace_text(ui, interface.interface_label.as_str()).on_hover_text(&interface.device_name);
                        ui.label(interface.role);
                        ui.label(interface.driver_provider.as_str());
                        monospace_text(ui, interface.driver_version.as_str());
                        monospace_text(ui, interface.driver_date.as_str());
                        ui.label(
                            RichText::new(interface.inf_name.as_str())
                                .font(monospace_font(13.0))
                                .color(TEXT_MUTED),
                        );
                        if interface.is_working {
                            status_pill(ui, "OK", Tone::Success);
                        } else {
                            status_pill(ui, "Problem", Tone::Danger);
                        }
                        ui.end_row();
                    }
                });
        });
    });
}
