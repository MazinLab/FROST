// tests/lakeshore625_tests.rs — Unit tests for lakeshore625.rs and adr_ramping.rs
//
// ── Hardware safety ───────────────────────────────────────────────────────────
//
// NO test in this file may activate physical hardware.  Every test that calls a
// controller method uses TEST_PORT — a path the OS will never successfully open.
// The serialport open() call fails before any bytes are transmitted, so no
// instrument is ever contacted.
//
// The safety argument:
//   1. All serial communication requires a successful port open first.
//   2. A nonexistent device path ("/dev/frost_no_such_port") can never be opened.
//   3. Therefore, no bytes ever reach any hardware.
//
// Tests that require a physical LS625 on /dev/ttyUSB0 are marked #[ignore] and
// must only be run deliberately: cargo test -- --include-ignored
//
// Run all safe tests (no hardware): cargo test

use std::fs;

use frost::lakeshore625::{
    LakeShore625Controller,
    CURRENT_LIMIT_MAX, VOLTAGE_LIMIT_MIN, VOLTAGE_LIMIT_MAX, RATE_LIMIT_MIN, RATE_LIMIT_MAX,
    parse_error_status, parse_error_compact,
    fmt_ramp_f64_opt, ramp_format_row, next_ramp_csv,
    RAMP_WIDTHS,
};
use frost::adr_ramping::{SOAK_TOLERANCE, ZERO_TOLERANCE};

// ── Safety constant + helper ──────────────────────────────────────────────────

const TEST_PORT: &str = "/dev/frost_no_such_port";

fn test_controller() -> LakeShore625Controller {
    LakeShore625Controller {
        port:          TEST_PORT.to_string(),
        baud_rate:     9600,
        error_message: None,
        output:        String::new(),
    }
}

// ── set_compliance_voltage validation ────────────────────────────────────────
//
// Validation fires before any serial attempt, so these tests never open a port.
// When validation passes, we get a serial error ("Failed to open ...") — that
// confirms the check did not block the call.

#[test]
fn compliance_voltage_below_min_rejected() {
    let mut ctrl = test_controller();
    let result = ctrl.set_compliance_voltage(0.099);
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(msg.contains("0.099"), "Error should mention the bad value. Got: {msg}");
    assert!(!msg.starts_with("Failed to open"), "Should fail at validation, not serial. Got: {msg}");
}

#[test]
fn compliance_voltage_at_min_passes_validation() {
    // 0.1 V is exactly VOLTAGE_LIMIT_MIN — must be accepted.
    let mut ctrl = test_controller();
    let err = ctrl.set_compliance_voltage(VOLTAGE_LIMIT_MIN).unwrap_err();
    assert!(
        err.starts_with("Failed to open"),
        "Expected serial error (validation passed). Got: {err}"
    );
}

#[test]
fn compliance_voltage_at_max_passes_validation() {
    // 5.0 V is exactly VOLTAGE_LIMIT_MAX — must be accepted.
    let mut ctrl = test_controller();
    let err = ctrl.set_compliance_voltage(VOLTAGE_LIMIT_MAX).unwrap_err();
    assert!(err.starts_with("Failed to open"), "Got: {err}");
}

#[test]
fn compliance_voltage_above_max_rejected() {
    let mut ctrl = test_controller();
    let result = ctrl.set_compliance_voltage(5.001);
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(msg.contains("5.001"), "Error should mention the bad value. Got: {msg}");
    assert!(!msg.starts_with("Failed to open"), "Should fail at validation, not serial. Got: {msg}");
}

// ── set_limits validation ─────────────────────────────────────────────────────

#[test]
fn limits_all_valid_midpoints_pass_validation() {
    // All three params at safe midpoints — validation passes, serial error expected.
    let mut ctrl = test_controller();
    let err = ctrl.set_limits(30.0, 2.5, 0.5).unwrap_err();
    assert!(err.starts_with("Failed to open"), "Got: {err}");
}

#[test]
fn limits_current_negative_rejected() {
    let mut ctrl = test_controller();
    let result = ctrl.set_limits(-1.0, 2.5, 0.5);
    assert!(result.is_err());
    assert!(!result.unwrap_err().starts_with("Failed to open"));
}

