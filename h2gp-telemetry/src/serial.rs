//! # USB Serial Port Communication & Frame Stream Synchronization
//!
//! Handles low-level I/O over the USB Virtual COM port, frame synchronization
//! searching for ASCII `"H2GP"` magic bytes (`0x48`, `0x32`, `0x47`, `0x50`),
//! binary packet unpacking, and bidirectional command uplink to the car's MCU.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;
use crate::protocol::{TelemetrySample, decode_ina_channel, decode_rev3_aux, TELEMETRY_PACKET_SIZE, TELEMETRY_KIND_AUX};
use crate::error::TelemetryError;
use crate::logger;

const USB_FRAME_MAGIC: [u8; 4] = [0x48, 0x32, 0x47, 0x50]; // "H2GP" in ASCII
const USB_HEADER_SIZE: usize = 10;
const USB_KIND_MAIN_TELEMETRY: u8 = 1;

/// Scans the operating system for currently connected serial communication ports.
///
/// Returns a list of system port names (e.g. `["/dev/ttyUSB0", "/dev/ttyACM0"]` on Linux,
/// or `["COM3", "COM4"]` on Windows). If no ports are found or scanning fails, returns an empty vector.
pub fn detect_available_ports() -> Vec<String> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.port_name)
        .collect()
}

/// Spawns a background thread to manage USB serial I/O.
///
/// Continuously reads from the specified serial port while `is_running` is true,
/// re-aligns stream frames on sync sequence boundaries, decodes telemetry samples,
/// and dispatches them to the UI thread via `tx`. Concurrently polls `cmd_rx` to
/// transmit uplink JSON commands back to the microcontroller.
pub fn start_serial_thread(
    port_name: String, 
    baud_rate: u32, 
    tx: Sender<TelemetrySample>,
    cmd_rx: Receiver<String>,
    is_running: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        if let Err(e) = run_serial_loop(&port_name, baud_rate, &tx, &cmd_rx, &is_running) {
            eprintln!("[serial] {}", e);
        }
        println!("[serial] Thread terminated cleanly.");
    });
}

/// Inner serial loop extracted as a function returning Result.
///
/// This pattern keeps the thread spawn boilerplate minimal while allowing
/// the core logic to use the `?` operator for clean, readable error propagation.
fn run_serial_loop(
    port_name: &str,
    baud_rate: u32,
    tx: &Sender<TelemetrySample>,
    cmd_rx: &Receiver<String>,
    is_running: &AtomicBool,
) -> Result<(), TelemetryError> {
    println!("Serial thread started on port: {} at {} baud", port_name, baud_rate);

    let mut port = serialport::new(port_name, baud_rate)
        .timeout(Duration::from_millis(20))
        .open()
        .map_err(|e| TelemetryError::SerialOpen {
            port: port_name.to_string(),
            baud: baud_rate,
            source: e,
        })?;

    let mut clone_port = port.try_clone().map_err(|e| TelemetryError::SerialClone {
        port: port_name.to_string(),
        source: e,
    })?;

    let mut buffer: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk = [0u8; 1024]; 
    let serial_start_time = std::time::Instant::now(); 

    while is_running.load(Ordering::Relaxed) {
        // Check for outgoing commands from the UI
        while let Ok(command_str) = cmd_rx.try_recv() {
            let formatted = format!("{}\n", command_str);
            if let Err(e) = clone_port.write_all(formatted.as_bytes()) {
                eprintln!("[serial] Failed to send command '{}': {}", command_str, e);
            } else {
                println!("[serial] TX → MCU: {}", command_str);
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
                            timestamp_ms: serial_start_time.elapsed().as_millis() as u32,
                            batt: batt_data,
                            fc: fc_data,
                            rev3_aux: Default::default(),
                            has_channel_data: true,
                            has_rev3_aux: false,
                        };

                        logger::append_to_csv(&sample, "data.csv");
                        tx.send(sample).map_err(|_| TelemetryError::ChannelDisconnected)?;
                        
                    } else if kind == TELEMETRY_KIND_AUX {
                        let aux_data = decode_rev3_aux(payload);
                        let sample = TelemetrySample {
                            timestamp_ms: serial_start_time.elapsed().as_millis() as u32,
                            batt: Default::default(),
                            fc: Default::default(),
                            rev3_aux: aux_data,
                            has_channel_data: false,
                            has_rev3_aux: true,
                        };

                        tx.send(sample).map_err(|_| TelemetryError::ChannelDisconnected)?;
                    }

                    buffer.drain(0..frame_length);
                }
            }
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                eprintln!("[serial] Read error: {}", e);
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_available_ports_does_not_panic() {
        let ports = detect_available_ports();
        // Just verify it returns a valid vector without crashing
        let _ = ports.len();
    }
}