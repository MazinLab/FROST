// tests/lakeshore350_tests.rs — LS350 overrange-detection tests
//
// Covers the shared overrange sentinel helper (Finding 4) and the canonical
// checked-Kelvin read the safety interlock reuses (Finding 3).
//
// Hardware safety: the only test that calls a controller method uses a
// nonexistent port so the OS rejects the open before any bytes reach an
// instrument. See FROST/CLAUDE.md safety rule.
//
// Run with: cargo test

use frost::lakeshore350::{looks_overrange, LakeShore350Controller};

// ── looks_overrange: shared SRDG?/KRDG? garbage detector ─────────────────────

#[test]
fn normal_numeric_reading_is_not_overrange() {
    assert!(!looks_overrange("1.2345", 'T'));
    assert!(!looks_overrange("+123.45", 'R'));
    assert!(!looks_overrange("-0.0012", 'T'));
}

#[test]
fn overlong_response_is_overrange() {
    // >15 chars indicates a garbage frame regardless of type.
    assert!(looks_overrange("1234567890123456", 'T'));
    assert!(looks_overrange("1234567890123456", 'R'));
}

#[test]
fn control_chars_are_overrange() {
    assert!(looks_overrange("12`45", 'T'));
    assert!(looks_overrange("12\x0045", 'R'));
}

#[test]
fn over_token_is_overrange_for_both_types() {
    assert!(looks_overrange("OVER", 'T'));
    assert!(looks_overrange("over", 'R'));
}

#[test]
fn prefix_specific_tokens() {
    // 'T' path recognizes T-prefixed overrange tokens; 'R' path R-prefixed.
    assert!(looks_overrange("T.", 'T'));
    assert!(looks_overrange("T_OVER", 'T'));
    assert!(looks_overrange("R.", 'R'));
    assert!(looks_overrange("R_OVER", 'R'));
    // The prefixes are distinct: a bare "T." should not trip the 'R' detector
    // (and vice-versa) unless it also contains "OVER".
    assert!(!looks_overrange("T.", 'R'));
    assert!(!looks_overrange("R.", 'T'));
}

// ── read_kelvin_checked: canonical valid-reading decision (fail-safe) ─────────

#[test]
fn checked_read_on_unopenable_port_is_none() {
    // Nonexistent port → open fails → no valid reading → None. This is the
    // exact fail-safe path the safety interlock relies on to BLOCK a start when
    // the 4K stage cannot be read. No bytes reach any instrument.
    let ls350 = LakeShore350Controller::with_port(
        "/dev/frost_no_such_port".to_string(),
        57600,
    );
    assert_eq!(ls350.read_kelvin_checked("D3"), None);
}
