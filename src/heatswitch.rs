// heatswitch.rs — Zaber T-NM17A04 stepper motor controller for FROST
//
// Wraps the Zaber binary protocol directly so no external heatswitch_driver
// binary is needed.  Blocking wait_for_idle is intentionally omitted from
// GUI-facing methods so the interface never freezes; call get_status() /
// get_position() to poll progress after issuing a move.

use serialport::SerialPort;
use std::io::{Read, Write};
use std::time::Duration;
use thiserror::Error;

// ── Default connection settings ──────────────────────────────
const DEFAULT_PORT: &str = "/dev/ttyUSB4";
const DEFAULT_BAUD: u32 = 9600;
const DEFAULT_DEVICE_ID: u8 = 1;

/// Standard travel for open / close operations (microsteps).
pub const HEATSWITCH_TRAVEL_STEPS: i32 = 115_200;

// ── Zaber binary protocol constants ──────────────────────────
const CMD_HOME: u8 = 1;
const CMD_MOVE_ABS: u8 = 20;
const CMD_MOVE_REL: u8 = 21;
const CMD_MOVE_VEL: u8 = 22;
const CMD_STOP: u8 = 23;
const CMD_GET_POS: u8 = 60;
const CMD_GET_SETTING: u8 = 53;

const SETTING_LIMIT_HOME_TRIGGERED: u32 = 103;
const SETTING_MAXSPEED: u32 = 42;
const SETTING_LIMIT_CW_TRIGGERED: u32 = 104;
const SETTING_LIMIT_CCW_TRIGGERED: u32 = 105;
const SETTING_DEVICE_STATUS: u32 = 54;

// ── Error type ───────────────────────────────────────────────
#[derive(Error, Debug)]
pub enum ZaberError {
    #[error("Serial port error: {0}")]
    Serial(#[from] serialport::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid response length: expected 6, got {0}")]
    InvalidResponseLength(usize),
    #[error("Communication timeout")]
    Timeout,
}

type ZResult<T> = std::result::Result<T, ZaberError>;

// ── Wire protocol structs ─────────────────────────────────────
#[repr(C, packed)]
struct ZaberCommand {
    device_id: u8,
    command: u8,
    data: u32,
}

#[repr(C, packed)]
struct ZaberResponse {
    device_id: u8,
    command: u8,
    data: u32,
}

// ── Low-level driver ─────────────────────────────────────────
struct ZaberDriver {
    port: Box<dyn SerialPort>,
    device_id: u8,
}

impl ZaberDriver {
    fn new(port_name: &str, baudrate: u32, device_id: u8) -> ZResult<Self> {
        let port = serialport::new(port_name, baudrate)
            .timeout(Duration::from_millis(1000))
            .open()?;
        Ok(ZaberDriver { port, device_id })
    }

    fn send_command(&mut self, command: u8, data: u32) -> ZResult<ZaberResponse> {
        let cmd = ZaberCommand {
            device_id: self.device_id,
            command,
            data: data.to_le(),
        };
        let cmd_bytes = unsafe {
            std::slice::from_raw_parts(
                &cmd as *const ZaberCommand as *const u8,
                std::mem::size_of::<ZaberCommand>(),
            )
        };
        self.port.write_all(cmd_bytes)?;

        let mut response_bytes = [0u8; 6];
        self.port.read_exact(&mut response_bytes)?;

        let response = unsafe {
            std::ptr::read(response_bytes.as_ptr() as *const ZaberResponse)
        };
        Ok(ZaberResponse {
            device_id: response.device_id,
            command: response.command,
            data: u32::from_le(response.data),
        })
    }

