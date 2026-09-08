//! # H2GP Telemetry Dashboard — Main Application Entry Point
//!
//! Provides the primary desktop application lifecycle, asynchronous data reception
//! via `std::sync::mpsc`, anomaly detection logic, and the immediate-mode rendering loop
//! using `eframe` / `egui`.

mod anomaly;
mod config;
mod protocol;
mod serial;
mod logger;
mod datamanager;
mod demo;
mod error;
mod ui;

use config::AppConfig;
use eframe::egui;
use protocol::TelemetrySample;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;
use std::collections::VecDeque;
use ui::{ChartTab, FanMode};

/// Current connection status of the telemetry feed.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    /// No live data feed or simulation is running.
    Disconnected,
    /// Actively receiving live telemetry over the serial port.
    Live { since: Instant },
    /// Replaying recorded telemetry in Demo Mode.
    DemoMode { since: Instant },
    /// An error occurred on the communications channel.
    Error(String),
}

impl ConnectionState {
    /// Returns true if either live telemetry or demo simulation is active.
    pub fn is_connected(&self) -> bool {
        matches!(self, ConnectionState::Live { .. } | ConnectionState::DemoMode { .. })
    }

    /// Returns a human-readable badge label and theme color for the UI toolbar.
    pub fn status_text(&self) -> (String, egui::Color32) {
        match self {
            ConnectionState::Disconnected => ("● DISCONNECTED".to_string(), egui::Color32::from_rgb(200, 60, 60)),
            ConnectionState::Live { since } => {
                let elapsed = since.elapsed().as_secs();
                let m = elapsed / 60;
                let s = elapsed % 60;
                (format!("● LIVE [{:02}:{:02}]", m, s), egui::Color32::from_rgb(0, 220, 100))
            }
            ConnectionState::DemoMode { since } => {
                let elapsed = since.elapsed().as_secs();
                let m = elapsed / 60;
                let s = elapsed % 60;
                (format!("● DEMO [{:02}:{:02}]", m, s), egui::Color32::from_rgb(80, 170, 255))
            }
            ConnectionState::Error(err) => (format!("● ERROR: {}", err), egui::Color32::from_rgb(255, 120, 40)),
        }
    }
}

/// The primary state container for the H2GP telemetry dashboard.
///
/// Holds the active communication channels, recent incoming telemetry samples,
/// the circular buffer manager (`DataManager`), UI configuration parameters,
/// and a bounded FIFO queue of recent anomaly alerts.
pub struct TelemetryApp {
    /// Active application configuration loaded from `config.toml` or defaults.
    pub config: AppConfig,
    /// Channel receiver for incoming telemetry samples from the serial or demo worker threads.
    pub rx: Receiver<TelemetrySample>,
    /// Cloning sender handle passed to newly spawned worker threads (serial or demo).
    pub telemetry_tx: Sender<TelemetrySample>,
    /// Optional uplink command sender for transmitting JSON commands to the car MCU.
    pub cmd_tx: Option<Sender<String>>,
    /// Shared cancellation flag allowing graceful shutdown of running background threads.
    pub shutdown_signal: Option<Arc<AtomicBool>>,
    /// The most recent main channel telemetry sample received from the car.
    pub latest_sample: Option<TelemetrySample>,
    /// The most recent auxiliary telemetry sample (external temps, flags, fan duty).
    pub latest_aux: Option<protocol::Rev3AuxData>,
    /// In-memory historical buffer manager with min/max tracking and moving averages.
    pub data_manager: datamanager::DataManager,
    /// Currently configured serial port name (e.g., "COM3" or "/dev/ttyUSB0").
    pub port_name: String,
    /// Current connection status of the telemetry feed.
    pub connection_state: ConnectionState,

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
    
    /// List of auto-detected serial ports currently present on the system.
    pub available_ports: Vec<String>,
    /// Pauses additions to the historical data manager buffer for visual inspection.
    pub is_paused: bool,
    /// Measured incoming packet rate in packets per second (Hz).
    pub pps: f64,
    /// Packet counter used to calculate `pps` over 1-second rolling windows.
    pub pps_counter: u32,
    /// Timestamp of the last PPS calculation.
    pub last_pps_update: Instant,
    /// Optional transient toast notification showing the status of screenshots (message, display_until).
    pub screenshot_toast: Option<(String, Instant)>,
    
    /// Bounded FIFO queue of formatted anomaly alert messages for the UI.
    pub recent_anomalies: VecDeque<String>, 
}

