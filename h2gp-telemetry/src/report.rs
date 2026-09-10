//! # Post-Race Analytics & Telemetry Reporting
//!
//! Generates comprehensive analytical post-session reports for the H2GP
//! hydrogen fuel cell vehicle. Evaluates hybrid powertrain balance,
//! energy split ratios, regenerative braking harvest, thermal stress,
//! and safety incident timelines.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::protocol::TelemetrySample;
use crate::anomaly::AnomalySeverity;
use crate::AnomalyRecord;
use crate::error::TelemetryError;

/// An individual incident entry in the post-race summary report.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportIncident {
    /// Timestamp relative to session start in milliseconds.
    pub timestamp_ms: u32,
    /// Severity level (`Warning` or `Critical`).
    pub severity: AnomalySeverity,
    /// Human-readable description of the anomaly.
    pub description: String,
}

/// Comprehensive post-session race telemetry report.
#[derive(Debug, Clone)]
pub struct SessionReport {
    /// Session or stint duration in seconds.
    pub duration_s: f64,
    /// Total number of valid telemetry frames recorded.
    pub total_samples: usize,
    /// Average packet reception rate in Hz.
    pub avg_pps: f64,

    // Hybrid Powertrain Energy
    /// Total cumulative energy delivered by the fuel cell in Joules.
    pub fc_energy_j: f64,
    /// Total cumulative energy delivered by the LiPo battery in Joules.
    pub batt_energy_j: f64,
    /// Total net energy consumed by the vehicle in Joules.
    pub total_energy_j: f64,
    /// Percentage of total energy provided by the hydrogen fuel cell (0–100%).
    pub fc_energy_ratio_pct: f64,

    // Regenerative Braking
    /// Total energy harvested during regenerative braking (negative current) in Joules.
    pub regen_energy_j: f64,
    /// Total electrical charge recovered in Ampere-hours.
    pub regen_ah: f64,
    /// Peak regenerative braking current in Amperes (negative value).
    pub peak_regen_current_a: f64,

    // Power & Electrical Extremes
    /// Average total power draw of the car in Watts.
    pub avg_total_power_w: f64,
    /// Average power supplied by the fuel cell in Watts.
    pub avg_fc_power_w: f64,
    /// Average power supplied by the battery in Watts.
    pub avg_batt_power_w: f64,
    /// Peak instantaneous power supplied by the fuel cell in Watts.
    pub peak_fc_power_w: f64,
    /// Peak instantaneous power supplied by the battery in Watts.
    pub peak_batt_power_w: f64,
    /// Peak current drawn from the battery in Amperes.
    pub peak_batt_current_a: f64,
    /// Peak current drawn from the fuel cell in Amperes.
    pub peak_fc_current_a: f64,
    /// Lowest recorded fuel cell voltage under load (excluding disconnected < 2.0 V) in Volts.
    pub min_fc_voltage_v: f64,

    // Thermal & Auxiliary Status
    /// Maximum temperature reached by the LiPo battery in °C.
    pub max_batt_temp_c: f64,
    /// Minimum temperature recorded for the battery in °C.
    pub min_batt_temp_c: f64,
    /// Maximum temperature reached by the fuel cell stack in °C.
    pub max_fc_temp_c: f64,
    /// Minimum temperature recorded for the fuel cell in °C.
    pub min_fc_temp_c: f64,
    /// Maximum fan speed duty cycle commanded during the run in %.
    pub max_fan_duty_pct: i32,

    // Incidents
    /// Count of critical safety hazards detected.
    pub critical_count: usize,
    /// Count of warning level deviations detected.
    pub warning_count: usize,
    /// Detailed list of all recorded incidents.
    pub incidents: Vec<ReportIncident>,
}

impl Default for SessionReport {
    fn default() -> Self {
        Self {
            duration_s: 0.0,
            total_samples: 0,
            avg_pps: 0.0,
            fc_energy_j: 0.0,
            batt_energy_j: 0.0,
            total_energy_j: 0.0,
            fc_energy_ratio_pct: 0.0,
            regen_energy_j: 0.0,
            regen_ah: 0.0,
            peak_regen_current_a: 0.0,
            avg_total_power_w: 0.0,
            avg_fc_power_w: 0.0,
            avg_batt_power_w: 0.0,
            peak_fc_power_w: 0.0,
            peak_batt_power_w: 0.0,
            peak_batt_current_a: 0.0,
            peak_fc_current_a: 0.0,
            min_fc_voltage_v: 0.0,
            max_batt_temp_c: 0.0,
            min_batt_temp_c: 0.0,
            max_fc_temp_c: 0.0,
            min_fc_temp_c: 0.0,
            max_fan_duty_pct: 0,
            critical_count: 0,
            warning_count: 0,
            incidents: Vec::new(),
        }
    }
}

