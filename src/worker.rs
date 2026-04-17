// worker.rs — background serial I/O worker for FROST GUI
//
// All blocking serial calls run here, off the egui render thread.
// The GUI reads `DeviceSnapshot` (via Arc<Mutex>) every frame — never blocking.
// User actions (button clicks) send a `GuiCommand` over an mpsc channel.
// The worker calls `ctx.request_repaint()` whenever the snapshot changes.

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::compressor::CryomechController;
use crate::lakeshore350::LakeShore350Controller;
use crate::lakeshore370::LakeShore370Controller;
use crate::lakeshore625::LakeShore625Controller;
use crate::record_temps::{start_recording_loop, TemperatureRecord};

// ── Button-state persistence (lock-file pattern) ───────────────
//
// Mirrors the record_temps lock-file approach.  Both files live under state/
// so they survive process restarts.  Existence == active; absence == inactive.
// The `_at` variants accept an explicit path for test isolation.

pub const COMPRESSOR_INTENT_PATH:  &str = "state/.compressor_intent";
pub const HEATSWITCH_OPEN_PATH:    &str = "state/.heatswitch_open";
pub const ADR_RAMP_RUNNING_PATH:   &str = "state/.adr_ramp_running";
pub const ADR_RAMP_LOG_PATH:       &str = "state/.adr_ramp_log";
pub const ADR_RAMP_STATUS_PATH:    &str = "state/.adr_ramp_status";
pub const ADR_RAMP_STOP_PATH:      &str = "state/.adr_ramp_stop_request";
pub const ADR_RAMP_RESULT_PATH:    &str = "state/.adr_ramp_result";

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

/// Write the ADR-ramp lock file, storing the subprocess PID.
pub fn set_adr_ramp_persisted_at(path: &Path, pid: u32) {
    ensure_parent(path);
    let _ = fs::write(path, pid.to_string());
}

pub fn set_adr_ramp_persisted(pid: u32) {
    set_adr_ramp_persisted_at(Path::new(ADR_RAMP_RUNNING_PATH), pid);
}

pub fn clear_adr_ramp_persisted_at(path: &Path) {
    let _ = fs::remove_file(path);
}

pub fn clear_adr_ramp_persisted() {
    clear_adr_ramp_persisted_at(Path::new(ADR_RAMP_RUNNING_PATH));
}

#[allow(dead_code)]
pub fn is_adr_ramp_persisted_at(path: &Path) -> bool {
    path.exists()
}

#[allow(dead_code)]
pub fn is_adr_ramp_persisted() -> bool {
    is_adr_ramp_persisted_at(Path::new(ADR_RAMP_RUNNING_PATH))
}

/// Returns the PID stored in the ADR-ramp lock file **only if that process is still running**.
pub fn get_adr_ramp_pid_at(path: &Path) -> Option<u32> {
    let pid = fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())?;
    if std::path::Path::new(&format!("/proc/{pid}")).exists() {
        Some(pid)
    } else {
        let _ = fs::remove_file(path);
        None
    }
}

pub fn get_adr_ramp_pid() -> Option<u32> {
    get_adr_ramp_pid_at(Path::new(ADR_RAMP_RUNNING_PATH))
}

/// Write the GL7 cooldown lock file, storing the subprocess PID.
pub const GL7_COOLDOWN_RUNNING_PATH: &str = "state/.gl7_cooldown_running";

pub fn set_gl7_cooldown_persisted(pid: u32) {
    ensure_parent(Path::new(GL7_COOLDOWN_RUNNING_PATH));
    let _ = fs::write(GL7_COOLDOWN_RUNNING_PATH, pid.to_string());
}

pub fn clear_gl7_cooldown_persisted() {
    let _ = fs::remove_file(GL7_COOLDOWN_RUNNING_PATH);
}

/// GL7 output-percentage state file written by the cooldown subprocess so the
/// worker can display live percentages without polling the LS350 port directly.
/// Format: four space-separated f64 values representing outputs 1–4.
pub const GL7_OUTPUTS_PATH: &str = "state/.gl7_outputs";

pub fn write_gl7_output_state(outputs: [f64; 4]) {
    ensure_parent(Path::new(GL7_OUTPUTS_PATH));
    let content = format!("{} {} {} {}", outputs[0], outputs[1], outputs[2], outputs[3]);
    let _ = fs::write(GL7_OUTPUTS_PATH, content);
}

