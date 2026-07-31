// tests/compressor_tests.rs — Cryomech controller state contract
//
// Covers the canonical `running` field (Finding 2): the single source of truth
// for "is the compressor on?", parsed once inside get_status() from the same
// comp_on() query. The worker and the safety interlock both read this instead of
// scraping the formatted status text, so they can never disagree.
//
// Hardware safety: get_status() is exercised only against a nonexistent port so
// the OS rejects the open before any bytes reach the compressor. See
// FROST/CLAUDE.md safety rule.
//
// Run with: cargo test

use frost::compressor::CryomechController;

#[test]
fn running_is_none_before_first_poll() {
    // Until a successful status query, "running" is unknown — not a stale false.
    let c = CryomechController::default();
    assert_eq!(c.running, None);
}

#[test]
fn get_status_on_unopenable_port_leaves_running_none() {
    // Connection failure must NOT flip running to a bogus value; it stays None
    // (unknown), and the worker preserves the last known state on None. No bytes
    // reach any instrument (nonexistent port).
    let mut c = CryomechController::default();
    c.port = "/dev/frost_no_such_port".to_string();
    c.get_status();
    assert!(c.error_message.is_some(), "expected a connection error");
    assert_eq!(c.running, None, "running must remain unknown on comms failure");
}
