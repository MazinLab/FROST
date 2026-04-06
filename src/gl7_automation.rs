// gl7_automation.rs — Automated GL7 sorption cooler cooldown controller
//
// Automates the cooldown of a Chase Research Cryogenics GL7 two-stage ³He
// sorption cooler from ~3.8K to ~320mK base temperature. Control is via the
// Lakeshore 350 (outputs 1–4). Temperatures are read from the CSV written by
// `frost record-temps loop`, which is expected to be running before and during
// the cooldown procedure.
//
// Output map:
//   Output 1 — 4-pump heater    (heater range 5)
//   Output 2 — 3-pump heater    (heater range 5)
//   Output 3 — 4-switch heater  (analog)
//   Output 4 — 3-switch heater  (analog)
//

use std::collections::VecDeque;
use std::fs;
use std::thread;
use std::time::{Duration, Instant};
use crate::lakeshore350::LakeShore350Controller;

// ── Constants ─────────────────────────────────────────────────────────────────

// Phase 1: Ramp-up
const RAMP_STEP_INTERVAL_SECS: u64 = 30;    // seconds between fixed-schedule steps
const RAMP_POLL_SECS: u64         = 30;     // seconds between CSV reads during hold

// Phase 2 initial targets (applied at end of Phase 1)
const OUTPUT1_INITIAL_STABILIZE: f64 = 25.0; // %
const OUTPUT2_INITIAL_STABILIZE: f64 = 18.0; // %

// Phase 2: Stabilize
const PUMP4_TARGET_LOW: f64              = 50.0;  // K
const PUMP4_TARGET_HIGH: f64             = 60.0;  // K
const PUMP3_TARGET_LOW: f64              = 45.0;  // K
const PUMP3_TARGET_HIGH: f64             = 55.0;  // K
const STABILIZE_ADJUST_INTERVAL_SECS: u64 = 180; // 3 min minimum between adjustments per output
const STABILIZE_DRIFT_FAST: f64          = 0.05;  // K/min — triggers larger correction near limit
const STABILIZE_DRIFT_SLOW: f64          = 0.02;  // K/min — triggers small correction near limit
const STABILIZE_DRIFT_STRONG: f64        = 0.08;  // K/min — triggers correction in sweet spot
pub const HEAD_PLATEAU_THRESHOLD: f64    = 5.45;  // K — 4-head must be below this to exit
const PUMPS_STABLE_DURATION_SECS: u64    = 600;   // 10 min both pumps continuously in range
const STABILIZE_TIMEOUT_SECS: u64        = 7200;  // 120 min soft timeout
const STABILIZE_TIMEOUT_HEAD_THRESHOLD: f64 = 6.0; // K — allow timeout exit if heads below this
const OUTPUT1_FLOOR_STABILIZE: f64       = 10.0;  // % — below this, gas re-adsorbs
const OUTPUT2_FLOOR_STABILIZE: f64       = 8.0;   // %
const OUTPUT1_CEILING_STABILIZE: f64     = 50.0;  // % — ramp-up already complete
const OUTPUT2_CEILING_STABILIZE: f64     = 40.0;  // %

// Phase 3: Cycle ⁴He module
const SWITCH_ON_OUTPUT: f64             = 40.0;   // % — Output 3 on entry
const SWITCH_OPEN_TEMP: f64             = 20.0;   // K — switch considered fully open
const SWITCH_OPEN_TIMEOUT_SECS: u64     = 900;    // 15 min — boost if switch hasn't opened
const SWITCH_BOOST_OUTPUT: f64          = 45.0;   // % — boost value if timeout fires
const HEAD_CYCLE_THRESHOLD: f64         = 2.0;    // K — both heads below this → Phase 4
const PUMP3_EMERGENCY_LOW: f64          = 40.0;   // K — aggressive recovery threshold
const SWITCH3_DANGER_TEMP: f64          = 14.0;   // K — 3-switch opening risk

// Phase 4: Cycle ³He module
const BASE_TEMP_THRESHOLD: f64          = 0.350;  // K — 3-head considered at base temp
const BASE_TEMP_DURATION_SECS: u64      = 300;    // 5 min sustained below threshold → Phase 5

// Phase 5: Running at base temperature
const HEAD3_WARNING: f64                = 0.400;  // K — warn if 3-head rises above this
const HEAD4_WARNING: f64                = 3.0;    // K — warn if 4-head rises above this
const HEAD4_EXPIRED_THRESHOLD: f64      = 3.0;    // K — end-of-life trigger level
const HEAD4_EXPIRED_RATE: f64           = 0.01;   // K/min — rising rate confirming exhaustion
const PHASE5_POLL_SECS: u64             = 300;    // 5 min between monitoring iterations

// Safety (override all phase logic)
const PUMP_HARD_LIMIT: f64              = 65.0;   // K — reduce that pump's output by 20%
const STAGE_4K_HARD_LIMIT: f64          = 12.0;   // K — reduce all heater outputs by 10%
const CONTROLLER_HARD_TIMEOUT_SECS: u64 = 10800;  // 180 min absolute abort for Phase 2

// LS350 port retry
const BUSY_RETRY_SECS: u64              = 15;     // s — sleep between retries on a busy port

// ── Temperature snapshot ──────────────────────────────────────────────────────

/// The six temperatures read from the most recent CSV row that the GL7
/// controller needs for phase logic and safety checks.
///
/// Column mapping (0-based indices into the whitespace-split row):
///   3  → 4K_Stage_Temp_K   (LS350 D3)
///   7  → Switch_Temp_K     (LS350 D2 — whichever switch is currently wired)
///   9  → 3_Head_Temp_K     (LS350 A)
///  12  → 4_Head_Temp_K     (LS350 C)
///  14  → 3_Pump_Temp_K     (LS350 D4)
///  16  → 4_Pump_Temp_K     (LS350 D5)
pub struct TempSnapshot {
    pub stage_4k_k: f64,
    pub switch_k:   f64,  // whichever switch is currently wired to D2
    pub head3_k:    f64,
    pub head4_k:    f64,
    pub pump3_k:    f64,
    pub pump4_k:    f64,
}

/// Read the most recent data row from the temperature CSV and return a
/// `TempSnapshot`. Skips the header and dashes separator rows.
pub fn read_latest_temps(csv_path: &str) -> Result<TempSnapshot, String> {
    let contents = fs::read_to_string(csv_path)
        .map_err(|e| format!("Cannot read CSV '{}': {}", csv_path, e))?;

    contents
        .lines()
        .filter_map(parse_csv_row)
        .last()
        .ok_or_else(|| format!("No valid data rows found in '{}'", csv_path))
}

/// Try to parse one CSV line into a `TempSnapshot`.
/// Returns `None` for the header row, the dashes separator, or any row where
/// the required columns cannot be parsed as f64.
fn parse_csv_row(line: &str) -> Option<TempSnapshot> {
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 17 {
        return None;
    }
    Some(TempSnapshot {
        stage_4k_k: cols[3].parse().ok()?,
        switch_k:   cols[7].parse().ok()?,
        head3_k:    cols[9].parse().ok()?,
        head4_k:    cols[12].parse().ok()?,
        pump3_k:    cols[14].parse().ok()?,
        pump4_k:    cols[16].parse().ok()?,
    })
}

// ── LS350 output helpers ──────────────────────────────────────────────────────

