// record_temps.rs — Temperature recording for FROST
//
// Reads sensor values (resistance / voltage) and calibrated temperatures from
// the LakeShore 350 (all 7 active inputs) and LakeShore 370 (input 1), then
// appends one fixed-width row to a date-stamped CSV file.
//
// Column format mirrors record_temps.py from lakeshore350-python:
//   fixed-width space-padded columns, left-justified, no commas.
//
// GUI usage:  call record_single_snapshot() — captures one row immediately.
// CLI usage:  call run_recording_loop()     — records every N seconds until Ctrl+C.
//
// CSV output: temps/<YYYY-MM-DD>_temperature_log.csv
//   Auto-increments to _1.csv, _2.csv … when a file for today already exists.

use crate::lakeshore350::LakeShore350Controller;
use crate::lakeshore370::LakeShore370Controller;
use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

// ── Column definitions ─────────────────────────────────────────
// Order mirrors record_temps.py: D3 → B → D2 → A → C → D4 → D5 → LS370
// Widths mirror self.column_widths in record_temps.py, with LS370 appended.
const HEADERS: &[&str] = &[
    "Timestamp",           // 28
    "Date",                // 12
    "Time",                // 12
    "4K_Stage_Temp_K",     // 17  (LS350 D3 kelvin)
    "ADR_Res_Ohm",         // 16  (LS350 B sensor Ω)
    "ADR_Temp_K",          // 15  (LS350 B kelvin)
    "Switch_Volt",         // 17  (LS350 D2 sensor V)
    "Switch_Temp_K",       // 17  (LS350 D2 calibrated K)
    "3_Head_Res_Ohm",      // 20  (LS350 A sensor Ω)
    "3_Head_Temp_K",       // 18  (LS350 A calibrated K)
    "4_Head_Res_Raw_Ohm",  // 20  (LS350 C sensor Ω raw)
    "4_Head_Res_Adj_Ohm",  // 20  (LS350 C sensor + 34.56 Ω)
    "4_Head_Temp_K",       // 16  (LS350 C calibrated K)
    "3_Pump_Volt",         // 17  (LS350 D4 sensor V)
    "3_Pump_Temp_K",       // 16  (LS350 D4 calibrated K)
    "4_Pump_Volt",         // 17  (LS350 D5 sensor V)
    "4_Pump_Temp_K",       // 16  (LS350 D5 calibrated K)
    "LS370_In1_Res_Ohm",   // 22  (LS370 input 1 resistance Ω)
    "LS370_In1_Temp_K",    // 18  (LS370 input 1 kelvin)
];

// Matches record_temps.py column_widths, extended for LS370
const WIDTHS: &[usize] = &[28, 12, 12, 17, 16, 15, 17, 17, 20, 18, 20, 20, 16, 17, 16, 17, 16, 22, 18];

// ── Recording-active lock file ─────────────────────────────────
// Written by start_recording_loop(); deleted when the loop stops.
// Survives process kills — on next launch the GUI will detect and resume.
const LOCK_PATH: &str = "temps/.recording_active";

/// Create the lock file that signals recording is in progress.
pub fn set_recording_active() {
    let _ = fs::write(LOCK_PATH, "");
}

/// Remove the lock file (recording has stopped).
pub fn clear_recording_active() {
    let _ = fs::remove_file(LOCK_PATH);
}

/// Returns true if the lock file exists (recording was active when last stopped).
pub fn is_recording_active() -> bool {
    Path::new(LOCK_PATH).exists()
}

// ── Data snapshot ──────────────────────────────────────────────
pub struct TemperatureRecord {
    pub timestamp: String,
    pub date:      String,
    pub time:      String,
    // LS350 — in Python column order
    pub d3_temp_k:      Option<f64>,   // 4K stage kelvin
    pub b_sensor_ohm:   Option<f64>,   // ADR resistance Ω
    pub b_temp_k:       Option<f64>,   // ADR kelvin
    pub d2_sensor_v:    Option<f64>,   // Switch voltage V
    pub d2_temp_k:      Option<f64>,   // Switch calibrated K
    pub a_sensor_ohm:   Option<f64>,   // 3-head resistance Ω
    pub a_temp_k:       Option<f64>,   // 3-head calibrated K
    pub c_sensor_raw:   Option<f64>,   // 4-head raw Ω
    pub c_sensor_adj:   Option<f64>,   // 4-head adjusted Ω (+34.56)
    pub c_temp_k:       Option<f64>,   // 4-head calibrated K
    pub d4_sensor_v:    Option<f64>,   // 3-pump voltage V
    pub d4_temp_k:      Option<f64>,   // 3-pump calibrated K
    pub d5_sensor_v:    Option<f64>,   // 4-pump voltage V
    pub d5_temp_k:      Option<f64>,   // 4-pump calibrated K
    // LS370
    pub ls370_resistance: Option<f64>,
    pub ls370_temp_k:     Option<f64>,
}

