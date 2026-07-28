use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use crate::protocol::TelemetrySample;

pub fn append_to_csv(sample: &TelemetrySample, filename: &str) {
    if !sample.has_channel_data {
        return;
    }

    let path = Path::new(filename);
    let file_exists = path.exists();

    let mut file = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(f) => f,
        err => {
            eprintln!("Failed to open log file: {:?}", err);
            return;
        }
    };

    // Write CSV header if the file is brand new
    if !file_exists || path.metadata().map(|m| m.len() == 0).unwrap_or(false) {
        let header = "timestamp,batt_v,batt_sv_mv,batt_i,batt_p,batt_e,batt_c,batt_t,\
                      fc_v,fc_sv_mv,fc_i,fc_p,fc_e,fc_c,fc_t\n";
        let _ = file.write_all(header.as_bytes());
    }

    let row = format!(
        "{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2}\n",
        sample.timestamp,
        sample.batt.v,
        sample.batt.shunt_mv,
        sample.batt.i,
        sample.batt.p,
        sample.batt.e,
        sample.batt.c,
        sample.batt.t,
        sample.fc.v,
        0.0, // fc_sv_mv placeholder
        sample.fc.i,
        sample.fc.p,
        sample.fc.e,
        sample.fc.c,
        sample.fc.t
    );

    let _ = file.write_all(row.as_bytes());
}