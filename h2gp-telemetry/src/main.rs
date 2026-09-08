mod protocol;
mod serial;
mod logger;
mod datamanager;
mod demo;
mod ui;

use eframe::egui;
use protocol::TelemetrySample;
use std::sync::mpsc::{self, Receiver, Sender};
use std::collections::VecDeque;
use ui::ChartTab;

pub struct TelemetryApp {
    pub rx: Receiver<TelemetrySample>,
    pub telemetry_tx: Sender<TelemetrySample>,
    pub cmd_tx: Option<Sender<String>>,
    pub latest_sample: Option<TelemetrySample>,
    pub latest_aux: Option<protocol::Rev3AuxData>,
    pub data_manager: datamanager::DataManager,
    pub port_name: String,
    pub is_connected: bool,

    pub fan_mode: String,
    pub fan_duty: i32,
    pub driver_code: String,
    
    pub chart_tab: ChartTab,
    pub chart_window: f64,
    
    pub packet_count: u32,
    pub show_diagnostics: bool,
    
    // Bounded FIFO queue for the UI
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
            fan_mode: "auto".to_string(),
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