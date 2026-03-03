# FROST (Fridge Remote Operations, Status, and Thermometry)

Primary cryostat control software for MEC' (MKID Exoplanet Camera) Prime

## Overview

FROST provides a single interface for controlling and monitoring multiple pieces of cryogenic equipment:

- **Lakeshore 625** - Superconducting Magnet Power Supply
- **Lakeshore 370** - AC Resistance Bridge  
- **Lakeshore 350** - Temperature Controller
- **Heatswitch Driver** - Zaber Stepper Motor Control
- **Cryomech Driver** - Pulse Tube Compressor Control

## Building and Running

```bash
# Build the project
cargo build --release

# Run the application
cargo run --release
```

## Requirements

- Rust 1.70+
- GUI environment (X11 or Wayland)

## Status


## License

APACHE 2.0