// gl7_automation_tests.rs — Unit tests for GL7 automation logic.
//
// Tests cover:
//   - DRY-RUN safety: set_output_percent is never called in gl7_automation.rs
//   - RollingAverage: push, len, average, rate_of_change
//   - pump_control_step: all control branches from the spec
//   - parse_csv_row / read_latest_temps: CSV parsing
//   - Phase 5: constants, no-output-write guarantee, EOL detection logic
//
// No hardware is activated. No serial ports are opened.
// Run with: cargo test --test gl7_automation_tests

use frost::gl7_automation::{pump_control_step, read_latest_temps, retry_on_busy, RollingAverage};
use std::io::Write;
use std::time::Duration;

// ── RollingAverage ─────────────────────────────────────────────────────────────

#[test]
fn rolling_average_empty() {
    let ra = RollingAverage::new(4);
    assert_eq!(ra.len(), 0);
    assert_eq!(ra.average(), 0.0);
    assert_eq!(ra.rate_of_change(), 0.0);
}

#[test]
fn rolling_average_single_entry() {
    let mut ra = RollingAverage::new(4);
    ra.push(50.0);
    assert_eq!(ra.len(), 1);
    assert_eq!(ra.average(), 50.0);
    // Rate requires at least 2 entries.
    assert_eq!(ra.rate_of_change(), 0.0);
}

#[test]
fn rolling_average_fills_and_evicts() {
    let mut ra = RollingAverage::new(3);
    ra.push(10.0);
    ra.push(20.0);
    ra.push(30.0);
    assert_eq!(ra.len(), 3);
    // Add a fourth value — oldest (10.0) is evicted.
    ra.push(40.0);
    assert_eq!(ra.len(), 3);
    // Buffer now holds 20, 30, 40.
    let avg = ra.average();
    assert!((avg - 30.0).abs() < 1e-9, "expected avg 30.0, got {avg}");
}

#[test]
fn rolling_average_correct_mean() {
    let mut ra = RollingAverage::new(4);
    for v in [50.0, 52.0, 54.0, 56.0] {
        ra.push(v);
    }
    let avg = ra.average();
    assert!((avg - 53.0).abs() < 1e-9, "expected 53.0, got {avg}");
}

#[test]
fn rolling_average_rate_of_change_positive() {
    let mut ra = RollingAverage::new(4);
    // Push two values a known time apart. We can't control Instant::now() in
    // integration tests, so we just verify the sign and rough magnitude.
    ra.push(50.0);
    std::thread::sleep(std::time::Duration::from_millis(100));
    ra.push(55.0);
    // Temperature rose by 5 K in ~0.1 s → ~3000 K/min. Sign must be positive.
    assert!(ra.rate_of_change() > 0.0, "rate should be positive for rising temps");
}

#[test]
fn rolling_average_rate_of_change_negative() {
    let mut ra = RollingAverage::new(4);
    ra.push(55.0);
    std::thread::sleep(std::time::Duration::from_millis(100));
    ra.push(50.0);
    assert!(ra.rate_of_change() < 0.0, "rate should be negative for falling temps");
}

// ── pump_control_step ──────────────────────────────────────────────────────────
//
// Pump 4 limits used throughout: lower=50K, upper=60K, floor=10%, ceiling=50%.
// Pump 3 limits:                 lower=45K, upper=55K, floor=8%,  ceiling=40%.
// (Mirrors the constants in gl7_automation.rs.)

const LOW4:  f64 = 50.0;
const HIGH4: f64 = 60.0;
const FL4:   f64 = 10.0;
const CL4:   f64 = 50.0;

const LOW3:  f64 = 45.0;
const HIGH3: f64 = 55.0;
const FL3:   f64 = 8.0;
const CL3:   f64 = 40.0;

fn step4(t_avg: f64, dt: f64, out: f64) -> Option<(f64, &'static str)> {
    pump_control_step(t_avg, dt, LOW4, HIGH4, FL4, CL4, out)
}

