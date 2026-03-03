# FROST

**Fridge Remote Operations, Software, and Thermometry**

Cryostat control for MEC' Prime 

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

🚧 **Early Development** - Basic GUI framework implemented, hardware integrations coming soon.

## License

MIT License - See LICENSE file for details.