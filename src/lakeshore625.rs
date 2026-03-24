// lakeshore625.rs — Lake Shore 625 Superconducting Magnet Power Supply controller for FROST
//
// Replicates the functionality of lakeshore625-python/lakeshore625/
// References: power_controller.py, main.py
//
// Serial settings (per Lakeshore 625 hardware spec):
//   9600 baud, 7-bit data, odd parity, 1 stop bit, 2 s timeout
// Command terminator: \r\n
// Response terminator: \n

use chrono::Local;
use serialport::{DataBits, Parity, StopBits};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
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

    /// `SETI?` — get the programmed target current setpoint (A).
    pub fn get_set_current(&mut self) {
        match self.send_command("SETI?") {
            Ok(r) if !r.is_empty() => { self.output = format!("Set current: {} A", r); self.error_message = None; }
            Ok(_) => self.error_message = Some("No response to SETI?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
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

    // ── Ramp data logging ───────────────────────────────────────

    /// Return the path that the next `run_logging` call will write to.
    /// Creates the `ramps/` directory if needed.
    pub fn next_log_path() -> String {
        const OUTPUT_DIR: &str = "ramps";
        std::fs::create_dir_all(OUTPUT_DIR).ok();
        let date = Local::now().format("%Y-%m-%d").to_string();
        next_ramp_csv(OUTPUT_DIR, &date)
    }

    /// Core logging loop — one row per minute.
    ///
    /// * `stop`    — set to `true` to end the loop cleanly (checked every 100 ms).
    /// * `verbose` — if `true`, prints the formatted table to stdout;
    ///               if `false`, writes to CSV only (suitable for background use).
    pub fn run_logging_until(&self, stop: Arc<AtomicBool>, verbose: bool) -> Result<(), String> {
        const OUTPUT_DIR:    &str = "ramps";
        const INTERVAL_SECS: u64 = 60;

        std::fs::create_dir_all(OUTPUT_DIR)
            .map_err(|e| format!("Cannot create '{}' directory: {e}", OUTPUT_DIR))?;

        let date_str = Local::now().format("%Y-%m-%d").to_string();
        let csv_path = next_ramp_csv(OUTPUT_DIR, &date_str);

        if verbose {
            println!("Starting LakeShore 625 ramp data logging...");
            println!("Recording interval: {} seconds", INTERVAL_SECS);
            println!("CSV file will be saved as: {}", csv_path);
            println!("Press Ctrl+C to stop recording and save data");
            println!("{}", "-".repeat(120));
            println!();
            print_ramp_header();
        }

        // Create the CSV file with a formatted header row immediately.
        {
            let header_line = ramp_format_row(
                &RAMP_HEADERS.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            );
            let mut f = OpenOptions::new()
                .create(true).write(true).truncate(true)
                .open(&csv_path)
                .map_err(|e| format!("Cannot create '{}': {e}", csv_path))?;
            writeln!(f, "{}", header_line)
                .map_err(|e| format!("Write error: {e}"))?;
        }

        let start = std::time::Instant::now();

        while !stop.load(Ordering::Relaxed) {
            let now   = Local::now();
            let ts    = now.format("%Y-%m-%dT%H:%M:%S%.6f").to_string();
            let date  = now.format("%Y-%m-%d").to_string();
            let time  = now.format("%H:%M:%S").to_string();
            let elap  = start.elapsed().as_secs_f64();

            let rate  = self.read_f64("RATE?");
            let cur   = self.read_f64("RDGI?");
            let volt  = self.read_f64("RDGV?");
            let field = self.read_f64("RDGF?");
            let err   = self.read_error_compact();

            let row = vec![
                ts,
                date,
                time,
                fmt_ramp_f64_opt(Some(elap), 1),
                fmt_ramp_f64_opt(rate,  4),
                fmt_ramp_f64_opt(cur,   4),
                fmt_ramp_f64_opt(volt,  4),
                fmt_ramp_f64_opt(field, 4),
                err,
            ];

            if verbose { print_ramp_row(&row); }

            if let Err(e) = append_ramp_row(&csv_path, &ramp_format_row(&row)) {
                eprintln!("Warning: could not write row: {e}");
            }

            // Sleep in 100 ms ticks so the stop flag is noticed promptly.
            for _ in 0..(INTERVAL_SECS * 10) {
                if stop.load(Ordering::Relaxed) { break; }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        Ok(())
    }

    /// Continuously log ramp data to a date-stamped CSV in `ramps/`.
    /// Prints a live formatted table to stdout.  Runs until Ctrl+C.
    pub fn run_logging(&self) -> Result<(), String> {
        self.run_logging_until(Arc::new(AtomicBool::new(false)), true)
    }

    /// Parse a raw SCPI response to `f64`, stripping a leading `+`.
    fn read_f64(&self, cmd: &str) -> Option<f64> {
        self.send_command(cmd).ok()
            .filter(|r| !r.is_empty())
            .and_then(|r| r.trim_start_matches('+').parse::<f64>().ok())
    }

    /// `ERSTR?` — compact semicolon-separated error string (or "None").
    fn read_error_compact(&self) -> String {
        match self.send_command("ERSTR?") {
            Ok(r) if !r.is_empty() => parse_error_compact(&r),
            _ => "None".to_string(),
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

// ── Ramp logging support ──────────────────────────────────────────
// Column headers and widths mirror logging.py from lakeshore625-python.
const RAMP_HEADERS: &[&str] = &[
    "Timestamp", "Date", "Time", "Elapsed_Seconds",
    "Ramp_Rate_A_per_s", "Current_A", "Voltage_V", "Field_T", "Error_Status",
];
const RAMP_WIDTHS: &[usize] = &[28, 12, 12, 16, 18, 14, 14, 14, 20];

/// Find the next available ramp log path (auto-increments `_1`, `_2`, …).
fn next_ramp_csv(dir: &str, date: &str) -> String {
    let base = format!("{}/{}_ramp_log.csv", dir, date);
    if !Path::new(&base).exists() { return base; }
    let mut n = 1u32;
    loop {
        let p = format!("{}/{}_ramp_log_{}.csv", dir, date, n);
        if !Path::new(&p).exists() { return p; }
        n += 1;
    }
}

/// Print fixed-width header + separator dashes to stdout.
fn print_ramp_header() {
    let line: String = RAMP_HEADERS.iter().zip(RAMP_WIDTHS.iter())
        .map(|(h, w)| format!("{:<width$}", h, width = w))
        .collect();
    println!("{}", line);
    let sep: String = RAMP_WIDTHS.iter().map(|w| "-".repeat(*w)).collect();
    println!("{}", sep);
}

/// Print one data row to stdout (every column padded to its width).
fn print_ramp_row(values: &[String]) {
    let line: String = values.iter().enumerate()
        .map(|(i, v)| format!("{:<width$}", v, width = RAMP_WIDTHS.get(i).copied().unwrap_or(0)))
        .collect();
    println!("{}", line);
}

/// Format an `Option<f64>` as `decimals`-place string, or `"NO_RESPONSE"`.
fn fmt_ramp_f64_opt(v: Option<f64>, decimals: usize) -> String {
    match v {
        Some(x) => format!("{:.prec$}", x, prec = decimals),
        None    => "NO_RESPONSE".to_string(),
    }
}

/// Serialize a row to a fixed-width string for the CSV.
/// All columns are padded except the last — mirrors `append_to_formatted_csv` in logging.py.
fn ramp_format_row(values: &[String]) -> String {
    let last = values.len().saturating_sub(1);
    values.iter().enumerate()
        .map(|(i, v)| {
            if i == last {
                v.clone()
            } else {
                format!("{:<width$}", v, width = RAMP_WIDTHS.get(i).copied().unwrap_or(0))
            }
        })
        .collect()
}

/// Append one formatted line to the CSV file.
fn append_ramp_row(path: &str, line: &str) -> Result<(), String> {
    let mut f = OpenOptions::new()
        .append(true).open(path)
        .map_err(|e| format!("Cannot open '{}': {e}", path))?;
    writeln!(f, "{}", line).map_err(|e| format!("Write error: {e}"))
}

/// Parse `ERSTR?` response into a compact semicolon-separated error string.
/// Mirrors `_parse_error_status` in logging.py.
fn parse_error_compact(response: &str) -> String {
    let parts: Vec<&str> = response.splitn(3, ',').collect();
    if parts.len() != 3 { return "Parse Error".to_string(); }
    let hw  = parts[0].trim().parse::<u32>().unwrap_or(0);
    let op  = parts[1].trim().parse::<u32>().unwrap_or(0);
    let psh = parts[2].trim().parse::<u32>().unwrap_or(0);
    let mut errors: Vec<&str> = Vec::new();
    if op  & 64 != 0 { errors.push("Magnet Crowbar"); }
    if op  & 32 != 0 { errors.push("Magnet Quench"); }
    if op  & 16 != 0 { errors.push("Remote Inhibit"); }
    if op  &  8 != 0 { errors.push("Temp High"); }
    if op  &  4 != 0 { errors.push("High Line Voltage"); }
    if op  &  2 != 0 { errors.push("Ext Program Error"); }
    if op  &  1 != 0 { errors.push("Calibration Error"); }
    if hw  & 32 != 0 { errors.push("DAC Error"); }
    if hw  & 16 != 0 { errors.push("Output Control Fail"); }
    if hw  &  8 != 0 { errors.push("Output Over Voltage"); }
    if hw  &  4 != 0 { errors.push("Output Over Current"); }
    if hw  &  2 != 0 { errors.push("Low Line Voltage"); }
    if hw  &  1 != 0 { errors.push("Temperature Fault"); }
    if psh &  2 != 0 { errors.push("PSH Short"); }
    if psh &  1 != 0 { errors.push("PSH Open"); }
    if errors.is_empty() { "None".to_string() } else { errors.join("; ") }
}