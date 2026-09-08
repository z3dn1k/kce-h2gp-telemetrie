//! Centralized error types for the H2GP telemetry system.
//!
//! Every error variant carries enough context to diagnose the problem
//! without needing to reproduce it — port names, file paths, expected
//! sizes vs. actual sizes, etc.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error("Serial port '{port}' could not be opened at {baud} baud: {source}")]
    SerialOpen {
        port: String,
        baud: u32,
        source: serialport::Error,
    },

    #[error("Serial port '{port}' could not be cloned for write access: {source}")]
    SerialClone {
        port: String,
        source: serialport::Error,
    },

    #[error("UI channel disconnected — receiver was dropped")]
    ChannelDisconnected,

    #[error("Demo file '{path}' could not be opened: {source}")]
    DemoFileOpen {
        path: String,
        source: std::io::Error,
    },

    #[error("Demo file '{path}' contains no data rows")]
    DemoFileEmpty {
        path: String,
    },

    #[error("Log file '{path}' could not be opened: {source}")]
    LogFileOpen {
        path: String,
        source: std::io::Error,
    },

    #[error("Failed to write to '{path}': {source}")]
    LogWrite {
        path: String,
        source: std::io::Error,
    },

    #[error("Report export failed: {0}")]
    ReportExport(#[from] std::io::Error),
}
