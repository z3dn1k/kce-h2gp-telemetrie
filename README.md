# H2Gp Telemetry Dashboard

A high-performance, low-latency telemetry dashboard for H2GP (Hydrogen Grand Prix) endurance racing, built entirely in Rust using [`egui`](https://github.com/emilk/egui). 

Designed with a "Silent Killer" minimalist dark aesthetic, this dashboard processes real-time serial data from the car's Main MCU, providing trackside engineers with instant visibility into both the Fuel Cell and Battery/C-Bank systems.

![Dashboard Preview](docs/dashboard-preview.png) <!-- Note: Add your screenshot here and push to a /docs folder! -->

## Key Features

*   **Bifurcated UI Layout:** Strict 50/50 split-screen separating the Battery/C-Bank (Cyan) and Fuel Cell (Red) metrics for instant visual orientation during high-pressure 8-hour races.
*   **Real-Time Rolling Oscilloscope:** Live `egui_plot` graphs with adjustable rolling time-windows (5s, 10s, 15s, 30s, 60s, 2m) to catch micro-stutters, transient voltage sags, and actuator spikes without barcode-compression.
*   **Hardware Diagnostics Sandbox:** Toggleable fixed-width diagnostic footer displaying the raw BATT/FC JSON frame, AUX string, and absolute packet counter (`PKT`) for instant trackside hardware/parser debugging.
*   **Actuator States:** Real-time visual tracking of Fuel Cell short-circuiting (*ZKRACOVANI*) and purging (*ODVOD*) via AUX bitmask flags.
*   **Bidirectional Control:** Send JSON commands directly back to the MCU to control Fan Modes (Auto/Manual/Off), PWM Duty Cycles, and update the 3-character Driver Sign.
*   **Data Archival & Rolling Stats:** Automatically logs all incoming valid packets to `data.csv` while maintaining a 1200-sample in-memory buffer to compute live rolling averages, minimums, and maximums.
*   **Demo Mode:** Built-in telemetry playback thread that simulates an 18-second track profile from a CSV file for offline UI testing and development.

## Tech Stack

*   **Language:** [Rust](https://www.rust-lang.org/)
*   **GUI Framework:** [eframe / egui](https://github.com/emilk/egui) (Immediate Mode GUI)
*   **Charting:** `egui_plot`
*   **Concurrency:** Standard library `mpsc` channels bridging the serial reading thread and the UI rendering thread.
