use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use crate::protocol::{ChannelData, TelemetrySample};

pub fn start_demo_thread(filename: &str, tx: Sender<TelemetrySample>) {
    let filename = filename.to_string();
    thread::spawn(move || {
        let file = File::open(filename).expect("Failed to open data.csv");
        let reader = BufReader::new(file);
        let mut lines = reader.lines().skip(1); // skip header

        while let Some(Ok(line)) = lines.next() {
            let fields: Vec<&str> = line.split(',').collect();
            if fields.len() >= 15 {
                let parse_f64 = |s: &str| s.trim().parse::<f64>().unwrap_or(0.0);
                
                let sample = TelemetrySample {
                    timestamp_ms: (parse_f64(fields[0]) * 1000.0) as u32,
                    has_channel_data: true,
                    batt: ChannelData {
                        v: parse_f64(fields[1]),
                        sv_mv: parse_f64(fields[2]),
                        i: parse_f64(fields[3]),
                        p: parse_f64(fields[4]),
                        e: parse_f64(fields[5]),
                        ah: parse_f64(fields[6]),
                        t: parse_f64(fields[7]),
                    },
                    fc: ChannelData {
                        v: parse_f64(fields[8]),
                        sv_mv: parse_f64(fields[9]),
                        i: parse_f64(fields[10]),
                        p: parse_f64(fields[11]),
                        e: parse_f64(fields[12]),
                        ah: parse_f64(fields[13]),
                        t: parse_f64(fields[14]),
                    },
                    has_rev3_aux: false,
                    rev3_aux: Default::default(),
                };

                if tx.send(sample).is_err() {
                    break; // Dashboard was closed
                }
            }
            thread::sleep(Duration::from_millis(100)); // Playback speed
        }
    });
}