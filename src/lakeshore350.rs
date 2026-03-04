// lakeshore350.rs — Lake Shore 350 Temperature Controller for FROST
//
// Replicates the functionality of lakeshore350-python/lakeshore350/
// References: temperature.py, lakeshore_display.py, panel_display.py, main.py
//
// Serial settings (per Lakeshore 350 hardware spec and Python driver):
//   57600 baud, 7-bit data, odd parity, 1 stop bit, 2 s timeout
// Command terminator: \n
// Response terminator: \n
//
// Hardware channel map:
//   Input A  — 3-head resistance thermometer (requires calibration in software)
//   Input B  — (empty / spare)
//   Input C  — 4-head resistance thermometer (requires calibration in software)
//   Input D1 — (empty / spare)
//   Input D2 — 4-switch (voltage → temperature via pumps_calibration)
//   Input D3 — 4K stage diode (calibrated directly on Lakeshore, curve 21)
//   Input D4 — 3-pump diode (voltage → temperature via pumps_calibration)
//   Input D5 — 4-pump diode (voltage → temperature via pumps_calibration)
//   Channels 1–8 — mirrors of Input A (not used by FROST)

use serialport::{DataBits, Parity, StopBits};
use std::io::{Read, Write};
use std::time::Duration;

// ── Default connection settings ───────────────────────────────
const DEFAULT_PORT: &str = "/dev/ttyUSB2";
const DEFAULT_BAUD: u32 = 57600;

/// All valid input / channel identifiers on the Lakeshore 350.
pub const ALL_INPUTS: [&str; 8] = ["A", "B", "C", "D1", "D2", "D3", "D4", "D5"];

// ── Controller state ──────────────────────────────────────────
pub struct LakeShore350Controller {
    pub port: String,
    pub baud_rate: u32,

    /// Last error message (shown in GUI / returned to CLI).
    pub error_message: Option<String>,
    /// General query output (shown in GUI output panel / printed by CLI).
    pub output: String,

    // ── GUI input fields (populated as tabs are added) ────────
    /// Selected input channel label (e.g. "A", "D3").
    pub selected_input: String,
}

impl Default for LakeShore350Controller {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT.to_string(),
            baud_rate: DEFAULT_BAUD,
            error_message: None,
            output: String::new(),
            selected_input: "A".to_string(),
        }
    }
}

impl LakeShore350Controller {
    // ── Serial helpers ────────────────────────────────────────

    /// Open port, send `command\n`, read one line response.
    /// Returns the trimmed response string, or an `Err` description.
    fn send_command(&self, command: &str) -> Result<String, String> {
        let mut port = serialport::new(&self.port, self.baud_rate)
            .data_bits(DataBits::Seven)
            .parity(Parity::Odd)
            .stop_bits(StopBits::One)
            .timeout(Duration::from_millis(2000))
            .open()
            .map_err(|e| format!("Failed to open {}: {}", self.port, e))?;

        // Clear stale input
        port.clear(serialport::ClearBuffer::Input).ok();

        // Send command + newline (matches Python: command + '\n')
        port.write_all(format!("{}\n", command).as_bytes())
            .map_err(|e| format!("Write error: {e}"))?;

        // 300 ms settling time (matches Python time.sleep(0.3))
        std::thread::sleep(Duration::from_millis(300));

        // Read response until \n or timeout
        let mut response = String::new();
        let mut byte = [0u8; 1];
        loop {
            match port.read(&mut byte) {
                Ok(1) => {
                    let c = byte[0] as char;
                    if c == '\n' {
                        break;
                    } else if c != '\r' {
                        response.push(c);
                    }
                }
                _ => break,
            }
        }

        Ok(response.trim().to_string())
    }

