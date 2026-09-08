//! # Anomaly Detection Engine
//!
//! Evaluates incoming telemetry samples against configured physical thresholds
//! to identify critical racing conditions (servo stalls, starvation, overheating, etc.).

use crate::config::AppConfig;
use crate::protocol::TelemetrySample;

/// Classification category for detected operational anomalies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyKind {
    /// Battery current exceeded safety threshold (e.g. servo stall).
    BattOvercurrent,
    /// Fuel cell voltage dropped under heavy load (hydrogen starvation).
    FcVoltageSag,
    /// Battery temperature exceeded safe thermal threshold.
    BattOvertemp,
    /// Active short-circuiting flag commanded on fuel cell.
    FcShort,
}

impl AnomalyKind {
    /// Short mnemonic identifier used for UI alert badges and log tags.
    pub fn tag(&self) -> &'static str {
        match self {
            AnomalyKind::BattOvercurrent => "BATT OVERCURRENT",
            AnomalyKind::FcVoltageSag => "FC V-SAG",
            AnomalyKind::BattOvertemp => "BATT OVERTEMP",
            AnomalyKind::FcShort => "FC SHORT",
        }
    }
}

/// A structured anomaly event describing a physical threshold violation.
#[derive(Debug, Clone, PartialEq)]
pub struct Anomaly {
    /// Timestamp in milliseconds when the anomaly was detected.
    pub timestamp_ms: u32,
    /// Classification category of the anomaly.
    pub kind: AnomalyKind,
    /// Formatted measurement string at the moment of violation (e.g., "16.42 A").
    pub value_str: String,
    /// Diagnostic description of the likely root cause.
    pub cause: &'static str,
}

impl Anomaly {
    /// Formats the anomaly into a UI log string `[MM:SS] TAG : VALUE`.
    pub fn format_ui(&self) -> String {
        let secs = self.timestamp_ms / 1000;
        let m = secs / 60;
        let s = secs % 60;
        format!("[{:02}:{:02}] {} : {}", m, s, self.kind.tag(), self.value_str)
    }
}

/// Evaluates a telemetry sample against the active configuration thresholds
/// and returns a list of all detected anomalies.
pub fn detect_anomalies(sample: &TelemetrySample, config: &AppConfig) -> Vec<Anomaly> {
    let mut anomalies = Vec::new();

    if sample.has_channel_data {
        if sample.batt.i > config.anomaly_batt_overcurrent_a {
            anomalies.push(Anomaly {
                timestamp_ms: sample.timestamp_ms,
                kind: AnomalyKind::BattOvercurrent,
                value_str: format!("{:.2} A", sample.batt.i),
                cause: "Servo stall",
            });
        }

        if sample.fc.v < config.anomaly_fc_vsag_v && sample.fc.v > config.anomaly_fc_min_v {
            anomalies.push(Anomaly {
                timestamp_ms: sample.timestamp_ms,
                kind: AnomalyKind::FcVoltageSag,
                value_str: format!("{:.2} V", sample.fc.v),
                cause: "Starvation",
            });
        }

        if sample.batt.t > config.anomaly_batt_overtemp_c {
            anomalies.push(Anomaly {
                timestamp_ms: sample.timestamp_ms,
                kind: AnomalyKind::BattOvertemp,
                value_str: format!("{:.1} C", sample.batt.t),
                cause: "High load",
            });
        }
    } else if sample.has_rev3_aux && (sample.rev3_aux.flags & 0x01) != 0 {
        anomalies.push(Anomaly {
            timestamp_ms: sample.timestamp_ms,
            kind: AnomalyKind::FcShort,
            value_str: "ZKRACOVANI".to_string(),
            cause: "Commanded FC short",
        });
    }

    anomalies
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ChannelData, Rev3AuxData};

    fn sample_with_batt(i: f64, t: f64) -> TelemetrySample {
        TelemetrySample {
            timestamp_ms: 65_000, // 01:05
            has_channel_data: true,
            has_rev3_aux: false,
            batt: ChannelData { i, t, ..Default::default() },
            fc: ChannelData { v: 12.0, ..Default::default() },
            rev3_aux: Default::default(),
        }
    }

    #[test]
    fn detects_batt_overcurrent() {
        let cfg = AppConfig::default();
        let sample = sample_with_batt(16.5, 25.0);
        let anoms = detect_anomalies(&sample, &cfg);
        assert_eq!(anoms.len(), 1);
        assert_eq!(anoms[0].kind, AnomalyKind::BattOvercurrent);
        assert_eq!(anoms[0].format_ui(), "[01:05] BATT OVERCURRENT : 16.50 A");
    }

    #[test]
    fn ignores_normal_current() {
        let cfg = AppConfig::default();
        let sample = sample_with_batt(5.0, 25.0);
        let anoms = detect_anomalies(&sample, &cfg);
        assert!(anoms.is_empty());
    }

    #[test]
    fn detects_fc_voltage_sag() {
        let cfg = AppConfig::default();
        let mut sample = sample_with_batt(2.0, 25.0);
        sample.fc.v = 7.5; // below 9.0 V sag threshold, above 2.0 V minimum
        let anoms = detect_anomalies(&sample, &cfg);
        assert_eq!(anoms.len(), 1);
        assert_eq!(anoms[0].kind, AnomalyKind::FcVoltageSag);
        assert_eq!(anoms[0].cause, "Starvation");
    }

    #[test]
    fn ignores_disconnected_fc_voltage() {
        let cfg = AppConfig::default();
        let mut sample = sample_with_batt(2.0, 25.0);
        sample.fc.v = 1.0; // below 2.0 V minimum -> disconnected, not starvation
        let anoms = detect_anomalies(&sample, &cfg);
        assert!(anoms.is_empty());
    }

    #[test]
    fn detects_batt_overtemp() {
        let cfg = AppConfig::default();
        let sample = sample_with_batt(2.0, 48.0); // above 45.0 °C
        let anoms = detect_anomalies(&sample, &cfg);
        assert_eq!(anoms.len(), 1);
        assert_eq!(anoms[0].kind, AnomalyKind::BattOvertemp);
    }

    #[test]
    fn detects_fc_short_flag() {
        let cfg = AppConfig::default();
        let sample = TelemetrySample {
            timestamp_ms: 10_000,
            has_channel_data: false,
            has_rev3_aux: true,
            batt: Default::default(),
            fc: Default::default(),
            rev3_aux: Rev3AuxData {
                flags: 0x01, // Bit 0 = short
                ..Default::default()
            },
        };
        let anoms = detect_anomalies(&sample, &cfg);
        assert_eq!(anoms.len(), 1);
        assert_eq!(anoms[0].kind, AnomalyKind::FcShort);
        assert_eq!(anoms[0].value_str, "ZKRACOVANI");
    }
}

