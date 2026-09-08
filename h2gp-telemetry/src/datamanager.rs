//! # Historical Data Management & Statistical Analysis
//!
//! Manages in-memory circular telemetry history, dynamic min/max tracking,
//! backward time jump protection (preventing distorted charts across MCU resets),
//! Simple Moving Averages (SMA) for noise attenuation, and post-race summary exports.

use std::fs::File;
use std::io::Write;
use crate::protocol::TelemetrySample;
use crate::error::TelemetryError;

/// In-memory manager for historical telemetry samples.
///
/// Implements a fixed-capacity circular buffer using batch drains for allocation efficiency,
/// caches running extrema (min/max) for both power channels, and provides moving averages.
pub struct DataManager {
    max_history: usize,
    batch_size: usize,
    history: Vec<TelemetrySample>,
    time_labels: Vec<f64>,

    // Min/Max Caches: (Min, Max)
    batt_v_mm: (f64, f64),
    batt_i_mm: (f64, f64),
    batt_p_mm: (f64, f64),
    fc_v_mm: (f64, f64),
    fc_i_mm: (f64, f64),
    fc_p_mm: (f64, f64),
}

impl DataManager {
    /// Generates and exports a post-race summary report to `summary.txt`.
    ///
    /// Computes total energy usage, peak currents, maximum temperatures,
    /// and total packet count from recorded history.
    pub fn export_summary(&self) -> Result<(), TelemetryError> {
        let mut file = File::create("summary.txt")?;
        
        // Safely extract the last sample without using .unwrap()
        if let Some(last_sample) = self.history.last() {
            let mut max_batt_t = -100.0f64;
            let mut max_fc_t = -100.0f64;

            for s in &self.history {
                if s.batt.t > max_batt_t { max_batt_t = s.batt.t; }
                if s.fc.t > max_fc_t { max_fc_t = s.fc.t; }
            }

            writeln!(file, "========================================")?;
            writeln!(file, "       H2GP POST-RACE ANALYTICS         ")?;
            writeln!(file, "========================================")?;
            writeln!(file, "Total Battery Energy Used : {:.2} J", last_sample.batt.e)?;
            writeln!(file, "Total Fuel Cell Energy    : {:.2} J", last_sample.fc.e)?;
            writeln!(file, "Peak Battery Current      : {:.2} A", self.batt_i_mm.1)?;
            writeln!(file, "Peak Fuel Cell Current    : {:.2} A", self.fc_i_mm.1)?;
            writeln!(file, "Max Battery Temp          : {:.1} °C", max_batt_t)?;
            writeln!(file, "Max Fuel Cell Temp        : {:.1} °C", max_fc_t)?;
            writeln!(file, "Total Packets Received    : {}", self.history.len())?;
            writeln!(file, "========================================")?;
        } else {
            writeln!(file, "No telemetry data available for summary.")?;
        }

        Ok(())
    }

    /// Creates a new `DataManager` with a specified maximum sample capacity.
    ///
    /// The buffer uses a batch size of `clamp(max_history / 10, 10, 500)` to amortize
    /// reallocation costs during circular draining.
    pub fn new(max_history: usize) -> Self {
        let batch_size = (max_history / 10).clamp(10, 500); 
        
        let mut dm = Self {
            max_history,
            batch_size,
            history: Vec::with_capacity(max_history + batch_size),
            time_labels: Vec::with_capacity(max_history + batch_size),
            batt_v_mm: (f64::INFINITY, f64::NEG_INFINITY),
            batt_i_mm: (f64::INFINITY, f64::NEG_INFINITY),
            batt_p_mm: (f64::INFINITY, f64::NEG_INFINITY),
            fc_v_mm: (f64::INFINITY, f64::NEG_INFINITY),
            fc_i_mm: (f64::INFINITY, f64::NEG_INFINITY),
            fc_p_mm: (f64::INFINITY, f64::NEG_INFINITY),
        };
        dm.reset_min_max();
        dm
    }

    /// Clears all recorded samples, time labels, and resets min/max statistics.
    pub fn clear(&mut self) {
        self.history.clear();
        self.time_labels.clear();
        self.reset_min_max();
    }

