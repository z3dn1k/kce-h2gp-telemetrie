#![allow(dead_code)]

pub const TELEMETRY_MAGIC: u16 = 0x3248; // "H2" little-endian
pub const TELEMETRY_PACKET_SIZE: usize = 88;
pub const TELEMETRY_KIND_AUX: u8 = 42;
pub const TELEMETRY_AUX_PACKET_SIZE: usize = 22;

#[derive(Default, Debug, Clone, Copy)]
pub struct ChannelData {
    pub v: f64,
    pub i: f64,
    pub p: f64,
    pub e: f64,
    pub c: f64,
    pub t: f64,
    pub ah: f64,
    pub shunt_mv: f64,
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
    pub timestamp: String,
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

fn get_u32_le(input: &[u8]) -> u32 {
    u32::from_le_bytes([input[0], input[1], input[2], input[3]])
}

fn get_u40_le(input: &[u8]) -> u64 {
    ((input[4] as u64) << 32)
        | ((input[3] as u64) << 24)
        | ((input[2] as u64) << 16)
        | ((input[1] as u64) << 8)
        | (input[0] as u64)
}

fn sign_extend_20(value: u32) -> i32 {
    let mask = (1u32 << 20) - 1;
    let sign = 1u32 << 19;
    let masked = value & mask;
    if (masked & sign) != 0 {
        (masked | !mask) as i32
    } else {
        masked as i32
    }
}

fn sign_extend_40(value: u64) -> i64 {
    let sign_bit = 1u64 << 39;
    let value_mask = (1u64 << 40) - 1;
    let masked = value & value_mask;
    if (masked & sign_bit) != 0 {
        (masked | !value_mask) as i64
    } else {
        masked as i64
    }
}

pub fn decode_ina_channel(input: &[u8]) -> ChannelData {
    let shunt_raw = sign_extend_20(get_u32_le(&input[0..4]) >> 4);
    let bus_voltage_raw = get_u32_le(&input[4..8]);
    let die_temp_raw = i16::from_le_bytes([input[8], input[9]]);
    let current_raw = sign_extend_20(get_u32_le(&input[10..14]) >> 4);
    let power_raw = get_u32_le(&input[14..18]);
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
        i,
        p,
        e,
        c,
        t,
        ah,
        shunt_mv,
    }
}

pub fn decode_rev3_aux(input: &[u8]) -> Rev3AuxData {
    if input.len() < 14 {
        return Rev3AuxData::default();
    }

    let sensor_count = input[0];
    let mut temps = [0.0; 4];
    for i in 0..4 {
        let raw = i16::from_le_bytes([input[1 + i * 2], input[2 + i * 2]]);
        temps[i] = if raw == i16::MIN { -127.0 } else { (raw as f64) / 16.0 };
    }

    let max_temp_raw = i16::from_le_bytes([input[9], input[10]]);
    let max_temperature_c = if max_temp_raw == i16::MIN { -127.0 } else { (max_temp_raw as f64) / 16.0 };
    let fan_duty_percent = input[11];
    let flags = u16::from_le_bytes([input[12], input[13]]);
    let fan_mode = if input.len() > 14 { input[14] } else { 0 };

    Rev3AuxData {
        valid: true,
        sensor_count,
        temperature_c: temps,
        max_temperature_c,
        fan_duty_percent,
        fan_control_temperature_c: 0.0,
        flags,
        fan_mode,
    }
}