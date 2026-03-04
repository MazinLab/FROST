# FROST (Fridge Remote Operations, Status, and Thermometry)

Primary cryostat control software for MEC' (MKID Exoplanet Camera) Prime

## Overview

FROST provides a single interface for controlling and monitoring all cryostat hardware

- **Lakeshore 625** - Superconducting Magnet Power Supply - controls ADR
- **Lakeshore 370** - AC Resistance Bridge - monitors device stage temperature 
- **Lakeshore 350** - Temperature Controller - all stage and gl7 thermometers 
- **Heatswitch Driver** - Zaber Stepper Motor Control - opens and closes heatswitch
- **Cryomech Driver** - Pulse Tube Compressor Control - monitors cryomech compressor 

## Building and Running

```bash
# Build the project
cargo build --release

# Run the application (GUI)
cargo run --release

# Run a CLI command directly
cargo run --release -- <device> <command>

```

## Installation (adding `frost` to your PATH)

After building in release mode the binary lives at `target/release/frost`.
The recommended way to install is a symlink — it requires no extra steps after
rebuilding and is immediately available system-wide:

```bash
sudo ln -sf /home/kids/FROST/target/release/frost /usr/local/bin/frost
```

This only needs to be run once. After any subsequent `cargo build --release`
the symlink automatically points to the updated binary.

Verify the installation, and get a list of all available commands:
```bash
frost --help
```

Example command to read input 2 on lakeshore370: 
```bash
frost lakeshore370 all 1
```

## Requirements

- Rust 1.70+
- GUI environment (X11 or Wayland)

## Dependencies

### cryomech_api (required for `compressor.rs`)

`compressor.rs` uses the `cryomech_api` crate for serial communication with the
Cryomech pulse tube compressor. It is referenced as a **path dependency**, so the
crate must be present at `../cryomech_api` relative to the FROST directory (i.e.
a sibling folder in the same parent directory).

To get it:
```bash
# Clone the cryomech_api repo into the parent directory (next to FROST/)
git clone <cryomech_api-repo-url> ../cryomech_api

# Or, if you already have it locally, ensure the folder is at:
#   <parent-of-FROST>/cryomech_api/
```

`cryomech_api` itself requires the `smdp` crate at `../smdp`. Both must be present
for FROST to compile with compressor support.

If you do **not** have the compressor hardware and want to build without it,
remove or comment out the `cryomech_api` dependency in `Cargo.toml` and the
`mod compressor;` line in `src/gui.rs`.

## GUI

To launch the interactive gui, we execute: 
```bash
frost gui
``` 

Or, to compile a fresh version of the gui: 
```bash
cargo run --release -- gui
```

## License

APACHE 2.0