#![allow(dead_code)]

pub const TELEMETRY_MAGIC: u16 = 0x3248; // "H2" little-endian
pub const TELEMETRY_PACKET_SIZE: usize = 88;
pub const TELEMETRY_KIND_AUX: u8 = 42;
pub const TELEMETRY_AUX_PACKET_SIZE: usize = 22;

#[derive(Default, Debug, Clone, Copy)]
pub struct ChannelData {
    pub v: f64,
    pub sv_mv: f64,
    pub i: f64,
    pub p: f64,
    pub e: f64,
    pub ah: f64, 
    pub t: f64,  // INA228 internal temperature
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Rev3AuxData {
    pub valid: bool,
    pub sensor_count: u8,
    pub temperature_c: [f64; 4],
    pub max_temperature_c: f64,
    pub fan_duty_percent: u8,
    pub fan_control_temperature_c: f64,
    pub flags: u16,
    pub fan_mode: u8,
}

#[derive(Default, Debug, Clone)]
pub struct TelemetrySample {
    pub timestamp_ms: u32,
    pub batt: ChannelData,
    pub fc: ChannelData,
    pub rev3_aux: Rev3AuxData,
    pub has_channel_data: bool,
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
    // idiomatic conversion: pad to 8 bytes and read natively
    let mut buf = [0u8; 8];
    buf[..5].copy_from_slice(&input[0..5]);
    u64::from_le_bytes(buf)
}

#[inline(always)]
fn sign_extend_20(value: u32) -> i32 {
    // Branchless sign extension. Shift the 20 bits to the top of 
    // the 32-bit register, then arithmetic shift right.
    ((value << 12) as i32) >> 12
}

#[inline(always)]
fn sign_extend_40(value: u64) -> i64 {
    // Branchless sign extension for 40-bit values.
    ((value << 24) as i64) >> 24
}

pub fn decode_ina_channel(input: &[u8]) -> ChannelData {
    // By explicitly asserting the minimum length here once, the compiler 
    // will elide ALL bounds checks for the rest of this function.
    assert!(input.len() >= 28, "INA channel data requires at least 28 bytes");

    let shunt_raw = sign_extend_20(u32::from_le_bytes(input[0..4].try_into().unwrap()) >> 4);
    let bus_voltage_raw = u32::from_le_bytes(input[4..8].try_into().unwrap());
    let die_temp_raw = i16::from_le_bytes(input[8..10].try_into().unwrap());
    let current_raw = sign_extend_20(u32::from_le_bytes(input[10..14].try_into().unwrap()) >> 4);
    let power_raw = u32::from_le_bytes(input[14..18].try_into().unwrap());
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

pub fn decode_rev3_aux(input: &[u8]) -> Rev3AuxData {
    if input.len() < 14 {
        return Rev3AuxData::default();
    }

    let sensor_count = input[0];
    
    // std::array::from_fn constructs the array in place without 
    // double-initializing memory like `[0.0; 4]` does.
    let temperature_c = std::array::from_fn(|i| {
        let offset = 1 + i * 2;
        let raw = i16::from_le_bytes(input[offset..offset+2].try_into().unwrap());
        if raw == i16::MIN { -127.0 } else { (raw as f64) / 16.0 }
    });

    let max_temp_raw = i16::from_le_bytes(input[9..11].try_into().unwrap());
    let max_temperature_c = if max_temp_raw == i16::MIN { -127.0 } else { (max_temp_raw as f64) / 16.0 };
    
    let fan_duty_percent = input[11];
    let flags = u16::from_le_bytes(input[12..14].try_into().unwrap());
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