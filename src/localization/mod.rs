//! Interface language. Texts are written in English in the code and looked up in a translation
//! table when another language is selected; anything without a translation stays in English.

mod spanish_texts;

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};
use windows::Win32::Globalization::GetUserDefaultUILanguage;

const SPANISH_PRIMARY_LANGUAGE_ID: u16 = 0x0A;
const PRIMARY_LANGUAGE_MASK: u16 = 0x3FF;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    English,
    Spanish,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::Spanish];

    /// Each language is named in itself, so it can be found whatever is currently selected.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Spanish => "Español",
        }
    }

    /// Spanish when Windows is displayed in Spanish, English otherwise.
    pub fn of_windows_display() -> Self {
        // SAFETY: no arguments; reads the user's display language.
        let language_id = unsafe { GetUserDefaultUILanguage() };
        if language_id & PRIMARY_LANGUAGE_MASK == SPANISH_PRIMARY_LANGUAGE_ID {
            Language::Spanish
        } else {
            Language::English
        }
    }

    fn as_number(self) -> u8 {
        match self {
            Language::English => 0,
            Language::Spanish => 1,
        }
    }

    fn from_number(number: u8) -> Self {
        if number == 1 {
            Language::Spanish
        } else {
            Language::English
        }
    }
}

impl Default for Language {
    fn default() -> Self {
        Language::of_windows_display()
    }
}

static CURRENT_LANGUAGE: AtomicU8 = AtomicU8::new(0);

pub fn set_current_language(language: Language) {
    CURRENT_LANGUAGE.store(language.as_number(), Ordering::Relaxed);
}

pub fn current_language() -> Language {
    Language::from_number(CURRENT_LANGUAGE.load(Ordering::Relaxed))
}

/// The text in the current language, or the English text when no translation exists.
pub fn tr(english_text: &str) -> &str {
    match current_language() {
        Language::English => english_text,
        Language::Spanish => spanish_table().get(english_text).copied().unwrap_or(english_text),
    }
}

/// Translates a template whose `{}` markers are then filled with `values` in order.
pub fn tr_format(english_template: &str, values: &[&dyn Display]) -> String {
    let mut remaining_values = values.iter();
    let mut filled_text = String::new();
    let mut template_parts = tr(english_template).split("{}").peekable();
    while let Some(part) = template_parts.next() {
        filled_text.push_str(part);
        if template_parts.peek().is_some()
            && let Some(value) = remaining_values.next()
        {
            filled_text.push_str(&value.to_string());
        }
    }
    filled_text
}

fn spanish_table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| spanish_texts::SPANISH_TEXTS.iter().copied().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_values_are_filled_in_order() {
        set_current_language(Language::English);
        assert_eq!(tr_format("{} of {} devices", &[&2, &3]), "2 of 3 devices");
    }

    #[test]
    fn every_spanish_template_keeps_its_placeholders() {
        for (english, spanish) in spanish_texts::SPANISH_TEXTS {
            assert_eq!(
                english.matches("{}").count(),
                spanish.matches("{}").count(),
                "{english}"
            );
        }
    }
}
