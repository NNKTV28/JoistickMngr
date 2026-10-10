//! Lighting animations. Each effect turns a lighting setting and the time since the effect started
//! into the colour and brightness to show right now.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::localization::tr;
use crate::profiles::{LightingSetting, RgbColor};

pub const MINIMUM_CYCLE_SECONDS: f32 = 0.3;
pub const MAXIMUM_CYCLE_SECONDS: f32 = 10.0;
const DEFAULT_CYCLE_SECONDS: f32 = 3.0;
/// Breathing never goes fully dark, so the controls stay visible at the bottom of each breath.
const BREATHING_LOWEST_BRIGHTNESS_SHARE: f64 = 0.12;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum LightingEffect {
    #[default]
    Static,
    /// Brightness rises and falls smoothly.
    Breathing { cycle_seconds: f32 },
    /// Runs through every hue.
    Rainbow { cycle_seconds: f32 },
    /// Blends back and forth between the main colour and `second_color`.
    Fade { second_color: RgbColor, cycle_seconds: f32 },
    /// Flashes on and off.
    Strobe { cycle_seconds: f32 },
}

/// The kinds of effect, without their settings, for choosing one in the interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightingEffectKind {
    Static,
    Breathing,
    Rainbow,
    Fade,
    Strobe,
}

impl LightingEffectKind {
    pub const ALL: [LightingEffectKind; 5] = [
        LightingEffectKind::Static,
        LightingEffectKind::Breathing,
        LightingEffectKind::Rainbow,
        LightingEffectKind::Fade,
        LightingEffectKind::Strobe,
    ];

    pub fn label(self) -> &'static str {
        tr(match self {
            LightingEffectKind::Static => "Static",
            LightingEffectKind::Breathing => "Breathing",
            LightingEffectKind::Rainbow => "Rainbow",
            LightingEffectKind::Fade => "Fade between two colours",
            LightingEffectKind::Strobe => "Strobe",
        })
    }

    /// The effect of this kind, keeping the speed and second colour of `previous` where it has them.
    pub fn build(self, previous: LightingEffect) -> LightingEffect {
        let cycle_seconds = previous.cycle_seconds().unwrap_or(DEFAULT_CYCLE_SECONDS);
        match self {
            LightingEffectKind::Static => LightingEffect::Static,
            LightingEffectKind::Breathing => LightingEffect::Breathing { cycle_seconds },
            LightingEffectKind::Rainbow => LightingEffect::Rainbow { cycle_seconds },
            LightingEffectKind::Fade => LightingEffect::Fade {
                second_color: previous.second_color().unwrap_or(RgbColor::new(0xFF, 0x2A, 0x1F)),
                cycle_seconds,
            },
            LightingEffectKind::Strobe => LightingEffect::Strobe {
                cycle_seconds: cycle_seconds.min(1.0),
            },
        }
    }
}

impl LightingEffect {
    pub fn kind(&self) -> LightingEffectKind {
        match self {
            LightingEffect::Static => LightingEffectKind::Static,
            LightingEffect::Breathing { .. } => LightingEffectKind::Breathing,
            LightingEffect::Rainbow { .. } => LightingEffectKind::Rainbow,
            LightingEffect::Fade { .. } => LightingEffectKind::Fade,
            LightingEffect::Strobe { .. } => LightingEffectKind::Strobe,
        }
    }

    pub fn is_animated(&self) -> bool {
        !matches!(self, LightingEffect::Static)
    }

    pub fn cycle_seconds(&self) -> Option<f32> {
        match *self {
            LightingEffect::Static => None,
            LightingEffect::Breathing { cycle_seconds }
            | LightingEffect::Rainbow { cycle_seconds }
            | LightingEffect::Fade { cycle_seconds, .. }
            | LightingEffect::Strobe { cycle_seconds } => Some(cycle_seconds),
        }
    }

    pub fn cycle_seconds_mut(&mut self) -> Option<&mut f32> {
        match self {
            LightingEffect::Static => None,
            LightingEffect::Breathing { cycle_seconds }
            | LightingEffect::Rainbow { cycle_seconds }
            | LightingEffect::Fade { cycle_seconds, .. }
            | LightingEffect::Strobe { cycle_seconds } => Some(cycle_seconds),
        }
    }

    pub fn second_color(&self) -> Option<RgbColor> {
        match *self {
            LightingEffect::Fade { second_color, .. } => Some(second_color),
            _ => None,
        }
    }

    pub fn second_color_mut(&mut self) -> Option<&mut RgbColor> {
        match self {
            LightingEffect::Fade { second_color, .. } => Some(second_color),
            _ => None,
        }
    }
}

/// Colour and brightness the LEDs show at one moment of an effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightingFrame {
    pub color: RgbColor,
    pub brightness_percent: u8,
}

