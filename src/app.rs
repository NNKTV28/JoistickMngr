//! The main window: owns the background services, routes between pages and handles the tray.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, CentralPanel, Frame, Margin, Panel, ScrollArea, TextureHandle, ViewportCommand};
use tray_icon::TrayIcon;

use crate::app_state::SharedAppState;
use crate::autostart::is_autostart_registered;
use crate::device_scanner::DeviceScannerService;
use crate::input_reader::InputReaderService;
use crate::known_devices::DeviceKind;
use crate::lighting::LightingService;
use crate::localization::tr;
use crate::mapping_engine::MappingEngine;
use crate::product_images::ProductImages;
use crate::settings::AppSettings;
use crate::single_instance::SingleInstanceGuard;
use crate::system_actions::bring_window_to_front;
use crate::theme::{BACKGROUND, BORDER, SIDEBAR_BACKGROUND, apply_cockpit_theme};
use crate::tray::{TrayRequest, create_tray_icon};
use crate::ui::lighting_page::LightingEditor;
use crate::ui::mapping_page::MappingEditor;
use crate::ui::profiles_page::ProfileEditor;
use crate::ui::widgets::Tone;
use crate::updater::UpdateService;
use crate::windows_update::WindowsUpdateService;

pub const WINDOW_TITLE: &str = "Joystick Manager";
const LIVE_PAGE_REFRESH_INTERVAL: Duration = Duration::from_millis(16);
const STATIC_PAGE_REFRESH_INTERVAL: Duration = Duration::from_millis(500);
const STATUS_BANNER_LIFETIME: Duration = Duration::from_secs(6);

/// Tray requests waiting for the next frame to update the app's own state.
type TrayRequestQueue = Arc<Mutex<Vec<TrayRequest>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Overview,
    Device(DeviceKind),
    Lighting,
    InputTester,
    KeyMapping,
    Profiles,
    Settings,
}

