//! Reusable building blocks for the cockpit look: cards, pills, buttons and live input gauges.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Frame, Margin, Rect, Response, RichText, Sense, Stroke, StrokeKind,
    TextureHandle, Ui, Vec2, pos2, vec2,
};

use crate::input_reader::HatDirection;
use crate::localization::tr;
use crate::profiles::{AxisTuning, LightingSetting, RgbColor};
use crate::theme::*;

const BUTTON_HEIGHT: f32 = 40.0;
const DEVICE_PREVIEW_HEIGHT: f32 = 190.0;
const VERTICAL_BAR_COLUMN_WIDTH: f32 = 78.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Success,
    Warning,
    Info,
    Danger,
    Neutral,
}

impl Tone {
    fn colors(self) -> (Color32, Color32) {
        match self {
            Tone::Success => (SUCCESS_BACKGROUND, SUCCESS_TEXT),
            Tone::Warning => (WARNING_BACKGROUND, WARNING_TEXT),
            Tone::Info => (INFO_BACKGROUND, INFO_TEXT),
            Tone::Danger => (DANGER_BACKGROUND, DANGER_TEXT),
            Tone::Neutral => (RAISED_BACKGROUND, TEXT_SECONDARY),
        }
    }
}

pub fn card_frame() -> Frame {
    Frame::new()
        .fill(CARD_BACKGROUND)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(CARD_CORNER_RADIUS))
        .inner_margin(Margin::same(20))
}

pub fn warning_card_frame() -> Frame {
    card_frame()
        .fill(WARNING_PANEL_BACKGROUND)
        .stroke(Stroke::new(1.0, WARNING_PANEL_BORDER))
}

/// A full-width card whose content is left aligned (column layouts would otherwise justify text).
pub fn card(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    show_full_width_frame(ui, card_frame(), add_contents);
}

pub fn warning_card(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    show_full_width_frame(ui, warning_card_frame(), add_contents);
}

fn show_full_width_frame(ui: &mut Ui, frame: Frame, add_contents: impl FnOnce(&mut Ui)) {
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), add_contents);
    });
}

/// Small uppercase label used above values and sections.
pub fn caption(ui: &mut Ui, text: &str) -> Response {
    ui.label(
        RichText::new(tr(text).to_uppercase())
            .font(condensed_font(13.0))
            .color(TEXT_MUTED)
            .extra_letter_spacing(1.4),
    )
}

pub fn colored_caption(ui: &mut Ui, text: &str, color: Color32) -> Response {
    ui.label(
        RichText::new(tr(text).to_uppercase())
            .font(condensed_font(13.0))
            .color(color)
            .extra_letter_spacing(1.4),
    )
}

pub fn card_title(ui: &mut Ui, text: &str) -> Response {
    ui.label(RichText::new(tr(text)).font(semibold_font(19.0)).color(TEXT_PRIMARY))
}

pub fn muted_text(ui: &mut Ui, text: impl Into<String>) -> Response {
    let text = text.into();
    ui.label(RichText::new(tr(&text)).color(TEXT_MUTED))
}

pub fn monospace_text(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(
        RichText::new(text.into())
            .font(monospace_font(13.5))
            .color(TEXT_PRIMARY),
    )
}

/// Page title block with a caption, a large title and an optional subtitle, plus actions on the right.
pub fn page_header(
    ui: &mut Ui,
    caption_text: &str,
    title: &str,
    subtitle: Option<&str>,
    add_actions: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            caption(ui, caption_text);
            ui.label(RichText::new(tr(title)).font(condensed_font(36.0)).color(TEXT_PRIMARY));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add_actions);
    });
    if let Some(subtitle) = subtitle {
        muted_text(ui, subtitle);
    }
    ui.add_space(14.0);
}

pub fn status_pill(ui: &mut Ui, text: &str, tone: Tone) -> Response {
    let (background, foreground) = tone.colors();
    Frame::new()
        .fill(background)
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| ui.label(RichText::new(tr(text)).size(13.5).color(foreground)))
        .response
}

pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    let label = RichText::new(tr(text))
        .font(semibold_font(15.0))
        .color(ACCENT_AMBER_TEXT);
    ui.add(
        egui::Button::new(label)
            .fill(ACCENT_AMBER)
            .stroke(Stroke::new(1.0, ACCENT_AMBER))
            .min_size(vec2(0.0, BUTTON_HEIGHT)),
    )
}

pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(tr(text)).color(TEXT_PRIMARY)).min_size(vec2(0.0, BUTTON_HEIGHT)))
}

