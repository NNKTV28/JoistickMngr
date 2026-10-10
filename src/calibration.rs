//! Stick drift correction. A worn or noisy stick does not rest exactly in the middle and jitters around
//! its resting point. Measuring the axis while nobody touches it gives its real centre and how far it
//! wanders; every reading is then re-centred and the wandering is swallowed by a matching deadzone.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::known_devices::DeviceKind;
use crate::storage::{application_data_directory, read_json_file, write_json_file};

const CALIBRATION_FILE_NAME: &str = "calibration.json";
/// Extra room on top of the measured jitter, so readings at the edge of the noise never leak through.
const DEADZONE_SAFETY_FACTOR: f64 = 1.5;
const MINIMUM_DEADZONE: f64 = 0.01;
const MAXIMUM_DEADZONE: f64 = 0.2;
const MINIMUM_SAMPLE_COUNT: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AxisCalibration {
    pub device: DeviceKind,
    pub axis_index: usize,
    /// Where the axis really rests, 0..=1 (ideally 0.5).
    pub rest_center: f64,
    /// Share of each half of the travel around the centre that reads as exactly centred.
    pub deadzone: f64,
}

impl AxisCalibration {
    /// Builds a calibration from readings taken while the axis was left alone.
    pub fn from_rest_samples(device: DeviceKind, axis_index: usize, samples: &[f64]) -> Option<Self> {
        if samples.len() < MINIMUM_SAMPLE_COUNT {
            return None;
        }
        let rest_center = samples.iter().sum::<f64>() / samples.len() as f64;
        let largest_wander = samples
            .iter()
            .map(|sample| (sample - rest_center).abs())
            .fold(0.0, f64::max);
        let wander_as_share_of_half_travel = largest_wander / 0.5;
        let deadzone =
            (wander_as_share_of_half_travel * DEADZONE_SAFETY_FACTOR).clamp(MINIMUM_DEADZONE, MAXIMUM_DEADZONE);
        Some(Self {
            device,
            axis_index,
            rest_center,
            deadzone,
        })
    }

    /// Re-centres a raw reading (0..=1) so the measured rest point reads 0.5 and the jitter around it
    /// reads as exactly centred, while both ends still reach 0 and 1.
    pub fn correct(&self, raw_value: f64) -> f64 {
        let offset = raw_value - self.rest_center;
        let half_travel = if offset >= 0.0 {
            1.0 - self.rest_center
        } else {
            self.rest_center
        };
        if half_travel <= 0.0 {
            return 0.5;
        }
        let deflection = (offset / half_travel).clamp(-1.0, 1.0);
        let outside_deadzone = ((deflection.abs() - self.deadzone) / (1.0 - self.deadzone)).max(0.0);
        0.5 + deflection.signum() * outside_deadzone * 0.5
    }

    /// How far the measured centre was from the true middle, in percent of half the travel.
    pub fn drift_percent(&self) -> f64 {
        (self.rest_center - 0.5) / 0.5 * 100.0
    }
}

/// Calibrations belong to the hardware, not to a profile, so they are stored on their own.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CalibrationStore {
    pub axes: Vec<AxisCalibration>,
}

impl CalibrationStore {
    pub fn load() -> Self {
        read_json_file(&calibration_file_path()).unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        write_json_file(&calibration_file_path(), self)
    }

    pub fn correct(&self, device: DeviceKind, axis_index: usize, raw_value: f64) -> f64 {
        self.axes
            .iter()
            .find(|calibration| calibration.device == device && calibration.axis_index == axis_index)
            .map_or(raw_value, |calibration| calibration.correct(raw_value))
    }

    /// Replaces any older calibration of the same axes.
    pub fn store(&mut self, new_calibrations: Vec<AxisCalibration>) {
        self.axes.retain(|existing| {
            !new_calibrations
                .iter()
                .any(|new| new.device == existing.device && new.axis_index == existing.axis_index)
        });
        self.axes.extend(new_calibrations);
    }
}

fn calibration_file_path() -> PathBuf {
    application_data_directory().join(CALIBRATION_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drifting_stick() -> AxisCalibration {
        let samples: Vec<f64> = (0..50)
            .map(|step| 0.53 + if step % 2 == 0 { 0.004 } else { -0.004 })
            .collect();
        AxisCalibration::from_rest_samples(DeviceKind::Joystick, 0, &samples).unwrap()
    }

    #[test]
    fn resting_stick_reads_exactly_centred() {
        let calibration = drifting_stick();
        assert_eq!(calibration.correct(0.53), 0.5);
        assert_eq!(calibration.correct(0.534), 0.5);
        assert_eq!(calibration.correct(0.526), 0.5);
    }

    #[test]
    fn full_deflection_still_reaches_both_ends() {
        let calibration = drifting_stick();
        assert_eq!(calibration.correct(1.0), 1.0);
        assert_eq!(calibration.correct(0.0), 0.0);
    }

    #[test]
    fn too_few_samples_give_no_calibration() {
        assert!(AxisCalibration::from_rest_samples(DeviceKind::Joystick, 0, &[0.5, 0.5]).is_none());
    }
}
