# 🏎️ H2GP Telemetry Dashboard

[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg?logo=rust)](https://www.rust-lang.org)
[![GUI](https://img.shields.io/badge/GUI-egui%200.31-blue.svg)](https://github.com/emilk/egui)
[![Tests](https://img.shields.io/badge/tests-45%20passed-brightgreen.svg)]()
[![Clippy](https://img.shields.io/badge/clippy-zero%20warnings-brightgreen.svg)]()
[![Unsafe](https://img.shields.io/badge/unsafe-forbidden-success.svg)]()
[![License](https://img.shields.io/badge/license-MIT-informational.svg)](LICENSE)

A high-performance, real-time telemetry acquisition, visualization, and diagnostic dashboard built in **Rust** using the immediate-mode **egui** framework. Developed for pit-lane endurance monitoring in the **Horizon Hydrogen Grand Prix (H2GP)** 1:10 scale hydrogen fuel cell RC car racing championship.

---

## 📋 Table of Contents

- [Overview](#-overview)
- [Key Features](#-key-features)
- [Architecture & Concurrency](#-architecture--concurrency)
- [Binary Communication Protocol](#-binary-communication-protocol)
- [Crates & Technical Justification](#-crates--technical-justification)
- [Configuration (`config.toml`)](#-configuration-configtoml)
- [Keyboard Shortcuts](#-keyboard-shortcuts)
- [Getting Started](#-getting-started)
- [Testing & Code Quality](#-testing--code-quality)
- [License & Author](#-license--author)

---

## 🔭 Overview

In the Horizon Hydrogen Grand Prix, vehicles are powered by a hybrid powertrain:
1. **Hydrogen Fuel Cell (PEMFC)** — delivers continuous baseline power from pressurized hydrogen gas.
2. **Battery / Capacitor Bank (BATT/CBANK)** — assists during high-draw acceleration bursts and absorbs regenerative energy.

Optimizing fuel consumption, managing capacitor state-of-charge, and preventing fuel cell membrane starvation requires instant, reliable trackside telemetry. This dashboard decodes raw binary sensor frames transmitted over USB/Radio at up to **30 Hz**, displays live dual-channel power flows, detects physical anomalies, logs data to disk without blocking the UI thread, and provides bidirectional control commands back to the vehicle's microcontroller.

---

## ✨ Key Features

- **🚀 Zero Compiler Warnings & `#![forbid(unsafe_code)]`**: Clean, idiomatic Rust with 100% memory safety.
- **⚡ Dual-Channel Real-Time Oscilloscope**: Rolling history plots with customizable time windows (5s, 10s, 15s, 30s, 60s, 2m) across Bus Voltage, Current, Power, and Cumulative Energy.
- **📊 Physical Comparison Dashboard**: Live comparison of the Battery/Capacitor bank vs. the Fuel Cell (Voltage, Current, Power, Die Temperature, Energy, Capacity, Shunt Voltage, and charging/discharging states).
- **⚠️ Autonomous Anomaly Engine (`anomaly.rs`)**: Standalone rule engine detecting battery overcurrent, fuel cell voltage sag (starvation), cell overtemperature, and short-circuit flags.
- **⏸️ Telemetry Freeze (Pause/Resume)**: Instantly pause the rolling chart with the `Space` key to inspect transient voltage sags and current spikes without missing background data logging.
- **🔌 Serial Port Auto-Detection**: Scans the host OS for available serial ports with dynamic dropdown selection and a live rescan (`↻`) button.
- **📶 Connection Quality Metric (PPS)**: Real-time packets-per-second monitoring with dynamic color thresholds (Green: $\ge 10\text{ PPS}$, Orange: $1\text{--}9\text{ PPS}$, Red: $0\text{ PPS}$).
- **📷 Instant PNG Screenshot Export**: Captures the entire dashboard UI state to timestamped PNG files (`screenshot_<timestamp>.png`) with transient toast notifications.
- **⏱️ Live Stint Uptime Tracker**: Dynamic `[MM:SS]` timers tracking the duration of active live and demo sessions.
- **🎮 Bidirectional Uplink Control**: Transmit JSON commands to the car MCU for cooling fan mode (Auto, Manual, Off), PWM duty percentage (0–100%), and matrix display driver callsign.
- **💾 Single-Point Non-Blocking Logging**: High-throughput buffered CSV streaming (`data.csv`) and anomaly logging (`anomalies.log`) decoupled from the rendering loop.
- **🧪 Offline Demo Mode**: Replay recorded historical telemetry with synthetic monotonic clocks at a steady 10 Hz for testing without physical hardware.

---

## 🏗️ Architecture & Concurrency

The application utilizes a multi-threaded architecture separated into decoupled I/O and rendering domains communicating via type-safe `std::sync::mpsc` channels:

```
┌────────────────────────────────┐               ┌────────────────────────────────┐
│      Serial Worker Thread      │               │       Demo Replay Thread       │
│          (serial.rs)           │               │           (demo.rs)            │
│  - Non-blocking USB streaming  │               │  - Reads recorded data.csv     │
│  - Stream magic synchronization│               │  - Synthetic monotonic clock   │
│  - Binary frame unpacking      │               │  - 10 Hz steady sample rate    │
│  - Buffered data.csv logging   │               └───────────────┬────────────────┘
└───────────────┬────────────────┘                               │
                │             mpsc::Sender<TelemetrySample>      │
                └────────────────────────┬───────────────────────┘
                                         │
                                         ▼ mpsc::Receiver<TelemetrySample>
                        ┌─────────────────────────────────┐
                        │      Immediate-Mode UI Loop     │
                        │       (main.rs & ui.rs)         │
                        │  - eframe / egui 30 FPS render  │
                        │  - Standalone Anomaly Engine    │
                        │  - Rolling DataManager buffer   │
                        │  - Interactive Oscilloscope     │
                        │  - Keyboard shortcut dispatch   │
                        └────────────────┬────────────────┘
                                         │ mpsc::Sender<String>
                                         ▼ (Outgoing JSON uplink commands)
                        ┌─────────────────────────────────┐
                        │       MCU Uplink Channel        │
                        │  - Fan mode & PWM duty          │
                        │  - Driver 3-letter sign         │
                        └─────────────────────────────────┘
```

- **Graceful Thread Shutdown**: Thread lifecycles are governed by `Arc<AtomicBool>`. Threads cleanly terminate upon disconnection or window closing (`impl Drop for TelemetryApp`).
- **Batch-Draining Circular Buffer**: `DataManager` accumulates data up to `max_history + batch_size` before executing amortized $O(1)$ batch slices (`history.drain(0..batch_size)`), avoiding costly single-element memory shifts.
- **Clock Monotonicity Guard**: Prevents "Z-fold" graphical spaghetti artifacts caused by MCU resets by detecting non-monotonic timestamps and resetting buffers.

---

## 📡 Binary Communication Protocol

To minimize serial bus latency and transmission overhead, raw text CSV streaming was replaced with a packed binary framing protocol.

### Frame Layout (10-byte header):
```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|       'H'     |      '2'      |      'G'      |      'P'      |  Magic (0x48, 0x32, 0x47, 0x50)
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|     Kind      |    Reserved   |          Payload Length       |  Kind (1=Main, 42=Aux)
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|          CRC / Sequence       |                                  Payload Data...
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### INA228 Dual-Channel Registers (28 bytes per channel):
The main packet carries two 28-byte raw register slices from **Texas Instruments INA228** power monitors:
- **Bus Voltage ($V$)**: 24-bit unsigned $\times 0.0001953125\text{ V}$
- **Current ($I$)**: 20-bit signed (arithmetic right-shift 4) with sign-extension $\times \frac{32.0}{524288.0}\text{ A}$
- **Shunt Voltage ($V_{shunt}$)**: 20-bit signed $\times 0.0003125 \times 1000.0\text{ mV}$
- **Die Temperature ($T$)**: 16-bit signed $\times \frac{1.0}{128.0}\,^\circ\text{C}$
- **Power ($P$)**: 24-bit unsigned $\times (3.2 \times I_{LSB})\text{ W}$
- **Energy ($E$)**: 40-bit unsigned $\times (16.0 \times P_{LSB})\text{ J}$
- **Charge ($Ah$)**: 40-bit signed $\times \frac{I_{LSB}}{3600.0}\text{ Ah}$

### AUX Packet (Kind 42):
Contains power board Rev3 auxiliary diagnostics:
- Up to 4 external temperature sensors ($T = \text{raw} / 16.0\,^\circ\text{C}$; sentinel `i16::MIN` maps to $-127.0\,^\circ\text{C}$ for disconnected probes).
- Fuel cell conditioning bitmask flags (`0x01`: Short-circuiting / *Zkracování*, `0x02`: Hydrogen Purge / *Odvod*).
- Active fan PWM duty percentage and commanded fan mode (`Auto`, `Manual`, `Off`).

---

## 📦 Crates & Technical Justification

| Crate | Version | Justification & Purpose |
|---|---|---|
| **`eframe` / `egui`** | `0.31` | Pure Rust immediate-mode GUI. Guaranteed 30–60 FPS rendering with zero Garbage Collector pauses, crucial for high-speed telemetry. |
| **`egui_plot`** | `0.31` | Native, GPU-accelerated 2D plotting tightly integrated with egui without webview overhead. |
| **`serialport`** | `4.5` | Cross-platform USB virtual COM port driver supporting non-blocking timeouts and port auto-detection. |
| **`serde` & `serde_json`** | `1.0` | Compile-time derived serialization for bidirectional uplink JSON commands and TOML configs. |
| **`toml`** | `0.8` | Human-readable configuration parsing and generation for non-recompiled runtime parameter tuning. |
| **`thiserror`** | `2.0` | Ergonomic, zero-cost, type-safe custom error enum (`TelemetryError`) replacing generic panics. |
| **`image`** | `0.25` | Lightweight pure-Rust RGBA pixel buffer decoding and PNG export without native C dependencies. |

---

## ⚙️ Configuration (`config.toml`)

Parameters are loaded at startup from `config.toml`. If missing, a commented configuration file with safe defaults is automatically generated:

```toml
serial_baud_rate = 115200
default_port = "COM3"
buffer_capacity = 1200
sma_window = 10
anomaly_batt_overcurrent_a = 15.0
anomaly_fc_vsag_v = 9.0
anomaly_fc_min_v = 2.0
anomaly_batt_overtemp_c = 45.0
anomaly_queue_capacity = 10
ui_refresh_interval_ms = 33
default_chart_window_s = 30.0
default_driver_code = "SKL"
default_fan_duty = 70
demo_interval_ms = 100
```

---

## ⌨️ Keyboard Shortcuts

| Shortcut | Action | Description |
|:---:|---|---|
| **`1`** | Voltage Tab | Switches oscilloscope to Bus Voltage (V) |
| **`2`** | Current Tab | Switches oscilloscope to Current Draw (A) |
| **`3`** | Power Tab | Switches oscilloscope to Instantaneous Power (W) |
| **`4`** | Energy Tab | Switches oscilloscope to Cumulative Energy (J) |
| **`Space`** | Pause / Resume | Freezes rolling chart updates for detailed spike inspection |
| **`C`** | Connect / Disconnect | Toggles live serial connection on the selected port |
| **`D`** | Demo Mode | Toggles offline replay simulation from `data.csv` |

> ℹ️ **Smart Focus Isolation**: Shortcuts are gated by `!ctx.wants_keyboard_input()`. When editing text fields (such as entering driver code or custom port names), keystrokes are treated as regular text.

---

## 🚀 Getting Started

### Prerequisites
- [Rust Toolchain](https://rustup.rs/) (version 1.80 or newer recommended)

### Build & Run
```bash
# Clone repository
git clone https://github.com/z3dn1k/kce-h2gp-telemetrie.git
cd kce-h2gp-telemetrie/h2gp-telemetry

# Run application
cargo run --release
```

1. **Offline Demo Simulation**: Press **`D`** on your keyboard (or click **DEMO (D)** in the toolbar). Live graphs, rolling stats, and telemetry feeds will instantly start playing back from `data.csv`.
2. **Live Vehicle Connection**: Connect your USB telemetry receiver, click **`↻`** to detect the port, select it from the dropdown, and press **`C`** (or click **CONNECT**).

---

## 🧪 Testing & Code Quality

```bash
# Run all 45 unit tests
cargo test

# Run strict Clippy linter
cargo clippy -- -D warnings

# Build complete offline HTML documentation
cargo doc --no-deps --open
```

### Test Suite Summary:
- **`protocol::tests` (22 tests)**: Sign extension (20-bit and 40-bit), INA228 LSB conversions, buffer underflow handling, and sentinel temperature mapping.
- **`datamanager::tests` (7 tests)**: Monotonicity reset guards, SMA rolling averages, min/max tracking, batch buffer draining, and summary exports.
- **`config::tests` (4 tests)**: TOML serialization round-trips, default fallback handling, and disk persistence.
- **`anomaly::tests` (6 tests)**: Voltage sag detection, overcurrent triggers, thermal safety limits, and condition flags.
- **`serial::tests` (1 test)**: Port detection safety and panic-free hardware discovery.
- **`main::tests` (5 tests)**: Connection state machine transitions, uptime formatting, pause toggles, PPS reset, and bounded alert queue rotation.

---

## 📄 License & Author

Developed by **Jan Zedník** (`z3dn1k`) for secondary school graduation thesis (*maturitní práce*) & **H2GP Racing Team**.

Released under the **[MIT License](LICENSE)**.

