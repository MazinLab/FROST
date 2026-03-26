// tests/gui_state_tests.rs — Stage 2: button-state persistence tests
//
// Verifies the lock-file mechanism used to restore compressor and ADR-ramp
// button appearance across GUI restarts.  Uses unique temp paths per test so
// parallel execution never collides.  No serial ports are opened.
//
// Run with: cargo test

use std::path::{Path, PathBuf};
use frost::worker::{
    set_compressor_intent_at, is_compressor_intent_at,
    set_adr_ramp_persisted_at, is_adr_ramp_persisted_at,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Unique temp path per test.  Uses thread ID so parallel tests don't collide.
fn tmp(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "frost_gui_state_{label}_{:?}_{}",
        std::thread::current().id(),
        std::process::id(),
    ));
    p
}

/// Unique temp path inside a subdirectory (tests dir-creation behaviour).
fn tmp_nested(dir_label: &str, file_label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "frost_gui_state_dir_{dir_label}_{:?}_{}",
        std::thread::current().id(),
        std::process::id(),
    ));
    p.push(file_label);
    p
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
    // Also try removing parent if it was created for this test:
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir(parent);
    }
}

// ── Compressor intent lock file ───────────────────────────────────────────────

#[test]
fn compressor_intent_write_true_creates_file() {
    let path = tmp("comp_true");
    let _ = std::fs::remove_file(&path); // clean slate

    set_compressor_intent_at(&path, true);
    assert!(path.exists(), "file must exist after set(true)");

    cleanup(&path);
}

#[test]
fn compressor_intent_write_false_removes_file() {
    let path = tmp("comp_false");
    std::fs::write(&path, "").unwrap(); // pre-create it

    set_compressor_intent_at(&path, false);
    assert!(!path.exists(), "file must be gone after set(false)");
}

#[test]
fn compressor_intent_missing_returns_false() {
    let path = tmp("comp_missing");
    let _ = std::fs::remove_file(&path);

    assert!(!is_compressor_intent_at(&path));
}

#[test]
fn compressor_intent_present_returns_true() {
    let path = tmp("comp_present");
    std::fs::write(&path, "").unwrap();

    assert!(is_compressor_intent_at(&path));

    cleanup(&path);
}

#[test]
fn compressor_intent_roundtrip_start_then_stop() {
    let path = tmp("comp_roundtrip");
    let _ = std::fs::remove_file(&path);

    set_compressor_intent_at(&path, true);
    assert!(is_compressor_intent_at(&path), "should be running after Start");

    set_compressor_intent_at(&path, false);
    assert!(!is_compressor_intent_at(&path), "should be stopped after Stop");
}

#[test]
fn compressor_intent_set_true_twice_is_idempotent() {
    let path = tmp("comp_idempotent");
    let _ = std::fs::remove_file(&path);

    set_compressor_intent_at(&path, true);
    set_compressor_intent_at(&path, true); // should not error
    assert!(is_compressor_intent_at(&path));

    cleanup(&path);
}

#[test]
fn compressor_intent_set_false_when_absent_is_no_op() {
    let path = tmp("comp_false_absent");
    let _ = std::fs::remove_file(&path);
    assert!(!path.exists());

    // Removing a file that doesn't exist must not panic.
    set_compressor_intent_at(&path, false);
    assert!(!path.exists());
}

// ── ADR ramp lock file ────────────────────────────────────────────────────────

#[test]
fn adr_ramp_write_true_creates_file() {
    let path = tmp("adr_true");
    let _ = std::fs::remove_file(&path);

    set_adr_ramp_persisted_at(&path, true);
    assert!(path.exists(), "file must exist after set(true)");

    cleanup(&path);
}

#[test]
fn adr_ramp_write_false_removes_file() {
    let path = tmp("adr_false");
    std::fs::write(&path, "").unwrap();

    set_adr_ramp_persisted_at(&path, false);
    assert!(!path.exists(), "file must be gone after set(false)");
}

