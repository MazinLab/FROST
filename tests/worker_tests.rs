// tests/worker_tests.rs — Architecture tests for the worker thread design
//
// Confirms the viability of the worker.rs approach: pure parsing logic,
// DeviceSnapshot invariants, and GuiCommand channel mechanics — all without
// touching real serial ports or spawning a GUI thread.
//
// Run with: cargo test

use std::sync::mpsc::channel;

use frost::worker::{
    DeviceSnapshot, GuiCommand,
    extract_temperature_value, format_kelvin_value,
    parse_single_value, parse_limits_from_output,
};

// ── extract_temperature_value ─────────────────────────────────────────────────

#[test]
fn extract_temp_arrow_format() {
    // read_input_intelligent primary output format: "→ X K"
    let input = "KRDG? A → 4.2134 K";
    assert_eq!(extract_temperature_value(input), "4.2134 K");
}

#[test]
fn extract_temp_colon_format() {
    // Fallback: "Input A: 4.2134 K"
    let input = "Input A: 4.2134 K";
    assert_eq!(extract_temperature_value(input), "4.2134 K");
}

#[test]
fn extract_temp_error_passthrough() {
    let input = "ERROR: timeout on port /dev/ttyUSB0";
    assert_eq!(extract_temperature_value(input), input);
}

#[test]
fn extract_temp_bare_string_trimmed() {
    // No recognized format — return trimmed input
    assert_eq!(extract_temperature_value("  raw garbage  "), "raw garbage");
}

// ── format_kelvin_value ───────────────────────────────────────────────────────

#[test]
fn format_kelvin_positive() {
    assert_eq!(format_kelvin_value("+1.2345"), "1.2345 K");
}

#[test]
fn format_kelvin_zero_shows_overload() {
    // LS370 returns 0.0 on overload
    assert_eq!(format_kelvin_value("+0.0000"), "+0.0000 (overload)");
}

#[test]
fn format_kelvin_negative_shows_overload() {
    assert_eq!(format_kelvin_value("-1.0000"), "-1.0000 (overload)");
}

#[test]
fn format_kelvin_unparseable_passthrough() {
    assert_eq!(format_kelvin_value("???"), "???");
}

// ── parse_single_value ────────────────────────────────────────────────────────

#[test]
fn parse_single_value_set_current() {
    // "Set current: 9.44 A" — third token is the numeric value
    assert_eq!(parse_single_value("Set current: 9.44 A"), Some(9.44));
}

#[test]
fn parse_single_value_ramp_rate() {
    assert_eq!(parse_single_value("Ramp rate: 0.01 A/s"), Some(0.01));
}

#[test]
fn parse_single_value_compliance_voltage() {
    assert_eq!(parse_single_value("Compliance voltage: 1.0 V"), Some(1.0));
}

#[test]
fn parse_single_value_too_short_returns_none() {
    assert_eq!(parse_single_value("only two"), None);
    assert_eq!(parse_single_value(""), None);
}

#[test]
fn parse_single_value_non_numeric_returns_none() {
    assert_eq!(parse_single_value("Set current: N/A"), None);
}

// ── parse_limits_from_output ──────────────────────────────────────────────────

#[test]
fn parse_limits_all_present() {
    let output = "Current limit: 9.44 A\nVoltage limit: 5.00 V\nRate limit: 0.10 A/s";
    assert_eq!(parse_limits_from_output(output), Some((9.44, 5.00, 0.10)));
}

#[test]
fn parse_limits_missing_one_returns_none() {
    // Rate line absent — can't safely set magnet limits with incomplete data
    let output = "Current limit: 9.44 A\nVoltage limit: 5.00 V";
    assert_eq!(parse_limits_from_output(output), None);
}

#[test]
fn parse_limits_empty_returns_none() {
    assert_eq!(parse_limits_from_output(""), None);
}

#[test]
fn parse_limits_error_prefix_returns_none() {
    assert_eq!(parse_limits_from_output("Error: timeout"), None);
}

// ── DeviceSnapshot invariants ─────────────────────────────────────────────────

