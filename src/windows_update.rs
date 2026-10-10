//! Searches Windows Update for newer drivers for the known peripherals and installs them.
//!
//! The Windows Update Agent is a COM API whose objects cannot move between threads, so every
//! search and install runs start to finish on its own worker thread.

use std::sync::{Arc, Mutex};

use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize};
use windows::Win32::System::UpdateAgent::{
    ISearchResult, IUpdate, IUpdateCollection, IUpdateSession, IWindowsDriverUpdate, UpdateCollection, UpdateSession,
    orcSucceeded, orcSucceededWithErrors,
};
use windows::core::{BSTR, Interface};

use crate::known_devices::KNOWN_DEVICES;

const DRIVER_SEARCH_CRITERIA: &str = "IsInstalled=0 and Type='Driver'";
const CLIENT_APPLICATION_ID: &str = "Joystick Manager";

#[derive(Clone, Debug, PartialEq)]
pub struct DriverUpdateOffer {
    pub title: String,
    pub hardware_id: String,
    pub provider: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum WindowsUpdateState {
    #[default]
    NotChecked,
    Searching,
    SearchFinished {
        offers: Vec<DriverUpdateOffer>,
    },
    Installing,
    InstallFinished {
        message: String,
    },
    Failed {
        message: String,
    },
}

impl WindowsUpdateState {
    pub fn is_busy(&self) -> bool {
        matches!(self, WindowsUpdateState::Searching | WindowsUpdateState::Installing)
    }
}

#[derive(Clone, Default)]
pub struct WindowsUpdateService {
    state: Arc<Mutex<WindowsUpdateState>>,
}

impl WindowsUpdateService {
    pub fn state(&self) -> WindowsUpdateState {
        self.state.lock().unwrap().clone()
    }

