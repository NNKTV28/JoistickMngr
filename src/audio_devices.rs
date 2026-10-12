//! Speakers, headsets and microphones through the Windows audio endpoints: their volume, mute,
//! live level and which one Windows uses by default.
//!
//! A background thread owns all COM objects. It refreshes the list every few seconds, reads the
//! levels many times per second for the meters, and carries out changes the interface asks for.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Devices::FunctionDiscovery::{PKEY_Device_FriendlyName, PKEY_DeviceInterface_FriendlyName};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, EDataFlow, ERole, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture,
    eCommunications, eConsole, eMultimedia, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, STGM_READ,
};
use windows::core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, PCWSTR, interface};

const LEVEL_READ_INTERVAL: Duration = Duration::from_millis(50);
const LIST_REFRESH_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioDirection {
    /// Speakers and headphones.
    Output,
    /// Microphones.
    Input,
}

impl AudioDirection {
    fn data_flow(self) -> EDataFlow {
        match self {
            AudioDirection::Output => eRender,
            AudioDirection::Input => eCapture,
        }
    }
}

/// One playback or recording endpoint, such as "Speakers (Razer Kraken V4 X)".
#[derive(Clone, Debug, PartialEq)]
pub struct AudioEndpoint {
    pub id: String,
    pub direction: AudioDirection,
    /// The endpoint's own name, such as "Speakers".
    pub name: String,
    /// The hardware it belongs to, such as "Razer Kraken V4 X".
    pub device_name: String,
    /// 0..1
    pub volume: f32,
    pub is_muted: bool,
    /// Loudest sample right now, 0..1.
    pub peak_level: f32,
    pub is_default: bool,
}

enum AudioCommand {
    SetVolume { endpoint_id: String, volume: f32 },
    SetMute { endpoint_id: String, is_muted: bool },
    MakeDefault { endpoint_id: String },
}

#[derive(Clone)]
pub struct AudioService {
    endpoints: Arc<Mutex<Vec<AudioEndpoint>>>,
    commands: Sender<AudioCommand>,
}

impl AudioService {
    pub fn start() -> Self {
        let (commands, received_commands) = mpsc::channel();
        let service = Self {
            endpoints: Arc::new(Mutex::new(Vec::new())),
            commands,
        };
        let endpoints = Arc::clone(&service.endpoints);
        std::thread::Builder::new()
            .name("audio-devices".into())
            .spawn(move || run_audio_thread(&endpoints, &received_commands))
            .expect("audio thread starts");
        service
    }

    pub fn endpoints(&self) -> Vec<AudioEndpoint> {
        self.endpoints.lock().unwrap().clone()
    }

    pub fn set_volume(&self, endpoint_id: &str, volume: f32) {
        let _ = self.commands.send(AudioCommand::SetVolume {
            endpoint_id: endpoint_id.to_owned(),
            volume: volume.clamp(0.0, 1.0),
        });
    }

    pub fn set_mute(&self, endpoint_id: &str, is_muted: bool) {
        let _ = self.commands.send(AudioCommand::SetMute {
            endpoint_id: endpoint_id.to_owned(),
            is_muted,
        });
    }

    pub fn make_default(&self, endpoint_id: &str) {
        let _ = self.commands.send(AudioCommand::MakeDefault {
            endpoint_id: endpoint_id.to_owned(),
        });
    }
}

/// An endpoint with the COM objects needed to read and change it.
struct OpenEndpoint {
    volume_control: IAudioEndpointVolume,
    level_meter: IAudioMeterInformation,
    state: AudioEndpoint,
}

fn run_audio_thread(shared_endpoints: &Mutex<Vec<AudioEndpoint>>, commands: &Receiver<AudioCommand>) {
    // SAFETY: initialises COM for this thread only; every COM object below stays on this thread.
    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
        return;
    }
    let Ok(enumerator) = create_device_enumerator() else {
        return;
    };
    let mut open_endpoints = Vec::new();
    let mut last_list_refresh: Option<Instant> = None;
    loop {
        if last_list_refresh.is_none_or(|refreshed| refreshed.elapsed() >= LIST_REFRESH_INTERVAL) {
            open_endpoints = open_active_endpoints(&enumerator);
            last_list_refresh = Some(Instant::now());
        }
        let mut list_needs_refresh = false;
        while let Ok(command) = commands.try_recv() {
            list_needs_refresh |= carry_out(command, &open_endpoints);
        }
        if list_needs_refresh {
            last_list_refresh = None;
            continue;
        }
        read_levels(&mut open_endpoints);
        *shared_endpoints.lock().unwrap() = open_endpoints.iter().map(|endpoint| endpoint.state.clone()).collect();
        std::thread::sleep(LEVEL_READ_INTERVAL);
    }
}