    fn reset_min_max(&mut self) {
        self.batt_v_mm = (f64::INFINITY, f64::NEG_INFINITY);
        self.batt_i_mm = (f64::INFINITY, f64::NEG_INFINITY);
        self.batt_p_mm = (f64::INFINITY, f64::NEG_INFINITY);
        self.fc_v_mm = (f64::INFINITY, f64::NEG_INFINITY);
        self.fc_i_mm = (f64::INFINITY, f64::NEG_INFINITY);
        self.fc_p_mm = (f64::INFINITY, f64::NEG_INFINITY);
    }

    /// Appends a new sample to the circular buffer.
    ///
    /// Automatically detects backward time jumps (e.g. from an MCU reboot or looped playback)
    /// and flushes the buffer to prevent chart plotting artifacts ("Z-folding").
    /// Running min/max values are updated in O(1) time.
    pub fn add_data(&mut self, sample: TelemetrySample) {
        let elapsed_seconds = sample.timestamp_ms as f64 / 1000.0;
        
        // Detect backward time jumps (e.g., Demo Mode restarting or MCU hard reset)
        // This prevents the Z-fold graph spaghetti.
        if let Some(&last_time) = self.time_labels.last() {
            if elapsed_seconds < last_time {
                self.clear();
            }
        }
        
        self.batt_v_mm.0 = self.batt_v_mm.0.min(sample.batt.v);
        self.batt_v_mm.1 = self.batt_v_mm.1.max(sample.batt.v);
        self.batt_i_mm.0 = self.batt_i_mm.0.min(sample.batt.i);
        self.batt_i_mm.1 = self.batt_i_mm.1.max(sample.batt.i);
        self.batt_p_mm.0 = self.batt_p_mm.0.min(sample.batt.p);
        self.batt_p_mm.1 = self.batt_p_mm.1.max(sample.batt.p);
        
        self.fc_v_mm.0 = self.fc_v_mm.0.min(sample.fc.v);
        self.fc_v_mm.1 = self.fc_v_mm.1.max(sample.fc.v);
        self.fc_i_mm.0 = self.fc_i_mm.0.min(sample.fc.i);
        self.fc_i_mm.1 = self.fc_i_mm.1.max(sample.fc.i);
        self.fc_p_mm.0 = self.fc_p_mm.0.min(sample.fc.p);
        self.fc_p_mm.1 = self.fc_p_mm.1.max(sample.fc.p);

        self.history.push(sample);
        self.time_labels.push(elapsed_seconds);

        if self.history.len() >= self.max_history + self.batch_size {
            let excess = self.history.len() - self.max_history;
            self.history.drain(0..excess);
            self.time_labels.drain(0..excess);
            self.recompute_min_max_full();
        }
    }

    fn recompute_min_max_full(&mut self) {
        if self.history.is_empty() {
            self.reset_min_max();
            return;
        }

        let mut bv_min = f64::INFINITY; let mut bv_max = f64::NEG_INFINITY;
        let mut bi_min = f64::INFINITY; let mut bi_max = f64::NEG_INFINITY;
        let mut bp_min = f64::INFINITY; let mut bp_max = f64::NEG_INFINITY;
        let mut fv_min = f64::INFINITY; let mut fv_max = f64::NEG_INFINITY;
        let mut fi_min = f64::INFINITY; let mut fi_max = f64::NEG_INFINITY;
        let mut fp_min = f64::INFINITY; let mut fp_max = f64::NEG_INFINITY;

        for s in &self.history {
            bv_min = bv_min.min(s.batt.v); bv_max = bv_max.max(s.batt.v);
            bi_min = bi_min.min(s.batt.i); bi_max = bi_max.max(s.batt.i);
            bp_min = bp_min.min(s.batt.p); bp_max = bp_max.max(s.batt.p);
            
            fv_min = fv_min.min(s.fc.v); fv_max = fv_max.max(s.fc.v);
            fi_min = fi_min.min(s.fc.i); fi_max = fi_max.max(s.fc.i);
            fp_min = fp_min.min(s.fc.p); fp_max = fp_max.max(s.fc.p);
        }

        self.batt_v_mm = (bv_min, bv_max);
        self.batt_i_mm = (bi_min, bi_max);
        self.batt_p_mm = (bp_min, bp_max);
        self.fc_v_mm = (fv_min, fv_max);
        self.fc_i_mm = (fi_min, fi_max);
        self.fc_p_mm = (fp_min, fp_max);
    }

