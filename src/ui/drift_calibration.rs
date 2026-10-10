//! "Fix stick drift": measure every centred axis at rest for a few seconds and store the correction.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};

use crate::app::JoystickManagerApp;
use crate::calibration::{AxisCalibration, CalibrationStore};
use crate::input_reader::{ControllerUpdate, InputReaderService};
use crate::known_devices::DeviceKind;
use crate::localization::{tr, tr_format};
use crate::theme::*;
use crate::ui::widgets::*;

/// Long enough to catch slow wandering as well as quick jitter.
const MEASUREMENT_DURATION: Duration = Duration::from_secs(30);
/// Steady samples give every moment the same weight when the resting point is averaged.
const STEADY_SAMPLE_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Default)]
struct AxisReadings {
    steady_samples: Vec<f64>,
    every_reading: Vec<f64>,
}

/// Readings collected while the user keeps their hands off the controls.
/// A background thread samples at a steady rate and also keeps every report the devices send.
pub struct DriftMeasurement {
    started_at: Instant,
    readings: Arc<Mutex<HashMap<(DeviceKind, usize), AxisReadings>>>,
    is_running: Arc<AtomicBool>,
}

impl DriftMeasurement {
    fn start(input_reader: &InputReaderService) -> Self {
        let measurement = Self {
            started_at: Instant::now(),
            readings: Arc::new(Mutex::new(HashMap::new())),
            is_running: Arc::new(AtomicBool::new(true)),
        };
        let reports = input_reader.subscribe_to_updates();
        let reader = input_reader.clone();
        let readings = Arc::clone(&measurement.readings);
        let is_running = Arc::clone(&measurement.is_running);
        std::thread::Builder::new()
            .name("drift-measurement".into())
            .spawn(move || collect_readings(&reader, &reports, &readings, &is_running))
            .expect("drift measurement thread starts");
        measurement
    }

    fn is_finished(&self) -> bool {
        self.started_at.elapsed() >= MEASUREMENT_DURATION
    }

    fn seconds_left(&self) -> u64 {
        MEASUREMENT_DURATION.saturating_sub(self.started_at.elapsed()).as_secs() + 1
    }

    fn progress(&self) -> f32 {
        (self.started_at.elapsed().as_secs_f32() / MEASUREMENT_DURATION.as_secs_f32()).min(1.0)
    }

    fn into_calibrations(self) -> Vec<AxisCalibration> {
        self.is_running.store(false, Ordering::Relaxed);
        let readings = std::mem::take(&mut *self.readings.lock().unwrap());
        let mut calibrations: Vec<AxisCalibration> = readings
            .into_iter()
            .filter_map(|((device, axis_index), axis_readings)| {
                AxisCalibration::from_rest_samples(
                    device,
                    axis_index,
                    &axis_readings.steady_samples,
                    &axis_readings.every_reading,
                )
            })
            .collect();
        calibrations.sort_by_key(|calibration| (calibration.device as u8, calibration.axis_index));
        calibrations
    }
}

impl Drop for DriftMeasurement {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::Relaxed);
    }
}

fn collect_readings(
    input_reader: &InputReaderService,
    reports: &Receiver<ControllerUpdate>,
    readings: &Mutex<HashMap<(DeviceKind, usize), AxisReadings>>,
    is_running: &AtomicBool,
) {
    while is_running.load(Ordering::Relaxed) {
        while let Ok(report) = reports.try_recv() {
            record_centred_axes(readings, report.device_kind, &report.snapshot.axis_values, false);
        }
        for device in DeviceKind::ALL {
            if let Some(snapshot) = input_reader.snapshot(device) {
                record_centred_axes(readings, device, &snapshot.axis_values, true);
            }
        }
        std::thread::sleep(STEADY_SAMPLE_INTERVAL);
    }
}

/// Only axes that rest in the middle can drift; throttles and brakes rest at an end.
fn record_centred_axes(
    readings: &Mutex<HashMap<(DeviceKind, usize), AxisReadings>>,
    device: DeviceKind,
    axis_values: &[f64],
    is_steady_sample: bool,
) {
    let definition = device.definition();
    let mut readings = readings.lock().unwrap();
    for (axis_index, value) in axis_values.iter().enumerate() {
        if !definition.axis_rests_at_center(axis_index) {
            continue;
        }
        let axis_readings = readings.entry((device, axis_index)).or_default();
        if is_steady_sample {
            axis_readings.steady_samples.push(*value);
        } else {
            axis_readings.every_reading.push(*value);
        }
    }
}

