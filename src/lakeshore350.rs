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
//   Input B  — ADR thermometer (resistance → Kelvin via KRDG?)
//   Input C  — 4-head resistance thermometer (requires calibration in software)
//   Input D1 — (empty / spare)
//   Input D2 — 4K stage diode (calibrated on Lakeshore, KRDG? returns Kelvin)
//   Input D3 — (spare)
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

    // ── Temperature / sensor readings ─────────────────────────

    /// `SRDG? <input>` — raw sensor reading.
    /// Returns the numeric string (Ω for resistive inputs, V for diodes),
    /// "R_OVER" on overrange, or "NO_RESPONSE" if the device is silent.
    /// Mirrors Python `read_sensor()` in temperature.py.
    fn read_sensor_raw(&self, input: &str) -> Result<String, String> {
        let r = self.send_command(&format!("SRDG? {input}"))?;
        if r.is_empty() {
            return Ok("NO_RESPONSE".to_string());
        }
        // Overrange indicators (mirrors Python length / garbage-char check)
        if r.len() > 15 || r.contains('`') || r.contains('\x00') {
            return Ok("R_OVER".to_string());
        }
        let up = r.to_uppercase();
        if up.contains("OVER") || up.contains("R.") || up.contains("R_") {
            return Ok("R_OVER".to_string());
        }
        Ok(r)
    }

    /// `RDGST? <input>` + `KRDG? <input>` — temperature in Kelvin.
    /// Returns the numeric K string, "T_OVER", "NO_RESPONSE", or an `Err`.
    /// Mirrors Python `read_temperature()` in temperature.py.
    fn read_kelvin_raw(&self, input: &str) -> Result<String, String> {
        // Check overrange status first (bit 32 set → T_OVER)
        if let Ok(status) = self.send_command(&format!("RDGST? {input}")) {
            if let Ok(code) = status.trim().parse::<u32>() {
                if code & 32 != 0 {
                    return Ok("T_OVER".to_string());
                }
            }
        }
        let r = self.send_command(&format!("KRDG? {input}"))?;
        if r.is_empty() {
            return Ok("NO_RESPONSE".to_string());
        }
        if r.len() > 15 || r.contains('`') || r.contains('\x00') {
            return Ok("T_OVER".to_string());
        }
        let up = r.to_uppercase();
        if up.contains("OVER") || up.contains("T.") || up.contains("T_") {
            return Ok("T_OVER".to_string());
        }
        // Zero on D-type inputs indicates overrange (mirrors Python)
        let inp_up = input.to_uppercase();
        if let Ok(v) = r.parse::<f64>() {
            if v == 0.0 && matches!(inp_up.as_str(), "D2" | "D3" | "D4" | "D5") {
                return Ok("T_OVER".to_string());
            }
        }
        Ok(r)
    }

    /// `SRDG? <input>` — get raw sensor reading into `self.output`.
    pub fn get_sensor(&mut self, input: &str) {
        let input = input.to_uppercase();
        if !ALL_INPUTS.contains(&input.as_str()) {
            self.error_message = Some(format!(
                "Invalid input '{}'. Must be one of: {}",
                input, ALL_INPUTS.join(", ")
            ));
            return;
        }
        match self.read_sensor_raw(&input) {
            Ok(r) => {
                let unit = sensor_unit(&input);
                self.output = if let Ok(v) = r.parse::<f64>() {
                    format!("Input {input}: {v:.4} {unit}\n")
                } else {
                    format!("Input {input}: {r}\n")
                };
                self.error_message = None;
            }
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `KRDG? <input>` (with `RDGST?` check) — get temperature in Kelvin into `self.output`.
    pub fn get_kelvin(&mut self, input: &str) {
        let input = input.to_uppercase();
        if !ALL_INPUTS.contains(&input.as_str()) {
            self.error_message = Some(format!(
                "Invalid input '{}'. Must be one of: {}",
                input, ALL_INPUTS.join(", ")
            ));
            return;
        }
        match self.read_kelvin_raw(&input) {
            Ok(r) => {
                self.output = if let Ok(v) = r.parse::<f64>() {
                    format!("Input {input}: {v:.4} K\n")
                } else {
                    format!("Input {input}: {r}\n")
                };
                self.error_message = None;
            }
            Err(e) => self.error_message = Some(e),
        }
    }

    /// Read B (ADR) and D2 (4K stage) and print sensor + Kelvin for each.
    /// Mirrors the core of `lakeshore350 --all` for these two inputs.
    pub fn get_all_readings(&mut self) {
        let mut out = String::new();

        // ── Input B (ADR) ─────────────────────────────────────
        let b_raw = self.read_sensor_raw("B")
            .unwrap_or_else(|e| format!("ERROR ({e})"));
        let b_kelvin = self.read_kelvin_raw("B")
            .unwrap_or_else(|e| format!("ERROR ({e})"));
        let b_raw_str = if let Ok(v) = b_raw.parse::<f64>() {
            format!("{v:.4} Ω")
        } else {
            b_raw
        };
        let b_k_str = if let Ok(v) = b_kelvin.parse::<f64>() {
            format!("{v:.4} K")
        } else {
            b_kelvin
        };
        out.push_str(&format!("Input B  (ADR):      {b_raw_str} → {b_k_str}\n"));

        // ── Input D2 (4K stage) ───────────────────────────────
        let d2_raw = self.read_sensor_raw("D2")
            .unwrap_or_else(|e| format!("ERROR ({e})"));
        let d2_kelvin = self.read_kelvin_raw("D2")
            .unwrap_or_else(|e| format!("ERROR ({e})"));
        let d2_raw_str = if let Ok(v) = d2_raw.parse::<f64>() {
            format!("{v:.4} V")
        } else {
            d2_raw
        };
        let d2_k_str = if let Ok(v) = d2_kelvin.parse::<f64>() {
            format!("{v:.4} K")
        } else {
            d2_kelvin
        };
        out.push_str(&format!("Input D2 (4K stage): {d2_raw_str} → {d2_k_str}\n"));

        self.output = out;
        self.error_message = None;
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

// ── Helpers ───────────────────────────────────────────────────

/// Returns the expected SI unit for `SRDG?` on a given input.
/// A, B, C, D1 are resistance thermometers → Ω.
/// D2–D5 are diodes / voltage sensors → V.
fn sensor_unit(input: &str) -> &'static str {
    match input.to_uppercase().as_str() {
        "A" | "B" | "C" | "D1" => "Ω",
        _ => "V",
    }
}