#[test]
fn snapshot_default_vec_lengths() {
    // GL7 has 4 outputs — all parallel vecs must match
    let s = DeviceSnapshot::default();
    assert_eq!(s.gl7_output_lines.len(), 4);
    assert_eq!(s.gl7_polled_pct.len(), 4);
    assert_eq!(s.gl7_set_results.len(), 4);
}

#[test]
fn snapshot_default_all_none() {
    let s = DeviceSnapshot::default();
    assert!(s.last_compressor_update.is_none());
    assert!(s.last_magnet_update.is_none());
    assert!(s.last_gl7_update.is_none());
    assert!(s.last_temp_update.is_none());
    assert!(s.compressor_cmd_result.is_none());
    assert!(s.magnet_cmd_result.is_none());
    assert!(s.magnet_polled_current_limit.is_none());
    assert!(s.magnet_polled_voltage_limit.is_none());
    assert!(s.magnet_polled_rate_limit.is_none());
    assert!(s.magnet_polled_ramp_rate.is_none());
    assert!(s.magnet_polled_compliance_voltage.is_none());
    assert!(s.magnet_polled_target_current.is_none());
    assert!(s.gl7_polled_pct.iter().all(|v| v.is_none()));
    assert!(s.gl7_set_results.iter().all(|v| v.is_none()));
}

#[test]
fn snapshot_clone_is_independent() {
    // Cheap clone must not alias the original — GUI reads a full copy each frame
    let mut s = DeviceSnapshot::default();
    s.compressor_status = "Running".to_string();
    let cloned = s.clone();
    s.compressor_status = "Stopped".to_string();
    assert_eq!(cloned.compressor_status, "Running");
}

// ── Result draining (.take()) pattern ────────────────────────────────────────
//
// The GUI calls .take() on each result field every frame.
// After take(), the field is None — prevents stale errors from reappearing.

#[test]
fn result_draining_consumed_after_take() {
    let mut s = DeviceSnapshot::default();
    s.compressor_cmd_result = Some(Ok(()));

    let first = s.compressor_cmd_result.take();
    assert!(first.is_some());

    // Second take is idempotent — nothing left to drain
    let second = s.compressor_cmd_result.take();
    assert!(second.is_none());
}

#[test]
fn result_draining_error_preserved_until_taken() {
    let mut s = DeviceSnapshot::default();
    s.magnet_cmd_result = Some(Err("Quench detected".to_string()));

    let r = s.magnet_cmd_result.take().unwrap();
    assert_eq!(r.unwrap_err(), "Quench detected");
    assert!(s.magnet_cmd_result.is_none());
}

// ── GuiCommand channel mechanics ─────────────────────────────────────────────

#[test]
fn channel_commands_received_in_order() {
    let (tx, rx) = channel::<GuiCommand>();

    tx.send(GuiCommand::StartCompressor).unwrap();
    tx.send(GuiCommand::RunAdrRamp { rate: 0.004, current: 5.0, soak_mins: 45 }).unwrap();
    tx.send(GuiCommand::SetGl7Output { output: 2, pct: 75.0 }).unwrap();

    assert!(matches!(rx.try_recv().unwrap(), GuiCommand::StartCompressor));
    assert!(matches!(rx.try_recv().unwrap(), GuiCommand::RunAdrRamp { current, .. } if current == 5.0));
    assert!(matches!(rx.try_recv().unwrap(), GuiCommand::SetGl7Output { output: 2, pct } if pct == 75.0));

    // Channel exhausted — non-blocking check returns empty
    assert!(rx.try_recv().is_err());
}

#[test]
fn channel_dropped_sender_returns_error() {
    // When the GUI drops, the worker's try_recv returns Err — clean shutdown signal
    let (tx, rx) = channel::<GuiCommand>();
    drop(tx);
    assert!(rx.try_recv().is_err());
}

#[test]
fn channel_cloned_sender_all_received() {
    // SerialWorker::send() takes &self, so multiple call sites share one Sender
    let (tx, rx) = channel::<GuiCommand>();
    let tx2 = tx.clone();

    tx.send(GuiCommand::StopCompressor).unwrap();
    tx2.send(GuiCommand::RunAdrRamp { rate: 0.05, current: 9.44, soak_mins: 45 }).unwrap();

    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(count, 2);
}
