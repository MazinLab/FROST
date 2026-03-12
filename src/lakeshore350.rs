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
//   Input D2 — Switch voltage reading (voltage only, no temperature calibration)
//   Input D3 — 4K stage diode (calibrated on Lakeshore, KRDG? returns Kelvin)
//   Input D4 — 3-pump diode (voltage → temperature via pumps_calibration)
//   Input D5 — 4-pump diode (voltage → temperature via pumps_calibration)
//   Channels 1–8 — mirrors of Input A (not used by FROST)

use serialport::{DataBits, Parity, StopBits};
use std::io::{Read, Write};
use std::time::Duration;
use std::path::Path;

// ── Default connection settings ───────────────────────────────
const DEFAULT_PORT: &str = "/dev/ttyUSB2";
const DEFAULT_BAUD: u32 = 57600;

/// All valid input / channel identifiers on the Lakeshore 350.
pub const ALL_INPUTS: [&str; 8] = ["A", "B", "C", "D1", "D2", "D3", "D4", "D5"];
/// All valid output identifiers on the Lakeshore 350.
pub const ALL_OUTPUTS: [u8; 4] = [1, 2, 3, 4];

// ── 3-head calibration ────────────────────────────────────────
/// 3-head resistance thermometer calibration.
/// Mirrors ThreeHeadCalibration class in head3_calibration.py.
pub struct ThreeHeadCalibration {
    resistances: Vec<f64>,
    temperatures: Vec<f64>,
}

impl ThreeHeadCalibration {
    /// Load calibration from CSV file (Temperature K, Resistance Ω).
    pub fn from_csv<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let mut resistances = Vec::new();
        let mut temperatures = Vec::new();
        
        let file = std::fs::File::open(&path)
            .map_err(|e| format!("Failed to open calibration file {:?}: {}", path.as_ref(), e))?;
        
        let mut reader = csv::Reader::from_reader(file);
        
        // Skip header and parse data
        for (i, result) in reader.records().enumerate() {
            let record = result.map_err(|e| format!("CSV parse error at line {}: {}", i + 2, e))?;
            
            if record.len() < 2 {
                continue; // Skip incomplete rows
            }
            
            if let (Ok(temp), Ok(resistance)) = (record[0].parse::<f64>(), record[1].parse::<f64>()) {
                if temp > 0.0 && resistance > 0.0 {
                    temperatures.push(temp);
                    resistances.push(resistance);
                }
            }
        }
        
        if resistances.is_empty() {
            return Err("No valid calibration data found in CSV".to_string());
        }
        
        Ok(Self { resistances, temperatures })
    }
    
    /// Convert resistance (Ω) to temperature (K) using linear interpolation.
    /// Mirrors resistance_to_temperature() method in Python.
    pub fn resistance_to_temperature(&self, resistance: f64) -> Option<f64> {
        if resistance <= 0.0 || self.resistances.is_empty() {
            return None;
        }
        
        // Find the interpolation range
        if resistance <= self.resistances[0] {
            return Some(self.temperatures[0]); // Below range, return first temp
        }
        if resistance >= self.resistances[self.resistances.len() - 1] {
            return Some(self.temperatures[self.temperatures.len() - 1]); // Above range, return last temp
        }
        
        // Linear interpolation between two points
        for i in 0..self.resistances.len() - 1 {
            if resistance >= self.resistances[i] && resistance <= self.resistances[i + 1] {
                let r1 = self.resistances[i];
                let r2 = self.resistances[i + 1];
                let t1 = self.temperatures[i];
                let t2 = self.temperatures[i + 1];
                
                // Linear interpolation: t = t1 + (t2 - t1) * (r - r1) / (r2 - r1)
                let temp = t1 + (t2 - t1) * (resistance - r1) / (r2 - r1);
                return Some(temp);
            }
        }
        
        None
    }
}

/// 4-head resistance thermometer calibration.
/// Mirrors FourHeadCalibration class in head4_calibration.py.
pub struct FourHeadCalibration {
    resistances: Vec<f64>,
    temperatures: Vec<f64>,
}

impl FourHeadCalibration {
    /// Load calibration from CSV file (Temperature K, Resistance Ω) and sort by resistance.
    pub fn from_csv<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let mut resistances = Vec::new();
        let mut temperatures = Vec::new();
        
        let file = std::fs::File::open(&path)
            .map_err(|e| format!("Failed to open calibration file {:?}: {}", path.as_ref(), e))?;
        
        let mut reader = csv::Reader::from_reader(file);
        
