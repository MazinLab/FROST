// heatswitch_tests.rs — integration tests for HeatswitchController
//
// Safety rule: all tests that call controller methods use a nonexistent port
// path so the OS rejects the open before any bytes reach the instrument.
// Tests that require a real Zaber device are marked #[ignore].

use frost::heatswitch::HeatswitchController;

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

// ── Hardware tests (require physical Zaber T-NM17A04 on /dev/ttyUSB4) ────────

/// Verify that close() drives the motor until stall and returns Ok.
///
/// Requires: Zaber T-NM17A04 connected to /dev/ttyUSB4, heat switch mechanism
/// free to move and able to reach its mechanical close stop within 30 seconds.
#[test]
#[ignore = "requires Zaber T-NM17A04 on /dev/ttyUSB4"]
fn close_until_resistance_real_hardware() {
    let mut hs = HeatswitchController::default();
    let result = hs.close();
    assert!(result.is_ok(), "close() failed on real hardware: {:?}", result);
}