    // ── Motion commands (non-blocking — send and return) ────
    fn home(&mut self) -> ZResult<()> {
        self.send_command(CMD_HOME, 0)?;
        Ok(())
    }
    fn move_absolute(&mut self, position: i32) -> ZResult<()> {
        self.send_command(CMD_MOVE_ABS, position as u32)?;
        Ok(())
    }
    fn move_relative(&mut self, steps: i32) -> ZResult<()> {
        self.send_command(CMD_MOVE_REL, steps as u32)?;
        Ok(())
    }
    fn move_velocity(&mut self, velocity: i32) -> ZResult<()> {
        self.send_command(CMD_MOVE_VEL, velocity as u32)?;
        Ok(())
    }
    fn stop(&mut self) -> ZResult<()> {
        self.send_command(CMD_STOP, 0)?;
        Ok(())
    }
    fn emergency_stop(&mut self) -> ZResult<()> {
        for _ in 0..3 {
            let _ = self.send_command(CMD_STOP, 0);
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    // ── Query commands ───────────────────────────────────────
    fn get_position(&mut self) -> ZResult<i32> {
        let r = self.send_command(CMD_GET_POS, 0)?;
        Ok(r.data as i32)
    }
    fn get_home_status(&mut self) -> ZResult<bool> {
        let r = self.send_command(CMD_GET_SETTING, SETTING_LIMIT_HOME_TRIGGERED)?;
        Ok(r.data != 0)
    }
    fn get_maxspeed(&mut self) -> ZResult<u32> {
        let r = self.send_command(CMD_GET_SETTING, SETTING_MAXSPEED)?;
        Ok(r.data)
    }
    fn get_cw_limit_status(&mut self) -> ZResult<bool> {
        let r = self.send_command(CMD_GET_SETTING, SETTING_LIMIT_CW_TRIGGERED)?;
        Ok(r.data != 0)
    }
    fn get_ccw_limit_status(&mut self) -> ZResult<bool> {
        let r = self.send_command(CMD_GET_SETTING, SETTING_LIMIT_CCW_TRIGGERED)?;
        Ok(r.data != 0)
    }
    fn get_device_status(&mut self) -> ZResult<u32> {
        let r = self.send_command(CMD_GET_SETTING, SETTING_DEVICE_STATUS)?;
        Ok(r.data)
    }
}

// ── GUI-facing controller ────────────────────────────────────
pub struct HeatswitchController {
    pub port: String,
    pub baud_rate: u32,
    pub device_id: u8,
    /// Last error, shown in the GUI.
    pub error_message: Option<String>,
    /// Status / query output, shown in the status panel.
    pub status_output: String,
    /// Step count used for manual CW / CCW / MoveRel inputs.
    pub step_input: i32,
    /// Absolute position target for MoveAbs input.
    pub abs_pos_input: i32,
    /// Velocity for move-velocity input.
    pub velocity_input: i32,
}

impl Default for HeatswitchController {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT.to_string(),
            baud_rate: DEFAULT_BAUD,
            device_id: DEFAULT_DEVICE_ID,
            error_message: None,
            status_output: String::new(),
            step_input: 1000,
            abs_pos_input: 0,
            velocity_input: 0,
        }
    }
}

impl HeatswitchController {
    fn connect(&self) -> Result<ZaberDriver, String> {
        ZaberDriver::new(&self.port, self.baud_rate, self.device_id)
            .map_err(|e| e.to_string())
    }

    // ── Status ───────────────────────────────────────────────
    pub fn get_status(&mut self) {
        match self.connect() {
            Ok(mut drv) => {
                let mut out = String::new();
                match drv.get_position() {
                    Ok(p)  => out.push_str(&format!("Position:         {} microsteps\n", p)),
                    Err(e) => out.push_str(&format!("Position:         ERROR ({})\n", e)),
                }
                match drv.get_home_status() {
                    Ok(h)  => out.push_str(&format!("Homed:            {}\n", if h { "Yes" } else { "No" })),
                    Err(e) => out.push_str(&format!("Homed:            ERROR ({})\n", e)),
                }
                match drv.get_cw_limit_status() {
                    Ok(l)  => out.push_str(&format!("CW Limit:         {}\n", if l { "TRIGGERED" } else { "OK" })),
                    Err(e) => out.push_str(&format!("CW Limit:         ERROR ({})\n", e)),
                }
                match drv.get_ccw_limit_status() {
                    Ok(l)  => out.push_str(&format!("CCW Limit:        {}\n", if l { "TRIGGERED" } else { "OK" })),
                    Err(e) => out.push_str(&format!("CCW Limit:        ERROR ({})\n", e)),
                }
                match drv.get_maxspeed() {
                    Ok(s)  => out.push_str(&format!("Max Speed:        {}\n", s)),
                    Err(e) => out.push_str(&format!("Max Speed:        ERROR ({})\n", e)),
                }
                match drv.get_device_status() {
                    Ok(s)  => out.push_str(&format!("Device Status:    0x{:08X}\n", s)),
                    Err(e) => out.push_str(&format!("Device Status:    ERROR ({})\n", e)),
                }
                out.push_str(&format!("Port:             {}\n", self.port));
                out.push_str(&format!("Baud Rate:        {}\n", self.baud_rate));
                out.push_str(&format!("Device ID:        {}\n", self.device_id));
                self.status_output = out;
                self.error_message = None;
            }
            Err(e) => self.error_message = Some(format!("Connection failed: {e}")),
        }
    }

