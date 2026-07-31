// safety.rs — cross-device start interlocks for FROST
//
// Enforces two preconditions before any potentially-dangerous process is
// STARTED (ADR ramp, GL7 automation, or an ON-write to a GL7 output):
//   1. The compressor must be running.
//   2. The 4K stage (LS350 input D3) must be below MAX_START_4K_STAGE_K.
//
// Key design rules (see project plan):
//   * These checks fire ONCE, at a start entry point. They are never
//     re-evaluated during a running process. If the 4K stage rises above the
//     threshold mid-run, nothing here stops the process.
//   * A user-toggled override ("safety off") bypasses the interlocks. The
//     override state is persisted to a file so the CLI (separate one-off
//     processes) and the GUI agree, and it survives restarts and sessions.
//   * Fail-safe: if a reading cannot be obtained, the start is BLOCKED.
//
// The decision logic (`evaluate_start`) is pure and takes plain values, so it
// is unit-tested without ever opening a serial port. Only the private gather
// helpers touch hardware.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::compressor::CryomechController;
use crate::lakeshore350::LakeShore350Controller;

/// The 4K stage must be strictly below this (K) for a start to be permitted.
/// A reading `>= MAX_START_4K_STAGE_K` blocks the start.
pub const MAX_START_4K_STAGE_K: f64 = 4.2;

/// Name of the override file whose PRESENCE means safety is OFF (interlocks
/// bypassed); absence means safety is ON. See `safety_override_path`.
pub const SAFETY_OVERRIDE_FILE: &str = "safety_disabled";

/// Environment variable set by the GUI on subprocesses it spawns (ADR ramp,
/// GL7 cooldown). Its presence tells `guard_start` the GUI has ALREADY gated
/// the start against its cached snapshot, so the subprocess must NOT re-read
/// the hardware — that would race the still-running worker's poll for the same
/// serial port. See `guard_start` and the GUI spawn sites.
pub const GUI_CHECKED_ENV: &str = "FROST_SAFETY_GUI_CHECKED";

/// Recommended max age for a snapshot reading to be considered fresh enough for
/// a start decision (~two 30 s poll intervals). Stale readings block (fail-safe).
pub const SNAPSHOT_MAX_AGE_SECS: u64 = 60;

/// Append-only audit log of safety FAILURES (blocked starts and safety-toggle
/// errors) — never passes. One timestamped line per event.
pub const SAFETY_LOG_PATH: &str = "logs/safety_log.txt";

// ── Interlock violations ──────────────────────────────────────
#[derive(Debug, Clone, PartialEq)]
pub enum Interlock {
    /// Compressor is confirmed not running.
    CompressorOff,
    /// 4K stage temperature is at or above the start threshold.
    StageTooWarm { temp_k: f64 },
    /// A required reading could not be obtained (blocks, fail-safe).
    /// The `String` names which reading failed.
    SensorUnreadable(String),
}

impl Interlock {
    /// Human-readable reason shown in CLI output / GUI.
    pub fn describe(&self) -> String {
        match self {
            Interlock::CompressorOff => "compressor is not running".to_string(),
            Interlock::StageTooWarm { temp_k } => format!(
                "4K stage is {temp_k:.3} K (must be below {MAX_START_4K_STAGE_K} K to start)"
            ),
            Interlock::SensorUnreadable(what) => {
                format!("could not read {what} (failing safe — treated as unsafe)")
            }
        }
    }
}

