#![allow(dead_code)]

use std::time::Instant;
use crate::protocol::TelemetrySample;

pub struct DataManager {
    max_history: usize,
    history: Vec<TelemetrySample>,
    time_labels: Vec<f64>,
    start_time: Instant,
}

impl DataManager {
    pub fn new(max_history: usize) -> Self {
        Self {
            max_history,
            history: Vec::with_capacity(max_history),
            time_labels: Vec::with_capacity(max_history),
            start_time: Instant::now(),
        }
    }

    pub fn clear(&mut self) {
        self.history.clear();
        self.time_labels.clear();
        self.start_time = Instant::now();
    }

    pub fn add_data(&mut self, sample: TelemetrySample) {
        let elapsed_seconds = self.start_time.elapsed().as_secs_f64();
        
        self.history.push(sample);
        self.time_labels.push(elapsed_seconds);

        if self.history.len() > self.max_history {
            self.history.remove(0);
            self.time_labels.remove(0);
        }
    }

    pub fn history(&self) -> &[TelemetrySample] {
        &self.history
    }

    pub fn time_labels(&self) -> &[f64] {
        &self.time_labels
    }

    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    pub fn compute_batt_voltage_avg(&self, window: usize) -> f64 {
        if self.history.is_empty() || window == 0 {
            return 0.0;
        }
        let len = self.history.len();
        let start = if len > window { len - window } else { 0 };
        let slice = &self.history[start..];
        
        let sum: f64 = slice.iter().map(|s| s.batt.v).sum();
        sum / (slice.len() as f64)
    }

    pub fn compute_fc_voltage_avg(&self, window: usize) -> f64 {
        if self.history.is_empty() || window == 0 {
            return 0.0;
        }
        let len = self.history.len();
        let start = if len > window { len - window } else { 0 };
        let slice = &self.history[start..];
        
        let sum: f64 = slice.iter().map(|s| s.fc.v).sum();
        sum / (slice.len() as f64)
    }
    pub fn compute_batt_current_avg(&self, window: usize) -> f64 {
        if self.history.is_empty() || window == 0 {
            return 0.0;
        }
        let len = self.history.len();
        let start = if len > window { len - window } else { 0 };
        let slice = &self.history[start..];
        
        let sum: f64 = slice.iter().map(|s| s.batt.i).sum();
        sum / (slice.len() as f64)
    }

    pub fn compute_fc_current_avg(&self, window: usize) -> f64 {
        if self.history.is_empty() || window == 0 {
            return 0.0;
        }
        let len = self.history.len();
        let start = if len > window { len - window } else { 0 };
        let slice = &self.history[start..];
        
        let sum: f64 = slice.iter().map(|s| s.fc.i).sum();
        sum / (slice.len() as f64)
    }

    // --- MIN / MAX TRACKING FOR BATT ---
    pub fn batt_v_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        let min = self.history.iter().fold(f64::INFINITY, |a, s| a.min(s.batt.v));
        let max = self.history.iter().fold(f64::NEG_INFINITY, |a, s| a.max(s.batt.v));
        (min, max)
    }

    pub fn batt_i_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        let min = self.history.iter().fold(f64::INFINITY, |a, s| a.min(s.batt.i));
        let max = self.history.iter().fold(f64::NEG_INFINITY, |a, s| a.max(s.batt.i));
        (min, max)
    }

    pub fn batt_p_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        let min = self.history.iter().fold(f64::INFINITY, |a, s| a.min(s.batt.p));
        let max = self.history.iter().fold(f64::NEG_INFINITY, |a, s| a.max(s.batt.p));
        (min, max)
    }

    // --- MIN / MAX TRACKING FOR FUEL CELL ---
    pub fn fc_v_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        let min = self.history.iter().fold(f64::INFINITY, |a, s| a.min(s.fc.v));
        let max = self.history.iter().fold(f64::NEG_INFINITY, |a, s| a.max(s.fc.v));
        (min, max)
    }

    pub fn fc_i_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        let min = self.history.iter().fold(f64::INFINITY, |a, s| a.min(s.fc.i));
        let max = self.history.iter().fold(f64::NEG_INFINITY, |a, s| a.max(s.fc.i));
        (min, max)
    }

    pub fn fc_p_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        let min = self.history.iter().fold(f64::INFINITY, |a, s| a.min(s.fc.p));
        let max = self.history.iter().fold(f64::NEG_INFINITY, |a, s| a.max(s.fc.p));
        (min, max)
    }

}