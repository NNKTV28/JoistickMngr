//! Controls the X56 RGB lighting through Logitech's DirectOutput SDK, which ships with the X56 software.
//!
//! The SDK is not thread safe, so one background thread owns it and receives commands over a channel.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use libloading::Library;
use windows::core::GUID;
use winreg::RegKey;
use winreg::enums::HKEY_LOCAL_MACHINE;

use crate::known_devices::DeviceKind;
use crate::lighting_effects::{LightingFrame, frame_at};
use crate::profiles::{LightingSetting, Profile};

const DEFAULT_DIRECT_OUTPUT_PATH: &str = r"C:\Program Files\Logitech\DirectOutput\DirectOutput.dll";
const DIRECT_OUTPUT_REGISTRY_KEYS: [&str; 2] = [r"SOFTWARE\Logitech\DirectOutput", r"SOFTWARE\Saitek\DirectOutput"];
const LIGHTING_PAGE_ID: u32 = 1;
const FLAG_SET_AS_ACTIVE_PAGE: u32 = 1;
const BRIGHTNESS_LED_INDEX: u32 = 0;
const COLOR_LED_INDEX: u32 = 1;
const DEVICE_RESCAN_INTERVAL: Duration = Duration::from_secs(4);
/// About 25 frames per second: smooth to the eye without flooding the USB connection.
const ANIMATION_FRAME_INTERVAL: Duration = Duration::from_millis(40);

const X56_STICK_DEVICE_TYPE: GUID = GUID::from_u128(0x40D17E5E_8091_409F_A2B5_24EF1915A812);
const X56_THROTTLE_DEVICE_TYPE: GUID = GUID::from_u128(0xEE9E44A9_46CD_470B_B650_9C5859992240);

type InitializeFunction = unsafe extern "system" fn(plugin_name: *const u16) -> i32;
type DeinitializeFunction = unsafe extern "system" fn() -> i32;
type EnumerateCallback = unsafe extern "system" fn(device_handle: *mut c_void, context: *mut c_void);
type EnumerateFunction = unsafe extern "system" fn(callback: EnumerateCallback, context: *mut c_void) -> i32;
type GetDeviceTypeFunction = unsafe extern "system" fn(device_handle: *mut c_void, device_type: *mut GUID) -> i32;
type AddPageFunction = unsafe extern "system" fn(device_handle: *mut c_void, page: u32, flags: u32) -> i32;
type SetLedFunction = unsafe extern "system" fn(device_handle: *mut c_void, page: u32, index: u32, value: u32) -> i32;

#[derive(Clone, Debug, PartialEq)]
pub enum LightingAvailability {
    Starting,
    Ready { connected_units: Vec<DeviceKind> },
    SoftwareNotInstalled,
    Failed(String),
}

enum LightingCommand {
    Apply {
        device_kind: DeviceKind,
        setting: LightingSetting,
    },
}

#[derive(Clone)]
pub struct LightingService {
    command_sender: Sender<LightingCommand>,
    availability: Arc<Mutex<LightingAvailability>>,
    applied_settings: Arc<Mutex<HashMap<DeviceKind, LightingSetting>>>,
}

impl LightingService {
    pub fn start() -> Self {
        let (command_sender, command_receiver) = mpsc::channel();
        let availability = Arc::new(Mutex::new(LightingAvailability::Starting));
        let applied_settings = Arc::new(Mutex::new(HashMap::new()));

        let thread_availability = Arc::clone(&availability);
        let thread_applied_settings = Arc::clone(&applied_settings);
        std::thread::Builder::new()
            .name("lighting".into())
            .spawn(move || run_lighting_thread(command_receiver, thread_availability, thread_applied_settings))
            .expect("lighting thread starts");

        Self {
            command_sender,
            availability,
            applied_settings,
        }
    }

    pub fn availability(&self) -> LightingAvailability {
        self.availability.lock().unwrap().clone()
    }

    pub fn applied_setting(&self, device_kind: DeviceKind) -> Option<LightingSetting> {
        self.applied_settings.lock().unwrap().get(&device_kind).copied()
    }

    pub fn apply(&self, device_kind: DeviceKind, setting: LightingSetting) {
        let _ = self
            .command_sender
            .send(LightingCommand::Apply { device_kind, setting });
    }

    pub fn apply_profile(&self, profile: &Profile) {
        self.apply(DeviceKind::Joystick, profile.joystick_lighting);
        self.apply(DeviceKind::Throttle, profile.throttle_lighting);
    }