fn create_device_enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    // SAFETY: standard creation of the system audio device enumerator.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

fn open_active_endpoints(enumerator: &IMMDeviceEnumerator) -> Vec<OpenEndpoint> {
    let mut endpoints = Vec::new();
    for direction in [AudioDirection::Output, AudioDirection::Input] {
        let default_id = default_endpoint_id(enumerator, direction);
        endpoints.extend(
            active_devices(enumerator, direction)
                .into_iter()
                .filter_map(|device| open_endpoint(device, direction, default_id.as_deref())),
        );
    }
    endpoints.sort_by(|first, second| {
        (&first.state.device_name, first.state.direction as u8)
            .cmp(&(&second.state.device_name, second.state.direction as u8))
    });
    endpoints
}

fn active_devices(enumerator: &IMMDeviceEnumerator, direction: AudioDirection) -> Vec<IMMDevice> {
    // SAFETY: the collection and its items are COM objects owned by this thread.
    unsafe {
        let Ok(collection) = enumerator.EnumAudioEndpoints(direction.data_flow(), DEVICE_STATE_ACTIVE) else {
            return Vec::new();
        };
        let count = collection.GetCount().unwrap_or(0);
        (0..count).filter_map(|index| collection.Item(index).ok()).collect()
    }
}

fn default_endpoint_id(enumerator: &IMMDeviceEnumerator, direction: AudioDirection) -> Option<String> {
    // SAFETY: asks for the current default endpoint of this direction.
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(direction.data_flow(), eConsole) }.ok()?;
    endpoint_id(&device)
}

fn open_endpoint(device: IMMDevice, direction: AudioDirection, default_id: Option<&str>) -> Option<OpenEndpoint> {
    let id = endpoint_id(&device)?;
    // SAFETY: activates the volume and meter interfaces of a live endpoint.
    let (volume_control, level_meter) = unsafe {
        (
            device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()?,
            device.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None).ok()?,
        )
    };
    let full_name = read_text_property(&device, &PKEY_Device_FriendlyName).unwrap_or_default();
    let device_name =
        read_text_property(&device, &PKEY_DeviceInterface_FriendlyName).unwrap_or_else(|| full_name.clone());
    let state = AudioEndpoint {
        is_default: default_id == Some(id.as_str()),
        id,
        direction,
        name: endpoint_name_without_device(&full_name, &device_name),
        device_name,
        volume: 0.0,
        is_muted: false,
        peak_level: 0.0,
    };
    Some(OpenEndpoint {
        volume_control,
        level_meter,
        state,
    })
}

fn endpoint_id(device: &IMMDevice) -> Option<String> {
    // SAFETY: GetId returns a COM-allocated string that is freed right after copying it.
    unsafe {
        let raw_id = device.GetId().ok()?;
        let id = raw_id.to_string().ok();
        CoTaskMemFree(Some(raw_id.0 as *const _));
        id
    }
}

fn read_text_property(device: &IMMDevice, key: &PROPERTYKEY) -> Option<String> {
    // SAFETY: reads one property from the endpoint's property store.
    let value = unsafe { device.OpenPropertyStore(STGM_READ).ok()?.GetValue(key).ok()? };
    let text = value.to_string();
    (!text.is_empty()).then_some(text)
}

/// "Speakers (Razer Kraken V4 X)" becomes "Speakers" when the device name is shown separately.
fn endpoint_name_without_device(full_name: &str, device_name: &str) -> String {
    full_name
        .strip_suffix(&format!(" ({device_name})"))
        .unwrap_or(full_name)
        .to_owned()
}