    /// Open port, send `command\n`, wait 200 ms — no response expected.
    /// Used for write-only commands such as INNAME.
    fn send_write_command(&self, command: &str) -> Result<(), String> {
        let mut port = serialport::new(&self.port, self.baud_rate)
            .data_bits(DataBits::Seven)
            .parity(Parity::Odd)
            .stop_bits(StopBits::One)
            .timeout(Duration::from_millis(2000))
            .open()
            .map_err(|e| format!("Failed to open {}: {}", self.port, e))?;

        port.clear(serialport::ClearBuffer::Input).ok();

        port.write_all(format!("{}\n", command).as_bytes())
            .map_err(|e| format!("Write error: {e}"))?;

        // 200 ms settling time (matches Python time.sleep(0.2))
        std::thread::sleep(Duration::from_millis(200));

        Ok(())
    }

    // ── Identity / info ───────────────────────────────────────

    /// `*IDN?` — device identification string.
    /// Equivalent to `lakeshore350 --info`.
    pub fn get_identification(&mut self) {
        match self.send_command("*IDN?") {
            Ok(r) if !r.is_empty() => {
                self.output = format!("Device Information:\n  {r}\n");
                self.error_message = None;
            }
            Ok(_) => self.error_message = Some("No response to *IDN?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
    }

    // ── Front panel display — INNAME ──────────────────────────

    /// `INNAME? <input>` — get the front panel label for one input.
    ///
    /// Valid inputs: A, B, C, D1, D2, D3, D4, D5.
    /// Puts the result in `self.output`.
    /// Equivalent to `lakeshore350 --display-show <input>`.
    pub fn get_display_name(&mut self, input: &str) {
        let input = input.to_uppercase();
        if !ALL_INPUTS.contains(&input.as_str()) {
            self.error_message = Some(format!(
                "Invalid input '{}'. Must be one of: {}",
                input,
                ALL_INPUTS.join(", ")
            ));
            return;
        }
        match self.send_command(&format!("INNAME? {input}")) {
            Ok(r) => {
                let name = if r.is_empty() { "(no name set)" } else { &r };
                self.output = format!("Input {input} display name: {name}\n");
                self.error_message = None;
            }
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `INNAME? <input>` for every input (A, B, C, D1–D5).
    ///
    /// Queries each input in turn and collects the results into `self.output`.
    /// Equivalent to `lakeshore350 --display-show-all`.
    pub fn get_all_display_names(&mut self) {
        let mut out = String::from("Front panel display names:\n");
        let mut last_err: Option<String> = None;

        for &inp in &ALL_INPUTS {
            match self.send_command(&format!("INNAME? {inp}")) {
                Ok(r) => {
                    let name = if r.is_empty() { "(no name set)" } else { &r };
                    out.push_str(&format!("  Input {inp}: {name}\n"));
                }
                Err(e) => {
                    out.push_str(&format!("  Input {inp}: ERROR ({e})\n"));
                    last_err = Some(e);
                }
            }
        }

        self.output = out;
        self.error_message = last_err;
    }

    /// `INNAME <input>,"<name>"` — set the front panel label for one input.
    ///
    /// Valid inputs: A, B, C, D1, D2, D3, D4, D5.
    /// Name is free text (no quotes needed by the caller).
    /// Equivalent to `lakeshore350 --display-set-name <input> <name>`.
    pub fn set_display_name(&mut self, input: &str, name: &str) -> Result<(), String> {
        let input = input.to_uppercase();
        if !ALL_INPUTS.contains(&input.as_str()) {
            return Err(format!(
                "Invalid input '{}'. Must be one of: {}",
                input,
                ALL_INPUTS.join(", ")
            ));
        }
        // The Lakeshore 350 expects: INNAME <input>,"<name>"
        self.send_write_command(&format!("INNAME {input},\"{name}\""))?;
        Ok(())
    }

    // ── Raw command ───────────────────────────────────────────

    /// Send an arbitrary command string and put the response in `output`.
    /// Equivalent to `lakeshore350 --raw-command <command>`.
    pub fn raw_command(&mut self, command: &str) {
        match self.send_command(command) {
            Ok(r) => {
                self.output = format!(
                    ">> {command}\n{}\n",
                    if r.is_empty() { "(no response)" } else { &r }
                );
                self.error_message = None;
            }
            Err(e) => self.error_message = Some(e),
        }
    }
}
