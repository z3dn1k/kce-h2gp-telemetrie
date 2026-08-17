use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;
use crate::protocol::{TelemetrySample, decode_ina_channel, decode_rev3_aux, TELEMETRY_PACKET_SIZE, TELEMETRY_KIND_AUX};
use crate::logger;

const USB_FRAME_MAGIC: [u8; 4] = [0x48, 0x32, 0x47, 0x50]; // "H2GP" little-endian
const USB_HEADER_SIZE: usize = 10;
const USB_KIND_MAIN_TELEMETRY: u8 = 1;

pub fn start_serial_thread(
    port_name: String, 
    baud_rate: u32, 
    tx: Sender<TelemetrySample>,
    cmd_rx: Receiver<String>
) {
    std::thread::spawn(move || {
        println!("Serial thread started on port: {} at {} baud", port_name, baud_rate);

        let port = serialport::new(&port_name, baud_rate)
            .timeout(Duration::from_millis(20))
            .open();

        let mut port = match port {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Failed to open serial port {}: {}", port_name, e);
                return;
            }
        };

        let mut clone_port = match port.try_clone() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Failed to clone serial port for writing: {}", e);
                return;
            }
        };

        let mut buffer: Vec<u8> = Vec::with_capacity(4096);
        let mut chunk = [0u8; 1024]; 

        // FIX 2: Start tracking time specifically on the serial thread
        let serial_start_time = std::time::Instant::now(); 

        loop {
            // Check for outgoing commands from the UI
            while let Ok(command_str) = cmd_rx.try_recv() {
                let formatted = format!("{}\n", command_str);
                if let Err(e) = clone_port.write_all(formatted.as_bytes()) {
                    eprintln!("Failed to write command to serial port: {}", e);
                } else {
                    println!("Sent command to MCU: {}", command_str);
                }
            }

            // Read incoming data from the serial port
            match port.read(&mut chunk) {
                Ok(n) if n > 0 => {
                    buffer.extend_from_slice(&chunk[..n]);

                    while buffer.len() >= USB_HEADER_SIZE {
                        if let Some(pos) = buffer.windows(4).position(|w| w == USB_FRAME_MAGIC) {
                            if pos > 0 {
                                buffer.drain(0..pos);
                            }
                        } else {
                            let keep = buffer.len().min(3);
                            let discard_to = buffer.len() - keep;
                            buffer.drain(0..discard_to);
                            break; 
                        }

                        if buffer.len() < USB_HEADER_SIZE {
                            break; 
                        }

                        let kind = buffer[4];
                        let payload_len = u16::from_le_bytes([buffer[6], buffer[7]]) as usize;
                        let frame_length = USB_HEADER_SIZE + payload_len;

                        if buffer.len() < frame_length {
                            break; 
                        }

                        let payload = &buffer[USB_HEADER_SIZE..frame_length];

                        if kind == USB_KIND_MAIN_TELEMETRY && payload.len() == TELEMETRY_PACKET_SIZE {
                            let batt_data = decode_ina_channel(&payload[16..44]);
                            let fc_data = decode_ina_channel(&payload[44..72]);

                            let sample = TelemetrySample {
                                // FIX 2: Stamp the time exactly when it's parsed from the buffer
                                timestamp_ms: serial_start_time.elapsed().as_millis() as u32,
                                batt: batt_data,
                                fc: fc_data,
                                rev3_aux: Default::default(),
                                has_channel_data: true,
                                has_rev3_aux: false,
                            };

                            logger::append_to_csv(&sample, "data.csv");
                            let _ = tx.send(sample);
                            
                        } else if kind == TELEMETRY_KIND_AUX {
                            let aux_data = decode_rev3_aux(payload);
                            let sample = TelemetrySample {
                                // FIX 2: Stamp the time exactly when it's parsed from the buffer
                                timestamp_ms: serial_start_time.elapsed().as_millis() as u32,
                                batt: Default::default(),
                                fc: Default::default(),
                                rev3_aux: aux_data,
                                has_channel_data: false,
                                has_rev3_aux: true,
                            };

                            let _ = tx.send(sample);
                        }

                        buffer.drain(0..frame_length);
                    }
                }
                Ok(_) => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => {
                    eprintln!("Serial read error: {}", e);
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    });
}