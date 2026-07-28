mod protocol;
mod serial;
mod logger;
mod datamanager;
mod demo;

use eframe::egui;
use protocol::{ChannelData, TelemetrySample};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

struct TelemetryApp {
    rx: Receiver<TelemetrySample>,
    telemetry_tx: Sender<TelemetrySample>,
    cmd_tx: Option<Sender<String>>,
    latest_sample: Option<TelemetrySample>,
    latest_aux: Option<protocol::Rev3AuxData>,
    data_manager: datamanager::DataManager,
    port_name: String,
    is_connected: bool,

    // Control States
    fan_mode: String,
    fan_duty: i32,
    driver_code: String,
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
        }
    }
}

impl eframe::App for TelemetryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(sample) = self.rx.try_recv() {
            if sample.has_channel_data {
                logger::append_to_csv(&sample, "data.csv");
                self.data_manager.add_data(sample.clone());
                self.latest_sample = Some(sample);
                self.is_connected = true;
            } else if sample.has_rev3_aux {
                self.latest_aux = Some(sample.rev3_aux);
            }
        }

        ctx.request_repaint();

        // 1. Top Toolbar (Connection & Demo)
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("H2Gp Telemetry (Rust)");
                ui.add_space(15.0);
                
                let status_text = if self.is_connected { "LIVE / DEMO" } else { "DISCONNECTED" };
                ui.colored_label(
                    if self.is_connected { egui::Color32::GREEN } else { egui::Color32::RED },
                    status_text
                );
                
                ui.add_space(15.0);
                ui.label("Port:");
                ui.text_edit_singleline(&mut self.port_name);

                if ui.button("CONNECT").clicked() {
                    let (cmd_tx, cmd_rx) = mpsc::channel();
                    self.cmd_tx = Some(cmd_tx);
                    
                    serial::start_serial_thread(
                        self.port_name.clone(),
                        115200,
                        self.telemetry_tx.clone(),
                        cmd_rx,
                    );
                }

                if ui.button("DEMO MODE").clicked() {
                    demo::start_demo_thread("data.csv", self.telemetry_tx.clone());
                }
            });
        });

        // 2. Bottom Control Panel (Fan & Window Sign Commands)
        egui::TopBottomPanel::bottom("controls").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("FAN MODE:");
                egui::ComboBox::from_label("")
                    .selected_text(&self.fan_mode)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.fan_mode, "auto".to_string(), "AUTO");
                        ui.selectable_value(&mut self.fan_mode, "manual".to_string(), "MANUAL");
                        ui.selectable_value(&mut self.fan_mode, "off".to_string(), "OFF");
                    });

                ui.add_space(10.0);
                ui.label("Duty:");
                ui.add(egui::Slider::new(&mut self.fan_duty, 0..=100).suffix("%"));

                if ui.button("SEND FAN").clicked() {
                    if let Some(tx) = &self.cmd_tx {
                        let json_cmd = if self.fan_mode == "manual" {
                            format!(r#"{{"cmd":"fan","mode":"manual","duty":{}}}"#, self.fan_duty)
                        } else {
                            format!(r#"{{"cmd":"fan","mode":"{}"}}"#, self.fan_mode)
                        };
                        let _ = tx.send(json_cmd);
                    }
                }

                ui.add_space(30.0);
                ui.label("SIGN DRIVER:");
                ui.add(egui::TextEdit::singleline(&mut self.driver_code).char_limit(3));

                if ui.button("SEND SIGN").clicked() {
                    if let Some(tx) = &self.cmd_tx {
                        let json_cmd = format!(r#"{{"cmd":"sign","driver":"{}"}}"#, self.driver_code.to_uppercase());
                        let _ = tx.send(json_cmd);
                    }
                }
            });
        });

        // 3. Central Dashboard Layout (Telemetry Panels, AUX, & Charts)
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(sample) = &self.latest_sample {
                ui.columns(2, |columns| {
                    columns[0].vertical(|ui| {
                        ui.heading("BATT / CBANK");
                        ui.separator();
                        ui.label(format!("Voltage: {:.3} V", sample.batt.v));
                        ui.label(format!("Current: {:.3} A", sample.batt.i));
                        ui.label(format!("Power:   {:.2} W", sample.batt.p));
                        ui.label(format!("Energy:  {:.1} J", sample.batt.e));
                        ui.label(format!("Capacity:{:.3} Ah", sample.batt.ah));
                        ui.label(format!("Batt Voltage AVG (5): {:.3} V", self.data_manager.compute_batt_voltage_avg(5)));
                    });

                    columns[1].vertical(|ui| {
                        ui.heading("FUEL CELL");
                        ui.separator();
                        ui.label(format!("Voltage: {:.3} V", sample.fc.v));
                        ui.label(format!("Current: {:.3} A", sample.fc.i));
                        ui.label(format!("Power:   {:.2} W", sample.fc.p));
                        ui.label(format!("Energy:  {:.1} J", sample.fc.e));
                        ui.label(format!("Capacity:{:.3} Ah", sample.fc.ah));
                        ui.label(format!("FC Voltage AVG (5): {:.3} V", self.data_manager.compute_fc_voltage_avg(5)));
                    });
                });

                if let Some(aux) = &self.latest_aux {
                    ui.add_space(10.0);
                    ui.separator();
                    ui.heading(format!("AUX Temperatures (Sensors Active: {})", aux.sensor_count));
                    ui.horizontal(|ui| {
                        for (idx, &temp) in aux.temperature_c.iter().enumerate() {
                            ui.label(format!("T{}: {:.1}°C", idx, temp));
                            ui.add_space(15.0);
                        }
                        ui.label(format!("Max T: {:.1}°C", aux.max_temperature_c));
                        ui.add_space(15.0);
                        ui.label(format!("Fan Duty: {}%", aux.fan_duty_percent));
                    });
                }

                ui.add_space(20.0);
                ui.separator();
                ui.heading("Battery Voltage History");

                egui_plot::Plot::new("voltage_history_plot")
                    .height(200.0)
                    .show(ui, |plot_ui| {
                        let history = self.data_manager.history();
                        let times = self.data_manager.time_labels();
                        let points: egui_plot::PlotPoints = times
                            .iter()
                            .zip(history.iter())
                            .map(|(&t, s)| [t, s.batt.v])
                            .collect();
                        plot_ui.line(egui_plot::Line::new(points).name("Batt Voltage (V)"));
                    });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.heading("Waiting for telemetry stream... Click CONNECT for COM port or DEMO MODE for testing.");
                });
            }
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 680.0]),
        ..Default::default()
    };

    eframe::run_native(
        "H2Gp Telemetry Dashboard",
        options,
        Box::new(|_cc| Ok(Box::<TelemetryApp>::default())),
    )
}