pub fn enabled_primary_button(ui: &mut Ui, is_enabled: bool, text: &str) -> Response {
    ui.add_enabled_ui(is_enabled, |ui| primary_button(ui, text)).inner
}

pub fn enabled_secondary_button(ui: &mut Ui, is_enabled: bool, text: &str) -> Response {
    ui.add_enabled_ui(is_enabled, |ui| secondary_button(ui, text)).inner
}

/// Two-column list of labels and values.
pub fn key_value_grid(ui: &mut Ui, grid_id: &str, rows: &[(&str, String)]) {
    egui::Grid::new(grid_id)
        .num_columns(2)
        .spacing([18.0, 6.0])
        .min_row_height(20.0)
        .show(ui, |ui| {
            for (label, value) in rows {
                muted_text(ui, *label);
                monospace_text(ui, value.as_str());
                ui.end_row();
            }
        });
}

pub fn horizontal_level_bar(ui: &mut Ui, fraction: f64, width: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), INSET_BACKGROUND);
    let filled_width = rect.width() * fraction.clamp(0.0, 1.0) as f32;
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(filled_width, rect.height())),
        CornerRadius::same(3),
        color,
    );
}

/// Label and value above a thin horizontal bar, as on the overview page.
pub fn labeled_axis_meter(ui: &mut Ui, label: &str, value_text: &str, fraction: f64, width: f32) {
    ui.allocate_ui_with_layout(vec2(width, 40.0), egui::Layout::top_down(egui::Align::LEFT), |ui| {
        ui.set_width(width);
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).size(13.5).color(TEXT_MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(value_text).font(monospace_font(13.0)));
            });
        });
        horizontal_level_bar(ui, fraction, width, LIVE_CYAN);
    });
}

/// Tall bar filled from the bottom, with a caption underneath.
pub fn vertical_level_bar(ui: &mut Ui, fraction: f64, label: &str, width: f32, color: Color32) {
    let column_width = width.max(VERTICAL_BAR_COLUMN_WIDTH);
    let centered_column = egui::Layout::top_down(egui::Align::Center);
    ui.allocate_ui_with_layout(vec2(column_width, 180.0), centered_column, |ui| {
        ui.set_width(column_width);
        let (rect, response) = ui.allocate_exact_size(vec2(width, 150.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(6), INSET_BACKGROUND);
        painter.rect_stroke(
            rect,
            CornerRadius::same(6),
            Stroke::new(1.0, GRID_LINE),
            StrokeKind::Inside,
        );
        let filled_height = rect.height() * fraction.clamp(0.0, 1.0) as f32;
        let filled_rect = Rect::from_min_max(pos2(rect.min.x, rect.max.y - filled_height), rect.max);
        painter.rect_filled(filled_rect, CornerRadius::same(6), color);
        response.on_hover_text(format!("{:.0}%", fraction * 100.0));
        ui.label(
            RichText::new(label.to_uppercase())
                .font(condensed_font(12.0))
                .color(TEXT_MUTED),
        );
    });
}

/// Square plot of a two-axis position, with 0.5 as the centre of both axes.
pub fn crosshair_plot(ui: &mut Ui, horizontal_value: f64, vertical_value: f64, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), INSET_BACKGROUND);
    painter.rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0, GRID_LINE),
        StrokeKind::Inside,
    );
    painter.line_segment([rect.center_top(), rect.center_bottom()], Stroke::new(1.0, GRID_LINE));
    painter.line_segment([rect.left_center(), rect.right_center()], Stroke::new(1.0, GRID_LINE));
    painter.circle_stroke(rect.center(), size * 0.12, Stroke::new(1.0, STRONG_BORDER));

    let marker_position = pos2(
        rect.min.x + rect.width() * horizontal_value.clamp(0.0, 1.0) as f32,
        rect.min.y + rect.height() * vertical_value.clamp(0.0, 1.0) as f32,
    );
    painter.circle_filled(marker_position, 12.0, LIVE_CYAN.gamma_multiply(0.18));
    painter.circle_filled(marker_position, 7.0, LIVE_CYAN);
}

