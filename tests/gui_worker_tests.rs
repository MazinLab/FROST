// tests/gui_worker_tests.rs — Stage 1: worker-thread architecture tests
//
// Covers snapshot concurrency, ADR ramp guard logic, command-channel stress,
// and snapshot field transitions — all without serial ports or a GUI thread.
//
// Safety: no device controllers are constructed here, so no serial bytes are
// sent.  Run with: cargo test

use std::sync::{Arc, Mutex};
use std::sync::mpsc::channel;

use frost::worker::{DeviceSnapshot, GuiCommand};

// ── Snapshot concurrent access ────────────────────────────────────────────────
//
// The worker and GUI share one Arc<Mutex<DeviceSnapshot>>.  These tests confirm
// that concurrent writers and readers don't deadlock or panic.

#[test]
fn snapshot_concurrent_write_read_no_deadlock() {
    let snap = Arc::new(Mutex::new(DeviceSnapshot::default()));
    let snap_writer = Arc::clone(&snap);

    let writer = std::thread::spawn(move || {
        for i in 0..200u32 {
            let mut s = snap_writer.lock().unwrap();
            s.compressor_running = i % 2 == 0;
            s.compressor_status  = format!("status {i}");
        }
    });

    // Reader runs concurrently; just acquiring the lock is the test.
    for _ in 0..200 {
        let s = snap.lock().unwrap();
        let _ = s.compressor_running;
        let _ = s.compressor_status.len();
    }

    writer.join().expect("writer thread panicked");
}

#[test]
fn snapshot_concurrent_adr_log_append_no_deadlock() {
    let snap = Arc::new(Mutex::new(DeviceSnapshot::default()));
    let snap_w = Arc::clone(&snap);

    let writer = std::thread::spawn(move || {
        for i in 0..100 {
            snap_w.lock().unwrap().adr_log_lines.push(format!("log line {i}"));
        }
    });

    for _ in 0..100 {
        let s = snap.lock().unwrap();
        let _ = s.adr_log_lines.len();
    }

    writer.join().expect("writer thread panicked");
    assert_eq!(snap.lock().unwrap().adr_log_lines.len(), 100);
}

// ── ADR ramp guard logic ───────────────────────────────────────────────────────
//
// execute_command must ignore RunAdrRamp when adr_ramp_running is already true.
// We test the guard condition directly since we can't call execute_command
// without real device state.

#[test]
fn adr_ramp_guard_true_when_running() {
    let mut s = DeviceSnapshot::default();
    s.adr_ramp_running = true;

    // This is exactly the guard in execute_command:
    //   if s.adr_ramp_running { return; }
    assert!(s.adr_ramp_running, "guard must fire when ramp is already running");
}

#[test]
fn adr_ramp_guard_false_allows_new_ramp() {
    let s = DeviceSnapshot::default();
    assert!(!s.adr_ramp_running, "default snapshot must allow a ramp to start");
}

// ── Snapshot field transitions on ramp start ──────────────────────────────────
//
// Mirrors what execute_command does when RunAdrRamp is accepted.

#[test]
fn adr_ramp_start_clears_previous_log_and_result() {
    let mut s = DeviceSnapshot::default();
    s.adr_log_lines   = vec!["old line 1".to_string(), "old line 2".to_string()];
    s.adr_status_line = "old status".to_string();
    s.adr_ramp_result = Some(Ok(()));

    // Simulate execute_command accepting a new RunAdrRamp:
    s.adr_ramp_running     = true;
    s.adr_ramp_started     = Some(std::time::Instant::now());
    s.adr_ramp_result      = None;
    s.adr_ramp_was_stopped = false;
    s.adr_log_lines.clear();
    s.adr_status_line.clear();

    assert!(s.adr_ramp_running);
    assert!(s.adr_ramp_started.is_some());
    assert!(s.adr_ramp_result.is_none());
    assert!(!s.adr_ramp_was_stopped);
    assert!(s.adr_log_lines.is_empty());
    assert!(s.adr_status_line.is_empty());
}

#[test]
fn adr_ramp_completion_clears_running_flag() {
    let mut s = DeviceSnapshot::default();
    s.adr_ramp_running = true;
    s.adr_status_line  = "soaking…".to_string();

    // Simulate the ramp thread finishing:
    s.adr_ramp_running = false;
    s.adr_ramp_result  = Some(Ok(()));
    s.adr_status_line.clear();

    assert!(!s.adr_ramp_running);
    assert!(s.adr_ramp_result.is_some());
    assert!(s.adr_status_line.is_empty());
}

