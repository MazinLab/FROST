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
use frost::record_temps::{set_recording_active, clear_recording_active, is_recording_active, get_recording_active_path};
use frost::worker::{
    get_gl7_cooldown_pid, set_gl7_cooldown_persisted, clear_gl7_cooldown_persisted,
    GL7_COOLDOWN_RUNNING_PATH,
    write_gl7_output_state, read_gl7_output_state, clear_gl7_output_state,
    GL7_OUTPUTS_PATH,
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
    tx.send(GuiCommand::RunAdrRamp).unwrap();
    tx.send(GuiCommand::SetGl7Output { output: 2, pct: 75.0 }).unwrap();

    assert!(matches!(rx.try_recv().unwrap(), GuiCommand::StartCompressor));
    assert!(matches!(rx.try_recv().unwrap(), GuiCommand::RunAdrRamp));
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
    tx2.send(GuiCommand::RunAdrRamp).unwrap();

    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(count, 2);
}

// ── Recording lock file auto-resume on startup ────────────────────────────────
//
// When the app closes while recording is active, the recording thread is killed
// by the OS before it can call clear_recording_active().  On the next launch,
// spawn() detects the stale lock file and sends a StartRecording command so
// recording resumes automatically and the Stop button is shown.

#[test]
fn stale_recording_lock_file_triggers_auto_resume_command() {
    use std::sync::mpsc::channel;
    use frost::worker::GuiCommand;

    let _ = std::fs::create_dir_all("temps");
    let was_active = is_recording_active();
    let original_path = get_recording_active_path();

    // Plant a stale lock file as if the previous process died mid-recording.
    // Store a fake CSV path to simulate an interrupted session with a known file.
    let fake_path = "temps/2026-01-01_temperature_log.csv";
    set_recording_active(fake_path);
    assert!(is_recording_active(), "lock file must be present to simulate interrupted session");

    // Replicate the spawn() startup logic: detect the lock file, read the stored
    // path, and queue a StartRecording command so recording resumes in the same file.
    let was_recording = is_recording_active();
    let resume_path = get_recording_active_path();
    let (tx, rx) = channel::<GuiCommand>();
    if was_recording {
        tx.send(GuiCommand::StartRecording {
            interval_secs: 30,
            output_dir: "temps".to_string(),
            resume_path,
        }).unwrap();
    }

    // The command must have been queued so the worker will resume recording.
    let cmd = rx.try_recv().expect("StartRecording command must be queued on auto-resume");
    let GuiCommand::StartRecording { interval_secs, output_dir, resume_path } = cmd else {
        panic!("expected StartRecording");
    };
    assert_eq!(interval_secs, 30);
    assert_eq!(output_dir, "temps");
    assert_eq!(resume_path.as_deref(), Some(fake_path), "resume_path must carry the stored CSV path");

    // Restore whatever state existed before the test ran.
    if was_active {
        set_recording_active(original_path.as_deref().unwrap_or(""));
    } else {
        clear_recording_active();
    }
}

// ── GL7 cooldown PID liveness check ──────────────────────────────────────────
//
// get_gl7_cooldown_pid() must return None for a dead PID and clean up the stale
// file, so that a crashed GL7 subprocess never permanently blocks GL7 and
// temperature polling.
//
// All three tests share the same lock-file path, so they use a static mutex to
// prevent concurrent writes from interfering with each other.

static GL7_PID_FILE_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn gl7_cooldown_dead_pid_returns_none_and_clears_file() {
    let _guard = GL7_PID_FILE_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    let _ = std::fs::create_dir_all("state");
    // PID u32::MAX is guaranteed never to exist on Linux.
    let dead_pid: u32 = u32::MAX;
    set_gl7_cooldown_persisted(dead_pid);
    assert!(
        std::path::Path::new(GL7_COOLDOWN_RUNNING_PATH).exists(),
        "lock file must exist before the check"
    );

    let result = get_gl7_cooldown_pid();

    assert!(result.is_none(), "dead PID must return None");
    assert!(
        !std::path::Path::new(GL7_COOLDOWN_RUNNING_PATH).exists(),
        "stale lock file must be removed after a dead-PID check"
    );
}

#[test]
fn gl7_cooldown_live_pid_returns_some() {
    let _guard = GL7_PID_FILE_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    let _ = std::fs::create_dir_all("state");
    // The current test process is definitely running.
    let live_pid = std::process::id();
    set_gl7_cooldown_persisted(live_pid);

    let result = get_gl7_cooldown_pid();

    assert_eq!(result, Some(live_pid), "running PID must be returned");

    // Clean up.
    clear_gl7_cooldown_persisted();
}

#[test]
fn gl7_cooldown_missing_file_returns_none() {
    let _guard = GL7_PID_FILE_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    clear_gl7_cooldown_persisted(); // ensure file is absent
    assert!(get_gl7_cooldown_pid().is_none(), "absent file must return None");
}

// ── GL7 output state file ─────────────────────────────────────────────────────

static GL7_OUTPUTS_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn gl7_output_state_round_trip() {
    let _guard = GL7_OUTPUTS_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    let _ = std::fs::create_dir_all("state");
    let inputs: [f64; 4] = [30.0, 18.5, 40.0, 0.0];

    write_gl7_output_state(inputs);
    let result = read_gl7_output_state();

    assert_eq!(result, Some(inputs), "read must return exactly what was written");

    clear_gl7_output_state();
}

#[test]
fn gl7_output_state_missing_file_returns_none() {
    let _guard = GL7_OUTPUTS_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    clear_gl7_output_state();

    assert!(read_gl7_output_state().is_none(), "absent state file must return None");
}

#[test]
fn gl7_output_state_update_single_slot() {
    let _guard = GL7_OUTPUTS_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    let _ = std::fs::create_dir_all("state");
    write_gl7_output_state([10.0, 20.0, 30.0, 40.0]);

    // Simulate update_output_state_file(2, 99.0): read-modify-write for output 2.
    let mut vals = read_gl7_output_state().unwrap();
    vals[1] = 99.0;
    write_gl7_output_state(vals);

    let result = read_gl7_output_state().unwrap();
    assert_eq!(result[0], 10.0);
    assert_eq!(result[1], 99.0);
    assert_eq!(result[2], 30.0);
    assert_eq!(result[3], 40.0);

    clear_gl7_output_state();
}

#[test]
fn gl7_output_state_clear_removes_file() {
    let _guard = GL7_OUTPUTS_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    let _ = std::fs::create_dir_all("state");
    write_gl7_output_state([1.0, 2.0, 3.0, 4.0]);
    assert!(std::path::Path::new(GL7_OUTPUTS_PATH).exists(), "file must exist before clear");

    clear_gl7_output_state();

    assert!(!std::path::Path::new(GL7_OUTPUTS_PATH).exists(), "file must be removed after clear");
}
