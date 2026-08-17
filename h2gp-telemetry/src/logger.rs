use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex, OnceLock};
use crate::protocol::TelemetrySample;

// Wrap the file writer in its own Arc<Mutex> so we don't lock the global registry
type LogWriter = Arc<Mutex<BufWriter<File>>>;
static LOGGERS: OnceLock<Mutex<HashMap<String, LogWriter>>> = OnceLock::new();

pub fn append_to_csv(sample: &TelemetrySample, filename: &str) {
    // Phase 1: Retrieve or create the file handle, then immediately release the global map lock.
    let writer_arc = {
        let mut loggers = LOGGERS.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap();
        loggers.entry(filename.to_string()).or_insert_with(|| {
            let file_exists = std::path::Path::new(filename).exists();
            let is_empty = !file_exists || std::fs::metadata(filename).map(|m| m.len()).unwrap_or(0) == 0;

            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(filename)
                .expect("Failed to open telemetry CSV");
            
            let mut writer = BufWriter::with_capacity(8192, file);
            
            if is_empty {
                writeln!(writer, "timestamp,batt_v,batt_sv_mv,batt_i,batt_p,batt_e,batt_ah,batt_t,fc_v,fc_sv_mv,fc_i,fc_p,fc_e,fc_ah,fc_t").unwrap();
            }
            Arc::new(Mutex::new(writer))
        }).clone()
    }; // Global HashMap lock is released here!

    // Phase 2: Lock only this specific file and write to the RAM buffer.
    // Notice we REMOVED the manual `.flush()`. The OS will now handle it silently in the background.
    let mut writer = writer_arc.lock().unwrap();
    writeln!(
        writer,
        "{:.1},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2}",
        sample.timestamp_ms as f64 / 1000.0,
        sample.batt.v, sample.batt.sv_mv, sample.batt.i, sample.batt.p, sample.batt.e, sample.batt.ah, sample.batt.t,
        sample.fc.v, sample.fc.sv_mv, sample.fc.i, sample.fc.p, sample.fc.e, sample.fc.ah, sample.fc.t
    ).unwrap();
}