// ── Pure decision core (no I/O — this is the unit under test) ──
/// Decide whether a start is permitted from already-obtained readings.
///
/// `compressor_running`: `Some(true/false)` = read succeeded; `None` = unreadable.
/// `stage_4k_k`: `Some(temp)` = read succeeded; `None` = unreadable / overrange.
///
/// Returns `Ok(())` if every precondition is met, otherwise `Err(violations)`
/// listing *all* violations at once. An unreadable sensor is itself a
/// violation (fail-safe).
pub fn evaluate_start(
    compressor_running: Option<bool>,
    stage_4k_k: Option<f64>,
) -> Result<(), Vec<Interlock>> {
    let mut violations = Vec::new();

    match compressor_running {
        Some(true) => {}
        Some(false) => violations.push(Interlock::CompressorOff),
        None => violations.push(Interlock::SensorUnreadable(
            "compressor running state".to_string(),
        )),
    }

    match stage_4k_k {
        Some(t) if t >= MAX_START_4K_STAGE_K => {
            violations.push(Interlock::StageTooWarm { temp_k: t })
        }
        Some(_) => {}
        None => violations.push(Interlock::SensorUnreadable(
            "4K stage temperature".to_string(),
        )),
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

// ── Snapshot adapter (Option A: reuse the GUI worker's cached reads) ──
// Parse the leading numeric token of a snapshot temperature string
// (e.g. "4.1234 K"). Overload / no-data / error strings ("T_OVER", "---",
// "ERROR (...)") have no leading float and yield `None`.
fn parse_stage_kelvin(d3_str: &str) -> Option<f64> {
    d3_str.split_whitespace().next()?.parse::<f64>().ok()
}

/// Evaluate a start using values pulled from the GUI worker's cached
/// `DeviceSnapshot` instead of fresh serial reads — avoids re-executing serial
/// commands and, within the GUI process, avoids racing the worker's own poll
/// for the same port.
///
/// A reading is used only if it is fresh: its age (`Instant::elapsed()` at the
/// call site, passed in here) must be `Some` and `<= max_age`. A stale,
/// never-updated, or unparseable reading becomes `None` and — via
/// `evaluate_start` — blocks the start (fail-safe). Compressor state is the
/// tighter case: it can flip the instant the compressor is stopped, so a tight
/// `max_age` (≈ two poll intervals) is intended.
///
/// This function is pure (takes `Duration`s, never reads a clock), so it is
/// unit-tested without hardware.
pub fn evaluate_start_from_snapshot(
    compressor_running: bool,
    compressor_age: Option<std::time::Duration>,
    stage_4k_d3: &str,
    temp_age: Option<std::time::Duration>,
    max_age: std::time::Duration,
) -> Result<(), Vec<Interlock>> {
    let fresh = |age: Option<std::time::Duration>| matches!(age, Some(a) if a <= max_age);

    let compressor = if fresh(compressor_age) {
        Some(compressor_running)
    } else {
        None
    };
    let stage = if fresh(temp_age) {
        parse_stage_kelvin(stage_4k_d3)
    } else {
        None
    };

    evaluate_start(compressor, stage)
}

// ── Failure audit log ─────────────────────────────────────────
/// Append a timestamped line to the safety log (path-injectable for tests).
/// Best-effort: never panics and never propagates I/O errors — a logging
/// failure must not itself block or crash a safety decision.
pub fn log_safety_event_at(path: &Path, message: &str) {
    let line = format!(
        "{} {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        message
    );
    ensure_parent(path);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Append a timestamped failure line to the default safety log.
pub fn log_safety_event(message: &str) {
    log_safety_event_at(Path::new(SAFETY_LOG_PATH), message);
}

// ── Persisted override state ──────────────────────────────────
fn ensure_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
}

/// Safety is enabled (ON) unless the override file is present.
/// Path-injectable form for testing against a temp directory.
pub fn is_safety_enabled_at(path: &Path) -> bool {
    // `exists()` is false when the file is absent OR cannot be stat'd; either
    // way we default to ON (fail-safe).
    !path.exists()
}

/// Absolute path to the safety override file: `$HOME/.frost/safety_disabled`.
///
/// It is deliberately **absolute and fixed**, not relative to the working
/// directory, so the CLI, the GUI, and any spawned subprocess all read and
/// write the SAME file regardless of where each was launched from — and so the
/// state persists across restarts. (A relative `state/` path would resolve
/// differently per process cwd, which is exactly why the CLI and GUI could
/// disagree.) Falls back to a project-relative path only if `$HOME` is unset.
pub fn safety_override_path() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => Path::new(&home).join(".frost").join(SAFETY_OVERRIDE_FILE),
        None => Path::new("state").join(".safety_disabled"),
    }
}

/// Safety is enabled (ON) unless the override file is present.
pub fn is_safety_enabled() -> bool {
    is_safety_enabled_at(&safety_override_path())
}

/// Set the persisted safety state. `enabled == true` → ON (remove override
/// file); `false` → OFF (create override file). Path-injectable for testing.
pub fn set_safety_at(path: &Path, enabled: bool) -> Result<(), String> {
    if enabled {
        // Turn safety ON: remove the override file. Absent already is success.
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Failed to clear safety override {path:?}: {e}")),
        }
    } else {
        // Turn safety OFF: create the override file.
        ensure_parent(path);
        fs::write(path, "safety disabled\n")
            .map_err(|e| format!("Failed to write safety override {path:?}: {e}"))
    }
}

/// Set the persisted safety state (default absolute file path).
pub fn set_safety(enabled: bool) -> Result<(), String> {
    set_safety_at(&safety_override_path(), enabled)
}

/// One-line status string for CLI / GUI display.
pub fn safety_status_string() -> String {
    if is_safety_enabled() {
        "Safety: ON (start interlocks active)".to_string()
    } else {
        "Safety: OFF (start interlocks bypassed)".to_string()
    }
}

// ── Hardware gather (the only part that touches serial) ───────
// The compressor has no per-command port override anywhere, so it uses the
// default port. The LS350 port CAN be overridden on the CLI (`--port`), so it
// is threaded in — see `guard_start_ls350`. Note: `/dev/ttyUSBn` enumeration is
// not guaranteed stable across reboots/replugs; the robust fix for that is
// stable device paths via udev rules (e.g. a SYMLINK+="frost_ls350" keyed on
// the adapter's serial number), which is an OS-config change, not code. Until
// then a mis-enumerated port usually fails the read and — being fail-safe —
// blocks the start.

