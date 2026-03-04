// compressor.rs — Cryomech pulse tube compressor controller for FROST
//
// Wraps cryomech_api directly so no external cryomech_driver binary is needed.
// See README.md for how to make the cryomech_api path dependency available.

use cryomech_api::{CryomechApiSmdpBuilder, SmdpVersion};
use cryomech_api::api::CryomechApiSmdp;

// ── Default connection settings ──────────────────────────────
// Adjust to match your hardware.
const DEFAULT_PORT: &str = "/dev/ttyUSB3";
const DEFAULT_BAUD: u32 = 115200;
const DEFAULT_ADDR: u8 = 16;

type Api = CryomechApiSmdp<Box<dyn serialport::SerialPort>>;

// ── Controller state ─────────────────────────────────────────
pub struct CryomechController {
    pub port: String,
    pub baud_rate: u32,
    pub device_addr: u8,
    /// Last error message, shown in the GUI.
    pub error_message: Option<String>,
    /// Output from get_status(), shown in the status panel.
    pub status_output: String,
    /// Output from get_all_readings() / get_temperature() / get_pressure() /
    /// get_system_info(), shown in the "All Readings" panel.
    pub all_output: String,
}

impl Default for CryomechController {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT.to_string(),
            baud_rate: DEFAULT_BAUD,
            device_addr: DEFAULT_ADDR,
            error_message: None,
            status_output: String::new(),
            all_output: String::new(),
        }
    }
}

impl CryomechController {
    // ── Internal: open serial connection ────────────────────
    fn connect(&self) -> Result<Api, String> {
        CryomechApiSmdpBuilder::new(&self.port)
            .baud(self.baud_rate)
            .device_addr(self.device_addr)
            .version(SmdpVersion::V2)
            .read_timeout_ms(1000)
            .build()
            .map_err(|e| e.to_string())
    }

    // ── Status ───────────────────────────────────────────────
    /// Refresh running state, runtime, and error flags into `status_output`.
    pub fn get_status(&mut self) {
        match self.connect() {
            Ok(mut api) => {
                let mut out = String::new();
                match api.comp_on() {
                    Ok(r)  => out.push_str(&format!("Running:         {}\n", if r { "Yes" } else { "No" })),
                    Err(e) => out.push_str(&format!("Running:         ERROR ({})\n", e)),
                }
                match api.comp_minutes() {
                    Ok(m)  => out.push_str(&format!("Runtime:         {:.1} hrs  ({} min)\n", m as f32 / 60.0, m)),
                    Err(e) => out.push_str(&format!("Runtime:         ERROR ({})\n", e)),
                }
                match api.err_code_status() {
                    Ok(e)  => out.push_str(&format!("Errors/Warnings: {}\n", if e { "Yes" } else { "No" })),
                    Err(e) => out.push_str(&format!("Errors/Warnings: ERROR ({})\n", e)),
                }
                self.status_output = out;
                self.error_message = None;
            }
            Err(e) => self.error_message = Some(format!("Connection failed: {e}")),
        }
    }

    // ── Start / Stop ─────────────────────────────────────────
    pub fn start_compressor(&mut self) -> Result<(), String> {
        let mut api = self.connect()?;
        api.start_compressor()
            .map_err(|e| e.to_string())
            .and_then(|ok| {
                if ok { Ok(()) }
                else  { Err("Start command sent but verification failed".to_string()) }
            })
    }

    pub fn stop_compressor(&mut self) -> Result<(), String> {
        let mut api = self.connect()?;
        api.stop_compressor()
            .map_err(|e| e.to_string())
            .and_then(|ok| {
                if ok { Ok(()) }
                else  { Err("Stop command sent but verification failed".to_string()) }
            })
    }

