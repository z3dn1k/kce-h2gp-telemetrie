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
    chart_window: f64, // Tracks the rolling time window in seconds
    
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
            chart_tab: ChartTab::Voltage,
            chart_window: 30.0, // Default to 30-second rolling window
            packet_count: 0,
            show_diagnostics: false,
        }
    }
}

// Helper to render large, clean metric blocks
fn render_metric(ui: &mut egui::Ui, title: &str, val_str: String, unit: &str, sub_text: String, sub_text_2: String, title_color: egui::Color32) {
    ui.vertical(|ui| {
        ui.add_space(2.0);
        // Title uses the dynamically passed color (Blue for BATT, Red for FC)
        ui.label(egui::RichText::new(title).color(title_color).size(10.0).strong());
        
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(val_str).color(egui::Color32::WHITE).size(42.0));
            ui.label(egui::RichText::new(unit).color(egui::Color32::from_gray(100)).size(16.0));
        });
        
        // Force consistent heights even if subtext is empty
        if !sub_text.is_empty() {
            ui.label(egui::RichText::new(sub_text).color(egui::Color32::from_gray(70)).size(10.0));
        } else {
            ui.label(egui::RichText::new(" ").size(10.0)); // Invisible space to lock height
        }
        
        if !sub_text_2.is_empty() {
            ui.label(egui::RichText::new(sub_text_2).color(egui::Color32::from_gray(70)).size(10.0));
        } else {
            ui.label(egui::RichText::new(" ").size(10.0)); // Invisible space to lock height
        }
        ui.add_space(6.0);
    });
}

// Helper for the small footer stats
fn render_mini_stat(ui: &mut egui::Ui, title: &str, val_str: String) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(title).color(egui::Color32::from_gray(90)).size(9.0));
        ui.label(egui::RichText::new(val_str).color(egui::Color32::LIGHT_GRAY).size(14.0).strong());
    });
}

