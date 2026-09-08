//! # Thread-Safe Telemetry File Logging
//!
//! Provides thread-safe, buffered CSV and anomaly logging utilizing a global registry
//! of `Arc<Mutex<BufWriter<File>>>` handles managed by `OnceLock`.
//!
//! Handles fail gracefully with console error diagnostics instead of panicking,
//! ensuring race telemetry continues unimpeded even if disk operations encounter issues.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex, OnceLock};
use crate::protocol::TelemetrySample;

/// Each log file gets its own `Arc<Mutex<BufWriter>>` so that writes to
/// different files never contend on the same lock.
type LogWriter = Arc<Mutex<BufWriter<File>>>;
static LOGGERS: OnceLock<Mutex<HashMap<String, LogWriter>>> = OnceLock::new();

/// Retrieves or lazily creates a buffered writer for the given filename.
///
/// Returns `None` if the file cannot be opened — the caller should skip
/// the write rather than crash.
fn get_or_create_writer(filename: &str, header: Option<&str>) -> Option<LogWriter> {
    let mut loggers = LOGGERS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    if let Some(writer) = loggers.get(filename) {
        return Some(writer.clone());
    }

    // First access — open (or create) the file
    let file_exists = std::path::Path::new(filename).exists();
    let is_empty = !file_exists
        || std::fs::metadata(filename)
            .map(|m| m.len())
            .unwrap_or(0)
            == 0;

    let file = match OpenOptions::new().create(true).append(true).open(filename) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[logger] Cannot open '{}': {}", filename, e);
            return None;
        }
    };

    let mut writer = BufWriter::with_capacity(8192, file);

    if is_empty {
        if let Some(hdr) = header {
            if let Err(e) = writeln!(writer, "{}", hdr) {
                eprintln!("[logger] Cannot write header to '{}': {}", filename, e);
            }
        }
    }

    let arc = Arc::new(Mutex::new(writer));
    loggers.insert(filename.to_string(), arc.clone());
    Some(arc)
}

/// Appends a telemetry sample as a comma-separated row in the designated CSV file.
///
/// If the file is newly created, automatically writes the CSV column header row first.
pub fn append_to_csv(sample: &TelemetrySample, filename: &str) {
    let header = "timestamp,batt_v,batt_sv_mv,batt_i,batt_p,batt_e,batt_ah,batt_t,\
                   fc_v,fc_sv_mv,fc_i,fc_p,fc_e,fc_ah,fc_t";

    let Some(writer_arc) = get_or_create_writer(filename, Some(header)) else {
        return;
    };

    let mut writer = writer_arc.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = writeln!(
        writer,
        "{:.1},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2}",
        sample.timestamp_ms as f64 / 1000.0,
        sample.batt.v, sample.batt.sv_mv, sample.batt.i, sample.batt.p, sample.batt.e, sample.batt.ah, sample.batt.t,
        sample.fc.v, sample.fc.sv_mv, sample.fc.i, sample.fc.p, sample.fc.e, sample.fc.ah, sample.fc.t
    ) {
        eprintln!("[logger] Write to '{}' failed: {}", filename, e);
    }
}

/// Logs a detected anomaly event to `anomalies.log` with formatted fixed-width columns.
pub fn log_anomaly(timestamp_ms: u32, value_str: &str, cause_str: &str) {
    let filename = "anomalies.log";
    let header = format!(
        "{:<20} {:<30} possible cause\n{}",
        "time stamp", "value of anomaly",
        "-------------------------------------------------------------------------------"
    );

    let Some(writer_arc) = get_or_create_writer(filename, Some(&header)) else {
        return;
    };

    let total_secs = timestamp_ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let time_str = format!("{:02}:{:02}", mins, secs);

    let mut writer = writer_arc.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = writeln!(writer, "{:<20} {:<30} {}", time_str, value_str, cause_str) {
        eprintln!("[logger] Write to '{}' failed: {}", filename, e);
    }
}