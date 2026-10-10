//! Keeps the input path responsive: high thread priority and a 1 ms system timer.

use windows::Win32::Media::timeBeginPeriod;
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST};

/// Lets the scheduler run the calling thread ahead of normal work, so input is handled the moment it arrives.
pub fn raise_current_thread_priority() {
    // SAFETY: GetCurrentThread returns a pseudo handle valid for this call.
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
    }
}

/// Makes timed waits (such as the mouse movement tick) wake within about 1 ms instead of 15.6 ms.
pub fn request_one_millisecond_timer_resolution() {
    // SAFETY: plain call; the resolution is held for the lifetime of the process.
    unsafe {
        timeBeginPeriod(1);
    }
}