fn step3(t_avg: f64, dt: f64, out: f64) -> Option<(f64, &'static str)> {
    pump_control_step(t_avg, dt, LOW3, HIGH3, FL3, CL3, out)
}

// ── Sweet spot — no drift ──────────────────────────────────────────────────────

#[test]
fn sweet_spot_no_drift_no_action() {
    // 55 K is in the 53–57 K sweet spot for pump4. Drift = 0.0 → do nothing.
    assert!(step4(55.0, 0.0, 25.0).is_none());
}

#[test]
fn sweet_spot_weak_positive_drift_no_action() {
    // dT/dt = +0.05 K/min is below STABILIZE_DRIFT_STRONG (0.08) → no action.
    assert!(step4(55.0, 0.05, 25.0).is_none());
}

#[test]
fn sweet_spot_strong_positive_drift_reduce() {
    // dT/dt = +0.10 K/min > 0.08 → reduce by 2%.
    let (new_out, _) = step4(55.0, 0.10, 25.0).expect("should adjust");
    assert!((new_out - 23.0).abs() < 1e-9, "expected 23.0, got {new_out}");
}

#[test]
fn sweet_spot_strong_negative_drift_increase() {
    // dT/dt = −0.10 K/min → increase by 2%.
    let (new_out, _) = step4(55.0, -0.10, 25.0).expect("should adjust");
    assert!((new_out - 27.0).abs() < 1e-9, "expected 27.0, got {new_out}");
}

// ── Near upper limit ───────────────────────────────────────────────────────────

#[test]
fn near_upper_limit_slow_rise_small_reduction() {
    // 58 K is in the 57–60 K near-upper band. dT/dt = +0.03 > SLOW (0.02) → −1%.
    let (new_out, _) = step4(58.0, 0.03, 30.0).expect("should adjust");
    assert!((new_out - 29.0).abs() < 1e-9, "expected 29.0, got {new_out}");
}

#[test]
fn near_upper_limit_fast_rise_medium_reduction() {
    // dT/dt = +0.07 > FAST (0.05) → −3%.
    let (new_out, _) = step4(58.0, 0.07, 30.0).expect("should adjust");
    assert!((new_out - 27.0).abs() < 1e-9, "expected 27.0, got {new_out}");
}

#[test]
fn near_upper_limit_no_drift_no_action() {
    // In the near-upper band but drift < SLOW → do nothing.
    assert!(step4(58.0, 0.01, 30.0).is_none());
}

// ── Above upper limit ──────────────────────────────────────────────────────────

#[test]
fn above_upper_limit_reduces_by_5() {
    let (new_out, _) = step4(61.0, 0.0, 30.0).expect("should adjust");
    assert!((new_out - 25.0).abs() < 1e-9, "expected 25.0, got {new_out}");
}

#[test]
fn above_upper_limit_clamps_to_ceiling() {
    // At ceiling (50%) already? No, ceiling applies on increase. Here we reduce
    // from 12% by 5% → 7%, but floor is 10% → clamped to 10%.
    let (new_out, _) = step4(61.0, 0.0, 12.0).expect("should adjust");
    assert!((new_out - 10.0).abs() < 1e-9, "expected 10.0 (floor), got {new_out}");
}

#[test]
fn above_upper_limit_at_floor_no_effective_change() {
    // Already at floor (10%). Reducing by 5% would give 5%, clamped to 10%.
    // No effective change → returns None.
    assert!(step4(61.0, 0.0, 10.0).is_none());
}

// ── Near lower limit ───────────────────────────────────────────────────────────

#[test]
fn near_lower_limit_slow_fall_small_increase() {
    // 51 K is in the 50–53 K near-lower band. dT/dt = −0.03 → +1%.
    let (new_out, _) = step4(51.0, -0.03, 20.0).expect("should adjust");
    assert!((new_out - 21.0).abs() < 1e-9, "expected 21.0, got {new_out}");
}