pub fn read_gl7_output_state() -> Option<[f64; 4]> {
    let content = fs::read_to_string(GL7_OUTPUTS_PATH).ok()?;
    let vals: Vec<f64> = content.split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if vals.len() == 4 {
        Some([vals[0], vals[1], vals[2], vals[3]])
    } else {
        None
    }
}

pub fn clear_gl7_output_state() {
    let _ = fs::remove_file(GL7_OUTPUTS_PATH);
}

/// Returns the PID stored in the GL7 cooldown lock file **only if that
/// process is still running**.  A dead process cannot have cleared the
/// file itself (it crashed), so this check prevents a stale lock from
/// permanently suppressing GL7 and temperature polls.
pub fn get_gl7_cooldown_pid() -> Option<u32> {
    let pid = fs::read_to_string(GL7_COOLDOWN_RUNNING_PATH)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())?;
    // /proc/<pid> exists for the lifetime of the process on Linux.
    if std::path::Path::new(&format!("/proc/{pid}")).exists() {
        Some(pid)
    } else {
        // Process is gone — clean up the stale lock file.
        let _ = fs::remove_file(GL7_COOLDOWN_RUNNING_PATH);
        None
    }
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

    // ── Temperature recording ─────────────────────────────────
    pub recording_active:       bool,
    pub recording_csv_path:     Option<String>,
    /// Drained by GUI each frame: Some(Ok(path)) = started, Some(Err) = failed.
    pub recording_start_result: Option<Result<String, String>>,

    // ── GL7 cooldown ──────────────────────────────────────────
    /// True while the GL7 cooldown subprocess is running (survives GUI restarts).
    pub gl7_cooldown_active: bool,

    // ── ADR ramp ──────────────────────────────────────────────
    pub adr_ramp_running: bool,
    pub adr_ramp_started: Option<Instant>,
    pub adr_ramp_result:  Option<Result<(), String>>,
    /// Permanent log lines accumulated during the ramp (cleared on next ramp start).
    pub adr_log_lines:    Vec<String>,
    /// Live-updating status line (countdown / polling readout); empty when idle.
    pub adr_status_line:  String,
    /// Set when the user manually stops the ramp; suppresses the thread's result write.
    pub adr_ramp_was_stopped: bool,
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
            recording_active:       false,
            recording_csv_path:     None,
            recording_start_result: None,
            gl7_cooldown_active: false,
            adr_ramp_running: false,
            adr_ramp_started: None,
            adr_ramp_result:  None,
            adr_log_lines:    Vec::new(),
            adr_status_line:  String::new(),
            adr_ramp_was_stopped: false,
        }
    }
}

// ── Commands from GUI to worker ────────────────────────────────

pub enum GuiCommand {
    StartCompressor,
    StopCompressor,
    SetGl7Output { output: u8, pct: f64 },
    RunAdrRamp,
    StopAdrRamp,
    StartRecording { interval_secs: u64, output_dir: String, resume_path: Option<String> },
    StopRecording,
    /// Update `DeviceSnapshot::gl7_cooldown_active` to reflect subprocess state.
    Gl7CooldownActive(bool),
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

        // Check for a recording lock file before the startup block so we can
        // auto-resume recording after all other state is restored.
        let was_recording = crate::record_temps::is_recording_active();

        // Restore button state from lock files written during the previous session.
        {
            let mut s = snapshot.lock().unwrap_or_else(|p| p.into_inner());
            // Seed compressor_running from the intent file so the button shows the
            // correct label immediately, before the first 30-second poll completes.
            s.compressor_running = is_compressor_intent();
            // Restore ADR ramp state: if the subprocess is still alive (PID check),
            // show the Stop button immediately.  A dead process cannot have cleared
            // the file itself, so get_adr_ramp_pid() detects and removes stale files.
            s.adr_ramp_running = get_adr_ramp_pid().is_some();
            // Restore GL7 cooldown state: if the lock file exists the subprocess
            // was running when the GUI last closed.
            s.gl7_cooldown_active = get_gl7_cooldown_pid().is_some();
        }

        let (cmd_tx, cmd_rx) = channel::<GuiCommand>();