        // Skip header and parse data
        for (i, result) in reader.records().enumerate() {
            let record = result.map_err(|e| format!("CSV parse error at line {}: {}", i + 2, e))?;
            
            if record.len() < 2 {
                continue; // Skip incomplete rows
            }
            
            if let (Ok(temp), Ok(resistance)) = (record[0].parse::<f64>(), record[1].parse::<f64>()) {
                if temp > 0.0 && resistance > 0.0 {
                    temperatures.push(temp);
                    resistances.push(resistance);
                }
            }
        }
        
        if resistances.is_empty() {
            return Err("No valid calibration data found in CSV".to_string());
        }
        
        // Sort by resistance (increasing order) as done in Python
        let mut pairs: Vec<(f64, f64)> = resistances.iter().zip(temperatures.iter()).map(|(&r, &t)| (r, t)).collect();
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        
        let (sorted_resistances, sorted_temperatures): (Vec<f64>, Vec<f64>) = pairs.into_iter().unzip();
        
        Ok(Self {
            resistances: sorted_resistances,
            temperatures: sorted_temperatures,
        })
    }
    
    /// Convert resistance (Ω) to temperature (K) using linear interpolation.
    /// Mirrors resistance_to_temperature() method in Python.
    pub fn resistance_to_temperature(&self, resistance: f64) -> Option<f64> {
        if resistance <= 0.0 || self.resistances.is_empty() {
            return None;
        }
        
        // Find the interpolation range
        if resistance <= self.resistances[0] {
            return Some(self.temperatures[0]); // Below range, return first temp
        }
        if resistance >= self.resistances[self.resistances.len() - 1] {
            return Some(self.temperatures[self.temperatures.len() - 1]); // Above range, return last temp
        }
        
        // Linear interpolation between two points
        for i in 0..self.resistances.len() - 1 {
            if resistance >= self.resistances[i] && resistance <= self.resistances[i + 1] {
                let r1 = self.resistances[i];
                let r2 = self.resistances[i + 1];
                let t1 = self.temperatures[i];
                let t2 = self.temperatures[i + 1];
                
                // Linear interpolation: t = t1 + (t2 - t1) * (r - r1) / (r2 - r1)
                let temp = t1 + (t2 - t1) * (resistance - r1) / (r2 - r1);
                return Some(temp);
            }
        }
        
        None
    }
}

/// Pump voltage-to-temperature calibration for 3-pump and 4-pump diodes.
/// Mirrors PumpCalibration class in pump_calibration.py.
pub struct PumpCalibration {
    voltages: Vec<f64>,
    temperatures: Vec<f64>,
}

impl PumpCalibration {
    /// Load calibration from CSV file (Temperature K, Voltage V) and sort by voltage.
    pub fn from_csv<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let mut voltages = Vec::new();
        let mut temperatures = Vec::new();
        
        let file = std::fs::File::open(&path)
            .map_err(|e| format!("Failed to open pump calibration file {:?}: {}", path.as_ref(), e))?;
        
        let mut reader = csv::Reader::from_reader(file);
        
        // Skip header and parse data
        for (i, result) in reader.records().enumerate() {
            let record = result.map_err(|e| format!("CSV parse error at line {}: {}", i + 2, e))?;
            
            if record.len() < 2 {
                continue; // Skip incomplete rows
            }
            
            if let (Ok(temp), Ok(voltage)) = (record[0].parse::<f64>(), record[1].parse::<f64>()) {
                if temp > 0.0 && voltage > 0.0 {
                    temperatures.push(temp);
                    voltages.push(voltage);
                }
            }
        }
        
        if voltages.is_empty() {
            return Err("No valid pump calibration data found in CSV".to_string());
        }
        
        // Sort by voltage (increasing order) as done in Python for interpolation
        let mut pairs: Vec<(f64, f64)> = voltages.iter().zip(temperatures.iter()).map(|(&v, &t)| (v, t)).collect();
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        
        let (sorted_voltages, sorted_temperatures): (Vec<f64>, Vec<f64>) = pairs.into_iter().unzip();
        
        Ok(Self {
            voltages: sorted_voltages,
            temperatures: sorted_temperatures,
        })
    }
    
    /// Convert voltage (V) to temperature (K) using linear interpolation.
    /// Mirrors convert_voltage_to_temperature() method in Python.
    pub fn voltage_to_temperature(&self, voltage: f64) -> Option<f64> {
        if voltage <= 0.0 || self.voltages.is_empty() {
            return None;
        }
        
        // Find the interpolation range
        if voltage <= self.voltages[0] {
            return Some(self.temperatures[0]); // Below range, return first temp
        }
        if voltage >= self.voltages[self.voltages.len() - 1] {
            return Some(self.temperatures[self.temperatures.len() - 1]); // Above range, return last temp
        }
        
        // Linear interpolation between two points
        for i in 0..self.voltages.len() - 1 {
            if voltage >= self.voltages[i] && voltage <= self.voltages[i + 1] {
                let v1 = self.voltages[i];
                let v2 = self.voltages[i + 1];
                let t1 = self.temperatures[i];
                let t2 = self.temperatures[i + 1];
                
                // Linear interpolation: t = t1 + (t2 - t1) * (v - v1) / (v2 - v1)
                let temp = t1 + (t2 - t1) * (voltage - v1) / (v2 - v1);
                return Some(temp);
            }
        }
        
        None
    }
}

