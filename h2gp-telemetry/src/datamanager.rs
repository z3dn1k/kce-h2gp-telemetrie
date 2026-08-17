#![allow(dead_code)]

use crate::protocol::TelemetrySample;

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

    pub fn add_data(&mut self, sample: TelemetrySample) {
        let elapsed_seconds = sample.timestamp_ms as f64 / 1000.0;
        
        // FIX: Detect backward time jumps (e.g., Demo Mode restarting or MCU hard reset)
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
        if self.history.is_empty() || window == 0 { return 0.0; }
        let len = self.history.len();
        let start = if len > window { len - window } else { 0 };
        let slice = &self.history[start..];
        let sum: f64 = slice.iter().map(|s| s.batt.v).sum();
        sum / (slice.len() as f64)
    }

    pub fn compute_fc_voltage_avg(&self, window: usize) -> f64 {
        if self.history.is_empty() || window == 0 { return 0.0; }
        let len = self.history.len();
        let start = if len > window { len - window } else { 0 };
        let slice = &self.history[start..];
        let sum: f64 = slice.iter().map(|s| s.fc.v).sum();
        sum / (slice.len() as f64)
    }

    pub fn compute_batt_current_avg(&self, window: usize) -> f64 {
        if self.history.is_empty() || window == 0 { return 0.0; }
        let len = self.history.len();
        let start = if len > window { len - window } else { 0 };
        let slice = &self.history[start..];
        let sum: f64 = slice.iter().map(|s| s.batt.i).sum();
        sum / (slice.len() as f64)
    }

    pub fn compute_fc_current_avg(&self, window: usize) -> f64 {
        if self.history.is_empty() || window == 0 { return 0.0; }
        let len = self.history.len();
        let start = if len > window { len - window } else { 0 };
        let slice = &self.history[start..];
        let sum: f64 = slice.iter().map(|s| s.fc.i).sum();
        sum / (slice.len() as f64)
    }

    pub fn batt_v_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        self.batt_v_mm
    }

    pub fn batt_i_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        self.batt_i_mm
    }

    pub fn batt_p_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        self.batt_p_mm
    }

    pub fn fc_v_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        self.fc_v_mm
    }

    pub fn fc_i_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        self.fc_i_mm
    }

    pub fn fc_p_min_max(&self) -> (f64, f64) {
        if self.history.is_empty() { return (0.0, 0.0); }
        self.fc_p_mm
    }
}