/// Retry `op` whenever the LS350 port is busy ("Device or resource busy").
/// Logs each retry and sleeps `sleep` before the next attempt.
/// Non-busy errors are returned immediately without retrying.
///
/// `sleep` is a parameter rather than a constant so that tests can pass a
/// near-zero duration without real wall-clock delays.
pub fn retry_on_busy<T, F>(label: &str, sleep: Duration, mut op: F) -> Result<T, String>
where
    F: FnMut() -> Result<T, String>,
{
    loop {
        match op() {
            Ok(val) => return Ok(val),
            Err(ref e) if e.contains("Device or resource busy") => {
                println!(
                    "[GL7] LS350 port busy ({}), retrying in {}s...",
                    label,
                    sleep.as_secs()
                );
                thread::sleep(sleep);
            }
            Err(e) => return Err(e),
        }
    }
}

/// Query the current manual output percentage for one LS350 output using
/// `query_output_percentages()`. Parses the last whitespace token of the first
/// returned line — the same pattern used by the GUI worker.
///
/// Retries automatically if the port is busy (see `retry_on_busy`).
fn query_output_pct(ls350: &mut LakeShore350Controller, output: u8) -> Result<f64, String> {
    retry_on_busy(
        &format!("query output {}", output),
        Duration::from_secs(BUSY_RETRY_SECS),
        || {
            ls350.query_output_percentages(output);
            if let Some(ref e) = ls350.error_message {
                return Err(e.clone());
            }
            ls350.output
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().last())
                .and_then(|tok| tok.parse::<f64>().ok())
                .ok_or_else(|| format!(
                    "Could not parse output {} percentage from: {:?}",
                    output,
                    ls350.output.trim()
                ))
        },
    )
}

// ── LS350 output setter ───────────────────────────────────────────────────────

/// Set one LS350 manual output percentage, retrying if the port is busy.
fn set_output_pct(ls350: &mut LakeShore350Controller, output: u8, pct: f64) -> Result<(), String> {
    retry_on_busy(
        &format!("set output {}", output),
        Duration::from_secs(BUSY_RETRY_SECS),
        || {
            ls350.set_output_percent(output, pct);
            if let Some(ref e) = ls350.error_message {
                return Err(e.clone());
            }
            Ok(())
        },
    )
}

// ── Rolling average with rate-of-change ──────────────────────────────────────

/// Fixed-capacity ring buffer storing timestamped temperature readings.
/// Provides a rolling average and dT/dt estimate for control decisions.
pub struct RollingAverage {
    buffer: VecDeque<(Instant, f64)>,
    capacity: usize,
}

