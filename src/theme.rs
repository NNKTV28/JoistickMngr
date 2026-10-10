//! Cockpit-style dark theme: colours, fonts and egui visuals.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Visuals,
};

pub const BACKGROUND: Color32 = Color32::from_rgb(0x0D, 0x10, 0x13);
pub const SIDEBAR_BACKGROUND: Color32 = Color32::from_rgb(0x11, 0x15, 0x1A);
pub const CARD_BACKGROUND: Color32 = Color32::from_rgb(0x15, 0x1A, 0x1F);
pub const INSET_BACKGROUND: Color32 = Color32::from_rgb(0x0F, 0x13, 0x17);
pub const RAISED_BACKGROUND: Color32 = Color32::from_rgb(0x1B, 0x21, 0x28);
pub const SELECTED_BACKGROUND: Color32 = Color32::from_rgb(0x1F, 0x26, 0x2E);
pub const BORDER: Color32 = Color32::from_rgb(0x22, 0x2A, 0x31);
pub const STRONG_BORDER: Color32 = Color32::from_rgb(0x2E, 0x37, 0x40);
pub const GRID_LINE: Color32 = Color32::from_rgb(0x25, 0x2D, 0x35);

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xE6, 0xEA, 0xEE);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xC3, 0xCC, 0xD5);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x93, 0xA0, 0xAC);

pub const ACCENT_AMBER: Color32 = Color32::from_rgb(0xF2, 0xA3, 0x3A);
pub const ACCENT_AMBER_TEXT: Color32 = Color32::from_rgb(0x17, 0x11, 0x0A);
pub const LIVE_CYAN: Color32 = Color32::from_rgb(0x4F, 0xD1, 0xE0);

pub const SUCCESS_TEXT: Color32 = Color32::from_rgb(0x7F, 0xE0, 0xA5);
pub const SUCCESS_BACKGROUND: Color32 = Color32::from_rgb(0x14, 0x30, 0x1F);
pub const SUCCESS_DOT: Color32 = Color32::from_rgb(0x5B, 0xD0, 0x8A);
pub const WARNING_TEXT: Color32 = Color32::from_rgb(0xF5, 0xBE, 0x6E);
pub const WARNING_BACKGROUND: Color32 = Color32::from_rgb(0x2E, 0x24, 0x12);
pub const WARNING_PANEL_BACKGROUND: Color32 = Color32::from_rgb(0x1E, 0x19, 0x10);
pub const WARNING_PANEL_BORDER: Color32 = Color32::from_rgb(0x4A, 0x38, 0x18);
pub const WARNING_BODY_TEXT: Color32 = Color32::from_rgb(0xD9, 0xCD, 0xB8);
pub const INFO_TEXT: Color32 = Color32::from_rgb(0x8F, 0xD3, 0xF5);
pub const INFO_BACKGROUND: Color32 = Color32::from_rgb(0x13, 0x28, 0x3A);
pub const DANGER_TEXT: Color32 = Color32::from_rgb(0xFF, 0x9B, 0x8F);
pub const DANGER_BACKGROUND: Color32 = Color32::from_rgb(0x3A, 0x17, 0x14);
pub const OFFLINE_DOT: Color32 = Color32::from_rgb(0x5A, 0x64, 0x6E);

pub const CARD_CORNER_RADIUS: u8 = 10;
pub const CONTROL_CORNER_RADIUS: u8 = 6;

const CONDENSED_FAMILY_NAME: &str = "condensed";
const SEMIBOLD_FAMILY_NAME: &str = "semibold";

pub fn condensed_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(CONDENSED_FAMILY_NAME.into()))
}

pub fn semibold_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD_FAMILY_NAME.into()))
}

pub fn monospace_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn apply_cockpit_theme(context: &egui::Context) {
    context.set_fonts(build_font_definitions());
    context.set_visuals(build_dark_visuals());
    context.all_styles_mut(|style| {
        style.text_styles = build_text_styles();
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(16.0, 10.0);
        style.spacing.interact_size.y = 36.0;
        style.spacing.slider_width = 220.0;
        style.spacing.icon_width = 20.0;
        style.spacing.icon_width_inner = 12.0;
    });
}

fn build_font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    register_font(
        &mut fonts,
        "barlow-regular",
        include_bytes!("../assets/fonts/Barlow-Regular.ttf"),
    );
    register_font(
        &mut fonts,
        "barlow-semibold",
        include_bytes!("../assets/fonts/Barlow-SemiBold.ttf"),
    );
    register_font(
        &mut fonts,
        "barlow-condensed",
        include_bytes!("../assets/fonts/BarlowCondensed-SemiBold.ttf"),
    );
    register_font(
        &mut fonts,
        "jetbrains-mono",
        include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    );

    put_font_first(&mut fonts, FontFamily::Proportional, "barlow-regular");
    put_font_first(&mut fonts, FontFamily::Monospace, "jetbrains-mono");
    create_family_with_fallbacks(&mut fonts, SEMIBOLD_FAMILY_NAME, "barlow-semibold");
    create_family_with_fallbacks(&mut fonts, CONDENSED_FAMILY_NAME, "barlow-condensed");
    fonts
}

fn register_font(fonts: &mut FontDefinitions, font_name: &str, font_bytes: &'static [u8]) {
    fonts
        .font_data
        .insert(font_name.to_owned(), Arc::new(FontData::from_static(font_bytes)));
}

fn put_font_first(fonts: &mut FontDefinitions, family: FontFamily, font_name: &str) {
    fonts
        .families
        .entry(family)
        .or_default()
        .insert(0, font_name.to_owned());
}

fn create_family_with_fallbacks(fonts: &mut FontDefinitions, family_name: &str, font_name: &str) {
    let mut family_fonts = vec![font_name.to_owned()];
    family_fonts.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts
        .families
        .insert(FontFamily::Name(family_name.into()), family_fonts);
}

fn build_text_styles() -> std::collections::BTreeMap<TextStyle, FontId> {
    [
        (TextStyle::Heading, condensed_font(32.0)),
        (TextStyle::Body, FontId::proportional(15.0)),
        (TextStyle::Button, FontId::proportional(15.0)),
        (TextStyle::Small, FontId::proportional(12.5)),
        (TextStyle::Monospace, monospace_font(13.0)),
    ]
    .into()
}

fn build_dark_visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    let control_radius = CornerRadius::same(CONTROL_CORNER_RADIUS);

    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = CARD_BACKGROUND;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.extreme_bg_color = INSET_BACKGROUND;
    visuals.faint_bg_color = CARD_BACKGROUND;
    visuals.hyperlink_color = LIVE_CYAN;
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.selection.bg_fill = ACCENT_AMBER;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT_AMBER_TEXT);

    visuals.widgets.noninteractive.bg_fill = CARD_BACKGROUND;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    for widget_visuals in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget_visuals.corner_radius = control_radius;
        widget_visuals.bg_fill = RAISED_BACKGROUND;
        widget_visuals.weak_bg_fill = RAISED_BACKGROUND;
        widget_visuals.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    }
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, STRONG_BORDER);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x4A, 0x56, 0x63));
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT_AMBER);
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, ACCENT_AMBER);
    visuals
}
