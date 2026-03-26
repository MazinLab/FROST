# FROST — Software Specification

**FROST** (Fridge Remote Operations, Software, and Thermometry)  
Primary cryostat control software for **MEC′** (MKID Exoplanet Camera) Prime  
Language: Rust | GUI: egui/eframe 0.27 | License: Apache 2.0

---

## 1. Overview

FROST will be a https server that contains all cryostat control commands, including temperature probing, ADR magnet control, and compressor status. It can be accessed through CLI to quickly execute commands, i.e. "frost get-temperature 4K_Stage" or "frost start ramp". This server can then be accessed by a gui, FROST-gui, which creates a simple user interface. 

The server will have several modules, each corresponding to a specific hardware piece. Each module is connected via serial interface. 

The modules will include:
- lakeshore350 : gets temperatures and controls heaters/outputs
- lakeshore625 : controls magnet ramping/de-ramping
- lakeshore370 : gets temperatures, eventually will be a PID control for lakeshore625 
- heatswitch : stepper motor (also controlled via serial) with commands to open/close 
- compressor : can be turned on/off, and status checked

## 2. Server Architecture

```
FROST/
├── src/
│   ├── main.rs          
│   ├── cli.rs           CLI
│   ├── compressor.rs    
│   ├── heatswitch.rs    
│   └── lakeshore625.rs
│   └── lakeshore350.rs
│   └── lakeshore370.rs
└── Cargo.toml
```

## 3. Hardware & Serial Interfaces

### 3.1 Lakeshore 

| Parameter     | Value                         |
|---------------|-------------------------------|
| Default port  | `/dev/ttyUSB0`                |
| Baud rate     | 9600                          |
| Data bits     | 7                             |
| Parity        | Odd                           |
| Stop bits     | 1                             |
| Timeout       | 2 000 ms                      |
| Terminator    | `\r\n` (TX) / `\n` (RX)      |
| Settling time | 200 ms after write            |

#### Supported commands

| Method                    | Command       | Description                              |
|---------------------------|---------------|------------------------------------------|
| `get_identification`      | `*IDN?`       | Device ID string                         |
| `get_baud_rate`           | `BAUD?`       | Baud rate code (0=9600 … 3=57600)        |
| `get_field`               | `RDGF?`       | Magnetic field (T)                       |
| `get_current`             | `RDGI?`       | Output current (A)                       |
| `get_voltage`             | `RDGV?`       | Output voltage (V)                       |
| `get_all_readings`        | RDGF/RDGI/RDGV | Field, current, and voltage together    |
| `set_current`             | `SETI <A>`    | Set target output current                |
| `get_ramp_rate`           | `RATE?`       | Read ramp rate (A/s)                     |
| `set_ramp_rate`           | `RATE <A/s>`  | Set ramp rate                            |
| `start_ramp`              | `RAMP`        | Begin current ramp                       |
| `stop_ramp`               | `STOP`        | Pause/stop current ramp                  |
| `get_compliance_voltage`  | `SETV?`       | Read compliance voltage limit (V)        |
| `set_compliance_voltage`  | `SETV <V>`    | Set compliance voltage limit (0.1–5.0 V) |
| `get_limits`              | `LIMIT?`      | Read current / voltage / rate limits     |
| `set_limits`              | `LIMIT <A>,<V>,<A/s>` | Set all three limits             |
| `get_quench_status`       | `QNCH?`       | Quench detection enable + step limit     |
| `set_quench_enable`       | `QNCH 0\|1`   | Enable/disable quench detection          |
| `set_quench_step_limit`   | `QNCH 1,<A/s>`| Set quench step limit                    |
| `set_quench_detection`    | `QNCH <en> <A/s>` | Set enable and step limit together   |
| `get_error_status`        | `ERSTR?`      | Hardware / operational / PSH error bits  |
| `raw_command`             | any           | Send arbitrary command string            |

#### ERSTR? bit-field parsing

`ERSTR?` returns `"hw,op,psh"` (three decimal integers).

**Hardware bits:** DAC processor (32), output control (16), over voltage (8), over current (4), low line (2), temperature fault (1).  
**Operational bits:** crowbar discharge (64), quench detected (32), remote inhibit (16), temp high (8), high line (4), ext current error (2), cal error (1).  
**PSH bits:** short circuit (2), open circuit (1).

---

### 3.2 Cryomech Pulse Tube Compressor

| Parameter     | Value             |
|---------------|-------------------|
| Default port  | `/dev/ttyUSB3`    |
| Baud rate     | 115 200           |
| Device addr   | 16                |
| Protocol      | SMDP V2           |
| Read timeout  | 1 000 ms          |

#### Supported operations

