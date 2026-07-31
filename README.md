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

After building, the binary lives at `target/release/frost`. The recommended installation is a symlink because it survives rebuilds without any extra steps:

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

All device ports have hardcoded defaults that match the MEC Prime set-up. They can be overridden per-command with `--port` and `--baud` flags.

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

There are no config files. If you need permanent port reassignment, update the `DEFAULT_PORT` constants in the relevant `src/<device>.rs` file.

### Busy-port retry behavior

If a serial port is temporarily held by another process, FROST retries the open before failing:

- **SCPI ports** (Lakeshore devices): up to 10 retries, 15 seconds apart (~2.5 minutes max)
- **Zaber port** (heatswitch): up to 2 retries, 15 seconds apart (~30 seconds max)

If retries are exhausted, the command fails with an error naming the port.

---

## GUI

Launch the interactive GUI:
```bash
frost gui
# or, to build and run in one step:
cargo run --release -- gui
```

The GUI runs a background worker thread that polls all devices every 30 seconds (staggered to avoid serial bus contention) while keeping the interface responsive. All serial I/O happens off the render thread.

### Layout

The GUI is divided into five sections displayed top-to-bottom. A fixed status bar sits above the scrollable content and is always visible, and a safety toggle sits at the top of the scrollable area.

**Safety Toggle**
- **Safety: ON / OFF** button at the top of the window — green when interlocks are active, red when bypassed
- Clicking it flips and persists the state (shared with the CLI, survives restarts)
- While OFF, a red **"⚠ Safety OFF — start interlocks are bypassed"** banner is shown
- See [Safety Interlocks](#safety-interlocks) for what the interlocks gate

**Status Bar**
- Always-visible strip at the very top showing a one-line system summary
- Five indicator chips: **Compressor**, **ADR Ramp**, **GL7**, **Recording**, **Heatswitch**
- Chip is green (active) or dark/muted (inactive)
- GL7 is considered active if any heater/switch output is non-zero or a cooldown subprocess is alive
- Heatswitch is considered active when open

**Thermometry**
- Live temperature readings from all LS350 inputs (4K Stage, ADR, 4-Switch, 3-Head, 4-Head, 3-Pump, 4-Pump) and LS370 input 1 (Device Stage)
- Inputs using custom calibration (3-head, 4-head, pump and switch diodes) show calibrated temperatures in Kelvin
- **Record Temperatures** button starts continuous CSV logging to `temps/YYYY-MM-DD_temperature_log.csv` at 30-second intervals; button turns red and reads "Stop Recording Temperatures" while active
- Recording state persists across restarts (`temps/.recording_active` lock file). The GUI detects an interrupted recording on startup and resumes it automatically

**Compressor**
- Start/Stop buttons with live status display (running state, error flags)
- Compressor intent is persisted to disk (`state/.compressor_intent`). If FROST is restarted, the GUI restores the last known compressor state

**ADR Cooldown**
- Live readouts: output current (A), voltage (V), magnetic field (T)
- Input fields for ramp rate (A/s), target current (A), and soak time (minutes)
- **Start ADR Cooldown** launches the full automated ramp sequence; button turns red and shows elapsed time while the ramp is running
- Live log output displayed in the GUI during the ramp
- Ramp state persisted to `state/.adr_ramp_running` to detect interrupted ramps on restart
- **Crash recovery:** if the GUI starts and finds a stale ramp lock file (subprocess died), it reads the LS625 current and automatically ramps to 0 A if the magnet is still energized above 0.5 A

**GL7 Sorption Cooler**
- **Start GL7 Cooldown** launches the full automated GL7 cooldown sequence (`frost gl7 cooldown`) as a subprocess; button turns red and reads "GL7 Cooldown In-Progress" until the process exits
- CSV path field: paste the path to the active temperature recording, or click **Current Temperature Recording** to auto-fill from the running recording
- Live output percentage display for all four outputs (4-Pump Heater, 3-Pump Heater, 4-Switch Heater, 3-Switch Heater)
- DragValue + **Set** buttons to manually override any output percentage

---

## CLI Reference

```
frost <device> [OPTIONS] <command> [ARGS]
```

Run `frost --help` for the full list of devices and commands, or `frost <device> --help` for device-specific options.

### Common Device Commands

| Device | Common commands | Notes |
|--------|----------------|-------|
| `frost adr` | `ramp <rate> <current> [--soak-mins N]`, `logging` | Full automated ADR ramp sequence |
| `frost compressor` | `status`, `start`, `stop`, `all` | Cryomech compressor control |
| `frost lakeshore625` | `current`, `set-current <A>`, `set-rate <A/s>`, `quench-status`, `raw` | Magnet power supply |
| `frost lakeshore370` | `kelvin <input>`, `resistance <input>`, `set-heater <pct>`, `raw` | Device stage thermometry |
| `frost lakeshore350` | `all`, `read <input>`, `set-output <N> <pct>`, `query-all-outputs`, `raw` | Stage thermometry + GL7 outputs |
| `frost heatswitch` | `open`, `close`, `home`, `stop`, `estop` | Zaber stepper motor |
| `frost record-temps` | `snapshot`, `loop [--interval N]` | Temperature CSV logging |
| `frost safety` | `status`, `on`, `off` | See [Safety Interlocks](#safety-interlocks) |

All Lakeshore devices accept `--port` and `--baud` overrides. The compressor also accepts `--addr`, the heatswitch accepts `--device-id`.

### `frost adr ramp` — ADR ramp sequence

```bash
frost adr ramp 0.005 9.44 --soak-mins 60
```

The ramp command runs the full automated sequence: start background logger → set ramp rate and target current → wait for current to reach target (within 0.04 A) → soak at constant current → open heat switch + 3 min wait → ramp to 0 A → stop logger.

### `frost heatswitch` — open/close behavior

`open` returns immediately — the motor executes asynchronously. `close` is blocking: it drives CCW until the home limit switch fires (position freezes at 0), polling until position stops changing or a 60-second timeout elapses.

### `frost lakeshore350` — input reference

| Input | Sensor | Calibration |
|-------|--------|-------------|
| A | 3-head thermometer | CSV (Ω → K) |
| C | 4-head thermometer | CSV (Ω + 34.56Ω offset → K) |
| D2 | 4-switch diode | CSV (V → K) |
| D3 | 4K stage diode | LS350 curve 21 (K direct) |
| D4 | 3-pump diode | CSV (V → K) |
| D5 | 4-pump diode | CSV (V → K) |

### `frost record-temps` — temperature logging

```bash
frost record-temps loop              # Log at 30 s intervals (run in tmux)
frost record-temps loop --interval 60
frost record-temps snapshot          # Single reading to stdout
```

Logs are written to `temps/YYYY-MM-DD_temperature_log.csv` (17 fixed-width space-padded columns covering all LS350 inputs and LS370 input 1 device stage). Ctrl+C ends the recording. It is recommended to run this in a tmux pane if using CLI instead of the GUI.

---

## GL7 Sorption Cooler Cooldown

The `frost gl7` commands automate the cooldown of the Chase Research Cryogenics GL7 two-stage helium-3/helium-4 sorption cooler from 4K stage temperature (~3.6K) to ~310mK base temperature. The controller reads temperature data from the CSV written by `frost record-temps loop` and writes output percentages to the Lakeshore 350. **Temperature recording must be running before and during the cooldown.**

### Output map

| Output | Connection | Type | Role |
|--------|-----------|------|------|
| Output 1 | 4-pump heater | Heater range 5 | Heats the ⁴He pump |
| Output 2 | 3-pump heater | Heater range 5 | Heats the ³He pump |
| Analog Output 3 | 4-switch heater | Analog | Opens the ⁴He heat switch |
| Analog Output 4 | 3-switch heater | Analog | Opens the ³He heat switch |

### CLI commands

All subcommands require `--csv` pointing to the temperature log written by `frost record-temps loop`. Run `frost gl7 --help` for the full list of options and per-phase flags.

```bash
# Run the full automated sequence (Phase 0 → 5)
frost gl7 cooldown --csv temps/YYYY-MM-DD_temperature_log.csv

# Or run individual phases (each accepts --outN flags for manual handoff)
frost gl7 check      --csv temps/...   # Phase 0: verify outputs at 0%
frost gl7 ramp-pumps --csv temps/...   # Phase 1: ramp up both pumps
frost gl7 stabilize  --csv temps/...   # Phase 2: hold pumps, wait for heads to cool
frost gl7 cycle-4he  --csv temps/...   # Phase 3: cycle ⁴He module
frost gl7 cycle-3he  --csv temps/...   # Phase 4: cycle ³He module
frost gl7 running    --csv temps/...   # Phase 5: monitor at base temperature
```

### Phase sequence

```
Phase 0: Output check  (~instant)
Phase 1: Ramp up both pumps  (~25 min)
Phase 2: Stabilize pumps, heads cool  (~60–90 min)
Phase 3: Cycle ⁴He module  (~60–75 min)
Phase 4: Cycle ³He module  (~25–35 min)
Phase 5: Running at base temperature  (~36 hours)
```

**Total time to base temperature: ~3 hours.** The full sequence can be run unattended with `frost gl7 cooldown`, or each phase can be run individually if manual intervention is needed between steps.

### Phase details

**Phase 0 — Output check:** Verifies all LS350 outputs are at 0% before starting. Thermal preconditions are enforced by the [safety interlocks](#safety-interlocks) (compressor running + 4K stage < 4.2K) at the point the cooldown is launched.

**Phase 1 — Ramp up:** Executes a fixed time-based ramp schedule (30%→50%→80% for Output 1, 30%→50%→60% for Output 2 over 90 seconds), then holds at 80%/60% and polls every 30 s. Once the 4-pump reaches 45K, Output 1 is stepped down by 8% per poll until it reaches 25%. Once the 3-pump reaches 42K, Output 2 is stepped down by 8% per poll until it reaches 18%. The two pumps step down independently. Phase 1 exits when both outputs have reached their floors (25% / 18%).

The 3-pump step-down starts at **42K**, which is 3K below the target band of 45–55K. This is intentional overshoot prevention: at 60% output the pump is rising at several K/min, so waiting until 45K to begin stepping down would result in the pump coasting well past 55K before the reduced output takes effect. Starting at 42K gives the step-down ~3K of runway to arrest the rise before the pump enters the target range. The same principle applies to the 4-pump, whose step-down begins at 45K — 15K below its 60K upper limit.

**Phase 2 — Stabilize:** Holds 4-pump at 50–60K and 3-pump at 45–55K using a rate-limited feedback loop (adjustments at most once every 3 minutes per output). Uses rolling averages and dT/dt to avoid reacting to noise. Exits when the **4-head** plateaus below 5.45K and both pumps have been in range for 10+ continuous minutes. Timeout exit available after 120 minutes if both heads are below 6.0K.

**Phase 3 — Cycle ⁴He:** Turns off 4-pump (Output 1 → 0%), opens 4-switch (Output 3 → 40%). Two concurrent control loops run every 30 seconds for the rest of the run:

- **4-switch regulation** (phases 3–5): keeps `Switch_Temp_K` (4-switch temperature) in **20–22K** by adjusting Output 3 in the range 20–45% (−5% if above 22K, +5% if below 20K). If the switch has not reached 20K after 15 minutes a warning is logged; the feedback will keep increasing Output 3 toward 45% until it opens.
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

**GUI:** Click **Record Temperatures** to start logging, then in the GL7 section click **Current Temperature Recording** to auto-fill the CSV path and **Start GL7 Cooldown**.

**CLI:** Start temperature recording in one tmux pane (`frost record-temps loop`), then run `frost gl7 cooldown --csv temps/...` in another. Individual phases can be run separately if manual intervention is needed between steps.

---

## Safety Interlocks

FROST enforces start-safety interlocks before any potentially dangerous operation is **started**. Two preconditions must both hold:

1. The **compressor is running**.
2. The **4K stage** (LS350 input D3) is **below 4.2 K** (a reading ≥ 4.2 K blocks).

### What is gated

| Operation | CLI | GUI |
|-----------|-----|-----|
| ADR ramp | `frost adr ramp …` | **Start ADR Cooldown** button |
| GL7 full cooldown | `frost gl7 cooldown …` | **Start GL7 Cooldown** button |
| GL7 cold-start ramp (Phase 1) | `frost gl7 ramp-pumps …` | — |
| Manual LS350 output ON | `frost lakeshore350 set-output <N> <pct>` (pct > 0), `outputs-set-range <N> <range>` (range ≠ 0) | GL7 output **Set** buttons (percentage > 0) |

**Not gated:** read-only queries; turning an output **off** (0% / range 0); `frost adr logging`; and the mid-sequence GL7 phases (`stabilize`, `cycle-4he`, `cycle-3he`, `running`), which resume an already-started cooldown where the 4K stage is expected to be above 4.2 K.

### Key behaviors

- **Start-only.** The interlock is checked **once**, at the moment a process starts. It is never re-evaluated during a run — if the 4K stage rises above 4.2 K while an ADR ramp or GL7 cooldown is in progress, the process is **not** stopped.
- **Fail-safe.** If a required reading cannot be obtained (serial error, sensor overrange, wrong port), the start is **blocked**, not allowed.
- **Persistent override.** Safety defaults to **ON** and can be toggled OFF via `frost safety off` (CLI) or the **Safety: ON / OFF** button in the GUI. The state is stored in `$HOME/.frost/safety_disabled`, shared between CLI and GUI, and survives restarts. Use `frost safety status` to check the current state; `frost safety on` to re-enable.
- **Failure audit log.** Every blocked start is announced on stderr and appended with a timestamp to `logs/safety_log.txt`. A blocked CLI command exits with an error naming the failed condition(s).

---

## Data Logging

Both log types auto-increment filenames (`_2.csv`, `_3.csv`, …) when a file for the current date already exists.

| Log | Location | Written by | Interval |
|-----|----------|-----------|----------|
| Ramp | `ramps/YYYY-MM-DD_ramp_log.csv` | `frost adr ramp` / `frost adr logging` | 1 row/min |
| Temperature | `temps/YYYY-MM-DD_temperature_log.csv` | `frost record-temps loop` / GUI recording | 30 s (configurable) |
| Safety | `logs/safety_log.txt` | Safety interlocks | On blocked starts only |

**Temperature CSV columns** (17 total, fixed-width space-padded):
`Timestamp, Date, Time, 4K_Stage_Temp, Switch_Volt, Switch_Temp, 3Head_Res, 3Head_Temp, 4Head_Res_Raw, 4Head_Res_Adj, 4Head_Temp, 3Pump_Volt, 3Pump_Temp, 4Pump_Volt, 4Pump_Temp, Device_Stage_Res, Device_Stage_Temp`

`Switch_Temp` (`Switch_Temp_K`) is the **4-switch temperature** (LS350 D2)

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