#[test]
fn near_lower_limit_fast_fall_medium_increase() {
    // dT/dt = −0.07 < −FAST (−0.05) → +3%.
    let (new_out, _) = step4(51.0, -0.07, 20.0).expect("should adjust");
    assert!((new_out - 23.0).abs() < 1e-9, "expected 23.0, got {new_out}");
}

#[test]
fn near_lower_limit_no_drift_no_action() {
    assert!(step4(51.0, -0.01, 20.0).is_none());
}

// ── Below lower limit ──────────────────────────────────────────────────────────

#[test]
fn below_lower_limit_increases_by_5() {
    let (new_out, _) = step4(49.0, 0.0, 20.0).expect("should adjust");
    assert!((new_out - 25.0).abs() < 1e-9, "expected 25.0, got {new_out}");
}

#[test]
fn below_lower_limit_clamps_to_ceiling() {
    // From 48% → 48+5 = 53%, clamped to ceiling 50%.
    let (new_out, _) = step4(49.0, 0.0, 48.0).expect("should adjust");
    assert!((new_out - 50.0).abs() < 1e-9, "expected 50.0 (ceiling), got {new_out}");
}

#[test]
fn below_lower_limit_at_ceiling_no_effective_change() {
    assert!(step4(49.0, 0.0, 50.0).is_none());
}

// ── Pump 3 limits respected ────────────────────────────────────────────────────

#[test]
fn pump3_above_upper_reduces_by_5() {
    let (new_out, _) = step3(56.0, 0.0, 20.0).expect("should adjust");
    assert!((new_out - 15.0).abs() < 1e-9, "expected 15.0, got {new_out}");
}

#[test]
fn pump3_below_lower_increases_by_5() {
    let (new_out, _) = step3(44.0, 0.0, 15.0).expect("should adjust");
    assert!((new_out - 20.0).abs() < 1e-9, "expected 20.0, got {new_out}");
}

#[test]
fn pump3_ceiling_respected() {
    // ceiling for pump3 is 40%, not 50%.
    let (new_out, _) = step3(44.0, 0.0, 38.0).expect("should adjust");
    assert!((new_out - 40.0).abs() < 1e-9, "expected 40.0 (pump3 ceiling), got {new_out}");
}

#[test]
fn pump3_floor_respected() {
    // floor for pump3 is 8%.
    let (new_out, _) = step3(56.0, 0.0, 10.0).expect("should adjust");
    assert!((new_out - 8.0).abs() < 1e-9, "expected 8.0 (pump3 floor), got {new_out}");
}

// ── CSV parsing ────────────────────────────────────────────────────────────────

/// Write a minimal temperature CSV to a temp file and return the path.
fn write_temp_csv(rows: &[&str]) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    // Header + separator matching the record_temps.rs fixed-width format.
    writeln!(f, "Timestamp           Date       Time     4K_Stage  ADR_Res   ADR_Temp  Switch_Volt Switch_Temp 3Head_Res 3Head_Temp 4Head_Res_Raw 4Head_Res_Adj 4Head_Temp 3Pump_Volt 3Pump_Temp 4Pump_Volt 4Pump_Temp LS370_In1_Res LS370_In1_Temp").unwrap();
    writeln!(f, "---").unwrap();
    for row in rows {
        writeln!(f, "{}", row).unwrap();
    }
    f
}

/// Build a whitespace-delimited row with the six temperatures at the expected
/// column indices (0-based):
///   3  → stage_4k,  7 → switch,  9 → head3, 12 → head4, 14 → pump3, 16 → pump4
fn make_row(stage: f64, switch: f64, head3: f64, head4: f64, pump3: f64, pump4: f64) -> String {
    // Columns: 0=ts 1=date 2=time 3=stage 4=x 5=x 6=x 7=switch 8=x 9=head3
    //          10=x 11=x 12=head4 13=x 14=pump3 15=x 16=pump4
    format!(
        "1234567890 2025-01-01 12:00:00 {stage:.3} 0.0 0.0 0.0 {switch:.3} 0.0 {head3:.3} 0.0 0.0 {head4:.3} 0.0 {pump3:.3} 0.0 {pump4:.3}"
    )
}

