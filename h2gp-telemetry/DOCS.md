# H2GP Telemetry System Architecture

## 1. System Overview
The H2GP Telemetry System is a low-latency, high-performance diagnostic dashboard designed for endurance hydrogen racing. Built in Rust utilizing the `egui` immediate-mode GUI framework, the software interfaces with a microcontroller (Main MCU) over a 115200 baud serial connection. The interface employs a high-contrast, dual-column aesthetic to monitor Fuel Cell (FC) and Battery (BATT) arrays simultaneously without visual clutter.

## 2. Telemetry Computations
The dashboard acts as a passive receiver, but it processes historical data to generate real-time statistics. 

### 2.1 Power and Energy
Power is calculated instantly per frame. Energy is integrated over time by the MCU, but the total instantaneous system load is derived as:
$$P_{total} = (V_{batt} \cdot I_{batt}) + (V_{fc} \cdot I_{fc})$$

The power balance ratio (FC load vs Battery load) dictates the progress bar rendering:
$$Ratio_{fc} = \frac{P_{fc}}{P_{total}}$$

### 2.2 Rolling Averages
To smooth out transient spikes from the physical hardware (e.g., servo movements or fuel cell short-circuiting), the dashboard computes an $N$-sample simple moving average (SMA) for voltages and currents, where $N=10$:
$$SMA = \frac{1}{N} \sum_{k=0}^{N-1} x_{t-k}$$

## 3. Communication Protocol

### 3.1 Downlink: MCU to PC
The MCU must transmit an un-nested, flattened JSON-like string at exactly 10Hz. The 15-column format requires the following explicit keys.

**Primary Telemetry Frame:**
```json
RX {"BATT":{"V":11.85,"I":8.20,"P":97.17,"E":103.0,"ah":0.023,"t":30.8,"sv_mv":24}, "FC":{"V":12.40,"I":6.00,"P":74.40,"E":26.4,"ah":0.006,"t":35.9,"sv_mv":0}}
```

**Secondary Auxiliary Frame (REV3):**
```text
AUX REV3 2 | T[ 26.8/ 27.9/ --.-/ --.-] | MAX 27.9C | F: 70% | FLG: 2
```
*Note: The `FLG` (Flags) byte is a bitmask. Bit 0 represents a Fuel Cell Short Circuit (Zkracovani). Bit 1 represents a Purge Valve actuation (Odvod).*

### 3.2 Uplink: PC to MCU
The dashboard transmits commands back to the MCU using strict JSON formatting. The MCU must implement an interrupt-driven or non-blocking serial read to parse these incoming commands without delaying the 10Hz transmission loop.

**Fan Control Command:**
```json
{"cmd":"fan","mode":"manual","duty":70}
```
**Driver Sign Command:**
```json
{"cmd":"sign","driver":"SKL"}
```

## 4. UI Architecture & Rendering

### 4.1 Bounding Boxes
To prevent layout shifting during data mutations (e.g., transitioning from `5.00` to `-15.00`), all floating diagnostic text utilizes fixed-width string padding:
`{:>6.2}` ensures a strict 6-character width with 2 decimal places. 

The central UI utilizes `ui.allocate_ui_with_layout` to force a rigid 50/50 horizontal split. This prevents right-aligned components in the left column from pushing the right column off-screen.

### 4.2 Time-Series Plotting
The history graph renders an oscilloscope-style sliding window to prevent data compression over an 8-hour race. Given the 1200-sample history limit, the plot filters points based on a dynamic time threshold:
$$t_{threshold} = t_{latest} - W$$
Where $W$ is the user-selected window size $\in \{5, 10, 15, 30, 60, 120\}$ seconds.