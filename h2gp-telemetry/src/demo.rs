use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use crate::protocol::{ChannelData, TelemetrySample};

pub fn start_demo_thread(filename: &str, tx: Sender<TelemetrySample>) {
    let filename = filename.to_string();
    thread::spawn(move || {
        println!("Demo reader thread started using file: {}", filename);
        
        loop {
            let file = match File::open(&filename) {
                Ok(f) => f,
                Err(_) => {
                    thread::sleep(Duration::from_millis(1000));
                    continue;
                }
            };
            
            let reader = BufReader::new(file);
            let mut lines = reader.lines();

            // Skip the CSV header row
            let _ = lines.next();

            for line in lines {
                if let Ok(content) = line {
                    let fields: Vec<&str> = content.split(',').collect();
                    if fields.len() >= 15 {
                        let timestamp = fields[0].to_string();
                        let parse_f64 = |s: &str| s.trim().parse::<f64>().unwrap_or(0.0);

                        let batt = ChannelData {
                            v: parse_f64(fields[1]),
                            shunt_mv: parse_f64(fields[2]),
                            i: parse_f64(fields[3]),
                            p: parse_f64(fields[4]),
                            e: parse_f64(fields[5]),
                            c: parse_f64(fields[6]),
                            t: parse_f64(fields[7]),
                            ah: parse_f64(fields[6]) / 3600.0,
                        };

                        let fc = ChannelData {
                            v: parse_f64(fields[8]),
                            shunt_mv: parse_f64(fields[9]),
                            i: parse_f64(fields[10]),
                            p: parse_f64(fields[11]),
                            e: parse_f64(fields[12]),
                            c: parse_f64(fields[13]),
                            t: parse_f64(fields[14]),
                            ah: parse_f64(fields[13]) / 3600.0,
                        };

                        let sample = TelemetrySample {
                            timestamp,
                            batt,
                            fc,
                            rev3_aux: Default::default(),
                            has_channel_data: true,
                            has_rev3_aux: false,
                        };

                        if tx.send(sample).is_err() {
                            return;
                        }
                        
                        // Adjusted playback delay to 250 ms for smoother analysis
                        thread::sleep(Duration::from_millis(250));
                    }
                }
            }
        }
    });
}