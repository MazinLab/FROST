// worker.rs — background serial I/O worker for FROST GUI
//
// All blocking serial calls run here, off the egui render thread.
// The GUI reads `DeviceSnapshot` (via Arc<Mutex>) every frame — never blocking.
// User actions (button clicks) send a `GuiCommand` over an mpsc channel.
// The worker calls `ctx.request_repaint()` whenever the snapshot changes.

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::compressor::CryomechController;
use crate::lakeshore350::LakeShore350Controller;
use crate::lakeshore370::LakeShore370Controller;
use crate::lakeshore625::LakeShore625Controller;

// ── Button-state persistence (lock-file pattern) ───────────────
//
// Mirrors the record_temps lock-file approach.  Both files live under state/
// so they survive process restarts.  Existence == active; absence == inactive.
// The `_at` variants accept an explicit path for test isolation.

pub const COMPRESSOR_INTENT_PATH: &str = "state/.compressor_intent";
pub const ADR_RAMP_RUNNING_PATH:   &str = "state/.adr_ramp_running";

fn ensure_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
}

/// Write or remove the compressor-intent lock file.
/// `running == true` → user last confirmed Start; `false` → Stop.
pub fn set_compressor_intent_at(path: &Path, running: bool) {
    if running {
        ensure_parent(path);
        let _ = fs::write(path, "");
    } else {
        let _ = fs::remove_file(path);
    }
}

pub fn set_compressor_intent(running: bool) {
    set_compressor_intent_at(Path::new(COMPRESSOR_INTENT_PATH), running);
}

pub fn is_compressor_intent_at(path: &Path) -> bool {
    path.exists()
}

pub fn is_compressor_intent() -> bool {
    is_compressor_intent_at(Path::new(COMPRESSOR_INTENT_PATH))
}

/// Write or remove the ADR-ramp lock file.
pub fn set_adr_ramp_persisted_at(path: &Path, running: bool) {
    if running {
        ensure_parent(path);
        let _ = fs::write(path, "");
    } else {
        let _ = fs::remove_file(path);
    }
}

pub fn set_adr_ramp_persisted(running: bool) {
    set_adr_ramp_persisted_at(Path::new(ADR_RAMP_RUNNING_PATH), running);
}

pub fn is_adr_ramp_persisted_at(path: &Path) -> bool {
    path.exists()
}

pub fn is_adr_ramp_persisted() -> bool {
    is_adr_ramp_persisted_at(Path::new(ADR_RAMP_RUNNING_PATH))
}

// ── Shared data types ──────────────────────────────────────────

/// Temperature readings from both Lakeshore controllers.
#[derive(Default, Clone)]
pub struct TemperatureReadings {
    pub ls350_a:  String,   // 3-head
    pub ls350_b:  String,   // ADR
    pub ls350_c:  String,   // 4-head
    pub ls350_d2: String,   // Switch voltage
    pub ls350_d3: String,   // 4K stage
    pub ls350_d4: String,   // 3-pump
    pub ls350_d5: String,   // 4-pump
    pub ls370_1:  String,   // Input 1
}

/// All device state updated by the worker thread and read by the GUI.
/// Clone is cheap — all fields are small Strings / primitives.
#[derive(Clone)]
pub struct DeviceSnapshot {
    // ── Compressor ────────────────────────────────────────────
    pub compressor_status:      String,
    pub compressor_running:     bool,
    pub last_compressor_update: Option<Instant>,
    /// Drained by GUI each frame: Some(Ok) = success, Some(Err) = failure.
    pub compressor_cmd_result:  Option<Result<(), String>>,

    // ── Magnet (LS625) ────────────────────────────────────────
    pub magnet_limits:  String,
    pub magnet_quench:  String,
    pub magnet_current: String,
    pub magnet_voltage: String,
    pub magnet_field:   String,
    /// Values polled from hardware; GUI syncs these into its edit fields.
    pub magnet_polled_current_limit:      Option<f64>,
    pub magnet_polled_voltage_limit:      Option<f64>,
    pub magnet_polled_rate_limit:         Option<f64>,
    pub magnet_polled_ramp_rate:          Option<f64>,
    pub magnet_polled_compliance_voltage: Option<f64>,
    pub magnet_polled_target_current:     Option<f64>,
    pub last_magnet_update:       Option<Instant>,
    /// Drained by GUI each frame.
    pub magnet_cmd_result:        Option<Result<(), String>>,
    pub magnet_rate_result:       Option<Result<(), String>>,
    pub magnet_compliance_result: Option<Result<(), String>>,
    pub magnet_limits_result:     Option<Result<(), String>>,

