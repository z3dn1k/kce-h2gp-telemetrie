//! # Application Configuration & Runtime Parameters
//!
//! Centralizes all operational thresholds, buffer sizes, timing intervals,
//! and communication settings. Enables parameter tuning without code recompilation
//! via standard TOML configuration files (`config.toml`).

use std::fs;
use serde::{Deserialize, Serialize};

/// Global configuration settings for the H2GP telemetry desktop application.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    /// Serial connection baud rate (standard for vehicle MCU: 115200).
    pub serial_baud_rate: u32,

    /// Default serial port path or name (e.g., "COM3" or "/dev/ttyUSB0").
    pub default_port: String,

    /// Maximum number of historical samples stored in the in-memory circular buffer.
    pub buffer_capacity: usize,

    /// Number of recent samples used for Simple Moving Average (SMA) calculation.
    pub sma_window: usize,

    /// Battery current threshold in Amperes triggering an overcurrent warning.
    pub anomaly_batt_overcurrent_a: f64,

    /// Fuel cell voltage sag threshold in Volts indicating hydrogen starvation.
    pub anomaly_fc_vsag_v: f64,

    /// Fuel cell minimum voltage threshold in Volts to distinguish sag from disconnect.
    pub anomaly_fc_min_v: f64,

    /// Battery temperature threshold in degrees Celsius triggering an overtemp warning.
    pub anomaly_batt_overtemp_c: f64,

    /// Maximum number of formatted anomaly alerts retained in the UI side panel queue.
    pub anomaly_queue_capacity: usize,

    /// Target UI rendering refresh interval in milliseconds (33 ms ≈ 30 FPS).
    pub ui_refresh_interval_ms: u64,

    /// Default time window width in seconds displayed on real-time history charts.
    pub default_chart_window_s: f64,

    /// Default 3-letter driver identifier code for onboard matrix display.
    pub default_driver_code: String,

    /// Default cooling fan PWM duty cycle percentage (0–100%).
    pub default_fan_duty: i32,

    /// Simulation playback sample interval in milliseconds for Demo Mode (100 ms = 10 Hz).
    pub demo_interval_ms: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            serial_baud_rate: 115_200,
            default_port: "COM3".to_string(),
            buffer_capacity: 1200,
            sma_window: 10,
            anomaly_batt_overcurrent_a: 15.0,
            anomaly_fc_vsag_v: 9.0,
            anomaly_fc_min_v: 2.0,
            anomaly_batt_overtemp_c: 45.0,
            anomaly_queue_capacity: 10,
            ui_refresh_interval_ms: 33,
            default_chart_window_s: 30.0,
            default_driver_code: "SKL".to_string(),
            default_fan_duty: 70,
            demo_interval_ms: 100,
        }
    }
}

impl AppConfig {
    /// Loads configuration from a TOML file, or creates it with defaults if missing.
    ///
    /// If the file at `path` cannot be parsed due to a syntax or format error,
    /// a warning is logged to `eprintln!` and the built-in defaults are returned.
    pub fn load_or_default(path: &str) -> Self {
        match fs::read_to_string(path) {
            Ok(content) => match toml::from_str::<AppConfig>(&content) {
                Ok(cfg) => {
                    println!("[config] Loaded configuration from '{}'", path);
                    cfg
                }
                Err(err) => {
                    eprintln!(
                        "[config] Failed to parse '{}' ({}), falling back to defaults",
                        path, err
                    );
                    Self::default()
                }
            },
            Err(_) => {
                let default_cfg = Self::default();
                if let Ok(serialized) = toml::to_string_pretty(&default_cfg) {
                    if let Err(err) = fs::write(path, serialized) {
                        eprintln!("[config] Could not write default config to '{}': {}", path, err);
                    } else {
                        println!("[config] Created default configuration file at '{}'", path);
                    }
                }
                default_cfg
            }
        }
    }

    /// Serializes and saves the configuration to the specified TOML file path.
    pub fn save(&self, path: &str) -> Result<(), std::io::Error> {
        let serialized = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(path, serialized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.serial_baud_rate, 115_200);
        assert_eq!(cfg.buffer_capacity, 1200);
        assert_eq!(cfg.sma_window, 10);
        assert_eq!(cfg.anomaly_batt_overcurrent_a, 15.0);
        assert_eq!(cfg.anomaly_fc_vsag_v, 9.0);
        assert_eq!(cfg.anomaly_fc_min_v, 2.0);
        assert_eq!(cfg.anomaly_batt_overtemp_c, 45.0);
        assert_eq!(cfg.anomaly_queue_capacity, 10);
        assert_eq!(cfg.ui_refresh_interval_ms, 33);
        assert_eq!(cfg.default_chart_window_s, 30.0);
        assert_eq!(cfg.demo_interval_ms, 100);
    }

    #[test]
    fn toml_roundtrip_serialization() {
        let original = AppConfig::default();
        let serialized = toml::to_string_pretty(&original).expect("TOML serialization should succeed");
        let deserialized: AppConfig = toml::from_str(&serialized).expect("TOML deserialization should succeed");
        assert_eq!(original, deserialized);
    }

    #[test]
    fn load_malformed_toml_falls_back_to_default() {
        let malformed = "buffer_capacity = 'not a number'";
        let parsed = toml::from_str::<AppConfig>(malformed);
        assert!(parsed.is_err());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let test_path = "test_config_roundtrip.toml";
        let mut cfg = AppConfig::default();
        cfg.serial_baud_rate = 230_400;
        cfg.default_port = "/dev/ttyUSB1".to_string();

        assert!(cfg.save(test_path).is_ok());
        let loaded = AppConfig::load_or_default(test_path);
        assert_eq!(loaded.serial_baud_rate, 230_400);
        assert_eq!(loaded.default_port, "/dev/ttyUSB1");

        let _ = fs::remove_file(test_path);
    }
}