// ── Recording snapshot data ─────────────────────────────────
/// All sensor + temperature values from every LS350 input, collected in one pass.
pub struct Ls350RecordingData {
    /// Input A (3-head): raw resistance in Ω.
    pub input_a_sensor_ohm: Option<f64>,
    /// Input A (3-head): calibrated temperature in K.
    pub input_a_temp_k: Option<f64>,
    /// Input B (ADR): raw resistance in Ω (SRDG?).
    pub input_b_sensor_ohm: Option<f64>,
    /// Input B (ADR): temperature in K (KRDG?).
    pub input_b_temp_k: Option<f64>,
    /// Input C (4-head): raw resistance in Ω.
    pub input_c_sensor_ohm: Option<f64>,
    /// Input C (4-head): calibrated temperature in K (with +34.56 Ω offset).
    pub input_c_temp_k: Option<f64>,
    /// Input D2 (switch): voltage in V.
    pub input_d2_sensor_v: Option<f64>,
    /// Input D2 (switch): calibrated temperature in K.
    pub input_d2_temp_k: Option<f64>,
    /// Input D3 (4K stage): voltage in V.
    pub input_d3_sensor_v: Option<f64>,
    /// Input D3 (4K stage): temperature in K (KRDG?).
    pub input_d3_temp_k: Option<f64>,
    /// Input D4 (3-pump): voltage in V.
    pub input_d4_sensor_v: Option<f64>,
    /// Input D4 (3-pump): calibrated temperature in K.
    pub input_d4_temp_k: Option<f64>,
    /// Input D5 (4-pump): voltage in V.
    pub input_d5_sensor_v: Option<f64>,
    /// Input D5 (4-pump): calibrated temperature in K.
    pub input_d5_temp_k: Option<f64>,
}

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
    
    /// 3-head temperature calibration (loaded lazily).
    three_head_cal: Option<ThreeHeadCalibration>,
    
    /// 4-head temperature calibration (loaded lazily).
    four_head_cal: Option<FourHeadCalibration>,
    
    /// Pump temperature calibration for D4/D5 (loaded lazily).
    pump_cal: Option<PumpCalibration>,
}

impl Default for LakeShore350Controller {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT.to_string(),
            baud_rate: DEFAULT_BAUD,
            error_message: None,
            output: String::new(),
            selected_input: "A".to_string(),
            three_head_cal: None,
            four_head_cal: None,
            pump_cal: None,
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

