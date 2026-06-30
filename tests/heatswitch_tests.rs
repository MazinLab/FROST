// heatswitch_tests.rs — integration tests for HeatswitchController and
// heatswitch worker state helpers.
//
// Safety rule: all tests that call controller methods use a nonexistent port
// path so the OS rejects the open before any bytes reach the instrument.
// Tests that require a real Zaber device are marked #[ignore].

use frost::heatswitch::HeatswitchController;
use frost::worker::{DeviceSnapshot, get_heatswitch_open_state_at, set_heatswitch_open_state_at};

fn bad_port_controller() -> HeatswitchController {
    HeatswitchController {
        port: "/dev/frost_no_such_port".to_string(),
        ..Default::default()
    }
}

// ── close() ──────────────────────────────────────────────────────────────────

#[test]
fn close_fails_on_bad_port() {
    let mut hs = bad_port_controller();
    let result = hs.close();
    assert!(result.is_err(), "close() must fail when the port does not exist");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("frost_no_such_port"),
        "error message should identify the port; got: {msg}"
    );
}

// ── open() ───────────────────────────────────────────────────────────────────

#[test]
fn open_fails_on_bad_port() {
    let mut hs = bad_port_controller();
    let result = hs.open();
    assert!(result.is_err(), "open() must fail when the port does not exist");
}

// ── State file helpers ────────────────────────────────────────────────────────

#[test]
fn heatswitch_state_roundtrip_open() {
    let path = std::env::temp_dir().join("frost_hs_state_open_test");
    set_heatswitch_open_state_at(&path, true);
    assert_eq!(get_heatswitch_open_state_at(&path), Some(true));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn heatswitch_state_roundtrip_closed() {
    let path = std::env::temp_dir().join("frost_hs_state_closed_test");
    set_heatswitch_open_state_at(&path, false);
    assert_eq!(get_heatswitch_open_state_at(&path), Some(false));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn heatswitch_state_absent_returns_none() {
    let path = std::env::temp_dir().join("frost_hs_state_absent_test_xyz");
    let _ = std::fs::remove_file(&path);
    assert_eq!(get_heatswitch_open_state_at(&path), None);
}

#[test]
fn device_snapshot_heatswitch_defaults_to_none() {
    let snap = DeviceSnapshot::default();
    assert!(snap.heatswitch_is_open.is_none(), "heatswitch_is_open should start as None");
    assert!(snap.heatswitch_cmd_result.is_none(), "heatswitch_cmd_result should start as None");
}

// ── Hardware tests (require physical Zaber T-NM17A04 on /dev/ttyUSB4) ────────

/// Verify that close() sends the CCW move command and returns Ok immediately.
///
/// Requires: Zaber T-NM17A04 connected to /dev/ttyUSB4.
#[test]
#[ignore = "requires Zaber T-NM17A04 on /dev/ttyUSB4"]
fn close_until_resistance_real_hardware() {
    let mut hs = HeatswitchController::default();
    let result = hs.close();
    assert!(result.is_ok(), "close() failed on real hardware: {:?}", result);
}
