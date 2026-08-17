use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use crate::protocol::{ChannelData, TelemetrySample};

pub fn start_demo_thread(filename: &str, tx: Sender<TelemetrySample>) {
    let filename = filename.to_string();
    thread::spawn(move || {
        let file = match File::open(&filename) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to open demo file {}: {}", filename, e);
                return;
            }
        };

        // 1. Read all rows from the CSV into memory once
        let reader = BufReader::new(file);
        let raw_lines: Vec<String> = reader.lines().skip(1).filter_map(|l| l.ok()).collect();

        if raw_lines.is_empty() {
            eprintln!("Demo file {} has no data rows!", filename);
            return;
        }

        // 2. Create a perfectly monotonic simulated timeline
        let mut simulated_time_ms: u32 = 0;

        // 3. Loop indefinitely
        'outer: loop {
            for line in &raw_lines {
                let mut fields = line.split(',');

                let mut next_f64 = || {
                    fields.next().unwrap_or("0").trim().parse::<f64>().unwrap_or(0.0)
                };

                // WE IGNORE the jagged CSV timestamp completely to prevent graph resets
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
                    // Use our guaranteed smooth timeline
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

                if tx.send(sample).is_err() {
                    break 'outer; // UI was closed
                }

                // Advance simulated time by exactly 100ms (matching the 10 Hz playback)
                simulated_time_ms += 100;
                thread::sleep(Duration::from_millis(100)); 
            }
        }
    });
}