#[test]
fn adr_ramp_missing_returns_false() {
    let path = tmp("adr_missing");
    let _ = std::fs::remove_file(&path);

    assert!(!is_adr_ramp_persisted_at(&path));
}

#[test]
fn adr_ramp_present_returns_true() {
    let path = tmp("adr_present");
    std::fs::write(&path, "").unwrap();

    assert!(is_adr_ramp_persisted_at(&path));

    cleanup(&path);
}

#[test]
fn adr_ramp_roundtrip_start_then_complete() {
    let path = tmp("adr_roundtrip");
    let _ = std::fs::remove_file(&path);

    set_adr_ramp_persisted_at(&path, true);
    assert!(is_adr_ramp_persisted_at(&path), "should be set while ramp runs");

    set_adr_ramp_persisted_at(&path, false);
    assert!(!is_adr_ramp_persisted_at(&path), "should be cleared when ramp completes");
}

#[test]
fn adr_ramp_set_false_when_absent_is_no_op() {
    let path = tmp("adr_false_absent");
    let _ = std::fs::remove_file(&path);

    // Must not panic:
    set_adr_ramp_persisted_at(&path, false);
    assert!(!path.exists());
}

// ── Parent-directory creation ─────────────────────────────────────────────────
//
// Both set_*_at helpers must create parent dirs automatically (mirrors
// state/.compressor_intent where state/ may not exist yet).

#[test]
fn compressor_intent_creates_missing_parent_directory() {
    let path = tmp_nested("comp_dir", ".compressor_intent");
    let parent = path.parent().unwrap();
    let _ = std::fs::remove_dir_all(parent); // ensure absent
    assert!(!parent.exists());

    set_compressor_intent_at(&path, true);

    assert!(parent.exists(), "parent directory must be created");
    assert!(path.exists(), "lock file must be created");

    cleanup(&path);
}

#[test]
fn adr_ramp_creates_missing_parent_directory() {
    let path = tmp_nested("adr_dir", ".adr_ramp_running");
    let parent = path.parent().unwrap();
    let _ = std::fs::remove_dir_all(parent);
    assert!(!parent.exists());

    set_adr_ramp_persisted_at(&path, true);

    assert!(parent.exists(), "parent directory must be created");
    assert!(path.exists(), "lock file must be created");

    cleanup(&path);
}

// ── Simulated restart scenario ────────────────────────────────────────────────
//
// Verifies the full lifecycle: ramp starts → lock file written → process dies
// (simulated by keeping file) → new process reads file → flag set → file cleared.

#[test]
fn adr_ramp_restart_cycle_interrupted_flag_set_once() {
    let path = tmp("adr_restart");
    let _ = std::fs::remove_file(&path);

    // Session 1: ramp starts
    set_adr_ramp_persisted_at(&path, true);
    assert!(is_adr_ramp_persisted_at(&path));

    // Process dies mid-ramp — lock file remains.

    // Session 2: startup reads lock file, sets interrupted flag, clears file.
    let interrupted = is_adr_ramp_persisted_at(&path);
    set_adr_ramp_persisted_at(&path, false); // clear so it's one-shot

    assert!(interrupted, "interrupted flag must be true on first restart");
    assert!(!is_adr_ramp_persisted_at(&path), "lock file must be gone after startup");

    // Session 3: startup sees no lock file — no warning.
    let interrupted_again = is_adr_ramp_persisted_at(&path);
    assert!(!interrupted_again, "no spurious warning on second restart");
}

#[test]
fn compressor_intent_restart_cycle_restores_running_state() {
    let path = tmp("comp_restart");
    let _ = std::fs::remove_file(&path);

    // Session 1: user clicks Start → compressor polled as running → file written.
    set_compressor_intent_at(&path, true);
    assert!(is_compressor_intent_at(&path));

    // Process exits.

    // Session 2: startup reads intent → seeds compressor_running = true.
    let intent_running = is_compressor_intent_at(&path);
    assert!(intent_running, "compressor must appear running immediately on restart");

    cleanup(&path);
}
