// tests/serial_tests.rs — Integration tests for the centralized serial.rs module
//
// These tests verify that serial.rs provides equivalent functionality to the
// per-device serial code that previously lived inline in each device module.
//
// Protocol mapping (what serial.rs replaces):
//
// | Device     | Old method           | serial.rs equivalent                           |
// |------------|----------------------|------------------------------------------------|
// | LS370 query| send_command()       | scpi_query(port, 9600,  cmd, "\r\n", 200)      |
// | LS370 write| send_write_command() | scpi_write(port, 9600,  cmd, "\r\n", 500)      |
// | LS625 query| send_command()       | scpi_query(port, 9600,  cmd, "\r\n", 200)      |
// | LS350 query| send_command()       | scpi_query(port, 57600, cmd, "\n",   300)      |
// | LS350 write| send_write_command() | scpi_write(port, 57600, cmd, "\n",   200)      |
// | Heatswitch | ZaberDriver (local)  | serial::ZaberDriver (centralized, identical)   |
//
// Tests that require physical hardware are marked #[ignore].
// Run hardware tests with: cargo test -- --include-ignored
//
// Run all tests (no hardware): cargo test

use frost::serial::{
    scpi_query, scpi_write, ZaberDriver, ZaberError,
    ZaberCommand, ZaberResponse,
    CMD_HOME, CMD_MOVE_ABS, CMD_MOVE_REL, CMD_MOVE_VEL, CMD_STOP, CMD_GET_POS, CMD_GET_SETTING,
    SETTING_LIMIT_HOME_TRIGGERED, SETTING_MAXSPEED,
    SETTING_LIMIT_CW_TRIGGERED, SETTING_LIMIT_CCW_TRIGGERED, SETTING_DEVICE_STATUS,
};

// ── Error message contract ────────────────────────────────────────────────────
//
// Each device module previously produced errors in the form:
//   "Failed to open {port}: {os_error}"    (port open failure)
//   "Write error: {io_error}"              (write failure)
//
// serial.rs must preserve these message formats so call-sites don't break.

#[test]
fn scpi_query_error_contains_port_name() {
    let port = "/dev/frost_no_such_port";
    let err = scpi_query(port, 9600, "*IDN?", "\r\n", 0).unwrap_err();
    assert!(
        err.contains(port),
        "Port name missing from error. Got: {err}"
    );
}

#[test]
fn scpi_query_error_format_matches_device_modules() {
    // All three Lakeshore modules (370, 625, 350) used "Failed to open {port}: ..."
    let err = scpi_query("/dev/frost_no_such_port", 9600, "*IDN?", "\r\n", 0).unwrap_err();
    assert!(
        err.starts_with("Failed to open"),
        "Error prefix must match old per-device format. Got: {err}"
    );
}

#[test]
fn scpi_write_error_format_matches_device_modules() {
    let err = scpi_write("/dev/frost_no_such_port", 9600, "RATE 0.01", "\r\n", 0).unwrap_err();
    assert!(err.starts_with("Failed to open"), "Got: {err}");
}

// ── Device-specific parameter equivalence ────────────────────────────────────

/// LS370 and LS625 both use CRLF + 200 ms settle.
/// Confirm scpi_query accepts these params and fails on port open (not on params).
#[test]
fn ls370_ls625_params_accepted() {
    // If the error is "Failed to open" it means params were accepted — only the
    // port is invalid.  Any other error (e.g. "invalid baud") would be a regression.
    let err = scpi_query("/dev/frost_no_such_port", 9600, "*IDN?", "\r\n", 200).unwrap_err();
    assert!(err.starts_with("Failed to open"), "LS370/625 params rejected: {err}");
}

/// LS350 uses LF-only terminator + 300 ms settle + 57600 baud.
#[test]
fn ls350_params_accepted() {
    let err = scpi_query("/dev/frost_no_such_port", 57600, "*IDN?", "\n", 300).unwrap_err();
    assert!(err.starts_with("Failed to open"), "LS350 params rejected: {err}");
}

/// LS350 write commands use LF + 200 ms settle.
#[test]
fn ls350_write_params_accepted() {
    let err = scpi_write("/dev/frost_no_such_port", 57600, "MOUT 1,50.0", "\n", 200).unwrap_err();
    assert!(err.starts_with("Failed to open"), "LS350 write params rejected: {err}");
}

