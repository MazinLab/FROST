# FROST (Fridge Remote Operations, Status, and Thermometry)

Primary cryostat control software for MEC' (MKID Exoplanet Camera) Prime

## Overview

FROST provides a single interface for controlling and monitoring all cryostat hardware via serial communication. It exposes both a command-line interface for scripting and direct device control, and a desktop GUI for interactive operation.

| Device | Role |
|--------|------|
| **Lakeshore 625** | Superconducting Magnet Power Supply — ramps/de-ramps ADR magnet |
| **Lakeshore 370** | AC Resistance Bridge — monitors device stage temperature, controls PID loop with LS625 |
| **Lakeshore 350** | Temperature Controller — all stage and GL7 thermometers, GL7 heater and switch outputs |
| **Heatswitch** | Zaber T-NM17A04 stepper motor — opens/closes heat switch during ADR ramp |
| **Compressor** | Cryomech compressor — turns PT cooling on/off during cooldown |

---

## Requirements

- Rust 1.70+
- GUI environment (X11 or Wayland) for the GUI mode
- `cryomech_api` crate at `../cryomech_api/` (path dependency — see [Dependencies](#dependencies))

---

## Installation

After building, the binary lives at `target/release/frost`. The recommended installation is a symlink — it survives rebuilds without any extra steps:

```bash
sudo ln -sf $(pwd)/target/release/frost /usr/local/bin/frost
```

Only needs to be run once. Every subsequent `cargo build --release` automatically updates the installed binary. This enables the user to use FROST functionality from any directory without being inside the project directory. 

Verify the installation:
```bash
frost --help
```

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
- Compressor intent is persisted to disk (`state/.compressor_intent`) — if FROST CLI or GUI is restarted, the GUI restores the last known compressor state

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
frost heatswitch close           # Move CCW until motor stalls against mechanical stop (blocking)
frost heatswitch home            # Home the motor
frost heatswitch reset           # Re-home
frost heatswitch stop            # Stop motion
frost heatswitch estop           # Emergency stop (3x STOP)
frost heatswitch move-abs <pos>  # Move to absolute position (microsteps)
frost heatswitch move-rel <steps> # Move relative (microsteps)
frost heatswitch cw <steps>      # Rotate clockwise
frost heatswitch ccw <steps>     # Rotate counter-clockwise
frost heatswitch safe-cw <steps> # Clamped CW move (1–1000 steps)
frost heatswitch safe-ccw <steps> # Clamped CCW move (1–1000 steps)
frost heatswitch move-vel <vel>  # Move at velocity
```

Note: `open` returns immediately — the motor executes asynchronously. Poll status separately to check completion. `close` is blocking: it moves CCW with 4× the standard travel (460800 steps) so the motor always reaches the mechanical stop regardless of starting position, then polls until the position stops changing (stall detected) or a 30-second timeout elapses, then sends STOP.

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
# Note that loop will continuously print out in the terminal and Ctrl+C will end the recording and truncate the .csv. It is recommended to run this command in a tmux pane
frost record-temps loop

# Log at a custom interval (seconds)
frost record-temps loop --interval 60
```

Logs are written to `temps/YYYY-MM-DD_temperature_log.csv`. If a file already exists for the current date, subsequent runs write to `_2.csv`, `_3.csv`, etc. The CSV format is 19 fixed-width space-padded columns covering all LS350 inputs and LS370 input 1.

---

## GL7 Sorption Cooler Cooldown

The `frost gl7` commands automate the cooldown of a Chase Research Cryogenics GL7 two-stage ³He sorption cooler from ~3.8K to ~320mK base temperature. The controller reads temperature data from the CSV written by `frost record-temps loop` and writes output percentages to the Lakeshore 350. **Temperature recording must be running before and during the cooldown.**

### Output map

| Output | Connection | Type | Role |
|--------|-----------|------|------|
| Output 1 | 4-pump heater | Heater range 5 | Heats the ⁴He pump |
| Output 2 | 3-pump heater | Heater range 5 | Heats the ³He pump |
| Analog Output 3 | 4-switch heater | Analog | Opens the ⁴He heat switch |
| Analog Output 4 | 3-switch heater | Analog | Opens the ³He heat switch |

### CLI commands

```bash
frost gl7 <subcommand> --csv <path>
```

All subcommands require `--csv` pointing to the temperature log written by `frost record-temps loop`.

```bash
# Check preconditions before starting a cooldown (Phase 0)
frost gl7 check --csv temps/YYYY-MM-DD_temperature_log.csv

# Ramp both pumps up (Phase 1)
frost gl7 ramp-pumps --csv temps/YYYY-MM-DD_temperature_log.csv

# Stabilize pumps, wait for heads to cool (Phase 2)
# --out1 and --out2 are the output percentages handed off from Phase 1
# (defaults: 25.0% and 18.0% if running Phase 2 standalone)
frost gl7 stabilize --csv temps/YYYY-MM-DD_temperature_log.csv
frost gl7 stabilize --csv temps/... --out1 25.0 --out2 18.0

# Cycle ⁴He module (Phase 3)
# --out2 is the 3-pump output % handed off from Phase 2 (default 18.0%)
frost gl7 cycle-4he --csv temps/YYYY-MM-DD_temperature_log.csv
frost gl7 cycle-4he --csv temps/... --out2 18.0

# Cycle ³He module (Phase 4)
# --out3 is the 4-switch output % handed off from Phase 3 (default 40.0%)
frost gl7 cycle-3he --csv temps/YYYY-MM-DD_temperature_log.csv
frost gl7 cycle-3he --csv temps/... --out3 40.0

# Monitor at base temperature (Phase 5)
# --out3/--out4 are switch heater percentages handed off from Phase 4 (default 40.0% each)
frost gl7 running --csv temps/YYYY-MM-DD_temperature_log.csv
frost gl7 running --csv temps/... --out3 40.0 --out4 40.0

# Run the full automated sequence: Phase 0 → Phase 5
frost gl7 cooldown --csv temps/YYYY-MM-DD_temperature_log.csv
```

### Phase sequence

```
Phase 0: Precondition check  (~instant)
Phase 1: Ramp up both pumps  (~25 min)
Phase 2: Stabilize pumps, heads cool  (~60–90 min)
Phase 3: Cycle ⁴He module  (~60–75 min)
Phase 4: Cycle ³He module  (~25–35 min)
Phase 5: Running at base temperature  (~36 hours)
```

**Total time to base temperature: ~3 hours.** The full sequence can be run unattended with `frost gl7 cooldown`, or each phase can be run individually if manual intervention is needed between steps.

### Phase details

**Phase 0 — Precondition check:** Verifies the system is in the expected cold state before starting. Checks 4K stage < 4.5K, both heat switches OFF (< 10K), both heads < 5K, both pumps < 10K.

**Phase 1 — Ramp up:** Executes a fixed time-based ramp schedule (30%→50%→80% for Output 1, 30%→50%→60% for Output 2 over 90 seconds), then holds at 80%/60% and polls every 30 s. Once the 4-pump reaches 45K, Output 1 is stepped down by 8% per poll until it reaches 25%. Once the 3-pump reaches 42K, Output 2 is stepped down by 8% per poll until it reaches 18%. The two pumps step down independently. Phase 1 exits when both outputs have reached their floors (25% / 18%).

**Phase 2 — Stabilize:** Holds 4-pump at 50–60K and 3-pump at 45–55K using a rate-limited feedback loop (adjustments at most once every 3 minutes per output). Uses rolling averages and dT/dt to avoid reacting to noise. Exits when the **4-head** plateaus below 5.45K, both pumps have been in range for 10+ continuous minutes, and the system has been settled for 5+ minutes. These exit conditions may be changed over time. Timeout exit available after 120 minutes if both heads are below 6.0K. 

**Phase 3 — Cycle ⁴He:** Turns off 4-pump (Output 1 → 0%), opens 4-switch (Output 3 → 40%). Two concurrent control loops run every 30 seconds for the rest of the run:

- **4-switch regulation** (phases 3–5): keeps `Switch_Temp_K` (4-switch temperature) in **20–22K** by adjusting Output 3 in the range 20–45% (−2% if above 22K, +2% if below 20K). If the switch has not reached 20K after 15 minutes a warning is logged; the feedback will keep increasing Output 3 toward 45% until it opens.
- **3-pump management**: Output 2 is boosted without a rate-limit if the 3-pump cools — the switch opening creates a large thermal disturbance that can rapidly cool the 3-pump. Includes **predictive lookahead**: if the rolling-average temperature extrapolated forward would drop below 45K within 1 minute (+8% boost) or 2 minutes (+5% boost), Output 2 is increased pre-emptively while the pump is still in range. Once the pump has already dropped below 45K the response is +8% (falling fast, < −0.3 K/min) or +3% (falling, < −0.1 K/min); below 40K the response is +10% (emergency). Output 2 can reach 100% during this phase.

Exits when both heads fall below 2.0K.

**Phase 4 — Cycle ³He:** Turns off 3-pump (Output 2 → 0%), opens 3-switch (Output 4 → 40%). Output 3 (4-switch) continues to be regulated in the 20–22K range as in Phase 3. No other adjustments are made. Exits when 3-head sustains below 350mK for 5 minutes.

**Phase 5 — Running:** Output 3 (4-switch) continues to be regulated in the 20–22K range. All other outputs are held at their Phase 4 exit values. Monitors every 5 minutes. Exits and alerts when ⁴He is exhausted (4-head > 3K and rising at > 0.01 K/min). Typical hold time: ~36 hours.

### Safety limits

These override all phase logic at every iteration:

| Condition | Action |
|-----------|--------|
| Any pump > 65K | Reduce that pump's output by 20% immediately |
| 4K stage > 12K | Reduce all heater outputs by 10% |
| Phase 2 running > 180 minutes | Log error, halt |
| Any output > 100% | Clamp to 100% |
| Any output < 0% | Clamp to 0% |

### Typical workflow

```bash
# 1. Start temperature recording (run in a tmux pane — must stay running)
frost record-temps loop --interval 30

# 2. In another pane, run the full cooldown
frost gl7 cooldown --csv temps/YYYY-MM-DD_temperature_log.csv
```

Or, to run phases individually with the ability to intervene between steps:

```bash
frost gl7 check      --csv temps/...
frost gl7 ramp-pumps --csv temps/...
frost gl7 stabilize  --csv temps/...
frost gl7 cycle-4he  --csv temps/...
frost gl7 cycle-3he  --csv temps/...
frost gl7 running    --csv temps/...
```

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

`Switch_Temp` (`Switch_Temp_K`) is the **4-switch temperature** (LS350 D2). It is the primary feedback signal for Output 3 regulation in phases 3–5 of the GL7 cooldown.

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