    pub fn get_position(&mut self) {
        match self.connect() {
            Ok(mut drv) => match drv.get_position() {
                Ok(p) => {
                    self.status_output = format!("Position: {} microsteps\n", p);
                    self.error_message = None;
                }
                Err(e) => self.error_message = Some(e.to_string()),
            },
            Err(e) => self.error_message = Some(format!("Connection failed: {e}")),
        }
    }

    // ── Heat-switch high-level ───────────────────────────────
    /// Open: CW 115200 steps (command sent, returns immediately).
    pub fn open(&mut self) -> Result<(), String> {
        let mut drv = self.connect()?;
        match drv.move_relative(HEATSWITCH_TRAVEL_STEPS) {
            Ok(()) => Ok(()),
            // Motor executes the command but sometimes doesn't send a timely
            // acknowledgement — treat a read timeout as success.
            Err(ZaberError::Io(e)) if e.kind() == std::io::ErrorKind::TimedOut => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Close: CCW 115200 steps (command sent, returns immediately).
    pub fn close(&mut self) -> Result<(), String> {
        let mut drv = self.connect()?;
        match drv.move_relative(-HEATSWITCH_TRAVEL_STEPS) {
            Ok(()) => Ok(()),
            Err(ZaberError::Io(e)) if e.kind() == std::io::ErrorKind::TimedOut => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    // ── Motion ───────────────────────────────────────────────
    pub fn home(&mut self) -> Result<(), String> {
        let mut drv = self.connect()?;
        drv.home().map_err(|e| e.to_string())
    }

    pub fn move_absolute(&mut self, position: i32) -> Result<(), String> {
        let mut drv = self.connect()?;
        drv.move_absolute(position).map_err(|e| e.to_string())
    }

    pub fn move_relative(&mut self, steps: i32) -> Result<(), String> {
        let mut drv = self.connect()?;
        drv.move_relative(steps).map_err(|e| e.to_string())
    }

    pub fn rotate_cw(&mut self, steps: i32) -> Result<(), String> {
        let mut drv = self.connect()?;
        drv.move_relative(steps.abs()).map_err(|e| e.to_string())
    }

    pub fn rotate_ccw(&mut self, steps: i32) -> Result<(), String> {
        let mut drv = self.connect()?;
        drv.move_relative(-steps.abs()).map_err(|e| e.to_string())
    }

    pub fn move_velocity(&mut self, velocity: i32) -> Result<(), String> {
        let mut drv = self.connect()?;
        drv.move_velocity(velocity).map_err(|e| e.to_string())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        let mut drv = self.connect()?;
        drv.stop().map_err(|e| e.to_string())
    }

    pub fn emergency_stop(&mut self) -> Result<(), String> {
        let mut drv = self.connect()?;
        drv.emergency_stop().map_err(|e| e.to_string())
    }

    pub fn reset(&mut self) -> Result<(), String> {
        let mut drv = self.connect()?;
        drv.home().map_err(|e| e.to_string())
    }

    // ── Safe (limited) moves ─────────────────────────────────
    pub fn safe_cw(&mut self, steps: i32) -> Result<(), String> {
        let clamped = steps.clamp(1, 1000);
        let mut drv = self.connect()?;
        drv.move_relative(clamped).map_err(|e| e.to_string())
    }

    pub fn safe_ccw(&mut self, steps: i32) -> Result<(), String> {
        let clamped = steps.clamp(1, 1000);
        let mut drv = self.connect()?;
        drv.move_relative(-clamped).map_err(|e| e.to_string())
    }
}