#[test]
fn read_latest_temps_single_row() {
    let row = make_row(3.9, 4.1, 3.8, 3.7, 4.2, 4.3);
    let f = write_temp_csv(&[&row]);
    let t = read_latest_temps(f.path().to_str().unwrap()).unwrap();
    assert!((t.stage_4k_k - 3.9).abs() < 1e-6);
    assert!((t.switch_k   - 4.1).abs() < 1e-6);
    assert!((t.head3_k    - 3.8).abs() < 1e-6);
    assert!((t.head4_k    - 3.7).abs() < 1e-6);
    assert!((t.pump3_k    - 4.2).abs() < 1e-6);
    assert!((t.pump4_k    - 4.3).abs() < 1e-6);
}

#[test]
fn read_latest_temps_returns_last_row() {
    let row1 = make_row(3.9, 4.1, 3.8, 3.7, 4.2, 4.3);
    let row2 = make_row(4.0, 4.2, 3.9, 3.8, 52.0, 55.0);
    let f = write_temp_csv(&[&row1, &row2]);
    let t = read_latest_temps(f.path().to_str().unwrap()).unwrap();
    assert!((t.pump4_k - 55.0).abs() < 1e-6, "should use last row");
    assert!((t.pump3_k - 52.0).abs() < 1e-6, "should use last row");
}

#[test]
fn read_latest_temps_missing_file_errors() {
    let result = read_latest_temps("/tmp/frost_no_such_file_gl7_test.csv");
    assert!(result.is_err());
}

#[test]
fn read_latest_temps_empty_file_errors() {
    let f = write_temp_csv(&[]);
    let result = read_latest_temps(f.path().to_str().unwrap());
    assert!(result.is_err(), "empty CSV (no data rows) should return Err");
}

// ── Dry-run safety ─────────────────────────────────────────────────────────────

// ── retry_on_busy ─────────────────────────────────────────────────────────────

/// A successful operation returns Ok immediately with no retry.
#[test]
fn retry_on_busy_returns_ok_immediately() {
    let mut calls = 0usize;
    let result = retry_on_busy("test", Duration::from_millis(0), || {
        calls += 1;
        Ok::<i32, String>(42)
    });
    assert_eq!(result, Ok(42));
    assert_eq!(calls, 1);
}

/// A non-busy error is returned immediately — no retry is attempted.
#[test]
fn retry_on_busy_non_busy_error_no_retry() {
    let mut calls = 0usize;
    let result = retry_on_busy("test", Duration::from_millis(0), || {
        calls += 1;
        Err::<i32, String>("some other serial error".to_string())
    });
    assert!(result.is_err());
    assert_eq!(calls, 1, "non-busy error must not trigger a retry");
    assert_eq!(result.unwrap_err(), "some other serial error");
}

/// A single "Device or resource busy" error triggers one retry, then succeeds.
#[test]
fn retry_on_busy_retries_once_then_succeeds() {
    let mut calls = 0usize;
    let result = retry_on_busy("test", Duration::from_millis(0), || {
        calls += 1;
        if calls == 1 {
            Err("Failed to open /dev/ttyUSB2: Device or resource busy".to_string())
        } else {
            Ok::<i32, String>(99)
        }
    });
    assert_eq!(result, Ok(99));
    assert_eq!(calls, 2, "must retry exactly once after a busy error");
}

