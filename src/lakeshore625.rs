// lakeshore625.rs — Lake Shore 625 Superconducting Magnet Power Supply controller for FROST
//
// Replicates the functionality of lakeshore625-python/lakeshore625/
// References: power_controller.py, main.py
//
// Serial settings (per Lakeshore 625 hardware spec):
//   9600 baud, 7-bit data, odd parity, 1 stop bit, 2 s timeout
// Command terminator: \r\n
// Response terminator: \n

use serialport::{DataBits, Parity, StopBits};
use std::io::{Read, Write};
use std::time::Duration;

// ── Default connection settings ──────────────────────────────
const DEFAULT_PORT: &str = "/dev/ttyUSB0";
const DEFAULT_BAUD: u32 = 9600;

// ── Validation limits (from Lakeshore 625 manual) ────────────
pub const CURRENT_LIMIT_MAX: f64 = 60.1;
pub const VOLTAGE_LIMIT_MIN: f64 = 0.1;
pub const VOLTAGE_LIMIT_MAX: f64 = 5.0;
pub const RATE_LIMIT_MIN: f64 = 0.0001;
pub const RATE_LIMIT_MAX: f64 = 99.999;

// ── Controller state ─────────────────────────────────────────
pub struct LakeShore625Controller {
    pub port: String,
    pub baud_rate: u32,

    /// Last error shown in the GUI.
    pub error_message: Option<String>,
    /// General query output shown in the output panel.
    pub output: String,

    // ── GUI input fields (for ADR tab controls) ───────────────
    /// Target current for SETI (A).
    pub target_current: f64,
    /// Ramp rate for RATE (A/s).
    pub ramp_rate: f64,
    /// Compliance voltage for SETV (V).
    pub compliance_voltage: f64,
    /// Max current limit for LIMIT (A).
    pub current_limit: f64,
    /// Max voltage limit for LIMIT (V).
    pub voltage_limit: f64,
    /// Max rate limit for LIMIT (A/s).
    pub rate_limit: f64,
    /// Quench step limit for QNCH (A/s).
    pub quench_step_limit: f64,
}

impl Default for LakeShore625Controller {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT.to_string(),
            baud_rate: DEFAULT_BAUD,
            error_message: None,
            output: String::new(),
            target_current: 0.0,
            ramp_rate: 0.01,
            compliance_voltage: 1.0,
            current_limit: 10.0,
            voltage_limit: 1.0,
            rate_limit: 0.1,
            quench_step_limit: 0.05,
        }
    }
}

impl LakeShore625Controller {
    // ── Serial connection ────────────────────────────────────
    /// Open a serial connection, send a command with CRLF, read back one line.
    /// Returns the stripped response string, or an error message.
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

        // Send command + CRLF
        port.write_all(format!("{}\r\n", command).as_bytes())
            .map_err(|e| format!("Write error: {e}"))?;

        // 200 ms settling time (matches Python time.sleep(0.2))
        std::thread::sleep(Duration::from_millis(200));

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

    // ── Identity / info ──────────────────────────────────────

