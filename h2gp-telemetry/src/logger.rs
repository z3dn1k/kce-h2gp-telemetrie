use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex, OnceLock};
use crate::protocol::TelemetrySample;

// Wrap the file writer in its own Arc<Mutex> so we don't lock the global registry
type LogWriter = Arc<Mutex<BufWriter<File>>>;
static LOGGERS: OnceLock<Mutex<HashMap<String, LogWriter>>> = OnceLock::new();

pub fn append_to_csv(sample: &TelemetrySample, filename: &str) {
    // Phase 1: Retrieve or create the file handle without using .unwrap() or .expect()
    let writer_arc = {
        // unwrap_or_else gracefully recovers the mutex even if a previous thread crashed holding it
        let mut loggers = LOGGERS.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap_or_else(|e| e.into_inner());
        
        if !loggers.contains_key(filename) {
            let file_exists = std::path::Path::new(filename).exists();
            let is_empty = !file_exists || std::fs::metadata(filename).map(|m| m.len()).unwrap_or(0) == 0;

            match OpenOptions::new().create(true).append(true).open(filename) {
                Ok(file) => {
                    let mut writer = BufWriter::with_capacity(8192, file);
                    if is_empty {
                        if let Err(e) = writeln!(writer, "timestamp,batt_v,batt_sv_mv,batt_i,batt_p,batt_e,batt_ah,batt_t,fc_v,fc_sv_mv,fc_i,fc_p,fc_e,fc_ah,fc_t") {
                            eprintln!("Failed to write CSV header: {}", e);
                        }
                    }
                    loggers.insert(filename.to_string(), Arc::new(Mutex::new(writer)));
                }
                Err(e) => {
                    // Graceful failure: Print to console and abort the write attempt without crashing
                    eprintln!("Failed to open {} for logging: {}", filename, e);
                    return; 
                }
            }
        }
        
        // Safe to unwrap here because we guarantee it was either inserted above or we returned early
        loggers.get(filename).unwrap().clone()
    }; 

    // Phase 2: Lock only this specific file and write safely
    let mut writer = writer_arc.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = writeln!(
        writer,
        "{:.1},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2}",
        sample.timestamp_ms as f64 / 1000.0,
        sample.batt.v, sample.batt.sv_mv, sample.batt.i, sample.batt.p, sample.batt.e, sample.batt.ah, sample.batt.t,
        sample.fc.v, sample.fc.sv_mv, sample.fc.i, sample.fc.p, sample.fc.e, sample.fc.ah, sample.fc.t
    ) {
        eprintln!("Failed to write telemetry data to CSV: {}", e);
    }
}

pub fn log_anomaly(timestamp_ms: u32, value_str: &str, cause_str: &str) {
    let filename = "anomalies.log";
    
    let writer_arc = {
        let mut loggers = LOGGERS.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap_or_else(|e| e.into_inner());
        
        if !loggers.contains_key(filename) {
            let file_exists = std::path::Path::new(filename).exists();
            let is_empty = !file_exists || std::fs::metadata(filename).map(|m| m.len()).unwrap_or(0) == 0;

            match OpenOptions::new().create(true).append(true).open(filename) {
                Ok(file) => {
                    let mut writer = BufWriter::with_capacity(4096, file);
                    if is_empty {
                        let _ = writeln!(writer, "{:<20} {:<30} {}", "time stamp", "value of anomaly", "possible cause");
                        let _ = writeln!(writer, "-------------------------------------------------------------------------------");
                    }
                    loggers.insert(filename.to_string(), Arc::new(Mutex::new(writer)));
                }
                Err(e) => {
                    eprintln!("Failed to open anomaly log: {}", e);
                    return;
                }
            }
        }
        loggers.get(filename).unwrap().clone()
    };

    let total_secs = timestamp_ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let time_str = format!("{:02}:{:02}", mins, secs);

    let mut writer = writer_arc.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = writeln!(writer, "{:<20} {:<30} {}", time_str, value_str, cause_str) {
        eprintln!("Failed to write to anomaly log: {}", e);
    }
}