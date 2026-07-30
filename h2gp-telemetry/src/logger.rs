use std::fs::OpenOptions;
use std::io::Write;
use crate::protocol::TelemetrySample;

pub fn append_to_csv(sample: &TelemetrySample, filename: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(filename)
        .unwrap();

    // Check if file is empty to write header
    if file.metadata().unwrap().len() == 0 {
        writeln!(file, "timestamp,batt_v,batt_sv_mv,batt_i,batt_p,batt_e,batt_ah,batt_t,fc_v,fc_sv_mv,fc_i,fc_p,fc_e,fc_ah,fc_t").unwrap();
    }

    writeln!(
        file,
        "{:.1},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2}",
        sample.timestamp_ms as f64 / 1000.0,
        sample.batt.v, sample.batt.sv_mv, sample.batt.i, sample.batt.p, sample.batt.e, sample.batt.ah, sample.batt.t,
        sample.fc.v, sample.fc.sv_mv, sample.fc.i, sample.fc.p, sample.fc.e, sample.fc.ah, sample.fc.t
    ).unwrap();
}