// ── Formatting helpers ─────────────────────────────────────────

/// Format an Option<f64> as a display string.
fn fmt_val(v: Option<f64>, decimals: usize) -> String {
    match v {
        Some(x) => format!("{:.prec$}", x, prec = decimals),
        None    => "N/A".to_string(),
    }
}

/// Pad a string to exactly `width` chars, left-justified.
fn pad(s: &str, width: usize) -> String {
    format!("{:<width$}", s, width = width)
}

impl TemperatureRecord {
    /// Build the ordered list of field strings that maps to HEADERS/WIDTHS.
    fn fields(&self) -> Vec<String> {
        vec![
            self.timestamp.clone(),
            self.date.clone(),
            self.time.clone(),
            fmt_val(self.d3_temp_k,    2),
            fmt_val(self.b_sensor_ohm, 4),
            fmt_val(self.b_temp_k,     3),
            fmt_val(self.d2_sensor_v,  2),
            fmt_val(self.d2_temp_k,    2),
            fmt_val(self.a_sensor_ohm, 4),
            fmt_val(self.a_temp_k,     3),
            fmt_val(self.c_sensor_raw, 4),
            fmt_val(self.c_sensor_adj, 4),
            fmt_val(self.c_temp_k,     3),
            fmt_val(self.d4_sensor_v,  2),
            fmt_val(self.d4_temp_k,    2),
            fmt_val(self.d5_sensor_v,  2),
            fmt_val(self.d5_temp_k,    2),
            fmt_val(self.ls370_resistance, 4),
            fmt_val(self.ls370_temp_k,     4),
        ]
    }

    /// Serialize to a fixed-width space-padded row (no commas), matching
    /// the format produced by record_temps.py.
    pub fn to_fixed_row(&self) -> String {
        let fields = self.fields();
        fields.iter()
            .enumerate()
            .map(|(i, v)| pad(v, WIDTHS[i]))
            .collect::<Vec<_>>()
            .join("")
            .trim_end()
            .to_string()
    }

    /// Formatted header line (same fixed-width layout).
    pub fn header_line() -> String {
        HEADERS.iter()
            .enumerate()
            .map(|(i, h)| pad(h, WIDTHS[i]))
            .collect::<Vec<_>>()
            .join("")
            .trim_end()
            .to_string()
    }

    /// Separator line of dashes, matching header width.
    pub fn separator_line() -> String {
        WIDTHS.iter().map(|&w| "-".repeat(w)).collect::<Vec<_>>().join("")
    }

    /// Human-readable single-line summary (for terminal loop output).
    pub fn to_display(&self) -> String {
        self.to_fixed_row()
    }
}

// ── CSV file helpers ───────────────────────────────────────────

/// Create a new log file for this run, auto-incrementing the suffix so each
/// run gets its own file:
///   first run  → YYYY-MM-DD_temperature_log.csv
///   second run → YYYY-MM-DD_temperature_log_2.csv
///   third run  → YYYY-MM-DD_temperature_log_3.csv  …
fn get_or_create_csv(dir: &str) -> Result<String, String> {
    let date = Local::now().format("%Y-%m-%d").to_string();
    fs::create_dir_all(dir)
        .map_err(|e| format!("Cannot create directory '{}': {}", dir, e))?;

    // Find the first path that does not yet exist
    let base = format!("{}/{}_temperature_log.csv", dir, date);
    let path = if !Path::new(&base).exists() {
        base
    } else {
        let mut n = 2u32;
        loop {
            let candidate = format!("{}/{}_temperature_log_{}.csv", dir, date, n);
            if !Path::new(&candidate).exists() {
                break candidate;
            }
            n += 1;
        }
    };

    // Create fresh file with header + separator
    let mut f = fs::File::create(&path)
        .map_err(|e| format!("Cannot create '{}': {}", path, e))?;
    writeln!(f, "{}", TemperatureRecord::header_line())
        .map_err(|e| format!("Header write error: {}", e))?;
    writeln!(f, "{}", TemperatureRecord::separator_line())
        .map_err(|e| format!("Separator write error: {}", e))?;
    Ok(path)
}

fn append_row(path: &str, row: &str) -> Result<(), String> {
    let mut f = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .map_err(|e| format!("Cannot open '{}': {}", path, e))?;
    writeln!(f, "{}", row)
        .map_err(|e| format!("Row write error: {}", e))?;
    Ok(())
}

// ── Core snapshot ──────────────────────────────────────────────