impl Default for TelemetryApp {
    fn default() -> Self {
        let config = AppConfig::load_or_default("config.toml");
        let (telemetry_tx, rx) = mpsc::channel();
        let buffer_cap = config.buffer_capacity;
        let default_port = config.default_port.clone();
        let fan_duty = config.default_fan_duty;
        let driver_code = config.default_driver_code.clone();
        let chart_window = config.default_chart_window_s;
        let anom_cap = config.anomaly_queue_capacity;
        let available_ports = serial::detect_available_ports();

        // If configured port is not found or empty, choose the first detected port if any
        let port_name = if !available_ports.is_empty() && (!available_ports.contains(&default_port) || default_port.is_empty()) {
            available_ports[0].clone()
        } else {
            default_port
        };

        Self {
            config,
            rx,
            telemetry_tx,
            cmd_tx: None,
            shutdown_signal: None,
            latest_sample: None,
            latest_aux: None,
            data_manager: datamanager::DataManager::new(buffer_cap),
            port_name,
            connection_state: ConnectionState::Disconnected,
            fan_mode: FanMode::Auto,
            fan_duty,
            driver_code,
            chart_tab: ChartTab::Voltage,
            chart_window,
            packet_count: 0,
            show_diagnostics: false,
            available_ports,
            is_paused: false,
            pps: 0.0,
            pps_counter: 0,
            last_pps_update: Instant::now(),
            screenshot_toast: None,
            recent_anomalies: VecDeque::with_capacity(anom_cap),
        }
    }
}

impl TelemetryApp {
    /// Signals the currently running worker thread (serial or demo) to terminate cleanly.
    pub fn stop_worker(&mut self) {
        if let Some(signal) = self.shutdown_signal.take() {
            signal.store(false, Ordering::Relaxed);
        }
    }

    /// Starts live serial telemetry acquisition using the currently selected `port_name`.
    pub fn start_live(&mut self) {
        self.stop_worker();
        let running = Arc::new(AtomicBool::new(true));
        self.shutdown_signal = Some(running.clone());
        self.connection_state = ConnectionState::Live { since: Instant::now() };
        let (cmd_tx, cmd_rx) = mpsc::channel();
        self.cmd_tx = Some(cmd_tx);
        serial::start_serial_thread(
            self.port_name.clone(),
            self.config.serial_baud_rate,
            self.telemetry_tx.clone(),
            cmd_rx,
            running,
        );
    }

    /// Starts replaying recorded historical telemetry from `data.csv`.
    pub fn start_demo(&mut self) {
        self.stop_worker();
        let running = Arc::new(AtomicBool::new(true));
        self.shutdown_signal = Some(running.clone());
        self.connection_state = ConnectionState::DemoMode { since: Instant::now() };
        demo::start_demo_thread("data.csv", self.telemetry_tx.clone(), running);
    }

    /// Disconnects any active communication threads and resets PPS.
    pub fn disconnect(&mut self) {
        self.stop_worker();
        self.connection_state = ConnectionState::Disconnected;
        self.pps = 0.0;
        self.pps_counter = 0;
    }

    /// Toggles the pause state of graph data streaming.
    pub fn toggle_pause(&mut self) {
        self.is_paused = !self.is_paused;
    }

    /// Logs an anomaly event both to the persistent file and to the UI FIFO alert queue.
    fn record_anomaly(&mut self, anom: &anomaly::Anomaly) {
        logger::log_anomaly(anom.timestamp_ms, &anom.value_str, anom.cause);
        
        let log_str = anom.format_ui();
        if self.recent_anomalies.len() >= self.config.anomaly_queue_capacity {
            self.recent_anomalies.pop_front();
        }
        self.recent_anomalies.push_back(log_str);
    }
}

impl Drop for TelemetryApp {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

impl eframe::App for TelemetryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle keyboard shortcuts when focus is not inside an active text input widget
        if !ctx.wants_keyboard_input() {
            ctx.input(|i| {
                if i.key_pressed(egui::Key::Num1) {
                    self.chart_tab = ChartTab::Voltage;
                } else if i.key_pressed(egui::Key::Num2) {
                    self.chart_tab = ChartTab::Current;
                } else if i.key_pressed(egui::Key::Num3) {
                    self.chart_tab = ChartTab::Power;
                } else if i.key_pressed(egui::Key::Num4) {
                    self.chart_tab = ChartTab::Energy;
                } else if i.key_pressed(egui::Key::Space) {
                    self.toggle_pause();
                } else if i.key_pressed(egui::Key::C) {
                    if self.connection_state.is_connected() {
                        self.disconnect();
                    } else {
                        self.start_live();
                    }
                } else if i.key_pressed(egui::Key::D) {
                    if matches!(self.connection_state, ConnectionState::DemoMode { .. }) {
                        self.disconnect();
                    } else {
                        self.start_demo();
                    }
                }
            });
        }

