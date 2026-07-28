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
}