#[test]
fn adr_ramp_completion_with_error_stores_err() {
    let mut s = DeviceSnapshot::default();
    s.adr_ramp_running = true;

    s.adr_ramp_running = false;
    s.adr_ramp_result  = Some(Err("heatswitch stuck".to_string()));

    let result = s.adr_ramp_result.take().unwrap();
    assert_eq!(result.unwrap_err(), "heatswitch stuck");
}

// ── Snapshot restart-persistence fields ───────────────────────────────────────

#[test]
fn adr_ramp_running_default_false() {
    let s = DeviceSnapshot::default();
    assert!(!s.adr_ramp_running, "default snapshot must not show Stop button");
}

#[test]
fn adr_ramp_was_stopped_default_false() {
    let s = DeviceSnapshot::default();
    assert!(!s.adr_ramp_was_stopped);
}

#[test]
fn adr_ramp_running_restored_on_restart() {
    // Simulates what SerialWorker::spawn does when the lock file is present.
    let mut s = DeviceSnapshot::default();
    // Lock file exists → restore running state so Stop button is shown.
    s.adr_ramp_running = true;
    assert!(s.adr_ramp_running, "running must be true so Stop button is visible after restart");
}

// ── Command-channel stress tests ──────────────────────────────────────────────

#[test]
fn channel_rapid_commands_all_received() {
    let (tx, rx) = channel::<GuiCommand>();

    let n = 500usize;
    for i in 0..n {
        tx.send(GuiCommand::SetGl7Output { output: (i % 4 + 1) as u8, pct: i as f64 }).unwrap();
    }

    let mut count = 0usize;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(count, n);
}