| Method              | Description                                          |
|---------------------|------------------------------------------------------|
| `get_status`        | Running state, runtime (hrs/min), error flags        |
| `start_compressor`  | Send start command and verify                        |
| `stop_compressor`   | Send stop command and verify                         |
| `get_temperature`   | Input/output water, helium gas, oil, CPU temps + min/max |
| `get_pressure`      | High/low side pressures + avg/min/max + bounce + current |
| `get_system_info`   | Firmware checksum, memory loss, CPU temp, clock battery |
| `get_all_readings`  | All of the above in one call                         |
| `clear_min_max`     | Reset min/max pressure and temperature records       |

---

### 3.3 Heat Switch — Zaber T-NM17A04 Stepper Motor

| Parameter     | Value          |
|---------------|----------------|
| Default port  | `/dev/ttyUSB4` |
| Baud rate     | 9 600          |
| Device ID     | 1              |
| Protocol      | Zaber binary (6-byte frames, little-endian data) |
| Cmd timeout   | 1 000 ms       |
| Travel (open/close) | 115 200 microsteps |

#### Zaber commands used

| Code | Command           |
|------|-------------------|
| 1    | HOME              |
| 20   | MOVE_ABS          |
| 21   | MOVE_REL          |
| 22   | MOVE_VEL          |
| 23   | STOP              |
| 53   | GET_SETTING       |
| 60   | GET_POSITION      |

Settings queried: `MAXSPEED` (42), `DEVICE_STATUS` (54), `LIMIT_HOME_TRIGGERED` (103), `LIMIT_CW_TRIGGERED` (104), `LIMIT_CCW_TRIGGERED` (105).

#### Supported operations (HeatswitchController)

| Method          | Description                                              |
|-----------------|----------------------------------------------------------|
| `open`          | CW move 115 200 steps (non-blocking)                    |
| `close`         | CCW move 115 200 steps (non-blocking)                   |
| `home`          | Send HOME command                                        |
| `reset`         | Re-home (clears stuck limit state)                       |
| `stop`          | Send single STOP                                         |
| `emergency_stop`| Send STOP × 3 with 10 ms gap                            |
| `move_absolute` | Move to absolute microstep position                      |
| `move_relative` | Move relative (+CW / −CCW)                              |
| `rotate_cw`     | CW by N steps                                            |
| `rotate_ccw`    | CCW by N steps                                           |
| `safe_cw`       | CW clamped to 1–1 000 steps                              |
| `safe_ccw`      | CCW clamped to 1–1 000 steps                             |
| `move_velocity` | Constant-velocity move (stop to halt)                    |
| `get_status`    | Position, limit states, max speed, device status         |
| `get_position`  | Current position in microsteps                           |

---

### 3.4 Lake Shore 350 — Temperature Controller

| Parameter     | Value             |
|---------------|-------------------|
| Default port  | `/dev/ttyUSB2`    |
| Baud rate     | 57 600            |
| Data bits     | 7                 |
| Parity        | Odd               |
| Stop bits     | 1                 |
| Timeout       | 2 000 ms          |
| Terminator    | `\n` (TX) / `readline` (RX) |
| Settling time | 300 ms after write |

#### Input map

| Input | Hardware                     | Units returned | Calibration                                    |
|-------|------------------------------|----------------|------------------------------------------------|
| A     | 3-head resistance thermometer | Ω → K          | Linear interp on `gl7_calibrations/3_head_cal.csv` |
| B     | (empty / RuOx placeholder)   | Ω → K          | `KRDG?` direct from Lakeshore                  |
| C     | 4-head resistance thermometer | Ω → K          | Linear interp on `gl7_calibrations/4_head_cal.csv` + 34.56 Ω offset |
| D1    | (empty)                      | Ω              | None                                           |
| D2    | 4-switch diode               | V → K          | Linear interp on `gl7_calibrations/pumps_switches_cal.csv` |
| D3    | 4K stage diode               | K              | Curve 21, calibrated directly on Lakeshore     |
| D4    | 3-pump diode                 | V → K          | Linear interp on `gl7_calibrations/pumps_switches_cal.csv` |
| D5    | 4-pump diode                 | V → K          | Linear interp on `gl7_calibrations/pumps_switches_cal.csv` |

Note: inputs A and C return resistance (Ω) from `SRDG?`; D4/D5 return voltage (V) from `SRDG?`.
D3 and B return Kelvin directly via `KRDG?`. Over-range is detected via `RDGST?` bit 32.

#### Serial commands