impl SessionReport {
    /// Computes a full post-session analytics report from raw telemetry history and anomaly records.
    ///
    /// Correctly handles empty datasets, division-by-zero risks, and calculates regenerative braking
    /// by integrating time intervals where battery current is negative ($I_{batt} < -0.05\,\text{A}$).
    #[must_use]
    pub fn from_telemetry(
        history: &[TelemetrySample],
        anomalies: &[AnomalyRecord],
        stint_duration_s: f64,
        current_pps: f64,
    ) -> Self {
        if history.is_empty() {
            return Self {
                duration_s: stint_duration_s,
                avg_pps: current_pps,
                ..Default::default()
            };
        }

        let total_samples = history.len();
        let first_ts = history.first().map_or(0, |s| s.timestamp_ms);
        let last_ts = history.last().map_or(0, |s| s.timestamp_ms);
        let measured_duration_s = if last_ts > first_ts {
            f64::from(last_ts - first_ts) / 1000.0
        } else {
            stint_duration_s
        };
        let duration_s = if measured_duration_s > 0.0 { measured_duration_s } else { stint_duration_s };

        let avg_pps = if duration_s > 0.5 {
            (total_samples as f64 / duration_s).min(100.0)
        } else {
            current_pps
        };

        // Energy totals from the last recorded sample
        let last_sample = history.last();
        let fc_energy_j = last_sample.map_or(0.0, |s| s.fc.e).max(0.0);
        let batt_energy_j = last_sample.map_or(0.0, |s| s.batt.e).max(0.0);
        let total_energy_j = fc_energy_j + batt_energy_j;
        let fc_energy_ratio_pct = if total_energy_j > 0.001 {
            (fc_energy_j / total_energy_j) * 100.0
        } else {
            0.0
        };

        // Regenerative braking and power integrations
        let mut regen_energy_j = 0.0;
        let mut regen_ah = 0.0;
        let mut peak_regen_current_a = 0.0f64;

        let mut sum_total_power = 0.0;
        let mut sum_fc_power = 0.0;
        let mut sum_batt_power = 0.0;

        let mut peak_fc_power_w = 0.0f64;
        let mut peak_batt_power_w = 0.0f64;
        let mut peak_batt_current_a = 0.0f64;
        let mut peak_fc_current_a = 0.0f64;
        let mut min_fc_voltage_v = f64::INFINITY;

        let mut max_batt_temp_c = -100.0f64;
        let mut min_batt_temp_c = 1000.0f64;
        let mut max_fc_temp_c = -100.0f64;
        let mut min_fc_temp_c = 1000.0f64;

        for (idx, sample) in history.iter().enumerate() {
            let p_total = sample.batt.p + sample.fc.p;
            sum_total_power += p_total;
            sum_fc_power += sample.fc.p;
            sum_batt_power += sample.batt.p;

            if sample.fc.p > peak_fc_power_w { peak_fc_power_w = sample.fc.p; }
            if sample.batt.p > peak_batt_power_w { peak_batt_power_w = sample.batt.p; }
            if sample.batt.i > peak_batt_current_a { peak_batt_current_a = sample.batt.i; }
            if sample.fc.i > peak_fc_current_a { peak_fc_current_a = sample.fc.i; }

            // Lowest FC voltage under load (ignoring disconnected fuel cell < 2.0 V)
            if sample.fc.v >= 2.0 && sample.fc.v < min_fc_voltage_v {
                min_fc_voltage_v = sample.fc.v;
            }

            // Temperatures (filter out invalid disconnected values < -30°C)
            if sample.batt.t > -30.0 {
                if sample.batt.t > max_batt_temp_c { max_batt_temp_c = sample.batt.t; }
                if sample.batt.t < min_batt_temp_c { min_batt_temp_c = sample.batt.t; }
            }
            if sample.fc.t > -30.0 {
                if sample.fc.t > max_fc_temp_c { max_fc_temp_c = sample.fc.t; }
                if sample.fc.t < min_fc_temp_c { min_fc_temp_c = sample.fc.t; }
            }

            // Regenerative braking: negative battery current
            if sample.batt.i < -0.05 {
                let current_neg = sample.batt.i;
                if current_neg < peak_regen_current_a {
                    peak_regen_current_a = current_neg;
                }

                // Determine dt
                let dt_s = if idx > 0 {
                    let prev_ts = history[idx - 1].timestamp_ms;
                    if sample.timestamp_ms > prev_ts {
                        (f64::from(sample.timestamp_ms - prev_ts) / 1000.0).clamp(0.01, 1.0)
                    } else {
                        0.1
                    }
                } else {
                    0.1
                };

                let regen_power_w = sample.batt.v * current_neg.abs();
                regen_energy_j += regen_power_w * dt_s;
                regen_ah += (current_neg.abs() * dt_s) / 3600.0;
            }
        }

        let n = total_samples as f64;
        let avg_total_power_w = sum_total_power / n;
        let avg_fc_power_w = sum_fc_power / n;
        let avg_batt_power_w = sum_batt_power / n;

        if min_fc_voltage_v.is_infinite() {
            min_fc_voltage_v = 0.0;
        }
        if min_batt_temp_c > 500.0 { min_batt_temp_c = 0.0; }
        if max_batt_temp_c < -50.0 { max_batt_temp_c = 0.0; }
        if min_fc_temp_c > 500.0 { min_fc_temp_c = 0.0; }
        if max_fc_temp_c < -50.0 { max_fc_temp_c = 0.0; }

        // Process anomalies
        let mut critical_count = 0;
        let mut warning_count = 0;
        let mut incidents = Vec::with_capacity(anomalies.len());

        for record in anomalies {
            match record.severity {
                AnomalySeverity::Critical => critical_count += 1,
                AnomalySeverity::Warning => warning_count += 1,
            }
            incidents.push(ReportIncident {
                timestamp_ms: record.timestamp_ms,
                severity: record.severity,
                description: record.ui_text.clone(),
            });
        }

        Self {
            duration_s,
            total_samples,
            avg_pps,
            fc_energy_j,
            batt_energy_j,
            total_energy_j,
            fc_energy_ratio_pct,
            regen_energy_j,
            regen_ah,
            peak_regen_current_a,
            avg_total_power_w,
            avg_fc_power_w,
            avg_batt_power_w,
            peak_fc_power_w,
            peak_batt_power_w,
            peak_batt_current_a,
            peak_fc_current_a,
            min_fc_voltage_v,
            max_batt_temp_c,
            min_batt_temp_c,
            max_fc_temp_c,
            min_fc_temp_c,
            max_fan_duty_pct: 0,
            critical_count,
            warning_count,
            incidents,
        }
    }

