//! # Immediate-Mode User Interface & Visualization (`egui`)
//!
//! Renders the live telemetry dashboard at ~30 FPS:
//! - **Top Toolbar**: Port selection, connection management, demo mode trigger, summary export.
//! - **Central Panel**: Dual-column live metrics comparing Battery/Capacitor Bank vs. Hydrogen Fuel Cell.
//! - **Real-time Chart**: Rolling history plots for Voltage, Current, Power, and Energy with selectable time windows.
//! - **Side Panel**: Bounded live anomaly log for rapid pit-crew situational awareness.
//! - **Bottom Panels**: Running averages, power-mix indicator, cooling fan controls, and raw packet diagnostics.

use eframe::egui;
use crate::TelemetryApp;
use crate::serial;

/// Selectable measurement channels displayed on the real-time history plot.
#[derive(PartialEq, Clone, Copy)]
pub enum ChartTab {
    /// Bus voltage curves for both channels in Volts (V).
    Voltage,
    /// Current draw curves for both channels in Amperes (A).
    Current,
    /// Instantaneous power curves in Watts (W).
    Power,
    /// Cumulative electrical energy curves in Joules (J).
    Energy,
}

/// Commanded operational mode for the vehicle cooling fan subsystem.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FanMode {
    /// Onboard microcontroller automatically regulates fan duty based on temperature probes.
    Auto,
    /// Driver/pit-crew manually commands fixed PWM duty percentage (0–100%).
    Manual,
    /// Cooling fan is explicitly disabled.
    Off,
}

impl FanMode {
    /// Returns the lowercase protocol string sent to the MCU in JSON commands.
    pub fn as_str(self) -> &'static str {
        match self {
            FanMode::Auto => "auto",
            FanMode::Manual => "manual",
            FanMode::Off => "off",
        }
    }
}

impl std::fmt::Display for FanMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FanMode::Auto => write!(f, "AUTO"),
            FanMode::Manual => write!(f, "MANUAL"),
            FanMode::Off => write!(f, "OFF"),
        }
    }
}