        let snap = Arc::clone(&snapshot);
        std::thread::spawn(move || worker_loop(snap, cmd_rx, ctx));

        let worker = Self { snapshot, cmd_tx };

        // Auto-resume recording if it was active when the GUI last closed.
        // Read the CSV path stored in the lock file so recording resumes in the
        // same file rather than creating a new one on each launch.
        if was_recording {
            let resume_path = crate::record_temps::get_recording_active_path();
            worker.send(GuiCommand::StartRecording {
                interval_secs: 30,
                output_dir: "temps".to_string(),
                resume_path,
            });
        }

        worker
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
    recording_stop_flag:  Option<Arc<AtomicBool>>,
    adr_ramp_log_offset:  usize,
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
            recording_stop_flag:  None,
            adr_ramp_log_offset:  0,
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

        // Sync GL7 cooldown state from the lock file so that cooldowns started
        // from the CLI are reflected in the GUI without a restart.
        {
            let lock_active = get_gl7_cooldown_pid().is_some();
            let mut s = snap.lock().unwrap_or_else(|p| p.into_inner());
            if s.gl7_cooldown_active != lock_active {
                s.gl7_cooldown_active = lock_active;
                drop(s);
                ctx.request_repaint();
            }
        }

        // Poll ADR ramp log/status files and detect natural subprocess exit.
        poll_adr_ramp(&mut state, &snap, &ctx);

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
            // The cooldown subprocess owns the LS350 port exclusively — skip
            // hardware polling to avoid exclusive-lock errors.  Instead, read
            // the output-percentage state file the subprocess maintains so the
            // GUI still shows live values during a cooldown.
            let cooldown_active = snap.lock().unwrap_or_else(|p| p.into_inner()).gl7_cooldown_active;
            if cooldown_active {
                if let Some(pcts) = read_gl7_output_state() {
                    let mut s = snap.lock().unwrap_or_else(|p| p.into_inner());
                    for (i, &pct) in pcts.iter().enumerate() {
                        s.gl7_polled_pct[i] = Some(pct);
                    }
                    s.last_gl7_update = Some(Instant::now());
                }
            } else {
                poll_gl7(&mut state, &snap);
            }
            state.last_gl7_poll = Instant::now();
            ctx.request_repaint();
        }

        if state.last_temp_poll.elapsed() >= POLL_INTERVAL {
            // Skip when the recording thread or GL7 cooldown subprocess owns the
            // LS350/LS370 ports.  Both set exclusive port locks; a simultaneous
            // open from the GUI worker produces hard failures.
            // - Recording active: recording callback writes fresh temps instead.
            // - GL7 cooldown active: subprocess writes LS350 outputs; concurrent
            //   reads from this thread will clash on the exclusive port lock.
            let cooldown_active = snap.lock().unwrap_or_else(|p| p.into_inner()).gl7_cooldown_active;
            if state.recording_stop_flag.is_none() && !cooldown_active {
                poll_temps(&mut state, &snap);
                ctx.request_repaint();
            }
            // Always advance the timer so this block fires exactly once per
            // interval — even when the poll is skipped.
            state.last_temp_poll = Instant::now();
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
            snap.lock().unwrap_or_else(|p| p.into_inner()).compressor_cmd_result = Some(result);
        }

        GuiCommand::StopCompressor => {
            let result = state.compressor.stop_compressor().map(|_| ());
            poll_compressor(state, snap);
            snap.lock().unwrap_or_else(|p| p.into_inner()).compressor_cmd_result = Some(result);
        }

        GuiCommand::SetGl7Output { output, pct } => {
            if !(1..=4).contains(&output) {
                eprintln!("[worker] SetGl7Output: output {output} out of range (must be 1–4); ignoring");
                return;
            }
            let idx = (output as usize) - 1;
            // Retry indefinitely on "Device or resource busy" — the GL7 subprocess
            // may still hold the port fd immediately after being killed.  Any other
            // error (write failure, bad port, etc.) breaks out immediately.
            let set_result: Result<(), String> = loop {
                state.ls350.set_output_percent(output, pct);
                match state.ls350.error_message.clone() {
                    Some(e) if e.contains("Device or resource busy") => {
                        std::thread::sleep(Duration::from_millis(500));
                    }
                    Some(e) => break Err(e),
                    None    => break Ok(()),
                }
            };
            if let Err(err) = set_result {
                snap.lock().unwrap_or_else(|p| p.into_inner()).gl7_set_results[idx] = Some(Err(err));
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
                let mut s = snap.lock().unwrap_or_else(|p| p.into_inner());
                s.gl7_output_lines[idx] = (l1, l2);
                s.gl7_polled_pct[idx]   = new_pct;
                s.gl7_set_results[idx]  = Some(Ok(()));
                s.last_gl7_update       = Some(Instant::now());
            }
        }

        GuiCommand::StartRecording { interval_secs, output_dir, resume_path } => {
            // Guard: ignore if already recording.
            {
                let s = snap.lock().unwrap_or_else(|p| p.into_inner());
                if s.recording_active { return; }
            }
            let snap_cb = Arc::clone(snap);
            let ctx_cb  = ctx.clone();
            match start_recording_loop(interval_secs, &output_dir, resume_path, move |rec| {
                let temps = temps_from_record(rec);
                let mut s = snap_cb.lock().unwrap_or_else(|p| p.into_inner());
                s.temperatures     = temps;
                s.last_temp_update = Some(Instant::now());
                drop(s);
                ctx_cb.request_repaint();
            }) {
                Ok((path, stop_flag)) => {
                    state.recording_stop_flag = Some(stop_flag);
                    let mut s = snap.lock().unwrap_or_else(|p| p.into_inner());
                    s.recording_active       = true;
                    s.recording_csv_path     = Some(path.clone());
                    s.recording_start_result = Some(Ok(path));
                }
                Err(e) => {
                    snap.lock().unwrap_or_else(|p| p.into_inner())
                        .recording_start_result = Some(Err(e));
                }
            }
        }

        GuiCommand::StopRecording => {
            if let Some(flag) = state.recording_stop_flag.take() {
                flag.store(true, Ordering::Relaxed);
            }
            let mut s = snap.lock().unwrap_or_else(|p| p.into_inner());
            s.recording_active = false;
        }

        GuiCommand::StopAdrRamp => {
            // Subprocess already killed by GUI. Set magnet to 0 and clean up.
            let _ = state.ls625.set_current(0.0);
            clear_adr_ramp_persisted();
            let _ = fs::remove_file(ADR_RAMP_STOP_PATH);
            let mut s = snap.lock().unwrap_or_else(|p| p.into_inner());
            s.adr_ramp_running     = false;
            s.adr_ramp_was_stopped = true;
            s.adr_status_line.clear();
            state.adr_ramp_log_offset = 0;
        }

        GuiCommand::Gl7CooldownActive(active) => {
            snap.lock().unwrap_or_else(|p| p.into_inner()).gl7_cooldown_active = active;
        }

        GuiCommand::RunAdrRamp => {
            // Guard: ignore if a ramp is already in progress.
            {
                let s = snap.lock().unwrap_or_else(|p| p.into_inner());
                if s.adr_ramp_running { return; }
            }
            // The GUI has already spawned the subprocess.  Clear old log/status
            // files and flip the snapshot flag so the Stop button shows immediately.
            let _ = fs::write(ADR_RAMP_LOG_PATH, "");
            let _ = fs::write(ADR_RAMP_STATUS_PATH, "");
            let _ = fs::remove_file(ADR_RAMP_RESULT_PATH);
            let _ = fs::remove_file(ADR_RAMP_STOP_PATH);
            let mut s = snap.lock().unwrap_or_else(|p| p.into_inner());
            s.adr_ramp_running     = true;
            s.adr_ramp_started     = Some(Instant::now());
            s.adr_ramp_result      = None;
            s.adr_ramp_was_stopped = false;
            s.adr_log_lines.clear();
            s.adr_status_line.clear();
            state.adr_ramp_log_offset = 0;
        }
    }
}

