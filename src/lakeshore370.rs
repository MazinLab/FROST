// lakeshore370.rs — Lake Shore 370 AC Resistance Bridge controller for FROST
//
// Replicates the functionality of lakeshore370-python/lakeshore370/
// References: temperature.py, outputs.py, main.py
//
// Serial settings (per Lakeshore 370 hardware spec):
//   9600 baud, 7-bit data, odd parity, 1 stop bit, 2 s timeout
// Command terminator: \r\n
// Response terminator: \r\n

use serialport::{DataBits, Parity, StopBits};
use std::io::{Read, Write};
use std::time::Duration;

// ── Default connection settings ───────────────────────────────
const DEFAULT_PORT: &str = "/dev/ttyUSB1";
const DEFAULT_BAUD: u32 = 9600;

// ── Validation limits ─────────────────────────────────────────
pub const INPUT_MIN: u8 = 1;
pub const INPUT_MAX: u8 = 16;
pub const HEATER_RANGE_MAX: u8 = 8;
pub const EXCITATION_MIN: u8 = 1;
pub const EXCITATION_MAX: u8 = 22;
pub const RANGE_CODE_MIN: u8 = 1;
pub const RANGE_CODE_MAX: u8 = 22;

// ── Heater range names ────────────────────────────────────────
pub const HEATER_RANGE_NAMES: [&str; 9] = [
    "Off",
    "31.6 µA (0.1 µW into 100 Ω)",
    "100 µA (1 µW into 100 Ω)",
    "316 µA (10 µW into 100 Ω)",
    "1 mA (100 µW into 100 Ω)",
    "3.16 mA (1 mW into 100 Ω)",
    "10 mA (10 mW into 100 Ω)",
    "31.6 mA (100 mW into 100 Ω)",
    "100 mA (1 W into 100 Ω)",
];

// ── Controller state ──────────────────────────────────────────
pub struct LakeShore370Controller {
    pub port: String,
    pub baud_rate: u32,

    /// Last error shown in the GUI.
    pub error_message: Option<String>,
    /// General query output shown in the output panel.
    pub output: String,

    // ── GUI input fields ──────────────────────────────────────
    /// Selected input channel (1–16).
    pub selected_input: u8,
    /// Heater output percentage (0–100).
    pub heater_output_pct: f64,
    /// Heater range code (0–8).
    pub heater_range: u8,
    /// Resistance range mode (0=manual, 1=current excitation, 2=voltage excitation).
    pub range_mode: u8,
    /// Resistance range excitation level (1–22).
    pub range_excitation: u8,
    /// Resistance range code (1–22).
    pub range_code: u8,
    /// Resistance range autorange (0=off, 1=on).
    pub range_autorange: u8,
    /// Resistance range current source off flag (0=source on, 1=source off).
    pub range_cs_off: u8,
    /// Analog output channel (1 or 2).
    pub analog_channel: u8,
}

impl Default for LakeShore370Controller {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT.to_string(),
            baud_rate: DEFAULT_BAUD,
            error_message: None,
            output: String::new(),
            selected_input: 1,
            heater_output_pct: 0.0,
            heater_range: 0,
            range_mode: 0,
            range_excitation: 5,
            range_code: 14,
            range_autorange: 0,
            range_cs_off: 0,
            analog_channel: 1,
        }
    }
}

impl LakeShore370Controller {
    // ── Serial connection ─────────────────────────────────────

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

    /// Send a write-only command (no response expected) with a longer settling time.
    /// Used for RDGRNG, MOUT, HTRRNG, ANALOG, etc. (matches Python time.sleep(0.5)).
    fn send_write_command(&self, command: &str) -> Result<(), String> {
        let mut port = serialport::new(&self.port, self.baud_rate)
            .data_bits(DataBits::Seven)
            .parity(Parity::Odd)
            .stop_bits(StopBits::One)
            .timeout(Duration::from_millis(2000))
            .open()
            .map_err(|e| format!("Failed to open {}: {}", self.port, e))?;

        port.clear(serialport::ClearBuffer::Input).ok();

        port.write_all(format!("{}\r\n", command).as_bytes())
            .map_err(|e| format!("Write error: {e}"))?;

        // 500 ms settling time for write commands
        std::thread::sleep(Duration::from_millis(500));

        Ok(())
    }

