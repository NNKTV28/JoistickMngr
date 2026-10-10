//! Sends synthetic keyboard and mouse input to whatever window has focus.
//!
//! Events are collected in a batch and handed to Windows in one `SendInput` call, so everything a
//! single controller report causes arrives together and without extra system calls. Keys are sent as
//! scan codes, which DirectInput games read, rather than as virtual keys.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC_EX, MOUSE_EVENT_FLAGS, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, MapVirtualKeyW, SendInput,
};

use crate::bindings::{MouseButtonKind, WheelDirection};

const EXTENDED_SCAN_CODE_PREFIX: u32 = 0xE000;
const WHEEL_NOTCH: i32 = 120;
const BACK_BUTTON_DATA: i32 = 1;
const FORWARD_BUTTON_DATA: i32 = 2;

#[derive(Default)]
pub struct InputBatch {
    inputs: Vec<INPUT>,
}

impl InputBatch {
    pub fn add_key(&mut self, virtual_key: u16, is_pressed: bool) {
        // SAFETY: plain conversion call with no pointers.
        let scan_code = unsafe { MapVirtualKeyW(u32::from(virtual_key), MAPVK_VK_TO_VSC_EX) };
        let mut flags = KEYEVENTF_SCANCODE;
        if scan_code & EXTENDED_SCAN_CODE_PREFIX == EXTENDED_SCAN_CODE_PREFIX {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !is_pressed {
            flags |= KEYEVENTF_KEYUP;
        }
        self.add_keyboard_event((scan_code & 0xFF) as u16, flags);
    }

    pub fn add_mouse_button(&mut self, button: MouseButtonKind, is_pressed: bool) {
        let (flags, extra_data) = match (button, is_pressed) {
            (MouseButtonKind::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
            (MouseButtonKind::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
            (MouseButtonKind::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
            (MouseButtonKind::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
            (MouseButtonKind::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
            (MouseButtonKind::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
            (MouseButtonKind::Back, true) => (MOUSEEVENTF_XDOWN, BACK_BUTTON_DATA),
            (MouseButtonKind::Back, false) => (MOUSEEVENTF_XUP, BACK_BUTTON_DATA),
            (MouseButtonKind::Forward, true) => (MOUSEEVENTF_XDOWN, FORWARD_BUTTON_DATA),
            (MouseButtonKind::Forward, false) => (MOUSEEVENTF_XUP, FORWARD_BUTTON_DATA),
        };
        self.add_mouse_event(0, 0, extra_data, flags);
    }

    pub fn add_wheel_notch(&mut self, direction: WheelDirection) {
        let amount = match direction {
            WheelDirection::Up => WHEEL_NOTCH,
            WheelDirection::Down => -WHEEL_NOTCH,
        };
        self.add_mouse_event(0, 0, amount, MOUSEEVENTF_WHEEL);
    }

    pub fn add_mouse_movement(&mut self, horizontal_pixels: i32, vertical_pixels: i32) {
        if horizontal_pixels != 0 || vertical_pixels != 0 {
            self.add_mouse_event(horizontal_pixels, vertical_pixels, 0, MOUSEEVENTF_MOVE);
        }
    }

    /// Hands every collected event to Windows at once.
    pub fn send(self) {
        if self.inputs.is_empty() {
            return;
        }
        // SAFETY: the slice holds fully initialised INPUT structures of the declared size.
        unsafe { SendInput(&self.inputs, size_of::<INPUT>() as i32) };
    }

    fn add_keyboard_event(&mut self, scan_code: u16, flags: KEYBD_EVENT_FLAGS) {
        self.inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: Default::default(),
                    wScan: scan_code,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }

    fn add_mouse_event(&mut self, horizontal: i32, vertical: i32, extra_data: i32, flags: MOUSE_EVENT_FLAGS) {
        self.inputs.push(INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: horizontal,
                    dy: vertical,
                    mouseData: extra_data as u32,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }
}
