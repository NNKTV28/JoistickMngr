//! The keyboard keys a control can be mapped to, with their Windows virtual-key codes.

use std::sync::OnceLock;

use eframe::egui;

#[derive(Clone, Debug)]
pub struct KeyboardKey {
    pub virtual_key: u16,
    pub name: String,
    /// The matching egui key, used to capture a key press in the mapping editor.
    pub egui_key: Option<egui::Key>,
}

pub fn all_keyboard_keys() -> &'static [KeyboardKey] {
    static KEYS: OnceLock<Vec<KeyboardKey>> = OnceLock::new();
    KEYS.get_or_init(build_key_list)
}

pub fn key_name(virtual_key: u16) -> String {
    all_keyboard_keys()
        .iter()
        .find(|key| key.virtual_key == virtual_key)
        .map(|key| key.name.clone())
        .unwrap_or_else(|| format!("Key 0x{virtual_key:02X}"))
}

pub fn find_key_for_egui_key(egui_key: egui::Key) -> Option<&'static KeyboardKey> {
    all_keyboard_keys().iter().find(|key| key.egui_key == Some(egui_key))
}

fn build_key_list() -> Vec<KeyboardKey> {
    let mut keys = Vec::new();
    for letter in b'A'..=b'Z' {
        add_key(
            &mut keys,
            u16::from(letter),
            &(letter as char).to_string(),
            Some(&(letter as char).to_string()),
        );
    }
    for digit in 0..=9u16 {
        add_key(&mut keys, 0x30 + digit, &digit.to_string(), Some(&digit.to_string()));
    }
    for function_number in 1..=24u16 {
        let name = format!("F{function_number}");
        add_key(&mut keys, 0x6F + function_number, &name, Some(&name));
    }
    for (virtual_key, name, egui_name) in NAMED_KEYS {
        add_key(&mut keys, *virtual_key, name, *egui_name);
    }
    for digit in 0..=9u16 {
        add_key(&mut keys, 0x60 + digit, &format!("Numpad {digit}"), None);
    }
    keys
}

fn add_key(keys: &mut Vec<KeyboardKey>, virtual_key: u16, name: &str, egui_name: Option<&str>) {
    keys.push(KeyboardKey {
        virtual_key,
        name: name.to_owned(),
        egui_key: egui_name.and_then(egui::Key::from_name),
    });
}

const NAMED_KEYS: &[(u16, &str, Option<&str>)] = &[
    (0x20, "Space", Some("Space")),
    (0x0D, "Enter", Some("Enter")),
    (0x1B, "Escape", Some("Escape")),
    (0x09, "Tab", Some("Tab")),
    (0x08, "Backspace", Some("Backspace")),
    (0x2D, "Insert", Some("Insert")),
    (0x2E, "Delete", Some("Delete")),
    (0x24, "Home", Some("Home")),
    (0x23, "End", Some("End")),
    (0x21, "Page Up", Some("PageUp")),
    (0x22, "Page Down", Some("PageDown")),
    (0x26, "Arrow Up", Some("Up")),
    (0x28, "Arrow Down", Some("Down")),
    (0x25, "Arrow Left", Some("Left")),
    (0x27, "Arrow Right", Some("Right")),
    (0xA0, "Left Shift", None),
    (0xA1, "Right Shift", None),
    (0xA2, "Left Ctrl", None),
    (0xA3, "Right Ctrl", None),
    (0xA4, "Left Alt", None),
    (0xA5, "Right Alt", None),
    (0x14, "Caps Lock", None),
    (0xBD, "Minus", Some("Minus")),
    (0xBB, "Equals", Some("Equals")),
    (0xDB, "[", Some("OpenBracket")),
    (0xDD, "]", Some("CloseBracket")),
    (0xDC, "\\", Some("Backslash")),
    (0xBA, ";", Some("Semicolon")),
    (0xDE, "'", Some("Quote")),
    (0xC0, "`", Some("Backtick")),
    (0xBC, ",", Some("Comma")),
    (0xBE, ".", Some("Period")),
    (0xBF, "/", Some("Slash")),
    (0x6A, "Numpad *", None),
    (0x6B, "Numpad +", None),
    (0x6D, "Numpad -", None),
    (0x6E, "Numpad .", None),
    (0x6F, "Numpad /", None),
];
