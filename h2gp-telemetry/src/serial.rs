use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;
use crate::protocol::{TelemetrySample, decode_ina_channel, decode_rev3_aux, TELEMETRY_PACKET_SIZE, TELEMETRY_KIND_AUX};

const USB_FRAME_MAGIC: u32 = 0x50473248; // "H2GP" little-endian
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

        let mut buffer: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 256];

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
                        let magic = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
                        if magic != USB_FRAME_MAGIC {
                            buffer.remove(0);
                            continue;
                        }

                        let kind = buffer[4];
                        let payload_len = u16::from_le_bytes([buffer[6], buffer[7]]) as usize;
                        let frame_length = USB_HEADER_SIZE + payload_len;

                        if buffer.len() < frame_length {
                            break;
                        }

                        let payload = buffer[USB_HEADER_SIZE..frame_length].to_vec();
                        buffer.drain(0..frame_length);

                        if kind == USB_KIND_MAIN_TELEMETRY && payload.len() == TELEMETRY_PACKET_SIZE {
                            let batt_data = decode_ina_channel(&payload[16..44]);
                            let fc_data = decode_ina_channel(&payload[44..72]);

                            let sample = TelemetrySample {
                                timestamp: "Live".to_string(),
                                batt: batt_data,
                                fc: fc_data,
                                rev3_aux: Default::default(),
                                has_channel_data: true,
                                has_rev3_aux: false,
                            };

                            let _ = tx.send(sample);
                        } else if kind == TELEMETRY_KIND_AUX {
                            let aux_data = decode_rev3_aux(&payload);
                            let sample = TelemetrySample {
                                timestamp: "Live".to_string(),
                                batt: Default::default(),
                                fc: Default::default(),
                                rev3_aux: aux_data,
                                has_channel_data: false,
                                has_rev3_aux: true,
                            };

                            let _ = tx.send(sample);
                        }
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