/// Horizontal bar that fills outward from the middle, for centred axes such as the rudder.
pub fn centered_axis_bar(ui: &mut Ui, value: f64, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(6), INSET_BACKGROUND);
    painter.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0, GRID_LINE),
        StrokeKind::Inside,
    );
    let center_x = rect.center().x;
    let value_x = rect.min.x + rect.width() * value.clamp(0.0, 1.0) as f32;
    let filled_rect = Rect::from_x_y_ranges(
        center_x.min(value_x)..=center_x.max(value_x),
        rect.min.y + 6.0..=rect.max.y - 6.0,
    );
    painter.rect_filled(filled_rect, CornerRadius::same(3), LIVE_CYAN);
    painter.line_segment([rect.center_top(), rect.center_bottom()], Stroke::new(1.0, TEXT_MUTED));
}

/// Numbered squares that light up while a button is held.
pub fn button_state_grid(ui: &mut Ui, button_states: &[bool]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (button_index, is_pressed) in button_states.iter().enumerate() {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
            let (background, foreground) = if *is_pressed {
                (ACCENT_AMBER, ACCENT_AMBER_TEXT)
            } else {
                (INSET_BACKGROUND, TEXT_MUTED)
            };
            ui.painter().rect_filled(rect, CornerRadius::same(6), background);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                button_index + 1,
                monospace_font(11.5),
                foreground,
            );
        }
    });
}

/// Small round gauge showing which way a hat switch is pushed.
pub fn hat_direction_indicator(ui: &mut Ui, direction: HatDirection, label: &str) {
    ui.vertical_centered(|ui| {
        ui.set_width(64.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
        let painter = ui.painter();
        painter.circle_filled(rect.center(), 26.0, INSET_BACKGROUND);
        painter.circle_stroke(rect.center(), 26.0, Stroke::new(1.0, GRID_LINE));
        let (offset_x, offset_y) = direction.offset();
        let marker_color = if direction == HatDirection::Centered {
            STRONG_BORDER
        } else {
            ACCENT_AMBER
        };
        painter.circle_filled(rect.center() + vec2(offset_x, offset_y) * 16.0, 6.0, marker_color);
        ui.label(
            RichText::new(label.to_uppercase())
                .font(condensed_font(12.0))
                .color(TEXT_MUTED),
        );
    });
}

/// Clickable colour square; `is_selected` draws an amber ring.
pub fn color_swatch(ui: &mut Ui, color: RgbColor, is_selected: bool, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
    let ring_color = if is_selected {
        ACCENT_AMBER
    } else if response.hovered() {
        TEXT_MUTED
    } else {
        STRONG_BORDER
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), to_color32(color));
    painter.rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(2.0, ring_color),
        StrokeKind::Inside,
    );
    response.on_hover_text(tooltip)
}

/// Product photo on a dark stage, lit from behind in the unit's LED colour.
/// Without a photo (offline, not downloaded yet) a ring stands in for the device.
pub fn device_preview(ui: &mut Ui, photo: Option<&TextureHandle>, glow: Option<LightingSetting>, label: &str) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, DEVICE_PREVIEW_HEIGHT), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), Color32::from_rgb(0x0A, 0x0D, 0x10));
    painter.rect_stroke(
        rect,
        CornerRadius::same(10),
        Stroke::new(1.0, GRID_LINE),
        StrokeKind::Inside,
    );
    if let Some(setting) = glow {
        paint_led_glow(ui, rect, setting);
    }
    match photo {
        Some(texture) => paint_photo_fitted(ui, rect.shrink(12.0), texture),
        None => paint_placeholder_ring(ui, rect, glow),
    }
    paint_preview_label(ui, rect, label);
}

fn paint_led_glow(ui: &Ui, rect: Rect, setting: LightingSetting) {
    let intensity = setting.brightness_percent as f32 / 100.0;
    let glow_color = to_color32(setting.color);
    for ring in (1..=12).rev() {
        let radius = 30.0 + ring as f32 * 7.0;
        let alpha = intensity * 0.06 * (13 - ring) as f32 / 12.0;
        ui.painter()
            .circle_filled(rect.center(), radius, glow_color.gamma_multiply(alpha));
    }
}

fn paint_photo_fitted(ui: &Ui, area: Rect, texture: &TextureHandle) {
    let photo_size = texture.size_vec2();
    let scale = (area.width() / photo_size.x).min(area.height() / photo_size.y);
    let photo_rect = Rect::from_center_size(area.center(), photo_size * scale);
    egui::Image::new(texture).paint_at(ui, photo_rect);
}

fn paint_placeholder_ring(ui: &Ui, rect: Rect, glow: Option<LightingSetting>) {
    let ring_color = match glow {
        Some(setting) => to_color32(setting.color).gamma_multiply(0.25 + setting.brightness_percent as f32 / 133.0),
        None => STRONG_BORDER,
    };
    ui.painter()
        .circle_stroke(rect.center(), 42.0, Stroke::new(3.0, ring_color));
}