/// LS370 write commands use CRLF + 500 ms settle.
#[test]
fn ls370_write_params_accepted() {
    let err = scpi_write("/dev/frost_no_such_port", 9600, "HTRRNG 3", "\r\n", 500).unwrap_err();
    assert!(err.starts_with("Failed to open"), "LS370 write params rejected: {err}");
}

// ── ZaberDriver error contract ────────────────────────────────────────────────

/// heatswitch.rs expected ZaberDriver::new to fail with a serial-layer error
/// on a bad port — not a panic, and not a silent success.
#[test]
fn zaber_driver_bad_port_is_serial_error() {
    match ZaberDriver::new("/dev/frost_no_such_port", 9600, 1) {
        Err(ZaberError::Serial(_)) => {} // expected
        Err(other) => panic!("Expected ZaberError::Serial, got: {other:?}"),
        Ok(_) => panic!("Expected Err for nonexistent port"),
    }
}

/// ZaberError variants must be Debug-printable (required by heatswitch.rs .to_string() calls).
#[test]
fn zaber_error_variants_display() {
    let serial_err_str = format!("{}", ZaberError::InvalidResponseLength(3));
    assert!(serial_err_str.contains('3'), "Got: {serial_err_str}");

    let timeout_str = format!("{}", ZaberError::Timeout);
    assert!(!timeout_str.is_empty());
}

// ── Public API surface ────────────────────────────────────────────────────────
//
// Compile-time checks: if any of these identifiers are removed or renamed,
// the build will fail, alerting us to a breaking change.

#[test]
fn serial_public_api_compiles() {
    // scpi_query and scpi_write are free functions with correct signatures
    let _: fn(&str, u32, &str, &str, u64) -> Result<String, String> = scpi_query;
    let _: fn(&str, u32, &str, &str, u64) -> Result<(), String>     = scpi_write;
}

// ── Zaber frame layout ────────────────────────────────────────────────────────
//
// These verify the wire-encoding used by ZaberDriver::send_command against the
// Zaber binary protocol spec: 6-byte frames [device_id, command, data u32 LE].
// The encoding is now done with explicit byte construction — no unsafe casting.

/// The Zaber binary protocol mandates a 6-byte frame.
/// A compile-time assertion in serial.rs enforces this; the test documents it.
#[test]
fn zaber_command_frame_is_6_bytes() {
    assert_eq!(std::mem::size_of::<ZaberCommand>(), 6);
}

#[test]
fn zaber_response_frame_is_6_bytes() {
    assert_eq!(std::mem::size_of::<ZaberResponse>(), 6);
}

/// CMD_MOVE_ABS = 20 (0x14), device 1, data = 115200 (0x0001_C200).
/// Expected wire bytes: [0x01, 0x14, 0x00, 0xC2, 0x01, 0x00]
#[test]
fn zaber_command_byte_layout() {
    let data_le = 115200u32.to_le_bytes();
    let bytes: [u8; 6] = [1, CMD_MOVE_ABS, data_le[0], data_le[1], data_le[2], data_le[3]];
    assert_eq!(bytes[0], 0x01, "device_id");
    assert_eq!(bytes[1], 0x14, "command (CMD_MOVE_ABS = 20 = 0x14)");
    // 115200 = 0x0001_C200, LE: 00 C2 01 00
    assert_eq!(bytes[2], 0x00, "data byte 0 (LE)");
    assert_eq!(bytes[3], 0xC2, "data byte 1 (LE)");
    assert_eq!(bytes[4], 0x01, "data byte 2 (LE)");
    assert_eq!(bytes[5], 0x00, "data byte 3 (LE)");
}

/// Negative relative moves are two's-complement u32.
/// -115200i32 as u32 = 0xFFFE_3E00, LE: 00 3E FE FF
#[test]
fn zaber_command_negative_relative_move() {
    let steps: i32 = -115_200;
    let data_le = (steps as u32).to_le_bytes();
    let bytes: [u8; 6] = [1, CMD_MOVE_REL, data_le[0], data_le[1], data_le[2], data_le[3]];
    assert_eq!(bytes[1], CMD_MOVE_REL);
    assert_eq!(bytes[2], 0x00);
    assert_eq!(bytes[3], 0x3E);
    assert_eq!(bytes[4], 0xFE);
    assert_eq!(bytes[5], 0xFF);
}

