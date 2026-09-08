//! # H2GP Telemetry Dashboard — Main Application Entry Point
//!
//! Provides the primary desktop application lifecycle, asynchronous data reception
//! via `std::sync::mpsc`, anomaly detection logic, and the immediate-mode rendering loop
//! using `eframe` / `egui`.

mod protocol;
mod serial;
mod logger;
mod datamanager;
mod demo;
mod error;
mod ui;

use eframe::egui;
use protocol::TelemetrySample;
use std::sync::mpsc::{self, Receiver, Sender};
use std::collections::VecDeque;
use ui::{ChartTab, FanMode};

/// The primary state container for the H2GP telemetry dashboard.
///
/// Holds the active communication channels, recent incoming telemetry samples,
/// the circular buffer manager (`DataManager`), UI configuration parameters,
/// and a bounded FIFO queue of recent anomaly alerts.
pub struct TelemetryApp {
    /// Channel receiver for incoming telemetry samples from the serial or demo worker threads.
    pub rx: Receiver<TelemetrySample>,
    /// Cloning sender handle passed to newly spawned worker threads (serial or demo).
    pub telemetry_tx: Sender<TelemetrySample>,
    /// Optional uplink command sender for transmitting JSON commands to the car MCU.
    pub cmd_tx: Option<Sender<String>>,
    /// The most recent main channel telemetry sample received from the car.
    pub latest_sample: Option<TelemetrySample>,
    /// The most recent auxiliary telemetry sample (external temps, flags, fan duty).
    pub latest_aux: Option<protocol::Rev3AuxData>,
    /// In-memory historical buffer manager with min/max tracking and moving averages.
    pub data_manager: datamanager::DataManager,
    /// Currently configured serial port name (e.g., "COM3" or "/dev/ttyUSB0").
    pub port_name: String,
    /// Connection indicator flag representing live telemetry stream presence.
    pub is_connected: bool,

    /// Selected fan operational mode (`Auto`, `Manual`, or `Off`).
    pub fan_mode: FanMode,
    /// Manual fan duty cycle percentage (0–100%).
    pub fan_duty: i32,
    /// Three-letter driver identifier code sent to the onboard matrix display.
    pub driver_code: String,
    
    /// Active telemetry parameter tab rendered in the history chart.
    pub chart_tab: ChartTab,
    /// Time window width in seconds displayed on the horizontal axis of the chart.
    pub chart_window: f64,
    
    /// Total count of valid telemetry packets decoded during the session.
    pub packet_count: u32,
    /// Toggle flag controlling visibility of the bottom diagnostics & raw packet panel.
    pub show_diagnostics: bool,
    
    /// Bounded FIFO queue (maximum 10 entries) of formatted anomaly alert messages for the UI.
    pub recent_anomalies: VecDeque<String>, 
}

impl Default for TelemetryApp {
    fn default() -> Self {
        let (telemetry_tx, rx) = mpsc::channel();
        Self {
            rx,
            telemetry_tx,
            cmd_tx: None,
            latest_sample: None,
            latest_aux: None,
            data_manager: datamanager::DataManager::new(1200),
            port_name: "COM3".to_string(),
            is_connected: false,
            fan_mode: FanMode::Auto,
            fan_duty: 70,
            driver_code: "SKL".to_string(),
            chart_tab: ChartTab::Voltage,
            chart_window: 30.0,
            packet_count: 0,
            show_diagnostics: false,
            recent_anomalies: VecDeque::with_capacity(10),
        }
    }
}

impl TelemetryApp {
    // Helper to log to file AND push to the UI queue simultaneously
    fn record_anomaly(&mut self, ts: u32, val: &str, cause: &str, anom_type: &str) {
        logger::log_anomaly(ts, val, cause);
        
        let secs = ts / 1000;
        let m = secs / 60;
        let s = secs % 60;
        let log_str = format!("[{:02}:{:02}] {} : {}", m, s, anom_type, val);
        
        // Strictly bound memory to 10 strings
        if self.recent_anomalies.len() >= 10 {
            self.recent_anomalies.pop_front();
        }
        self.recent_anomalies.push_back(log_str);
    }
}

impl eframe::App for TelemetryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        
        while let Ok(sample) = self.rx.try_recv() {
            self.packet_count += 1;
            
            if sample.has_channel_data {
                logger::append_to_csv(&sample, "data.csv");
                self.data_manager.add_data(sample.clone());
                
                // Anomaly Engine utilizing the new helper
                if sample.batt.i > 15.0 {
                    self.record_anomaly(sample.timestamp_ms, &format!("{:.2} A", sample.batt.i), "Servo stall", "BATT OVERCURRENT");
                }
                if sample.fc.v < 9.0 && sample.fc.v > 2.0 { 
                    self.record_anomaly(sample.timestamp_ms, &format!("{:.2} V", sample.fc.v), "Starvation", "FC V-SAG");
                }
                if sample.batt.t > 45.0 {
                    self.record_anomaly(sample.timestamp_ms, &format!("{:.1} C", sample.batt.t), "High load", "BATT OVERTEMP");
                }

                self.latest_sample = Some(sample);
                self.is_connected = true;
                
            } else if sample.has_rev3_aux {
                if (sample.rev3_aux.flags & 0x01) != 0 {
                    self.record_anomaly(sample.timestamp_ms, "ZKRACOVANI", "Commanded FC short", "FC SHORT");
                }
                self.latest_aux = Some(sample.rev3_aux);
            }
        }

        ui::render_dashboard(self, ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "H2Gp Telemetry Dashboard",
        options,
        Box::new(|cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.window_fill = egui::Color32::from_rgb(10, 10, 10);
            visuals.panel_fill = egui::Color32::from_rgb(10, 10, 10);
            visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(18, 18, 18);
            cc.egui_ctx.set_visuals(visuals);

            Ok(Box::<TelemetryApp>::default())
        }),
    )
}