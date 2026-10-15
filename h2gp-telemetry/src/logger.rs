//! # Thread-Safe Telemetry File Logging
//!
//! Provides thread-safe, buffered CSV and anomaly logging utilizing a global registry
//! of `Arc<Mutex<BufWriter<File>>>` handles managed by `OnceLock`.
//!
//! Handles fail gracefully with console error diagnostics instead of panicking,
//! ensuring race telemetry continues unimpeded even if disk operations encounter issues.

use crate::protocol::TelemetrySample;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex, OnceLock};

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
    let is_empty = !file_exists || std::fs::metadata(filename).map(|m| m.len()).unwrap_or(0) == 0;

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
        sample.batt.v,
        sample.batt.sv_mv,
        sample.batt.i,
        sample.batt.p,
        sample.batt.e,
        sample.batt.ah,
        sample.batt.t,
        sample.fc.v,
        sample.fc.sv_mv,
        sample.fc.i,
        sample.fc.p,
        sample.fc.e,
        sample.fc.ah,
        sample.fc.t
    ) {
        eprintln!("[logger] Write to '{}' failed: {}", filename, e);
    }
}

/// Logs a detected anomaly event to `anomalies.log` with formatted fixed-width columns.
pub fn log_anomaly(timestamp_ms: u32, value_str: &str, cause_str: &str, filename: &str) {
    let header = format!(
        "{:<20} {:<30} possible cause\n{}",
        "time stamp",
        "value of anomaly",
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
    let _ = writer.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ChannelData, Rev3AuxData, TelemetrySample};

    fn make_test_sample() -> TelemetrySample {
        TelemetrySample {
            timestamp_ms: 5000,
            batt: ChannelData {
                v: 12.5,
                sv_mv: 1.0,
                i: 3.2,
                p: 40.0,
                e: 100.0,
                ah: 2.5,
                t: 28.0,
            },
            fc: ChannelData {
                v: 11.0,
                sv_mv: 0.5,
                i: 1.1,
                p: 12.1,
                e: 50.0,
                ah: 1.0,
                t: 30.0,
            },
            rev3_aux: Rev3AuxData::default(),
            has_channel_data: true,
            has_rev3_aux: false,
        }
    }

    #[test]
    fn test_csv_append_creates_file_with_header() {
        let filename = "test_logger_csv_header.csv";
        let _ = std::fs::remove_file(filename);

        append_to_csv(&make_test_sample(), filename);

        use std::io::Write;
        if let Some(arc) = get_or_create_writer(filename, None) {
            let mut w = arc.lock().unwrap();
            let _ = w.flush();
        }

        // Force flush by dropping global state - read what was written
        // The file should exist and contain a header
        let content = std::fs::read_to_string(filename).unwrap_or_default();
        assert!(
            content.contains("batt_v"),
            "CSV header should contain 'batt_v'"
        );
        let _ = std::fs::remove_file(filename);
    }

    #[test]
    fn test_anomaly_log_format() {
        let filename = "test_anomaly_log_format.log";
        let _ = std::fs::remove_file(filename);

        log_anomaly(65000, "15.2 A", "Overcurrent", filename);

        let content = std::fs::read_to_string(filename).unwrap_or_default();
        assert!(
            content.contains("01:05"),
            "Should contain formatted time 01:05 for 65000ms"
        );
        assert!(
            content.contains("15.2 A"),
            "Should contain the value string"
        );
        assert!(
            content.contains("Overcurrent"),
            "Should contain the cause string"
        );
        let _ = std::fs::remove_file(filename);
    }

    #[test]
    fn test_anomaly_flush_persists_immediately() {
        let filename = "test_anomaly_flush.log";
        let _ = std::fs::remove_file(filename);

        log_anomaly(1000, "9.5 V", "Voltage sag", filename);

        // Read immediately - should be on disk due to flush()
        let content = std::fs::read_to_string(filename).unwrap_or_default();
        assert!(
            !content.is_empty(),
            "Anomaly should be flushed to disk immediately"
        );
        let _ = std::fs::remove_file(filename);
    }
}