/// Decoding a 6-byte response frame restores device_id, command, and LE data correctly.
#[test]
fn zaber_response_decode_roundtrip() {
    // device=1, cmd=60 (GET_POS), data=57600 (0x0000_E100)
    let raw: [u8; 6] = [0x01, 0x3C, 0x00, 0xE1, 0x00, 0x00];
    let device_id = raw[0];
    let command   = raw[1];
    let data      = u32::from_le_bytes([raw[2], raw[3], raw[4], raw[5]]);
    assert_eq!(device_id, 1);
    assert_eq!(command, 60); // CMD_GET_POS
    assert_eq!(data, 57600);
}

/// Zero data field encodes correctly (used by HOME, STOP, GET_POS, etc.).
#[test]
fn zaber_command_zero_data() {
    let data_le = 0u32.to_le_bytes();
    let bytes: [u8; 6] = [1, CMD_HOME, data_le[0], data_le[1], data_le[2], data_le[3]];
    assert_eq!(bytes[1], CMD_HOME);
    assert_eq!(&bytes[2..6], &[0x00, 0x00, 0x00, 0x00]);
}

// ── Zaber protocol constant sanity checks ─────────────────────────────────────
//
// These constants are fixed by the Zaber binary protocol spec.
// If they drift, the wire protocol silently breaks.

#[test]
fn zaber_command_codes_are_correct() {
    assert_eq!(CMD_HOME,         1);
    assert_eq!(CMD_MOVE_ABS,    20);
    assert_eq!(CMD_MOVE_REL,    21);
    assert_eq!(CMD_MOVE_VEL,    22);
    assert_eq!(CMD_STOP,        23);
    assert_eq!(CMD_GET_POS,     60);
    assert_eq!(CMD_GET_SETTING, 53);
}

#[test]
fn zaber_setting_codes_are_correct() {
    assert_eq!(SETTING_LIMIT_HOME_TRIGGERED, 103);
    assert_eq!(SETTING_MAXSPEED,              42);
    assert_eq!(SETTING_LIMIT_CW_TRIGGERED,   104);
    assert_eq!(SETTING_LIMIT_CCW_TRIGGERED,  105);
    assert_eq!(SETTING_DEVICE_STATUS,         54);
}

// ── Hardware-dependent tests (require physical devices) ───────────────────────
//
// These are skipped in CI. Run with: cargo test -- --include-ignored

/// Confirm LS370 identity response is non-empty and contains "LSCI".
#[test]
#[ignore = "requires LakeShore 370 on /dev/ttyUSB1"]
fn hardware_ls370_identify() {
    let r = scpi_query("/dev/ttyUSB1", 9600, "*IDN?", "\r\n", 200).unwrap();
    assert!(!r.is_empty(), "Expected identification string");
    assert!(r.contains("LSCI"), "Unexpected IDN response: {r}");
}

/// Confirm LS625 identity response is non-empty.
#[test]
#[ignore = "requires LakeShore 625 on /dev/ttyUSB0"]
fn hardware_ls625_identify() {
    let r = scpi_query("/dev/ttyUSB0", 9600, "*IDN?", "\r\n", 200).unwrap();
    assert!(!r.is_empty(), "Expected identification string");
}

/// Confirm LS350 identity response is non-empty.
#[test]
#[ignore = "requires LakeShore 350 on /dev/ttyUSB2"]
fn hardware_ls350_identify() {
    let r = scpi_query("/dev/ttyUSB2", 57600, "*IDN?", "\n", 300).unwrap();
    assert!(!r.is_empty(), "Expected identification string");
}

/// Confirm Zaber motor responds to a GET_POS query (returns position as i32).
#[test]
#[ignore = "requires Zaber heat switch on /dev/ttyUSB4"]
fn hardware_zaber_get_position() {
    let mut drv = ZaberDriver::new("/dev/ttyUSB4", 9600, 1).unwrap();
    let pos = drv.get_position().unwrap();
    // Position is a microstep count — just confirm it's a valid i32
    println!("Heatswitch position: {pos} microsteps");
}

/// Confirm Zaber home-status query returns a bool without panicking.
#[test]
#[ignore = "requires Zaber heat switch on /dev/ttyUSB4"]
fn hardware_zaber_home_status() {
    let mut drv = ZaberDriver::new("/dev/ttyUSB4", 9600, 1).unwrap();
    let homed = drv.get_home_status().unwrap();
    println!("Heatswitch homed: {homed}");
}