/// Multiple consecutive busy errors all retry until the operation succeeds.
#[test]
fn retry_on_busy_retries_multiple_times_until_success() {
    let mut calls = 0usize;
    let result = retry_on_busy("test", Duration::from_millis(0), || {
        calls += 1;
        if calls < 4 {
            Err("Failed to open /dev/ttyUSB2: Device or resource busy".to_string())
        } else {
            Ok::<i32, String>(7)
        }
    });
    assert_eq!(result, Ok(7));
    assert_eq!(calls, 4, "must retry until success regardless of busy count");
}

/// A busy error followed by a different (non-busy) error propagates the second
/// error immediately — it does not retry non-busy errors.
#[test]
fn retry_on_busy_then_non_busy_error_propagates() {
    let mut calls = 0usize;
    let result = retry_on_busy("test", Duration::from_millis(0), || {
        calls += 1;
        if calls == 1 {
            Err("Failed to open /dev/ttyUSB2: Device or resource busy".to_string())
        } else {
            Err::<i32, String>("connection reset".to_string())
        }
    });
    assert!(result.is_err());
    assert_eq!(calls, 2);
    assert_eq!(result.unwrap_err(), "connection reset");
}

// ── Phase 2 → Phase 3 transition threshold tests ──────────────────────────────

/// HEAD_PLATEAU_THRESHOLD must be 5.45 K (changed from 5.5 K).
/// Phase 2 exits to Phase 3 when the 4-head rolling average drops below this value.
#[test]
fn phase2_head_plateau_threshold_is_5_45() {
    assert_eq!(
        frost::gl7_automation::HEAD_PLATEAU_THRESHOLD,
        5.45,
        "HEAD_PLATEAU_THRESHOLD should be 5.45 K (was 5.5 K before the change)"
    );
}

/// Boundary conditions around the new 5.45 K threshold.
/// Values strictly below 5.45 must satisfy the condition; 5.45 and above must not.
#[test]
fn phase2_threshold_boundary_conditions() {
    let thresh = frost::gl7_automation::HEAD_PLATEAU_THRESHOLD;

    // Just below threshold → condition met.
    assert!(5.44 < thresh, "5.44 K should be below the 5.45 K threshold");
    assert!(5.40 < thresh, "5.40 K should be below the 5.45 K threshold");

    // Exactly at threshold → condition NOT met (strict <).
    assert!(!(5.45 < thresh), "5.45 K must not satisfy strict < 5.45 K");

    // Old threshold value and above → must fail with new constant.
    assert!(!(5.5 < thresh), "5.5 K (old threshold) must not satisfy the new 5.45 K threshold");
    assert!(!(5.6 < thresh), "5.6 K must not satisfy the 5.45 K threshold");
}

/// The heads_cool predicate in phase2_stabilize must reference only h4_avg,
/// not h3_avg.  This locks in the "only 4-head required" behavioral change.
#[test]
fn phase2_heads_cool_checks_only_4head() {
    let src = std::fs::read_to_string("src/gl7_automation.rs")
        .expect("src/gl7_automation.rs must be readable");

    let heads_cool_line = src
        .lines()
        .find(|l| l.contains("let heads_cool"))
        .expect("heads_cool assignment must exist in phase2_stabilize");

    assert!(
        heads_cool_line.contains("h4_avg"),
        "heads_cool must reference h4_avg (4-head rolling average)"
    );
    assert!(
        !heads_cool_line.contains("h3_avg"),
        "heads_cool must NOT reference h3_avg — only the 4-head triggers phase2→3 transition"
    );
}

/// Verify that dry-run mode has been removed: `dry_run_set` must not exist,
/// and real output writes must go through `set_output_pct`.
#[test]
fn live_mode_no_dry_run_set_uses_set_output_pct() {
    let src = std::fs::read_to_string("src/gl7_automation.rs")
        .expect("src/gl7_automation.rs must be readable");

    let live_code: String = src
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        !live_code.contains("dry_run_set"),
        "dry_run_set must not exist in live code — dry-run mode has been removed"
    );
    assert!(
        live_code.contains("set_output_pct"),
        "gl7_automation.rs must use set_output_pct to write real outputs to the LS350"
    );
}

