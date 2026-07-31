// tests/adr_ramping_tests.rs — retry logic for the ADR ramp's LS625 set-commands
//
// HARDWARE SAFETY: these tests exercise only the pure retry/classification
// logic with synthetic closures. No serial port is opened and no device
// controller method is called, so no bytes can reach any instrument.

use std::cell::Cell;
use std::time::Duration;

use frost::adr_ramping::{is_transient_lock_error, retry_on_busy_cfg};

const NO_DELAY: Duration = Duration::from_millis(0);

// ── Error classification ─────────────────────────────────────────────────────

#[test]
fn transient_lock_errors_are_recognized() {
    assert!(is_transient_lock_error("Failed to open /dev/ttyUSB0: Device or resource busy"));
    assert!(is_transient_lock_error("could not gain exclusive lock on serial port"));
    assert!(is_transient_lock_error("Resource temporarily unavailable (lock held)"));
}

#[test]
fn genuine_faults_are_not_transient() {
    assert!(!is_transient_lock_error("Instrument returned NAK"));
    assert!(!is_transient_lock_error("quench detected"));
    assert!(!is_transient_lock_error("checksum mismatch"));
}

// ── Retry behavior ───────────────────────────────────────────────────────────

#[test]
fn succeeds_immediately_when_op_ok() {
    let calls = Cell::new(0);
    let r = retry_on_busy_cfg(|| { calls.set(calls.get() + 1); Ok(()) }, 5, NO_DELAY);
    assert!(r.is_ok());
    assert_eq!(calls.get(), 1, "should not retry a success");
}

#[test]
fn retries_busy_then_succeeds() {
    let calls = Cell::new(0);
    let r = retry_on_busy_cfg(
        || {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err("Device or resource busy".to_string())
            } else {
                Ok(())
            }
        },
        5,
        NO_DELAY,
    );
    assert!(r.is_ok());
    assert_eq!(calls.get(), 3, "should retry until the busy condition clears");
}

#[test]
fn gives_up_after_max_tries_on_persistent_busy() {
    let calls = Cell::new(0);
    let r = retry_on_busy_cfg(
        || { calls.set(calls.get() + 1); Err("exclusive lock".to_string()) },
        5,
        NO_DELAY,
    );
    assert!(r.is_err());
    assert_eq!(calls.get(), 5, "should attempt exactly max_tries times");
}

#[test]
fn genuine_fault_fails_immediately_without_retry() {
    // A non-transient error (e.g. a real instrument fault) must NOT be retried —
    // retrying a genuine fault would just waste time and mask the problem.
    let calls = Cell::new(0);
    let r = retry_on_busy_cfg(
        || { calls.set(calls.get() + 1); Err("quench detected".to_string()) },
        5,
        NO_DELAY,
    );
    assert!(r.is_err());
    assert_eq!(calls.get(), 1, "a genuine fault must fail on the first attempt");
}