    /// Validate output number (1–4).
    fn validate_output(output_num: u8) -> Result<(), String> {
        if ALL_OUTPUTS.contains(&output_num) {
            Ok(())
        } else {
            Err("Output number must be 1, 2, 3, or 4.".to_string())
        }
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
            if v == 0.0 && matches!(inp_up.as_str(), "D3" | "D4" | "D5") {
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

    /// Read A (3-head), B (ADR), C (4-head), D3 (4K stage), D4 (3-pump), and D5 (4-pump) and print sensor + Kelvin/calibrated for each.
    /// Mirrors the core of `lakeshore350 --all` for these six key inputs.
    pub fn get_all_readings(&mut self) {
        let mut out = String::new();

        // ── Input A (3-head) ──────────────────────────────────
        out.push_str(&format!("{}\n", self.get_3head_temperature_internal()));

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

        // ── Input C (4-head) ──────────────────────────────────
        out.push_str(&format!("{}\n", self.get_4head_temperature_internal()));

        // ── Input D3 (4K stage) ───────────────────────────────
        let d3_raw = self.read_sensor_raw("D3")
            .unwrap_or_else(|e| format!("ERROR ({e})"));
        let d3_kelvin = self.read_kelvin_raw("D3")
            .unwrap_or_else(|e| format!("ERROR ({e})"));
        let d3_raw_str = if let Ok(v) = d3_raw.parse::<f64>() {
            format!("{v:.4} V")
        } else {
            d3_raw
        };
        let d3_k_str = if let Ok(v) = d3_kelvin.parse::<f64>() {
            format!("{v:.4} K")
        } else {
            d3_kelvin
        };
        out.push_str(&format!("Input D3 (4K stage): {d3_raw_str} → {d3_k_str}\n"));

        // ── Input D4 (3-pump) ─────────────────────────────────
        out.push_str(&format!("{}\n", self.get_3pump_temperature_internal()));

        // ── Input D5 (4-pump) ─────────────────────────────────
        out.push_str(&format!("{}\n", self.get_4pump_temperature_internal()));

        self.output = out;
        self.error_message = None;
    }

    // ── 3-head calibrated temperature ─────────────────────────

    /// Load 3-head calibration from CSV file if not already loaded.
    fn ensure_3head_calibration(&mut self) -> Result<(), String> {
        if self.three_head_cal.is_none() {
            // Try standard paths
            let paths = [
                "src/gl7_calibrations/3_head_cal.csv",
                "gl7_calibrations/3_head_cal.csv",
                "../gl7_calibrations/3_head_cal.csv",
            ];
            
            for path in &paths {
                if Path::new(path).exists() {
                    match ThreeHeadCalibration::from_csv(path) {
                        Ok(cal) => {
                            self.three_head_cal = Some(cal);
                            return Ok(());
                        }
                        Err(e) => return Err(format!("Failed to load calibration from {}: {}", path, e)),
                    }
                }
            }
            
            return Err("3-head calibration file not found in expected locations".to_string());
        }
        Ok(())
    }

    /// Load 4-head calibration from CSV file if not already loaded.
    fn ensure_4head_calibration(&mut self) -> Result<(), String> {
        if self.four_head_cal.is_none() {
            // Try standard paths
            let paths = [
                "src/gl7_calibrations/4_head_cal.csv",
                "gl7_calibrations/4_head_cal.csv",
                "../gl7_calibrations/4_head_cal.csv",
            ];
            
            for path in &paths {
                if Path::new(path).exists() {
                    match FourHeadCalibration::from_csv(path) {
                        Ok(cal) => {
                            self.four_head_cal = Some(cal);
                            return Ok(());
                        }
                        Err(e) => return Err(format!("Failed to load 4-head calibration from {}: {}", path, e)),
                    }
                }
            }
            
            return Err("4-head calibration file not found in expected locations".to_string());
        }
        Ok(())
    }

    /// Load pump calibration from CSV file if not already loaded.
    fn ensure_pump_calibration(&mut self) -> Result<(), String> {
        if self.pump_cal.is_none() {
            // Try standard paths
            let paths = [
                "src/gl7_calibrations/pumps_switches_cal.csv",
                "gl7_calibrations/pumps_switches_cal.csv",
                "../gl7_calibrations/pumps_switches_cal.csv",
            ];
            
            for path in &paths {
                if Path::new(path).exists() {
                    match PumpCalibration::from_csv(path) {
                        Ok(cal) => {
                            self.pump_cal = Some(cal);
                            return Ok(());
                        }
                        Err(e) => return Err(format!("Failed to load pump calibration from {}: {}", path, e)),
                    }
                }
            }
            
            return Err("Pump calibration file not found in expected locations".to_string());
        }
        Ok(())
    }

    /// Read all inputs and return structured data for CSV recording.
    /// Errors on individual channels are represented as `None`.
    pub fn read_for_recording(&mut self) -> Ls350RecordingData {
        // ── Input A (3-head): resistance Ω + calibrated K ─────────────
        let a_sensor = self.read_sensor_raw("A").ok().and_then(|s| s.parse::<f64>().ok());
        let _ = self.ensure_3head_calibration();
        let a_temp = a_sensor.and_then(|r| {
            self.three_head_cal.as_ref()?.resistance_to_temperature(r)
        });

        // ── Input B (ADR): resistance Ω + KRDG K ──────────────────────
        let b_sensor = self.read_sensor_raw("B").ok().and_then(|s| s.parse::<f64>().ok());
        let b_temp   = self.read_kelvin_raw("B").ok().and_then(|s| s.parse::<f64>().ok());

        // ── Input C (4-head): resistance Ω + calibrated K ─────────────
        let c_sensor = self.read_sensor_raw("C").ok().and_then(|s| s.parse::<f64>().ok());
        let _ = self.ensure_4head_calibration();
        let c_temp = c_sensor.and_then(|r| {
            let adj = r + 34.56; // matches Python main.py fudge factor
            self.four_head_cal.as_ref()?.resistance_to_temperature(adj)
        });

        // ── Input D2 (switch): voltage V + pump calibrated K ──────────
        let d2_sensor = self.read_sensor_raw("D2").ok().and_then(|s| s.parse::<f64>().ok());
        let _ = self.ensure_pump_calibration();
        let d2_temp = d2_sensor.and_then(|v| {
            self.pump_cal.as_ref()?.voltage_to_temperature(v)
        });

        // ── Input D3 (4K stage): voltage V + KRDG K ───────────────────
        let d3_sensor = self.read_sensor_raw("D3").ok().and_then(|s| s.parse::<f64>().ok());
        let d3_temp   = self.read_kelvin_raw("D3").ok().and_then(|s| s.parse::<f64>().ok());

        // ── Input D4 (3-pump): voltage V + pump calibrated K ──────────
        let d4_sensor = self.read_sensor_raw("D4").ok().and_then(|s| s.parse::<f64>().ok());
        let d4_temp = d4_sensor.and_then(|v| {
            self.pump_cal.as_ref()?.voltage_to_temperature(v)
        });

        // ── Input D5 (4-pump): voltage V + pump calibrated K ──────────
        let d5_sensor = self.read_sensor_raw("D5").ok().and_then(|s| s.parse::<f64>().ok());
        let d5_temp = d5_sensor.and_then(|v| {
            self.pump_cal.as_ref()?.voltage_to_temperature(v)
        });

        Ls350RecordingData {
            input_a_sensor_ohm: a_sensor,
            input_a_temp_k:     a_temp,
            input_b_sensor_ohm: b_sensor,
            input_b_temp_k:     b_temp,
            input_c_sensor_ohm: c_sensor,
            input_c_temp_k:     c_temp,
            input_d2_sensor_v:  d2_sensor,
            input_d2_temp_k:    d2_temp,
            input_d3_sensor_v:  d3_sensor,
            input_d3_temp_k:    d3_temp,
            input_d4_sensor_v:  d4_sensor,
            input_d4_temp_k:    d4_temp,
            input_d5_sensor_v:  d5_sensor,
            input_d5_temp_k:    d5_temp,
        }
    }

    /// `SRDG? D4` + calibration → 3-pump temperature in Kelvin.
    /// Mirrors convert_pump_voltage_to_temperature() in pump_calibration.py.
    fn get_3pump_temperature_internal(&mut self) -> String {
        // Ensure calibration is loaded
        if let Err(e) = self.ensure_pump_calibration() {
            return format!("Input D4 (3-pump): ERROR ({})", e);
        }
        
        // Read voltage from Input D4
        match self.read_sensor_raw("D4") {
            Ok(v_str) => {
                if let Ok(voltage) = v_str.parse::<f64>() {
                    // Apply calibration
                    if let Some(ref cal) = self.pump_cal {
                        if let Some(temp_k) = cal.voltage_to_temperature(voltage) {
                            format!("Input D4 (3-pump): {:.4} V → {:.4} K (calibrated)", voltage, temp_k)
                        } else {
                            format!("Input D4 (3-pump): {:.4} V → ERROR (calibration failed)", voltage)
                        }
                    } else {
                        format!("Input D4 (3-pump): {:.4} V → ERROR (calibration not loaded)", voltage)
                    }
                } else {
                    format!("Input D4 (3-pump): {}", v_str)
                }
            }
            Err(e) => format!("Input D4 (3-pump): ERROR ({})", e),
        }
    }

    /// `SRDG? D5` + calibration → 4-pump temperature in Kelvin.
    /// Mirrors convert_pump_voltage_to_temperature() in pump_calibration.py.
    fn get_4pump_temperature_internal(&mut self) -> String {
        // Ensure calibration is loaded
        if let Err(e) = self.ensure_pump_calibration() {
            return format!("Input D5 (4-pump): ERROR ({})", e);
        }
        
        // Read voltage from Input D5
        match self.read_sensor_raw("D5") {
            Ok(v_str) => {
                if let Ok(voltage) = v_str.parse::<f64>() {
                    // Apply calibration
                    if let Some(ref cal) = self.pump_cal {
                        if let Some(temp_k) = cal.voltage_to_temperature(voltage) {
                            format!("Input D5 (4-pump): {:.4} V → {:.4} K (calibrated)", voltage, temp_k)
                        } else {
                            format!("Input D5 (4-pump): {:.4} V → ERROR (calibration failed)", voltage)
                        }
                    } else {
                        format!("Input D5 (4-pump): {:.4} V → ERROR (calibration not loaded)", voltage)
                    }
                } else {
                    format!("Input D5 (4-pump): {}", v_str)
                }
            }
            Err(e) => format!("Input D5 (4-pump): ERROR ({})", e),
        }
    }

    /// `SRDG? D2` + calibration → switch temperature in Kelvin.
    /// Uses the same pump calibration as D4 and D5.
    fn get_switch_temperature_internal(&mut self) -> String {
        // Ensure calibration is loaded
        if let Err(e) = self.ensure_pump_calibration() {
            return format!("Input D2 (switch): ERROR ({})", e);
        }
        
        // Read voltage from Input D2
        match self.read_sensor_raw("D2") {
            Ok(v_str) => {
                if let Ok(voltage) = v_str.parse::<f64>() {
                    // Apply calibration
                    if let Some(ref cal) = self.pump_cal {
                        if let Some(temp_k) = cal.voltage_to_temperature(voltage) {
                            format!("Input D2 (switch): {:.4} V → {:.4} K (calibrated)", voltage, temp_k)
                        } else {
                            format!("Input D2 (switch): {:.4} V → ERROR (calibration failed)", voltage)
                        }
                    } else {
                        format!("Input D2 (switch): {:.4} V → ERROR (calibration not loaded)", voltage)
                    }
                } else {
                    format!("Input D2 (switch): {}", v_str)
                }
            }
            Err(e) => format!("Input D2 (switch): ERROR ({})", e),
        }
    }

    /// `SRDG? C` + calibration → 4-head temperature in Kelvin.
    /// Mirrors convert_4head_resistance_to_temperature() in head4_calibration.py.
    fn get_4head_temperature_internal(&mut self) -> String {
        // Ensure calibration is loaded
        if let Err(e) = self.ensure_4head_calibration() {
            return format!("Input C (4-head): ERROR ({})", e);
        }
        
        // Read resistance from Input C
        match self.read_sensor_raw("C") {
            Ok(r_str) => {
                if let Ok(resistance) = r_str.parse::<f64>() {
                    // Apply fudge factor: add 34.56 ohms (matches Python main.py line 115)
                    let calibrated_resistance = resistance + 34.56;
                    
                    // Apply calibration to corrected resistance
                    if let Some(ref cal) = self.four_head_cal {
                        if let Some(temp_k) = cal.resistance_to_temperature(calibrated_resistance) {
                            format!("Input C (4-head): {:.4} Ω (raw), {:.4} Ω (calibrated) → {:.4} K", resistance, calibrated_resistance, temp_k)
                        } else {
                            format!("Input C (4-head): {:.4} Ω (raw), {:.4} Ω (calibrated) → ERROR (calibration failed)", resistance, calibrated_resistance)
                        }
                    } else {
                        format!("Input C (4-head): {:.4} Ω → ERROR (calibration not loaded)", resistance)
                    }
                } else {
                    format!("Input C (4-head): {}", r_str)
                }
            }
            Err(e) => format!("Input C (4-head): ERROR ({})", e),
        }
    }

    /// `SRDG? A` + calibration → 3-head temperature in Kelvin.
    /// Mirrors convert_3head_resistance_to_temperature() in head3_calibration.py.
    fn get_3head_temperature_internal(&mut self) -> String {
        // Ensure calibration is loaded
        if let Err(e) = self.ensure_3head_calibration() {
            return format!("Input A (3-head): ERROR ({})", e);
        }
        
        // Read resistance from Input A
        match self.read_sensor_raw("A") {
            Ok(r_str) => {
                if let Ok(resistance) = r_str.parse::<f64>() {
                    // Apply calibration
                    if let Some(ref cal) = self.three_head_cal {
                        if let Some(temp_k) = cal.resistance_to_temperature(resistance) {
                            format!("Input A (3-head): {:.4} Ω → {:.4} K (calibrated)", resistance, temp_k)
                        } else {
                            format!("Input A (3-head): {:.4} Ω → ERROR (calibration failed)", resistance)
                        }
                    } else {
                        format!("Input A (3-head): {:.4} Ω → ERROR (calibration not loaded)", resistance)
                    }
                } else {
                    format!("Input A (3-head): {}", r_str)
                }
            }
            Err(e) => format!("Input A (3-head): ERROR ({})", e),
        }
    }

    /// Intelligent input reading - shows appropriate data based on input type.
    /// Input A (3-head): resistance + calibrated temperature
    /// Input B (ADR): resistance + Kelvin  
    /// Input C (4-head): resistance + calibrated temperature
    /// Input D3 (4K stage): voltage + Kelvin
    /// Input D4 (3-pump): voltage + calibrated temperature
    /// Input D5 (4-pump): voltage + calibrated temperature
    /// Input D2 (switch): voltage only
    /// Input D1: whatever the Lakeshore shows
    pub fn read_input_intelligent(&mut self, input: &str) {
        let input = input.to_uppercase();
        if !ALL_INPUTS.contains(&input.as_str()) {
            self.error_message = Some(format!(
                "Invalid input '{}'. Must be one of: {}",
                input,
                ALL_INPUTS.join(", ")
            ));
            return;
        }

        match input.as_str() {
            "A" => {
                // 3-head: show resistance + calibrated temperature
                self.output = format!("{}\n", self.get_3head_temperature_internal());
                self.error_message = None;
            }
            "B" => {
                // ADR: show resistance + Kelvin
                let sensor = self.read_sensor_raw("B").unwrap_or_else(|e| format!("ERROR ({})", e));
                let kelvin = self.read_kelvin_raw("B").unwrap_or_else(|e| format!("ERROR ({})", e));
                let sensor_str = if let Ok(v) = sensor.parse::<f64>() {
                    format!("{:.4} Ω", v)
                } else {
                    sensor
                };
                let kelvin_str = if let Ok(v) = kelvin.parse::<f64>() {
                    format!("{:.4} K", v)
                } else {
                    kelvin
                };
                self.output = format!("Input B (ADR): {} → {}\n", sensor_str, kelvin_str);
                self.error_message = None;
            }
            "C" => {
                // 4-head: show resistance + calibrated temperature
                self.output = format!("{}\n", self.get_4head_temperature_internal());
                self.error_message = None;
            }
            "D3" => {
                // 4K stage: show voltage + Kelvin
                let sensor = self.read_sensor_raw("D3").unwrap_or_else(|e| format!("ERROR ({})", e));
                let kelvin = self.read_kelvin_raw("D3").unwrap_or_else(|e| format!("ERROR ({})", e));
                let sensor_str = if let Ok(v) = sensor.parse::<f64>() {
                    format!("{:.4} V", v)
                } else {
                    sensor
                };
                let kelvin_str = if let Ok(v) = kelvin.parse::<f64>() {
                    format!("{:.4} K", v)
                } else {
                    kelvin
                };
                self.output = format!("Input D3 (4K stage): {} → {}\n", sensor_str, kelvin_str);
                self.error_message = None;
            }
            "D4" => {
                // 3-pump: show voltage + calibrated temperature
                self.output = format!("{}\n", self.get_3pump_temperature_internal());
                self.error_message = None;
            }
            "D2" => {
                // Switch: show voltage + calibrated temperature
                self.output = format!("{}\n", self.get_switch_temperature_internal());
                self.error_message = None;
            }
            "D5" => {
                // 4-pump: show voltage + calibrated temperature
                self.output = format!("{}\n", self.get_4pump_temperature_internal());
                self.error_message = None;
            }
            _ => {
                // Other inputs: show sensor reading only
                match self.read_sensor_raw(&input) {
                    Ok(r) => {
                        let unit = sensor_unit(&input);
                        self.output = if let Ok(v) = r.parse::<f64>() {
                            format!("Input {}: {:.4} {}\n", input, v, unit)
                        } else {
                            format!("Input {}: {}\n", input, r)
                        };
                        self.error_message = None;
                    }
                    Err(e) => self.error_message = Some(e),
                }
            }
        }
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

    // ── Outputs (heaters + analog) ────────────────────────────

    /// `RANGE <output>,<range>` — set output range for output 1–4.
    /// Mirrors `set_heater_range()` in outputs.py.
    pub fn set_output_range(&mut self, output_num: u8, range_val: i32) {
        if let Err(e) = Self::validate_output(output_num) {
            self.error_message = Some(e);
            return;
        }
        match self.send_write_command(&format!("RANGE {output_num},{range_val}")) {
            Ok(()) => {
                self.output = format!("Sent: RANGE {output_num},{range_val}\n");
                self.error_message = None;
            }
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `MOUT <output>,<percent>` — set manual output percentage.
    /// Mirrors `set_outputs()` in outputs.py.
    pub fn set_output_percent(&mut self, output_num: u8, percent: f64) {
        if let Err(e) = Self::validate_output(output_num) {
            self.error_message = Some(e);
            return;
        }
        if !(0.0..=100.0).contains(&percent) {
            self.error_message = Some("Percent must be between 0 and 100.".to_string());
            return;
        }
        match self.send_write_command(&format!("MOUT {output_num},{percent}")) {
            Ok(()) => {
                self.output = format!("Set Output {output_num} to {percent}%\n");
                self.error_message = None;
            }
            Err(e) => self.error_message = Some(e),
        }
    }

    /// `HTRSET` or `ANALOG` — set output configuration parameters.
    /// Outputs 1–2 use `HTRSET <output>,<r>,<imax>,<imax_user>,<mode>`.
    /// Outputs 3–4 use `ANALOG <output>,<input>,<units>,<high>,<low>,<polarity>`.
    /// Mirrors `set_output_params()` in outputs.py.
    pub fn set_output_params(&mut self, output_num: u8, params: &[String]) {
        if let Err(e) = Self::validate_output(output_num) {
            self.error_message = Some(e);
            return;
        }
        let expected = if output_num <= 2 { 4 } else { 5 };
        if params.len() != expected {
            self.error_message = Some(format!(
                "Expected {expected} parameter(s) for output {output_num}."
            ));
            return;
        }
        let param_str = params.join(",");
        let cmd = if output_num <= 2 {
            format!("HTRSET {output_num},{param_str}")
        } else {
            format!("ANALOG {output_num},{param_str}")
        };
        match self.send_write_command(&cmd) {
            Ok(()) => {
                self.output = format!("Sent: {cmd}\n");
                self.error_message = None;
            }
            Err(e) => self.error_message = Some(e),
        }
    }

    /// Query output status using MOUT, OUTMODE, HTRSET/HTR, AOUT/ANALOG, RANGE.
    /// Mirrors `query_outputs()` in outputs.py.
    pub fn query_output(&mut self, output_num: u8) {
        if let Err(e) = Self::validate_output(output_num) {
            self.error_message = Some(e);
            return;
        }

        let mut out = String::new();
        let mut last_err: Option<String> = None;

        match self.send_command(&format!("MOUT? {output_num}")) {
            Ok(r) => out.push_str(&format!(
                "MOUT (Manual Output Percentage) {output_num} Status: {r}\n"
            )),
            Err(e) => {
                out.push_str(&format!("MOUT? {output_num}: ERROR ({e})\n"));
                last_err = Some(e);
            }
        }

        if output_num <= 2 {
            match self.send_command(&format!("HTR? {output_num}")) {
                Ok(r) => out.push_str(&format!("HTR? {output_num} : {r}\n")),
                Err(e) => {
                    out.push_str(&format!("HTR? {output_num}: ERROR ({e})\n"));
                    last_err = Some(e);
                }
            }
            match self.send_command(&format!("HTRSET? {output_num}")) {
                Ok(r) => out.push_str(&format!(
                    "HTRSET? (<htr resistance>,<max current>,<max user current>,<current/power>) {output_num} : {r}\n"
                )),
                Err(e) => {
                    out.push_str(&format!("HTRSET? {output_num}: ERROR ({e})\n"));
                    last_err = Some(e);
                }
            }
        } else {
            match self.send_command(&format!("AOUT? {output_num}")) {
                Ok(r) => out.push_str(&format!("AOUT? {output_num} Status: {r}\n")),
                Err(e) => {
                    out.push_str(&format!("AOUT? {output_num}: ERROR ({e})\n"));
                    last_err = Some(e);
                }
            }
            match self.send_command(&format!("ANALOG? {output_num}")) {
                Ok(r) => out.push_str(&format!("ANALOG? {output_num} Status: {r}\n")),
                Err(e) => {
                    out.push_str(&format!("ANALOG? {output_num}: ERROR ({e})\n"));
                    last_err = Some(e);
                }
            }
        }

        match self.send_command(&format!("OUTMODE? {output_num}")) {
            Ok(r) => out.push_str(&format!("OUTMODE? {output_num} Status: {r}\n")),
            Err(e) => {
                out.push_str(&format!("OUTMODE? {output_num}: ERROR ({e})\n"));
                last_err = Some(e);
            }
        }

        match self.send_command(&format!("RANGE? {output_num}")) {
            Ok(r) => out.push_str(&format!("RANGE? {output_num} Status: {r}\n")),
            Err(e) => {
                out.push_str(&format!("RANGE? {output_num}: ERROR ({e})\n"));
                last_err = Some(e);
            }
        }

        self.output = out;
        self.error_message = last_err;
    }

    /// Query output status for all outputs 1–4.
    /// Mirrors calling `query_outputs()` for each output in outputs.py.
    pub fn query_all_outputs(&mut self) {
        let mut out = String::new();
        let mut last_err: Option<String> = None;

        for &output_num in &ALL_OUTPUTS {
            self.query_output(output_num);
            out.push_str(&format!("Output {output_num}:\n"));
            out.push_str(&self.output);
            if !self.output.ends_with('\n') {
                out.push('\n');
            }
            if let Some(e) = self.error_message.clone() {
                last_err = Some(e);
            }
            out.push('\n');
        }

        self.output = out;
        self.error_message = last_err;
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
