//! # Binary Telemetry Protocol & INA228 Sensor Decoder
//!
//! Provides parsing and conversion routines for raw binary telemetry packets transmitted
//! by the vehicle's onboard microcontroller over USB serial.
//!
//! ## INA228 Decoding Details
//! Each electrical power channel (Battery and Fuel Cell) transmits a 28-byte raw register block.
//! Quantities are converted into physical units using calibrated LSB scaling factors:
//! - Bus Voltage ($V_{bus}$): $LSB = 0.0001953125\text{ V}$
//! - Shunt Voltage ($V_{shunt}$): $LSB = 0.0003125\text{ mV}$
//! - Current ($I$): $LSB = \frac{32}{524288}\text{ A}$ (20-bit signed integer)
//! - Die Temperature ($T_{die}$): $LSB = \frac{1}{128}\text{ }^\circ\text{C}$
//! - Power ($P$): $LSB = 3.2 \times I_{LSB}\text{ W}$
//! - Energy ($E$): $LSB = 16.0 \times P_{LSB}\text{ J}$ (40-bit unsigned integer)
//! - Charge ($Q$ / $\text{Ah}$): Derived from 40-bit signed charge register converted to Ampere-hours.

/// Sync magic for the legacy "H2" frame format (little-endian 0x3248).
/// Retained for protocol documentation; the active parser uses `USB_FRAME_MAGIC` in serial.rs.
#[allow(dead_code)]
pub const TELEMETRY_MAGIC: u16 = 0x3248;

/// Expected payload size in bytes for the main telemetry channel packet.
pub const TELEMETRY_PACKET_SIZE: usize = 88;

/// Protocol packet kind identifier for auxiliary sensor messages.
pub const TELEMETRY_KIND_AUX: u8 = 42;

/// Expected aux payload size per protocol spec. Kept as a reference constant;
/// the decoder uses a minimum-length check instead.
#[allow(dead_code)]
pub const TELEMETRY_AUX_PACKET_SIZE: usize = 22;

/// Decoded electrical measurements from a single INA228 power monitor channel.
#[derive(Default, Debug, Clone, Copy)]
pub struct ChannelData {
    /// Bus voltage in Volts (V).
    pub v: f64,
    /// Shunt resistor differential voltage in millivolts (mV).
    pub sv_mv: f64,
    /// Current flow in Amperes (A). Positive indicates discharge; negative indicates charging.
    pub i: f64,
    /// Instantaneous electrical power in Watts (W).
    pub p: f64,
    /// Accumulated electrical energy in Joules (J).
    pub e: f64,
    /// Accumulated electric charge in Ampere-hours (Ah).
    pub ah: f64, 
    /// Internal INA228 die temperature in degrees Celsius (°C).
    pub t: f64,
}

/// Decoded auxiliary sensor and control state from the vehicle power distribution board.
#[derive(Default, Debug, Clone, Copy)]
pub struct Rev3AuxData {
    /// Indicates whether the auxiliary packet was successfully parsed and valid.
    pub valid: bool,
    /// Number of active external temperature probes connected to the MCU.
    pub sensor_count: u8,
    /// Temperature readings from up to 4 external temperature sensors in degrees Celsius (°C).
    pub temperature_c: [f64; 4],
    /// Maximum temperature recorded across all active external probes in degrees Celsius (°C).
    pub max_temperature_c: f64,
    /// Current cooling fan duty cycle percentage (0–100%).
    pub fan_duty_percent: u8,
    /// Target control temperature currently used by the onboard fan controller.
    pub fan_control_temperature_c: f64,
    /// Status bitflags: Bit 0 indicates commanded FC shorting; Bit 1 indicates hydrogen purge active.
    pub flags: u16,
    /// Onboard fan operational mode identifier (0 = Auto, 1 = Manual, 2 = Off).
    pub fan_mode: u8,
}

/// A unified telemetry sample containing synchronized electrical and status measurements.
#[derive(Default, Debug, Clone)]
pub struct TelemetrySample {
    /// Timestamp in milliseconds elapsed since telemetry stream inception.
    pub timestamp_ms: u32,
    /// Decoded measurements for the battery / capacitor bank channel.
    pub batt: ChannelData,
    /// Decoded measurements for the hydrogen fuel cell channel.
    pub fc: ChannelData,
    /// Decoded auxiliary sensor and vehicle status state.
    pub rev3_aux: Rev3AuxData,
    /// Flag indicating whether this sample contains valid main power channel data.
    pub has_channel_data: bool,
    /// Flag indicating whether this sample contains valid auxiliary sensor data.
    pub has_rev3_aux: bool,
}

// Scaling constants matching firmware / C++ specs
const INA_CURRENT_LSB_A: f64 = 32.0 / 524288.0;
const INA_SHUNT_MV_LSB: f64 = 0.0003125;
const INA_BUS_VOLT_LSB: f64 = 0.0001953125;
const INA_TEMP_LSB_C: f64 = 1.0 / 128.0;
const INA_POWER_LSB_W: f64 = 3.2 * INA_CURRENT_LSB_A;
const INA_ENERGY_LSB_J: f64 = 16.0 * INA_POWER_LSB_W;

