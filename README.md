# FROST (Fridge Remote Operations, Status, and Thermometry)

Primary cryostat control software for MEC' (MKID Exoplanet Camera) Prime

## Overview

FROST provides a single interface for controlling and monitoring all cryostat hardware via serial communication. It exposes both a command-line interface for scripting and direct device control, and a desktop GUI for interactive operation.

| Device | Role |
|--------|------|
| **Lakeshore 625** | Superconducting Magnet Power Supply — ramps/de-ramps ADR magnet |
| **Lakeshore 370** | AC Resistance Bridge — monitors device stage temperature, controls PID loop with LS625 |
| **Lakeshore 350** | Temperature Controller — all stage and GL7 thermometers, GL7 heater outputs |
| **Heatswitch** | Zaber T-NM17A04 stepper motor — opens/closes heat switch during ADR ramp |
| **Compressor** | Cryomech pulse tube compressor — turns PT cooling on/off during cooldown |

---

## Requirements

- Rust 1.70+
- GUI environment (X11 or Wayland) for the GUI mode
- `cryomech_api` crate at `../cryomech_api/` (path dependency — see [Dependencies](#dependencies))

---

## Building

```bash
# Build optimized release binary
cargo build --release

# Run tests (no hardware required — all tests use nonexistent port paths)
cargo test

# Run hardware-dependent tests (requires physical devices connected)
cargo test -- --include-ignored
```

---

## Installation

After building, the binary lives at `target/release/frost`. The recommended installation is a symlink — it survives rebuilds without any extra steps:

```bash
sudo ln -sf $(pwd)/target/release/frost /usr/local/bin/frost
```

Only needs to be run once. Every subsequent `cargo build --release` automatically updates the installed binary.

Verify the installation:
```bash
frost --help
```

---

## Serial Port Configuration

All device ports have hardcoded defaults that match the lab setup. They can be overridden per-command with `--port` and `--baud` flags.

| Device | Default Port | Default Baud |
|--------|-------------|--------------|
| Lakeshore 625 | `/dev/ttyUSB0` | 9600 |
| Lakeshore 370 | `/dev/ttyUSB1` | 9600 |
| Lakeshore 350 | `/dev/ttyUSB2` | 57600 |
| Compressor | `/dev/ttyUSB3` | 115200 |
| Heatswitch | `/dev/ttyUSB4` | 9600 |

To use a different port for a single command:
```bash
frost lakeshore625 --port /dev/ttyUSB5 current
frost lakeshore370 --port /dev/ttyUSB2 --baud 1200 kelvin 1
frost heatswitch --port /dev/ttyUSB1 --device-id 2 open
frost compressor --port /dev/ttyUSB0 --addr 17 status
```

There are no config files — if you need permanent port reassignment, update the `DEFAULT_PORT` constants in the relevant `src/<device>.rs` file.

---

## GUI

Launch the interactive GUI:
```bash
frost gui
# or, to build and run in one step:
cargo run --release -- gui
```

The GUI runs a background worker thread that polls all devices every 30 seconds (staggered to avoid serial bus contention) while keeping the interface responsive. All serial I/O happens off the render thread.

### GUI Sections

**Compressor**
- Start/Stop buttons with live status display
- Shows running state and last update time
- Compressor intent is persisted to disk (`state/.compressor_intent`) — if FROST is restarted, the GUI restores the last known compressor state

**Magnet (Lakeshore 625)**
- Live readouts: current (A), voltage (V), field
- Editable fields: current setpoint, current/voltage/rate limits, ramp rate, compliance voltage
- "Set" buttons apply each parameter to the instrument
- Values are auto-populated from hardware polls when new data arrives

**GL7 Outputs (Lakeshore 350)**
- Displays output percentage for each of the 4 GL7 heater/switch outputs
- Editable percentage fields with "Set" buttons to apply

**Thermometry**
- Live temperature readings from all LS350 inputs (A, B, C, D2, D3, D4, D5) and LS370 input 1
- Inputs using custom calibration (3-head, 4-head, pump diodes) show calibrated temperatures in Kelvin
- Direct Lakeshore-calibrated inputs (B, D3) show kelvin directly

**Temperature Recording**
- Start/Stop buttons for continuous CSV logging
- Logs all thermometry inputs at 30-second intervals to `temps/YYYY-MM-DD_temperature_log.csv`
- Recording state persists across restarts (`temps/.recording_active` lock file) — the GUI detects an interrupted recording on startup and resumes it

**ADR Ramp**
- Input fields for ramp rate (A/s), target current (A), and soak time (minutes)
- Start button executes the full automated ramp sequence
- Live log output displayed in the GUI during ramp
- Ramp state persisted to `state/.adr_ramp_running` — detects interrupted ramps on restart

### Theme

17 color themes selectable from the top of the GUI window (Default, Dark, Light Blue, Purple, and more).

---

## CLI Reference

```
frost <device> [OPTIONS] <command> [ARGS]
```

### `frost adr` — ADR Operations

```bash
# Run a full automated ADR ramp sequence
frost adr ramp <rate_A_per_s> <target_current_A> [--soak-mins N]

# Example: ramp at 0.005 A/s to 9.44 A, soak 60 minutes
frost adr ramp 0.050 9.44 --soak-mins 60

# Start LS625 ramp data logging only (writes to ramps/ CSV)
frost adr logging
```

The `ramp` command runs the full automated sequence:
1. Start background ramp logger (LS625 data to CSV, 1 row/min)
2. Set ramp rate and target current — instrument begins ramping
3. Wait for current to reach within 0.04 A of target
4. Soak at constant current for `--soak-mins` (default 45)
5. Open heat switch + wait 3 minutes
6. Ramp current to 0 A, wait until ≤ 0.004 A
7. Stop logger

### `frost compressor` — Compressor Control

```bash
frost compressor [--port PORT] [--baud BAUD] [--addr ADDR] <command>

frost compressor status          # Running state, runtime, error flags
frost compressor start           # Start the compressor
frost compressor stop            # Stop the compressor
frost compressor temperature     # Water/helium/oil/CPU temps + min/max
frost compressor pressure        # High/low side pressures + motor current
frost compressor system          # Firmware info, memory status, CPU temp
frost compressor all             # All of the above in one call
frost compressor clear-min-max   # Reset min/max records
```

### `frost heatswitch` — Heat Switch (Zaber Motor)

```bash
frost heatswitch [--port PORT] [--baud BAUD] [--device-id ID] <command>

frost heatswitch open            # Move +115200 microsteps (open position)
frost heatswitch close           # Move -115200 microsteps (closed position)
frost heatswitch home            # Home the motor
frost heatswitch reset           # Re-home
frost heatswitch stop            # Stop motion
frost heatswitch estop           # Emergency stop (3x STOP)
frost heatswitch move-abs <pos>  # Move to absolute position (microsteps)
frost heatswitch move-rel <steps># Move relative (microsteps)
frost heatswitch cw <steps>      # Rotate clockwise
frost heatswitch ccw <steps>     # Rotate counter-clockwise
frost heatswitch safe-cw <steps> # Clamped CW move (1–1000 steps)
frost heatswitch safe-ccw <steps># Clamped CCW move (1–1000 steps)
frost heatswitch move-vel <vel>  # Move at velocity
```

Note: `open` and `close` return immediately — the motor executes asynchronously. Poll status separately to check completion.

### `frost lakeshore625` — Magnet Power Supply

```bash
frost lakeshore625 [--port PORT] <command>

frost lakeshore625 identify
frost lakeshore625 current            # Live current readout
frost lakeshore625 voltage            # Live voltage readout
frost lakeshore625 field              # Live field readout
frost lakeshore625 all                # Current + voltage + field
frost lakeshore625 get-current        # Programmed current setpoint (SETI?)
frost lakeshore625 set-current <A>    # Set target current
frost lakeshore625 get-rate           # Ramp rate (A/s)
frost lakeshore625 set-rate <A/s>     # Set ramp rate
frost lakeshore625 start-ramp         # Begin ramping to setpoint
frost lakeshore625 stop-ramp          # Halt ramp
frost lakeshore625 get-compliance     # Compliance voltage (V)
frost lakeshore625 set-compliance <V> # Set compliance voltage (0.1–5.0 V)
frost lakeshore625 get-limits         # Current/voltage/rate limits
frost lakeshore625 set-limits <A> <V> <A/s>  # Set all limits
frost lakeshore625 quench-status
frost lakeshore625 enable-quench
frost lakeshore625 disable-quench
frost lakeshore625 set-quench <step_limit>
frost lakeshore625 error-status       # Hardware/operation/PSH error register
frost lakeshore625 logging            # Start ramp data logging to CSV
frost lakeshore625 raw "<SCPI cmd>"   # Send arbitrary SCPI command
```

### `frost lakeshore370` — Device Stage Thermometry / PID

```bash
frost lakeshore370 [--port PORT] [--baud BAUD] <command>

frost lakeshore370 identify
frost lakeshore370 kelvin <input>          # Temperature in K (input 1–16)
frost lakeshore370 resistance <input>      # Resistance in Ω
frost lakeshore370 power <input>           # Excitation power
frost lakeshore370 input-status <input>    # Input status flags
frost lakeshore370 all <input>             # Kelvin + resistance + power
frost lakeshore370 get-range <input>       # Resistance range config
frost lakeshore370 set-range <input> <mode> <excitation> <range> <autorange> <cs_off>
frost lakeshore370 get-heater             # Heater output (%)
frost lakeshore370 set-heater <pct>       # Set heater output 0–100%
frost lakeshore370 get-heater-range       # Heater range (0=Off, 1–8)
frost lakeshore370 set-heater-range <N>   # Set heater range
frost lakeshore370 heater-status          # Heater error status
frost lakeshore370 get-analog <ch>        # Analog output config (ch 1–2)
frost lakeshore370 analog-output <ch>     # Analog output value
frost lakeshore370 set-analog-off <ch>
frost lakeshore370 set-analog-channel <...>
frost lakeshore370 set-analog-manual <...>
frost lakeshore370 set-analog-still <...>
frost lakeshore370 raw "<SCPI cmd>"

# Example: read device stage temperature
frost lakeshore370 kelvin 1
```

### `frost lakeshore350` — Stage Thermometry / GL7 Control

```bash
frost lakeshore350 [--port PORT] [--baud BAUD] <command>

frost lakeshore350 identify
frost lakeshore350 read <input>           # Single input reading (A, B, C, D1–D5)
frost lakeshore350 all                    # All inputs with calibrated temps
frost lakeshore350 display-show <input>   # Show display name for input
frost lakeshore350 display-show-all       # All display names
frost lakeshore350 display-set-name <input> <name>
frost lakeshore350 set-output <N> <pct>   # Set output N (1–4) to percentage
frost lakeshore350 query-output <N>       # Query output N parameters
frost lakeshore350 query-all-outputs      # Query all 4 outputs
frost lakeshore350 outputs-set-params <N> <params...>  # Set HTRSET or ANALOG params
frost lakeshore350 outputs-set-range <N> <range>       # Set output range
frost lakeshore350 raw "<SCPI cmd>"

# Examples
frost lakeshore350 all                   # All stage temperatures
frost lakeshore350 set-output 1 50.0     # Set 4-pump heater to 50%
frost lakeshore350 query-all-outputs     # GL7 output status
```

**Input reference:**

| Input | Sensor | Calibration |
|-------|--------|-------------|
| A | 3-head thermometer | CSV (Ω → K) |
| B | RuOx | LS350 internal curve (K direct) |
| C | 4-head thermometer | CSV (Ω + 34.56Ω offset → K) |
| D2 | Switch diode | CSV (V → K) |
| D3 | 4K stage diode | LS350 curve 21 (K direct) |
| D4 | 3-pump diode | CSV (V → K) |
| D5 | 4-pump diode | CSV (V → K) |

### `frost record-temps` — Continuous Temperature Logging

```bash
frost record-temps [--port-350 PORT] [--port-370 PORT] <command>

# Take a single snapshot and print to stdout
frost record-temps snapshot

# Log continuously at 30-second intervals (Ctrl+C to stop)
frost record-temps loop

# Log at a custom interval (seconds)
frost record-temps loop --interval 60
```

Logs are written to `temps/YYYY-MM-DD_temperature_log.csv`. If a file already exists for the current date, subsequent runs write to `_2.csv`, `_3.csv`, etc. The CSV format is 19 fixed-width space-padded columns covering all LS350 inputs and LS370 input 1.

---

## Data Logging

### Ramp Logs

Written during `frost adr ramp` or `frost adr logging`. Location: `ramps/`

```
ramps/YYYY-MM-DD_ramp_log.csv
ramps/YYYY-MM-DD_ramp_log_2.csv   # if first file exists, auto-increments
```

Columns: `Timestamp, Rate, Current, Voltage, Field, Error Status`
Interval: 1 row per minute

### Temperature Logs

Written during `frost record-temps loop` or via the GUI recording controls. Location: `temps/`

```
temps/YYYY-MM-DD_temperature_log.csv
temps/YYYY-MM-DD_temperature_log_2.csv
```

Columns (19 total, fixed-width space-padded):
`Timestamp, Date, Time, 4K_Stage_Temp, ADR_Res, ADR_Temp, Switch_Volt, Switch_Temp, 3Head_Res, 3Head_Temp, 4Head_Res_Raw, 4Head_Res_Adj, 4Head_Temp, 3Pump_Volt, 3Pump_Temp, 4Pump_Volt, 4Pump_Temp, LS370_In1_Res, LS370_In1_Temp`

Interval: 30 seconds (default), configurable with `--interval`

---

## Dependencies

### cryomech_api

`compressor.rs` uses the `cryomech_api` crate (SMDP V2 protocol) as a **path dependency**. It must be present at `../cryomech_api/` relative to the FROST directory. The `cryomech_api` crate itself requires `smdp` at `../smdp/`.

```bash
# Clone both into the parent directory (sibling folders next to FROST/)
git clone <cryomech_api-repo-url> ../cryomech_api
git clone <smdp-repo-url> ../smdp
```

Expected directory layout:
```
parent/
├── FROST/
├── cryomech_api/
└── smdp/
```

If you do not have the compressor hardware and want to build without it, remove the `cryomech_api` dependency from `Cargo.toml` and the `mod compressor;` declaration from `src/lib.rs` and `src/cli.rs`.

---

## Architecture Notes

**Serial communication:** All devices communicate over serial. Each command opens the port, executes, and closes immediately — there is no persistent connection. SCPI devices (LS625, LS370, LS350) use 7-bit odd parity framing; Zaber uses 6-byte binary frames; the compressor uses SMDP V2 binary protocol via `cryomech_api`.

**Worker thread:** The GUI spawns a background thread for all serial I/O so the render loop is never blocked. Device state is shared via `Arc<Mutex<DeviceSnapshot>>` and polled every 30 seconds.

**Persistent state:** The `state/` directory holds lock files that survive process restarts. The GUI reads these on startup to restore compressor intent and detect interrupted ADR ramps.

**Calibration files:** The LS350 loads calibration CSVs (Ω→K or V→K interpolation tables) lazily on first use for the 3-head, 4-head, and pump diode sensors.

---

## License

Apache 2.0