    // ── Identity / info ───────────────────────────────────────

    /// `*IDN?` — device identification string.
    pub fn get_identification(&mut self) {
        match self.send_command("*IDN?") {
            Ok(r) if !r.is_empty() => { self.output = format!("ID: {r}"); self.error_message = None; }
            Ok(_) => self.error_message = Some("No response to *IDN?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `BAUD?` — baud rate code (0=300, 1=1200, 2=9600).
    pub fn get_baud_rate(&mut self) {
        match self.send_command("BAUD?") {
            Ok(r) if !r.is_empty() => {
                let readable = match r.as_str() {
                    "0" => "300",
                    "1" => "1200",
                    "2" => "9600",
                    other => other,
                };
                self.output = format!("Baud rate: {readable} baud (code: {r})");
                self.error_message = None;
            }
            Ok(_) => self.error_message = Some("No response to BAUD?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `BAUD <code>` — set baud rate (0=300, 1=1200, 2=9600).
    pub fn set_baud_rate(&mut self, code: u8) -> Result<(), String> {
        if code > 2 {
            return Err(format!("Baud rate code must be 0 (300), 1 (1200), or 2 (9600), got {code}"));
        }
        self.send_write_command(&format!("BAUD {code}"))?;
        Ok(())
    }

    // ── Temperature / resistance readings ─────────────────────

    /// `RDGK? <input>` — read temperature in Kelvin.
    pub fn read_kelvin(&self, input: u8) -> Result<String, String> {
        if !(INPUT_MIN..=INPUT_MAX).contains(&input) {
            return Err(format!("Input must be {INPUT_MIN}–{INPUT_MAX}, got {input}"));
        }
        let r = self.send_command(&format!("RDGK? {input}"))?;
        if r.is_empty() { return Err(format!("No response to RDGK? {input}")); }
        Ok(r)
    }

    /// `RDGR? <input>` — read resistance in Ohms.
    pub fn read_resistance(&self, input: u8) -> Result<String, String> {
        if !(INPUT_MIN..=INPUT_MAX).contains(&input) {
            return Err(format!("Input must be {INPUT_MIN}–{INPUT_MAX}, got {input}"));
        }
        let r = self.send_command(&format!("RDGR? {input}"))?;
        if r.is_empty() { return Err(format!("No response to RDGR? {input}")); }
        Ok(r)
    }

    /// `RDGPWR? <input>` — read excitation power in Watts.
    pub fn read_excitation_power(&self, input: u8) -> Result<String, String> {
        if !(INPUT_MIN..=INPUT_MAX).contains(&input) {
            return Err(format!("Input must be {INPUT_MIN}–{INPUT_MAX}, got {input}"));
        }
        let r = self.send_command(&format!("RDGPWR? {input}"))?;
        if r.is_empty() { return Err(format!("No response to RDGPWR? {input}")); }
        Ok(r)
    }

    /// `RDGST? <input>` — read input status byte.
    pub fn read_status(&self, input: u8) -> Result<String, String> {
        if !(INPUT_MIN..=INPUT_MAX).contains(&input) {
            return Err(format!("Input must be {INPUT_MIN}–{INPUT_MAX}, got {input}"));
        }
        let r = self.send_command(&format!("RDGST? {input}"))?;
        if r.is_empty() { return Err(format!("No response to RDGST? {input}")); }
        Ok(r)
    }

    /// Read temperature, resistance, and power for one input into `output`.
    pub fn get_all_readings(&mut self, input: u8) {
        if !(INPUT_MIN..=INPUT_MAX).contains(&input) {
            self.error_message = Some(format!("Input must be {INPUT_MIN}–{INPUT_MAX}"));
            return;
        }
        let mut out = format!("Input {input}:\n");

        match self.send_command(&format!("RDGK? {input}")) {
            Ok(r) if !r.is_empty() => {
                match r.parse::<f64>() {
                    Ok(k) if k > 0.0 => out.push_str(&format!("  Temperature: {k:.4} K\n")),
                    Ok(_)            => out.push_str(&format!("  Temperature: {r} (overload)\n")),
                    Err(_)           => out.push_str(&format!("  Temperature: {r}\n")),
                }
            }
            Ok(_)  => out.push_str("  Temperature: NO_RESPONSE\n"),
            Err(e) => out.push_str(&format!("  Temperature: ERROR ({e})\n")),
        }

        match self.send_command(&format!("RDGR? {input}")) {
            Ok(r) if !r.is_empty() => {
                match r.parse::<f64>() {
                    Ok(ohms) if ohms >= 0.0 => out.push_str(&format!("  Resistance:  {ohms:.4} Ω\n")),
                    Ok(_)                   => out.push_str(&format!("  Resistance:  {r} (overload)\n")),
                    Err(_)                  => out.push_str(&format!("  Resistance:  {r}\n")),
                }
            }
            Ok(_)  => out.push_str("  Resistance:  NO_RESPONSE\n"),
            Err(e) => out.push_str(&format!("  Resistance:  ERROR ({e})\n")),
        }

        match self.send_command(&format!("RDGPWR? {input}")) {
            Ok(r) if !r.is_empty() => {
                match r.parse::<f64>() {
                    Ok(w)  => out.push_str(&format!("  Power:       {}\n", format_power(w))),
                    Err(_) => out.push_str(&format!("  Power:       {r}\n")),
                }
            }
            Ok(_)  => out.push_str("  Power:       NO_RESPONSE\n"),
            Err(e) => out.push_str(&format!("  Power:       ERROR ({e})\n")),
        }

        self.output = out;
        self.error_message = None;
    }

    // ── Resistance range configuration ────────────────────────

    /// `RDGRNG? <input>` — get resistance range configuration into `output`.
    pub fn get_resistance_range(&mut self, input: u8) {
        if !(INPUT_MIN..=INPUT_MAX).contains(&input) {
            self.error_message = Some(format!("Input must be {INPUT_MIN}–{INPUT_MAX}"));
            return;
        }
        match self.send_command(&format!("RDGRNG? {input}")) {
            Ok(r) if !r.is_empty() => {
                self.output = parse_resistance_range(&r, input);
                self.error_message = None;
            }
            Ok(_) => self.error_message = Some(format!("No response to RDGRNG? {input}")),
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `RDGRNG <input>,<mode>,<excitation>,<range>,<autorange>,<cs_off>` — set resistance range.
    ///
    /// - `mode`: 0=manual, 1=current excitation, 2=voltage excitation
    /// - `excitation`: 1–22
    /// - `range`: 1–22
    /// - `autorange`: 0=off, 1=on
    /// - `cs_off`: 0=current source on, 1=current source off
    pub fn set_resistance_range(
        &mut self,
        input: u8,
        mode: u8,
        excitation: u8,
        range: u8,
        autorange: u8,
        cs_off: u8,
    ) -> Result<(), String> {
        if !(INPUT_MIN..=INPUT_MAX).contains(&input) {
            return Err(format!("Input must be {INPUT_MIN}–{INPUT_MAX}, got {input}"));
        }
        if mode > 2 {
            return Err(format!("Mode must be 0 (manual), 1 (current), or 2 (voltage), got {mode}"));
        }
        if !(EXCITATION_MIN..=EXCITATION_MAX).contains(&excitation) {
            return Err(format!("Excitation must be {EXCITATION_MIN}–{EXCITATION_MAX}, got {excitation}"));
        }
        if !(RANGE_CODE_MIN..=RANGE_CODE_MAX).contains(&range) {
            return Err(format!("Range code must be {RANGE_CODE_MIN}–{RANGE_CODE_MAX}, got {range}"));
        }
        if autorange > 1 {
            return Err(format!("Autorange must be 0 (off) or 1 (on), got {autorange}"));
        }
        if cs_off > 1 {
            return Err(format!("cs_off must be 0 (source on) or 1 (source off), got {cs_off}"));
        }
        self.send_write_command(&format!(
            "RDGRNG {input},{mode},{excitation},{range},{autorange},{cs_off}"
        ))?;
        Ok(())
    }

    // ── Heater output ─────────────────────────────────────────

    /// `HTR?` — get heater output percentage into `output`.
    pub fn get_heater_output(&mut self) {
        match self.send_command("HTR?") {
            Ok(r) if !r.is_empty() => {
                match r.parse::<f64>() {
                    Ok(pct) => { self.output = format!("Heater output: {pct:.3}%"); }
                    Err(_)  => { self.output = format!("Heater output: {r}"); }
                }
                self.error_message = None;
            }
            Ok(_) => self.error_message = Some("No response to HTR?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `MOUT <percent>` — set heater output (0.000–100.000%).
    pub fn set_heater_output(&mut self, percent: f64) -> Result<(), String> {
        if !(0.0..=100.0).contains(&percent) {
            return Err(format!("Heater output must be 0.0–100.0%, got {percent}"));
        }
        self.send_write_command(&format!("MOUT {percent:.3}"))?;
        Ok(())
    }

    // ── Heater range ──────────────────────────────────────────

    /// `HTRRNG?` — get heater range into `output`.
    pub fn get_heater_range(&mut self) {
        match self.send_command("HTRRNG?") {
            Ok(r) if !r.is_empty() => {
                let code = r.parse::<usize>().unwrap_or(usize::MAX);
                let name = HEATER_RANGE_NAMES.get(code).copied().unwrap_or("Unknown");
                self.output = format!("Heater range: {code} — {name}");
                self.error_message = None;
            }
            Ok(_) => self.error_message = Some("No response to HTRRNG?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `HTRRNG <range>` — set heater range (0=off … 8=100 mA / 1 W).
    pub fn set_heater_range(&mut self, range: u8) -> Result<(), String> {
        if range > HEATER_RANGE_MAX {
            return Err(format!("Heater range must be 0–{HEATER_RANGE_MAX}, got {range}"));
        }
        self.send_write_command(&format!("HTRRNG {range}"))?;
        Ok(())
    }

    // ── Heater status ─────────────────────────────────────────

    /// `HTRST?` — get heater status into `output`.
    pub fn get_heater_status(&mut self) {
        match self.send_command("HTRST?") {
            Ok(r) if !r.is_empty() => {
                let code = r.parse::<u32>().unwrap_or(0);
                self.output = format!("Heater status: {code} (0x{code:02X})");
                self.error_message = None;
            }
            Ok(_) => self.error_message = Some("No response to HTRST?".to_string()),
            Err(e) => self.error_message = Some(e),
        }
    }

    // ── Analog outputs ────────────────────────────────────────

    /// `ANALOG? <channel>` — get analog output configuration into `output`.
    pub fn get_analog_config(&mut self, channel: u8) {
        if channel < 1 || channel > 2 {
            self.error_message = Some("Analog channel must be 1 or 2".to_string());
            return;
        }
        match self.send_command(&format!("ANALOG? {channel}")) {
            Ok(r) if !r.is_empty() => {
                self.output = parse_analog_config(&r, channel);
                self.error_message = None;
            }
            Ok(_) => self.error_message = Some(format!("No response to ANALOG? {channel}")),
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `AOUT? <channel>` — get analog output current value (%) into `output`.
    pub fn get_analog_output(&mut self, channel: u8) {
        if channel < 1 || channel > 2 {
            self.error_message = Some("Analog channel must be 1 or 2".to_string());
            return;
        }
        match self.send_command(&format!("AOUT? {channel}")) {
            Ok(r) if !r.is_empty() => {
                match r.parse::<f64>() {
                    Ok(pct) => { self.output = format!("Analog output {channel}: {pct:.3}%"); }
                    Err(_)  => { self.output = format!("Analog output {channel}: {r}"); }
                }
                self.error_message = None;
            }
            Ok(_) => self.error_message = Some(format!("No response to AOUT? {channel}")),
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `ANALOG <channel>,<polarity>,0,0,0,0,0,0` — turn analog output off.
    pub fn set_analog_off(&mut self, channel: u8) -> Result<(), String> {
        if channel < 1 || channel > 2 {
            return Err("Analog channel must be 1 or 2".to_string());
        }
        self.send_write_command(&format!("ANALOG {channel},0,0,0,0,0,0,0"))?;
        Ok(())
    }

    /// `ANALOG <ch>,<pol>,1,<input>,<src>,<high>,<low>,0` — monitor a reading (channel mode).
    ///
    /// - `polarity`: 0=unipolar (0–+10 V), 1=bipolar (−10 to +10 V)
    /// - `input`: 1–16, the input channel to monitor
    /// - `data_source`: 1=Kelvin, 2=Ohms, 3=Linear Data
    /// - `high_value` / `low_value`: scaling endpoints
    pub fn set_analog_channel_mode(
        &mut self,
        channel: u8,
        polarity: u8,
        input: u8,
        data_source: u8,
        high_value: f64,
        low_value: f64,
    ) -> Result<(), String> {
        if channel < 1 || channel > 2 {
            return Err("Analog channel must be 1 or 2".to_string());
        }
        if polarity > 1 {
            return Err("Polarity must be 0 (unipolar) or 1 (bipolar)".to_string());
        }
        if !(INPUT_MIN..=INPUT_MAX).contains(&input) {
            return Err(format!("Input must be {INPUT_MIN}–{INPUT_MAX}, got {input}"));
        }
        if data_source < 1 || data_source > 3 {
            return Err("Data source must be 1 (Kelvin), 2 (Ohms), or 3 (Linear Data)".to_string());
        }
        self.send_write_command(&format!(
            "ANALOG {channel},{polarity},1,{input},{data_source},{high_value},{low_value},0"
        ))?;
        Ok(())
    }

    /// `ANALOG <ch>,<pol>,2,0,0,0,0,<manual>` — set manual output value.
    pub fn set_analog_manual_mode(
        &mut self,
        channel: u8,
        polarity: u8,
        manual_value: f64,
    ) -> Result<(), String> {
        if channel < 1 || channel > 2 {
            return Err("Analog channel must be 1 or 2".to_string());
        }
        if polarity > 1 {
            return Err("Polarity must be 0 (unipolar) or 1 (bipolar)".to_string());
        }
        self.send_write_command(&format!(
            "ANALOG {channel},{polarity},2,0,0,0,0,{manual_value}"
        ))?;
        Ok(())
    }

    /// `ANALOG 2,<pol>,4,0,0,0,0,0` — still heater mode (channel 2 only).
    pub fn set_analog_still_mode(&mut self, polarity: u8) -> Result<(), String> {
        if polarity > 1 {
            return Err("Polarity must be 0 (unipolar) or 1 (bipolar)".to_string());
        }
        self.send_write_command(&format!("ANALOG 2,{polarity},4,0,0,0,0,0"))?;
        Ok(())
    }

    // ── Raw command ───────────────────────────────────────────

    /// Send an arbitrary command string and put the response in `output`.
    pub fn raw_command(&mut self, command: &str) {
        match self.send_command(command) {
            Ok(r) => {
                self.output = format!(
                    ">> {command}\n{}",
                    if r.is_empty() { "(no response)" } else { &r }
                );
                self.error_message = None;
            }
            Err(e) => self.error_message = Some(e),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────

/// Format a power value in the most appropriate SI prefix.
/// Mirrors the scaling logic in main.py.
fn format_power(watts: f64) -> String {
    if watts.abs() < 1e-12 {
        format!("{:.3} fW", watts * 1e15)
    } else if watts.abs() < 1e-9 {
        format!("{:.3} pW", watts * 1e12)
    } else if watts.abs() < 1e-6 {
        format!("{:.3} nW", watts * 1e9)
    } else if watts.abs() < 1e-3 {
        format!("{:.3} µW", watts * 1e6)
    } else {
        format!("{:.3} mW", watts * 1e3)
    }
}

/// Parse `RDGRNG?` response `"mode,excitation,range,autorange,cs_off"` into human-readable text.
fn parse_resistance_range(response: &str, input: u8) -> String {
    let parts: Vec<&str> = response.splitn(5, ',').collect();
    if parts.len() != 5 {
        return format!("Input {input} range config (raw): {response}");
    }
    let mode  = parts[0].trim().parse::<u8>().unwrap_or(0);
    let exc   = parts[1].trim().parse::<u8>().unwrap_or(0);
    let range = parts[2].trim().parse::<u8>().unwrap_or(0);
    let auto  = parts[3].trim().parse::<u8>().unwrap_or(0);
    let cs    = parts[4].trim().parse::<u8>().unwrap_or(0);

    let mode_str = match mode {
        0 => "Manual",
        1 => "Current excitation",
        2 => "Voltage excitation",
        _ => "Unknown",
    };
    let auto_str = if auto == 1 { "On" } else { "Off" };
    let cs_str   = if cs == 1   { "Off" } else { "On" };

    format!(
        "Input {input} Resistance Range:\n  Mode:        {mode} ({mode_str})\n  Excitation:  {exc} (level 1–22)\n  Range:       {range} (range 1–22)\n  Autorange:   {auto} ({auto_str})\n  Current src: {cs} ({cs_str})"
    )
}

/// Parse `ANALOG?` response `"polarity,mode,channel,source,high,low,manual"` into human-readable text.
fn parse_analog_config(response: &str, channel: u8) -> String {
    let parts: Vec<&str> = response.splitn(7, ',').collect();
    if parts.len() < 7 {
        return format!("Analog output {channel} config (raw): {response}");
    }
    let polarity = parts[0].trim().parse::<u8>().unwrap_or(0);
    let mode     = parts[1].trim().parse::<u8>().unwrap_or(0);
    let input_ch = parts[2].trim().parse::<u8>().unwrap_or(0);
    let source   = parts[3].trim().parse::<u8>().unwrap_or(0);
    let high     = parts[4].trim().parse::<f64>().unwrap_or(0.0);
    let low      = parts[5].trim().parse::<f64>().unwrap_or(0.0);
    let manual   = parts[6].trim().parse::<f64>().unwrap_or(0.0);

    let pol_str  = if polarity == 1 { "Bipolar (−10 to +10 V)" } else { "Unipolar (0 to +10 V)" };
    let mode_str = match mode {
        0 => "Off",
        1 => "Channel",
        2 => "Manual",
        3 => "Zone",
        4 => "Still",
        _ => "Unknown",
    };
    let src_str  = match source {
        1 => "Kelvin",
        2 => "Ohms",
        3 => "Linear Data",
        _ => "N/A",
    };

    format!(
        "Analog output {channel}:\n  Polarity:    {polarity} ({pol_str})\n  Mode:        {mode} ({mode_str})\n  Input ch.:   {input_ch}\n  Data source: {source} ({src_str})\n  High value:  {high}\n  Low value:   {low}\n  Manual val:  {manual}"
    )
}
