//! # Offline Replay Simulation (Demo Mode)
//!
//! Replays recorded telemetry data from a CSV log file at a steady 10 Hz rate.
//! Employs a synthetic monotonic clock to guarantee smooth graph rendering
//! independent of recording timestamp jitter.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use crate::protocol::{ChannelData, TelemetrySample};
use crate::error::TelemetryError;

/// Spawns a dedicated worker thread that replays telemetry rows from a CSV file.
///
/// Reads all lines into memory and continuously transmits reconstituted `TelemetrySample`
/// structs into `tx` at 100 ms intervals (10 Hz). Loops indefinitely until the receiver is closed.
pub fn start_demo_thread(filename: &str, tx: Sender<TelemetrySample>) {
    let filename = filename.to_string();
    thread::spawn(move || {
        if let Err(e) = run_demo_loop(&filename, &tx) {
            eprintln!("[demo] {}", e);
        }
    });
}

/// Inner demo loop extracted as a function returning Result.
///
/// Opens a recorded CSV file, ignores its original timestamps, and replays
/// the data at a smooth 10 Hz with a synthetic monotonic clock.
/// Loops forever until the UI channel disconnects.
fn run_demo_loop(
    filename: &str,
    tx: &Sender<TelemetrySample>,
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

    // Loop indefinitely
    loop {
        for line in &raw_lines {
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

            let sample = TelemetrySample {
                timestamp_ms: simulated_time_ms,
                has_channel_data: true,
                batt: ChannelData {
                    v: b_v, sv_mv: b_sv, i: b_i, p: b_p, e: b_e, ah: b_ah, t: b_t,
                },
                fc: ChannelData {
                    v: f_v, sv_mv: f_sv, i: f_i, p: f_p, e: f_e, ah: f_ah, t: f_t,
                },
                has_rev3_aux: false,
                rev3_aux: Default::default(),
            };

            tx.send(sample).map_err(|_| TelemetryError::ChannelDisconnected)?;

            // Advance simulated time by exactly 100ms (matching the 10 Hz playback)
            simulated_time_ms += 100;
            thread::sleep(Duration::from_millis(100)); 
        }
    }
}