/// Read whether the compressor is running, in one SMDP round-trip.
/// `None` = could not determine (connection/read failure) → blocks (fail-safe).
fn read_compressor_running() -> Option<bool> {
    CryomechController::default().is_running().ok()
}

/// Read the 4K stage (LS350 input D3) temperature in Kelvin from the given port.
/// `None` = read failed or overrange. Delegates to the LS350 driver's canonical
/// `read_kelvin_checked` so the overrange definition (RDGST? bit 32, garbage
/// frames, zero-on-D-input) lives in exactly one place and this interlock can
/// never drift from the driver's notion of a valid reading.
fn read_4k_stage_kelvin(port: &str, baud: u32) -> Option<f64> {
    LakeShore350Controller::with_port(port.to_string(), baud).read_kelvin_checked("D3")
}

/// Read both interlock inputs over serial and decide. `ls350` overrides the
/// LS350 port/baud when the caller drove a non-default port; `None` = defaults.
fn run_serial_checks(context: &str, ls350: Option<(String, u32)>) -> Result<(), String> {
    let (port, baud) = ls350.unwrap_or_else(|| {
        let d = LakeShore350Controller::default();
        (d.port, d.baud_rate)
    });
    let compressor = read_compressor_running();
    let stage = read_4k_stage_kelvin(&port, baud);
    to_result(context, evaluate_start(compressor, stage))
}

// ── Guard used by start entry points ──────────────────────────
/// Gate a start. Returns `Ok(())` if the process may start, or an
/// `Err(reason)` describing why it is blocked.
///
/// `context` names the operation (e.g. "ADR ramp", "GL7 cooldown") for the
/// message shown to the user. When safety is OFF, this always returns `Ok(())`
/// after logging a loud warning — the override never fails silently.
pub fn guard_start(context: &str) -> Result<(), String> {
    guard_start_ls350_opt(context, None)
}

/// Like `guard_start`, but reads the 4K stage from an explicitly-supplied LS350
/// port/baud (used when the gated CLI command drove a non-default `--port`, so
/// the interlock checks the same instrument being driven).
pub fn guard_start_ls350(context: &str, port: String, baud: u32) -> Result<(), String> {
    guard_start_ls350_opt(context, Some((port, baud)))
}

fn guard_start_ls350_opt(context: &str, ls350: Option<(String, u32)>) -> Result<(), String> {
    // A GUI-spawned subprocess was already gated against the worker's cached
    // snapshot (Option A). Re-reading here would race the worker's poll for the
    // same serial port, so trust the upstream check and skip — but announce it,
    // so a stray FROST_SAFETY_GUI_CHECKED in a terminal can't silently disable
    // the interlocks.
    if std::env::var(GUI_CHECKED_ENV).is_ok() {
        eprintln!(
            "[SAFETY] {context}: skipping serial interlock check ({GUI_CHECKED_ENV} set — \
             GUI already verified). If you did not launch this from the GUI, safety is being \
             bypassed unexpectedly."
        );
        return Ok(());
    }

    if !is_safety_enabled() {
        eprintln!("[SAFETY] Safety OFF — {context} starting with start interlocks bypassed.");
        return Ok(());
    }

    run_serial_checks(context, ls350)
}

/// Gate a start using the GUI worker's cached snapshot (Option A) instead of
/// fresh serial reads — used by GUI-resident start paths. Same override and
/// message contract as `guard_start`.
pub fn guard_start_from_snapshot(
    context: &str,
    compressor_running: bool,
    compressor_age: Option<std::time::Duration>,
    stage_4k_d3: &str,
    temp_age: Option<std::time::Duration>,
    max_age: std::time::Duration,
) -> Result<(), String> {
    if !is_safety_enabled() {
        eprintln!("[SAFETY] Safety OFF — {context} starting with start interlocks bypassed.");
        return Ok(());
    }

    to_result(
        context,
        evaluate_start_from_snapshot(
            compressor_running,
            compressor_age,
            stage_4k_d3,
            temp_age,
            max_age,
        ),
    )
}

/// Format an `evaluate_*` result into the user-facing `guard_*` result. A block
/// (failure) is announced on stderr AND appended to the safety log with a
/// timestamp; passes are neither announced nor logged.
fn to_result(context: &str, decision: Result<(), Vec<Interlock>>) -> Result<(), String> {
    match decision {
        Ok(()) => Ok(()),
        Err(violations) => {
            let reasons = violations
                .iter()
                .map(Interlock::describe)
                .collect::<Vec<_>>()
                .join("; ");
            let msg = format!("{context} blocked by safety interlock: {reasons}");
            eprintln!("[SAFETY] {msg}");
            log_safety_event(&msg);
            Err(format!(
                "{msg}. To override, run `frost safety off` or toggle the GUI Safety button."
            ))
        }
    }
}