    // ── Temperatures ─────────────────────────────────────────
    /// Read all temperature sensors into `all_output`.
    pub fn get_temperature(&mut self) -> Result<(), String> {
        let mut api = self.connect()?;
        let mut out = String::from("Temperature Readings:\n");

        // Current
        if let Ok(t) = api.input_water_temp()  { out.push_str(&format!("  Input Water:   {:.1} °C\n", t)); }
        if let Ok(t) = api.output_water_temp() { out.push_str(&format!("  Output Water:  {:.1} °C\n", t)); }
        if let Ok(t) = api.helium_temp()       { out.push_str(&format!("  Helium Gas:    {:.1} °C\n", t)); }
        if let Ok(t) = api.oil_temp()          { out.push_str(&format!("  Oil:           {:.1} °C\n", t)); }
        if let Ok(t) = api.cpu_temp()          { out.push_str(&format!("  CPU:           {:.1} °C\n", t)); }

        // Minimums
        out.push_str("  Minimums:\n");
        if let Ok(t) = api.min_input_water_temp()  { out.push_str(&format!("    Input Water:   {:.1} °C\n", t)); }
        if let Ok(t) = api.min_output_water_temp() { out.push_str(&format!("    Output Water:  {:.1} °C\n", t)); }
        if let Ok(t) = api.min_helium_temp()       { out.push_str(&format!("    Helium Gas:    {:.1} °C\n", t)); }
        if let Ok(t) = api.min_oil_temp()          { out.push_str(&format!("    Oil:           {:.1} °C\n", t)); }

        // Maximums
        out.push_str("  Maximums:\n");
        if let Ok(t) = api.max_input_water_temp()  { out.push_str(&format!("    Input Water:   {:.1} °C\n", t)); }
        if let Ok(t) = api.max_output_water_temp() { out.push_str(&format!("    Output Water:  {:.1} °C\n", t)); }
        if let Ok(t) = api.max_helium_temp()       { out.push_str(&format!("    Helium Gas:    {:.1} °C\n", t)); }
        if let Ok(t) = api.max_oil_temp()          { out.push_str(&format!("    Oil:           {:.1} °C\n", t)); }

        if let Ok(fail) = api.temp_sensor_fail() {
            out.push_str(&format!("  Sensor status: {}\n", if fail { "FAILED" } else { "OK" }));
        }

        self.all_output = out;
        Ok(())
    }

    // ── Pressures ────────────────────────────────────────────
    /// Read all pressure sensors into `all_output`.
    pub fn get_pressure(&mut self) -> Result<(), String> {
        let mut api = self.connect()?;
        let mut out = String::from("Pressure Readings:\n");

        // Current
        if let Ok(p) = api.high_side_pressure() { out.push_str(&format!("  High Side:     {:.1} PSI\n", p)); }
        if let Ok(p) = api.low_side_pressure()  { out.push_str(&format!("  Low Side:      {:.1} PSI\n", p)); }

        // Averages
        if let Ok(p) = api.avg_high_side_pressure() { out.push_str(&format!("  Avg High Side: {:.1} PSI\n", p)); }
        if let Ok(p) = api.avg_low_side_pressure()  { out.push_str(&format!("  Avg Low Side:  {:.1} PSI\n", p)); }

        // Minimums
        out.push_str("  Minimums:\n");
        if let Ok(p) = api.min_high_side_pressure() { out.push_str(&format!("    High Side:   {:.1} PSI\n", p)); }
        if let Ok(p) = api.min_low_side_pressure()  { out.push_str(&format!("    Low Side:    {:.1} PSI\n", p)); }

        // Maximums
        out.push_str("  Maximums:\n");
        if let Ok(p) = api.max_high_side_pressure() { out.push_str(&format!("    High Side:   {:.1} PSI\n", p)); }
        if let Ok(p) = api.max_low_side_pressure()  { out.push_str(&format!("    Low Side:    {:.1} PSI\n", p)); }

        if let Ok(d) = api.avg_delta_pressure()       { out.push_str(&format!("  Avg Delta:     {:.1} PSI\n", d)); }
        if let Ok(b) = api.high_side_pressure_deriv() { out.push_str(&format!("  High Bounce:   {:.1} PSI\n", b)); }
        if let Ok(c) = api.motor_current_amps()       { out.push_str(&format!("  Motor Current: {} A\n", c)); }

        if let Ok(fail) = api.pressure_sensor_fail() {
            out.push_str(&format!("  Sensor status: {}\n", if fail { "FAILED" } else { "OK" }));
        }

        self.all_output = out;
        Ok(())
    }

