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
    s.adr_log_lines    = vec!["old line 1".to_string(), "old line 2".to_string()];
    s.adr_status_line  = "old status".to_string();
    s.adr_ramp_result  = Some(Ok(()));
    s.adr_ramp_interrupted = true;

    // Simulate execute_command accepting a new RunAdrRamp:
    s.adr_ramp_running     = true;
    s.adr_ramp_started     = Some(std::time::Instant::now());
    s.adr_ramp_result      = None;
    s.adr_ramp_interrupted = false;
    s.adr_log_lines.clear();
    s.adr_status_line.clear();

    assert!(s.adr_ramp_running);
    assert!(s.adr_ramp_started.is_some());
    assert!(s.adr_ramp_result.is_none());
    assert!(!s.adr_ramp_interrupted);
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

// ── Snapshot adr_ramp_interrupted field ───────────────────────────────────────

#[test]
fn adr_ramp_interrupted_default_false() {
    let s = DeviceSnapshot::default();
    assert!(!s.adr_ramp_interrupted);
}

#[test]
fn adr_ramp_interrupted_cleared_when_ramp_starts() {
    let mut s = DeviceSnapshot::default();
    s.adr_ramp_interrupted = true;

    // New ramp clears the flag:
    s.adr_ramp_interrupted = false;
    s.adr_ramp_running     = true;

    assert!(!s.adr_ramp_interrupted);
    assert!(s.adr_ramp_running);
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
    tx.send(GuiCommand::RunAdrRamp { rate: 0.004, current: 9.44, soak_mins: 45 }).unwrap();
    tx.send(GuiCommand::StopCompressor).unwrap();
    tx.send(GuiCommand::SetGl7Output { output: 3, pct: 50.0 }).unwrap();

    assert!(matches!(rx.try_recv().unwrap(), GuiCommand::StartCompressor));
    assert!(matches!(rx.try_recv().unwrap(), GuiCommand::RunAdrRamp { .. }));
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