    pub fn offers_for_hardware_id(&self, hardware_id_fragment: &str) -> Vec<DriverUpdateOffer> {
        match self.state() {
            WindowsUpdateState::SearchFinished { offers } => offers
                .into_iter()
                .filter(|offer| offer.hardware_id.to_uppercase().contains(hardware_id_fragment))
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn start_search(&self) {
        if self.state().is_busy() {
            return;
        }
        *self.state.lock().unwrap() = WindowsUpdateState::Searching;
        let state = Arc::clone(&self.state);
        std::thread::spawn(move || {
            let new_state = match run_with_com(search_driver_offers) {
                Ok(offers) => WindowsUpdateState::SearchFinished { offers },
                Err(error) => WindowsUpdateState::Failed {
                    message: format!("Windows Update search failed: {error}"),
                },
            };
            *state.lock().unwrap() = new_state;
        });
    }

    /// Installs the offered drivers for one device only, identified by its `VID_xxxx&PID_xxxx` fragment.
    pub fn start_install(&self, hardware_id_fragment: String) {
        if self.state().is_busy() {
            return;
        }
        *self.state.lock().unwrap() = WindowsUpdateState::Installing;
        let state = Arc::clone(&self.state);
        std::thread::spawn(move || {
            let new_state = match run_with_com(|| install_driver_offers(&hardware_id_fragment)) {
                Ok(message) => WindowsUpdateState::InstallFinished { message },
                Err(error) => WindowsUpdateState::Failed {
                    message: format!("Driver install failed: {error}"),
                },
            };
            *state.lock().unwrap() = new_state;
        });
    }
}

fn run_with_com<T>(work: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    // SAFETY: COM is initialised and released on this worker thread only.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let result = work();
    unsafe { CoUninitialize() };
    result
}

fn search_driver_offers() -> anyhow::Result<Vec<DriverUpdateOffer>> {
    let session = create_update_session()?;
    let matching_updates = find_matching_driver_updates(&session, is_for_any_known_device)?;
    matching_updates.iter().map(describe_driver_update).collect()
}

fn install_driver_offers(hardware_id_fragment: &str) -> anyhow::Result<String> {
    let session = create_update_session()?;
    let matching_updates =
        find_matching_driver_updates(&session, |hardware_id| hardware_id.contains(hardware_id_fragment))?;
    if matching_updates.is_empty() {
        return Ok("No driver updates were offered for this device.".into());
    }
    let update_collection = build_update_collection(&matching_updates)?;
    download_updates(&session, &update_collection)?;
    install_updates(&session, &update_collection)
}

fn create_update_session() -> anyhow::Result<IUpdateSession> {
    // SAFETY: standard COM activation of the Windows Update Agent session object.
    unsafe {
        let session: IUpdateSession = CoCreateInstance(&UpdateSession, None, CLSCTX_ALL)?;
        session.SetClientApplicationID(&BSTR::from(CLIENT_APPLICATION_ID))?;
        Ok(session)
    }
}

/// Driver updates Windows Update offers whose upper-case hardware id passes `is_wanted`.
fn find_matching_driver_updates(
    session: &IUpdateSession,
    is_wanted: impl Fn(&str) -> bool,
) -> anyhow::Result<Vec<IUpdate>> {
    // SAFETY: plain COM calls on interfaces owned by this thread.
    unsafe {
        let searcher = session.CreateUpdateSearcher()?;
        let search_result: ISearchResult = searcher.Search(&BSTR::from(DRIVER_SEARCH_CRITERIA))?;
        let updates = search_result.Updates()?;
        let mut matching_updates = Vec::new();
        for index in 0..updates.Count()? {
            let update = updates.get_Item(index)?;
            let hardware_id = driver_hardware_id(&update).unwrap_or_default().to_uppercase();
            if is_wanted(&hardware_id) {
                matching_updates.push(update);
            }
        }
        Ok(matching_updates)
    }
}

fn is_for_any_known_device(hardware_id: &str) -> bool {
    KNOWN_DEVICES
        .iter()
        .any(|device| hardware_id.contains(&device.hardware_id_fragment()))
}

fn driver_hardware_id(update: &IUpdate) -> anyhow::Result<String> {
    // SAFETY: the cast only succeeds for driver updates, which expose IWindowsDriverUpdate.
    unsafe { Ok(update.cast::<IWindowsDriverUpdate>()?.DriverHardwareID()?.to_string()) }
}

fn describe_driver_update(update: &IUpdate) -> anyhow::Result<DriverUpdateOffer> {
    // SAFETY: plain COM property reads.
    unsafe {
        let driver_update = update.cast::<IWindowsDriverUpdate>()?;
        Ok(DriverUpdateOffer {
            title: update.Title()?.to_string(),
            hardware_id: driver_update.DriverHardwareID()?.to_string(),
            provider: driver_update.DriverProvider()?.to_string(),
        })
    }
}

fn build_update_collection(updates: &[IUpdate]) -> anyhow::Result<IUpdateCollection> {
    // SAFETY: standard COM activation and collection calls.
    unsafe {
        let collection: IUpdateCollection = CoCreateInstance(&UpdateCollection, None, CLSCTX_ALL)?;
        for update in updates {
            if !update.EulaAccepted()?.as_bool() {
                update.AcceptEula()?;
            }
            collection.Add(update)?;
        }
        Ok(collection)
    }
}

fn download_updates(session: &IUpdateSession, updates: &IUpdateCollection) -> anyhow::Result<()> {
    // SAFETY: plain COM calls on interfaces owned by this thread.
    unsafe {
        let downloader = session.CreateUpdateDownloader()?;
        downloader.SetUpdates(updates)?;
        let result_code = downloader.Download()?.ResultCode()?;
        anyhow::ensure!(
            result_code == orcSucceeded || result_code == orcSucceededWithErrors,
            "download finished with result code {}",
            result_code.0
        );
        Ok(())
    }
}

fn install_updates(session: &IUpdateSession, updates: &IUpdateCollection) -> anyhow::Result<String> {
    // SAFETY: plain COM calls on interfaces owned by this thread.
    unsafe {
        let installer = session.CreateUpdateInstaller()?;
        installer.SetUpdates(updates)?;
        let installation_result = installer.Install()?;
        let result_code = installation_result.ResultCode()?;
        anyhow::ensure!(
            result_code == orcSucceeded || result_code == orcSucceededWithErrors,
            "installation finished with result code {}",
            result_code.0
        );
        let restart_note = if installation_result.RebootRequired()?.as_bool() {
            " Restart Windows to finish."
        } else {
            ""
        };
        Ok(format!(
            "Installed {} driver update(s).{restart_note}",
            updates.Count()?
        ))
    }
}