    // ── System info ───────────────────────────────────────────
    /// Read firmware/hardware info into `all_output`.
    pub fn get_system_info(&mut self) -> Result<(), String> {
        let mut api = self.connect()?;
        let mut out = String::from("System Information:\n");

        if let Ok(cs)  = api.fw_checksum()    { out.push_str(&format!("  Firmware Checksum:  0x{:08X}\n", cs)); }
        if let Ok(ml)  = api.mem_loss()        { out.push_str(&format!("  Memory Loss:        {}\n", if ml { "Yes" } else { "No" })); }
        if let Ok(t)   = api.cpu_temp()        { out.push_str(&format!("  CPU Temperature:    {:.1} °C\n", t)); }
        if let Ok(ok)  = api.clock_batt_ok()   { out.push_str(&format!("  Clock Battery OK:   {}\n", if ok { "Yes" } else { "No" })); }
        if let Ok(low) = api.clock_batt_low()  { out.push_str(&format!("  Clock Battery Low:  {}\n", if low { "Yes" } else { "No" })); }

        self.all_output = out;
        Ok(())
    }

    // ── All readings ─────────────────────────────────────────
    /// Read status + system info + all temps + all pressures into `all_output`.
    pub fn get_all_readings(&mut self) {
        match self.connect() {
            Ok(mut api) => {
                let mut out = String::new();

                // Status
                out.push_str("=== Compressor Status ===\n");
                if let Ok(r)   = api.comp_on()         { out.push_str(&format!("  Running:           {}\n", if r { "Yes" } else { "No" })); }
                if let Ok(min) = api.comp_minutes()     { out.push_str(&format!("  Runtime:           {:.1} hrs ({} min)\n", min as f32 / 60.0, min)); }
                if let Ok(e)   = api.err_code_status()  { out.push_str(&format!("  Errors/Warnings:   {}\n", if e { "Yes" } else { "No" })); }
                out.push('\n');

                // System info
                out.push_str("=== System Information ===\n");
                if let Ok(cs) = api.fw_checksum()   { out.push_str(&format!("  Firmware Checksum: 0x{:08X}\n", cs)); }
                if let Ok(ml) = api.mem_loss()       { out.push_str(&format!("  Memory Loss:       {}\n", if ml { "Yes" } else { "No" })); }
                if let Ok(t)  = api.cpu_temp()       { out.push_str(&format!("  CPU Temp:          {:.1} °C\n", t)); }
                if let Ok(ok) = api.clock_batt_ok()  { out.push_str(&format!("  Clock Battery OK:  {}\n", if ok { "Yes" } else { "No" })); }
                out.push('\n');

                // Temperatures
                out.push_str("=== Temperatures ===\n");
                if let Ok(t) = api.input_water_temp()  { out.push_str(&format!("  Input Water:   {:.1} °C\n", t)); }
                if let Ok(t) = api.output_water_temp() { out.push_str(&format!("  Output Water:  {:.1} °C\n", t)); }
                if let Ok(t) = api.helium_temp()       { out.push_str(&format!("  Helium Gas:    {:.1} °C\n", t)); }
                if let Ok(t) = api.oil_temp()          { out.push_str(&format!("  Oil:           {:.1} °C\n", t)); }
                out.push('\n');

                // Pressures
                out.push_str("=== Pressures ===\n");
                if let Ok(p) = api.high_side_pressure() { out.push_str(&format!("  High Side:     {:.1} PSI\n", p)); }
                if let Ok(p) = api.low_side_pressure()  { out.push_str(&format!("  Low Side:      {:.1} PSI\n", p)); }
                if let Ok(c) = api.motor_current_amps() { out.push_str(&format!("  Motor Current: {} A\n", c)); }

                self.all_output = out;
                self.error_message = None;
            }
            Err(e) => self.error_message = Some(format!("Connection failed: {e}")),
        }
    }

    // ── Clear min/max ─────────────────────────────────────────
    pub fn clear_min_max(&mut self) -> Result<(), String> {
        let mut api = self.connect()?;
        api.clear_press_temp_min_max().map_err(|e| e.to_string())
    }
}