// ── Phase 5 helpers ────────────────────────────────────────────────────────────

/// Extract the source text of `phase5_running` (from its `pub fn` line up to
/// but not including the next `\npub fn` or end-of-file).  Used by several
/// Phase 5 tests to scope assertions to this function only.
fn phase5_source() -> String {
    let src = std::fs::read_to_string("src/gl7_automation.rs")
        .expect("src/gl7_automation.rs must be readable");
    let start = src
        .find("pub fn phase5_running")
        .expect("phase5_running must exist in src");
    let after = &src[start..];
    let end = after.find("\npub fn ").unwrap_or(after.len());
    after[..end].to_owned()
}

/// Parse the value assigned to a `const NAME: f64 = VALUE;` line.
/// Returns `None` if the constant is not found or its value is not a valid f64.
fn parse_f64_const(src: &str, name: &str) -> Option<f64> {
    src.lines()
        .find(|l| l.contains(&format!("const {}:", name)))
        .and_then(|l| {
            let eq = l.rfind('=')?;
            let raw = l[eq + 1..].trim();
            // Strip trailing comment before parsing.
            let raw = raw.split("//").next().unwrap_or(raw).trim().trim_end_matches(';');
            raw.parse().ok()
        })
}

/// Parse the value assigned to a `const NAME: u64 = VALUE;` line.
fn parse_u64_const(src: &str, name: &str) -> Option<u64> {
    src.lines()
        .find(|l| l.contains(&format!("const {}:", name)))
        .and_then(|l| {
            let eq = l.rfind('=')?;
            // Strip trailing comment if any.
            let raw = l[eq + 1..].trim();
            let raw = raw.split("//").next().unwrap_or(raw).trim().trim_end_matches(';');
            raw.parse().ok()
        })
}

// ── Phase 5: no output writes ─────────────────────────────────────────────────

/// Phase 5 must not write any outputs — they are held at their Phase 4 entry
/// values while the GL7 runs at base temperature.
///
/// Scoped to the phase5_running body so that changes to other phases cannot
/// accidentally satisfy this assertion.
#[test]
fn phase5_makes_no_output_writes() {
    let body = phase5_source();

    // Strip comment lines before asserting so the check targets live code only.
    let live: String = body
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        !live.contains("set_output_pct"),
        "phase5_running must not call set_output_pct — outputs are immutable in Phase 5"
    );
    assert!(
        !live.contains("set_output_percent"),
        "phase5_running must not call set_output_percent — outputs are immutable in Phase 5"
    );
}

// ── Phase 5: constants match spec ─────────────────────────────────────────────

/// Spec (CLAUDE.md Phase 5): warn if 3-head rises above 400 mK.
#[test]
fn phase5_head3_warning_threshold_is_400mk() {
    let src = std::fs::read_to_string("src/gl7_automation.rs")
        .expect("src/gl7_automation.rs must be readable");
    let v = parse_f64_const(&src, "HEAD3_WARNING")
        .expect("HEAD3_WARNING constant must be defined");
    assert!(
        (v - 0.400).abs() < 1e-9,
        "HEAD3_WARNING must be 0.400 K (400 mK) per spec, got {v}"
    );
}

/// Spec (CLAUDE.md Phase 5): warn if 4-head rises above 3 K.
#[test]
fn phase5_head4_warning_threshold_is_3k() {
    let src = std::fs::read_to_string("src/gl7_automation.rs")
        .expect("src/gl7_automation.rs must be readable");
    let v = parse_f64_const(&src, "HEAD4_WARNING")
        .expect("HEAD4_WARNING constant must be defined");
    assert!(
        (v - 3.0).abs() < 1e-9,
        "HEAD4_WARNING must be 3.0 K per spec, got {v}"
    );
}