    // ── GL7 sorption cooler (LS350 outputs 1–4) ───────────────
    pub gl7_output_lines: Vec<(String, String)>,
    pub gl7_polled_pct:   Vec<Option<f64>>,
    pub last_gl7_update:  Option<Instant>,
    /// Drained by GUI each frame.
    pub gl7_set_results:  Vec<Option<Result<(), String>>>,

    // ── Thermometry ───────────────────────────────────────────
    pub temperatures:     TemperatureReadings,
    pub last_temp_update: Option<Instant>,

    // ── ADR ramp ──────────────────────────────────────────────
    pub adr_ramp_running: bool,
    pub adr_ramp_started: Option<Instant>,
    pub adr_ramp_result:  Option<Result<(), String>>,
    /// Permanent log lines accumulated during the ramp (cleared on next ramp start).
    pub adr_log_lines:    Vec<String>,
    /// Live-updating status line (countdown / polling readout); empty when idle.
    pub adr_status_line:  String,
    /// Set on startup when the ADR-ramp lock file was present but no ramp is running.
    /// Signals "ramp was interrupted at last close"; cleared when user starts a new ramp.
    pub adr_ramp_interrupted: bool,
}

impl Default for DeviceSnapshot {
    fn default() -> Self {
        Self {
            compressor_status:      String::new(),
            compressor_running:     false,
            last_compressor_update: None,
            compressor_cmd_result:  None,
            magnet_limits:  String::new(),
            magnet_quench:  String::new(),
            magnet_current: String::new(),
            magnet_voltage: String::new(),
            magnet_field:   String::new(),
            magnet_polled_current_limit:      None,
            magnet_polled_voltage_limit:      None,
            magnet_polled_rate_limit:         None,
            magnet_polled_ramp_rate:          None,
            magnet_polled_compliance_voltage: None,
            magnet_polled_target_current:     None,
            last_magnet_update:       None,
            magnet_cmd_result:        None,
            magnet_rate_result:       None,
            magnet_compliance_result: None,
            magnet_limits_result:     None,
            gl7_output_lines: vec![(String::new(), String::new()); 4],
            gl7_polled_pct:   vec![None; 4],
            last_gl7_update:  None,
            gl7_set_results:  vec![None; 4],
            temperatures:     TemperatureReadings::default(),
            last_temp_update: None,
            adr_ramp_running: false,
            adr_ramp_started: None,
            adr_ramp_result:  None,
            adr_log_lines:    Vec::new(),
            adr_status_line:  String::new(),
            adr_ramp_interrupted: false,
        }
    }
}

// ── Commands from GUI to worker ────────────────────────────────

pub enum GuiCommand {
    StartCompressor,
    StopCompressor,
    SetGl7Output { output: u8, pct: f64 },
    RunAdrRamp   { rate: f64, current: f64, soak_mins: u64 },
}

// ── Public worker handle ───────────────────────────────────────

pub struct SerialWorker {
    pub snapshot: Arc<Mutex<DeviceSnapshot>>,
    pub cmd_tx:   Sender<GuiCommand>,
}

impl SerialWorker {
    /// Spawn the background thread.  `ctx` is used to trigger GUI repaints.
    pub fn spawn(ctx: egui::Context) -> Self {
        let snapshot = Arc::new(Mutex::new(DeviceSnapshot::default()));

        // Restore button state from lock files written during the previous session.
        {
            let mut s = snapshot.lock().unwrap();
            // Seed compressor_running from the intent file so the button shows the
            // correct label immediately, before the first 30-second poll completes.
            s.compressor_running = is_compressor_intent();
            // Flag an interrupted ADR ramp so the GUI can warn the user.
            if is_adr_ramp_persisted() {
                s.adr_ramp_interrupted = true;
                // Clear the lock file — one warning per interruption is enough.
                set_adr_ramp_persisted(false);
            }
        }

        let (cmd_tx, cmd_rx) = channel::<GuiCommand>();

        let snap = Arc::clone(&snapshot);
        std::thread::spawn(move || worker_loop(snap, cmd_rx, ctx));

        Self { snapshot, cmd_tx }
    }

    /// Non-blocking send — fire and forget.
    pub fn send(&self, cmd: GuiCommand) {
        let _ = self.cmd_tx.send(cmd);
    }
}

// ── Worker loop ────────────────────────────────────────────────

const POLL_INTERVAL: Duration = Duration::from_secs(30);

struct WorkerState {
    compressor: CryomechController,
    ls625:      LakeShore625Controller,
    ls350:      LakeShore350Controller,
    ls370:      LakeShore370Controller,
    last_compressor_poll: Instant,
    last_magnet_poll:     Instant,
    last_gl7_poll:        Instant,
    last_temp_poll:       Instant,
}

