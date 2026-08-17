#include <Arduino.h>
#include <math.h>

// --- Binary Protocol Definitions ---
// These structures match the exact byte offsets defined in protocol.rs and serial.rs

struct __attribute__((packed)) INA228Data {
    int32_t  shunt_raw;       // 20-bit, shifted << 4
    uint32_t bus_voltage_raw; // 32-bit
    int16_t  die_temp_raw;    // 16-bit
    int32_t  current_raw;     // 20-bit, shifted << 4
    uint32_t power_raw;       // 24-bit
    uint8_t  energy_raw[5];   // 40-bit
    uint8_t  charge_raw[5];   // 40-bit
}; // Exactly 28 bytes

struct __attribute__((packed)) TelemetryPayload {
    uint8_t prefix[16];   // Padding to push BATT to offset 16
    INA228Data batt;      // Offsets 16 to 44
    INA228Data fc;        // Offsets 44 to 72
    uint8_t suffix[16];   // Padding to reach 88 bytes total
}; // Exactly 88 bytes

struct __attribute__((packed)) USBHeader {
    char magic[4];        // "H2GP"
    uint8_t kind;         // 1 for Main Telemetry, 42 for AUX
    uint8_t res1;         
    uint16_t payload_len; // 88 for Main
    uint16_t res2;        
}; // Exactly 10 bytes

struct __attribute__((packed)) USBFrame {
    USBHeader header;
    TelemetryPayload payload;
}; // Exactly 98 bytes

// --- State Variables ---
unsigned long lastTelemetryTime = 0;
const int TELEMETRY_INTERVAL = 100; // 100ms = 10Hz transmission rate

// Helper function to reverse-engineer floating point values back into INA228 raw bytes
void pack_ina_data(float v, float i, float t, float e, float ah, float sv_mv, INA228Data* out) {
    float p = v * i;
    
    out->bus_voltage_raw = (uint32_t)(v / 0.0001953125f);
    out->current_raw     = ((int32_t)(i / 0.00006103515625f)) << 4;
    out->power_raw       = (uint32_t)(p / 0.0001953125f);
    out->die_temp_raw    = (int16_t)(t * 128.0f);
    out->shunt_raw       = ((int32_t)(sv_mv / 0.3125f)) << 4;
    
    // Pack 40-bit energy
    uint64_t e_raw = (uint64_t)(e / 0.003125f);
    memcpy(out->energy_raw, &e_raw, 5);
    
    // Pack 40-bit charge
    int64_t c_raw = (int64_t)((ah * 3600.0f) / 0.00006103515625f);
    memcpy(out->charge_raw, &c_raw, 5);
}

void setup() {
    // Must match the 115200 baud rate defined in serial.rs
    Serial.begin(115200);
}

void loop() {
    unsigned long currentMillis = millis();

    // 1. Receive Commands from the Rust PC Dashboard
    if (Serial.available()) {
        // Rust sends commands ending in \n via clone_port.write_all()
        String cmd = Serial.readStringUntil('\n');
        cmd.trim();
        
        if (cmd.length() > 0) {
            // In your real firmware, you would use ArduinoJson to parse this.
            // Example incoming: {"cmd":"fan","mode":"manual","duty":70}
            // For now, we will just silently consume it to keep the buffer clean.
        }
    }

    // 2. Transmit Binary Telemetry at 10Hz
    if (currentMillis - lastTelemetryTime >= TELEMETRY_INTERVAL) {
        lastTelemetryTime = currentMillis;

        USBFrame frame;
        memset(&frame, 0, sizeof(USBFrame));

        // Construct the 10-byte header expected by serial.rs
        frame.header.magic[0] = 'H';
        frame.header.magic[1] = '2';
        frame.header.magic[2] = 'G';
        frame.header.magic[3] = 'P';
        frame.header.kind = 1; // USB_KIND_MAIN_TELEMETRY
        frame.header.payload_len = sizeof(TelemetryPayload); // 88

        // Generate dynamic fake data for the UI
        float fake_batt_v = 11.5f + (sin(currentMillis / 1000.0f) * 0.5f);
        float fake_batt_i = 8.0f + (cos(currentMillis / 800.0f) * 2.0f);
        float fake_fc_i   = 6.0f + (sin(currentMillis / 500.0f) * 1.5f);

        // Pack BATT data
        pack_ina_data(fake_batt_v, fake_batt_i, 30.8f, 103.0f, 0.0230f, 24.0f, &frame.payload.batt);
        
        // Pack FC data
        pack_ina_data(12.40f, fake_fc_i, 35.9f, 26.4f, 0.0060f, 0.0f, &frame.payload.fc);

        // Blast the raw 98 bytes over Serial
        Serial.write((uint8_t*)&frame, sizeof(USBFrame));
    }
}