    /// Exports the report as a modern, self-contained HTML document to the specified path.
    pub fn export_html(&self, driver_code: &str, file_path: &Path) -> Result<(), TelemetryError> {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(file_path)?;

        let (time_str, date_str) = format_current_datetime();

        let status_color = if self.critical_count > 0 {
            "#FF1744"
        } else if self.warning_count > 0 {
            "#FFEA00"
        } else {
            "#00E676"
        };

        let status_label = if self.critical_count > 0 {
            format!("KRITICKÉ RIZIKO ({} incidentů)", self.critical_count)
        } else if self.warning_count > 0 {
            format!("VAROVÁNÍ ({} incidentů)", self.warning_count)
        } else {
            "NOMINÁLNÍ STAV — BEZ ZÁVAD".to_string()
        };

        writeln!(file, "<!DOCTYPE html>")?;
        writeln!(file, "<html lang=\"cs\">")?;
        writeln!(file, "<head>")?;
        writeln!(file, "  <meta charset=\"UTF-8\">")?;
        writeln!(file, "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">")?;
        writeln!(file, "  <title>H2GP Telemetrie — Závěrečný report [{}]</title>", driver_code)?;
        writeln!(file, "  <style>")?;
        writeln!(file, r#"
    :root {{
      --bg: #0d0f12;
      --card-bg: #161a20;
      --card-border: #262c36;
      --text: #f0f3f6;
      --text-dim: #8b949e;
      --accent: #38ef7d;
      --warn: #ffea00;
      --crit: #ff1744;
      --cyan: #00d2ff;
    }}
    body {{
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
      background-color: var(--bg);
      color: var(--text);
      margin: 0;
      padding: 30px 20px;
    }}
    .container {{
      max-width: 960px;
      margin: 0 auto;
    }}
    header {{
      display: flex;
      justify-content: space-between;
      align-items: center;
      border-bottom: 2px solid var(--card-border);
      padding-bottom: 20px;
      margin-bottom: 25px;
    }}
    h1 {{
      margin: 0;
      font-size: 24px;
      letter-spacing: 1px;
      color: #fff;
    }}
    .subhead {{
      font-size: 13px;
      color: var(--text-dim);
      margin-top: 5px;
    }}
    .status-badge {{
      display: inline-block;
      padding: 6px 14px;
      border-radius: 6px;
      font-weight: bold;
      font-size: 13px;
      border: 1px solid;
    }}
    .grid {{
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
      gap: 16px;
      margin-bottom: 25px;
    }}
    .card {{
      background: var(--card-bg);
      border: 1px solid var(--card-border);
      border-radius: 8px;
      padding: 16px;
    }}
    .card-title {{
      font-size: 11px;
      font-weight: 700;
      text-transform: uppercase;
      color: var(--text-dim);
      letter-spacing: 0.5px;
      margin-bottom: 8px;
    }}
    .card-val {{
      font-size: 26px;
      font-weight: 800;
      font-family: monospace;
      color: #fff;
    }}
    .card-val.accent {{ color: var(--accent); }}
    .card-val.cyan {{ color: var(--cyan); }}
    .card-val.warn {{ color: var(--warn); }}
    .card-val.crit {{ color: var(--crit); }}
    .card-detail {{
      font-size: 12px;
      color: var(--text-dim);
      margin-top: 6px;
    }}
    table {{
      width: 100%;
      border-collapse: collapse;
      margin-top: 15px;
      font-size: 13px;
    }}
    th, td {{
      padding: 10px 12px;
      text-align: left;
      border-bottom: 1px solid var(--card-border);
    }}
    th {{
      color: var(--text-dim);
      font-weight: 600;
      text-transform: uppercase;
      font-size: 11px;
      background: #12151a;
    }}
    tr:hover {{ background: rgba(255,255,255,0.02); }}
    .badge-warn {{ color: var(--warn); font-weight: bold; }}
    .badge-crit {{ color: var(--crit); font-weight: bold; }}
    footer {{
      margin-top: 35px;
      text-align: center;
      font-size: 11px;
      color: var(--text-dim);
      border-top: 1px solid var(--card-border);
      padding-top: 15px;
    }}
    @media print {{
      body {{ background: #fff; color: #000; }}
      .card {{ border: 1px solid #ccc; background: #fafafa; }}
      .card-val {{ color: #000 !important; }}
      header {{ border-bottom: 2px solid #000; }}
    }}
  "#)?;
        writeln!(file, "  </style>")?;
        writeln!(file, "</head>")?;
        writeln!(file, "<body>")?;
        writeln!(file, "  <div class=\"container\">")?;

        // Header
        writeln!(file, "    <header>")?;
        writeln!(file, "      <div>")?;
        writeln!(file, "        <h1>🏎 H2GP POST-RACE ANALÝZA JÍZDY</h1>")?;
        writeln!(file, "        <div class=\"subhead\">Vozidlo: H2GP RC Speciál | Pilot: <strong>{}</strong> | Datum: {} {}</div>", driver_code, date_str, time_str)?;
        writeln!(file, "      </div>")?;
        writeln!(file, "      <div>")?;
        writeln!(file, "        <span class=\"status-badge\" style=\"border-color: {}; color: {}; background: rgba(0,0,0,0.3);\">● {}</span>", status_color, status_color, status_label)?;
        writeln!(file, "      </div>")?;
        writeln!(file, "    </header>")?;

        // KPI Grid 1: Stint & Energy
        writeln!(file, "    <div class=\"grid\">")?;
        writeln!(file, "      <div class=\"card\">")?;
        writeln!(file, "        <div class=\"card-title\">Délka jízdy & Pakety</div>")?;
        writeln!(file, "        <div class=\"card-val cyan\">{:02}:{:04.1}</div>", (self.duration_s as u32) / 60, self.duration_s % 60.0)?;
        writeln!(file, "        <div class=\"card-detail\">{} vzorků @ {:.1} PPS</div>", self.total_samples, self.avg_pps)?;
        writeln!(file, "      </div>")?;

        writeln!(file, "      <div class=\"card\">")?;
        writeln!(file, "        <div class=\"card-title\">Hybridní bilance (FC Podíl)</div>")?;
        writeln!(file, "        <div class=\"card-val accent\">{:.1} %</div>", self.fc_energy_ratio_pct)?;
        writeln!(file, "        <div class=\"card-detail\">FC: {:.1} J | Baterie: {:.1} J</div>", self.fc_energy_j, self.batt_energy_j)?;
        writeln!(file, "      </div>")?;

        writeln!(file, "      <div class=\"card\">")?;
        writeln!(file, "        <div class=\"card-title\">Sklizeň rekuperace</div>")?;
        writeln!(file, "        <div class=\"card-val accent\">+{:.1} J</div>", self.regen_energy_j)?;
        writeln!(file, "        <div class=\"card-detail\">+{:.4} Ah | Max: {:.2} A</div>", self.regen_ah, self.peak_regen_current_a)?;
        writeln!(file, "      </div>")?;

        writeln!(file, "      <div class=\"card\">")?;
        writeln!(file, "        <div class=\"card-title\">Celková energie</div>")?;
        writeln!(file, "        <div class=\"card-val\">{:.1} J</div>", self.total_energy_j)?;
        writeln!(file, "        <div class=\"card-detail\">Prům. příkon: {:.1} W</div>", self.avg_total_power_w)?;
        writeln!(file, "      </div>")?;
        writeln!(file, "    </div>")?;

        // KPI Grid 2: Electrical & Thermals
        writeln!(file, "    <div class=\"grid\">")?;
        writeln!(file, "      <div class=\"card\">")?;
        writeln!(file, "        <div class=\"card-title\">Špičkový výkon (P_max)</div>")?;
        writeln!(file, "        <div class=\"card-val\">{:.1} W</div>", self.peak_batt_power_w.max(self.peak_fc_power_w))?;
        writeln!(file, "        <div class=\"card-detail\">Batt P_max: {:.1} W | FC P_max: {:.1} W</div>", self.peak_batt_power_w, self.peak_fc_power_w)?;
        writeln!(file, "      </div>")?;

        writeln!(file, "      <div class=\"card\">")?;
        writeln!(file, "        <div class=\"card-title\">Špičkový proud (I_max)</div>")?;
        writeln!(file, "        <div class=\"card-val warn\">{:.2} A</div>", self.peak_batt_current_a)?;
        writeln!(file, "        <div class=\"card-detail\">Batt: {:.2} A | FC: {:.2} A</div>", self.peak_batt_current_a, self.peak_fc_current_a)?;
        writeln!(file, "      </div>")?;

        writeln!(file, "      <div class=\"card\">")?;
        writeln!(file, "        <div class=\"card-title\">Min. napětí FC pod zátěží</div>")?;
        writeln!(file, "        <div class=\"card-val\">{:.2} V</div>", self.min_fc_voltage_v)?;
        writeln!(file, "        <div class=\"card-detail\">Indikace hladovění vodíku (&lt; 9.0 V)</div>")?;
        writeln!(file, "      </div>")?;

        writeln!(file, "      <div class=\"card\">")?;
        writeln!(file, "        <div class=\"card-title\">Teplotní extrémy</div>")?;
        writeln!(file, "        <div class=\"card-val\">{:.1} °C</div>", self.max_batt_temp_c.max(self.max_fc_temp_c))?;
        writeln!(file, "        <div class=\"card-detail\">Batt Max: {:.1} °C | FC Max: {:.1} °C</div>", self.max_batt_temp_c, self.max_fc_temp_c)?;
        writeln!(file, "      </div>")?;
        writeln!(file, "    </div>")?;

        // Incident Table
        writeln!(file, "    <div class=\"card\" style=\"margin-top: 10px;\">")?;
        writeln!(file, "      <div class=\"card-title\" style=\"font-size: 13px;\">Záznam bezpečnostních anomálií a incidentů ({})</div>", self.incidents.len())?;

        if self.incidents.is_empty() {
            writeln!(file, "      <p style=\"color: var(--accent); margin: 15px 0 5px 0; font-size: 13px;\">✔ Během jízdy nebyly zaznamenány žádné anomálie. Všechny subsystémy pracovaly v nominálním rozsahu.</p>")?;
        } else {
            writeln!(file, "      <table>")?;
            writeln!(file, "        <thead><tr><th>Čas</th><th>Závažnost</th><th>Popis incidentu</th></tr></thead>")?;
            writeln!(file, "        <tbody>")?;
            for inc in &self.incidents {
                let badge_class = match inc.severity {
                    AnomalySeverity::Critical => "badge-crit",
                    AnomalySeverity::Warning => "badge-warn",
                };
                let tag = inc.severity.tag_prefix();
                let time_sec = f64::from(inc.timestamp_ms) / 1000.0;
                let m = (time_sec as u32) / 60;
                let s = time_sec % 60.0;

                writeln!(file, "          <tr>")?;
                writeln!(file, "            <td style=\"font-family: monospace; font-size: 12px;\">{:02}:{:04.1}</td>", m, s)?;
                writeln!(file, "            <td class=\"{}\">{}</td>", badge_class, tag)?;
                writeln!(file, "            <td>{}</td>", inc.description)?;
                writeln!(file, "          </tr>")?;
            }
            writeln!(file, "        </tbody>")?;
            writeln!(file, "      </table>")?;
        }
        writeln!(file, "    </div>")?;

        // Footer
        writeln!(file, "    <footer>")?;
        writeln!(file, "      Vygenerováno telemetrickým systémem H2GP Telemetry v1.0.2 | Autoři: Jan Zedník & Tým H2GP | MIT Licencováno")?;
        writeln!(file, "    </footer>")?;

        writeln!(file, "  </div>")?;
        writeln!(file, "</body>")?;
        writeln!(file, "</html>")?;

        Ok(())
    }

    /// Exports the report as a clean GitHub-Flavored Markdown file.
    pub fn export_markdown(&self, driver_code: &str, file_path: &Path) -> Result<(), TelemetryError> {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(file_path)?;
        let (time_str, date_str) = format_current_datetime();

        writeln!(file, "# 🏎 H2GP Post-Race Report — Pilot: {}", driver_code)?;
        writeln!(file)?;
        writeln!(file, "**Datum a čas:** {} {} | **Délka jízdy:** {:02}:{:04.1} | **Vzorků:** {} (@ {:.1} PPS)",
            date_str, time_str, (self.duration_s as u32) / 60, self.duration_s % 60.0, self.total_samples, self.avg_pps)?;
        writeln!(file)?;

        if self.critical_count > 0 {
            writeln!(file, "> [!CAUTION]")?;
            writeln!(file, "> **Kritické riziko:** Během jízdy nastalo {} kritických bezpečnostních incidentů!", self.critical_count)?;
            writeln!(file)?;
        } else if self.warning_count > 0 {
            writeln!(file, "> [!WARNING]")?;
            writeln!(file, "> **Varování:** Zaznamenáno {} provozních odchylek.", self.warning_count)?;
            writeln!(file)?;
        } else {
            writeln!(file, "> [!NOTE]")?;
            writeln!(file, "> **Nominální stav:** Jízda proběhla bez zaznamenaných anomálií.")?;
            writeln!(file)?;
        }

        writeln!(file, "## 1. Hybridní energetická bilance")?;
        writeln!(file)?;
        writeln!(file, "| Parametr | Hodnota | Poznámka |")?;
        writeln!(file, "| :--- | :--- | :--- |")?;
        writeln!(file, "| **Podíl palivového článku** | **{:.1} %** | Cíl pro vytrvalost: > 70 % |", self.fc_energy_ratio_pct)?;
        writeln!(file, "| **Energie palivového článku** | {:.2} J | Dodáno z vodíku |", self.fc_energy_j)?;
        writeln!(file, "| **Energie akumulátoru** | {:.2} J | Dodáno z LiPo |", self.batt_energy_j)?;
        writeln!(file, "| **Celková spotřebovaná energie** | {:.2} J | Celkový výdej vozu |", self.total_energy_j)?;
        writeln!(file, "| **Rekuperovaná energie brzděním** | **+{:.2} J** (+{:.4} Ah) | Ušetřeno motorgenerátorem |", self.regen_energy_j, self.regen_ah)?;
        writeln!(file, "| **Špičkový rekuperační proud** | {:.2} A | Maximální brzdný proud |", self.peak_regen_current_a)?;
        writeln!(file)?;

        writeln!(file, "## 2. Výkonové a tepelné extrémy")?;
        writeln!(file)?;
        writeln!(file, "| Metrika | Baterie (LiPo) | Palivový článek (FC) | Celkem |")?;
        writeln!(file, "| :--- | :--- | :--- | :--- |")?;
        writeln!(file, "| **Průměrný výkon** | {:.2} W | {:.2} W | **{:.2} W** |", self.avg_batt_power_w, self.avg_fc_power_w, self.avg_total_power_w)?;
        writeln!(file, "| **Špičkový výkon (P_max)** | {:.2} W | {:.2} W | **{:.2} W** |", self.peak_batt_power_w, self.peak_fc_power_w, self.peak_batt_power_w.max(self.peak_fc_power_w))?;
        writeln!(file, "| **Špičkový proud (I_max)** | {:.2} A | {:.2} A | — |", self.peak_batt_current_a, self.peak_fc_current_a)?;
        writeln!(file, "| **Min. napětí pod zátěží** | — | **{:.2} V** | Limit hladovění: 9.0 V |", self.min_fc_voltage_v)?;
        writeln!(file, "| **Maximální teplota** | {:.1} °C | {:.1} °C | Limit: 45.0 °C |", self.max_batt_temp_c, self.max_fc_temp_c)?;
        writeln!(file)?;

        writeln!(file, "## 3. Záznam incidentů a anomálií ({})", self.incidents.len())?;
        writeln!(file)?;
        if self.incidents.is_empty() {
            writeln!(file, "*Žádné anomálie nebyly zaznamenány.*")?;
        } else {
            writeln!(file, "| Čas | Závažnost | Detail incidentu |")?;
            writeln!(file, "| :--- | :--- | :--- |")?;
            for inc in &self.incidents {
                let time_sec = f64::from(inc.timestamp_ms) / 1000.0;
                let m = (time_sec as u32) / 60;
                let s = time_sec % 60.0;
                let tag = inc.severity.tag_prefix();
                writeln!(file, "| `{:02}:{:04.1}` | **{}** | {} |", m, s, tag, inc.description)?;
            }
        }

        Ok(())
    }

    /// Formats the summary as a clean clipboard-ready plain text string.
    #[must_use]
    pub fn to_clipboard_text(&self, driver_code: &str) -> String {
        let (time_str, date_str) = format_current_datetime();
        format!(
            "========================================\n\
             H2GP POST-RACE REPORT [{}]\n\
             Datum: {} {} | Doba: {:02}:{:04.1}\n\
             ========================================\n\
             Hybridní bilance (FC) : {:.1} %\n\
             Energie FC            : {:.1} J\n\
             Energie Baterie       : {:.1} J\n\
             Celková energie       : {:.1} J\n\
             Rekuperace brzdění    : +{:.1} J (+{:.4} Ah)\n\
             Průměrný výkon        : {:.1} W\n\
             Špičkový proud Batt   : {:.2} A\n\
             Špičkový proud FC     : {:.2} A\n\
             Min. napětí FC        : {:.2} V\n\
             Max. teplota Baterie  : {:.1} °C\n\
             Max. teplota FC       : {:.1} °C\n\
             Počet incidentů       : {} (Krit: {}, Var: {})\n\
             ========================================",
            driver_code, date_str, time_str,
            (self.duration_s as u32) / 60, self.duration_s % 60.0,
            self.fc_energy_ratio_pct, self.fc_energy_j, self.batt_energy_j,
            self.total_energy_j, self.regen_energy_j, self.regen_ah,
            self.avg_total_power_w, self.peak_batt_current_a, self.peak_fc_current_a,
            self.min_fc_voltage_v, self.max_batt_temp_c, self.max_fc_temp_c,
            self.incidents.len(), self.critical_count, self.warning_count
        )
    }
}

/// Generates a standardized timestamp slug (`YYYYMMDD_HHMMSS`) for output filenames.
#[must_use]
pub fn generate_timestamp_slug() -> String {
    let now = SystemTime::now();
    let secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

    let days = (secs / 86400) as i64;
    let day_secs = (secs % 86400) as u32;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    let (year, month, day) = civil_from_days(days);
    format!("{:04}{:02}{:02}_{:02}{:02}{:02}", year, month, day, hours, minutes, seconds)
}

/// Helper function to format current UTC date and time as strings.
fn format_current_datetime() -> (String, String) {
    let now = SystemTime::now();
    let secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

    let days = (secs / 86400) as i64;
    let day_secs = (secs % 86400) as u32;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    let (year, month, day) = civil_from_days(days);
    let time_str = format!("{:02}:{:02}:{:02} UTC", hours, minutes, seconds);
    let date_str = format!("{:04}-{:02}-{:02}", year, month, day);
    (time_str, date_str)
}

/// Howard Hinnant's algorithm to compute Gregorian civil year, month, day from days since 1970-01-01.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Computes output file paths in the `reports/` folder for a newly generated report.
#[must_use]
pub fn report_file_paths(slug: &str) -> (PathBuf, PathBuf) {
    let html_path = PathBuf::from(format!("reports/report_{}.html", slug));
    let md_path = PathBuf::from(format!("reports/report_{}.md", slug));
    (html_path, md_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ChannelData, Rev3AuxData};

    fn make_sample(ts: u32, batt_v: f64, batt_i: f64, fc_v: f64, fc_i: f64, batt_e: f64, fc_e: f64) -> TelemetrySample {
        TelemetrySample {
            timestamp_ms: ts,
            batt: ChannelData {
                v: batt_v,
                i: batt_i,
                p: batt_v * batt_i,
                e: batt_e,
                ah: batt_e / (batt_v * 3600.0),
                sv_mv: 10.0,
                t: 35.0,
            },
            fc: ChannelData {
                v: fc_v,
                i: fc_i,
                p: fc_v * fc_i,
                e: fc_e,
                ah: fc_e / (fc_v * 3600.0),
                sv_mv: 10.0,
                t: 38.0,
            },
            rev3_aux: Rev3AuxData::default(),
            has_channel_data: true,
            has_rev3_aux: false,
        }
    }

    #[test]
    fn test_empty_telemetry_report() {
        let report = SessionReport::from_telemetry(&[], &[], 10.0, 10.0);
        assert_eq!(report.total_samples, 0);
        assert_eq!(report.total_energy_j, 0.0);
        assert_eq!(report.fc_energy_ratio_pct, 0.0);
        assert_eq!(report.critical_count, 0);
    }

    #[test]
    fn test_energy_split_and_regen_calculation() {
        let mut samples = Vec::new();
        // Sample 0: normal
        samples.push(make_sample(0, 7.4, 5.0, 12.0, 4.0, 37.0, 48.0));
        // Sample 1: braking regen (negative battery current -2.0 A)
        samples.push(make_sample(100, 7.6, -2.0, 12.0, 1.0, 35.0, 60.0));
        // Sample 2: normal cruise
        samples.push(make_sample(200, 7.3, 4.0, 12.0, 5.0, 50.0, 100.0));

        let report = SessionReport::from_telemetry(&samples, &[], 2.0, 10.0);

        assert_eq!(report.total_samples, 3);
        assert!(report.fc_energy_j >= 99.0);
        assert!(report.batt_energy_j >= 49.0);
        assert!(report.fc_energy_ratio_pct > 60.0);
        // Regen was detected
        assert!(report.regen_energy_j > 0.0, "Expected positive harvested regen energy");
        assert!(report.regen_ah > 0.0);
        assert_eq!(report.peak_regen_current_a, -2.0);
    }

    #[test]
    fn test_html_and_markdown_exports() {
        let samples = vec![
            make_sample(0, 7.4, 3.0, 12.0, 4.0, 10.0, 20.0),
            make_sample(1000, 7.4, 3.0, 12.0, 4.0, 30.0, 60.0),
        ];
        let anomalies = vec![
            AnomalyRecord {
                timestamp_ms: 500,
                severity: AnomalySeverity::Warning,
                ui_text: "FC V-DROP : 10.2 V".to_string(),
            }
        ];

        let report = SessionReport::from_telemetry(&samples, &anomalies, 1.0, 10.0);
        assert_eq!(report.warning_count, 1);
        assert_eq!(report.critical_count, 0);

        let temp_dir = std::env::temp_dir();
        let html_file = temp_dir.join("test_report.html");
        let md_file = temp_dir.join("test_report.md");

        assert!(report.export_html("TST", &html_file).is_ok());
        assert!(report.export_markdown("TST", &md_file).is_ok());

        let html_content = fs::read_to_string(&html_file).expect("Read html");
        assert!(html_content.contains("H2GP POST-RACE ANALÝZA JÍZDY"));
        assert!(html_content.contains("TST"));
        assert!(html_content.contains("FC V-DROP : 10.2 V"));

        let md_content = fs::read_to_string(&md_file).expect("Read md");
        assert!(md_content.contains("# 🏎 H2GP Post-Race Report"));
        assert!(md_content.contains("TST"));

        let clip_text = report.to_clipboard_text("TST");
        assert!(clip_text.contains("H2GP POST-RACE REPORT [TST]"));

        let _ = fs::remove_file(html_file);
        let _ = fs::remove_file(md_file);
    }

    #[test]
    fn test_timestamp_slug_format() {
        let slug = generate_timestamp_slug();
        assert_eq!(slug.len(), 15, "Expected format YYYYMMDD_HHMMSS (15 chars)");
        assert!(slug.contains('_'));
    }

    #[test]
    fn test_report_on_demo_data_csv() {
        if let Ok(file) = File::open("data.csv") {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(file);
            let mut samples = Vec::new();
            for (idx, line) in reader.lines().skip(1).enumerate() {
                if let Ok(line) = line {
                    if let Some(sample) = crate::demo::parse_csv_line(&line, (idx as u32) * 100) {
                        samples.push(sample);
                    }
                }
            }
            if !samples.is_empty() {
                let report = SessionReport::from_telemetry(&samples, &[], 80.0, 10.0);
                assert!(report.total_samples >= 500);
                assert!(report.regen_energy_j > 0.0, "Expected regen energy from braking in data.csv");
                assert!(report.regen_ah > 0.0);
                assert!(report.fc_energy_ratio_pct > 0.0 && report.fc_energy_ratio_pct < 100.0);
            }
        }
    }
}