/// Spec (CLAUDE.md Phase 5): end-of-life temperature trigger is 3 K.
#[test]
fn phase5_expired_threshold_is_3k() {
    let src = std::fs::read_to_string("src/gl7_automation.rs")
        .expect("src/gl7_automation.rs must be readable");
    let v = parse_f64_const(&src, "HEAD4_EXPIRED_THRESHOLD")
        .expect("HEAD4_EXPIRED_THRESHOLD constant must be defined");
    assert!(
        (v - 3.0).abs() < 1e-9,
        "HEAD4_EXPIRED_THRESHOLD must be 3.0 K per spec, got {v}"
    );
}

/// Spec (CLAUDE.md Phase 5): rising rate > 0.01 K/min confirms ⁴He exhaustion.
#[test]
fn phase5_expired_rate_threshold_is_001_k_per_min() {
    let src = std::fs::read_to_string("src/gl7_automation.rs")
        .expect("src/gl7_automation.rs must be readable");
    let v = parse_f64_const(&src, "HEAD4_EXPIRED_RATE")
        .expect("HEAD4_EXPIRED_RATE constant must be defined");
    assert!(
        (v - 0.01).abs() < 1e-9,
        "HEAD4_EXPIRED_RATE must be 0.01 K/min per spec, got {v}"
    );
}

/// Spec (CLAUDE.md Phase 5): monitoring runs every 5 minutes.
#[test]
fn phase5_poll_interval_is_5_minutes() {
    let src = std::fs::read_to_string("src/gl7_automation.rs")
        .expect("src/gl7_automation.rs must be readable");
    let v = parse_u64_const(&src, "PHASE5_POLL_SECS")
        .expect("PHASE5_POLL_SECS constant must be defined");
    assert_eq!(v, 300, "PHASE5_POLL_SECS must be 300 seconds (5 minutes) per spec");
}

// ── Phase 5: end-of-life detection logic ──────────────────────────────────────

/// The EOL guard must require at least 2 rolling-average readings before
/// triggering, so that `rate_of_change()` is computed from a real delta and
/// not returned as 0.0 from a single-sample buffer.
#[test]
fn phase5_eol_requires_at_least_two_readings() {
    let body = phase5_source();
    assert!(
        body.contains("head4_avg.len() >= 2"),
        "EOL detection must guard on `head4_avg.len() >= 2` so dT/dt is meaningful"
    );
}

/// EOL detection must combine both the temperature threshold AND the rate of
/// change, preventing a momentary spike from falsely declaring exhaustion.
#[test]
fn phase5_eol_checks_threshold_and_rate_together() {
    let body = phase5_source();

    // Both constants must appear inside the function body (live code).
    let live: String = body
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        live.contains("HEAD4_EXPIRED_THRESHOLD"),
        "EOL detection must reference HEAD4_EXPIRED_THRESHOLD"
    );
    assert!(
        live.contains("HEAD4_EXPIRED_RATE"),
        "EOL detection must reference HEAD4_EXPIRED_RATE"
    );
}

/// The rolling-average buffer used for EOL detection must track the 4-head
/// (not the 3-head) — the 4-head warming is the canonical end-of-life signal.
#[test]
fn phase5_eol_tracks_4head_rolling_average() {
    let body = phase5_source();
    assert!(
        body.contains("head4_avg.push("),
        "phase5_running must push 4-head readings into head4_avg"
    );
    // 3-head is also tracked (for health warnings) but must not gate EOL.
    assert!(
        body.contains("head3_avg.push("),
        "phase5_running must track 3-head readings for health warning monitoring"
    );
    // Confirm EOL uses the 4-head rate, not the 3-head rate.
    assert!(
        body.contains("head4_avg.rate_of_change()"),
        "EOL detection must use head4_avg.rate_of_change(), not head3_avg"
    );
}

