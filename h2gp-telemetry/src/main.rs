mod protocol;
mod serial;
mod logger;
mod datamanager;
mod demo;

use eframe::egui;
use protocol::TelemetrySample;
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(PartialEq)]
enum ChartTab {
    Voltage,
    Current,
    Power,
    Energy,
}

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
    
    // Active chart state
    chart_tab: ChartTab,
    
    // Packet Counter & UI Toggles
    packet_count: u32,
    show_diagnostics: bool,
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
            
            // Initialize chart to Voltage when the app first opens
            chart_tab: ChartTab::Voltage,
            
            // Initialize counters and toggles
            packet_count: 0,
            show_diagnostics: false,
        }
    }
}

impl eframe::App for TelemetryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(sample) = self.rx.try_recv() {
            self.packet_count += 1; // Increment packet counter

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

        // 2. Diagnostics & Raw Data Footer (Absolute Bottom)
        if self.show_diagnostics {
            egui::TopBottomPanel::bottom("diagnostics").show(ctx, |ui| {
                ui.add_space(3.0);
                ui.horizontal(|ui| {
                    // Reconstruct JSON-like string for BATT & FC
                    if let Some(sample) = &self.latest_sample {
                        let rx_str = format!(
                            r#"RX {{"BATT":{{"V":{:.2},"I":{:.2},"P":{:.2},"E":{:.1},"ah":{:.4},"t":{:.1},"sv_mv":{:.0}}}, "FC":{{"V":{:.2},"I":{:.2},"P":{:.2},"E":{:.1},"ah":{:.4},"t":{:.1},"sv_mv":{:.0}}}}}"#,
                            sample.batt.v, sample.batt.i, sample.batt.p, sample.batt.e, sample.batt.ah, sample.batt.t, sample.batt.sv_mv,
                            sample.fc.v, sample.fc.i, sample.fc.p, sample.fc.e, sample.fc.ah, sample.fc.t, sample.fc.sv_mv
                        );
                        ui.label(egui::RichText::new(rx_str).color(egui::Color32::DARK_GRAY).monospace());
                    } else {
                        ui.label(egui::RichText::new("RX: Waiting for data...").color(egui::Color32::DARK_GRAY).monospace());
                    }

                    ui.separator();

                    // Reconstruct AUX String
                    if let Some(aux) = &self.latest_aux {
                        let aux_str = format!("AUX REV3 {} | T[{:.1}/{:.1}/{:.1}/{:.1}] | MAX {:.1}C | F:{}% | FLG:{}",
                            aux.sensor_count,
                            aux.temperature_c[0], aux.temperature_c[1], aux.temperature_c[2], aux.temperature_c[3],
                            aux.max_temperature_c, aux.fan_duty_percent, aux.flags
                        );
                        ui.label(egui::RichText::new(aux_str).color(egui::Color32::DARK_GRAY).monospace());
                    } else {
                        ui.label(egui::RichText::new("AUX: No data...").color(egui::Color32::DARK_GRAY).monospace());
                    }

                    // Push PKT counter to the far right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("PKT: {}", self.packet_count)).monospace());
                    });
                });
                ui.add_space(3.0);
            });
        }

        // 2a. Statistics Row & Power Balance Bar
        egui::TopBottomPanel::bottom("stats_row").show(ctx, |ui| {
            ui.add_space(5.0);
            if let Some(sample) = &self.latest_sample {
                ui.horizontal(|ui| {
                    // Calculate 10-sample rolling averages
                    let batt_v_avg = self.data_manager.compute_batt_voltage_avg(10);
                    let batt_i_avg = self.data_manager.compute_batt_current_avg(10);
                    let fc_v_avg = self.data_manager.compute_fc_voltage_avg(10);
                    let fc_i_avg = self.data_manager.compute_fc_current_avg(10);

                    // Display Averages
                    ui.label(format!("AVG BATT: {:.2} V | {:.2} A", batt_v_avg, batt_i_avg));
                    ui.separator();
                    ui.label(format!("AVG FC: {:.2} V | {:.2} A", fc_v_avg, fc_i_avg));
                    ui.separator();
                    
                    // Display Total Energy
                    let total_e = sample.batt.e + sample.fc.e;
                    ui.label(format!("TOTAL ENERGY: {:.1} J", total_e));
                    
                    ui.separator();
                    
                    // Power Balance Bar
                    let total_p = sample.batt.p + sample.fc.p;
                    // Prevent division by zero
                    let fc_ratio = if total_p > 0.0 { (sample.fc.p / total_p).clamp(0.0, 1.0) } else { 0.0 };
                    
                    ui.label("Power Balance:");
                    let progress = egui::ProgressBar::new(fc_ratio as f32)
                        .text(format!("FC {:.1}%", fc_ratio * 100.0));
                    ui.add(progress);
                });
            } else {
                ui.label("Awaiting telemetry stream to calculate statistics...");
            }
            ui.add_space(5.0);
        });

        // 2b. Bottom Control Panel (Fan & Window Sign Commands)
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

                // Push the diagnostics toggle to the far right of the control panel
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut self.show_diagnostics, "🛠 Diagnostics");
                });
            });
        });

        // 3. Central Dashboard Layout (Telemetry Panels, AUX, & Charts)
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(sample) = &self.latest_sample {
                ui.columns(2, |columns| {
                    columns[0].vertical(|ui| {
                        // Dynamic Battery State Header
                        ui.horizontal(|ui| {
                            ui.heading("BATT / CBANK");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // Determine state based on current direction
                                let (state, color) = if sample.batt.i > 0.5 {
                                    ("VYBIJENI", egui::Color32::YELLOW)
                                } else if sample.batt.i < -0.2 {
                                    ("NABIJENI", egui::Color32::LIGHT_GREEN)
                                } else {
                                    ("IDLE", egui::Color32::GRAY)
                                };
                                ui.colored_label(color, format!("● {}", state));
                            });
                        });
                        ui.separator();
                        
                        let (v_min, v_max) = self.data_manager.batt_v_min_max();
                        let (i_min, i_max) = self.data_manager.batt_i_min_max();
                        let (p_min, p_max) = self.data_manager.batt_p_min_max();

                        ui.label(format!("Voltage: {:.3} V", sample.batt.v));
                        ui.small(format!("Min: {:.3} V | Max: {:.3} V", v_min, v_max));
                        ui.small(format!("Sec. Voltage (sv_mv): {:.2} mV", sample.batt.sv_mv));
                        ui.add_space(5.0);

                        ui.label(format!("Current: {:.3} A", sample.batt.i));
                        ui.small(format!("Min: {:.3} A | Max: {:.3} A", i_min, i_max));
                        ui.add_space(5.0);

                        ui.label(format!("Power:   {:.2} W", sample.batt.p));
                        ui.small(format!("Min: {:.2} W | Max: {:.2} W", p_min, p_max));
                        ui.add_space(5.0);

                        ui.label(format!("Energy:  {:.1} J", sample.batt.e));
                        ui.label(format!("Capacity:{:.3} Ah", sample.batt.ah));
                        
                        ui.add_space(5.0);
                        ui.label(format!("INA228 Temp: {:.1} °C", sample.batt.t));
                        
                        ui.add_space(5.0);
                        ui.label(format!("Batt Voltage AVG (5): {:.3} V", self.data_manager.compute_batt_voltage_avg(5)));
                    });

                    columns[1].vertical(|ui| {
                        // Dynamic Fuel Cell State Header
                        ui.horizontal(|ui| {
                            ui.heading("FUEL CELL");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let mut state = "IDLE";
                                let mut color = egui::Color32::GRAY;
                                
                                // Check AUX flags for actuator states
                                if let Some(aux) = &self.latest_aux {
                                    if (aux.flags & 0x01) != 0 {
                                        state = "SHORT (ZKRACOVANI)";
                                        color = egui::Color32::RED;
                                    } else if (aux.flags & 0x02) != 0 {
                                        state = "PURGE (ODVOD)";
                                        color = egui::Color32::LIGHT_BLUE;
                                    }
                                }
                                ui.colored_label(color, format!("● {}", state));
                            });
                        });
                        ui.separator();
                        
                        let (v_min, v_max) = self.data_manager.fc_v_min_max();
                        let (i_min, i_max) = self.data_manager.fc_i_min_max();
                        let (p_min, p_max) = self.data_manager.fc_p_min_max();

                        ui.label(format!("Voltage: {:.3} V", sample.fc.v));
                        ui.small(format!("Min: {:.3} V | Max: {:.3} V", v_min, v_max));
                        ui.small(format!("Sec. Voltage (sv_mv): {:.2} mV", sample.fc.sv_mv));
                        ui.add_space(5.0);

                        ui.label(format!("Current: {:.3} A", sample.fc.i));
                        ui.small(format!("Min: {:.3} A | Max: {:.3} A", i_min, i_max));
                        ui.add_space(5.0);

                        ui.label(format!("Power:   {:.2} W", sample.fc.p));
                        ui.small(format!("Min: {:.2} W | Max: {:.2} W", p_min, p_max));
                        ui.add_space(5.0);

                        ui.label(format!("Energy:  {:.1} J", sample.fc.e));
                        ui.label(format!("Capacity:{:.3} Ah", sample.fc.ah));
                        
                        ui.add_space(5.0);
                        ui.label(format!("INA228 Temp: {:.1} °C", sample.fc.t));
                        
                        ui.add_space(5.0);
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
                
                // Chart Navigation Tabs
                ui.horizontal(|ui| {
                    ui.heading("Telemetry History:");
                    ui.add_space(10.0);
                    ui.selectable_value(&mut self.chart_tab, ChartTab::Voltage, "Voltage (V)");
                    ui.selectable_value(&mut self.chart_tab, ChartTab::Current, "Current (A)");
                    ui.selectable_value(&mut self.chart_tab, ChartTab::Power, "Power (W)");
                    ui.selectable_value(&mut self.chart_tab, ChartTab::Energy, "Energy (J)");
                });

                // Extract and map data based on the selected tab
                let history = self.data_manager.history();
                let times = self.data_manager.time_labels();

                let mut batt_points = Vec::with_capacity(history.len());
                let mut fc_points = Vec::with_capacity(history.len());

                for (&t, s) in times.iter().zip(history.iter()) {
                    let (b_val, f_val) = match self.chart_tab {
                        ChartTab::Voltage => (s.batt.v, s.fc.v),
                        ChartTab::Current => (s.batt.i, s.fc.i),
                        ChartTab::Power   => (s.batt.p, s.fc.p),
                        ChartTab::Energy  => (s.batt.e, s.fc.e),
                    };
                    batt_points.push([t, b_val]);
                    fc_points.push([t, f_val]);
                }

                let y_axis_label = match self.chart_tab {
                    ChartTab::Voltage => "Voltage (V)",
                    ChartTab::Current => "Current (A)",
                    ChartTab::Power   => "Power (W)",
                    ChartTab::Energy  => "Energy (J)",
                };

                // Render the plot with a legend and dynamic lines
                egui_plot::Plot::new("history_plot")
                    .height(250.0)
                    .legend(egui_plot::Legend::default())
                    .show(ui, |plot_ui| {
                        plot_ui.line(
                            egui_plot::Line::new(egui_plot::PlotPoints::new(batt_points))
                                .name(format!("Batt {}", y_axis_label))
                                .color(egui::Color32::LIGHT_BLUE)
                        );
                        plot_ui.line(
                            egui_plot::Line::new(egui_plot::PlotPoints::new(fc_points))
                                .name(format!("FC {}", y_axis_label))
                                .color(egui::Color32::LIGHT_GREEN)
                        );
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