#[test]
fn limits_current_at_max_passes_validation() {
    // 60.1 A is exactly CURRENT_LIMIT_MAX — must be allowed.
    let mut ctrl = test_controller();
    let err = ctrl.set_limits(CURRENT_LIMIT_MAX, 2.5, 0.5).unwrap_err();
    assert!(err.starts_with("Failed to open"), "Got: {err}");
}

#[test]
fn limits_current_above_max_rejected() {
    let mut ctrl = test_controller();
    let result = ctrl.set_limits(60.11, 2.5, 0.5);
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(msg.contains("60.11"), "Got: {msg}");
    assert!(!msg.starts_with("Failed to open"));
}

#[test]
fn limits_voltage_below_min_rejected() {
    let mut ctrl = test_controller();
    let result = ctrl.set_limits(30.0, 0.099, 0.5);
    assert!(result.is_err());
    assert!(!result.unwrap_err().starts_with("Failed to open"));
}

#[test]
fn limits_voltage_at_min_passes_validation() {
    let mut ctrl = test_controller();
    let err = ctrl.set_limits(30.0, VOLTAGE_LIMIT_MIN, 0.5).unwrap_err();
    assert!(err.starts_with("Failed to open"), "Got: {err}");
}

#[test]
fn limits_voltage_at_max_passes_validation() {
    let mut ctrl = test_controller();
    let err = ctrl.set_limits(30.0, VOLTAGE_LIMIT_MAX, 0.5).unwrap_err();
    assert!(err.starts_with("Failed to open"), "Got: {err}");
}

#[test]
fn limits_voltage_above_max_rejected() {
    let mut ctrl = test_controller();
    let result = ctrl.set_limits(30.0, 5.001, 0.5);
    assert!(result.is_err());
    assert!(!result.unwrap_err().starts_with("Failed to open"));
}

#[test]
fn limits_rate_below_min_rejected() {
    let mut ctrl = test_controller();
    let result = ctrl.set_limits(30.0, 2.5, 0.00009);
    assert!(result.is_err());
    assert!(!result.unwrap_err().starts_with("Failed to open"));
}

#[test]
fn limits_rate_at_min_passes_validation() {
    let mut ctrl = test_controller();
    let err = ctrl.set_limits(30.0, 2.5, RATE_LIMIT_MIN).unwrap_err();
    assert!(err.starts_with("Failed to open"), "Got: {err}");
}

#[test]
fn limits_rate_at_max_passes_validation() {
    let mut ctrl = test_controller();
    let err = ctrl.set_limits(30.0, 2.5, RATE_LIMIT_MAX).unwrap_err();
    assert!(err.starts_with("Failed to open"), "Got: {err}");
}

#[test]
fn limits_rate_above_max_rejected() {
    let mut ctrl = test_controller();
    let result = ctrl.set_limits(30.0, 2.5, 100.0);
    assert!(result.is_err());
    assert!(!result.unwrap_err().starts_with("Failed to open"));
}

// ── Unvalidated commands (document the gap) ───────────────────────────────────
//
// set_current and set_ramp_rate have NO range validation — out-of-range values
// are forwarded to the wire without being checked.  These tests pin that behavior:
// the only error is a serial error, not a bounds check.  If validation is added
// later, these tests should be updated to assert Err at the validation layer.

#[test]
fn set_current_negative_not_validated_reaches_serial() {
    let mut ctrl = test_controller();
    let err = ctrl.set_current(-1.0).unwrap_err();
    assert!(
        err.starts_with("Failed to open"),
        "set_current has no validation — expected serial error. Got: {err}"
    );
}

#[test]
fn set_current_above_hardware_max_not_validated_reaches_serial() {
    let mut ctrl = test_controller();
    let err = ctrl.set_current(CURRENT_LIMIT_MAX + 1.0).unwrap_err();
    assert!(
        err.starts_with("Failed to open"),
        "set_current has no validation — expected serial error. Got: {err}"
    );
}