    /// `*IDN?` — device identification string.
    pub fn get_identification(&mut self) {
        match self.send_command("*IDN?") {
            Ok(r) if !r.is_empty() => { self.output = format!("ID: {r}"); self.error_message = None; }
            Ok(_) => self.error_message = Some("No response to *IDN?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `BAUD?` — baud rate code (0=9600, 1=19200, 2=38400, 3=57600).
    pub fn get_baud_rate(&mut self) {
        match self.send_command("BAUD?") {
            Ok(r) if !r.is_empty() => {
                let readable = match r.as_str() {
                    "0" => "9600",
                    "1" => "19200",
                    "2" => "38400",
                    "3" => "57600",
                    other => other,
                };
                self.output = format!("Baud rate: {readable} (code: {r})");
                self.error_message = None;
            }
            Ok(_) => self.error_message = Some("No response to BAUD?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
    }

    // ── Readings ─────────────────────────────────────────────

    /// `RDGF?` — magnetic field in Tesla.
    pub fn get_field(&mut self) -> Result<String, String> {
        let r = self.send_command("RDGF?")?;
        if r.is_empty() { return Err("No response to RDGF?".to_string()); }
        Ok(r)
    }

    /// `RDGI?` — output current in Amps.
    pub fn get_current(&mut self) -> Result<String, String> {
        let r = self.send_command("RDGI?")?;
        if r.is_empty() { return Err("No response to RDGI?".to_string()); }
        Ok(r)
    }

    /// `RDGV?` — output voltage in Volts.
    pub fn get_voltage(&mut self) -> Result<String, String> {
        let r = self.send_command("RDGV?")?;
        if r.is_empty() { return Err("No response to RDGV?".to_string()); }
        Ok(r)
    }

    /// Read field, current, and voltage into `output`.
    pub fn get_all_readings(&mut self) {
        let mut out = String::new();
        match self.send_command("RDGF?") {
            Ok(r) if !r.is_empty() => out.push_str(&format!("Field:   {} T\n", r)),
            Ok(_)  => out.push_str("Field:   NO_RESPONSE\n"),
            Err(e) => out.push_str(&format!("Field:   ERROR ({})\n", e)),
        }
        match self.send_command("RDGI?") {
            Ok(r) if !r.is_empty() => out.push_str(&format!("Current: {} A\n", r)),
            Ok(_)  => out.push_str("Current: NO_RESPONSE\n"),
            Err(e) => out.push_str(&format!("Current: ERROR ({})\n", e)),
        }
        match self.send_command("RDGV?") {
            Ok(r) if !r.is_empty() => out.push_str(&format!("Voltage: {} V\n", r)),
            Ok(_)  => out.push_str("Voltage: NO_RESPONSE\n"),
            Err(e) => out.push_str(&format!("Voltage: ERROR ({})\n", e)),
        }
        self.output = out;
        self.error_message = None;
    }

    // ── Current setpoint ─────────────────────────────────────

    /// `SETI <current>` — set target output current (A).
    pub fn set_current(&mut self, current: f64) -> Result<(), String> {
        self.send_command(&format!("SETI {current}"))?;
        Ok(())
    }

    // ── Ramp rate ────────────────────────────────────────────

    /// `RATE?` — get current ramp rate (A/s) into `output`.
    pub fn get_ramp_rate(&mut self) {
        match self.send_command("RATE?") {
            Ok(r) if !r.is_empty() => { self.output = format!("Ramp rate: {} A/s", r); self.error_message = None; }
            Ok(_) => self.error_message = Some("No response to RATE?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `RATE <rate>` — set ramp rate (A/s).
    pub fn set_ramp_rate(&mut self, rate: f64) -> Result<(), String> {
        self.send_command(&format!("RATE {rate}"))?;
        Ok(())
    }

    // ── Ramp control ─────────────────────────────────────────

    /// `RAMP` — start current ramp.
    pub fn start_ramp(&mut self) -> Result<(), String> {
        self.send_command("RAMP")?;
        Ok(())
    }

    /// `STOP` — stop / pause current ramp.
    pub fn stop_ramp(&mut self) -> Result<(), String> {
        self.send_command("STOP")?;
        Ok(())
    }

    // ── Compliance voltage ───────────────────────────────────

    /// `SETV?` — get compliance voltage limit into `output`.
    pub fn get_compliance_voltage(&mut self) {
        match self.send_command("SETV?") {
            Ok(r) if !r.is_empty() => { self.output = format!("Compliance voltage: {} V", r); self.error_message = None; }
            Ok(_) => self.error_message = Some("No response to SETV?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `SETV <voltage>` — set compliance voltage limit (0.1–5.0 V).
    pub fn set_compliance_voltage(&mut self, voltage: f64) -> Result<(), String> {
        if !(VOLTAGE_LIMIT_MIN..=VOLTAGE_LIMIT_MAX).contains(&voltage) {
            return Err(format!("Compliance voltage must be {VOLTAGE_LIMIT_MIN}–{VOLTAGE_LIMIT_MAX} V, got {voltage}"));
        }
        self.send_command(&format!("SETV {voltage}"))?;
        Ok(())
    }

    // ── Limits ───────────────────────────────────────────────

    /// `LIMIT?` — get all max limits into `output`.
    pub fn get_limits(&mut self) {
        match self.send_command("LIMIT?") {
            Ok(r) if !r.is_empty() => {
                let parts: Vec<&str> = r.splitn(3, ',').collect();
                if parts.len() == 3 {
                    self.output = format!(
                        "Current limit: {} A\nVoltage limit: {} V\nRate limit:    {} A/s",
                        parts[0].trim(), parts[1].trim(), parts[2].trim()
                    );
                } else {
                    self.output = format!("Limits: {r}");
                }
                self.error_message = None;
            }
            Ok(_) => self.error_message = Some("No response to LIMIT?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `LIMIT <current>, <voltage>, <rate>` — set all max limits.
    /// Validates ranges per Lakeshore 625 manual before sending.
    pub fn set_limits(&mut self, current: f64, voltage: f64, rate: f64) -> Result<(), String> {
        if !(0.0..=CURRENT_LIMIT_MAX).contains(&current) {
            return Err(format!("Current limit must be 0–{CURRENT_LIMIT_MAX} A, got {current}"));
        }
        if !(VOLTAGE_LIMIT_MIN..=VOLTAGE_LIMIT_MAX).contains(&voltage) {
            return Err(format!("Voltage limit must be {VOLTAGE_LIMIT_MIN}–{VOLTAGE_LIMIT_MAX} V, got {voltage}"));
        }
        if !(RATE_LIMIT_MIN..=RATE_LIMIT_MAX).contains(&rate) {
            return Err(format!("Rate limit must be {RATE_LIMIT_MIN}–{RATE_LIMIT_MAX} A/s, got {rate}"));
        }
        self.send_command(&format!("LIMIT {current}, {voltage}, {rate}"))?;
        Ok(())
    }

    // ── Quench detection ─────────────────────────────────────

    /// `QNCH?` — get quench detection status into `output`.
    pub fn get_quench_status(&mut self) {
        match self.send_command("QNCH?") {
            Ok(r) if !r.is_empty() => {
                let parts: Vec<&str> = r.splitn(2, ',').collect();
                if parts.len() == 2 {
                    let enabled = parts[0].trim() == "1";
                    self.output = format!(
                        "Quench Detection: {}\nStep Limit:       {} A/s",
                        if enabled { "ON" } else { "OFF" },
                        parts[1].trim()
                    );
                } else {
                    self.output = format!("Quench status: {r}");
                }
                self.error_message = None;
            }
            Ok(_) => self.error_message = Some("No response to QNCH?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `QNCH 1` / `QNCH 0` — enable or disable quench detection.
    pub fn set_quench_enable(&mut self, enable: bool) -> Result<(), String> {
        self.send_command(&format!("QNCH {}", if enable { 1 } else { 0 }))?;
        Ok(())
    }

    /// `QNCH 1,<step_limit>` — set quench step limit (A/s) while leaving enable state.
    pub fn set_quench_step_limit(&mut self, step_limit: f64) -> Result<(), String> {
        self.send_command(&format!("QNCH 1,{step_limit}"))?;
        Ok(())
    }

    /// `QNCH <enable> <step_limit>` — set quench detection enable and step limit together.
    pub fn set_quench_detection(&mut self, enable: bool, step_limit: f64) -> Result<(), String> {
        self.send_command(&format!("QNCH {} {step_limit}", if enable { 1 } else { 0 }))?;
        Ok(())
    }

    // ── Error status ─────────────────────────────────────────

    /// `ERSTR?` — get and parse the error status register into `output`.
    /// Replicates the bit-field parsing from power_controller.py::get_error_status().
    pub fn get_error_status(&mut self) {
        match self.send_command("ERSTR?") {
            Ok(r) if !r.is_empty() => {
                self.output = parse_error_status(&r);
                self.error_message = None;
            }
            Ok(_) => self.error_message = Some("No response to ERSTR?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
    }

    // ── Raw command ──────────────────────────────────────────

    /// Send an arbitrary command string and put the response in `output`.
    pub fn raw_command(&mut self, command: &str) {
        match self.send_command(command) {
            Ok(r) => {
                self.output = format!(">> {command}\n{}", if r.is_empty() { "(no response)" } else { &r });
                self.error_message = None;
            }
            Err(e) => self.error_message = Some(e),
        }
    }
}

// ── ERSTR? bit-field parser ───────────────────────────────────
/// Parses `ERSTR?` response `"hw,op,psh"` into a human-readable string.
/// Mirrors the logic in power_controller.py::get_error_status().
fn parse_error_status(response: &str) -> String {
    let parts: Vec<&str> = response.splitn(3, ',').collect();
    if parts.len() != 3 {
        return format!("Raw error status: {response}");
    }

    let hw  = parts[0].trim().parse::<u32>().unwrap_or(0);
    let op  = parts[1].trim().parse::<u32>().unwrap_or(0);
    let psh = parts[2].trim().parse::<u32>().unwrap_or(0);

    let mut out = String::from("Error Status Register:\n");

    // Hardware errors
    if hw == 0 {
        out.push_str("  Hardware Errors:    None\n");
    } else {
        out.push_str(&format!("  Hardware Errors:    {hw}\n"));
        if hw & 32 != 0 { out.push_str("    - DAC Processor Not Responding\n"); }
        if hw & 16 != 0 { out.push_str("    - Output Control Failure\n"); }
        if hw &  8 != 0 { out.push_str("    - Output Over Voltage\n"); }
        if hw &  4 != 0 { out.push_str("    - Output Over Current\n"); }
        if hw &  2 != 0 { out.push_str("    - Low Line Voltage\n"); }
        if hw &  1 != 0 { out.push_str("    - Temperature Fault\n"); }
    }

    // Operational errors
    if op == 0 {
        out.push_str("  Operational Errors: None\n");
    } else {
        out.push_str(&format!("  Operational Errors: {op}\n"));
        if op & 64 != 0 { out.push_str("    - Magnet Discharging Through Crowbar\n"); }
        if op & 32 != 0 { out.push_str("    - Magnet Quench Detected\n"); }
        if op & 16 != 0 { out.push_str("    - Remote Inhibit Detected\n"); }
        if op &  8 != 0 { out.push_str("    - Temperature High\n"); }
        if op &  4 != 0 { out.push_str("    - High Line Voltage\n"); }
        if op &  2 != 0 { out.push_str("    - External Current Program Error\n"); }
        if op &  1 != 0 { out.push_str("    - Calibration Error\n"); }
    }

    // PSH errors
    if psh == 0 {
        out.push_str("  PSH Errors:         None\n");
    } else {
        out.push_str(&format!("  PSH Errors:         {psh}\n"));
        if psh & 2 != 0 { out.push_str("    - PSH Short Circuit\n"); }
        if psh & 1 != 0 { out.push_str("    - PSH Open Circuit\n"); }
    }

    out
}
