// adr_ramping.rs — Automated ADR ramp sequence for FROST
//
// Usage:  frost adr ramp [OPTIONS] <rate> <current>
//
//   <rate>              Ramp rate in A/s  (passed to LakeShore 625 RATE)
//   <current>           Target current in A (passed to LakeShore 625 SETI)
//
// Options:
//   --soak-mins <MINS>  Soak duration in minutes before demagnetising (default: 45)
//
// Sequence
//   1. Start background ramp logger (silent CSV, one row/min)
//   2. Set ramp rate on LakeShore 625
//   3. Set target current on LakeShore 625  (instrument begins ramping immediately)
//   4. Soak at that current for `soak_mins` minutes  (default 45)
//   5. Open heat switch via Zaber motor
//   6. Wait 3 minutes for heat switch to fully open
//   7. Set current to 0 A on LakeShore 625  (instrument ramps back down)
//   8. Wait for current to reach ≤ 0.004 A
//   9. Stop background logger

use std::io::{Write, stdout};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use crate::heatswitch::HeatswitchController;
use crate::lakeshore625::LakeShore625Controller;

// ── Ramp tolerances ───────────────────────────────────────────────────────────

/// Current must be within this many amps of the target before the soak begins.
pub const SOAK_TOLERANCE: f64 = 0.04;

/// Current must drop to this level (amps) or below before the sequence completes.
pub const ZERO_TOLERANCE: f64 = 0.004;

// ── Log message type (used by GUI to display live ramp progress) ─────────────