#[test]
fn set_ramp_rate_zero_not_validated_reaches_serial() {
    let mut ctrl = test_controller();
    let err = ctrl.set_ramp_rate(0.0).unwrap_err();
    assert!(
        err.starts_with("Failed to open"),
        "set_ramp_rate has no validation — expected serial error. Got: {err}"
    );
}

#[test]
fn set_ramp_rate_above_max_not_validated_reaches_serial() {
    let mut ctrl = test_controller();
    let err = ctrl.set_ramp_rate(RATE_LIMIT_MAX + 1.0).unwrap_err();
    assert!(
        err.starts_with("Failed to open"),
        "set_ramp_rate has no validation — expected serial error. Got: {err}"
    );
}

// ── parse_error_status ────────────────────────────────────────────────────────

#[test]
fn error_status_all_zeros_shows_none_for_all_sections() {
    let out = parse_error_status("0,0,0");
    assert!(out.contains("Hardware Errors:    None"),    "Got:\n{out}");
    assert!(out.contains("Operational Errors: None"),    "Got:\n{out}");
    assert!(out.contains("PSH Errors:         None"),    "Got:\n{out}");
}

#[test]
fn error_status_hw_temperature_fault() {
    // hw bit 1
    let out = parse_error_status("1,0,0");
    assert!(out.contains("Temperature Fault"),           "Got:\n{out}");
    assert!(out.contains("Operational Errors: None"),    "Got:\n{out}");
}

#[test]
fn error_status_hw_low_line_voltage() {
    // hw bit 2
    let out = parse_error_status("2,0,0");
    assert!(out.contains("Low Line Voltage"),            "Got:\n{out}");
}

#[test]
fn error_status_hw_output_over_current() {
    // hw bit 4
    let out = parse_error_status("4,0,0");
    assert!(out.contains("Output Over Current"),         "Got:\n{out}");
}

#[test]
fn error_status_hw_output_over_voltage() {
    // hw bit 8
    let out = parse_error_status("8,0,0");
    assert!(out.contains("Output Over Voltage"),         "Got:\n{out}");
}

#[test]
fn error_status_hw_output_control_failure() {
    // hw bit 16
    let out = parse_error_status("16,0,0");
    assert!(out.contains("Output Control Failure"),      "Got:\n{out}");
}

#[test]
fn error_status_hw_dac_processor_not_responding() {
    // hw bit 32
    let out = parse_error_status("32,0,0");
    assert!(out.contains("DAC Processor Not Responding"), "Got:\n{out}");
}

#[test]
fn error_status_op_quench_detected() {
    // op bit 32
    let out = parse_error_status("0,32,0");
    assert!(out.contains("Magnet Quench Detected"),      "Got:\n{out}");
    assert!(out.contains("Hardware Errors:    None"),    "Got:\n{out}");
}

#[test]
fn error_status_op_magnet_crowbar() {
    // op bit 64
    let out = parse_error_status("0,64,0");
    assert!(out.contains("Magnet Discharging Through Crowbar"), "Got:\n{out}");
}

#[test]
fn error_status_op_remote_inhibit() {
    // op bit 16
    let out = parse_error_status("0,16,0");
    assert!(out.contains("Remote Inhibit Detected"),     "Got:\n{out}");
}

#[test]
fn error_status_psh_open_circuit() {
    // psh bit 1
    let out = parse_error_status("0,0,1");
    assert!(out.contains("PSH Open Circuit"),            "Got:\n{out}");
}

#[test]
fn error_status_psh_short_circuit() {
    // psh bit 2
    let out = parse_error_status("0,0,2");
    assert!(out.contains("PSH Short Circuit"),           "Got:\n{out}");
}

#[test]
fn error_status_multiple_bits_all_appear() {
    // hw=3 (bits 1+2), op=32 (quench), psh=3 (bits 1+2)
    let out = parse_error_status("3,32,3");
    assert!(out.contains("Temperature Fault"),           "Got:\n{out}");
    assert!(out.contains("Low Line Voltage"),            "Got:\n{out}");
    assert!(out.contains("Magnet Quench Detected"),      "Got:\n{out}");
    assert!(out.contains("PSH Open Circuit"),            "Got:\n{out}");
    assert!(out.contains("PSH Short Circuit"),           "Got:\n{out}");
}