    pub fn apply_profile_dimmed(&self, profile: &Profile, maximum_brightness_percent: u8) {
        let dim = |setting: LightingSetting| setting.with_brightness_capped_at(maximum_brightness_percent);
        self.apply(DeviceKind::Joystick, dim(profile.joystick_lighting));
        self.apply(DeviceKind::Throttle, dim(profile.throttle_lighting));
    }
}

fn run_lighting_thread(
    command_receiver: Receiver<LightingCommand>,
    availability: Arc<Mutex<LightingAvailability>>,
    applied_settings: Arc<Mutex<HashMap<DeviceKind, LightingSetting>>>,
) {
    let Some(library_path) = find_direct_output_library() else {
        *availability.lock().unwrap() = LightingAvailability::SoftwareNotInstalled;
        return;
    };
    let mut direct_output = match DirectOutput::load_and_initialize(&library_path) {
        Ok(direct_output) => direct_output,
        Err(error) => {
            *availability.lock().unwrap() = LightingAvailability::Failed(error.to_string());
            return;
        }
    };

    let animation_started_at = Instant::now();
    let mut last_device_rescan = Instant::now();
    let mut written_frames: HashMap<DeviceKind, LightingFrame> = HashMap::new();
    refresh_and_publish_units(&mut direct_output, &availability);
    loop {
        let wait_time = time_until_next_update(&applied_settings, last_device_rescan);
        match command_receiver.recv_timeout(wait_time) {
            Ok(LightingCommand::Apply { device_kind, setting }) => {
                applied_settings.lock().unwrap().insert(device_kind, setting);
                written_frames.remove(&device_kind);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if last_device_rescan.elapsed() >= DEVICE_RESCAN_INTERVAL {
            refresh_and_publish_units(&mut direct_output, &availability);
            last_device_rescan = Instant::now();
            written_frames.clear();
        }
        let settings_snapshot = applied_settings.lock().unwrap().clone();
        write_changed_frames(
            &direct_output,
            &settings_snapshot,
            animation_started_at.elapsed(),
            &mut written_frames,
        );
    }
}

/// Animated effects need a new frame soon; static lighting only needs the periodic device rescan.
fn time_until_next_update(
    applied_settings: &Mutex<HashMap<DeviceKind, LightingSetting>>,
    last_device_rescan: Instant,
) -> Duration {
    let any_animated = applied_settings
        .lock()
        .unwrap()
        .values()
        .any(|setting| setting.effect.is_animated());
    if any_animated {
        ANIMATION_FRAME_INTERVAL
    } else {
        DEVICE_RESCAN_INTERVAL.saturating_sub(last_device_rescan.elapsed())
    }
}

/// Sends each unit its current frame, skipping units whose LEDs already show it.
fn write_changed_frames(
    direct_output: &DirectOutput,
    settings: &HashMap<DeviceKind, LightingSetting>,
    time_since_start: Duration,
    written_frames: &mut HashMap<DeviceKind, LightingFrame>,
) {
    for (device_kind, setting) in settings {
        let frame = frame_at(*setting, time_since_start);
        if written_frames.get(device_kind) != Some(&frame) {
            direct_output.write_frame(*device_kind, frame);
            written_frames.insert(*device_kind, frame);
        }
    }
}

fn refresh_and_publish_units(direct_output: &mut DirectOutput, availability: &Mutex<LightingAvailability>) {
    direct_output.refresh_connected_units();
    *availability.lock().unwrap() = LightingAvailability::Ready {
        connected_units: direct_output.connected_kinds(),
    };
}

fn find_direct_output_library() -> Option<String> {
    let local_machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let registered_path = DIRECT_OUTPUT_REGISTRY_KEYS.iter().find_map(|key_path| {
        let key = local_machine.open_subkey(key_path).ok()?;
        key.get_value::<String, _>("DirectOutput").ok()
    });
    registered_path
        .into_iter()
        .chain(std::iter::once(DEFAULT_DIRECT_OUTPUT_PATH.to_owned()))
        .find(|path| std::path::Path::new(path).exists())
}

/// Function pointers resolved from DirectOutput.dll. The library field keeps them valid.
struct DirectOutput {
    _library: Library,
    deinitialize: DeinitializeFunction,
    enumerate: EnumerateFunction,
    get_device_type: GetDeviceTypeFunction,
    add_page: AddPageFunction,
    set_led: SetLedFunction,
    unit_handles: HashMap<DeviceKind, usize>,
}

impl DirectOutput {
    fn load_and_initialize(library_path: &str) -> anyhow::Result<Self> {
        // SAFETY: DirectOutput.dll is Logitech's signed SDK; the function signatures match DirectOutput.h.
        unsafe {
            let library = Library::new(library_path)?;
            let initialize = *library.get::<InitializeFunction>(b"DirectOutput_Initialize")?;
            let direct_output = Self {
                deinitialize: *library.get(b"DirectOutput_Deinitialize")?,
                enumerate: *library.get(b"DirectOutput_Enumerate")?,
                get_device_type: *library.get(b"DirectOutput_GetDeviceType")?,
                add_page: *library.get(b"DirectOutput_AddPage")?,
                set_led: *library.get(b"DirectOutput_SetLed")?,
                _library: library,
                unit_handles: HashMap::new(),
            };
            let plugin_name: Vec<u16> = "Joystick Manager\0".encode_utf16().collect();
            let result = initialize(plugin_name.as_ptr());
            anyhow::ensure!(result >= 0, "DirectOutput_Initialize failed with {result:#x}");
            Ok(direct_output)
        }
    }

    fn connected_kinds(&self) -> Vec<DeviceKind> {
        let mut kinds: Vec<DeviceKind> = self.unit_handles.keys().copied().collect();
        kinds.sort_by_key(|kind| *kind as u8);
        kinds
    }

    /// Picks up plugged or unplugged units and restores the last colour on newly found ones.
    /// Picks up plugged or unplugged units and claims a lighting page on newly found ones.
    fn refresh_connected_units(&mut self) {
        let current_handles = self.enumerate_unit_handles();
        for (device_kind, handle) in &current_handles {
            let is_new_unit = self.unit_handles.get(device_kind) != Some(handle);
            if is_new_unit {
                self.claim_lighting_page(*handle);
            }
        }
        self.unit_handles = current_handles;
    }

    fn enumerate_unit_handles(&self) -> HashMap<DeviceKind, usize> {
        let mut raw_handles: Vec<usize> = Vec::new();
        // SAFETY: the callback only runs during this call and receives a pointer to `raw_handles`.
        unsafe {
            (self.enumerate)(
                collect_device_handle,
                &mut raw_handles as *mut Vec<usize> as *mut c_void,
            );
        }
        raw_handles
            .into_iter()
            .filter_map(|handle| Some((self.device_kind_of(handle)?, handle)))
            .collect()
    }

    fn device_kind_of(&self, handle: usize) -> Option<DeviceKind> {
        let mut device_type = GUID::zeroed();
        // SAFETY: handle came from DirectOutput_Enumerate and device_type is a valid out pointer.
        unsafe { (self.get_device_type)(handle as *mut c_void, &mut device_type) };
        match device_type {
            X56_STICK_DEVICE_TYPE => Some(DeviceKind::Joystick),
            X56_THROTTLE_DEVICE_TYPE => Some(DeviceKind::Throttle),
            _ => None,
        }
    }

    fn claim_lighting_page(&self, handle: usize) {
        // SAFETY: handle came from DirectOutput_Enumerate.
        unsafe { (self.add_page)(handle as *mut c_void, LIGHTING_PAGE_ID, FLAG_SET_AS_ACTIVE_PAGE) };
    }

    fn write_frame(&self, device_kind: DeviceKind, frame: LightingFrame) {
        if let Some(handle) = self.unit_handles.get(&device_kind) {
            self.write_led_values(*handle, frame);
        }
    }

    fn write_led_values(&self, handle: usize, frame: LightingFrame) {
        let brightness = frame.brightness_percent.min(100) as u32;
        // SAFETY: handle came from DirectOutput_Enumerate and our page was added to it.
        unsafe {
            (self.set_led)(
                handle as *mut c_void,
                LIGHTING_PAGE_ID,
                BRIGHTNESS_LED_INDEX,
                brightness,
            );
            (self.set_led)(
                handle as *mut c_void,
                LIGHTING_PAGE_ID,
                COLOR_LED_INDEX,
                frame.color.to_colorref(),
            );
        }
    }
}

impl Drop for DirectOutput {
    fn drop(&mut self) {
        // SAFETY: matches the successful DirectOutput_Initialize call.
        unsafe { (self.deinitialize)() };
    }
}

unsafe extern "system" fn collect_device_handle(device_handle: *mut c_void, context: *mut c_void) {
    // SAFETY: context is the `Vec<usize>` passed by `enumerate_unit_handles`.
    let handles = unsafe { &mut *(context as *mut Vec<usize>) };
    handles.push(device_handle as usize);
}