    /// Returns a slice of all recorded historical telemetry samples currently in the buffer.
    pub fn history(&self) -> &[TelemetrySample] {
        &self.history
    }

    /// Returns a slice of elapsed time offsets in seconds corresponding to each sample in `history()`.
    pub fn time_labels(&self) -> &[f64] {
        &self.time_labels
    }

    /// Returns `true` if no telemetry samples have been received or the buffer was cleared.
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    /// Computes the Simple Moving Average (SMA) of battery voltage over the last `window` samples.
    pub fn compute_batt_voltage_avg(&self, window: usize) -> f64 {
        if self.history.is_empty() || window == 0 { return 0.0; }
        let len = self.history.len();
        let start = len.saturating_sub(window);
        let slice = &self.history[start..];
        let sum: f64 = slice.iter().map(|s| s.batt.v).sum();
        sum / (slice.len() as f64)
    }

    /// Computes the Simple Moving Average (SMA) of fuel cell voltage over the last `window` samples.
    pub fn compute_fc_voltage_avg(&self, window: usize) -> f64 {
        if self.history.is_empty() || window == 0 { return 0.0; }
        let len = self.history.len();
        let start = len.saturating_sub(window);
        let slice = &self.history[start..];
        let sum: f64 = slice.iter().map(|s| s.fc.v).sum();
        sum / (slice.len() as f64)
    }

    /// Computes the Simple Moving Average (SMA) of battery current over the last `window` samples.
    pub fn compute_batt_current_avg(&self, window: usize) -> f64 {
        if self.history.is_empty() || window == 0 { return 0.0; }
        let len = self.history.len();
        let start = len.saturating_sub(window);
        let slice = &self.history[start..];
        let sum: f64 = slice.iter().map(|s| s.batt.i).sum();
        sum / (slice.len() as f64)
    }

    /// Computes the Simple Moving Average (SMA) of fuel cell current over the last `window` samples.
    pub fn compute_fc_current_avg(&self, window: usize) -> f64 {
        if self.history.is_empty() || window == 0 { return 0.0; }
        let len = self.history.len();
        let start = len.saturating_sub(window);
        let slice = &self.history[start..];
        let sum: f64 = slice.iter().map(|s| s.fc.i).sum();
        sum / (slice.len() as f64)
    }