#[test]
fn error_status_malformed_no_commas_falls_back_to_raw() {
    let out = parse_error_status("garbage_no_commas");
    assert!(out.contains("garbage_no_commas"),           "Got:\n{out}");
}

#[test]
fn error_status_two_parts_only_falls_back_to_raw() {
    let out = parse_error_status("1,2");
    assert!(out.contains("1,2"),                         "Got:\n{out}");
}

// ── parse_error_compact ───────────────────────────────────────────────────────

#[test]
fn error_compact_all_zeros_returns_none_string() {
    assert_eq!(parse_error_compact("0,0,0"), "None");
}

#[test]
fn error_compact_quench_detected() {
    assert_eq!(parse_error_compact("0,32,0"), "Magnet Quench");
}

#[test]
fn error_compact_crowbar_and_quench_joined_with_semicolon() {
    // op bits 64 + 32 = 96
    let out = parse_error_compact("0,96,0");
    assert!(out.contains("Magnet Crowbar"),    "Got: {out}");
    assert!(out.contains("Magnet Quench"),     "Got: {out}");
    assert!(out.contains(';'),                 "Expected semicolon separator. Got: {out}");
}

#[test]
fn error_compact_all_sections_contribute() {
    // hw=1 (Temperature Fault), op=32 (Quench), psh=1 (PSH Open)
    let out = parse_error_compact("1,32,1");
    assert!(out.contains("Temperature Fault"), "Got: {out}");
    assert!(out.contains("Magnet Quench"),     "Got: {out}");
    assert!(out.contains("PSH Open"),          "Got: {out}");
}

#[test]
fn error_compact_malformed_returns_parse_error() {
    assert_eq!(parse_error_compact("notvalid"), "Parse Error");
}

#[test]
fn error_compact_two_parts_returns_parse_error() {
    assert_eq!(parse_error_compact("1,2"), "Parse Error");
}

// ── fmt_ramp_f64_opt ──────────────────────────────────────────────────────────

#[test]
fn fmt_ramp_some_four_decimals() {
    assert_eq!(fmt_ramp_f64_opt(Some(1.2345), 4), "1.2345");
}

#[test]
fn fmt_ramp_some_zero_four_decimals() {
    assert_eq!(fmt_ramp_f64_opt(Some(0.0), 4), "0.0000");
}

#[test]
fn fmt_ramp_some_one_decimal() {
    assert_eq!(fmt_ramp_f64_opt(Some(42.5), 1), "42.5");
}

#[test]
fn fmt_ramp_none_returns_no_response() {
    assert_eq!(fmt_ramp_f64_opt(None, 4), "NO_RESPONSE");
}

// ── ramp_format_row ───────────────────────────────────────────────────────────

#[test]
fn ramp_format_row_last_column_not_padded() {
    // Build a full 9-column row where the last value is shorter than its slot.
    // Every column except the last should be padded to its RAMP_WIDTHS entry;
    // the last must appear verbatim at the end of the string.
    let mut values: Vec<String> = RAMP_WIDTHS.iter()
        .map(|w| "X".repeat(w - 1))
        .collect();
    *values.last_mut().unwrap() = "LAST".to_string();

    let row = ramp_format_row(&values);
    assert!(
        row.ends_with("LAST"),
        "Last column must not be padded. Row ends with: {:?}",
        &row[row.len().saturating_sub(12)..]
    );
}

#[test]
fn ramp_format_row_non_last_columns_are_padded() {
    // A two-element row: the first should be padded to RAMP_WIDTHS[0] (28 chars),
    // the second (last) must not be padded.
    let values = vec!["A".to_string(), "B".to_string()];
    let row = ramp_format_row(&values);
    assert_eq!(&row[..RAMP_WIDTHS[0]], format!("{:<28}", "A"));
    assert!(row.ends_with('B'), "Last column should not be padded. Got: {row:?}");
}

#[test]
fn ramp_format_row_single_element_not_padded() {
    // With one element it is both first and last — no padding applied.
    let row = ramp_format_row(&["only".to_string()]);
    assert_eq!(row, "only");
}

#[test]
fn ramp_format_row_empty_does_not_panic() {
    let row = ramp_format_row(&[]);
    assert_eq!(row, "");
}