impl JoystickManagerApp {
    pub(crate) fn show_drift_card(&mut self, ui: &mut Ui) {
        self.finish_drift_measurement_when_due();
        let calibration = self.app_state.lock().unwrap().calibration.clone();
        let mut start_clicked = false;
        let mut reset_clicked = false;
        let mut cancel_clicked = false;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                card_title(ui, "Stick drift");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (text, tone) = if calibration.axes.is_empty() {
                        ("Not corrected", Tone::Neutral)
                    } else {
                        ("Drift corrected", Tone::Success)
                    };
                    status_pill(ui, text, tone);
                });
            });
            muted_text(
                ui,
                "Measures where each stick axis really rests and how much it jitters, then re-centres it and hides the jitter. \
                 The correction applies to everything this app sends: the virtual controller, the mouse and analog keys.",
            );
            ui.add_space(8.0);
            match &self.drift_measurement {
                Some(measurement) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new(tr_format(
                                "Keep your hands off the controls… {} s",
                                &[&measurement.seconds_left()],
                            ))
                            .color(LIVE_CYAN),
                        );
                        cancel_clicked = secondary_button(ui, "Cancel").clicked();
                    });
                    ui.add(egui::ProgressBar::new(measurement.progress()).desired_width(420.0));
                }
                None => {
                    ui.horizontal(|ui| {
                        start_clicked = primary_button(ui, "Fix stick drift").clicked();
                        reset_clicked =
                            enabled_secondary_button(ui, !calibration.axes.is_empty(), "Remove correction").clicked();
                    });
                }
            }
            show_calibration_table(ui, &calibration);
        });
        if cancel_clicked {
            self.drift_measurement = None;
        }
        if start_clicked {
            self.drift_measurement = Some(DriftMeasurement::start(&self.services.input_reader));
        }
        if reset_clicked {
            self.replace_calibration(CalibrationStore::default());
            self.show_status("Stick drift correction removed.", Tone::Info);
        }
    }

    /// Stores the result once a running measurement has collected its 30 seconds.
    fn finish_drift_measurement_when_due(&mut self) {
        if self
            .drift_measurement
            .as_ref()
            .is_some_and(DriftMeasurement::is_finished)
        {
            self.finish_drift_measurement();
        }
    }

    fn finish_drift_measurement(&mut self) {
        let Some(measurement) = self.drift_measurement.take() else {
            return;
        };
        let new_calibrations = measurement.into_calibrations();
        let corrected_axis_count = new_calibrations.len();
        let mut calibration = self.app_state.lock().unwrap().calibration.clone();
        calibration.store(new_calibrations);
        self.replace_calibration(calibration);
        self.show_status(
            tr_format("Stick drift fixed on {} axes.", &[&corrected_axis_count]),
            Tone::Success,
        );
    }

    fn replace_calibration(&mut self, calibration: CalibrationStore) {
        let save_result = calibration.save();
        self.app_state.lock().unwrap().calibration = calibration;
        if let Err(error) = save_result {
            self.show_status(tr_format("Could not save settings: {}", &[&error]), Tone::Danger);
        }
    }
}

fn show_calibration_table(ui: &mut Ui, calibration: &CalibrationStore) {
    if calibration.axes.is_empty() {
        return;
    }
    ui.add_space(8.0);
    egui::Grid::new("drift-corrections")
        .num_columns(3)
        .spacing([28.0, 6.0])
        .show(ui, |ui| {
            for heading in ["Axis", "Drift found", "Jitter hidden"] {
                caption(ui, heading);
            }
            ui.end_row();
            for axis in &calibration.axes {
                let definition = axis.device.definition();
                ui.label(format!(
                    "{} · {}",
                    definition.short_name,
                    definition.axis_name(axis.axis_index)
                ));
                monospace_text(ui, format!("{:+.1}%", axis.drift_percent()));
                monospace_text(ui, format!("{:.1}%", axis.deadzone * 100.0));
                ui.end_row();
            }
        });
    ui.label(
        RichText::new(tr("Measured once per device; run it again if the stick wears further."))
            .size(13.0)
            .color(TEXT_MUTED),
    );
}