// Helper to render large, clean metric blocks
fn render_metric(ui: &mut egui::Ui, title: &str, val_str: String, unit: &str, sub_text: String, sub_text_2: String, title_color: egui::Color32) {
    ui.vertical(|ui| {
        ui.add_space(2.0);
        ui.label(egui::RichText::new(title).color(title_color).size(10.0).strong());
        
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(val_str).color(egui::Color32::WHITE).size(42.0));
            ui.label(egui::RichText::new(unit).color(egui::Color32::from_gray(100)).size(16.0));
        });
        
        if !sub_text.is_empty() {
            ui.label(egui::RichText::new(sub_text).color(egui::Color32::from_gray(70)).size(10.0));
        } else {
            ui.label(egui::RichText::new(" ").size(10.0)); 
        }
        
        if !sub_text_2.is_empty() {
            ui.label(egui::RichText::new(sub_text_2).color(egui::Color32::from_gray(70)).size(10.0));
        } else {
            ui.label(egui::RichText::new(" ").size(10.0)); 
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

/// Primary dashboard layout and rendering function called every frame by `TelemetryApp::update`.
pub fn render_dashboard(app: &mut TelemetryApp, ctx: &egui::Context) {
    // 1. Top Toolbar (Minimalist)
    egui::TopBottomPanel::top("toolbar").frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(15, 15, 15)).inner_margin(8.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("H2Gp KCE").color(egui::Color32::WHITE).size(14.0).strong());
            ui.add_space(15.0);
            
            ui.label(egui::RichText::new("PORT:").color(egui::Color32::from_gray(100)).size(10.0));
            if !app.available_ports.is_empty() {
                egui::ComboBox::from_id_salt("port_combo")
                    .selected_text(&app.port_name)
                    .show_ui(ui, |ui| {
                        for port in &app.available_ports {
                            ui.selectable_value(&mut app.port_name, port.clone(), port);
                        }
                    });
            } else {
                ui.add(egui::TextEdit::singleline(&mut app.port_name).desired_width(70.0));
            }

            if ui.small_button("↻").on_hover_text("Znovu vyhledat dostupné sériové porty (Rescan)").clicked() {
                app.available_ports = serial::detect_available_ports();
                if !app.available_ports.is_empty() && !app.available_ports.contains(&app.port_name) {
                    app.port_name = app.available_ports[0].clone();
                }
            }

            ui.add_space(5.0);

            if !app.connection_state.is_connected() {
                if ui.button("CONNECT (C)").on_hover_text("Připojit k sériovému portu [Klávesa C]").clicked() {
                    app.start_live();
                }

                if ui.button("DEMO (D)").on_hover_text("Spustit demo simulaci z data.csv [Klávesa D]").clicked() {
                    app.start_demo();
                }
            } else if ui.button("DISCONNECT (C)").on_hover_text("Odpojit aktivní spojení [Klávesa C]").clicked() {
                app.disconnect();
            }

            if app.is_paused {
                if ui.button(egui::RichText::new("▶ RESUME (Space)").color(egui::Color32::from_rgb(255, 215, 0)))
                    .on_hover_text("Obnovit aktualizaci grafu [Mezerník]")
                    .clicked() 
                {
                    app.toggle_pause();
                }
            } else if ui.button("⏸ PAUSE (Space)").on_hover_text("Pozastavit aktualizaci grafu [Mezerník]").clicked() {
                app.toggle_pause();
            }

            if ui.button("📷 SCREENSHOT").on_hover_text("Uložit snímek obrazovky do PNG").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }

            if ui.button("📄 REPORT (P)").on_hover_text("Zobrazit pozávodní analytický report a možnosti exportu [P]").clicked() {
                app.open_report_modal();
            }

            ui.add_space(15.0);
            let (status_text, status_color) = app.connection_state.status_text();
            ui.label(egui::RichText::new(status_text).color(status_color).size(12.0).strong());

            if app.connection_state.is_connected() {
                ui.add_space(10.0);
                let (pps_col, pps_txt) = if app.pps >= 10.0 {
                    (egui::Color32::from_rgb(0, 220, 100), format!("{:.0} PPS", app.pps))
                } else if app.pps > 0.0 {
                    (egui::Color32::from_rgb(255, 180, 50), format!("{:.0} PPS", app.pps))
                } else {
                    (egui::Color32::from_rgb(220, 60, 60), "0 PPS".to_string())
                };
                ui.label(egui::RichText::new(pps_txt).color(pps_col).size(11.0).monospace().strong());
            }

            if let Some((msg, _)) = &app.screenshot_toast {
                ui.add_space(10.0);
                ui.label(egui::RichText::new(msg).color(egui::Color32::from_rgb(0, 255, 180)).strong().size(11.0));
            }

            if let Some((msg, _)) = &app.report_toast {
                ui.add_space(10.0);
                ui.label(egui::RichText::new(msg).color(egui::Color32::from_rgb(0, 210, 255)).strong().size(11.0));
            }
            
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new("[1-4 Grafy | Space Pauza | R Recenter | P Report | C Spojení | D Demo]").color(egui::Color32::from_gray(80)).size(10.0));
            });
        });
    });

   // 2. Diagnostics & Raw Data Footer (Absolute Bottom)
    if app.show_diagnostics {
        egui::TopBottomPanel::bottom("diagnostics").frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(5, 5, 5)).inner_margin(6.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(sample) = &app.latest_sample {
                    let rx_str = format!(
                        r#"RX {{"BATT":{{"V":{:>5.2},"I":{:>6.2},"P":{:>6.2},"E":{:>6.1},"ah":{:>6.4},"t":{:>4.1},"sv_mv":{:>5.0}}}, "FC":{{"V":{:>5.2},"I":{:>6.2},"P":{:>6.2},"E":{:>6.1},"ah":{:>6.4},"t":{:>4.1},"sv_mv":{:>5.0}}}}}"#,
                        sample.batt.v, sample.batt.i, sample.batt.p, sample.batt.e, sample.batt.ah, sample.batt.t, sample.batt.sv_mv,
                        sample.fc.v, sample.fc.i, sample.fc.p, sample.fc.e, sample.fc.ah, sample.fc.t, sample.fc.sv_mv
                    );
                    ui.label(egui::RichText::new(rx_str).color(egui::Color32::WHITE).size(12.0).monospace());
                }

                if let Some(aux) = &app.latest_aux {
                    let aux_str = format!("AUX REV3 {} | T[{:>5.1}/{:>5.1}/{:>5.1}/{:>5.1}] | MAX {:>5.1}C | F:{:>3}% | FLG:{:>3}",
                        aux.sensor_count, aux.temperature_c[0], aux.temperature_c[1], aux.temperature_c[2], aux.temperature_c[3],
                        aux.max_temperature_c, aux.fan_duty_percent, aux.flags
                    );
                    ui.label(egui::RichText::new(aux_str).color(egui::Color32::WHITE).size(12.0).monospace());
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(format!("PKT: {:>6}", app.packet_count)).color(egui::Color32::WHITE).size(12.0).monospace());
                });
            });
        });
    }

    // 3. Control Panel Footer
    egui::TopBottomPanel::bottom("controls").frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(12, 12, 12)).inner_margin(8.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("FAN").color(egui::Color32::from_gray(100)).size(10.0));
            egui::ComboBox::from_id_salt("fan_cb").selected_text(app.fan_mode.to_string()).show_ui(ui, |ui| {
                ui.selectable_value(&mut app.fan_mode, FanMode::Auto, "AUTO");
                ui.selectable_value(&mut app.fan_mode, FanMode::Manual, "MANUAL");
                ui.selectable_value(&mut app.fan_mode, FanMode::Off, "OFF");
            });

            ui.add(egui::Slider::new(&mut app.fan_duty, 0..=100).suffix("%"));
            if ui.button("SEND").clicked() {
                if let Some(tx) = &app.cmd_tx {
                    let cmd = match app.fan_mode {
                        FanMode::Manual => format!(r#"{{"cmd":"fan","mode":"manual","duty":{}}}"#, app.fan_duty),
                        mode => format!(r#"{{"cmd":"fan","mode":"{}"}}"#, mode.as_str()),
                    };
                    let _ = tx.send(cmd);
                }
            }

            ui.add_space(20.0);
            ui.label(egui::RichText::new("SIGN").color(egui::Color32::from_gray(100)).size(10.0));
            ui.add(egui::TextEdit::singleline(&mut app.driver_code).char_limit(3).desired_width(40.0));
            if ui.button("SEND").clicked() {
                if let Some(tx) = &app.cmd_tx {
                    let _ = tx.send(format!(r#"{{"cmd":"sign","driver":"{}"}}"#, app.driver_code.to_uppercase()));
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.toggle_value(&mut app.show_diagnostics, "🛠 DIAGNOSTICS");
            });
        });
    });

    // 4. Mini Stats Row (Bottom)
    egui::TopBottomPanel::bottom("stats_row").frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(18, 18, 18)).inner_margin(12.0)).show(ctx, |ui| {
        if let Some(sample) = &app.latest_sample {
            ui.horizontal(|ui| {
                let batt_v_avg = app.data_manager.compute_batt_voltage_avg(app.config.sma_window);
                let batt_i_avg = app.data_manager.compute_batt_current_avg(app.config.sma_window);
                let fc_v_avg = app.data_manager.compute_fc_voltage_avg(app.config.sma_window);
                let fc_i_avg = app.data_manager.compute_fc_current_avg(app.config.sma_window);

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

    // Live Anomaly Side Panel
    egui::SidePanel::right("anomaly_panel")
        .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(15, 15, 15)).inner_margin(12.0))
        .exact_width(240.0)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("STAV SYSTÉMU").color(egui::Color32::from_gray(140)).size(10.0).strong());
            ui.add_space(4.0);

            // Overarching real-time vehicle health badge
            let (status_title, status_desc, text_color, bg_color) = app.health_status.badge_info();
            egui::Frame::NONE
                .fill(bg_color)
                .stroke(egui::Stroke::new(1.0_f32, text_color))
                .corner_radius(4.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(status_title).color(text_color).strong().size(12.0));
                        ui.label(egui::RichText::new(status_desc).color(egui::Color32::from_gray(180)).size(10.0));
                    });
                });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            ui.label(egui::RichText::new("HISTORIE ANOMÁLIÍ").color(egui::Color32::from_gray(140)).size(10.0).strong());
            ui.add_space(6.0);

            if app.recent_anomalies.is_empty() {
                ui.label(egui::RichText::new("Žádné zaznamenané anomálie.").color(egui::Color32::from_gray(100)).size(11.0));
            } else {
                egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                    for anom in &app.recent_anomalies {
                        let color = match anom.severity {
                            crate::anomaly::AnomalySeverity::Critical => egui::Color32::from_rgb(255, 75, 75),
                            crate::anomaly::AnomalySeverity::Warning => egui::Color32::from_rgb(255, 205, 50),
                        };
                        ui.label(egui::RichText::new(&anom.ui_text).color(color).monospace().size(11.0));
                        ui.add_space(4.0);
                    }
                });
            }
        });

    // 5. Central Data Dashboard
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(10, 10, 10)).inner_margin(12.0)).show(ctx, |ui| {
        if let Some(sample) = &app.latest_sample {
            let bg_frame = egui::Frame::NONE.fill(egui::Color32::from_rgb(15, 15, 15)).corner_radius(4.0).inner_margin(12.0);
            
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

                        let (v_min, v_max) = app.data_manager.batt_v_min_max();
                        let (i_min, i_max) = app.data_manager.batt_i_min_max();
                        let (p_min, p_max) = app.data_manager.batt_p_min_max();

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
                                if let Some(aux) = &app.latest_aux {
                                    if (aux.flags & 0x01) != 0 { state = "SHORT (ZKRACOVANI)"; color = fc_color; } 
                                    else if (aux.flags & 0x02) != 0 { state = "PURGE (ODVOD)"; color = egui::Color32::from_rgb(80, 180, 220); }
                                }
                                ui.label(egui::RichText::new(format!("● {}", state)).color(color).size(11.0).strong());
                            });
                        });
                        ui.add_space(10.0);

                        let (v_min, v_max) = app.data_manager.fc_v_min_max();
                        let (i_min, i_max) = app.data_manager.fc_i_min_max();
                        let (p_min, p_max) = app.data_manager.fc_p_min_max();

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
            if let Some(aux) = &app.latest_aux {
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

            // Chart Navigation, Window & Zoom Toolbar
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("GRAPHS").color(egui::Color32::from_gray(120)).size(14.0).strong());
                ui.add_space(15.0);
                if ui.selectable_value(&mut app.chart_tab, ChartTab::Voltage, "[1] NAPĚTÍ (V)").clicked() {
                    app.recenter_chart();
                }
                if ui.selectable_value(&mut app.chart_tab, ChartTab::Current, "[2] PROUD (A)").clicked() {
                    app.recenter_chart();
                }
                if ui.selectable_value(&mut app.chart_tab, ChartTab::Power, "[3] VÝKON (W)").clicked() {
                    app.recenter_chart();
                }
                if ui.selectable_value(&mut app.chart_tab, ChartTab::Energy, "[4] ENERGIE (J)").clicked() {
                    app.recenter_chart();
                }

                if app.is_paused {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("⏸ POZASTAVENO [Mezerník]")
                            .color(egui::Color32::from_rgb(255, 200, 50))
                            .size(11.0)
                            .strong(),
                    );
                }

                ui.add_space(20.0);
                ui.label(egui::RichText::new("WINDOW:").color(egui::Color32::from_gray(120)).size(10.0).strong());
                if ui.selectable_value(&mut app.chart_window, 5.0, "5s").clicked() { app.recenter_chart(); }
                if ui.selectable_value(&mut app.chart_window, 10.0, "10s").clicked() { app.recenter_chart(); }
                if ui.selectable_value(&mut app.chart_window, 15.0, "15s").clicked() { app.recenter_chart(); }
                if ui.selectable_value(&mut app.chart_window, 30.0, "30s").clicked() { app.recenter_chart(); }
                if ui.selectable_value(&mut app.chart_window, 60.0, "60s").clicked() { app.recenter_chart(); }
                if ui.selectable_value(&mut app.chart_window, 120.0, "2m").clicked() { app.recenter_chart(); }

                ui.add_space(15.0);
                ui.separator();
                ui.add_space(10.0);

                let is_zoomed = if app.recenter_chart {
                    false
                } else {
                    egui_plot::PlotMemory::load(ctx, egui::Id::new("history_plot"))
                        .map(|m| !m.auto_bounds.x)
                        .unwrap_or(false)
                };

                if is_zoomed {
                    ui.label(egui::RichText::new("🔍 ZOOM").color(egui::Color32::from_rgb(255, 200, 50)).size(11.0).strong());
                } else {
                    ui.label(egui::RichText::new("● LIVE").color(egui::Color32::from_rgb(0, 220, 100)).size(11.0).strong());
                }

                if ui.button(egui::RichText::new("➕ ZOOM IN").size(11.0))
                    .on_hover_text("Přiblížit graf k pozici kurzoru nebo středu (nebo použijte kolečko myši)")
                    .clicked()
                {
                    app.zoom_chart(1.4);
                }

                if ui.button(egui::RichText::new("➖ ZOOM OUT").size(11.0))
                    .on_hover_text("Oddálit graf od pozice kurzoru nebo středu (nebo použijte kolečko myši)")
                    .clicked()
                {
                    app.zoom_chart(0.71);
                }

                let recenter_btn = egui::Button::new(
                    egui::RichText::new("⟲ RECENTER")
                        .color(if is_zoomed { egui::Color32::from_rgb(80, 220, 255) } else { egui::Color32::WHITE })
                        .size(11.0)
                        .strong()
                );
                if ui.add(recenter_btn)
                    .on_hover_text("Vrátit se zpět na živý generovaný graf (Klávesa: R nebo dvojklik na graf)")
                    .clicked()
                {
                    app.recenter_chart();
                }
            });
            ui.add_space(5.0);

            // Extract data
            let history = app.data_manager.history();
            let times = app.data_manager.time_labels();

            let is_zoomed = if app.recenter_chart {
                false
            } else {
                egui_plot::PlotMemory::load(ctx, egui::Id::new("history_plot"))
                    .map(|m| !m.auto_bounds.x)
                    .unwrap_or(false)
            };

            let start_idx = if is_zoomed {
                0
            } else {
                let latest_time = times.last().copied().unwrap_or(0.0);
                let time_threshold = latest_time - app.chart_window;
                times.partition_point(|&t| t < time_threshold)
            };

            let mut batt_points = Vec::with_capacity(times.len().saturating_sub(start_idx));
            let mut fc_points = Vec::with_capacity(times.len().saturating_sub(start_idx));

            for i in start_idx..times.len() {
                let t = times[i];
                let s = &history[i];
                
                let (b_val, f_val) = match app.chart_tab {
                    ChartTab::Voltage => (s.batt.v, s.fc.v),
                    ChartTab::Current => (s.batt.i, s.fc.i),
                    ChartTab::Power   => (s.batt.p, s.fc.p),
                    ChartTab::Energy  => (s.batt.e, s.fc.e),
                };
                batt_points.push([t, b_val]);
                fc_points.push([t, f_val]);
            }

            let y_axis_label = match app.chart_tab {
                ChartTab::Voltage => "NAPETI (V)",
                ChartTab::Current => "PROUD (A)",
                ChartTab::Power   => "VYKON (W)",
                ChartTab::Energy  => "ENERGIE (J)",
            };

            let mut plot = egui_plot::Plot::new("history_plot")
                .height(ui.available_height())
                .show_background(false)
                .show_axes([false, true]) 
                .legend(egui_plot::Legend::default())
                .allow_zoom(true)
                .allow_drag(true)
                .allow_scroll(true)
                .allow_boxed_zoom(true)
                .allow_double_click_reset(true)
                .coordinates_formatter(
                    egui_plot::Corner::LeftBottom,
                    egui_plot::CoordinatesFormatter::new(move |pt, _| {
                        format!("t = {:.1} s  |  {:.2}", pt.x, pt.y)
                    }),
                );

            if app.recenter_chart {
                plot = plot.reset();
                app.recenter_chart = false;
            }

            plot.show(ui, |plot_ui| {
                if let Some(factor) = app.pending_zoom_factor.take() {
                    if let Some(hover) = plot_ui.pointer_coordinate() {
                        plot_ui.zoom_bounds(egui::Vec2::splat(factor), hover);
                    } else {
                        let bounds = plot_ui.plot_bounds();
                        let center = egui_plot::PlotPoint::new(
                            (bounds.min()[0] + bounds.max()[0]) * 0.5,
                            (bounds.min()[1] + bounds.max()[1]) * 0.5,
                        );
                        plot_ui.zoom_bounds(egui::Vec2::splat(factor), center);
                    }
                }

                plot_ui.line(
                    egui_plot::Line::new(egui_plot::PlotPoints::new(batt_points))
                        .name(format!("BATT {}", y_axis_label))
                        .width(2.0_f32)
                        .color(egui::Color32::from_rgb(60, 180, 220)) 
                );
                plot_ui.line(
                    egui_plot::Line::new(egui_plot::PlotPoints::new(fc_points))
                        .name(format!("FC {}", y_axis_label))
                        .width(2.0_f32)
                        .color(egui::Color32::from_rgb(220, 60, 60)) 
                );
            });
        } else {
            ui.centered_and_justified(|ui| {
                ui.heading("Waiting for telemetry stream... Click CONNECT for COM port or DEMO MODE for testing.");
            });
        }
    });

    render_report_modal(app, ctx);
}

