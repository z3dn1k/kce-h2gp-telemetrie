#include <Arduino.h>
#include <math.h>
#include <stdio.h>

// H2GP Telemetry Transmitter (Main MCU)
// Flashes fake telemetry data to test the Rust Dashboard

unsigned long lastTelemetryTime = 0;
const int TELEMETRY_INTERVAL = 100; // 100ms = 10Hz transmission rate

void setup() {
    // Must match the 115200 baud rate defined in our Rust serial.rs
    Serial.begin(115200);
}

void loop() {
    unsigned long currentMillis = millis();

    // Transmit Telemetry at 10Hz
    if (currentMillis - lastTelemetryTime >= TELEMETRY_INTERVAL) {
        lastTelemetryTime = currentMillis;

        // 1. Generate some fake fluctuating data so the graphs move
        float fake_batt_v = 11.5 + (sin(currentMillis / 1000.0) * 0.5); // Sine wave between 11.0 and 12.0
        float fake_batt_i = 8.0 + (cos(currentMillis / 800.0) * 2.0);
        float fake_fc_i = 6.0 + (sin(currentMillis / 500.0) * 1.5);
        
        // 2. Create a buffer large enough to hold the JSON string
        char jsonBuffer[300];

        // 3. Format the BATT and FC data into the exact JSON structure Rust expects
        snprintf(jsonBuffer, sizeof(jsonBuffer),
            "{\"BATT\":{\"V\":%.2f,\"I\":%.2f,\"P\":%.2f,\"E\":103.0,\"ah\":0.0230,\"t\":30.8,\"sv_mv\":24},"
            "\"FC\":{\"V\":12.40,\"I\":%.2f,\"P\":%.2f,\"E\":26.4,\"ah\":0.0060,\"t\":35.9,\"sv_mv\":0}}",
            fake_batt_v, 
            fake_batt_i, 
            (fake_batt_v * fake_batt_i), // Power = V * I
            fake_fc_i,
            (12.40 * fake_fc_i)          // Power = V * I
        );

        // 4. Blast it over Serial
        Serial.println(jsonBuffer);
    }
}