pub fn frame_at(setting: LightingSetting, time_since_start: Duration) -> LightingFrame {
    let progress = setting
        .effect
        .cycle_seconds()
        .map_or(0.0, |cycle_seconds| cycle_progress(time_since_start, cycle_seconds));
    let full_brightness = setting.brightness_percent;
    match setting.effect {
        LightingEffect::Static => LightingFrame {
            color: setting.color,
            brightness_percent: full_brightness,
        },
        LightingEffect::Breathing { .. } => LightingFrame {
            color: setting.color,
            brightness_percent: scale_brightness(full_brightness, breathing_share(progress)),
        },
        LightingEffect::Rainbow { .. } => LightingFrame {
            color: color_from_hue(progress),
            brightness_percent: full_brightness,
        },
        LightingEffect::Fade { second_color, .. } => LightingFrame {
            color: blend_colors(setting.color, second_color, triangle_wave(progress)),
            brightness_percent: full_brightness,
        },
        LightingEffect::Strobe { .. } => LightingFrame {
            color: setting.color,
            brightness_percent: if progress < 0.5 { full_brightness } else { 0 },
        },
    }
}

/// The setting with its colour and brightness replaced by the current frame, for animated previews.
/// `seconds_since_start` is any steadily increasing clock, such as the interface's own time.
pub fn setting_showing_current_frame(setting: LightingSetting, seconds_since_start: f64) -> LightingSetting {
    let frame = frame_at(setting, Duration::from_secs_f64(seconds_since_start.max(0.0)));
    LightingSetting {
        color: frame.color,
        brightness_percent: frame.brightness_percent,
        ..setting
    }
}

/// How far through the current cycle we are, 0..1.
fn cycle_progress(time_since_start: Duration, cycle_seconds: f32) -> f64 {
    let cycle_seconds = f64::from(cycle_seconds.clamp(MINIMUM_CYCLE_SECONDS, MAXIMUM_CYCLE_SECONDS));
    (time_since_start.as_secs_f64() % cycle_seconds) / cycle_seconds
}

/// 0 → 1 → 0 over one cycle.
fn triangle_wave(progress: f64) -> f64 {
    1.0 - (progress * 2.0 - 1.0).abs()
}

/// A smooth rise and fall, like breathing, between the lowest share and full brightness.
fn breathing_share(progress: f64) -> f64 {
    let wave = (1.0 - (progress * std::f64::consts::TAU).cos()) / 2.0;
    BREATHING_LOWEST_BRIGHTNESS_SHARE + (1.0 - BREATHING_LOWEST_BRIGHTNESS_SHARE) * wave
}

fn scale_brightness(full_brightness: u8, share: f64) -> u8 {
    (f64::from(full_brightness) * share).round() as u8
}

fn blend_colors(first: RgbColor, second: RgbColor, share_of_second: f64) -> RgbColor {
    let blend_channel = |first_channel: u8, second_channel: u8| {
        let blended =
            f64::from(first_channel) + (f64::from(second_channel) - f64::from(first_channel)) * share_of_second;
        blended.round() as u8
    };
    RgbColor::new(
        blend_channel(first.red, second.red),
        blend_channel(first.green, second.green),
        blend_channel(first.blue, second.blue),
    )
}

/// Fully saturated colour for a hue given as 0..1 around the colour wheel.
fn color_from_hue(hue: f64) -> RgbColor {
    let scaled_hue = (hue.rem_euclid(1.0)) * 6.0;
    let rising = scaled_hue.fract();
    let falling = 1.0 - rising;
    let (red, green, blue) = match scaled_hue as u8 {
        0 => (1.0, rising, 0.0),
        1 => (falling, 1.0, 0.0),
        2 => (0.0, 1.0, rising),
        3 => (0.0, falling, 1.0),
        4 => (rising, 0.0, 1.0),
        _ => (1.0, 0.0, falling),
    };
    let to_channel = |share: f64| (share * 255.0).round() as u8;
    RgbColor::new(to_channel(red), to_channel(green), to_channel(blue))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting_with(effect: LightingEffect) -> LightingSetting {
        LightingSetting {
            color: RgbColor::new(0, 0, 255),
            brightness_percent: 100,
            effect,
        }
    }

    #[test]
    fn rainbow_starts_red_and_passes_green() {
        let rainbow = setting_with(LightingEffect::Rainbow { cycle_seconds: 3.0 });
        assert_eq!(frame_at(rainbow, Duration::ZERO).color, RgbColor::new(255, 0, 0));
        assert_eq!(
            frame_at(rainbow, Duration::from_secs(1)).color,
            RgbColor::new(0, 255, 0)
        );
    }

    #[test]
    fn breathing_is_dimmest_at_the_start_of_each_breath() {
        let breathing = setting_with(LightingEffect::Breathing { cycle_seconds: 2.0 });
        assert_eq!(frame_at(breathing, Duration::ZERO).brightness_percent, 12);
        assert_eq!(frame_at(breathing, Duration::from_secs(1)).brightness_percent, 100);
    }

    #[test]
    fn fade_reaches_the_second_colour_half_way() {
        let fade = setting_with(LightingEffect::Fade {
            second_color: RgbColor::new(255, 0, 0),
            cycle_seconds: 2.0,
        });
        assert_eq!(frame_at(fade, Duration::from_secs(1)).color, RgbColor::new(255, 0, 0));
    }
}