// ── next_ramp_csv path generation ────────────────────────────────────────────
//
// Uses uniquely-named temp directories to avoid interfering with any real ramp/
// logs and to make tests independent of each other.

#[test]
fn next_ramp_csv_no_file_returns_base_path() {
    let dir = std::env::temp_dir().join("frost_test_ramp_nofile");
    fs::create_dir_all(&dir).unwrap();
    let dir_s = dir.to_str().unwrap();

    let path = next_ramp_csv(dir_s, "2099-01-01");
    assert_eq!(path, format!("{}/2099-01-01_ramp_log.csv", dir_s));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn next_ramp_csv_base_exists_returns_suffix_1() {
    let dir = std::env::temp_dir().join("frost_test_ramp_base");
    fs::create_dir_all(&dir).unwrap();
    let dir_s = dir.to_str().unwrap();

    fs::write(format!("{}/2099-01-02_ramp_log.csv", dir_s), "").unwrap();

    let path = next_ramp_csv(dir_s, "2099-01-02");
    assert_eq!(path, format!("{}/2099-01-02_ramp_log_1.csv", dir_s));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn next_ramp_csv_base_and_1_exist_returns_suffix_2() {
    let dir = std::env::temp_dir().join("frost_test_ramp_1exists");
    fs::create_dir_all(&dir).unwrap();
    let dir_s = dir.to_str().unwrap();

    fs::write(format!("{}/2099-01-03_ramp_log.csv",   dir_s), "").unwrap();
    fs::write(format!("{}/2099-01-03_ramp_log_1.csv", dir_s), "").unwrap();

    let path = next_ramp_csv(dir_s, "2099-01-03");
    assert_eq!(path, format!("{}/2099-01-03_ramp_log_2.csv", dir_s));

    fs::remove_dir_all(&dir).ok();
}

// ── ADR ramp tolerance constants ──────────────────────────────────────────────
//
// These tolerances are protocol-significant: changing them silently would alter
// when the instrument transitions between ramp phases.

#[test]
fn soak_tolerance_is_0_04_amps() {
    assert_eq!(SOAK_TOLERANCE, 0.04,
        "Soak entry tolerance must be exactly 0.04 A (within-target threshold)");
}

#[test]
fn zero_tolerance_is_0_004_amps() {
    assert_eq!(ZERO_TOLERANCE, 0.004,
        "Zero approach tolerance must be exactly 0.004 A (ramp-down completion threshold)");
}

// ── Hardware-dependent tests (require physical LS625 on /dev/ttyUSB0) ─────────

#[test]
#[ignore = "requires LakeShore 625 on /dev/ttyUSB0"]
fn hardware_ls625_identify() {
    let mut ctrl = LakeShore625Controller::default();
    ctrl.get_identification();
    assert!(ctrl.error_message.is_none(), "Error: {:?}", ctrl.error_message);
    assert!(!ctrl.output.is_empty(), "Expected identification string in output");
}

#[test]
#[ignore = "requires LakeShore 625 on /dev/ttyUSB0"]
fn hardware_ls625_get_current_returns_parseable_float() {
    let mut ctrl = LakeShore625Controller::default();
    let r = ctrl.get_current().expect("get_current failed");
    r.trim_start_matches('+')
        .parse::<f64>()
        .expect("Current reading should be a parseable f64");
}

#[test]
#[ignore = "requires LakeShore 625 on /dev/ttyUSB0"]
fn hardware_ls625_get_field_returns_parseable_float() {
    let mut ctrl = LakeShore625Controller::default();
    let r = ctrl.get_field().expect("get_field failed");
    r.trim_start_matches('+')
        .parse::<f64>()
        .expect("Field reading should be a parseable f64");
}

#[test]
#[ignore = "requires LakeShore 625 on /dev/ttyUSB0"]
fn hardware_ls625_get_voltage_returns_parseable_float() {
    let mut ctrl = LakeShore625Controller::default();
    let r = ctrl.get_voltage().expect("get_voltage failed");
    r.trim_start_matches('+')
        .parse::<f64>()
        .expect("Voltage reading should be a parseable f64");
}