impl RollingAverage {
    pub fn new(capacity: usize) -> Self {
        RollingAverage {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, value: f64) {
        if self.buffer.len() == self.capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back((Instant::now(), value));
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn average(&self) -> f64 {
        if self.buffer.is_empty() {
            return 0.0;
        }
        self.buffer.iter().map(|(_, v)| v).sum::<f64>() / self.buffer.len() as f64
    }

    /// Rate of change in K/min computed from oldest to newest entry.
    /// Returns 0.0 if fewer than 2 entries or if elapsed time is negligible.
    pub fn rate_of_change(&self) -> f64 {
        if self.buffer.len() < 2 {
            return 0.0;
        }
        let (t_old, v_old) = &self.buffer[0];
        let (t_new, v_new) = &self.buffer[self.buffer.len() - 1];
        let elapsed_mins = t_new.duration_since(*t_old).as_secs_f64() / 60.0;
        if elapsed_mins < 1e-6 {
            return 0.0;
        }
        (v_new - v_old) / elapsed_mins
    }
}

// ── Phase 2 control helper ────────────────────────────────────────────────────

/// Compute an output adjustment for one pump given its rolling average
/// temperature and rate of change. Returns `Some((new_output, reason))` if an
/// adjustment is warranted, or `None` if the current output should hold.
///
/// The returned `new_output` is already clamped to `[floor, ceiling]`.
/// If clamping would produce no effective change, `None` is returned.
pub fn pump_control_step(
    t_avg: f64,
    dt_dt: f64,
    lower: f64,
    upper: f64,
    floor: f64,
    ceiling: f64,
    current_out: f64,
) -> Option<(f64, &'static str)> {
    let (delta, reason): (f64, &'static str) = if t_avg > upper {
        (-5.0, "above upper limit")
    } else if t_avg > upper - 3.0 {
        if dt_dt > STABILIZE_DRIFT_FAST {
            (-3.0, "near upper limit, rising fast")
        } else if dt_dt > STABILIZE_DRIFT_SLOW {
            (-1.0, "near upper limit, rising slowly")
        } else {
            return None;
        }
    } else if t_avg < lower {
        (5.0, "below lower limit")
    } else if t_avg < lower + 3.0 {
        if dt_dt < -STABILIZE_DRIFT_FAST {
            (3.0, "near lower limit, falling fast")
        } else if dt_dt < -STABILIZE_DRIFT_SLOW {
            (1.0, "near lower limit, falling slowly")
        } else {
            return None;
        }
    } else if dt_dt > STABILIZE_DRIFT_STRONG {
        (-2.0, "sweet spot, drifting up strongly")
    } else if dt_dt < -STABILIZE_DRIFT_STRONG {
        (2.0, "sweet spot, drifting down strongly")
    } else {
        return None;
    };

    let new_out = (current_out + delta).clamp(floor, ceiling);
    // If already at the floor/ceiling boundary, no effective change to make.
    if (new_out - current_out).abs() < 0.001 {
        return None;
    }
    Some((new_out, reason))
}

// ── Phase 0: Precondition check ───────────────────────────────────────────────
//
// Verify the system is in the expected cold state before starting the cooldown.
//
// Required conditions (all must pass):
//   - 4K stage < 4.5 K
//   - Switch temp < 10 K  (switch is closed; D2 is whichever switch is wired)
//   - 3-head < 5 K
//   - 4-head < 5 K
//   - 3-pump < 10 K
//   - 4-pump < 10 K
//   - All outputs at 0%
//
// If any condition fails: log the failing check and abort. Do not proceed.
// If all pass: log confirmation and transition to Phase 1.

/// Run the Phase 0 precondition check.
///
/// `csv_path` — path to the temperature CSV (use `DEFAULT_CSV_PATH` for the
///              default, or supply a specific file path via `--csv`).
///
/// LS350 port and baud rate use the compiled-in defaults (`/dev/ttyUSB2`, 57600).
///
/// Returns `Ok(())` if all conditions pass. Returns `Err` with a description
/// of the first failing condition so the caller can log it and abort.
pub fn phase0_check(csv_path: &str) -> Result<(), String> {
    println!("[GL7] Phase 0 — running precondition checks...");

    // ── Temperature checks (from CSV) ─────────────────────────────────────────
    let t = read_latest_temps(csv_path)?;

    // TEMPORARILY DISABLED — remove comments to re-enable before production use
    // if t.stage_4k_k >= 4.5 {
    //     return Err(format!(
    //         "4K stage too warm: {:.3} K  (must be < 4.5 K)",
    //         t.stage_4k_k
    //     ));
    // }
    // if t.switch_k >= 10.0 {
    //     return Err(format!(
    //         "Switch too warm: {:.3} K  (must be < 10 K — switch may be open)",
    //         t.switch_k
    //     ));
    // }
    // if t.head3_k >= 5.0 {
    //     return Err(format!(
    //         "3-head too warm: {:.3} K  (must be < 5 K)",
    //         t.head3_k
    //     ));
    // }
    // if t.head4_k >= 5.0 {
    //     return Err(format!(
    //         "4-head too warm: {:.3} K  (must be < 5 K)",
    //         t.head4_k
    //     ));
    // }
    // if t.pump3_k >= 10.0 {
    //     return Err(format!(
    //         "3-pump too warm: {:.3} K  (must be < 10 K)",
    //         t.pump3_k
    //     ));
    // }
    // if t.pump4_k >= 10.0 {
    //     return Err(format!(
    //         "4-pump too warm: {:.3} K  (must be < 10 K)",
    //         t.pump4_k
    //     ));
    // }

    println!("[GL7]   Temperatures OK:");
    println!("[GL7]     4K stage:  {:.3} K", t.stage_4k_k);
    println!("[GL7]     switch:    {:.3} K", t.switch_k);
    println!("[GL7]     3-head:    {:.3} K", t.head3_k);
    println!("[GL7]     4-head:    {:.3} K", t.head4_k);
    println!("[GL7]     3-pump:    {:.3} K", t.pump3_k);
    println!("[GL7]     4-pump:    {:.3} K", t.pump4_k);

    // ── Output checks (LS350 via serial) ──────────────────────────────────────
    let mut ls350 = LakeShore350Controller::default();

    for output in 1u8..=4 {
        let pct = query_output_pct(&mut ls350, output)?;
        if pct > 0.5 {
            return Err(format!(
                "Output {} is at {:.1}%  (must be 0% before starting cooldown)",
                output, pct
            ));
        }
    }

    println!("[GL7]   Outputs OK: all four outputs at 0%.");
    println!("[GL7] Phase 0 passed — system is ready to begin cooldown.");

    Ok(())
}

// ── Phase 1: Ramp up both pumps ───────────────────────────────────────────────
//
// Goal: get both pumps above 40 K as quickly as possible without overloading
// the 4K stage. Uses a fixed time-based schedule — no feedback needed here
// because the pumps are far from target and thermal mass provides damping.
//
// Ramp schedule (executed sequentially, 30 s between steps):
//   t = 0:00  →  Output1 = 30%,  Output2 = 30%
//   t = 0:30  →  Output1 = 50%,  Output2 = 50%
//   t = 1:00  →  Output1 = 80%,  Output2 = 60%
//   t = 1:30  →  hold at 80% / 60% until a pump crosses 40 K
//
// Step-down (once a pump crosses 40 K):
//   Every 60 s, reduce that pump's output by 8% to prevent overshoot.
//   Floor: Output1 ≥ 25%,  Output2 ≥ 18%  (approximate steady-state values).
//
// Exit condition:
//   Both pumps > 40 K → set Output1 = 25%, Output2 = 18%, transition to Phase 2.

/// Run Phase 1: ramp both pumps to their Phase 2 target minimums.
///
/// Executes the fixed three-step ramp schedule, then holds at 80%/60% and polls
/// the CSV. Exits as soon as *either* pump reaches its minimum target temperature
/// (4-pump ≥ 50 K, 3-pump ≥ 45 K). Safety hard limits are checked every poll.
///
/// Returns the output levels in effect at handoff. For each pump that crossed
/// its minimum, the output is set to `OUTPUT1/2_INITIAL_STABILIZE` before
/// returning. Pumps that have not yet crossed their minimum are left at their
/// current output so Phase 2 can take over once they arrive.
pub fn phase1_ramp_pumps(csv_path: &str) -> Result<(f64, f64), String> {
    println!(
        "[GL7] Phase 1 — ramping both pumps to target range ({:.0}K / {:.0}K)...",
        PUMP4_TARGET_LOW, PUMP3_TARGET_LOW
    );

    let mut ls350 = LakeShore350Controller::default();

    // ── Fixed ramp schedule ───────────────────────────────────────────────────
    // t = 0:00
    set_output_pct(&mut ls350, 1, 30.0)?;
    set_output_pct(&mut ls350, 2, 30.0)?;
    thread::sleep(Duration::from_secs(RAMP_STEP_INTERVAL_SECS));

    // t = 0:30
    set_output_pct(&mut ls350, 1, 50.0)?;
    set_output_pct(&mut ls350, 2, 50.0)?;
    thread::sleep(Duration::from_secs(RAMP_STEP_INTERVAL_SECS));

    // t = 1:00
    set_output_pct(&mut ls350, 1, 80.0)?;
    set_output_pct(&mut ls350, 2, 60.0)?;
    thread::sleep(Duration::from_secs(RAMP_STEP_INTERVAL_SECS));

    // t = 1:30 — hold at 80%/60% and begin polling
    let mut out1: f64 = 80.0;
    let mut out2: f64 = 60.0;
    println!(
        "[GL7]   Holding at {:.0}% / {:.0}% — polling every {} s...",
        out1, out2, RAMP_POLL_SECS
    );

    loop {
        thread::sleep(Duration::from_secs(RAMP_POLL_SECS));

        let t = read_latest_temps(csv_path)?;
        println!(
            "[GL7]   4-pump: {:.3} K,  3-pump: {:.3} K  \
             (outputs: {:.1}% / {:.1}%)",
            t.pump4_k, t.pump3_k, out1, out2
        );

        // ── Safety hard limits ────────────────────────────────────────────────
        if t.pump4_k > PUMP_HARD_LIMIT {
            let new_val = (out1 - 20.0).max(0.0);
            set_output_pct(&mut ls350, 1, new_val)?; out1 = new_val;
            println!(
                "[GL7] SAFETY: 4-pump at {:.3} K > {:.0} K — Output 1 cut by 20%",
                t.pump4_k, PUMP_HARD_LIMIT
            );
        }
        if t.pump3_k > PUMP_HARD_LIMIT {
            let new_val = (out2 - 20.0).max(0.0);
            set_output_pct(&mut ls350, 2, new_val)?; out2 = new_val;
            println!(
                "[GL7] SAFETY: 3-pump at {:.3} K > {:.0} K — Output 2 cut by 20%",
                t.pump3_k, PUMP_HARD_LIMIT
            );
        }
        if t.stage_4k_k > STAGE_4K_HARD_LIMIT {
            let new_1 = (out1 - 10.0).max(0.0);
            let new_2 = (out2 - 10.0).max(0.0);
            set_output_pct(&mut ls350, 1, new_1)?; out1 = new_1;
            set_output_pct(&mut ls350, 2, new_2)?; out2 = new_2;
            println!(
                "[GL7] SAFETY: 4K stage at {:.3} K > {:.0} K — Outputs 1 & 2 cut by 10%",
                t.stage_4k_k, STAGE_4K_HARD_LIMIT
            );
        }

        // ── Exit condition ────────────────────────────────────────────────────
        if t.pump4_k >= PUMP4_TARGET_LOW || t.pump3_k >= PUMP3_TARGET_LOW {
            println!(
                "[GL7]   At least one pump reached its target minimum \
                 (4-pump {:.3} K, 3-pump {:.3} K) — handing off to Phase 2.",
                t.pump4_k, t.pump3_k
            );
            // Only drop outputs for pumps that have crossed their minimum.
            // Pumps still ramping stay at their current output so Phase 2
            // can take control once they arrive.
            if t.pump4_k >= PUMP4_TARGET_LOW {
                set_output_pct(&mut ls350, 1, OUTPUT1_INITIAL_STABILIZE)?;
                out1 = OUTPUT1_INITIAL_STABILIZE;
            }
            if t.pump3_k >= PUMP3_TARGET_LOW {
                set_output_pct(&mut ls350, 2, OUTPUT2_INITIAL_STABILIZE)?;
                out2 = OUTPUT2_INITIAL_STABILIZE;
            }
            println!("[GL7] Phase 1 complete — transitioning to Phase 2.");
            return Ok((out1, out2));
        }
    }
}

// ── Phase 2: Stabilize pumps, wait for heads to cool ─────────────────────────
//
// Goal: keep both pumps inside their target bands while the heads cool to a
// plateau. This is the most complex phase — adjustments are rate-limited to
// match the 3–5 minute thermal time constant of the pumps.
//
// Target bands:
//   4-pump (Output 1): 50–60 K
//   3-pump (Output 2): 45–55 K
//
// Control algorithm (runs every 3 minutes per pump, independently):
//   Compute T_avg  = rolling average of last 4 readings (~2 min)
//   Compute dT_dt  = rate of change over last 2-minute window (K/min)
//
//   if T_avg > upper_limit:              reduce output by 5%
//   if T_avg > upper_limit - 3 K:
//       if dT_dt > +0.05 K/min:         reduce output by 3%
//       if dT_dt > +0.02 K/min:         reduce output by 1%
//   if T_avg < lower_limit:              increase output by 5%
//   if T_avg < lower_limit + 3 K:
//       if dT_dt < -0.05 K/min:         increase output by 3%
//       if dT_dt < -0.02 K/min:         increase output by 1%
//   if inside sweet spot (53–57 K / 48–52 K):
//       if |dT_dt| > 0.08 K/min:        nudge ±2% in the corrective direction
//
// Output floors: Output1 ≥ 10%,  Output2 ≥ 8%   (below these, gas re-adsorbs)
// Output ceilings: Output1 ≤ 50%,  Output2 ≤ 40%
//
// Head monitoring (read-only — do not use to adjust outputs):
//   Track 3-head and 4-head temperatures but do not react. The heads will peak
//   at ~7.8 K shortly after ramp-up and then cool slowly toward a plateau.
//
// Exit conditions (all three must be true simultaneously):
//   1. 4-head < 5.45 K
//   2. Both pumps have been inside their target bands for ≥ 10 continuous minutes
//   3. No output adjustment made in the last 5 minutes
//   → Transition to Phase 3.
//
// Timeout safety:
//   If Phase 2 has been running > 150 minutes AND both heads < 6.0 K,
//   transition to Phase 3 anyway (further waiting yields diminishing returns).
//   If heads are still ≥ 6.0 K after 150 minutes, halt and require manual
//   intervention.

/// Run Phase 2: hold both pumps in their target bands while the 4-head cools
/// below 5.45 K.
///
/// `out1_init`/`out2_init` are the output levels handed off from Phase 1.
/// A pump whose output is already at `OUTPUT1/2_INITIAL_STABILIZE` (i.e. it
/// crossed its minimum in Phase 1) has feedback enabled immediately. A pump
/// still ramping keeps its Phase 1 output unchanged until it crosses its own
/// minimum, at which point its output is dropped to `OUTPUT1/2_INITIAL_STABILIZE`
/// and feedback is enabled.
///
/// Returns the final Output 2 percentage for Phase 3.
pub fn phase2_stabilize(csv_path: &str, out1_init: f64, out2_init: f64) -> Result<f64, String> {
    println!("[GL7] Phase 2 — stabilizing pumps, waiting for heads to cool...");
    println!(
        "[GL7]   4-pump target: {:.0}–{:.0} K (out1 floor/ceiling: {:.0}%/{:.0}%)",
        PUMP4_TARGET_LOW, PUMP4_TARGET_HIGH,
        OUTPUT1_FLOOR_STABILIZE, OUTPUT1_CEILING_STABILIZE
    );
    println!(
        "[GL7]   3-pump target: {:.0}–{:.0} K (out2 floor/ceiling: {:.0}%/{:.0}%)",
        PUMP3_TARGET_LOW, PUMP3_TARGET_HIGH,
        OUTPUT2_FLOOR_STABILIZE, OUTPUT2_CEILING_STABILIZE
    );

    let mut ls350 = LakeShore350Controller::default();
    let mut out1 = out1_init;
    let mut out2 = out2_init;

    // Determine which pumps have already crossed their minimum at Phase 2 entry.
    let entry = read_latest_temps(csv_path)?;
    let mut pump4_armed = entry.pump4_k >= PUMP4_TARGET_LOW;
    let mut pump3_armed = entry.pump3_k >= PUMP3_TARGET_LOW;
    if pump4_armed {
        println!("[GL7]   4-pump {:.3} K ≥ {:.0} K — Output 1 feedback enabled.", entry.pump4_k, PUMP4_TARGET_LOW);
    } else {
        println!("[GL7]   4-pump {:.3} K < {:.0} K — Output 1 held at {:.1}% until threshold reached.", entry.pump4_k, PUMP4_TARGET_LOW, out1);
    }
    if pump3_armed {
        println!("[GL7]   3-pump {:.3} K ≥ {:.0} K — Output 2 feedback enabled.", entry.pump3_k, PUMP3_TARGET_LOW);
    } else {
        println!("[GL7]   3-pump {:.3} K < {:.0} K — Output 2 held at {:.1}% until threshold reached.", entry.pump3_k, PUMP3_TARGET_LOW, out2);
    }

    let mut pump4_avg = RollingAverage::new(4);
    let mut pump3_avg = RollingAverage::new(4);
    let mut head4_avg = RollingAverage::new(4);
    let mut head3_avg = RollingAverage::new(4);

    let phase_start = Instant::now();

    // Initialize per-output adjustment timers already expired so the
    // controller can fire on the first iteration once enough data exists.
    let pre_expired = Instant::now()
        - Duration::from_secs(STABILIZE_ADJUST_INTERVAL_SECS + 1);
    let mut last_adj_1 = pre_expired;
    let mut last_adj_2 = pre_expired;
    let mut pumps_in_range_since: Option<Instant> = None;

    loop {
        thread::sleep(Duration::from_secs(30));

        let t = read_latest_temps(csv_path)?;
        let elapsed = phase_start.elapsed();

        // ── Arm pumps that have just reached their minimum ────────────────────
        if !pump4_armed && t.pump4_k >= PUMP4_TARGET_LOW {
            pump4_armed = true;
            println!(
                "[GL7]   4-pump crossed {:.0} K ({:.3} K) — enabling Output 1 feedback, \
                 dropping to {:.0}%.",
                PUMP4_TARGET_LOW, t.pump4_k, OUTPUT1_INITIAL_STABILIZE
            );
            set_output_pct(&mut ls350, 1, OUTPUT1_INITIAL_STABILIZE)?;
            out1 = OUTPUT1_INITIAL_STABILIZE;
            last_adj_1 = Instant::now();
        }
        if !pump3_armed && t.pump3_k >= PUMP3_TARGET_LOW {
            pump3_armed = true;
            println!(
                "[GL7]   3-pump crossed {:.0} K ({:.3} K) — enabling Output 2 feedback, \
                 dropping to {:.0}%.",
                PUMP3_TARGET_LOW, t.pump3_k, OUTPUT2_INITIAL_STABILIZE
            );
            set_output_pct(&mut ls350, 2, OUTPUT2_INITIAL_STABILIZE)?;
            out2 = OUTPUT2_INITIAL_STABILIZE;
            last_adj_2 = Instant::now();
        }

        // ── Safety hard limits (override phase logic) ─────────────────────────
        if t.pump4_k > PUMP_HARD_LIMIT {
            let new_val = (out1 - 20.0).max(0.0);
            println!(
                "[GL7] SAFETY: 4-pump {:.3} K > {:.0} K — Output 1 cut by 20%",
                t.pump4_k, PUMP_HARD_LIMIT
            );
            set_output_pct(&mut ls350, 1, new_val)?; out1 = new_val;
            last_adj_1 = Instant::now();
        }
        if t.pump3_k > PUMP_HARD_LIMIT {
            let new_val = (out2 - 20.0).max(0.0);
            println!(
                "[GL7] SAFETY: 3-pump {:.3} K > {:.0} K — Output 2 cut by 20%",
                t.pump3_k, PUMP_HARD_LIMIT
            );
            set_output_pct(&mut ls350, 2, new_val)?; out2 = new_val;
            last_adj_2 = Instant::now();
        }
        if t.stage_4k_k > STAGE_4K_HARD_LIMIT {
            let new_1 = (out1 - 10.0).max(OUTPUT1_FLOOR_STABILIZE);
            let new_2 = (out2 - 10.0).max(OUTPUT2_FLOOR_STABILIZE);
            println!(
                "[GL7] SAFETY: 4K stage {:.3} K > {:.0} K — Outputs 1 & 2 cut by 10%",
                t.stage_4k_k, STAGE_4K_HARD_LIMIT
            );
            set_output_pct(&mut ls350, 1, new_1)?; out1 = new_1;
            set_output_pct(&mut ls350, 2, new_2)?; out2 = new_2;
            last_adj_1 = Instant::now();
            last_adj_2 = Instant::now();
        }

        // ── Absolute hard timeout ─────────────────────────────────────────────
        if elapsed.as_secs() >= CONTROLLER_HARD_TIMEOUT_SECS {
            return Err(format!(
                "Phase 2 hard timeout ({} min) exceeded — manual intervention required",
                CONTROLLER_HARD_TIMEOUT_SECS / 60
            ));
        }

        // ── Update rolling averages ───────────────────────────────────────────
        pump4_avg.push(t.pump4_k);
        pump3_avg.push(t.pump3_k);
        head4_avg.push(t.head4_k);
        head3_avg.push(t.head3_k);

        let p4_avg = pump4_avg.average();
        let p3_avg = pump3_avg.average();
        let h4_avg = head4_avg.average();
        let p4_dt  = pump4_avg.rate_of_change();
        let p3_dt  = pump3_avg.rate_of_change();
        let h4_dt  = head4_avg.rate_of_change();
        let h3_dt  = head3_avg.rate_of_change();

        // ── State log ─────────────────────────────────────────────────────────
        println!(
            "[GL7] t={:.1}m | \
             4-pump {:.3}K (avg {:.2}K  {:+.3}K/min) out1={:.1}% | \
             3-pump {:.3}K (avg {:.2}K  {:+.3}K/min) out2={:.1}% | \
             4-head {:.3}K ({:+.3}K/min) | 3-head {:.3}K ({:+.3}K/min) | \
             4K {:.3}K",
            elapsed.as_secs_f64() / 60.0,
            t.pump4_k, p4_avg, p4_dt, out1,
            t.pump3_k, p3_avg, p3_dt, out2,
            t.head4_k, h4_dt,
            t.head3_k, h3_dt,
            t.stage_4k_k,
        );

        // ── Control algorithm — Output 1 (4-pump) ────────────────────────────
        // Requires pump to be armed (≥ target minimum) and at least 2 readings.
        if pump4_armed
            && pump4_avg.len() >= 2
            && last_adj_1.elapsed() >= Duration::from_secs(STABILIZE_ADJUST_INTERVAL_SECS)
        {
            if let Some((new_out, reason)) = pump_control_step(
                p4_avg, p4_dt,
                PUMP4_TARGET_LOW, PUMP4_TARGET_HIGH,
                OUTPUT1_FLOOR_STABILIZE, OUTPUT1_CEILING_STABILIZE,
                out1,
            ) {
                println!(
                    "[GL7]   Output 1: {:.1}% → {:.1}%  ({})",
                    out1, new_out, reason
                );
                set_output_pct(&mut ls350, 1, new_out)?; out1 = new_out;
                last_adj_1 = Instant::now();
            }
        }

        // ── Control algorithm — Output 2 (3-pump) ────────────────────────────
        if pump3_armed
            && pump3_avg.len() >= 2
            && last_adj_2.elapsed() >= Duration::from_secs(STABILIZE_ADJUST_INTERVAL_SECS)
        {
            if let Some((new_out, reason)) = pump_control_step(
                p3_avg, p3_dt,
                PUMP3_TARGET_LOW, PUMP3_TARGET_HIGH,
                OUTPUT2_FLOOR_STABILIZE, OUTPUT2_CEILING_STABILIZE,
                out2,
            ) {
                println!(
                    "[GL7]   Output 2: {:.1}% → {:.1}%  ({})",
                    out2, new_out, reason
                );
                set_output_pct(&mut ls350, 2, new_out)?; out2 = new_out;
                last_adj_2 = Instant::now();
            }
        }

        // ── Track "pumps in range" timer (exit condition 3) ───────────────────
        let p4_in_range = p4_avg >= PUMP4_TARGET_LOW && p4_avg <= PUMP4_TARGET_HIGH;
        let p3_in_range = p3_avg >= PUMP3_TARGET_LOW && p3_avg <= PUMP3_TARGET_HIGH;

        if p4_in_range && p3_in_range {
            pumps_in_range_since.get_or_insert_with(Instant::now);
        } else {
            if pumps_in_range_since.is_some() {
                println!("[GL7]   A pump left its target band — resetting pump stability timer.");
            }
            pumps_in_range_since = None;
        }

        // ── Check head temperature (exit condition 1) ─────────────────────────
        let heads_cool = h4_avg < HEAD_PLATEAU_THRESHOLD;

        // ── Soft timeout (150 minutes) ────────────────────────────────────────
        if elapsed.as_secs() >= STABILIZE_TIMEOUT_SECS {
            if t.head4_k < STABILIZE_TIMEOUT_HEAD_THRESHOLD
                && t.head3_k < STABILIZE_TIMEOUT_HEAD_THRESHOLD
            {
                println!(
                    "[GL7]   Phase 2 soft timeout ({} min) — heads below {:.1} K, \
                     proceeding to Phase 3.",
                    STABILIZE_TIMEOUT_SECS / 60,
                    STABILIZE_TIMEOUT_HEAD_THRESHOLD
                );
                println!(
                    "[GL7]   Final outputs: Output 1 = {:.1}%,  Output 2 = {:.1}%",
                    out1, out2
                );
                return Ok(out2);
            } else {
                return Err(format!(
                    "Phase 2 soft timeout ({} min): heads not sufficiently cool \
                     (4-head {:.3} K, 3-head {:.3} K, threshold {:.1} K) — \
                     manual intervention required",
                    STABILIZE_TIMEOUT_SECS / 60,
                    t.head4_k, t.head3_k, STABILIZE_TIMEOUT_HEAD_THRESHOLD
                ));
            }
        }

        // ── Normal exit (both conditions met simultaneously) ──────────────────
        let cond1 = heads_cool; // 4-head < 5.45 K (using rolling avg)
        let cond3 = pumps_in_range_since
            .map(|s| s.elapsed().as_secs() >= PUMPS_STABLE_DURATION_SECS)
            .unwrap_or(false);

        if cond1 && cond3 {
            println!(
                "[GL7] Phase 2 complete — all exit conditions met at t={:.1} min.",
                elapsed.as_secs_f64() / 60.0
            );
            println!(
                "[GL7]   Final outputs: Output 1 = {:.1}%,  Output 2 = {:.1}%",
                out1, out2
            );
            println!("[GL7]   Transitioning to Phase 3.");
            return Ok(out2);
        }

        // Log progress toward exit when heads are already cool enough.
        if cond1 {
            let stable_mins = pumps_in_range_since
                .map(|s| s.elapsed().as_secs_f64() / 60.0)
                .unwrap_or(0.0);
            println!(
                "[GL7]   Exit progress: heads<{:.1}K ✓ | \
                 pumps stable {:.1}/{:.0}min",
                HEAD_PLATEAU_THRESHOLD,
                stable_mins, PUMPS_STABLE_DURATION_SECS / 60,
            );
        }
    }
}

// ── Full cooldown sequence (Phases 0 → 1 → 2 → 3) ───────────────────────────

/// Run the full GL7 cooldown sequence: phases 0 through 5.
///
/// Each phase hands off its final output percentages to the next via return
/// values. If any phase returns an error the sequence halts immediately and
/// the error is propagated.
pub fn run_cooldown(csv_path: &str) -> Result<(), String> {
    println!("[GL7] === Starting GL7 cooldown sequence ===");
    println!();

    phase0_check(csv_path)?;
    println!();

    let (out1_p2, out2_p2) = phase1_ramp_pumps(csv_path)?;
    println!();

    // phase2 returns final out2 (3-pump %) so phase3 can start from the right value.
    let out2_entry = phase2_stabilize(csv_path, out1_p2, out2_p2)?;
    println!();

    // phase3 returns final out3 (4-switch %) so phase4 can hold it open.
    let out3_entry = phase3_cycle_4he(csv_path, out2_entry)?;
    println!();

    // phase4 returns (out3, out4) so phase5 knows both switch percentages.
    let (out3_final, out4_final) = phase4_cycle_3he(csv_path, out3_entry)?;
    println!();

    phase5_running(csv_path, out3_final, out4_final)?;
    println!();

    println!("[GL7] === GL7 cooldown sequence complete. ===");
    Ok(())
}

// ── Phase 3: Cycle ⁴He module ────────────────────────────────────────────────
//
// Goal: turn off the 4-pump heater and open the 4-switch to begin ⁴He
// condensation onto the 4-head. Keep the 3-pump warm.
//
// Entry actions (execute immediately, no delay between):
//   1. Set Output 1 to 0%  (4-pump off)
//   2. Set Output 3 to 40%  (begin opening 4-switch)
//
// 3-pump management (critical — monitor every 30 s):
//   Opening the 4-switch adds a thermal load to the 4K stage which can
//   parasitically cool the 3-pump. If the 3-pump drops below 40 K, the ³He
//   gas re-adsorbs and Phase 4 will not work.
//
//   if 3-pump_avg < 40 K:              increase Output 2 by 10%  (aggressive)
//   else if 3-pump_avg < 45 K:
//       if dT_dt < -0.3 K/min:        increase Output 2 by 8%
//       else if dT_dt < -0.1 K/min:   increase Output 2 by 3%
//   else if 3-pump_avg > 55 K:        reduce Output 2 by 5%
//   Output 2 may go up to 100% — normal during Phase 3.
//   There is no minimum interval between adjustments (unlike Phase 2).
//
// 4-switch monitoring:
//   Watch for switch_k to exceed 20 K (switch fully open).
//   If not reached 20 K after 15 min, increase Output 3 to 45%.
//
// 3-switch protection:
//   `switch_k` (D2) is the 4-switch. The 3-switch temp is not a separate CSV
//   column; it tracks close to the 4K stage when Output 4 = 0 (Phase 3 entry).
//   Use `stage_4k_k` as a proxy. If it rises above 14 K:
//     reduce Output 3 by 5%, increase Output 2 by 5%.
//
// Exit condition:
//   Both heads (rolling avg) < 2.0 K → transition to Phase 4.
//   Typically takes 60–75 minutes from entry.

/// Run Phase 3: turn off the 4-pump and open the 4-switch to condense ⁴He.
///
/// `out2_entry` is the Output 2 percentage handed off from Phase 2.
pub fn phase3_cycle_4he(csv_path: &str, out2_entry: f64) -> Result<f64, String> {
    println!("[GL7] Phase 3 — cycling ⁴He module (4-pump off, 4-switch opening)...");

    let mut ls350 = LakeShore350Controller::default();
    let out1: f64 = 0.0;               // 4-pump — off for all of Phase 3
    let mut out2: f64 = out2_entry;    // 3-pump — carried from Phase 2
    let mut out3: f64 = SWITCH_ON_OUTPUT; // 4-switch — opened on entry
    // Output 4 (3-switch) stays at 0% throughout Phase 3.

    // ── Entry actions ─────────────────────────────────────────────────────────
    set_output_pct(&mut ls350, 1, 0.0)?;
    set_output_pct(&mut ls350, 3, SWITCH_ON_OUTPUT)?;
    println!(
        "[GL7]   4-pump off, 4-switch at {:.0}%.  3-pump entry output: {:.1}%",
        SWITCH_ON_OUTPUT, out2
    );

    let phase_start = Instant::now();
    let mut switch_boost_applied = false;
    let mut switch_opened_logged = false;

    let mut pump3_avg = RollingAverage::new(4);
    let mut head4_avg = RollingAverage::new(4);
    let mut head3_avg = RollingAverage::new(4);

    loop {
        thread::sleep(Duration::from_secs(30));

        let t = read_latest_temps(csv_path)?;
        let elapsed = phase_start.elapsed();

        // ── Update rolling averages ───────────────────────────────────────────
        pump3_avg.push(t.pump3_k);
        head4_avg.push(t.head4_k);
        head3_avg.push(t.head3_k);

        let p3_avg = pump3_avg.average();
        let p3_dt  = pump3_avg.rate_of_change();
        let h4_avg = head4_avg.average();
        let h3_avg = head3_avg.average();

        // ── State log ─────────────────────────────────────────────────────────
        println!(
            "[GL7] t={:.1}m | \
             4-head {:.3}K (avg {:.2}K) | 3-head {:.3}K (avg {:.2}K) | \
             3-pump {:.3}K (avg {:.2}K  {:+.3}K/min) out2={:.1}% | \
             4-switch {:.3}K out3={:.1}% | 4K stage {:.3}K",
            elapsed.as_secs_f64() / 60.0,
            t.head4_k, h4_avg,
            t.head3_k, h3_avg,
            t.pump3_k, p3_avg, p3_dt, out2,
            t.switch_k, out3,
            t.stage_4k_k,
        );

        // ── Safety hard limits ────────────────────────────────────────────────
        if t.pump3_k > PUMP_HARD_LIMIT {
            let new_val = (out2 - 20.0).max(0.0);
            println!(
                "[GL7] SAFETY: 3-pump {:.3} K > {:.0} K — Output 2 cut by 20%",
                t.pump3_k, PUMP_HARD_LIMIT
            );
            set_output_pct(&mut ls350, 2, new_val)?; out2 = new_val;
        }
        if t.stage_4k_k > STAGE_4K_HARD_LIMIT {
            let new_val = (out2 - 10.0).max(0.0);
            println!(
                "[GL7] SAFETY: 4K stage {:.3} K > {:.0} K — Output 2 cut by 10%",
                t.stage_4k_k, STAGE_4K_HARD_LIMIT
            );
            set_output_pct(&mut ls350, 2, new_val)?; out2 = new_val;
        }

        // ── 3-switch protection ───────────────────────────────────────────────
        // stage_4k_k is the proxy for 3-switch temp (Output 4 = 0% in Phase 3).
        if t.stage_4k_k > SWITCH3_DANGER_TEMP {
            let new_out3 = (out3 - 5.0).max(0.0);
            let new_out2 = (out2 + 5.0).min(100.0);
            println!(
                "[GL7] WARNING: 3-switch proxy (4K stage) {:.3} K > {:.0} K — \
                 reducing Output 3: {:.1}% → {:.1}%, boosting Output 2: {:.1}% → {:.1}%",
                t.stage_4k_k, SWITCH3_DANGER_TEMP,
                out3, new_out3, out2, new_out2,
            );
            set_output_pct(&mut ls350, 3, new_out3)?; out3 = new_out3;
            set_output_pct(&mut ls350, 2, new_out2)?; out2 = new_out2;
        }

        // ── 4-switch monitoring ───────────────────────────────────────────────
        if t.switch_k >= SWITCH_OPEN_TEMP && !switch_opened_logged {
            println!(
                "[GL7]   4-switch open ({:.3} K ≥ {:.0} K) at t={:.1} min.",
                t.switch_k, SWITCH_OPEN_TEMP, elapsed.as_secs_f64() / 60.0
            );
            switch_opened_logged = true;
        }
        if !switch_boost_applied
            && elapsed.as_secs() >= SWITCH_OPEN_TIMEOUT_SECS
            && t.switch_k < SWITCH_OPEN_TEMP
        {
            println!(
                "[GL7]   4-switch not open after {} min ({:.3} K < {:.0} K) — \
                 boosting Output 3: {:.1}% → {:.1}%",
                SWITCH_OPEN_TIMEOUT_SECS / 60, t.switch_k, SWITCH_OPEN_TEMP,
                out3, SWITCH_BOOST_OUTPUT,
            );
            set_output_pct(&mut ls350, 3, SWITCH_BOOST_OUTPUT)?; out3 = SWITCH_BOOST_OUTPUT;
            switch_boost_applied = true;
        }

        // ── 3-pump management ─────────────────────────────────────────────────
        // No minimum interval between adjustments — the 3-pump can cool rapidly
        // when the 4-switch opens and needs aggressive, immediate response.
        if p3_avg < PUMP3_EMERGENCY_LOW {
            let new_val = (out2 + 10.0).min(100.0);
            println!(
                "[GL7]   3-pump avg {:.2} K < {:.0} K (emergency) — \
                 Output 2: {:.1}% → {:.1}%",
                p3_avg, PUMP3_EMERGENCY_LOW, out2, new_val,
            );
            set_output_pct(&mut ls350, 2, new_val)?; out2 = new_val;
        } else if p3_avg < PUMP3_TARGET_LOW {
            // 40 K ≤ p3_avg < 45 K — moderate intervention based on dT/dt
            if p3_dt < -0.3 {
                let new_val = (out2 + 8.0).min(100.0);
                println!(
                    "[GL7]   3-pump avg {:.2} K, falling fast ({:+.3} K/min) — \
                     Output 2: {:.1}% → {:.1}%",
                    p3_avg, p3_dt, out2, new_val,
                );
                set_output_pct(&mut ls350, 2, new_val)?; out2 = new_val;
            } else if p3_dt < -0.1 {
                let new_val = (out2 + 3.0).min(100.0);
                println!(
                    "[GL7]   3-pump avg {:.2} K, falling ({:+.3} K/min) — \
                     Output 2: {:.1}% → {:.1}%",
                    p3_avg, p3_dt, out2, new_val,
                );
                set_output_pct(&mut ls350, 2, new_val)?; out2 = new_val;
            }
        } else if p3_avg > PUMP3_TARGET_HIGH {
            // > 55 K — bleed off excess heat
            let new_val = (out2 - 5.0).max(0.0);
            println!(
                "[GL7]   3-pump avg {:.2} K > {:.0} K (too warm) — \
                 Output 2: {:.1}% → {:.1}%",
                p3_avg, PUMP3_TARGET_HIGH, out2, new_val,
            );
            set_output_pct(&mut ls350, 2, new_val)?; out2 = new_val;
        }

        // ── Exit condition ────────────────────────────────────────────────────
        // Use rolling averages to avoid triggering on a single noisy reading.
        if h4_avg < HEAD_CYCLE_THRESHOLD && h3_avg < HEAD_CYCLE_THRESHOLD {
            println!(
                "[GL7] Phase 3 complete — both heads below {:.1} K \
                 (4-head avg {:.3} K, 3-head avg {:.3} K) at t={:.1} min.",
                HEAD_CYCLE_THRESHOLD, h4_avg, h3_avg,
                elapsed.as_secs_f64() / 60.0,
            );
            println!(
                "[GL7]   Final outputs: out1={:.1}%  out2={:.1}%  out3={:.1}%  out4=0.0%",
                out1, out2, out3,
            );
            println!("[GL7]   Transitioning to Phase 4.");
            return Ok(out3);
        }
    }
}

// ── Phase 4: Cycle ³He module ─────────────────────────────────────────────────
//
// Goal: turn off the 3-pump heater and open the 3-switch to begin ³He
// condensation onto the 3-head (base temperature stage).
//
// Entry actions (execute immediately, no delay between):
//   1. Set Output 2 to 0%  (3-pump off)
//   2. Set Output 4 to 40%  (begin opening 3-switch)
//
// Monitoring:
//   This phase is passive — no output adjustments are needed.
//   Leave Output 3 at its current value (keeps 4-switch open).
//   Leave Output 4 at 40%.
//   3-head will cool: ~1.7 K → ~0.5 K in ~5 min, then slowly to ~320–340 mK.
//
// Safety:
//   4K stage > 12 K: reduce Outputs 3 and 4 by 10% each.
//
// Exit condition:
//   3-head < 350 mK sustained for ≥ 5 minutes → transition to Phase 5.
//   (Typically takes 25–35 minutes from Phase 4 entry.)

/// Run Phase 4: turn off the 3-pump and open the 3-switch to condense ³He.
///
/// `out3_entry` is the Output 3 percentage handed off from Phase 3 (keeps the
/// 4-switch open).
pub fn phase4_cycle_3he(csv_path: &str, out3_entry: f64) -> Result<(f64, f64), String> {
    println!("[GL7] Phase 4 — cycling ³He module (3-pump off, 3-switch opening)...");

    let mut ls350 = LakeShore350Controller::default();
    let out2: f64 = 0.0;               // 3-pump — off for all of Phase 4
    let mut out3: f64 = out3_entry;    // 4-switch — carried from Phase 3
    let mut out4: f64 = SWITCH_ON_OUTPUT; // 3-switch — opened on entry

    // ── Entry actions ─────────────────────────────────────────────────────────
    set_output_pct(&mut ls350, 2, 0.0)?;
    set_output_pct(&mut ls350, 4, SWITCH_ON_OUTPUT)?;
    println!(
        "[GL7]   3-pump off, 3-switch at {:.0}%.  4-switch held at {:.1}%.",
        SWITCH_ON_OUTPUT, out3
    );

    let phase_start = Instant::now();
    let mut base_temp_since: Option<Instant> = None;

    loop {
        thread::sleep(Duration::from_secs(30));

        let t = read_latest_temps(csv_path)?;
        let elapsed = phase_start.elapsed();

        // ── Safety hard limit ─────────────────────────────────────────────────
        if t.stage_4k_k > STAGE_4K_HARD_LIMIT {
            let new_out3 = (out3 - 10.0).max(0.0);
            let new_out4 = (out4 - 10.0).max(0.0);
            println!(
                "[GL7] SAFETY: 4K stage {:.3} K > {:.0} K — Outputs 3 & 4 cut by 10%",
                t.stage_4k_k, STAGE_4K_HARD_LIMIT
            );
            set_output_pct(&mut ls350, 3, new_out3)?; out3 = new_out3;
            set_output_pct(&mut ls350, 4, new_out4)?; out4 = new_out4;
        }

        // ── State log ─────────────────────────────────────────────────────────
        println!(
            "[GL7] t={:.1}m | 3-head {:.4} K | 4-head {:.3} K | \
             out3={:.1}%  out4={:.1}% | 4K stage {:.3} K",
            elapsed.as_secs_f64() / 60.0,
            t.head3_k, t.head4_k,
            out3, out4,
            t.stage_4k_k,
        );

        // ── Exit condition: 3-head < 350 mK sustained for 5 minutes ──────────
        if t.head3_k < BASE_TEMP_THRESHOLD {
            if base_temp_since.is_none() {
                println!(
                    "[GL7]   3-head below {:.3} K — starting base-temp timer.",
                    BASE_TEMP_THRESHOLD
                );
            }
            base_temp_since.get_or_insert_with(Instant::now);
        } else {
            if base_temp_since.is_some() {
                println!(
                    "[GL7]   3-head rose above {:.3} K ({:.4} K) — resetting base-temp timer.",
                    BASE_TEMP_THRESHOLD, t.head3_k
                );
            }
            base_temp_since = None;
        }

        if let Some(since) = base_temp_since {
            let held_secs = since.elapsed().as_secs();
            if held_secs >= BASE_TEMP_DURATION_SECS {
                println!(
                    "[GL7] Phase 4 complete — 3-head below {:.3} K for {} min \
                     ({:.4} K) at t={:.1} min.",
                    BASE_TEMP_THRESHOLD,
                    BASE_TEMP_DURATION_SECS / 60,
                    t.head3_k,
                    elapsed.as_secs_f64() / 60.0,
                );
                println!(
                    "[GL7]   Final outputs: out1=0.0%  out2={:.1}%  out3={:.1}%  out4={:.1}%",
                    out2, out3, out4,
                );
                println!("[GL7]   Transitioning to Phase 5.");
                return Ok((out3, out4));
            } else {
                println!(
                    "[GL7]   Base-temp timer: {:.1}/{:.0} min",
                    held_secs as f64 / 60.0,
                    BASE_TEMP_DURATION_SECS / 60,
                );
            }
        }
    }
}

// ── Phase 5: Running at base temperature ──────────────────────────────────────
//
// Goal: hold at base temperature and monitor system health.
//
// Expected steady state:
//   Output 1: 0%       (4-pump off)
//   Output 2: 0%       (3-pump off)
//   Output 3: ~35–45%  (4-switch held open)
//   Output 4: ~35–45%  (3-switch held open)
//
// Health monitoring (every 5 minutes):
//   - 3-head should be 320–340 mK. Warn if > 400 mK.
//   - 4-head should be ~0.87 K (settling). Warn if > 3 K.
//   - Both pump temps should be < 10 K and slowly decreasing toward 4 K.
//   - 4K stage should be < 4.5 K.
//
// End-of-life detection:
//   The GL7 run ends when the ⁴He is exhausted. The 4-head will begin warming.
//   If 4-head > 3 K AND dT_dt > +0.01 K/min:
//       log total run duration and alert the user — the run has ended.
//       Typical hold time is ~36 hours.

/// Run Phase 5: monitor the GL7 at base temperature until ⁴He is exhausted.
///
/// `out3_entry` and `out4_entry` are the switch heater percentages handed off
/// from Phase 4. No output changes are made during this phase.
///
/// Returns `Ok(())` when end-of-life is detected (4-head > 3 K and rising).
pub fn phase5_running(csv_path: &str, out3_entry: f64, out4_entry: f64) -> Result<(), String> {
    println!("[GL7] Phase 5 — running at base temperature.");
    println!(
        "[GL7]   Outputs held: out1=0.0%  out2=0.0%  out3={:.1}%  out4={:.1}%",
        out3_entry, out4_entry,
    );
    println!("[GL7]   Monitoring every {} min. Typical hold time ~36 hours.", PHASE5_POLL_SECS / 60);

    let phase_start = Instant::now();
    let mut head4_avg = RollingAverage::new(4);
    let mut head3_avg = RollingAverage::new(4);

    loop {
        thread::sleep(Duration::from_secs(PHASE5_POLL_SECS));

        let t = read_latest_temps(csv_path)?;
        let elapsed = phase_start.elapsed();

        head4_avg.push(t.head4_k);
        head3_avg.push(t.head3_k);

        let h4_dt = head4_avg.rate_of_change();

        println!(
            "[GL7] t={:.1}h | 3-head {:.4} K | 4-head {:.3} K ({:+.4} K/min) | \
             3-pump {:.3} K | 4-pump {:.3} K | 4K stage {:.3} K",
            elapsed.as_secs_f64() / 3600.0,
            t.head3_k,
            t.head4_k, h4_dt,
            t.pump3_k, t.pump4_k,
            t.stage_4k_k,
        );

        // ── Health warnings ───────────────────────────────────────────────────
        if t.head3_k > HEAD3_WARNING {
            println!(
                "[GL7] WARNING: 3-head {:.4} K > {:.3} K — base temperature elevated.",
                t.head3_k, HEAD3_WARNING
            );
        }
        if t.head4_k > HEAD4_WARNING {
            println!(
                "[GL7] WARNING: 4-head {:.3} K > {:.1} K.",
                t.head4_k, HEAD4_WARNING
            );
        }

        // ── End-of-life detection ─────────────────────────────────────────────
        // Requires at least 2 readings so dT/dt is meaningful.
        if head4_avg.len() >= 2
            && t.head4_k > HEAD4_EXPIRED_THRESHOLD
            && h4_dt > HEAD4_EXPIRED_RATE
        {
            println!(
                "[GL7] *** END OF LIFE DETECTED ***"
            );
            println!(
                "[GL7]   4-head {:.3} K > {:.1} K and rising at {:+.4} K/min \
                 (threshold {:+.3} K/min).",
                t.head4_k, HEAD4_EXPIRED_THRESHOLD,
                h4_dt, HEAD4_EXPIRED_RATE,
            );
            println!(
                "[GL7]   Total run time: {:.1} hours.",
                elapsed.as_secs_f64() / 3600.0
            );
            println!("[GL7]   ⁴He exhausted — GL7 run complete.");
            return Ok(());
        }
    }
}

// ── Safety limits (override all phase logic) ──────────────────────────────────
//
// These checks run at the top of every control loop iteration, before any
// phase-specific logic. They take immediate corrective action.
//
//   Any pump > 65 K:                reduce that pump's output by 20%
//   4K stage > 12 K:                reduce all heater outputs (1 & 2) by 10%
//   Output commanded > 100%:        clamp to 100%
//   Output commanded < 0%:          clamp to 0%
//   3-switch temp > 14 K (Phase 3): reduce Output 3 by 5%, increase Output 2 by 5%
//   Phase 2 running > 180 min:      halt and require manual intervention