#[inline(always)]
fn get_u40_le(input: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    if input.len() >= 5 {
        buf[..5].copy_from_slice(&input[0..5]);
    }
    u64::from_le_bytes(buf)
}

#[inline(always)]
fn sign_extend_20(value: u32) -> i32 {
    ((value << 12) as i32) >> 12
}

#[inline(always)]
fn sign_extend_40(value: u64) -> i64 {
    ((value << 24) as i64) >> 24
}

/// Decodes a 28-byte binary payload into floating point telemetry data.
/// Uses safe slice extraction to guarantee panic-free execution even if 
/// the serial buffer provides malformed or truncated data.
pub fn decode_ina_channel(input: &[u8]) -> ChannelData {
    if input.len() < 28 {
        return ChannelData::default();
    }

    // Safe slice conversions replacing all .unwrap() calls
    let shunt_raw = sign_extend_20(u32::from_le_bytes(input[0..4].try_into().unwrap_or([0; 4])) >> 4);
    let bus_voltage_raw = u32::from_le_bytes(input[4..8].try_into().unwrap_or([0; 4]));
    let die_temp_raw = i16::from_le_bytes(input[8..10].try_into().unwrap_or([0; 2]));
    let current_raw = sign_extend_20(u32::from_le_bytes(input[10..14].try_into().unwrap_or([0; 4])) >> 4);
    let power_raw = u32::from_le_bytes(input[14..18].try_into().unwrap_or([0; 4]));
    let energy_raw = get_u40_le(&input[18..23]);
    let charge_raw = sign_extend_40(get_u40_le(&input[23..28]));

    let v = (bus_voltage_raw as f64) * INA_BUS_VOLT_LSB;
    let i = (current_raw as f64) * INA_CURRENT_LSB_A;
    let p = (power_raw as f64) * INA_POWER_LSB_W;
    let t = (die_temp_raw as f64) * INA_TEMP_LSB_C;
    let e = (energy_raw as f64) * INA_ENERGY_LSB_J;
    let c = (charge_raw as f64) * INA_CURRENT_LSB_A;
    let ah = c / 3600.0;
    let shunt_mv = (shunt_raw as f64) * INA_SHUNT_MV_LSB * 1000.0;

    ChannelData {
        v,
        sv_mv: shunt_mv,
        i,
        p,
        e,
        ah,
        t,
    }
}