        // Process incoming screenshot events from egui
        for event in &ctx.input(|i| i.raw.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                let width = image.size[0] as u32;
                let height = image.size[1] as u32;
                let mut raw_bytes = Vec::with_capacity((width * height * 4) as usize);
                for pixel in &image.pixels {
                    raw_bytes.extend_from_slice(&pixel.to_array());
                }
                if let Some(rgba_img) = image::RgbaImage::from_raw(width, height, raw_bytes) {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let filename = format!("screenshot_{}.png", ts);
                    match rgba_img.save(&filename) {
                        Ok(()) => {
                            println!("[screenshot] Successfully saved to {}", filename);
                            self.screenshot_toast = Some((
                                format!("📷 Snímek uložen: {}", filename),
                                Instant::now() + std::time::Duration::from_secs(4),
                            ));
                        }
                        Err(e) => {
                            eprintln!("[screenshot] Failed to save {}: {}", filename, e);
                            self.screenshot_toast = Some((
                                format!("❌ Chyba ukládání: {}", e),
                                Instant::now() + std::time::Duration::from_secs(4),
                            ));
                        }
                    }
                }
            }
        }
        
        while let Ok(sample) = self.rx.try_recv() {
            self.packet_count += 1;
            self.pps_counter += 1;
            
            // Evaluate configured anomaly rules via the standalone anomaly engine
            for anom in anomaly::detect_anomalies(&sample, &self.config) {
                self.record_anomaly(&anom);
            }

            if sample.has_channel_data {
                if !self.is_paused {
                    self.data_manager.add_data(sample.clone());
                }

                if self.connection_state == ConnectionState::Disconnected {
                    self.connection_state = ConnectionState::Live { since: Instant::now() };
                }

                self.latest_sample = Some(sample);
                
            } else if sample.has_rev3_aux {
                self.latest_aux = Some(sample.rev3_aux);
            }
        }

        // Update rolling packets-per-second (PPS) metric every 1.0 second
        let elapsed_s = self.last_pps_update.elapsed().as_secs_f64();
        if elapsed_s >= 1.0 {
            if self.connection_state.is_connected() {
                self.pps = self.pps_counter as f64 / elapsed_s;
            } else {
                self.pps = 0.0;
            }
            self.pps_counter = 0;
            self.last_pps_update = Instant::now();
        }

        // Clear expired screenshot toast notification
        if let Some((_, expire_time)) = &self.screenshot_toast {
            if Instant::now() > *expire_time {
                self.screenshot_toast = None;
            }
        }

        ui::render_dashboard(self, ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(self.config.ui_refresh_interval_ms));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_state_is_connected_checks() {
        assert!(!ConnectionState::Disconnected.is_connected());
        assert!(!ConnectionState::Error("failed".into()).is_connected());
        assert!(ConnectionState::Live { since: Instant::now() }.is_connected());
        assert!(ConnectionState::DemoMode { since: Instant::now() }.is_connected());
    }

    #[test]
    fn connection_state_status_text_formatting() {
        let (text, _) = ConnectionState::Disconnected.status_text();
        assert!(text.contains("DISCONNECTED"));

        let (live_text, _) = (ConnectionState::Live { since: Instant::now() }).status_text();
        assert!(live_text.contains("LIVE"));

        let (demo_text, _) = (ConnectionState::DemoMode { since: Instant::now() }).status_text();
        assert!(demo_text.contains("DEMO"));

        let (err_text, _) = (ConnectionState::Error("Port busy".into())).status_text();
        assert!(err_text.contains("Port busy"));
    }

    #[test]
    fn telemetry_app_toggle_pause() {
        let mut app = TelemetryApp::default();
        assert!(!app.is_paused);
        app.toggle_pause();
        assert!(app.is_paused);
        app.toggle_pause();
        assert!(!app.is_paused);
    }

    #[test]
    fn telemetry_app_disconnect_resets_pps_and_state() {
        let mut app = TelemetryApp::default();
        app.connection_state = ConnectionState::Live { since: Instant::now() };
        app.pps = 25.0;
        app.pps_counter = 10;

        app.disconnect();

        assert_eq!(app.connection_state, ConnectionState::Disconnected);
        assert_eq!(app.pps, 0.0);
        assert_eq!(app.pps_counter, 0);
    }

    #[test]
    fn anomaly_queue_bounded_capacity() {
        let mut app = TelemetryApp::default();
        let cap = app.config.anomaly_queue_capacity;

        for i in 0..(cap + 5) {
            let anom = anomaly::Anomaly {
                timestamp_ms: i as u32 * 100,
                kind: anomaly::AnomalyKind::BattOvercurrent,
                value_str: format!("{} A", i),
                cause: "overcurrent",
            };
            app.record_anomaly(&anom);
        }

        assert_eq!(app.recent_anomalies.len(), cap);
        // The last element should be the latest anomaly
        let last = app.recent_anomalies.back().unwrap();
        assert!(last.contains("BATT OVERCURRENT"));
    }
}