impl WorkerState {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            compressor: CryomechController::default(),
            ls625:      LakeShore625Controller::default(),
            ls350:      LakeShore350Controller::default(),
            ls370:      LakeShore370Controller::default(),
            // Stagger polls to match original gui.rs timing:
            //   Compressor fires immediately, magnet +10 s, GL7 +20 s, temps +28 s
            last_compressor_poll: now - Duration::from_secs(35),
            last_magnet_poll:     now - Duration::from_secs(22),
            last_gl7_poll:        now - Duration::from_secs(12),
            last_temp_poll:       now - Duration::from_secs(2),
        }
    }
}

fn worker_loop(
    snap: Arc<Mutex<DeviceSnapshot>>,
    rx:   Receiver<GuiCommand>,
    ctx:  egui::Context,
) {
    let mut state = WorkerState::new();

    loop {
        // Drain all pending commands before polling.
        while let Ok(cmd) = rx.try_recv() {
            execute_command(cmd, &mut state, &snap, &ctx);
            ctx.request_repaint();
        }

        // Periodic polls — each blocks this thread, but the GUI reads the
        // snapshot without waiting so it stays fully responsive.
        if state.last_compressor_poll.elapsed() >= POLL_INTERVAL {
            poll_compressor(&mut state, &snap);
            state.last_compressor_poll = Instant::now();
            ctx.request_repaint();
        }

        if state.last_magnet_poll.elapsed() >= POLL_INTERVAL {
            poll_magnet(&mut state, &snap);
            state.last_magnet_poll = Instant::now();
            ctx.request_repaint();
        }

        if state.last_gl7_poll.elapsed() >= POLL_INTERVAL {
            poll_gl7(&mut state, &snap);
            state.last_gl7_poll = Instant::now();
            ctx.request_repaint();
        }

        if state.last_temp_poll.elapsed() >= POLL_INTERVAL {
            poll_temps(&mut state, &snap);
            state.last_temp_poll = Instant::now();
            ctx.request_repaint();
        }

        // Brief sleep so we're not busy-spinning between polls.
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ── Command execution ──────────────────────────────────────────

fn execute_command(cmd: GuiCommand, state: &mut WorkerState, snap: &Arc<Mutex<DeviceSnapshot>>, ctx: &egui::Context) {
    match cmd {
        GuiCommand::StartCompressor => {
            let result = state.compressor.start_compressor().map(|_| ());
            poll_compressor(state, snap);
            snap.lock().unwrap().compressor_cmd_result = Some(result);
        }

        GuiCommand::StopCompressor => {
            let result = state.compressor.stop_compressor().map(|_| ());
            poll_compressor(state, snap);
            snap.lock().unwrap().compressor_cmd_result = Some(result);
        }

        GuiCommand::SetGl7Output { output, pct } => {
            let idx = (output as usize).saturating_sub(1);
            state.ls350.set_output_percent(output, pct);
            if let Some(err) = state.ls350.error_message.clone() {
                snap.lock().unwrap().gl7_set_results[idx] = Some(Err(err));
            } else {
                state.ls350.query_output_percentages(output);
                let (l1, l2, new_pct) = if state.ls350.error_message.is_none() {
                    let mut lines = state.ls350.output.lines();
                    let l1 = lines.next().unwrap_or("").to_string();
                    let l2 = lines.next().unwrap_or("").to_string();
                    let p  = l1.split_whitespace()
                               .last()
                               .and_then(|s| s.parse::<f64>().ok());
                    (l1, l2, p)
                } else {
                    (String::new(), String::new(), None)
                };
                let mut s = snap.lock().unwrap();
                s.gl7_output_lines[idx] = (l1, l2);
                s.gl7_polled_pct[idx]   = new_pct;
                s.gl7_set_results[idx]  = Some(Ok(()));
            }
        }

        GuiCommand::RunAdrRamp { rate, current, soak_mins } => {
            // Guard: ignore if a ramp is already in progress.
            {
                let s = snap.lock().unwrap();
                if s.adr_ramp_running {
                    return;
                }
            }
            set_adr_ramp_persisted(true);
            {
                let mut s = snap.lock().unwrap();
                s.adr_ramp_running    = true;
                s.adr_ramp_started    = Some(Instant::now());
                s.adr_ramp_result     = None;
                s.adr_ramp_interrupted = false;
                s.adr_log_lines.clear();
                s.adr_status_line.clear();
            }

            // Channel for live progress messages from the ramp thread to the GUI.
            let (log_tx, log_rx) = std::sync::mpsc::channel::<crate::adr_ramping::AdrLogMsg>();

            // Relay thread: receives log messages and writes them into the snapshot.
            let snap_relay = Arc::clone(snap);
            let ctx_relay  = ctx.clone();
            let relay = std::thread::spawn(move || {
                use crate::adr_ramping::AdrLogMsg;
                while let Ok(msg) = log_rx.recv() {
                    let mut s = snap_relay.lock().unwrap();
                    match msg {
                        AdrLogMsg::Line(line)     => s.adr_log_lines.push(line),
                        AdrLogMsg::Status(status) => s.adr_status_line = status,
                    }
                    drop(s);
                    ctx_relay.request_repaint();
                }
            });

            // Ramp thread: runs the sequence, drops log_tx when done (closes channel).
            let snap_ramp = Arc::clone(snap);
            let ctx_ramp  = ctx.clone();
            std::thread::spawn(move || {
                let result = crate::adr_ramping::run_adr_ramp(rate, current, soak_mins, Some(&log_tx));
                drop(log_tx);           // signal relay to drain and exit
                let _ = relay.join();   // wait for all messages to flush into snapshot
                set_adr_ramp_persisted(false);
                let mut s = snap_ramp.lock().unwrap();
                s.adr_ramp_running = false;
                s.adr_ramp_result  = Some(result);
                s.adr_status_line.clear();
                drop(s);
                ctx_ramp.request_repaint();
            });
        }
    }
}

// ── Poll functions ─────────────────────────────────────────────

fn poll_compressor(state: &mut WorkerState, snap: &Arc<Mutex<DeviceSnapshot>>) {
    state.compressor.get_status();
    let mut s = snap.lock().unwrap();
    if let Some(e) = state.compressor.error_message.clone() {
        s.compressor_status  = format!("Error: {e}");
        s.compressor_running = false;
        // Don't update the intent file on poll errors — preserve the last known
        // good state so a transient comms glitch doesn't lose the user's intent.
    } else {
        s.compressor_status  = state.compressor.status_output.clone();
        s.compressor_running = state.compressor.status_output
            .lines()
            .any(|l| l.contains("Running:") && l.contains("Yes"));
        // Persist so the GUI button shows the right label immediately on restart.
        set_compressor_intent(s.compressor_running);
    }
    s.last_compressor_update = Some(Instant::now());
}

fn poll_magnet(state: &mut WorkerState, snap: &Arc<Mutex<DeviceSnapshot>>) {
    // LIMIT?
    state.ls625.get_limits();
    let limits_str = if state.ls625.error_message.is_none() {
        state.ls625.output.clone()
    } else {
        format!("Error: {}", state.ls625.error_message.as_deref().unwrap_or(""))
    };
    let parsed_limits = parse_limits_from_output(&limits_str);

    // SETI?
    state.ls625.get_set_current();
    let target_current = if state.ls625.error_message.is_none() {
        parse_single_value(&state.ls625.output)
    } else {
        None
    };

    // RATE?
    state.ls625.get_ramp_rate();
    let ramp_rate = if state.ls625.error_message.is_none() {
        parse_single_value(&state.ls625.output)
    } else {
        None
    };

    // SETV?
    state.ls625.get_compliance_voltage();
    let compliance = if state.ls625.error_message.is_none() {
        parse_single_value(&state.ls625.output)
    } else {
        None
    };

    // QNCH?
    state.ls625.get_quench_status();
    let quench = if state.ls625.error_message.is_none() {
        state.ls625.output.clone()
    } else {
        String::new()
    };

    // Live readings
    let current = state.ls625.get_current().unwrap_or_default();
    let voltage = state.ls625.get_voltage().unwrap_or_default();
    let field   = state.ls625.get_field().unwrap_or_default();

    let mut s = snap.lock().unwrap();
    s.magnet_limits  = limits_str;
    s.magnet_quench  = quench;
    s.magnet_current = current;
    s.magnet_voltage = voltage;
    s.magnet_field   = field;
    if let Some((c, v, r)) = parsed_limits {
        s.magnet_polled_current_limit = Some(c);
        s.magnet_polled_voltage_limit = Some(v);
        s.magnet_polled_rate_limit    = Some(r);
    }
    s.magnet_polled_target_current     = target_current;
    s.magnet_polled_ramp_rate          = ramp_rate;
    s.magnet_polled_compliance_voltage = compliance;
    s.last_magnet_update = Some(Instant::now());
}

fn poll_gl7(state: &mut WorkerState, snap: &Arc<Mutex<DeviceSnapshot>>) {
    let mut lines: Vec<(String, String)> = vec![(String::new(), String::new()); 4];
    let mut pcts:  Vec<Option<f64>>      = vec![None; 4];

    for (i, &output_num) in [1u8, 2, 3, 4].iter().enumerate() {
        state.ls350.query_output_percentages(output_num);
        if state.ls350.error_message.is_none() {
            let mut ls = state.ls350.output.lines();
            let l1 = ls.next().unwrap_or("").to_string();
            let l2 = ls.next().unwrap_or("").to_string();
            pcts[i]  = l1.split_whitespace().last().and_then(|s| s.parse::<f64>().ok());
            lines[i] = (l1, l2);
        }
    }

    let mut s = snap.lock().unwrap();
    s.gl7_output_lines = lines;
    s.gl7_polled_pct   = pcts;
    s.last_gl7_update  = Some(Instant::now());
}

fn poll_temps(state: &mut WorkerState, snap: &Arc<Mutex<DeviceSnapshot>>) {
    state.ls350.read_input_intelligent("A");
    let ls350_a = extract_temperature_value(&state.ls350.output);

    state.ls350.read_input_intelligent("B");
    let ls350_b = extract_temperature_value(&state.ls350.output);

    state.ls350.read_input_intelligent("C");
    let ls350_c = extract_temperature_value(&state.ls350.output);

    state.ls350.read_input_intelligent("D2");
    let ls350_d2 = extract_temperature_value(&state.ls350.output);

    state.ls350.read_input_intelligent("D3");
    let ls350_d3 = extract_temperature_value(&state.ls350.output);

    state.ls350.read_input_intelligent("D4");
    let ls350_d4 = extract_temperature_value(&state.ls350.output);

    state.ls350.read_input_intelligent("D5");
    let ls350_d5 = extract_temperature_value(&state.ls350.output);

    let ls370_1 = match state.ls370.read_kelvin(1) {
        Ok(k)  => format_kelvin_value(&k),
        Err(e) => format!("ERROR ({e})"),
    };

    let mut s = snap.lock().unwrap();
    s.temperatures = TemperatureReadings {
        ls350_a, ls350_b, ls350_c, ls350_d2, ls350_d3, ls350_d4, ls350_d5,
        ls370_1,
    };
    s.last_temp_update = Some(Instant::now());
}

// ── Helpers (moved from gui.rs) ────────────────────────────────

/// Extract the temperature string from `read_input_intelligent` output.
pub fn extract_temperature_value(temp_str: &str) -> String {
    if temp_str.contains("ERROR") {
        return temp_str.to_string();
    }
    if let Some(arrow_pos) = temp_str.find('\u{2192}') {
        let after = &temp_str[arrow_pos + '\u{2192}'.len_utf8()..];
        if let Some(k_pos) = after.find(" K") {
            return after[..k_pos + 2].trim().to_string();
        }
    }
    if let Some(colon_pos) = temp_str.rfind(':') {
        let after = temp_str[colon_pos + 1..].trim();
        if after.ends_with(" K") {
            return after.to_string();
        }
    }
    temp_str.trim().to_string()
}

/// Format a raw KRDG? response (e.g. "+1.2345") to "1.2345 K".
pub fn format_kelvin_value(k_str: &str) -> String {
    match k_str.parse::<f64>() {
        Ok(k) if k > 0.0 => format!("{k:.4} K"),
        Ok(_)            => format!("{k_str} (overload)"),
        Err(_)           => k_str.to_string(),
    }
}

/// Extract the third whitespace-delimited token as f64.
/// Handles "Set current: 9.44 A", "Ramp rate: 0.01 A/s", "Compliance voltage: 1.0 V".
pub fn parse_single_value(output: &str) -> Option<f64> {
    output.split_whitespace().nth(2)?.parse().ok()
}

/// Parse "Current limit: X A\nVoltage limit: Y V\nRate limit: Z A/s" → (X, Y, Z).
pub fn parse_limits_from_output(output: &str) -> Option<(f64, f64, f64)> {
    let mut current = None;
    let mut voltage = None;
    let mut rate    = None;
    for line in output.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("Current") => { current = parts.nth(1).and_then(|s| s.parse().ok()); }
            Some("Voltage") => { voltage = parts.nth(1).and_then(|s| s.parse().ok()); }
            Some("Rate")    => { rate    = parts.nth(1).and_then(|s| s.parse().ok()); }
            _ => {}
        }
    }
    match (current, voltage, rate) {
        (Some(c), Some(v), Some(r)) => Some((c, v, r)),
        _ => None,
    }
}