fn paint_preview_label(ui: &Ui, rect: Rect, label: &str) {
    ui.painter().text(
        rect.left_bottom() + vec2(12.0, -10.0),
        Align2::LEFT_BOTTOM,
        label,
        monospace_font(11.5),
        TEXT_MUTED,
    );
}

/// Tiny plot of the response curve described by an axis tuning.
pub fn tuning_curve_preview(ui: &mut Ui, tuning: &AxisTuning) {
    let (rect, _) = ui.allocate_exact_size(vec2(72.0, 30.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(4), INSET_BACKGROUND);
    let points: Vec<egui::Pos2> = (0..=24)
        .map(|step| {
            let input = step as f64 / 24.0;
            let output = tuning.apply_to_reading(input, false);
            pos2(
                rect.min.x + rect.width() * input as f32,
                rect.max.y - rect.height() * output as f32,
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(2.0, LIVE_CYAN)));
}

/// Full-width clickable row used in the sidebar.
pub fn navigation_item(ui: &mut Ui, label: &str, is_selected: bool, leading_dot: Option<Color32>) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
    let painter = ui.painter();
    if is_selected {
        painter.rect_filled(rect, CornerRadius::same(6), SELECTED_BACKGROUND);
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
            CornerRadius::same(1),
            ACCENT_AMBER,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(6), RAISED_BACKGROUND);
    }
    let text_color = if is_selected { ACCENT_AMBER } else { TEXT_SECONDARY };
    let mut text_x = rect.min.x + 16.0;
    if let Some(dot_color) = leading_dot {
        painter.circle_filled(pos2(text_x + 4.0, rect.center().y), 4.0, dot_color);
        text_x += 18.0;
    }
    painter.text(
        pos2(text_x, rect.center().y),
        Align2::LEFT_CENTER,
        tr(label),
        egui::FontId::proportional(15.0),
        text_color,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn to_color32(color: RgbColor) -> Color32 {
    Color32::from_rgb(color.red, color.green, color.blue)
}

pub fn format_axis_value(value: f64, rests_at_center: bool) -> String {
    if rests_at_center {
        format!("{:+.2}", value * 2.0 - 1.0)
    } else {
        format!("{value:.2}")
    }
}

/// A right-pointing arrow drawn with lines, so it never depends on a font having the glyph.
pub fn flow_arrow(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(22.0, 16.0), Sense::hover());
    let painter = ui.painter();
    let stroke = Stroke::new(1.5, TEXT_MUTED);
    let middle_y = rect.center().y;
    let tip = pos2(rect.max.x - 3.0, middle_y);
    painter.line_segment([pos2(rect.min.x + 3.0, middle_y), tip], stroke);
    painter.line_segment([tip, pos2(tip.x - 5.0, middle_y - 4.0)], stroke);
    painter.line_segment([tip, pos2(tip.x - 5.0, middle_y + 4.0)], stroke);
}

/// A dot that glows amber while the thing it belongs to is active.
pub fn activity_light(ui: &mut Ui, is_active: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    let painter = ui.painter();
    if is_active {
        painter.circle_filled(rect.center(), 7.0, ACCENT_AMBER.gamma_multiply(0.25));
        painter.circle_filled(rect.center(), 4.5, ACCENT_AMBER);
    } else {
        painter.circle_stroke(rect.center(), 4.0, Stroke::new(1.0, STRONG_BORDER));
    }
}

/// A short bar showing how far (0..=1) an analog control is pushed.
pub fn deflection_meter(ui: &mut Ui, deflection: f64) {
    let (rect, _) = ui.allocate_exact_size(vec2(44.0, 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), INSET_BACKGROUND);
    let filled_width = rect.width() * deflection.clamp(0.0, 1.0) as f32;
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(filled_width, rect.height())),
        CornerRadius::same(3),
        ACCENT_AMBER,
    );
}

/// Small coloured label naming a category, such as the kind of output a binding produces.
/// Drawn at a fixed size so table rows never stretch it.
pub fn category_tag(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(tr(text).to_owned(), egui::FontId::proportional(12.0), color);
    let tag_size = galley.size() + vec2(12.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(tag_size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(4), color.gamma_multiply(0.15));
    painter.rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, color.gamma_multiply(0.5)),
        StrokeKind::Inside,
    );
    painter.galley(rect.center() - galley.size() / 2.0, galley, color);
    response
}
