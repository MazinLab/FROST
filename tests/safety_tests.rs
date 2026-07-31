// safety_tests.rs — start-interlock decision logic and override-state tests.
//
// HARDWARE SAFETY: every test in this file exercises only pure decision
// functions and file-based override state. None of them call a device
// controller method or open a serial port, so no bytes can ever reach an
// instrument. In particular `safety::guard_start` is NOT tested here: it opens
// the real default device ports internally, so calling it could touch
// hardware. Its behaviour is covered indirectly by testing the pure cores it
// delegates to (`evaluate_start`).

use std::path::PathBuf;
use std::time::Duration;

use frost::safety::{
    self, evaluate_start, evaluate_start_from_snapshot, Interlock, MAX_START_4K_STAGE_K,
};

// ── Helpers ───────────────────────────────────────────────────

/// A unique override-file path in the system temp dir, isolated per test so
/// parallel tests never collide. Not date/rng based (deterministic name).
fn temp_override_path(test_name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("frost_safety_test_{test_name}")).join(".safety_disabled")
}

fn cleanup(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir(parent);
    }
}

fn has_compressor_off(v: &[Interlock]) -> bool {
    v.iter().any(|i| matches!(i, Interlock::CompressorOff))
}
fn has_too_warm(v: &[Interlock]) -> bool {
    v.iter().any(|i| matches!(i, Interlock::StageTooWarm { .. }))
}
fn has_unreadable(v: &[Interlock]) -> bool {
    v.iter().any(|i| matches!(i, Interlock::SensorUnreadable(_)))
}

// ── evaluate_start: the pure decision core ────────────────────

#[test]
fn evaluate_start_all_good_passes() {
    assert!(evaluate_start(Some(true), Some(4.0)).is_ok());
}

#[test]
fn evaluate_start_compressor_off_blocks() {
    let err = evaluate_start(Some(false), Some(4.0)).unwrap_err();
    assert!(has_compressor_off(&err));
    assert!(!has_too_warm(&err));
    assert_eq!(err.len(), 1);
}

#[test]
fn evaluate_start_stage_too_warm_blocks() {
    let err = evaluate_start(Some(true), Some(4.5)).unwrap_err();
    assert!(has_too_warm(&err));
    assert!(!has_compressor_off(&err));
    // The offending temperature is carried in the variant.
    assert!(err.iter().any(|i| matches!(i, Interlock::StageTooWarm { temp_k } if (*temp_k - 4.5).abs() < 1e-9)));
}

#[test]
fn evaluate_start_boundary_exactly_threshold_blocks() {
    // >= MAX blocks: exactly 4.2 K must NOT be allowed to start.
    let err = evaluate_start(Some(true), Some(MAX_START_4K_STAGE_K)).unwrap_err();
    assert!(has_too_warm(&err));
}

#[test]
fn evaluate_start_just_below_threshold_passes() {
    assert!(evaluate_start(Some(true), Some(MAX_START_4K_STAGE_K - 0.001)).is_ok());
}

#[test]
fn evaluate_start_both_violations_reported_together() {
    let err = evaluate_start(Some(false), Some(5.0)).unwrap_err();
    assert!(has_compressor_off(&err));
    assert!(has_too_warm(&err));
    assert_eq!(err.len(), 2);
}

#[test]
fn evaluate_start_unreadable_compressor_blocks_failsafe() {
    let err = evaluate_start(None, Some(4.0)).unwrap_err();
    assert!(has_unreadable(&err));
}

#[test]
fn evaluate_start_unreadable_temp_blocks_failsafe() {
    let err = evaluate_start(Some(true), None).unwrap_err();
    assert!(has_unreadable(&err));
}

#[test]
fn evaluate_start_both_unreadable_blocks() {
    let err = evaluate_start(None, None).unwrap_err();
    assert_eq!(err.len(), 2);
    assert!(err.iter().all(|i| matches!(i, Interlock::SensorUnreadable(_))));
}

// ── evaluate_start_from_snapshot: staleness + string parsing ──

const MAX_AGE: Duration = Duration::from_secs(60);

#[test]
fn snapshot_fresh_and_good_passes() {
    let r = evaluate_start_from_snapshot(
        true,
        Some(Duration::from_secs(10)),
        "4.1234 K",
        Some(Duration::from_secs(10)),
        MAX_AGE,
    );
    assert!(r.is_ok());
}

#[test]
fn snapshot_stale_compressor_blocks() {
    // Compressor reading older than max_age → treated as unreadable → block,
    // even though the cached bit says "running".
    let err = evaluate_start_from_snapshot(
        true,
        Some(Duration::from_secs(120)),
        "4.1234 K",
        Some(Duration::from_secs(10)),
        MAX_AGE,
    )
    .unwrap_err();
    assert!(has_unreadable(&err));
}

#[test]
fn snapshot_stale_temp_blocks() {
    let err = evaluate_start_from_snapshot(
        true,
        Some(Duration::from_secs(10)),
        "4.1234 K",
        Some(Duration::from_secs(120)),
        MAX_AGE,
    )
    .unwrap_err();
    assert!(has_unreadable(&err));
}