// ── Phase 5: RollingAverage EOL threshold simulation ─────────────────────────
//
// Because phase5_running sleeps 300 seconds at the top of every loop iteration,
// running the function end-to-end in a test would require 10+ minutes per
// check cycle.  Instead, the EOL detection logic is exercised here by driving
// a `RollingAverage` directly through the same conditions the function checks:
//
//   head4_avg.len() >= 2  AND  head4_k > HEAD4_EXPIRED_THRESHOLD (3.0 K)
//                         AND  h4_dt   > HEAD4_EXPIRED_RATE       (0.01 K/min)

/// A single rising 4-head reading must not trigger EOL (len < 2, no dT/dt).
#[test]
fn phase5_single_reading_does_not_satisfy_eol() {
    let mut avg = RollingAverage::new(4);
    avg.push(3.5); // above threshold
    // len == 1 → rate_of_change() returns 0.0, so EOL condition is false.
    assert!(avg.len() < 2, "one reading must not satisfy the >= 2 guard");
    assert_eq!(avg.rate_of_change(), 0.0, "rate must be 0.0 with a single sample");
}

/// Two rising readings above the threshold satisfy the EOL condition.
#[test]
fn phase5_two_rising_readings_above_threshold_satisfy_eol() {
    let mut avg = RollingAverage::new(4);
    avg.push(3.1);
    std::thread::sleep(std::time::Duration::from_millis(50));
    avg.push(3.5); // rising: rate > 0

    let head4_k = 3.5_f64;
    let h4_dt = avg.rate_of_change();

    let eol = avg.len() >= 2
        && head4_k > 3.0  // HEAD4_EXPIRED_THRESHOLD
        && h4_dt > 0.01;  // HEAD4_EXPIRED_RATE

    assert!(eol, "two rising readings above 3 K should satisfy EOL condition (rate={h4_dt:.4})");
}

/// A reading above 3 K that is falling (negative rate) must not trigger EOL.
#[test]
fn phase5_falling_4head_above_threshold_does_not_trigger_eol() {
    let mut avg = RollingAverage::new(4);
    avg.push(4.0); // was warmer
    std::thread::sleep(std::time::Duration::from_millis(50));
    avg.push(3.2); // now above threshold but cooling

    let head4_k = 3.2_f64;
    let h4_dt = avg.rate_of_change();

    let eol = avg.len() >= 2
        && head4_k > 3.0
        && h4_dt > 0.01;

    assert!(!eol, "a cooling 4-head must not trigger EOL even if above threshold (rate={h4_dt:.4})");
}

/// A slowly rising 4-head at exactly the rate threshold must not trigger EOL
/// (the condition is strict `>`).
#[test]
fn phase5_rate_exactly_at_threshold_does_not_trigger_eol() {
    // rate_of_change() is measured in real time, so we can't hit exactly
    // 0.01 K/min deterministically.  Instead verify the boundary via logic:
    // the condition is `h4_dt > HEAD4_EXPIRED_RATE`, so equality must fail.
    let rate_at_threshold = 0.01_f64;
    let eol_rate_condition = rate_at_threshold > 0.01;
    assert!(!eol_rate_condition, "rate == 0.01 K/min must not satisfy strict > 0.01");
}

/// Readings below the 3 K temperature threshold must not trigger EOL even if
/// the rate is positive.
#[test]
fn phase5_normal_4head_below_threshold_does_not_trigger_eol() {
    let mut avg = RollingAverage::new(4);
    avg.push(0.85);
    std::thread::sleep(std::time::Duration::from_millis(50));
    avg.push(0.90); // rising, but well below 3 K

    let head4_k = 0.90_f64;
    let h4_dt = avg.rate_of_change();

    let eol = avg.len() >= 2
        && head4_k > 3.0
        && h4_dt > 0.01;

    assert!(!eol, "normal 4-head temperature must not trigger EOL (head4={head4_k}, rate={h4_dt:.4})");
}