/// Decodes auxiliary sensor data from a variable-length payload.
/// Parses environmental temperature probes and state flags safely.
pub fn decode_rev3_aux(input: &[u8]) -> Rev3AuxData {
    if input.len() < 14 {
        return Rev3AuxData::default();
    }

    let sensor_count = input[0];
    
    let temperature_c = std::array::from_fn(|i| {
        let offset = 1 + i * 2;
        if offset + 2 <= input.len() {
            let raw = i16::from_le_bytes(input[offset..offset+2].try_into().unwrap_or([0; 2]));
            if raw == i16::MIN { -127.0 } else { (raw as f64) / 16.0 }
        } else {
            -127.0
        }
    });

    let max_temp_raw = i16::from_le_bytes(input[9..11].try_into().unwrap_or([0; 2]));
    let max_temperature_c = if max_temp_raw == i16::MIN { -127.0 } else { (max_temp_raw as f64) / 16.0 };
    
    let fan_duty_percent = input[11];
    let flags = u16::from_le_bytes(input[12..14].try_into().unwrap_or([0; 2]));
    let fan_mode = if input.len() > 14 { input[14] } else { 0 };

    Rev3AuxData {
        valid: true,
        sensor_count,
        temperature_c,
        max_temperature_c,
        fan_duty_percent,
        fan_control_temperature_c: 0.0,
        flags,
        fan_mode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── sign_extend_20 ──────────────────────────────────────────────

    #[test]
    fn sign_extend_20_preserves_positive() {
        // 0x7FFFF = 524287, the maximum positive 20-bit value
        assert_eq!(sign_extend_20(0x7FFFF), 524287);
    }

    #[test]
    fn sign_extend_20_extends_negative() {
        // 0xFFFFF is -1 in 20-bit two's complement
        assert_eq!(sign_extend_20(0xFFFFF), -1);
    }

    #[test]
    fn sign_extend_20_minimum_negative() {
        // 0x80000 is the most negative 20-bit value (-524288)
        assert_eq!(sign_extend_20(0x80000), -524288);
    }

    #[test]
    fn sign_extend_20_zero() {
        assert_eq!(sign_extend_20(0), 0);
    }

    // ── sign_extend_40 ──────────────────────────────────────────────

    #[test]
    fn sign_extend_40_preserves_positive() {
        assert_eq!(sign_extend_40(0x7F_FFFF_FFFF), 0x7F_FFFF_FFFF);
    }

    #[test]
    fn sign_extend_40_extends_negative() {
        // 40-bit all-ones = -1
        assert_eq!(sign_extend_40(0xFF_FFFF_FFFF), -1);
    }

    #[test]
    fn sign_extend_40_zero() {
        assert_eq!(sign_extend_40(0), 0);
    }

    // ── get_u40_le ──────────────────────────────────────────────────

    #[test]
    fn get_u40_le_reads_five_bytes() {
        let bytes = [0x01, 0x02, 0x03, 0x04, 0x05];
        let result = get_u40_le(&bytes);
        let expected = u64::from_le_bytes([0x01, 0x02, 0x03, 0x04, 0x05, 0, 0, 0]);
        assert_eq!(result, expected);
    }

    #[test]
    fn get_u40_le_short_input_returns_zero() {
        let bytes = [0xFF, 0xFF];
        let result = get_u40_le(&bytes);
        assert_eq!(result, 0);
    }

    // ── decode_ina_channel ──────────────────────────────────────────

    #[test]
    fn decode_channel_returns_default_on_short_input() {
        let result = decode_ina_channel(&[0u8; 10]);
        assert_eq!(result.v, 0.0);
        assert_eq!(result.i, 0.0);
        assert_eq!(result.p, 0.0);
        assert_eq!(result.t, 0.0);
    }

    #[test]
    fn decode_channel_returns_default_on_empty_input() {
        let result = decode_ina_channel(&[]);
        assert_eq!(result.v, 0.0);
    }

    #[test]
    fn decode_channel_zeroed_input_produces_zeroed_output() {
        let result = decode_ina_channel(&[0u8; 28]);
        assert_eq!(result.v, 0.0);
        assert_eq!(result.i, 0.0);
        assert_eq!(result.p, 0.0);
        assert_eq!(result.e, 0.0);
        assert_eq!(result.ah, 0.0);
        assert_eq!(result.t, 0.0);
        assert_eq!(result.sv_mv, 0.0);
    }

    #[test]
    fn decode_channel_known_bus_voltage() {
        // Place a known value in the bus voltage field (bytes 4..8).
        // bus_voltage_raw = 1 => v = 1 * INA_BUS_VOLT_LSB = 0.0001953125 V
        let mut input = [0u8; 28];
        input[4] = 1; // u32 LE = 1
        let result = decode_ina_channel(&input);
        assert!((result.v - INA_BUS_VOLT_LSB).abs() < 1e-12);
    }

    #[test]
    fn decode_channel_known_temperature() {
        // Place a known value in the die temp field (bytes 8..10).
        // die_temp_raw = 128 (i16 LE) => t = 128 / 128 = 1.0 °C
        let mut input = [0u8; 28];
        input[8] = 128; // i16 LE: 128
        input[9] = 0;
        let result = decode_ina_channel(&input);
        assert!((result.t - 1.0).abs() < 1e-12);
    }

    // ── decode_rev3_aux ─────────────────────────────────────────────

    #[test]
    fn decode_aux_returns_default_on_short_input() {
        let result = decode_rev3_aux(&[0u8; 5]);
        assert!(!result.valid);
        assert_eq!(result.sensor_count, 0);
    }

    #[test]
    fn decode_aux_parses_sensor_count() {
        let mut input = [0u8; 15];
        input[0] = 4; // sensor_count = 4
        let result = decode_rev3_aux(&input);
        assert!(result.valid);
        assert_eq!(result.sensor_count, 4);
    }

    #[test]
    fn decode_aux_parses_fan_duty() {
        let mut input = [0u8; 15];
        input[11] = 75; // fan_duty_percent = 75%
        let result = decode_rev3_aux(&input);
        assert_eq!(result.fan_duty_percent, 75);
    }

    #[test]
    fn decode_aux_parses_flags() {
        let mut input = [0u8; 15];
        input[12] = 0x03; // flags = 3 (bit 0 = SHORT, bit 1 = PURGE)
        input[13] = 0x00;
        let result = decode_rev3_aux(&input);
        assert_eq!(result.flags, 3);
    }

    #[test]
    fn decode_aux_parses_fan_mode_when_present() {
        let mut input = [0u8; 15];
        input[14] = 2; // fan_mode = 2
        let result = decode_rev3_aux(&input);
        assert_eq!(result.fan_mode, 2);
    }

    #[test]
    fn decode_aux_fan_mode_defaults_when_payload_short() {
        // Exactly 14 bytes — fan_mode field is absent
        let input = [0u8; 14];
        let result = decode_rev3_aux(&input);
        assert_eq!(result.fan_mode, 0);
    }

    #[test]
    fn decode_aux_sentinel_temperature() {
        // i16::MIN (0x8000 LE = [0x00, 0x80]) should map to -127.0
        let mut input = [0u8; 15];
        input[1] = 0x00;
        input[2] = 0x80; // sensor 0 = i16::MIN
        let result = decode_rev3_aux(&input);
        assert_eq!(result.temperature_c[0], -127.0);
    }

    #[test]
    fn decode_aux_positive_temperature() {
        // 25.0 °C * 16 = 400 = 0x0190 LE = [0x90, 0x01]
        let mut input = [0u8; 15];
        input[1] = 0x90;
        input[2] = 0x01;
        let result = decode_rev3_aux(&input);
        assert!((result.temperature_c[0] - 25.0).abs() < 1e-6);
    }
}