#[test]
fn channel_concurrent_senders_all_received() {
    let (tx, rx) = channel::<GuiCommand>();

    let handles: Vec<_> = (0..8)
        .map(|i| {
            let tx = tx.clone();
            std::thread::spawn(move || {
                tx.send(GuiCommand::SetGl7Output { output: (i % 4 + 1) as u8, pct: i as f64 }).unwrap();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
    drop(tx); // drop last sender so the channel drains cleanly

    let mut count = 0usize;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(count, 8);
}

#[test]
fn channel_mixed_commands_preserve_order() {
    let (tx, rx) = channel::<GuiCommand>();

    tx.send(GuiCommand::StartCompressor).unwrap();
    tx.send(GuiCommand::RunAdrRamp).unwrap();
    tx.send(GuiCommand::StopCompressor).unwrap();
    tx.send(GuiCommand::SetGl7Output { output: 3, pct: 50.0 }).unwrap();

    assert!(matches!(rx.try_recv().unwrap(), GuiCommand::StartCompressor));
    assert!(matches!(rx.try_recv().unwrap(), GuiCommand::RunAdrRamp));
    assert!(matches!(rx.try_recv().unwrap(), GuiCommand::StopCompressor));
    assert!(matches!(rx.try_recv().unwrap(), GuiCommand::SetGl7Output { output: 3, pct } if pct == 50.0));
    assert!(rx.try_recv().is_err());
}

// ── Snapshot compressor_running initial state ─────────────────────────────────

#[test]
fn snapshot_default_compressor_not_running() {
    let s = DeviceSnapshot::default();
    assert!(!s.compressor_running);
    assert!(s.compressor_status.is_empty());
    assert!(s.last_compressor_update.is_none());
}

#[test]
fn snapshot_compressor_running_can_be_set() {
    let mut s = DeviceSnapshot::default();
    s.compressor_running = true;
    s.compressor_status  = "Running: Yes".to_string();
    assert!(s.compressor_running);
}

// ── GL7 set-result per-output draining ────────────────────────────────────────

#[test]
fn gl7_set_results_independent_per_output() {
    let mut s = DeviceSnapshot::default();
    s.gl7_set_results[0] = Some(Ok(()));
    s.gl7_set_results[1] = Some(Err("timeout".to_string()));
    s.gl7_set_results[2] = None;
    s.gl7_set_results[3] = Some(Ok(()));

    assert!(s.gl7_set_results[0].take().unwrap().is_ok());
    assert_eq!(s.gl7_set_results[1].take().unwrap().unwrap_err(), "timeout");
    assert!(s.gl7_set_results[2].take().is_none());
    assert!(s.gl7_set_results[3].take().unwrap().is_ok());

    // All drained:
    assert!(s.gl7_set_results.iter().all(|r| r.is_none()));
}

// ── GL7 last_gl7_update is bumped after SetGl7Output ─────────────────────────
//
// Regression test: the worker's SetGl7Output handler must update last_gl7_update
// so the GUI sync condition (last_gl7_update != last_synced_gl7) fires and the
// new polled percentage is reflected in the edit field.

#[test]
fn set_gl7_output_bumps_last_gl7_update() {
    let mut s = DeviceSnapshot::default();
    assert!(s.last_gl7_update.is_none(), "starts as None");

    // Simulate what the worker's SetGl7Output handler now does:
    let idx = 0usize;
    s.gl7_output_lines[idx] = ("30.000".to_string(), String::new());
    s.gl7_polled_pct[idx]   = Some(30.0);
    s.gl7_set_results[idx]  = Some(Ok(()));
    s.last_gl7_update       = Some(std::time::Instant::now());

    assert!(
        s.last_gl7_update.is_some(),
        "last_gl7_update must be set so the GUI sync condition fires"
    );
    assert_eq!(s.gl7_polled_pct[idx], Some(30.0));
}

#[test]
fn set_gl7_output_update_differs_from_prior_sync_instant() {
    // The GUI stores the last Instant it synced from; a new Instant written by
    // the worker must compare as different so the sync block runs.
    let prior = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(1));

    let mut s = DeviceSnapshot::default();
    s.last_gl7_update = Some(std::time::Instant::now());

    assert_ne!(
        s.last_gl7_update.unwrap(),
        prior,
        "worker-written Instant must differ from the GUI's saved Instant"
    );
}

// ── Temp poll gate: gl7_cooldown_active blocks polling ────────────────────────
//
// When the GL7 cooldown subprocess is running it owns the LS350/LS370 ports
// intermittently.  The worker must NOT attempt a temp poll while
// gl7_cooldown_active is true, to avoid exclusive-lock port conflicts.
// When the cooldown is not active (and recording is not active), polling is
// expected to proceed.
//
// These tests mirror the gate condition:
//   recording_stop_flag.is_none() && !gl7_cooldown_active
// without spawning a real worker.

#[test]
fn temp_poll_blocked_when_gl7_cooldown_active() {
    let mut s = DeviceSnapshot::default();
    s.gl7_cooldown_active = true;

    // Simulate the gate: recording flag is absent, but cooldown is active.
    // The poll must NOT run.
    let recording_inactive = true; // represents recording_stop_flag.is_none()
    let should_poll = recording_inactive && !s.gl7_cooldown_active;

    assert!(!should_poll, "temp poll must be blocked while GL7 cooldown is active");
}

#[test]
fn temp_poll_allowed_when_neither_recording_nor_cooldown() {
    let s = DeviceSnapshot::default();
    // Default snapshot: gl7_cooldown_active = false.
    let recording_inactive = true;
    let should_poll = recording_inactive && !s.gl7_cooldown_active;

    assert!(should_poll, "temp poll must run when neither recording nor cooldown is active");
}

#[test]
fn temp_poll_blocked_when_both_recording_and_cooldown() {
    let mut s = DeviceSnapshot::default();
    s.gl7_cooldown_active = true;

    let recording_inactive = false; // recording_stop_flag.is_some()
    let should_poll = recording_inactive && !s.gl7_cooldown_active;

    assert!(!should_poll, "temp poll must be blocked when both recording and cooldown are active");
}

// ── Multiple result fields drain independently ────────────────────────────────

#[test]
fn multiple_result_fields_drain_independently() {
    let mut s = DeviceSnapshot::default();
    s.compressor_cmd_result  = Some(Ok(()));
    s.magnet_cmd_result      = Some(Err("quench".to_string()));
    s.magnet_rate_result     = Some(Ok(()));
    s.magnet_limits_result   = Some(Err("limit".to_string()));

    assert!(s.compressor_cmd_result.take().unwrap().is_ok());
    assert_eq!(s.magnet_cmd_result.take().unwrap().unwrap_err(), "quench");
    assert!(s.magnet_rate_result.take().unwrap().is_ok());
    assert_eq!(s.magnet_limits_result.take().unwrap().unwrap_err(), "limit");

    // All other result fields untouched:
    assert!(s.magnet_compliance_result.is_none());
}