/// Collect one snapshot from both instruments.
/// Per-channel errors are stored as None — never fatal.
pub fn take_snapshot(
    ls350: &mut LakeShore350Controller,
    ls370: &mut LakeShore370Controller,
) -> TemperatureRecord {
    let now  = Local::now();
    let d    = ls350.read_for_recording();

    let ls370_resistance = ls370.read_resistance(1).ok()
        .and_then(|s| s.trim().parse::<f64>().ok());
    let ls370_temp = ls370.read_kelvin(1).ok()
        .and_then(|s| s.trim().parse::<f64>().ok());

    TemperatureRecord {
        timestamp: now.format("%Y-%m-%dT%H:%M:%S").to_string(),
        date:      now.format("%Y-%m-%d").to_string(),
        time:      now.format("%H:%M:%S").to_string(),

        d3_temp_k:    d.input_d3_temp_k,
        b_sensor_ohm: d.input_b_sensor_ohm,
        b_temp_k:     d.input_b_temp_k,
        d2_sensor_v:  d.input_d2_sensor_v,
        d2_temp_k:    d.input_d2_temp_k,
        a_sensor_ohm: d.input_a_sensor_ohm,
        a_temp_k:     d.input_a_temp_k,
        c_sensor_raw: d.input_c_sensor_ohm,
        c_sensor_adj: d.input_c_sensor_ohm.map(|r| r + 34.56),
        c_temp_k:     d.input_c_temp_k,
        d4_sensor_v:  d.input_d4_sensor_v,
        d4_temp_k:    d.input_d4_temp_k,
        d5_sensor_v:  d.input_d5_sensor_v,
        d5_temp_k:    d.input_d5_temp_k,

        ls370_resistance,
        ls370_temp_k: ls370_temp,
    }
}

// ── Public API ─────────────────────────────────────────────────

/// Take one snapshot, append to today's CSV, return status string.
pub fn record_single_snapshot(
    ls350: &mut LakeShore350Controller,
    ls370: &mut LakeShore370Controller,
    output_dir: &str,
) -> Result<String, String> {
    let record = take_snapshot(ls350, ls370);
    let path   = get_or_create_csv(output_dir)?;
    append_row(&path, &record.to_fixed_row())?;
    Ok(format!("Saved to {}  ({})", path, record.timestamp))
}

/// Spawn a background recording thread, returning the CSV path and a stop flag
/// immediately so the GUI can display the path and toggle the button.
/// Call `stop_flag.store(true, Ordering::Relaxed)` to halt the thread.
pub fn start_recording_loop(
    interval_secs: u64,
    output_dir: &str,
) -> Result<(String, Arc<AtomicBool>), String> {
    let path      = get_or_create_csv(output_dir)?;
    set_recording_active();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop_flag);
    let path_clone = path.clone();

    std::thread::spawn(move || {
        let mut ls350 = LakeShore350Controller::default();
        let mut ls370 = LakeShore370Controller::default();

        while !stop_clone.load(Ordering::Relaxed) {
            let record = take_snapshot(&mut ls350, &mut ls370);
            let _ = append_row(&path_clone, &record.to_fixed_row());

            // Sleep in 100 ms ticks so the stop flag is checked quickly
            for _ in 0..(interval_secs * 10) {
                if stop_clone.load(Ordering::Relaxed) { break; }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        // Graceful stop — clean up the lock file
        clear_recording_active();
    });

    Ok((path, stop_flag))
}

/// Record every `interval_secs` seconds, printing a formatted table to
/// stdout and appending each row to today's CSV.  Runs until Ctrl+C.
pub fn run_recording_loop(
    ls350_port: &str,
    ls350_baud: u32,
    ls370_port: &str,
    ls370_baud: u32,
    interval_secs: u64,
    output_dir: &str,
) {
    let mut ls350 = LakeShore350Controller::default();
    ls350.port      = ls350_port.to_string();
    ls350.baud_rate = ls350_baud;

    let mut ls370 = LakeShore370Controller::default();
    ls370.port      = ls370_port.to_string();
    ls370.baud_rate = ls370_baud;

    let path = match get_or_create_csv(output_dir) {
        Ok(p)  => p,
        Err(e) => { eprintln!("Error: {e}"); return; }
    };

    println!("FROST temperature recording");
    println!("Interval : {} seconds", interval_secs);
    println!("LS350    : {} @ {} baud", ls350_port, ls350_baud);
    println!("LS370    : {} @ {} baud", ls370_port, ls370_baud);
    println!("CSV file : {}", path);
    println!("Press Ctrl+C to stop.  Data is saved after each reading.");
    println!();
    println!("{}", TemperatureRecord::header_line());
    println!("{}", TemperatureRecord::separator_line());

    loop {
        let record = take_snapshot(&mut ls350, &mut ls370);
        println!("{}", record.to_fixed_row());
        if let Err(e) = append_row(&path, &record.to_fixed_row()) {
            eprintln!("Warning: could not write row: {e}");
        }
        std::thread::sleep(std::time::Duration::from_secs(interval_secs));
    }
}