// ── ADR ramp file polling ──────────────────────────────────────

fn poll_adr_ramp(state: &mut WorkerState, snap: &Arc<Mutex<DeviceSnapshot>>, ctx: &egui::Context) {
    let ramp_running = snap.lock().unwrap_or_else(|p| p.into_inner()).adr_ramp_running;
    if !ramp_running {
        return;
    }

    // Poll log file for new lines since last read.
    if let Ok(content) = fs::read_to_string(ADR_RAMP_LOG_PATH) {
        if content.len() > state.adr_ramp_log_offset {
            let new_lines: Vec<String> = content[state.adr_ramp_log_offset..]
                .lines()
                .map(|l| l.to_string())
                .collect();
            if !new_lines.is_empty() {
                snap.lock().unwrap_or_else(|p| p.into_inner())
                    .adr_log_lines.extend(new_lines);
                ctx.request_repaint();
            }
            state.adr_ramp_log_offset = content.len();
        }
    }

    // Poll status file (live countdown / polling readout).
    if let Ok(status) = fs::read_to_string(ADR_RAMP_STATUS_PATH) {
        let status = status.trim_end_matches('\n').to_string();
        let mut s = snap.lock().unwrap_or_else(|p| p.into_inner());
        if s.adr_status_line != status {
            s.adr_status_line = status;
            drop(s);
            ctx.request_repaint();
        }
    }

    // Detect natural subprocess exit via PID liveness check.
    if get_adr_ramp_pid().is_none() {
        let result: Result<(), String> = match fs::read_to_string(ADR_RAMP_RESULT_PATH) {
            Ok(s) if s.trim() == "ok"           => Ok(()),
            Ok(s) if s.starts_with("error:")    => Err(s[6..].trim().to_string()),
            _                                    => Ok(()),
        };
        let mut s = snap.lock().unwrap_or_else(|p| p.into_inner());
        if s.adr_ramp_running {
            s.adr_ramp_running = false;
            if !s.adr_ramp_was_stopped {
                s.adr_ramp_result = Some(result);
            }
            s.adr_status_line.clear();
            state.adr_ramp_log_offset = 0;
        }
        drop(s);
        ctx.request_repaint();
    }
}