    /// Returns cached `(min, max)` battery voltage in Volts across active buffer history.
    pub fn batt_v_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        self.batt_v_mm
    }

    /// Returns cached `(min, max)` battery current in Amperes across active buffer history.
    pub fn batt_i_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        self.batt_i_mm
    }

    /// Returns cached `(min, max)` battery power in Watts across active buffer history.
    pub fn batt_p_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        self.batt_p_mm
    }

    /// Returns cached `(min, max)` fuel cell voltage in Volts across active buffer history.
    pub fn fc_v_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        self.fc_v_mm
    }

    /// Returns cached `(min, max)` fuel cell current in Amperes across active buffer history.
    pub fn fc_i_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        self.fc_i_mm
    }

    /// Returns cached `(min, max)` fuel cell power in Watts across active buffer history.
    pub fn fc_p_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        self.fc_p_mm
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ChannelData;

    fn make_test_sample(timestamp_ms: u32, batt_v: f64, batt_i: f64, fc_v: f64, fc_i: f64) -> TelemetrySample {
        TelemetrySample {
            timestamp_ms,
            batt: ChannelData {
                v: batt_v,
                sv_mv: 0.0,
                i: batt_i,
                p: batt_v * batt_i,
                e: 100.0,
                ah: 1.5,
                t: 25.0,
            },
            fc: ChannelData {
                v: fc_v,
                sv_mv: 0.0,
                i: fc_i,
                p: fc_v * fc_i,
                e: 50.0,
                ah: 0.8,
                t: 35.0,
            },
            rev3_aux: Default::default(),
            has_channel_data: true,
            has_rev3_aux: false,
        }
    }

    #[test]
    fn new_datamanager_is_empty() {
        let dm = DataManager::new(100);
        assert!(dm.is_empty());
        assert_eq!(dm.history().len(), 0);
        assert_eq!(dm.time_labels().len(), 0);
        assert_eq!(dm.batt_v_min_max(), (0.0, 0.0));
        assert_eq!(dm.fc_v_min_max(), (0.0, 0.0));
    }

    #[test]
    fn add_data_tracks_history_and_time() {
        let mut dm = DataManager::new(100);
        dm.add_data(make_test_sample(1000, 12.0, 1.0, 8.0, 2.0));
        dm.add_data(make_test_sample(2000, 12.5, 1.5, 8.5, 2.5));

        assert_eq!(dm.history().len(), 2);
        assert_eq!(dm.time_labels(), &[1.0, 2.0]);
        assert!(!dm.is_empty());
    }

    #[test]
    fn min_max_tracking() {
        let mut dm = DataManager::new(100);
        dm.add_data(make_test_sample(1000, 12.0, 2.0, 10.0, 1.0));
        dm.add_data(make_test_sample(2000, 10.0, 5.0, 8.0, 3.0));
        dm.add_data(make_test_sample(3000, 14.0, 1.0, 11.0, 0.5));

        assert_eq!(dm.batt_v_min_max(), (10.0, 14.0));
        assert_eq!(dm.batt_i_min_max(), (1.0, 5.0));
        assert_eq!(dm.fc_v_min_max(), (8.0, 11.0));
        assert_eq!(dm.fc_i_min_max(), (0.5, 3.0));
    }

    #[test]
    fn backward_time_jump_clears_buffer() {
        let mut dm = DataManager::new(100);
        dm.add_data(make_test_sample(1000, 12.0, 1.0, 8.0, 2.0));
        dm.add_data(make_test_sample(2000, 12.2, 1.2, 8.2, 2.2));
        assert_eq!(dm.history().len(), 2);

        // Backward jump (e.g. MCU reset)
        dm.add_data(make_test_sample(500, 11.0, 0.5, 7.5, 1.0));

        // Should have reset and kept only the new sample
        assert_eq!(dm.history().len(), 1);
        assert_eq!(dm.time_labels(), &[0.5]);
        assert_eq!(dm.batt_v_min_max(), (11.0, 11.0));
    }

    #[test]
    fn sma_calculations() {
        let mut dm = DataManager::new(100);
        assert_eq!(dm.compute_batt_voltage_avg(10), 0.0);
        assert_eq!(dm.compute_batt_voltage_avg(0), 0.0);

        dm.add_data(make_test_sample(1000, 10.0, 1.0, 6.0, 2.0));
        dm.add_data(make_test_sample(2000, 20.0, 3.0, 8.0, 4.0));
        dm.add_data(make_test_sample(3000, 30.0, 5.0, 10.0, 6.0));

        // Full average (window >= len)
        assert!((dm.compute_batt_voltage_avg(5) - 20.0).abs() < 1e-9);
        assert!((dm.compute_batt_current_avg(5) - 3.0).abs() < 1e-9);
        assert!((dm.compute_fc_voltage_avg(5) - 8.0).abs() < 1e-9);
        assert!((dm.compute_fc_current_avg(5) - 4.0).abs() < 1e-9);

        // Window = 2: average of last two samples (20.0 and 30.0 -> 25.0)
        assert!((dm.compute_batt_voltage_avg(2) - 25.0).abs() < 1e-9);
        assert!((dm.compute_batt_current_avg(2) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn circular_buffer_drain_and_recompute() {
        let max_history = 20;
        let mut dm = DataManager::new(max_history);

        // Push enough items to trigger drain (max_history + batch_size)
        // For max_history = 20, batch_size is clamp(20/10, 10, 500) = 10.
        // Total threshold before drain = 20 + 10 = 30.
        for i in 1..=35 {
            dm.add_data(make_test_sample(i * 1000, i as f64, 1.0, 5.0, 1.0));
        }

        // Buffer should have drained excess items and capped to max_history (or close, within batch)
        assert!(dm.history().len() <= max_history + 10);
        // Min should reflect values currently in history, not the drained ones (i=1..15 drained)
        let (min_v, max_v) = dm.batt_v_min_max();
        assert!(min_v > 1.0);
        assert_eq!(max_v, 35.0);
    }

    #[test]
    fn export_summary_empty_and_populated() {
        let dm = DataManager::new(100);
        assert!(dm.export_summary().is_ok());

        let mut populated_dm = DataManager::new(100);
        populated_dm.add_data(make_test_sample(1000, 12.0, 2.0, 9.0, 1.5));
        assert!(populated_dm.export_summary().is_ok());

        let _ = std::fs::remove_file("summary.txt");
    }
}