impl eframe::App for TelemetryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        
        // Drain the channel as fast as possible on every frame cycle
        while let Ok(sample) = self.rx.try_recv() {
            self.packet_count += 1;
            if sample.has_channel_data {
                self.data_manager.add_data(sample.clone());
                self.latest_sample = Some(sample);
                self.is_connected = true;
            } else if sample.has_rev3_aux {
                self.latest_aux = Some(sample.rev3_aux);
            }
        }

        // 1. Top Toolbar (Minimalist)
        egui::TopBottomPanel::top("toolbar").frame(egui::Frame::none().fill(egui::Color32::from_rgb(15, 15, 15)).inner_margin(8.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("H2Gp KCE").color(egui::Color32::WHITE).size(14.0).strong());
                ui.add_space(20.0);
                
                ui.label(egui::RichText::new("PORT:").color(egui::Color32::from_gray(100)).size(10.0));
                ui.add(egui::TextEdit::singleline(&mut self.port_name).desired_width(60.0));

                if ui.button("CONNECT").clicked() {
                    let (cmd_tx, cmd_rx) = mpsc::channel();
                    self.cmd_tx = Some(cmd_tx);
                    serial::start_serial_thread(self.port_name.clone(), 115200, self.telemetry_tx.clone(), cmd_rx);
                }

                if ui.button("DEMO MODE").clicked() {
                    demo::start_demo_thread("data.csv", self.telemetry_tx.clone());
                }

                ui.add_space(20.0);
                let (status_text, status_color) = if self.is_connected { 
                    ("● LIVE", egui::Color32::from_rgb(0, 200, 100)) 
                } else { 
                    ("● DISCONNECTED", egui::Color32::from_rgb(200, 50, 50)) 
                };
                ui.label(egui::RichText::new(status_text).color(status_color).size(12.0));
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("UNIT: MAIN MCU").color(egui::Color32::from_gray(80)).size(10.0));
                });
            });
        });

       // 2. Diagnostics & Raw Data Footer (Absolute Bottom)
        if self.show_diagnostics {
            egui::TopBottomPanel::bottom("diagnostics").frame(egui::Frame::none().fill(egui::Color32::from_rgb(5, 5, 5)).inner_margin(6.0)).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(sample) = &self.latest_sample {
                        let rx_str = format!(
                            r#"RX {{"BATT":{{"V":{:>5.2},"I":{:>6.2},"P":{:>6.2},"E":{:>6.1},"ah":{:>6.4},"t":{:>4.1},"sv_mv":{:>5.0}}}, "FC":{{"V":{:>5.2},"I":{:>6.2},"P":{:>6.2},"E":{:>6.1},"ah":{:>6.4},"t":{:>4.1},"sv_mv":{:>5.0}}}}}"#,
                            sample.batt.v, sample.batt.i, sample.batt.p, sample.batt.e, sample.batt.ah, sample.batt.t, sample.batt.sv_mv,
                            sample.fc.v, sample.fc.i, sample.fc.p, sample.fc.e, sample.fc.ah, sample.fc.t, sample.fc.sv_mv
                        );
                        ui.label(egui::RichText::new(rx_str).color(egui::Color32::WHITE).size(12.0).monospace());
                    }

                    if let Some(aux) = &self.latest_aux {
                        let aux_str = format!("AUX REV3 {} | T[{:>5.1}/{:>5.1}/{:>5.1}/{:>5.1}] | MAX {:>5.1}C | F:{:>3}% | FLG:{:>3}",
                            aux.sensor_count, aux.temperature_c[0], aux.temperature_c[1], aux.temperature_c[2], aux.temperature_c[3],
                            aux.max_temperature_c, aux.fan_duty_percent, aux.flags
                        );
                        ui.label(egui::RichText::new(aux_str).color(egui::Color32::WHITE).size(12.0).monospace());
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("PKT: {:>6}", self.packet_count)).color(egui::Color32::WHITE).size(12.0).monospace());
                    });
                });
            });
        }

        // 3. Control Panel Footer
        egui::TopBottomPanel::bottom("controls").frame(egui::Frame::none().fill(egui::Color32::from_rgb(12, 12, 12)).inner_margin(8.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("FAN").color(egui::Color32::from_gray(100)).size(10.0));
                egui::ComboBox::from_id_source("fan_cb").selected_text(&self.fan_mode).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.fan_mode, "auto".to_string(), "AUTO");
                    ui.selectable_value(&mut self.fan_mode, "manual".to_string(), "MANUAL");
                    ui.selectable_value(&mut self.fan_mode, "off".to_string(), "OFF");
                });

                ui.add(egui::Slider::new(&mut self.fan_duty, 0..=100).suffix("%"));
                if ui.button("SEND").clicked() {
                    if let Some(tx) = &self.cmd_tx {
                        let cmd = if self.fan_mode == "manual" { format!(r#"{{"cmd":"fan","mode":"manual","duty":{}}}"#, self.fan_duty) } 
                                  else { format!(r#"{{"cmd":"fan","mode":"{}"}}"#, self.fan_mode) };
                        let _ = tx.send(cmd);
                    }
                }

                ui.add_space(20.0);
                ui.label(egui::RichText::new("SIGN").color(egui::Color32::from_gray(100)).size(10.0));
                ui.add(egui::TextEdit::singleline(&mut self.driver_code).char_limit(3).desired_width(40.0));
                if ui.button("SEND").clicked() {
                    if let Some(tx) = &self.cmd_tx {
                        let _ = tx.send(format!(r#"{{"cmd":"sign","driver":"{}"}}"#, self.driver_code.to_uppercase()));
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut self.show_diagnostics, "🛠 DIAGNOSTICS");
                });
            });
        });

        // 4. Mini Stats Row (Bottom)
        egui::TopBottomPanel::bottom("stats_row").frame(egui::Frame::none().fill(egui::Color32::from_rgb(18, 18, 18)).inner_margin(12.0)).show(ctx, |ui| {
            if let Some(sample) = &self.latest_sample {
                ui.horizontal(|ui| {
                    let batt_v_avg = self.data_manager.compute_batt_voltage_avg(10);
                    let batt_i_avg = self.data_manager.compute_batt_current_avg(10);
                    let fc_v_avg = self.data_manager.compute_fc_voltage_avg(10);
                    let fc_i_avg = self.data_manager.compute_fc_current_avg(10);

                    render_mini_stat(ui, "FC AVG V", format!("{:.2} V", fc_v_avg));
                    ui.add_space(30.0);
                    render_mini_stat(ui, "FC AVG I", format!("{:.2} A", fc_i_avg));
                    ui.add_space(30.0);
                    render_mini_stat(ui, "BATT AVG V", format!("{:.2} V", batt_v_avg));
                    ui.add_space(30.0);
                    render_mini_stat(ui, "BATT AVG I", format!("{:.2} A", batt_i_avg));
                    ui.add_space(30.0);
                    render_mini_stat(ui, "FC TOTAL E", format!("{:.1} J", sample.fc.e));
                    ui.add_space(30.0);
                    render_mini_stat(ui, "BATT TOTAL E", format!("{:.1} J", sample.batt.e));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let total_p = sample.batt.p + sample.fc.p;
                        let fc_ratio = if total_p > 0.0 { (sample.fc.p / total_p).clamp(0.0, 1.0) } else { 0.0 };
                        
                        let progress = egui::ProgressBar::new(fc_ratio as f32)
                            .text(format!("{:.1}W", sample.batt.p))
                            .fill(egui::Color32::from_rgb(50, 100, 150))
                            .desired_width(200.0);
                        ui.add(progress);
                        ui.label(egui::RichText::new("POWER MIX (SIGNED)").color(egui::Color32::from_gray(80)).size(10.0));
                    });
                });
            } else {
                ui.label(egui::RichText::new("Awaiting data...").color(egui::Color32::from_gray(100)));
            }
        });

        // 5. Central Data Dashboard
        egui::CentralPanel::default().frame(egui::Frame::none().fill(egui::Color32::from_rgb(10, 10, 10)).inner_margin(12.0)).show(ctx, |ui| {
            if let Some(sample) = &self.latest_sample {
                let bg_frame = egui::Frame::none().fill(egui::Color32::from_rgb(15, 15, 15)).rounding(4.0).inner_margin(12.0);
                
                bg_frame.show(ui, |ui| {
                    let total_width = ui.available_width();
                    let col_width = (total_width - 30.0) / 2.0;

                    ui.horizontal_top(|ui| {
                        let batt_color = egui::Color32::from_rgb(60, 180, 220);
                        let fc_color = egui::Color32::from_rgb(220, 60, 60);

                        // LEFT COLUMN: BATT / CBANK
                        let batt_resp = ui.allocate_ui_with_layout(egui::vec2(col_width, 0.0), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("●").color(batt_color));
                                ui.label(egui::RichText::new("BATT / CBANK").color(egui::Color32::WHITE).size(16.0).strong());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let (state, color) = if sample.batt.i > 0.5 { ("VYBIJENI", egui::Color32::from_rgb(220, 180, 50)) } 
                                    else if sample.batt.i < -0.2 { ("NABIJENI", egui::Color32::from_rgb(80, 200, 120)) } 
                                    else { ("IDLE", egui::Color32::from_gray(100)) };
                                    ui.label(egui::RichText::new(format!("● {}", state)).color(color).size(11.0).strong());
                                });
                            });
                            ui.add_space(10.0);

                            let (v_min, v_max) = self.data_manager.batt_v_min_max();
                            let (i_min, i_max) = self.data_manager.batt_i_min_max();
                            let (p_min, p_max) = self.data_manager.batt_p_min_max();

                            egui::Grid::new("batt_grid_1").min_col_width(col_width / 3.0 - 10.0).show(ui, |ui| {
                                render_metric(ui, "NAPETI", format!("{:.3}", sample.batt.v), "V", format!("MIN {:.3}  MAX {:.3}", v_min, v_max), format!("SV_MV {:.0}", sample.batt.sv_mv), batt_color);
                                render_metric(ui, "PROUD", format!("{:.3}", sample.batt.i), "A", format!("MIN {:.3}  MAX {:.3}", i_min, i_max), "".to_string(), batt_color);
                                render_metric(ui, "VYKON", format!("{:.3}", sample.batt.p), "W", format!("MIN {:.3}  MAX {:.3}", p_min, p_max), "".to_string(), batt_color);
                            });
                            ui.add_space(15.0);
                            egui::Grid::new("batt_grid_2").min_col_width(col_width / 3.0 - 10.0).show(ui, |ui| {
                                render_metric(ui, "TEPLOTA", format!("{:.1}", sample.batt.t), "C", "INA228 INTERNAL".to_string(), "".to_string(), batt_color);
                                render_metric(ui, "ENERGIE", format!("{:.2}", sample.batt.e), "J", "".to_string(), "".to_string(), batt_color);
                                render_metric(ui, "KAPACITA", format!("{:.4}", sample.batt.ah), "Ah", "".to_string(), "".to_string(), batt_color);
                            });
                        }).response;

                        // CUSTOM DUAL-COLORED SEPARATOR
                        let (line_rect, _) = ui.allocate_exact_size(egui::vec2(8.0, batt_resp.rect.height()), egui::Sense::hover());
                        
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(egui::pos2(line_rect.min.x + 1.0, line_rect.min.y), egui::vec2(2.0, line_rect.height())),
                            0.0,
                            batt_color,
                        );
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(egui::pos2(line_rect.min.x + 5.0, line_rect.min.y), egui::vec2(2.0, line_rect.height())),
                            0.0,
                            fc_color,
                        );

                        // RIGHT COLUMN: FUEL CELL
                        ui.allocate_ui_with_layout(egui::vec2(col_width, 0.0), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("●").color(fc_color));
                                ui.label(egui::RichText::new("FUEL CELL").color(egui::Color32::WHITE).size(16.0).strong());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let mut state = "IDLE";
                                    let mut color = egui::Color32::from_gray(100);
                                    if let Some(aux) = &self.latest_aux {
                                        if (aux.flags & 0x01) != 0 { state = "SHORT (ZKRACOVANI)"; color = fc_color; } 
                                        else if (aux.flags & 0x02) != 0 { state = "PURGE (ODVOD)"; color = egui::Color32::from_rgb(80, 180, 220); }
                                    }
                                    ui.label(egui::RichText::new(format!("● {}", state)).color(color).size(11.0).strong());
                                });
                            });
                            ui.add_space(10.0);

                            let (v_min, v_max) = self.data_manager.fc_v_min_max();
                            let (i_min, i_max) = self.data_manager.fc_i_min_max();
                            let (p_min, p_max) = self.data_manager.fc_p_min_max();

                            egui::Grid::new("fc_grid_1").min_col_width(col_width / 3.0 - 10.0).show(ui, |ui| {
                                render_metric(ui, "NAPETI", format!("{:.3}", sample.fc.v), "V", format!("MIN {:.3}  MAX {:.3}", v_min, v_max), format!("SV_MV {:.0}", sample.fc.sv_mv), fc_color);
                                render_metric(ui, "PROUD", format!("{:.3}", sample.fc.i), "A", format!("MIN {:.3}  MAX {:.3}", i_min, i_max), "".to_string(), fc_color);
                                render_metric(ui, "VYKON", format!("{:.3}", sample.fc.p), "W", format!("MIN {:.3}  MAX {:.3}", p_min, p_max), "".to_string(), fc_color);
                            });
                            ui.add_space(15.0);
                            egui::Grid::new("fc_grid_2").min_col_width(col_width / 3.0 - 10.0).show(ui, |ui| {
                                render_metric(ui, "TEPLOTA", format!("{:.1}", sample.fc.t), "C", "INA228 INTERNAL".to_string(), "".to_string(), fc_color);
                                render_metric(ui, "ENERGIE", format!("{:.2}", sample.fc.e), "J", "".to_string(), "".to_string(), fc_color);
                                render_metric(ui, "KAPACITA", format!("{:.4}", sample.fc.ah), "Ah", "".to_string(), "".to_string(), fc_color);
                            });
                        });
                    });
                });

                ui.add_space(15.0);

                // AUX Temperatures Row
                if let Some(aux) = &self.latest_aux {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("TEMP CIDLO 1").color(egui::Color32::from_rgb(100, 180, 160)).size(10.0).strong());
                        ui.label(egui::RichText::new(format!("{:.1} C", aux.temperature_c[0])).color(egui::Color32::LIGHT_GRAY).size(12.0));
                        ui.add_space(30.0);
                        ui.label(egui::RichText::new("TEMP CIDLO 2").color(egui::Color32::from_rgb(100, 180, 160)).size(10.0).strong());
                        ui.label(egui::RichText::new(format!("{:.1} C", aux.temperature_c[1])).color(egui::Color32::LIGHT_GRAY).size(12.0));
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(format!("{:.1} C", aux.max_temperature_c)).color(egui::Color32::LIGHT_GRAY).size(12.0));
                            ui.label(egui::RichText::new("MAX TEMP").color(egui::Color32::from_gray(90)).size(10.0));
                        });
                    });
                    ui.add_space(5.0);
                    ui.separator();
                }

                ui.add_space(10.0);

                // Chart Navigation & Rolling Window Selector
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("GRAPHS").color(egui::Color32::from_gray(120)).size(14.0).strong());
                    ui.add_space(20.0);
                    ui.selectable_value(&mut self.chart_tab, ChartTab::Voltage, "NAPETI (V)");
                    ui.selectable_value(&mut self.chart_tab, ChartTab::Current, "PROUD (A)");
                    ui.selectable_value(&mut self.chart_tab, ChartTab::Power, "VYKON (W)");
                    ui.selectable_value(&mut self.chart_tab, ChartTab::Energy, "ENERGIE (J)");

                    ui.add_space(40.0);
                    ui.label(egui::RichText::new("WINDOW:").color(egui::Color32::from_gray(120)).size(10.0).strong());
                    ui.selectable_value(&mut self.chart_window, 5.0, "5s");   
                    ui.selectable_value(&mut self.chart_window, 10.0, "10s"); 
                    ui.selectable_value(&mut self.chart_window, 15.0, "15s");
                    ui.selectable_value(&mut self.chart_window, 30.0, "30s");
                    ui.selectable_value(&mut self.chart_window, 60.0, "60s");
                    ui.selectable_value(&mut self.chart_window, 120.0, "2m");
                });
                ui.add_space(5.0);

                // Extract data
                let history = self.data_manager.history();
                let times = self.data_manager.time_labels();

                let mut batt_points = Vec::new();
                let mut fc_points = Vec::new();

                let latest_time = times.last().copied().unwrap_or(0.0);
                let time_threshold = latest_time - self.chart_window;

                let start_idx = times.partition_point(|&t| t < time_threshold);

                for i in start_idx..times.len() {
                    let t = times[i];
                    let s = &history[i];
                    
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
                    ChartTab::Voltage => "NAPETI (V)",
                    ChartTab::Current => "PROUD (A)",
                    ChartTab::Power   => "VYKON (W)",
                    ChartTab::Energy  => "ENERGIE (J)",
                };

                egui_plot::Plot::new("history_plot")
                    .height(ui.available_height())
                    .show_background(false)
                    .show_axes([false, true]) 
                    .legend(egui_plot::Legend::default())
                    .show(ui, |plot_ui| {
                        plot_ui.line(
                            egui_plot::Line::new(egui_plot::PlotPoints::new(batt_points))
                                .name(format!("BATT {}", y_axis_label))
                                .width(2.0)
                                .color(egui::Color32::from_rgb(60, 180, 220)) 
                        );
                        plot_ui.line(
                            egui_plot::Line::new(egui_plot::PlotPoints::new(fc_points))
                                .name(format!("FC {}", y_axis_label))
                                .width(2.0)
                                .color(egui::Color32::from_rgb(220, 60, 60)) 
                        );
                    });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.heading("Waiting for telemetry stream... Click CONNECT for COM port or DEMO MODE for testing.");
                });
            }
        });

        // FIX 4: Request a steady 30 FPS repaint (33ms). 
        // This keeps the UI highly responsive without maxing out a CPU core.
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