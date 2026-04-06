// lib.rs — FROST library root (exposes modules for integration testing)
//
// The binary entrypoint is src/main.rs. This lib target exists solely to
// allow the tests/ directory to import FROST modules without duplicating code.
// Only modules needed for testing are re-exported here.

pub mod cli;
pub mod gui;
pub mod serial;
pub mod compressor;
pub mod heatswitch;
pub mod lakeshore625;
pub mod lakeshore350;
pub mod lakeshore370;
pub mod record_temps;
pub mod adr_ramping;
pub mod gl7_automation;
pub mod worker;
