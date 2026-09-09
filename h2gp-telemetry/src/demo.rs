//! # Offline Replay Simulation (Demo Mode)
//!
//! Replays recorded telemetry data from a CSV log file at a steady 10 Hz rate.
//! Employs a synthetic monotonic clock to guarantee smooth graph rendering
//! independent of recording timestamp jitter.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use crate::protocol::{ChannelData, TelemetrySample};
use crate::error::TelemetryError;

/// Spawns a dedicated worker thread that replays telemetry rows from a CSV file.
///
/// Reads all lines into memory and continuously transmits reconstituted `TelemetrySample`
/// structs into `tx` at 100 ms intervals (10 Hz). Stops when `is_running` is set to false
/// or the channel receiver is closed.
pub fn start_demo_thread(filename: &str, tx: Sender<TelemetrySample>, is_running: Arc<AtomicBool>) {
    let filename = filename.to_string();
    thread::spawn(move || {
        if let Err(e) = run_demo_loop(&filename, &tx, &is_running) {
            eprintln!("[demo] {}", e);
        }
        println!("[demo] Thread terminated cleanly.");
    });
}

/// Inner demo loop extracted as a function returning Result.
///
/// Opens a recorded CSV file, ignores its original timestamps, and replays
/// the data at a smooth 10 Hz with a synthetic monotonic clock.
/// Loops until canceled or until the UI channel disconnects.
fn run_demo_loop(
    filename: &str,
    tx: &Sender<TelemetrySample>,
    is_running: &AtomicBool,
) -> Result<(), TelemetryError> {
    let file = File::open(filename).map_err(|e| TelemetryError::DemoFileOpen {
        path: filename.to_string(),
        source: e,
    })?;

    // Read all rows from the CSV into memory once
    let reader = BufReader::new(file);
    let raw_lines: Vec<String> = reader.lines().skip(1).filter_map(|l| l.ok()).collect();

    if raw_lines.is_empty() {
        return Err(TelemetryError::DemoFileEmpty {
            path: filename.to_string(),
        });
    }

    // Create a perfectly monotonic simulated timeline
    let mut simulated_time_ms: u32 = 0;

    // Loop until stopped
    while is_running.load(Ordering::Relaxed) {
        for line in &raw_lines {
            if !is_running.load(Ordering::Relaxed) {
                break;
            }

            if let Some(sample) = parse_csv_line(line, simulated_time_ms) {
                tx.send(sample).map_err(|_| TelemetryError::ChannelDisconnected)?;
            }

            // Advance simulated time by exactly 100ms (matching the 10 Hz playback)
            simulated_time_ms += 100;
            thread::sleep(Duration::from_millis(100)); 
        }
    }

    Ok(())
}

/// Parses a single comma-delimited line from the demo CSV into a `TelemetrySample`.
///
/// Automatically assigns `timestamp_ms` from the synthetic monotonic clock
/// and maps both battery and fuel cell physical channels.
pub fn parse_csv_line(line: &str, timestamp_ms: u32) -> Option<TelemetrySample> {
    let mut fields = line.split(',');

    let mut next_f64 = || {
        fields.next().unwrap_or("0").trim().parse::<f64>().unwrap_or(0.0)
    };

    // Ignore the jagged CSV timestamp to prevent graph resets
    let _ignored_csv_timestamp = next_f64(); 

    let b_v = next_f64();
    let b_sv = next_f64();
    let b_i = next_f64();
    let b_p = next_f64();
    let b_e = next_f64();
    let b_ah = next_f64();
    let b_t = next_f64();

    let f_v = next_f64();
    let f_sv = next_f64();
    let f_i = next_f64();
    let f_p = next_f64();
    let f_e = next_f64();
    let f_ah = next_f64();
    let f_t = next_f64();

    Some(TelemetrySample {
        timestamp_ms,
        has_channel_data: true,
        batt: ChannelData {
            v: b_v, sv_mv: b_sv, i: b_i, p: b_p, e: b_e, ah: b_ah, t: b_t,
        },
        fc: ChannelData {
            v: f_v, sv_mv: f_sv, i: f_i, p: f_p, e: f_e, ah: f_ah, t: f_t,
        },
        has_rev3_aux: false,
        rev3_aux: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anomaly::{self, AnomalyKind};
    use crate::config::AppConfig;
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    #[test]
    fn parse_csv_line_valid_row() {
        let line = "Demo,12.2429,36.1673,2.4112,29.5196,122.9520,0.1501,28.50,13.1653,31.5837,2.1056,27.7205,97.7721,0.1201,38.02";
        let sample = parse_csv_line(line, 500).expect("Should parse sample");
        assert_eq!(sample.timestamp_ms, 500);
        assert_eq!(sample.batt.v, 12.2429);
        assert_eq!(sample.batt.i, 2.4112);
        assert_eq!(sample.fc.v, 13.1653);
        assert_eq!(sample.fc.i, 2.1056);
        assert_eq!(sample.batt.t, 28.50);
        assert_eq!(sample.fc.t, 38.02);
    }

    #[test]
    fn demo_dataset_contains_anomalies_and_regen() {
        let file = File::open("data.csv").expect("data.csv must exist in project root");
        let reader = BufReader::new(file);
        let config = AppConfig::default();

        let mut sample_count = 0;
        let mut overcurrent_detected = false;
        let mut vsag_detected = false;
        let mut overtemp_detected = false;
        let mut regen_detected = false;

        for (idx, line) in reader.lines().skip(1).enumerate() {
            let line = line.expect("Read line");
            if line.trim().is_empty() {
                continue;
            }
            sample_count += 1;
            let sample = parse_csv_line(&line, (idx as u32) * 100).expect("Parse line");

            if sample.batt.i < -0.2 {
                regen_detected = true;
            }

            for anom in anomaly::detect_anomalies(&sample, &config) {
                match anom.kind {
                    AnomalyKind::BattOvercurrent => overcurrent_detected = true,
                    AnomalyKind::FcVoltageSag => vsag_detected = true,
                    AnomalyKind::BattOvertemp => overtemp_detected = true,
                    _ => {}
                }
            }
        }

        assert!(sample_count >= 500, "Dataset should have at least 500 samples (got {})", sample_count);
        assert!(overcurrent_detected, "Dataset should contain BATT OVERCURRENT anomaly");
        assert!(vsag_detected, "Dataset should contain FC V-SAG anomaly");
        assert!(overtemp_detected, "Dataset should contain BATT OVERTEMP anomaly");
        assert!(regen_detected, "Dataset should contain regenerative braking (negative current)");
    }
}