fn read_levels(endpoints: &mut [OpenEndpoint]) {
    for endpoint in endpoints {
        // SAFETY: reads values from interfaces activated on this thread.
        unsafe {
            endpoint.state.volume = endpoint.volume_control.GetMasterVolumeLevelScalar().unwrap_or(0.0);
            endpoint.state.is_muted = endpoint.volume_control.GetMute().is_ok_and(|muted| muted.as_bool());
            endpoint.state.peak_level = endpoint.level_meter.GetPeakValue().unwrap_or(0.0);
        }
    }
}

/// Applies one change. Returns true when the list must be read again (the default changed).
fn carry_out(command: AudioCommand, endpoints: &[OpenEndpoint]) -> bool {
    let find = |endpoint_id: &str| endpoints.iter().find(|endpoint| endpoint.state.id == endpoint_id);
    match command {
        AudioCommand::SetVolume { endpoint_id, volume } => {
            if let Some(endpoint) = find(&endpoint_id) {
                // SAFETY: changes the volume of an endpoint owned by this thread.
                let _ = unsafe {
                    endpoint
                        .volume_control
                        .SetMasterVolumeLevelScalar(volume, std::ptr::null())
                };
            }
            false
        }
        AudioCommand::SetMute { endpoint_id, is_muted } => {
            if let Some(endpoint) = find(&endpoint_id) {
                // SAFETY: changes the mute state of an endpoint owned by this thread.
                let _ = unsafe { endpoint.volume_control.SetMute(is_muted, std::ptr::null()) };
            }
            false
        }
        AudioCommand::MakeDefault { endpoint_id } => {
            let _ = make_default_for_every_role(&endpoint_id);
            true
        }
    }
}

/// Windows has no public call for choosing the default device; the sound settings use this
/// interface, and so do the well-known audio switchers. Only the order of the methods matters;
/// the ones before `set_default_endpoint` are listed to keep the layout right.
#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: IUnknown {
    fn get_mix_format(&self, device_id: PCWSTR, format: *mut *mut core::ffi::c_void) -> HRESULT;
    fn get_device_format(&self, device_id: PCWSTR, default: i32, format: *mut *mut core::ffi::c_void) -> HRESULT;
    fn reset_device_format(&self, device_id: PCWSTR) -> HRESULT;
    fn set_device_format(
        &self,
        device_id: PCWSTR,
        endpoint_format: *mut core::ffi::c_void,
        mix_format: *mut core::ffi::c_void,
    ) -> HRESULT;
    fn get_processing_period(
        &self,
        device_id: PCWSTR,
        default: i32,
        default_period: *mut i64,
        minimum_period: *mut i64,
    ) -> HRESULT;
    fn set_processing_period(&self, device_id: PCWSTR, period: *mut i64) -> HRESULT;
    fn get_share_mode(&self, device_id: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
    fn set_share_mode(&self, device_id: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
    fn get_property_value(
        &self,
        device_id: PCWSTR,
        store: i32,
        key: *const PROPERTYKEY,
        value: *mut core::ffi::c_void,
    ) -> HRESULT;
    fn set_property_value(
        &self,
        device_id: PCWSTR,
        store: i32,
        key: *const PROPERTYKEY,
        value: *mut core::ffi::c_void,
    ) -> HRESULT;
    fn set_default_endpoint(&self, device_id: PCWSTR, role: ERole) -> HRESULT;
    fn set_endpoint_visibility(&self, device_id: PCWSTR, visible: i32) -> HRESULT;
}

const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

fn make_default_for_every_role(endpoint_id: &str) -> windows::core::Result<()> {
    let wide_id: Vec<u16> = endpoint_id.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the id is a valid, null-terminated wide string for the duration of the calls.
    unsafe {
        let policy_config: IPolicyConfig = CoCreateInstance(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)?;
        for role in [eConsole, eMultimedia, eCommunications] {
            policy_config
                .set_default_endpoint(PCWSTR(wide_id.as_ptr()), role)
                .ok()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_name_is_taken_off_the_endpoint_name() {
        assert_eq!(
            endpoint_name_without_device("Speakers (Vieta Pro Astra)", "Vieta Pro Astra"),
            "Speakers"
        );
    }

    #[test]
    fn endpoint_name_without_the_device_stays_whole() {
        assert_eq!(
            endpoint_name_without_device("Digital Output", "Realtek"),
            "Digital Output"
        );
    }
}