impl Page {
    fn shows_live_input(self) -> bool {
        matches!(self, Page::Overview | Page::InputTester | Page::KeyMapping)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevicePageTab {
    Information,
    Drivers,
    Power,
}

pub struct StatusBanner {
    pub message: String,
    pub tone: Tone,
    shown_at: Instant,
}

pub struct BackgroundServices {
    pub mapping_engine: MappingEngine,
    pub device_scanner: DeviceScannerService,
    pub input_reader: InputReaderService,
    pub lighting: LightingService,
    pub windows_update: WindowsUpdateService,
    pub updater: UpdateService,
}

pub struct JoystickManagerApp {
    pub(crate) app_state: SharedAppState,
    pub(crate) services: BackgroundServices,
    pub(crate) current_page: Page,
    pub(crate) device_page_tab: DevicePageTab,
    pub(crate) lighting_editor: LightingEditor,
    pub(crate) mapping_editor: MappingEditor,
    pub(crate) profile_editor: ProfileEditor,
    pub(crate) autostart_registered: bool,
    product_images: ProductImages,
    status_banner: Option<StatusBanner>,
    tray_requests: TrayRequestQueue,
    window_is_visible: bool,
    _tray_icon: Option<TrayIcon>,
    _single_instance: SingleInstanceGuard,
}

impl JoystickManagerApp {
    pub fn new(
        creation_context: &eframe::CreationContext<'_>,
        app_state: SharedAppState,
        services: BackgroundServices,
        single_instance: SingleInstanceGuard,
        starts_visible: bool,
    ) -> Self {
        let egui_context = creation_context.egui_ctx.clone();
        apply_cockpit_theme(&egui_context);

        let tray_requests = TrayRequestQueue::default();
        let respond_to_request = {
            let tray_requests = Arc::clone(&tray_requests);
            let egui_context = egui_context.clone();
            move |request: TrayRequest| respond_to_tray_request(request, &tray_requests, &egui_context)
        };
        let tray_icon = create_tray_icon(respond_to_request.clone()).ok();
        single_instance.listen_for_show_requests(move || respond_to_request(TrayRequest::ShowWindow));

        let effective_profile = app_state.lock().unwrap().effective_profile();
        let profile_editor = ProfileEditor::new(&app_state.lock().unwrap());
        Self {
            lighting_editor: LightingEditor::from_profile(&effective_profile),
            mapping_editor: MappingEditor::default(),
            profile_editor,
            app_state,
            services,
            current_page: Page::Overview,
            device_page_tab: DevicePageTab::Drivers,
            autostart_registered: is_autostart_registered(),
            product_images: ProductImages::start_loading(egui_context.clone()),
            status_banner: None,
            tray_requests,
            window_is_visible: starts_visible,
            _tray_icon: tray_icon,
            _single_instance: single_instance,
        }
    }

    pub(crate) fn show_status(&mut self, message: impl Into<String>, tone: Tone) {
        let message: String = message.into();
        self.status_banner = Some(StatusBanner {
            message: tr(&message).to_owned(),
            tone,
            shown_at: Instant::now(),
        });
    }

    pub(crate) fn open_page(&mut self, page: Page) {
        self.current_page = page;
    }

    /// Logitech's product photo of the device, once downloaded.
    pub(crate) fn product_photo(
        &mut self,
        egui_context: &egui::Context,
        device_kind: DeviceKind,
    ) -> Option<TextureHandle> {
        self.product_images.texture(egui_context, device_kind)
    }

    /// Changes settings and saves them right away, reporting a failed save in the status banner.
    pub(crate) fn update_settings(&mut self, change: impl FnOnce(&mut AppSettings)) {
        let save_result = {
            let mut state = self.app_state.lock().unwrap();
            change(&mut state.settings);
            state.settings.save()
        };
        if let Err(error) = save_result {
            self.show_status(format!("Could not save settings: {error}"), Tone::Danger);
        }
    }

    /// Shows a checkbox bound to one setting and saves the setting when it changes.
    pub(crate) fn setting_checkbox(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        read_setting: impl Fn(&AppSettings) -> bool,
        write_setting: impl FnOnce(&mut AppSettings, bool),
    ) {
        let mut value = read_setting(&self.app_state.lock().unwrap().settings);
        if ui.checkbox(&mut value, tr(label)).changed() {
            self.update_settings(|settings| write_setting(settings, value));
        }
    }

    /// Re-applies whichever profile is in charge, after it was changed or activated.
    pub(crate) fn apply_current_lighting(&mut self) {
        crate::automation::apply_current_lighting(&self.app_state, &self.services.lighting);
        let effective_profile = self.app_state.lock().unwrap().effective_profile();
        self.lighting_editor = LightingEditor::from_profile(&effective_profile);
    }

    fn handle_tray_requests(&mut self, context: &egui::Context) {
        let pending_requests: Vec<TrayRequest> = std::mem::take(&mut *self.tray_requests.lock().unwrap());
        for request in pending_requests {
            match request {
                TrayRequest::ShowWindow => self.show_window(context),
                TrayRequest::Quit => std::process::exit(0),
            }
        }
    }

    fn handle_close_request(&mut self, context: &egui::Context) {
        let close_requested = context.input(|input| input.viewport().close_requested());
        let close_to_tray = self.app_state.lock().unwrap().settings.close_to_tray;
        if close_requested && close_to_tray {
            context.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_window(context);
        }
    }

    fn show_window(&mut self, context: &egui::Context) {
        context.send_viewport_cmd(ViewportCommand::Visible(true));
        context.send_viewport_cmd(ViewportCommand::Minimized(false));
        context.send_viewport_cmd(ViewportCommand::Focus);
        self.window_is_visible = true;
    }

    fn hide_window(&mut self, context: &egui::Context) {
        context.send_viewport_cmd(ViewportCommand::Visible(false));
        self.window_is_visible = false;
    }

    fn schedule_next_refresh(&self, context: &egui::Context) {
        if !self.window_is_visible {
            return;
        }
        let refresh_interval = if self.current_page.shows_live_input() {
            LIVE_PAGE_REFRESH_INTERVAL
        } else {
            STATIC_PAGE_REFRESH_INTERVAL
        };
        context.request_repaint_after(refresh_interval);
    }

    fn show_current_page(&mut self, ui: &mut egui::Ui) {
        match self.current_page {
            Page::Overview => self.show_overview_page(ui),
            Page::Device(device_kind) => self.show_device_page(ui, device_kind),
            Page::Lighting => self.show_lighting_page(ui),
            Page::InputTester => self.show_input_page(ui),
            Page::KeyMapping => self.show_mapping_page(ui),
            Page::Profiles => self.show_profiles_page(ui),
            Page::Settings => self.show_settings_page(ui),
        }
    }

    fn show_status_banner(&mut self, ui: &mut egui::Ui) {
        let banner_expired = self
            .status_banner
            .as_ref()
            .is_some_and(|banner| banner.shown_at.elapsed() > STATUS_BANNER_LIFETIME);
        if banner_expired {
            self.status_banner = None;
        }
        if let Some(banner) = &self.status_banner {
            crate::ui::widgets::status_pill(ui, &banner.message, banner.tone);
            ui.add_space(10.0);
        }
    }
}

/// Runs right when the tray or a second launch asks for something. A hidden eframe window
/// receives no frames, so the window is restored through Win32 here and the queued request lets
/// the next frame bring the app's own state in line.
fn respond_to_tray_request(request: TrayRequest, tray_requests: &TrayRequestQueue, egui_context: &egui::Context) {
    match request {
        TrayRequest::Quit => std::process::exit(0),
        TrayRequest::ShowWindow => {
            bring_window_to_front(WINDOW_TITLE);
            tray_requests.lock().unwrap().push(TrayRequest::ShowWindow);
            egui_context.request_repaint();
        }
    }
}

impl eframe::App for JoystickManagerApp {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_tray_requests(context);
        self.handle_close_request(context);
        self.schedule_next_refresh(context);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        Panel::left("navigation")
            .exact_size(250.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(SIDEBAR_BACKGROUND)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .inner_margin(Margin::symmetric(14, 20)),
            )
            .show(ui, |ui| self.show_navigation(ui));

        CentralPanel::no_frame()
            .frame(Frame::new().fill(BACKGROUND).inner_margin(Margin::symmetric(36, 28)))
            .show(ui, |ui| {
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    self.show_status_banner(ui);
                    self.show_current_page(ui);
                });
            });
    }
}
