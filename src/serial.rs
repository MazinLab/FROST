// serial.rs — shared serial I/O for FROST
//
// SCPI text protocol (Lakeshore 625, 370, 350):
//   7-bit data, odd parity, 1 stop bit, 2 s timeout.
//   TX terminator and post-write settling time vary per device.
//
// Zaber binary protocol (heat switch):
//   8N1, 1 s timeout, 6-byte little-endian frames.

use serialport::{DataBits, Parity, SerialPort, StopBits};
use std::io::{Read, Write};
use std::time::Duration;
use thiserror::Error;

// ── SCPI text protocol ────────────────────────────────────────

/// Send a SCPI query command and return the trimmed response line.
///
/// All three Lakeshore devices share 7O1 framing; only the TX terminator and
/// settling time differ:
///
/// | Device  | `tx_term` | `settle_ms` |
/// |---------|-----------|-------------|
/// | LS 625  | `"\r\n"`  | 200         |
/// | LS 370  | `"\r\n"`  | 200         |
/// | LS 350  | `"\n"`    | 300         |
pub fn scpi_query(
    port: &str,
    baud: u32,
    command: &str,
    tx_term: &str,
    settle_ms: u64,
) -> Result<String, String> {
    let mut handle = open_scpi_port(port, baud)?;
    handle.clear(serialport::ClearBuffer::Input).ok();
    handle
        .write_all(format!("{command}{tx_term}").as_bytes())
        .map_err(|e| format!("Write error: {e}"))?;

    std::thread::sleep(Duration::from_millis(settle_ms));

    let mut response = String::new();
    let mut byte = [0u8; 1];
    loop {
        match handle.read(&mut byte) {
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

/// Send a SCPI write command with no response expected.
///
/// Same framing as [`scpi_query`] but skips the read step.
/// Used for configuration commands on Lakeshore 370 (500 ms) and 350 (200 ms).
pub fn scpi_write(
    port: &str,
    baud: u32,
    command: &str,
    tx_term: &str,
    settle_ms: u64,
) -> Result<(), String> {
    let mut handle = open_scpi_port(port, baud)?;
    handle.clear(serialport::ClearBuffer::Input).ok();
    handle
        .write_all(format!("{command}{tx_term}").as_bytes())
        .map_err(|e| format!("Write error: {e}"))?;
    std::thread::sleep(Duration::from_millis(settle_ms));
    Ok(())
}

fn open_scpi_port(port: &str, baud: u32) -> Result<Box<dyn SerialPort>, String> {
    loop {
        match serialport::new(port, baud)
            .data_bits(DataBits::Seven)
            .parity(Parity::Odd)
            .stop_bits(StopBits::One)
            .timeout(Duration::from_millis(2000))
            .open()
        {
            Ok(handle) => return Ok(handle),
            Err(e) if e.to_string().contains("Device or resource busy") => {
                eprintln!("Port {port} busy, retrying in 15 s…");
                std::thread::sleep(Duration::from_secs(15));
            }
            Err(e) => return Err(format!("Failed to open {port}: {e}")),
        }
    }
}

// ── Zaber binary protocol ─────────────────────────────────────

// Command codes — fixed by Zaber binary protocol spec
pub const CMD_HOME: u8 = 1;
pub const CMD_MOVE_ABS: u8 = 20;
pub const CMD_MOVE_REL: u8 = 21;
pub const CMD_MOVE_VEL: u8 = 22;
pub const CMD_STOP: u8 = 23;
pub const CMD_GET_POS: u8 = 60;
pub const CMD_GET_SETTING: u8 = 53;

// Setting codes — fixed by Zaber binary protocol spec
pub const SETTING_LIMIT_HOME_TRIGGERED: u32 = 103;
pub const SETTING_MAXSPEED: u32 = 42;
pub const SETTING_LIMIT_CW_TRIGGERED: u32 = 104;
pub const SETTING_LIMIT_CCW_TRIGGERED: u32 = 105;
pub const SETTING_DEVICE_STATUS: u32 = 54;

#[derive(Error, Debug)]
#[allow(dead_code)]
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

/// Wire-format frame sent to a Zaber device (6 bytes, packed LE).
#[repr(C, packed)]
pub struct ZaberCommand {
    pub device_id: u8,
    pub command: u8,
    pub data: u32,
}

/// Wire-format frame received from a Zaber device (6 bytes, packed LE).
#[repr(C, packed)]
pub struct ZaberResponse {
    pub device_id: u8,
    pub command: u8,
    pub data: u32,
}

// Compile-time guarantee: struct layout must stay 6 bytes to match the wire protocol.
const _: () = assert!(std::mem::size_of::<ZaberCommand>()  == 6);
const _: () = assert!(std::mem::size_of::<ZaberResponse>() == 6);

pub struct ZaberDriver {
    port: Box<dyn SerialPort>,
    device_id: u8,
}

impl ZaberDriver {
    pub fn new(port_name: &str, baudrate: u32, device_id: u8) -> ZResult<Self> {
        let port = serialport::new(port_name, baudrate)
            .timeout(Duration::from_millis(1000))
            .open()?;
        Ok(ZaberDriver { port, device_id })
    }

    fn send_command(&mut self, command: u8, data: u32) -> ZResult<ZaberResponse> {
        // Serialise: device_id, command, data in little-endian — 6 bytes total.
        let data_le = data.to_le_bytes();
        let cmd_bytes: [u8; 6] = [
            self.device_id, command,
            data_le[0], data_le[1], data_le[2], data_le[3],
        ];
        self.port.write_all(&cmd_bytes)?;

        // Deserialise the 6-byte response frame.
        let mut buf = [0u8; 6];
        self.port.read_exact(&mut buf)?;

        Ok(ZaberResponse {
            device_id: buf[0],
            command:   buf[1],
            data: u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]),
        })
    }

    pub fn home(&mut self) -> ZResult<()> {
        self.send_command(CMD_HOME, 0)?;
        Ok(())
    }
    pub fn move_absolute(&mut self, position: i32) -> ZResult<()> {
        self.send_command(CMD_MOVE_ABS, position as u32)?;
        Ok(())
    }
    pub fn move_relative(&mut self, steps: i32) -> ZResult<()> {
        self.send_command(CMD_MOVE_REL, steps as u32)?;
        Ok(())
    }
    pub fn move_velocity(&mut self, velocity: i32) -> ZResult<()> {
        self.send_command(CMD_MOVE_VEL, velocity as u32)?;
        Ok(())
    }
    pub fn stop(&mut self) -> ZResult<()> {
        self.send_command(CMD_STOP, 0)?;
        Ok(())
    }
    pub fn emergency_stop(&mut self) -> ZResult<()> {
        for _ in 0..3 {
            let _ = self.send_command(CMD_STOP, 0);
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
    pub fn get_position(&mut self) -> ZResult<i32> {
        let r = self.send_command(CMD_GET_POS, 0)?;
        Ok(r.data as i32)
    }
    pub fn get_home_status(&mut self) -> ZResult<bool> {
        let r = self.send_command(CMD_GET_SETTING, SETTING_LIMIT_HOME_TRIGGERED)?;
        Ok(r.data != 0)
    }
    pub fn get_maxspeed(&mut self) -> ZResult<u32> {
        let r = self.send_command(CMD_GET_SETTING, SETTING_MAXSPEED)?;
        Ok(r.data)
    }
    pub fn get_cw_limit_status(&mut self) -> ZResult<bool> {
        let r = self.send_command(CMD_GET_SETTING, SETTING_LIMIT_CW_TRIGGERED)?;
        Ok(r.data != 0)
    }
    pub fn get_ccw_limit_status(&mut self) -> ZResult<bool> {
        let r = self.send_command(CMD_GET_SETTING, SETTING_LIMIT_CCW_TRIGGERED)?;
        Ok(r.data != 0)
    }
    pub fn get_device_status(&mut self) -> ZResult<u32> {
        let r = self.send_command(CMD_GET_SETTING, SETTING_DEVICE_STATUS)?;
        Ok(r.data)
    }
}