// ── Recording helpers ──────────────────────────────────────────

fn format_opt_kelvin(v: Option<f64>) -> String {
    match v {
        Some(k) if k > 0.0 => format!("{k:.4} K"),
        Some(_)             => "T_OVER".to_string(),
        None                => "---".to_string(),
    }
}

fn temps_from_record(rec: &TemperatureRecord) -> TemperatureReadings {
    TemperatureReadings {
        ls350_a:  format_opt_kelvin(rec.a_temp_k),
        ls350_b:  format_opt_kelvin(rec.b_temp_k),
        ls350_c:  format_opt_kelvin(rec.c_temp_k),
        ls350_d2: format_opt_kelvin(rec.d2_temp_k),
        ls350_d3: format_opt_kelvin(rec.d3_temp_k),
        ls350_d4: format_opt_kelvin(rec.d4_temp_k),
        ls350_d5: format_opt_kelvin(rec.d5_temp_k),
        ls370_1:  format_opt_kelvin(rec.ls370_temp_k),
    }
}

// ── Poll functions ─────────────────────────────────────────────

fn poll_compressor(state: &mut WorkerState, snap: &Arc<Mutex<DeviceSnapshot>>) {
    state.compressor.get_status();
    let mut s = snap.lock().unwrap_or_else(|p| p.into_inner());
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

    let mut s = snap.lock().unwrap_or_else(|p| p.into_inner());
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
    // Seed from the last known-good values so a transient serial failure on
    // one output doesn't wipe out the other successfully-polled values.
    let (mut lines, mut pcts) = {
        let s = snap.lock().unwrap_or_else(|p| p.into_inner());
        (s.gl7_output_lines.clone(), s.gl7_polled_pct.clone())
    };

    for (i, &output_num) in [1u8, 2, 3, 4].iter().enumerate() {
        state.ls350.query_output_percentages(output_num);
        if state.ls350.error_message.is_none() {
            let mut ls = state.ls350.output.lines();
            let l1 = ls.next().unwrap_or("").to_string();
            let l2 = ls.next().unwrap_or("").to_string();
            pcts[i]  = l1.split_whitespace().last().and_then(|s| s.parse::<f64>().ok());
            lines[i] = (l1, l2);
        }
        // On failure: leave pcts[i] and lines[i] at their previous values.
    }

    let mut s = snap.lock().unwrap_or_else(|p| p.into_inner());
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

    let mut s = snap.lock().unwrap_or_else(|p| p.into_inner());
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