#[test]
fn snapshot_never_updated_blocks() {
    // No last_update timestamp at all → unreadable → block.
    let err = evaluate_start_from_snapshot(true, None, "4.1234 K", None, MAX_AGE).unwrap_err();
    assert_eq!(err.len(), 2);
    assert!(err.iter().all(|i| matches!(i, Interlock::SensorUnreadable(_))));
}

#[test]
fn snapshot_age_exactly_max_is_fresh() {
    // age == max_age counts as fresh (inclusive bound).
    let r = evaluate_start_from_snapshot(
        true,
        Some(MAX_AGE),
        "4.1234 K",
        Some(MAX_AGE),
        MAX_AGE,
    );
    assert!(r.is_ok());
}

#[test]
fn snapshot_overload_string_blocks() {
    let err = evaluate_start_from_snapshot(
        true,
        Some(Duration::from_secs(5)),
        "T_OVER",
        Some(Duration::from_secs(5)),
        MAX_AGE,
    )
    .unwrap_err();
    assert!(has_unreadable(&err));
}

#[test]
fn snapshot_no_data_dashes_blocks() {
    let err = evaluate_start_from_snapshot(
        true,
        Some(Duration::from_secs(5)),
        "---",
        Some(Duration::from_secs(5)),
        MAX_AGE,
    )
    .unwrap_err();
    assert!(has_unreadable(&err));
}

#[test]
fn snapshot_error_string_blocks() {
    let err = evaluate_start_from_snapshot(
        true,
        Some(Duration::from_secs(5)),
        "ERROR (timeout)",
        Some(Duration::from_secs(5)),
        MAX_AGE,
    )
    .unwrap_err();
    assert!(has_unreadable(&err));
}

#[test]
fn snapshot_compressor_off_blocks() {
    let err = evaluate_start_from_snapshot(
        false,
        Some(Duration::from_secs(5)),
        "4.1234 K",
        Some(Duration::from_secs(5)),
        MAX_AGE,
    )
    .unwrap_err();
    assert!(has_compressor_off(&err));
    assert!(!has_too_warm(&err));
}

#[test]
fn snapshot_warm_stage_blocks() {
    let err = evaluate_start_from_snapshot(
        true,
        Some(Duration::from_secs(5)),
        "4.5000 K",
        Some(Duration::from_secs(5)),
        MAX_AGE,
    )
    .unwrap_err();
    assert!(has_too_warm(&err));
}

#[test]
fn snapshot_boundary_stage_blocks() {
    let err = evaluate_start_from_snapshot(
        true,
        Some(Duration::from_secs(5)),
        "4.2000 K",
        Some(Duration::from_secs(5)),
        MAX_AGE,
    )
    .unwrap_err();
    assert!(has_too_warm(&err));
}

// ── Persisted override state ──────────────────────────────────

#[test]
fn override_default_is_safety_on() {
    let path = temp_override_path("default_on");
    cleanup(&path);
    // No file present → safety ON.
    assert!(safety::is_safety_enabled_at(&path));
    cleanup(&path);
}

#[test]
fn override_set_off_then_on_roundtrips() {
    let path = temp_override_path("roundtrip");
    cleanup(&path);

    // Turn OFF → file created, safety disabled.
    safety::set_safety_at(&path, false).unwrap();
    assert!(path.exists());
    assert!(!safety::is_safety_enabled_at(&path));

    // Turn ON → file removed, safety enabled.
    safety::set_safety_at(&path, true).unwrap();
    assert!(!path.exists());
    assert!(safety::is_safety_enabled_at(&path));

    cleanup(&path);
}

#[test]
fn override_set_on_when_absent_is_ok() {
    let path = temp_override_path("idempotent_on");
    cleanup(&path);
    // Enabling when already enabled (no file) must succeed, not error.
    assert!(safety::set_safety_at(&path, true).is_ok());
    assert!(safety::is_safety_enabled_at(&path));
    cleanup(&path);
}

// ── Failure audit log ─────────────────────────────────────────

#[test]
fn log_appends_timestamped_line() {
    let path = std::env::temp_dir()
        .join("frost_safety_test_log")
        .join("safety_log.txt");
    let _ = std::fs::remove_file(&path);

    safety::log_safety_event_at(&path, "ADR ramp blocked: compressor is not running");
    safety::log_safety_event_at(&path, "GL7 cooldown blocked: 4K stage is 5.000 K");

    let contents = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 2, "each event should append one line");
    assert!(lines[0].contains("compressor is not running"));
    assert!(lines[1].contains("4K stage is 5.000 K"));
    // Each line is timestamped (starts with a YYYY- date).
    assert!(lines[0].starts_with("20"), "line should start with a timestamp: {}", lines[0]);

    let _ = std::fs::remove_file(&path);
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir(parent);
    }
}

// ── Interlock messages ────────────────────────────────────────

#[test]
fn interlock_describe_mentions_cause() {
    assert!(Interlock::CompressorOff.describe().to_lowercase().contains("compressor"));
    assert!(Interlock::StageTooWarm { temp_k: 4.5 }.describe().contains("4.5"));
    assert!(Interlock::SensorUnreadable("4K stage".into())
        .describe()
        .to_lowercase()
        .contains("4k stage"));
}
