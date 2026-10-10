//! Notification area icon with a small menu to reopen or quit the application.
//!
//! The icon belongs to a hidden window created on the UI thread. The UI thread's message loop
//! already delivers messages to every window it owns, so clicks are handled even while the main
//! window is hidden.

use std::sync::OnceLock;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIcon, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow,
    GetCursorPos, HICON, MF_SEPARATOR, MF_STRING, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WINDOW_EX_STYLE, WM_APP, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
    WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::app_icon::render_logo_rgba;
use crate::localization::tr;

const TRAY_ICON_SIZE: u32 = 32;
const TRAY_ICON_ID: u32 = 1;
const TOOLTIP: &str = "Joystick Manager";
/// Message the shell sends to the hidden window when the icon is clicked.
const TRAY_CALLBACK_MESSAGE: u32 = WM_APP + 1;
const OPEN_MENU_COMMAND: u32 = 1;
const QUIT_MENU_COMMAND: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayRequest {
    ShowWindow,
    Quit,
}

type TrayRequestHandler = Box<dyn Fn(TrayRequest) + Send + Sync>;

/// Where the hidden window's procedure sends the user's clicks. Set once, by the only tray icon.
static REQUEST_HANDLER: OnceLock<TrayRequestHandler> = OnceLock::new();
/// The shell broadcasts this message after Explorer restarts, so the icon can be added again.
static TASKBAR_CREATED_MESSAGE: OnceLock<u32> = OnceLock::new();
/// The logo icon handle, kept so the icon can be added again after Explorer restarts.
static LOGO_ICON_HANDLE: OnceLock<usize> = OnceLock::new();

/// The icon in the notification area. Removed when dropped.
pub struct TrayIcon {
    window: HWND,
    icon: HICON,
}

/// Creates the tray icon. `on_request` runs on the UI thread as soon as the user clicks,
/// even while the window is hidden.
pub fn create_tray_icon(on_request: impl Fn(TrayRequest) + Send + Sync + 'static) -> anyhow::Result<TrayIcon> {
    if REQUEST_HANDLER.set(Box::new(on_request)).is_err() {
        anyhow::bail!("the tray icon already exists");
    }
    TASKBAR_CREATED_MESSAGE.get_or_init(|| {
        // SAFETY: registers a message name; the string is a valid static wide string.
        unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) }
    });
    let window = create_hidden_window()?;
    let icon = create_logo_icon()?;
    LOGO_ICON_HANDLE.get_or_init(|| icon.0 as usize);
    add_to_notification_area(window, icon)?;
    Ok(TrayIcon { window, icon })
}

fn add_to_notification_area(window: HWND, icon: HICON) -> anyhow::Result<()> {
    let mut icon_data = icon_data(window);
    icon_data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    icon_data.uCallbackMessage = TRAY_CALLBACK_MESSAGE;
    icon_data.hIcon = icon;
    copy_into_wide_buffer(TOOLTIP, &mut icon_data.szTip);
    // SAFETY: icon_data is fully initialised and refers to a live window and icon.
    unsafe { Shell_NotifyIconW(NIM_ADD, &icon_data) }.ok()?;
    Ok(())
}

/// The fields that identify our icon to the shell.
fn icon_data(window: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: TRAY_ICON_ID,
        ..Default::default()
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        // SAFETY: the window and icon were created by this struct and are released exactly once.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(self.window));
            let _ = DestroyIcon(self.icon);
            let _ = DestroyWindow(self.window);
        }
    }
}

fn create_hidden_window() -> anyhow::Result<HWND> {
    let class_name = w!("NikiCoding.JoystickManager.Tray");
    // SAFETY: the class and window use a static name and a procedure that lives for the whole program.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(tray_window_procedure),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&window_class);
        let window = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Joystick Manager tray"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        Ok(window)
    }
}

/// Builds a Windows icon from the app logo. Windows wants blue, green, red, alpha byte order.
fn create_logo_icon() -> anyhow::Result<HICON> {
    let mut color_pixels = render_logo_rgba(TRAY_ICON_SIZE);
    for pixel in color_pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let transparency_mask = vec![0_u8; (TRAY_ICON_SIZE * TRAY_ICON_SIZE / 8) as usize];
    // SAFETY: both buffers have the sizes Windows expects for a 32-bit icon of this size.
    let icon = unsafe {
        CreateIcon(
            None,
            TRAY_ICON_SIZE as i32,
            TRAY_ICON_SIZE as i32,
            1,
            32,
            transparency_mask.as_ptr(),
            color_pixels.as_ptr(),
        )?
    };
    Ok(icon)
}

extern "system" fn tray_window_procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == TRAY_CALLBACK_MESSAGE {
        handle_icon_click(window, lparam.0 as u32);
        return LRESULT(0);
    }
    if TASKBAR_CREATED_MESSAGE.get() == Some(&message) {
        add_icon_again_after_explorer_restart(window);
        return LRESULT(0);
    }
    // SAFETY: every other message gets the default handling.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn handle_icon_click(window: HWND, mouse_message: u32) {
    let request = match mouse_message {
        WM_LBUTTONUP | WM_LBUTTONDBLCLK => Some(TrayRequest::ShowWindow),
        WM_RBUTTONUP => show_tray_menu(window),
        _ => None,
    };
    if let (Some(request), Some(handler)) = (request, REQUEST_HANDLER.get()) {
        handler(request);
    }
}

/// Shows "Open" and "Quit" at the mouse pointer and returns what the user picked.
fn show_tray_menu(window: HWND) -> Option<TrayRequest> {
    let open_text = HSTRING::from(tr("Open Joystick Manager"));
    let quit_text = HSTRING::from(tr("Quit"));
    // SAFETY: the menu is created, shown and destroyed here; the strings outlive the calls.
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let _ = AppendMenuW(menu, MF_STRING, OPEN_MENU_COMMAND as usize, PCWSTR(open_text.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, QUIT_MENU_COMMAND as usize, PCWSTR(quit_text.as_ptr()));
        let mut pointer = POINT::default();
        let _ = GetCursorPos(&mut pointer);
        // Without this the menu stays open when the user clicks elsewhere.
        let _ = SetForegroundWindow(window);
        let chosen_command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            pointer.x,
            pointer.y,
            None,
            window,
            None,
        );
        let _ = DestroyMenu(menu);
        match chosen_command.0 as u32 {
            OPEN_MENU_COMMAND => Some(TrayRequest::ShowWindow),
            QUIT_MENU_COMMAND => Some(TrayRequest::Quit),
            _ => None,
        }
    }
}

fn add_icon_again_after_explorer_restart(window: HWND) {
    if let Some(&icon_handle) = LOGO_ICON_HANDLE.get() {
        let _ = add_to_notification_area(window, HICON(icon_handle as *mut _));
    }
}

fn copy_into_wide_buffer(text: &str, buffer: &mut [u16]) {
    let wide_text: Vec<u16> = text.encode_utf16().take(buffer.len() - 1).collect();
    buffer[..wide_text.len()].copy_from_slice(&wide_text);
    buffer[wide_text.len()] = 0;
}
