#include <ArduinoJson.h>

const int FAN_PWM_PIN = D3; 

unsigned long lastTelemetryTime = 0;
const unsigned long TELEMETRY_INTERVAL = 100; // 10Hz transmission

void setup() {
  Serial.begin(115200);
  pinMode(FAN_PWM_PIN, OUTPUT);
  analogWrite(FAN_PWM_PIN, 0);
}

void loop() {
  // 1. Transmit 88-byte binary frame at 10Hz
  if (millis() - lastTelemetryTime >= TELEMETRY_INTERVAL) {
    lastTelemetryTime = millis();
    sendFakeTelemetry();
  }

  // 2. Receive JSON commands from the Rust dashboard
  if (Serial.available()) {
    String jsonStr = Serial.readStringUntil('\n');
    
    StaticJsonDocument<256> doc;
    DeserializationError error = deserializeJson(doc, jsonStr);

    if (!error && doc["cmd"] == "fan") {
      int duty = doc["duty"] | 0;
      String mode = doc["mode"] | "auto";
      
      if (mode == "off") duty = 0;
      
      // Map 0-100% duty cycle to 0-255 physical PWM resolution
      analogWrite(FAN_PWM_PIN, map(duty, 0, 100, 0, 255));
    }
  }
}

void sendFakeTelemetry() {
  // 10 byte header + 88 byte payload = 98 bytes total
  uint8_t buffer[98] = {0}; 

  // --- HEADER ALIGNMENT ---
  buffer[0] = 0x48; buffer[1] = 0x32; buffer[2] = 0x47; buffer[3] = 0x50; // "H2GP" Magic Bytes
  buffer[4] = 1;  // Kind: Main Telemetry
  buffer[6] = 88; // Payload length LSB
  buffer[7] = 0;  // Payload length MSB

  // --- BATT DATA (Offset 16 in payload -> 26 in buffer) ---
  // Voltage (12.0V) -> 12.0 / 0.0001953125 = 61440 = 0xF000 (Little Endian)
  buffer[26 + 4] = 0x00; buffer[26 + 5] = 0xF0; buffer[26 + 6] = 0x00; buffer[26 + 7] = 0x00;
  // Current (3.5A) -> 3.5 / (32/524288) shifted << 4 = 917504 = 0x0E0000
  buffer[26 + 10] = 0x00; buffer[26 + 11] = 0x00; buffer[26 + 12] = 0x0E; buffer[26 + 13] = 0x00;

  // --- FUEL CELL DATA (Offset 44 in payload -> 54 in buffer) ---
  // Voltage (10.5V) -> 10.5 / 0.0001953125 = 53760 = 0xD200 
  buffer[54 + 4] = 0x00; buffer[54 + 5] = 0xD2; buffer[54 + 6] = 0x00; buffer[54 + 7] = 0x00;
  // Current (1.2A) -> 1.2 / (32/524288) shifted << 4 = 314572 = 0x04CC0C
  buffer[54 + 10] = 0x0C; buffer[54 + 11] = 0xCC; buffer[54 + 12] = 0x04; buffer[54 + 13] = 0x00;

  Serial.write(buffer, sizeof(buffer));
}