/// Renders an interactive modal window displaying comprehensive post-race analytics and export options.
fn render_report_modal(app: &mut TelemetryApp, ctx: &egui::Context) {
    if !app.show_report_modal {
        return;
    }

    let mut is_open = app.show_report_modal;
    let mut close_requested = false;
    let mut export_action: Option<(&'static str, String)> = None;

    egui::Window::new("📊 ZÁVĚREČNÁ ANALÝZA JÍZDY (POST-RACE REPORT)")
        .open(&mut is_open)
        .collapsible(false)
        .resizable(true)
        .default_width(740.0)
        .default_height(580.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            if let Some(report) = &app.active_report {
                // Header info
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("🏎 H2GP RC Speciál").color(egui::Color32::from_rgb(0, 210, 255)).strong().size(13.0));
                    ui.separator();
                    ui.label(egui::RichText::new(format!("Pilot: {}", app.driver_code)).color(egui::Color32::WHITE).strong());
                    ui.separator();
                    let m = (report.duration_s as u32) / 60;
                    let s = report.duration_s % 60.0;
                    ui.label(egui::RichText::new(format!("Doba stintu: {:02}:{:04.1}", m, s)).color(egui::Color32::from_gray(200)));
                    ui.separator();
                    ui.label(egui::RichText::new(format!("{} vzorků ({:.1} PPS)", report.total_samples, report.avg_pps)).color(egui::Color32::from_gray(180)));
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                // 4 Metric Scorecards in 2 Columns
                ui.columns(2, |cols| {
                    // Column 1
                    cols[0].group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new("HYBRIDNÍ ENERGETICKÁ BILANCE").color(egui::Color32::from_rgb(0, 230, 118)).size(11.0).strong());
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("{:.1} %", report.fc_energy_ratio_pct)).color(egui::Color32::from_rgb(0, 230, 118)).size(22.0).strong());
                            ui.label(egui::RichText::new("podíl palivového článku").color(egui::Color32::from_gray(160)).size(11.0));
                        });
                        ui.label(egui::RichText::new(format!("• Energie FC: {:.1} J | Baterie: {:.1} J", report.fc_energy_j, report.batt_energy_j)).color(egui::Color32::from_gray(180)).size(11.0));
                        ui.label(egui::RichText::new(format!("• Celková spotřeba: {:.1} J (prům. {:.1} W)", report.total_energy_j, report.avg_total_power_w)).color(egui::Color32::from_gray(180)).size(11.0));
                    });

                    cols[0].add_space(8.0);

                    cols[0].group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new("SKLIZEŇ REKUPERACE BRZDĚNÍM").color(egui::Color32::from_rgb(0, 210, 255)).size(11.0).strong());
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("+{:.1} J", report.regen_energy_j)).color(egui::Color32::from_rgb(0, 210, 255)).size(22.0).strong());
                            ui.label(egui::RichText::new(format!("(+{:.4} Ah)", report.regen_ah)).color(egui::Color32::from_gray(160)).size(11.0));
                        });
                        ui.label(egui::RichText::new(format!("• Špičkový brzdný proud: {:.2} A", report.peak_regen_current_a)).color(egui::Color32::from_gray(180)).size(11.0));
                        ui.label(egui::RichText::new("• Úspora energie generátorem před zatáčkami").color(egui::Color32::from_gray(140)).size(10.0));
                    });

                    // Column 2
                    cols[1].group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new("PROUDOVÉ A VÝKONOVÉ EXTRÉMY").color(egui::Color32::from_rgb(255, 215, 0)).size(11.0).strong());
                        ui.add_space(4.0);
                        let p_max = report.peak_batt_power_w.max(report.peak_fc_power_w);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("{:.1} W", p_max)).color(egui::Color32::WHITE).size(22.0).strong());
                            ui.label(egui::RichText::new("špičkový příkon (P_max)").color(egui::Color32::from_gray(160)).size(11.0));
                        });
                        ui.label(egui::RichText::new(format!("• Max. proud: Batt {:.2} A | FC {:.2} A", report.peak_batt_current_a, report.peak_fc_current_a)).color(egui::Color32::from_gray(180)).size(11.0));
                        ui.label(egui::RichText::new(format!("• Min. napětí FC pod zátěží: {:.2} V", report.min_fc_voltage_v)).color(egui::Color32::from_gray(180)).size(11.0));
                    });

                    cols[1].add_space(8.0);

                    cols[1].group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new("TEPLOTNÍ A BEZPEČNOSTNÍ STAV").color(egui::Color32::from_rgb(255, 140, 0)).size(11.0).strong());
                        ui.add_space(4.0);
                        let max_t = report.max_batt_temp_c.max(report.max_fc_temp_c);
                        let t_col = if max_t > 45.0 {
                            egui::Color32::from_rgb(255, 23, 68)
                        } else if max_t > 40.0 {
                            egui::Color32::from_rgb(255, 234, 0)
                        } else {
                            egui::Color32::from_rgb(0, 230, 118)
                        };
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("{:.1} °C", max_t)).color(t_col).size(22.0).strong());
                            ui.label(egui::RichText::new("max. teplota článků").color(egui::Color32::from_gray(160)).size(11.0));
                        });
                        ui.label(egui::RichText::new(format!("• Teploty: Batt Max {:.1} °C | FC Max {:.1} °C", report.max_batt_temp_c, report.max_fc_temp_c)).color(egui::Color32::from_gray(180)).size(11.0));
                        let incident_txt = if report.critical_count > 0 {
                            format!("🔴 {} kritických | 🟡 {} varování", report.critical_count, report.warning_count)
                        } else if report.warning_count > 0 {
                            format!("🟡 {} varování", report.warning_count)
                        } else {
                            "🟢 0 anomálií — čistá jízda".to_string()
                        };
                        ui.label(egui::RichText::new(format!("• Incidenty: {}", incident_txt)).color(egui::Color32::from_gray(180)).size(11.0));
                    });
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                // Incident List (Scrollable)
                ui.label(egui::RichText::new(format!("PŘEHLED BEZPEČNOSTNÍCH INCIDENTŮ ({})", report.incidents.len())).color(egui::Color32::from_gray(140)).size(11.0).strong());
                ui.add_space(4.0);

                egui::ScrollArea::vertical().max_height(130.0).show(ui, |ui| {
                    if report.incidents.is_empty() {
                        ui.label(egui::RichText::new("✔ Během stintu nebyly detekovány žádné anomálie.").color(egui::Color32::from_rgb(0, 230, 118)).size(12.0));
                    } else {
                        for inc in &report.incidents {
                            let time_s = f64::from(inc.timestamp_ms) / 1000.0;
                            let m = (time_s as u32) / 60;
                            let s = time_s % 60.0;
                            let (color, tag) = match inc.severity {
                                crate::anomaly::AnomalySeverity::Critical => (egui::Color32::from_rgb(255, 60, 60), "[CRIT]"),
                                crate::anomaly::AnomalySeverity::Warning => (egui::Color32::from_rgb(255, 220, 0), "[WARN]"),
                            };

                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("[{:02}:{:04.1}]", m, s)).color(egui::Color32::from_gray(120)).monospace().size(11.0));
                                ui.label(egui::RichText::new(tag).color(color).strong().size(11.0));
                                ui.label(egui::RichText::new(&inc.description).color(color).size(11.0));
                            });
                        }
                    }
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                // Action Buttons at the bottom
                ui.horizontal(|ui| {
                    if ui.button("🌐 ULOŽIT HTML").on_hover_text("Uložit grafický HTML dashboard do složky reports/").clicked() {
                        let slug = crate::report::generate_timestamp_slug();
                        let (html_path, _) = crate::report::report_file_paths(&slug);
                        if let Err(e) = report.export_html(&app.driver_code, &html_path) {
                            export_action = Some(("error", format!("Chyba exportu: {}", e)));
                        } else {
                            export_action = Some(("success", format!("Report uložen do {}", html_path.display())));
                        }
                    }

                    if ui.button("📝 ULOŽIT MARKDOWN").on_hover_text("Uložit přehledný Markdown report do složky reports/").clicked() {
                        let slug = crate::report::generate_timestamp_slug();
                        let (_, md_path) = crate::report::report_file_paths(&slug);
                        if let Err(e) = report.export_markdown(&app.driver_code, &md_path) {
                            export_action = Some(("error", format!("Chyba exportu: {}", e)));
                        } else {
                            export_action = Some(("success", format!("Markdown uložen do {}", md_path.display())));
                        }
                    }

                    if ui.button("📋 KOPÍROVAT").on_hover_text("Zkopírovat textový souhrn do schránky").clicked() {
                        let clip = report.to_clipboard_text(&app.driver_code);
                        ctx.copy_text(clip);
                        export_action = Some(("success", "Souhrn zkopírován do schránky".to_string()));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("❌ ZAVŘÍT (Esc)").clicked() {
                            close_requested = true;
                        }
                    });
                });
            } else {
                ui.label("Žádná data pro report nejsou k dispozici.");
            }
        });

    if close_requested {
        is_open = false;
    }

    if let Some((kind, msg)) = export_action {
        if kind == "success" {
            println!("[report] {}", msg);
            app.report_toast = Some((format!("✔ {}", msg), std::time::Instant::now() + std::time::Duration::from_secs(4)));
        } else {
            eprintln!("[report] {}", msg);
            app.report_toast = Some((format!("❌ {}", msg), std::time::Instant::now() + std::time::Duration::from_secs(4)));
        }
    }

    app.show_report_modal = is_open;
}