| Command              | Description                                        |
|----------------------|----------------------------------------------------|
| `*IDN?`              | Device identification string                       |
| `KRDG? <input>`      | Kelvin temperature reading                         |
| `SRDG? <input>`      | Sensor reading (Ω for A/C/D1, V for D2–D5)        |
| `RDGST? <input>`     | Status register — bit 32 = over-range              |
| `INNAME? <input>`    | Front panel display name for an input              |
| `INNAME <input>,<name>` | Set front panel display name                    |
| `MOUT? <output>`     | Query manual output percentage                     |
| `MOUT <output>,<pct>` | Set manual output percentage (0–100 %)           |
| `HTR? <output>`      | Read heater output (W or %)                        |
| `HTRSET? <output>`   | Query heater setup (resistance, max I, mode)       |
| `HTRSET <output>,…`  | Set heater setup                                   |
| `RANGE? <output>`    | Query heater range                                 |
| `RANGE <output>,<n>` | Set heater range                                   |
| `OUTMODE? <output>`  | Query output mode                                  |
| `AOUT? <output>`     | Analog output current value                        |
| `ANALOG? <output>`   | Query analog output config                         |
| `ANALOG <output>,…`  | Set analog output config                           |

#### Output map

| Output | Hardware         | Command | Recommended config                          |
|--------|------------------|---------|---------------------------------------------|
| 1      | 4-pump heater    | HTRSET  | 50 Ω, max user current 0.1 A, read current  |
| 2      | 3-pump heater    | HTRSET  | 25 Ω, max user current 1.73 A, read current |
| 3      | 4-switch analog  | ANALOG  | No input, Kelvin, 5 V max, 0 V min, unipolar |
| 4      | 3-switch analog  | ANALOG  | No input, Kelvin, 5 V max, 0 V min, unipolar |

#### Python CLI reference (`lakeshore350`)

| Argument                              | Description                                      |
|---------------------------------------|--------------------------------------------------|
| `--all`                               | Read all inputs A–D5 with calibration applied    |
| `--info`                              | `*IDN?` device identification                    |
| `--outputs-query <n>`                 | Query output n (MOUT/HTR/HTRSET/AOUT/ANALOG/OUTMODE/RANGE) |
| `--outputs-query-all`                 | Query all 4 outputs                              |
| `--outputs-set <n> <pct>`             | Set output n to pct % via `MOUT`                |
| `--outputs-set-params [n,p1,p2,…]`    | Set HTRSET (outputs 1–2) or ANALOG (outputs 3–4) |
| `--outputs-set-range <n> <range>`     | Set heater range via `RANGE`                    |
| `--display`                           | Show front panel display status                  |
| `--display-show <input>`              | Show `INNAME` for a specific input               |
| `--display-show-all`                  | Show `INNAME` for all inputs                     |
| `--display-set-name <input> <name>`   | Set `INNAME` for an input                        |
| `--raw-command <cmd>`                 | Send arbitrary serial command                    |

---

### 3.5 Lake Shore 370 — AC Resistance Bridge

| Parameter     | Value                              |
|---------------|------------------------------------|
| Default port  | `/dev/ttyUSB1`                     |
| Baud rate     | 9 600 (also supports 300, 1 200)   |
| Data bits     | 7                                  |
| Parity        | Odd                                |
| Stop bits     | 1                                  |
| Timeout       | 2 000 ms                           |
| Terminator    | `\r\n` (TX) / `\r\n` (RX)         |
| Settling time | 100 ms after write (500 ms for writes) |
| Inputs        | 1–16 (integer channel numbers)     |
| Analog outs   | 1–2                                |

#### Serial commands — readings

| Command               | Description                                             |
|-----------------------|---------------------------------------------------------|
| `*IDN?`               | Device identification string                            |
| `BAUD?`               | Baud rate code (0=300, 1=1200, 2=9600)                  |
| `BAUD <code>`         | Set baud rate                                           |
| `RDGK? <input>`       | Kelvin temperature (≤0 treated as T_OVER)               |
| `RDGR? <input>`       | Resistance in Ω (<0 treated as R_OVER)                  |
| `RDGPWR? <input>`     | Excitation power in W (auto-scaled fW–mW in display)   |
| `RDGST? <input>`      | Status register (integer bitmask)                       |
| `RDGRNG? <input>`     | Resistance range config: `mode,excitation,range,autorange,cs_off` |
| `RDGRNG <input>,…`    | Set resistance range                                    |

Over-range is detected by `"OVERLD"` in the response string or by a ≤0 Kelvin value.

#### `RDGRNG` field reference

| Field       | Values                              |
|-------------|-------------------------------------|
| `mode`      | 0=manual, 1=current, 2=voltage      |
| `excitation`| 1–22                                |
| `range`     | 1–22                                |
| `autorange` | 0=off, 1=on                         |
| `cs_off`    | 0=current source on, 1=off          |

#### Serial commands — heater & analog outputs

