//! Keeps one copy of the application running. A second launch asks the first one to show its window.

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForSingleObject,
};
use windows::core::w;

pub struct SingleInstanceGuard {
    instance_mutex: usize,
    show_window_event: usize,
}

/// Returns a guard when this is the first running copy. Otherwise signals the running copy and returns `None`.
pub fn acquire_single_instance() -> Option<SingleInstanceGuard> {
    // SAFETY: named kernel objects with static names; handles are closed in Drop.
    unsafe {
        let instance_mutex = CreateMutexW(None, true, w!("Local\\NikiCoding.PerifeMngr.Instance")).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(instance_mutex);
            signal_running_instance_to_show();
            return None;
        }
        let show_window_event = CreateEventW(None, false, false, w!("Local\\NikiCoding.PerifeMngr.ShowWindow")).ok()?;
        Some(SingleInstanceGuard {
            instance_mutex: instance_mutex.0 as usize,
            show_window_event: show_window_event.0 as usize,
        })
    }
}

fn signal_running_instance_to_show() {
    // SAFETY: the event is opened only to set it and closed right after.
    unsafe {
        if let Ok(show_window_event) =
            OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\NikiCoding.PerifeMngr.ShowWindow"))
        {
            let _ = SetEvent(show_window_event);
            let _ = CloseHandle(show_window_event);
        }
    }
}

impl SingleInstanceGuard {
    /// Calls `on_show_requested` every time another launch of the application is attempted.
    pub fn listen_for_show_requests(&self, on_show_requested: impl Fn() + Send + 'static) {
        let show_window_event = self.show_window_event;
        std::thread::Builder::new()
            .name("single-instance".into())
            .spawn(move || {
                loop {
                    // SAFETY: the event handle stays open for the lifetime of the process.
                    unsafe { WaitForSingleObject(HANDLE(show_window_event as *mut _), INFINITE) };
                    on_show_requested();
                }
            })
            .expect("single instance listener starts");
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        // SAFETY: both handles were created by `acquire_single_instance`.
        unsafe {
            let _ = CloseHandle(HANDLE(self.show_window_event as *mut _));
            let _ = CloseHandle(HANDLE(self.instance_mutex as *mut _));
        }
    }
}