/// A progress message emitted by `run_adr_ramp`.
/// The GUI relays these into the ADR log panel; the CLI ignores them (stdout
/// is used directly instead).
pub enum AdrLogMsg {
    /// A completed line — append to the log.
    Line(String),
    /// A live-updating status line — replace the current status display.
    Status(String),
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Run the full automated ADR ramp sequence.
///
/// Logging runs automatically in the background (silent CSV, one row/min)
/// from the moment the ramp starts until the sequence completes.
///
/// Ports use the compiled-in defaults: LS625 → `/dev/ttyUSB0`, heat switch → `/dev/ttyUSB4`.
///
/// Pass `Some(tx)` to receive live progress messages in the GUI; pass `None`
/// for CLI use (output goes to stdout only).
pub fn run_adr_ramp(
    rate: f64,
    current: f64,
    soak_mins: u64,
    log: Option<&Sender<AdrLogMsg>>,
) -> Result<(), String> {
    // Closures that print to stdout AND optionally send to the GUI channel.
    // `log` is `Option<&Sender<...>>` which is Copy, so both closures can capture it.
    let ll = |msg: String| {                          // permanent log line
        println!("{}", msg);
        if let Some(tx) = log { let _ = tx.send(AdrLogMsg::Line(msg)); }
    };
    let ls = |msg: String| {                          // live status line (no newline)
        print!("\r{}", msg);
        stdout().flush().ok();
        if let Some(tx) = log { let _ = tx.send(AdrLogMsg::Status(msg)); }
    };
    // ── Start background ramp logger ──────────────────────────────────────────
    let log_path   = LakeShore625Controller::next_log_path();
    ll(format!("[ADR] Ramp data logging → {}", log_path));

    let stop_flag  = Arc::new(AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop_flag);
    let log_thread = std::thread::spawn(move || {
        let ctrl = LakeShore625Controller::default();
        if let Err(e) = ctrl.run_logging_until(stop_clone, false) {
            eprintln!("[ADR] Logging error: {e}");
        }
    });

    // ── Step 1: Set ramp rate ─────────────────────────────────────────────────
    ll(format!("[ADR] Step 1/7 — Setting ramp rate to {} A/s ...", rate));
    let mut ls625 = LakeShore625Controller::default();
    if let Err(e) = ls625.set_ramp_rate(rate) {
        stop_flag.store(true, Ordering::Relaxed);
        let _ = log_thread.join();
        return Err(e);
    }
    ll(format!("[ADR]           Ramp rate set to {} A/s.", rate));

    // ── Step 2: Set target current (instrument starts ramping immediately) ────
    ll(format!("[ADR] Step 2/7 — Setting target current to {} A ...", current));
    if let Err(e) = ls625.set_current(current) {
        stop_flag.store(true, Ordering::Relaxed);
        let _ = log_thread.join();
        return Err(e);
    }
    ll(format!("[ADR]           Target current set to {} A.  Instrument is now ramping.", current));

    // ── Step 2.5: Wait for current to reach target ────────────────────────────
    ll(format!("[ADR]           → Waiting for current to get within {} A of {} A...", SOAK_TOLERANCE, current));
    let tolerance = SOAK_TOLERANCE;

    loop {
        std::thread::sleep(Duration::from_secs(2)); // Check every 2 seconds

        match ls625.get_current() {
            Ok(current_str) => {
                if let Ok(actual_current) = current_str.parse::<f64>() {
                    ls(format!("[ADR]           Current: {:.3} A / {:.3} A target", actual_current, current));

                    if (actual_current - current).abs() <= tolerance {
                        // Finalize the status line as a permanent log entry.
                        ll(format!("[ADR]           ✓ Target current reached: {:.3} A (within {:.2} A of target)", actual_current, tolerance));
                        if let Some(tx) = log { let _ = tx.send(AdrLogMsg::Status(String::new())); }
                        break;
                    }
                } else {
                    continue;
                }
            }
            Err(_) => continue,
        }
    }

    // ── Step 3: Soak ─────────────────────────────────────────────────────────
    ll(format!(
        "[ADR] Step 3/7 — Soaking for {} minute{} at {} A.  Press Ctrl+C to abort.",
        soak_mins,
        if soak_mins == 1 { "" } else { "s" },
        current,
    ));
    ll(format!("[ADR]           → Soaking at {} A...", current));
    let soak_duration = Duration::from_secs(soak_mins * 60);
    countdown(soak_duration, log);
    ll("[ADR]           ✓ Soak complete.".to_string());

    // ── Step 4: Open heat switch ──────────────────────────────────────────────
    ll("[ADR] Step 4/7 — Opening heat switch in 30 seconds...".to_string());
    ll("[ADR]           Press Ctrl+C to abort before heat switch opens.".to_string());

    // 30-second countdown warning
    for i in (1..=30).rev() {
        ls(format!("[ADR]           Opening heat switch in {} seconds...  ", i));
        std::thread::sleep(Duration::from_secs(1));
    }
    ls("[ADR]           Opening heat switch now...                     ".to_string());
    println!();
    if let Some(tx) = log { let _ = tx.send(AdrLogMsg::Status(String::new())); }

    let mut hs = HeatswitchController::default();
    if let Err(e) = hs.open() {
        stop_flag.store(true, Ordering::Relaxed);
        let _ = log_thread.join();
        return Err(format!("Heat switch open failed: {e}"));
    }
    ll("[ADR]           ✓ Heat switch open command sent.  Motor is moving.".to_string());

    // ── Step 5: Wait for heat switch to fully open ────────────────────────────
    ll("[ADR] Step 5/7 — Waiting 3 minutes for heat switch to fully open...".to_string());
    let buffer_duration = Duration::from_secs(3 * 60); // 3 minutes
    countdown(buffer_duration, log);
    ll("[ADR]           ✓ Heat switch buffer complete.".to_string());

    // ── Step 6: Ramp current to 0 ────────────────────────────────────────────
    ll("[ADR] Step 6/7 — Setting target current to 0 A (instrument ramping down) ...".to_string());
    if let Err(e) = ls625.set_current(0.0) {
        stop_flag.store(true, Ordering::Relaxed);
        let _ = log_thread.join();
        return Err(e);
    }
    ll("[ADR]           Target current set to 0 A.  Instrument is ramping down.".to_string());

    // ── Step 7: Wait for current to reach near zero ──────────────────────────
    ll(format!("[ADR] Step 7/7 — Waiting for current to reach {} A or lower...", ZERO_TOLERANCE));
    let zero_tolerance = ZERO_TOLERANCE;

    loop {
        std::thread::sleep(Duration::from_secs(2)); // Check every 2 seconds

        match ls625.get_current() {
            Ok(current_str) => {
                if let Ok(actual_current) = current_str.parse::<f64>() {
                    ls(format!("[ADR]           Current: {:.3} A (target: ≤ {:.3} A)", actual_current, zero_tolerance));

                    if actual_current <= zero_tolerance {
                        ll(format!("[ADR]           ✓ Current reached near zero: {:.3} A (≤ {:.3} A)", actual_current, zero_tolerance));
                        if let Some(tx) = log { let _ = tx.send(AdrLogMsg::Status(String::new())); }
                        break;
                    }
                } else {
                    continue;
                }
            }
            Err(_) => continue,
        }
    }

    // ── Stop logger ───────────────────────────────────────────────────────────
    stop_flag.store(true, Ordering::Relaxed);
    let _ = log_thread.join();

    ll("[ADR] ──────────────────────────────────────────────────────────".to_string());
    ll(format!("[ADR] Sequence complete.  Ramp log: {}", log_path));

    Ok(())
}

// ── Countdown helper ──────────────────────────────────────────────────────────

/// Blocks for `duration`, printing a live countdown to stdout that updates every
/// second by overwriting the current line with `\r`.  A final newline is printed
/// once the soak is over.  Pass `Some(tx)` to also relay status to the GUI.
fn countdown(duration: Duration, log: Option<&Sender<AdrLogMsg>>) {
    let start = Instant::now();
    let total_secs = duration.as_secs();

    loop {
        let elapsed = start.elapsed();
        if elapsed >= duration {
            break;
        }

        let remaining = duration.saturating_sub(elapsed);
        let ela_secs  = elapsed.as_secs();
        let rem_secs  = remaining.as_secs();

        let msg = format!(
            "[ADR]           {:02}:{:02} elapsed  |  {:02}:{:02} remaining  ({:.0}%)   ",
            ela_secs / 60, ela_secs % 60,
            rem_secs / 60, rem_secs % 60,
            (ela_secs as f64 / total_secs as f64) * 100.0,
        );
        print!("\r{}", msg);
        stdout().flush().ok();
        if let Some(tx) = log { let _ = tx.send(AdrLogMsg::Status(msg)); }

        std::thread::sleep(Duration::from_secs(1));
    }

    // Final line: show 100 % complete and move to next line.
    let final_msg = format!(
        "[ADR]           {:02}:{:02} elapsed  |  00:00 remaining  (100%)   ",
        total_secs / 60, total_secs % 60,
    );
    print!("\r{}", final_msg);
    stdout().flush().ok();
    println!();
    if let Some(tx) = log { let _ = tx.send(AdrLogMsg::Status(String::new())); }
}