| Command             | Description                                                   |
|---------------------|---------------------------------------------------------------|
| `MOUT <pct>`        | Set heater manual output (0.0–100.0 %)                        |
| `HTR?`              | Get heater output percentage                                  |
| `HTRRNG <n>`        | Set heater current range (0–8, see table below)               |
| `HTRRNG?`           | Get heater current range code                                 |
| `HTRST?`            | Get heater status code                                        |
| `ANALOG <ch>,…`     | Set analog output config (polarity, mode, input, source, high, low, manual) |
| `ANALOG? <ch>`      | Get analog output config                                      |
| `AOUT? <ch>`        | Get analog output current value                               |

#### Heater range codes (`HTRRNG`)

| Code | Current   | Power into 100 Ω |
|------|-----------|------------------|
| 0    | Off       | —                |
| 1    | 31.6 µA   | 0.1 µW           |
| 2    | 100 µA    | 1 µW             |
| 3    | 316 µA    | 10 µW            |
| 4    | 1 mA      | 100 µW           |
| 5    | 3.16 mA   | 1 mW             |
| 6    | 10 mA     | 10 mW            |
| 7    | 31.6 mA   | 100 mW           |
| 8    | 100 mA    | 1 W              |

#### `ANALOG` mode codes

| Code | Mode                             |
|------|----------------------------------|
| 0    | Off                              |
| 1    | Channel (tracks input reading)   |
| 2    | Manual                           |
| 3    | Zone                             |
| 4    | Still (channel 2 only)           |

#### Python CLI reference (`lakeshore370`)

| Argument                                    | Description                                              |
|---------------------------------------------|----------------------------------------------------------|
| `--info`                                    | `*IDN?` + `BAUD?`                                        |
| `--read-temp <input>`                       | `RDGK?` — Kelvin reading                                 |
| `--read-resistance <input>`                 | `RDGR?` — resistance (Ω)                                 |
| `--read-power <input>`                      | `RDGPWR?` — excitation power (auto-scaled)               |
| `--read-status <input>`                     | `RDGST?` — status bitmask                                |
| `--get-range <input>`                       | `RDGRNG?` — resistance range config                      |
| `--set-range <input> <mode> <exc> <rng> <ar> <cs>` | `RDGRNG` — set range                            |
| `--heater-output <pct>`                     | `MOUT` — set heater %                                    |
| `--get-heater-output`                       | `HTR?`                                                   |
| `--heater-range <n>`                        | `HTRRNG` — set range code                                |
| `--get-heater-range`                        | `HTRRNG?`                                                |
| `--get-heater-status`                       | `HTRST?`                                                 |
| `--analog-config <ch> [args]`               | `ANALOG` — set analog output config                      |
| `--get-analog-config <ch>`                  | `ANALOG?`                                                |
| `--get-analog-output <ch>`                  | `AOUT?`                                                  |
| `--scan [inputs…]`                          | Scan listed inputs (temp, resistance, power, status)     |
| `--scan-range <start> <stop>`               | Scan contiguous input range                              |
| `--all`                                     | Read all inputs 1–16                                     |
| `--get-baud`                                | `BAUD?`                                                  |
| `--set-baud <code>`                         | `BAUD` — 0=300, 1=1200, 2=9600                           |
| `--port <port>`                             | Override serial port (default `/dev/ttyUSB1`)            |
| `--baudrate <baud>`                         | Override baud rate                                       |
| `--raw-command <cmd>`                       | Send arbitrary serial command                            |

---

## 4. GUI

I want the FROST-gui to be based on the FROST server interface 

### 4.1 Tabs

Status - main front page, will eventually have a button to "start cooldown" or "start warm-up" that will turn on/off the compressor. Should also have a button to start ramp/stop ramp which will be based on the lakeshore625 and heatswitch. Automation not currently set up. Temp recording should also be automatic here. 

Thermometry - should display current thermometry from lakeshore370 and lakeshore350 with the option of recording thermometry 

Compressor - show compressor status, errors, turn on/off 

Heat switch - turn on/off heatswitch 


## 5. CLI

Invoked by passing any argument to the binary. Uses **clap 4** (derive API). Example of turning off compressor: 

```
frost compressor stop
```


## 6. Dependencies

| Crate          | Version | Purpose                              |
|----------------|---------|--------------------------------------|
| `eframe`       | 0.27    | Native window + event loop           |
| `egui`         | 0.27    | Immediate-mode GUI widgets           |
| `serialport`   | 4       | Cross-platform serial I/O            |
| `thiserror`    | 1       | Ergonomic error type derivation      |
| `cryomech_api` | path    | SMDP API for Cryomech compressor     |
| `smdp`         | path    | SMDP packet framing (req by above)   |

---

## 7. Build & Run

```bash
# Requires cryomech_api/ and smdp/ as siblings of FROST/

cargo build --release
cargo run --release              # GUI mode
cargo run --release -- --help    # CLI help
```

Minimum Rust version: **1.70**. Requires an X11 or Wayland display for GUI mode.