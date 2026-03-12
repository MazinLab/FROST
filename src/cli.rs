// cli.rs — FROST command-line interface
//
//   frost gui
//   frost compressor [--port /dev/ttyUSB3] <command>
//   frost heatswitch [--port /dev/ttyUSB4] <command>
//   frost lakeshore625 [--port /dev/ttyUSB0] <command>
//   frost lakeshore370 [--port /dev/ttyUSB1] <command>
//   frost lakeshore350 [--port /dev/ttyUSB2] <command>
//
// Run `frost --help` or `frost <device> --help` for full option lists.

use clap::{Parser, Subcommand};

use crate::compressor::CryomechController;
use crate::heatswitch::{HeatswitchController, HEATSWITCH_TRAVEL_STEPS};
use crate::lakeshore625::LakeShore625Controller;
use crate::lakeshore370::LakeShore370Controller;
use crate::lakeshore350::LakeShore350Controller;

// ── Top-level CLI ─────────────────────────────────────────────
#[derive(Parser)]
#[command(name = "frost")]
#[command(about = "FROST — Fridge Remote Operations, Software, and Thermometry")]
#[command(version)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    device: Device,
}

#[derive(Subcommand)]
enum Device {
    /// Launch the graphical user interface
    Gui,

    /// Cryomech pulse tube compressor
    Compressor {
        #[arg(long, default_value = "/dev/ttyUSB3", help = "Serial port")]
        port: String,
        #[arg(long, default_value = "115200", help = "Baud rate")]
        baud: u32,
        #[arg(long, default_value = "16", help = "Device address")]
        addr: u8,
        #[command(subcommand)]
        command: CompressorCmd,
    },

    /// Zaber T-NM17A04 heat switch stepper motor
    Heatswitch {
        #[arg(long, default_value = "/dev/ttyUSB4", help = "Serial port")]
        port: String,
        #[arg(long, default_value = "9600", help = "Baud rate")]
        baud: u32,
        #[arg(long, default_value = "1", help = "Device ID")]
        device_id: u8,
        #[command(subcommand)]
        command: HeatswitchCmd,
    },

    /// Lake Shore 625 superconducting magnet power supply
    Lakeshore625 {
        #[arg(long, default_value = "/dev/ttyUSB0", help = "Serial port")]
        port: String,
        #[command(subcommand)]
        command: Lakeshore625Cmd,
    },

    /// Lake Shore 370 AC resistance bridge
    Lakeshore370 {
        #[arg(long, default_value = "/dev/ttyUSB1", help = "Serial port")]
        port: String,
        #[arg(long, default_value = "9600", help = "Baud rate")]
        baud: u32,
        #[command(subcommand)]
        command: Lakeshore370Cmd,
    },

    /// Lake Shore 350 temperature controller
    Lakeshore350 {
        #[arg(long, default_value = "/dev/ttyUSB2", help = "Serial port")]
        port: String,
        #[arg(long, default_value = "57600", help = "Baud rate")]
        baud: u32,
        #[command(subcommand)]
        command: Lakeshore350Cmd,
    },

    /// Record temperatures from LS350 and LS370 to a date-stamped CSV in FROST/temps
    RecordTemps {
        #[command(subcommand)]
        command: RecordTempsCmd,
    },
}

// ── Compressor subcommands ────────────────────────────────────
#[derive(Subcommand)]
enum CompressorCmd {
    /// Get compressor status (running, runtime, errors)
    Status,
    /// Start the compressor
    Start,
    /// Stop the compressor
    Stop,
    /// Get all temperature readings with min/max
    Temperature,
    /// Get all pressure readings with min/max
    Pressure,
    /// Get system info (firmware checksum, CPU temp, clock battery)
    System,
    /// Get all readings (status + system + temps + pressures)
    All,
    /// Clear min/max pressure and temperature records
    ClearMinMax,
}

// ── Heat switch subcommands ───────────────────────────────────
#[derive(Subcommand)]
enum HeatswitchCmd {
    /// Get full motor status (position, limits, speed, device status)
    Status,
    /// Get current motor position (microsteps)
    Position,
    /// Open heat switch (CW 115200 steps — returns immediately)
    Open,
    /// Close heat switch (CCW 115200 steps — returns immediately)
    Close,
    /// Home the motor
    Home,
    /// Force reset by re-homing (use when stuck at a limit)
    Reset,
    /// Stop motor
    Stop,
    /// Emergency stop — sends 3 stop commands in quick succession
    Estop,
    /// Move to absolute position (microsteps)
    MoveAbs { position: i32 },
    /// Move relative to current position (positive = CW, negative = CCW)
    MoveRel { steps: i32 },
    /// Rotate clockwise by N steps
    Cw { steps: i32 },
    /// Rotate counter-clockwise by N steps
    Ccw { steps: i32 },
    /// Safe CW move (clamped to 1–1000 steps to prevent damage)
    SafeCw { steps: i32 },
    /// Safe CCW move (clamped to 1–1000 steps to prevent damage)
    SafeCcw { steps: i32 },
    /// Move at constant velocity (send `stop` to halt)
    MoveVel { velocity: i32 },
}

// ── LakeShore 625 subcommands ─────────────────────────────────
#[derive(Subcommand)]
enum Lakeshore625Cmd {
    /// Get device identification (*IDN?)
    Identify,
    /// Get current baud rate setting (BAUD?)
    Baud,
    /// Get magnetic field reading in Tesla (RDGF?)
    Field,
    /// Get output current in Amps (RDGI?)
    Current,
    /// Get output voltage in Volts (RDGV?)
    Voltage,
    /// Get field, current, and voltage together
    All,
    /// Set target output current in Amps (SETI)
    SetCurrent { current: f64 },
    /// Get current ramp rate in A/s (RATE?)
    GetRate,
    /// Set ramp rate in A/s (RATE)
    SetRate { rate: f64 },
    /// Start current ramp (RAMP)
    StartRamp,
    /// Stop / pause current ramp (STOP)
    StopRamp,
    /// Get compliance voltage limit in V (SETV?)
    GetCompliance,
    /// Set compliance voltage limit in V, 0.1–5.0 (SETV)
    SetCompliance { voltage: f64 },
    /// Get all max limits: current, voltage, rate (LIMIT?)
    GetLimits,
    /// Set all max limits: current(A) voltage(V) rate(A/s) (LIMIT)
    SetLimits { current: f64, voltage: f64, rate: f64 },
    /// Get quench detection status and step limit (QNCH?)
    QuenchStatus,
    /// Enable quench detection (QNCH 1)
    EnableQuench,
    /// Disable quench detection (QNCH 0)
    DisableQuench,
    /// Set quench detection: enable(0/1) and step_limit in A/s (QNCH)
    SetQuench { enable: u8, step_limit: f64 },
    /// Get and parse error status register (ERSTR?)
    ErrorStatus,
    /// Send a raw command string and print the response
    Raw {
        #[arg(required = true, num_args = 1..)]
        command: Vec<String>,
    },
}

// ── LakeShore 370 subcommands ────────────────────────────────
#[derive(Subcommand)]
enum Lakeshore370Cmd {
    /// Get device identification (*IDN?)
    Identify,
    /// Get baud rate code (BAUD?)
    Baud,
    /// Set baud rate code: 0=300, 1=1200, 2=9600 (BAUD)
    SetBaud { code: u8 },
    /// Read temperature in Kelvin for one input (RDGK?)
    Kelvin { input: u8 },
    /// Read resistance in Ohms for one input (RDGR?)
    Resistance { input: u8 },
    /// Read excitation power in Watts for one input (RDGPWR?)
    Power { input: u8 },
    /// Read input status byte for one input (RDGST?)
    InputStatus { input: u8 },
    /// Read temperature, resistance, and power for one input
    All { input: u8 },
    /// Get resistance range configuration for one input (RDGRNG?)
    GetRange { input: u8 },
    /// Set resistance range for one input (RDGRNG)
    SetRange {
        input: u8,
        /// 0=manual, 1=current excitation, 2=voltage excitation
        mode: u8,
        /// Excitation level 1–22
        excitation: u8,
        /// Range code 1–22
        range: u8,
        /// 0=off, 1=on
        autorange: u8,
        /// 0=current source on, 1=off
        cs_off: u8,
    },
    /// Get heater output percentage (HTR?)
    GetHeater,
    /// Set heater output percentage 0.0–100.0 (MOUT)
    SetHeater { percent: f64 },
    /// Get heater range (HTRRNG?)
    GetHeaterRange,
    /// Set heater range 0–8 (HTRRNG)
    SetHeaterRange { range: u8 },
    /// Get heater status register (HTRST?)
    HeaterStatus,
    /// Get analog output configuration for channel 1 or 2 (ANALOG?)
    GetAnalog { channel: u8 },
    /// Get analog output current value (%) for channel 1 or 2 (AOUT?)
    AnalogOutput { channel: u8 },
    /// Turn analog output off for channel 1 or 2 (ANALOG … mode=0)
    SetAnalogOff { channel: u8 },
    /// Set analog output to channel-monitor mode (ANALOG … mode=1)
    SetAnalogChannel {
        channel: u8,
        /// 0=unipolar, 1=bipolar
        polarity: u8,
        /// Input channel 1–16 to monitor
        input: u8,
        /// 1=Kelvin, 2=Ohms, 3=Linear Data
        data_source: u8,
        high_value: f64,
        low_value: f64,
    },
    /// Set analog output to manual mode (ANALOG … mode=2)
    SetAnalogManual {
        channel: u8,
        /// 0=unipolar, 1=bipolar
        polarity: u8,
        manual_value: f64,
    },
    /// Set analog output 2 to still-heater mode (ANALOG … mode=4)
    SetAnalogStill {
        /// 0=unipolar, 1=bipolar
        polarity: u8,
    },
    /// Send a raw command string and print the response
    Raw {
        #[arg(required = true, num_args = 1..)]
        command: Vec<String>,
    },
}

// ── LakeShore 350 subcommands ────────────────────────────────
#[derive(Subcommand)]
enum Lakeshore350Cmd {
    /// Get device identification (*IDN?)
    Identify,
    /// Read sensor/temperature for one input: A, B, C, D1–D5 (intelligent reading)
    Read { input: String },
    /// Read all key inputs: A (3-head), B (ADR), C (4-head), D3 (4K stage), D4 (3-pump), D5 (4-pump)
    All,
    /// Get front panel display name for one input (INNAME?)
    /// Valid inputs: A, B, C, D1, D2, D3, D4, D5
    DisplayShow { input: String },
    /// Get front panel display names for all inputs (INNAME? A … D5)
    DisplayShowAll,
    /// Set front panel display name for one input (INNAME)
    /// Valid inputs: A, B, C, D1, D2, D3, D4, D5
    DisplaySetName {
        input: String,
        name: String,
    },
    /// Set manual output percentage 0–100 (MOUT)
    SetOutput {
        output: u8,
        percent: f64,
    },
    /// Query output status for one output (MOUT?, HTR?/HTRSET? or AOUT?/ANALOG?, OUTMODE?, RANGE?)
    QueryOutput {
        output: u8,
    },
    /// Query output status for all outputs 1–4
    QueryAllOutputs,
    /// Set output configuration parameters (HTRSET or ANALOG)
    /// For output 1–2: <resistance>,<max current>,<max user current>,<current/power>
    /// For output 3–4: <input>,<units>,<high value>,<low value>,<polarity>
    OutputsSetParams {
        output: u8,
        #[arg(required = true, num_args = 1..)]
        params: Vec<String>,
    },
    /// Set output range (RANGE)
    OutputsSetRange {
        output: u8,
        range: i32,
    },
    /// Send a raw command string and print the response
    Raw {
        #[arg(required = true, num_args = 1..)]
        command: Vec<String>,
    },
}

// ── RecordTemps subcommands ───────────────────────────────────────
#[derive(Subcommand)]
enum RecordTempsCmd {
    /// Take a single temperature snapshot and append one row to today's CSV
    Snapshot,
    /// Record temperatures continuously (one row every N seconds) until Ctrl+C
    Loop {
        /// Recording interval in seconds (default: 30)
        #[arg(long, default_value = "30")]
        interval: u64,
    },
}

// ── Entry point ───────────────────────────────────────────────
pub fn run() -> Result<(), String> {
    let cli = Cli::parse();

    match cli.device {
        Device::Gui => {
            crate::gui::run().map_err(|e| e.to_string())
        }
        Device::Compressor { port, baud, addr, command } => {
            let mut ctrl = CryomechController::default();
            ctrl.port = port;
            ctrl.baud_rate = baud;
            ctrl.device_addr = addr;
            run_compressor(&mut ctrl, command)
        }
        Device::Heatswitch { port, baud, device_id, command } => {
            let mut ctrl = HeatswitchController::default();
            ctrl.port = port;
            ctrl.baud_rate = baud;
            ctrl.device_id = device_id;
            run_heatswitch(&mut ctrl, command)
        }
        Device::Lakeshore625 { port, command } => {
            let mut ctrl = LakeShore625Controller::default();
            ctrl.port = port;
            run_lakeshore625(&mut ctrl, command)
        }
        Device::Lakeshore370 { port, baud, command } => {
            let mut ctrl = LakeShore370Controller::default();
            ctrl.port = port;
            ctrl.baud_rate = baud;
            run_lakeshore370(&mut ctrl, command)
        }
        Device::Lakeshore350 { port, baud, command } => {
            let mut ctrl = LakeShore350Controller::default();
            ctrl.port = port;
            ctrl.baud_rate = baud;
            run_lakeshore350(&mut ctrl, command)
        }
        Device::RecordTemps { command } => {
            run_record_temps(command)
        }
    }
}

// ── Compressor dispatch ───────────────────────────────────────
fn run_compressor(ctrl: &mut CryomechController, cmd: CompressorCmd) -> Result<(), String> {
    match cmd {
        CompressorCmd::Status => {
            ctrl.get_status();
            print_ctrl(&ctrl.status_output, &ctrl.error_message)
        }
        CompressorCmd::Start => {
            ctrl.start_compressor()?;
            println!("Compressor started successfully.");
            ctrl.get_status();
            print!("{}", ctrl.status_output);
            Ok(())
        }
        CompressorCmd::Stop => {
            ctrl.stop_compressor()?;
            println!("Compressor stopped successfully.");
            ctrl.get_status();
            print!("{}", ctrl.status_output);
            Ok(())
        }
        CompressorCmd::Temperature => {
            ctrl.get_temperature()?;
            print!("{}", ctrl.all_output);
            Ok(())
        }
        CompressorCmd::Pressure => {
            ctrl.get_pressure()?;
            print!("{}", ctrl.all_output);
            Ok(())
        }
        CompressorCmd::System => {
            ctrl.get_system_info()?;
            print!("{}", ctrl.all_output);
            Ok(())
        }
        CompressorCmd::All => {
            ctrl.get_all_readings();
            print_ctrl(&ctrl.all_output, &ctrl.error_message)
        }
        CompressorCmd::ClearMinMax => {
            ctrl.clear_min_max()?;
            println!("Min/max values cleared.");
            Ok(())
        }
    }
}

// ── Heat switch dispatch ──────────────────────────────────────
fn run_heatswitch(ctrl: &mut HeatswitchController, cmd: HeatswitchCmd) -> Result<(), String> {
    match cmd {
        HeatswitchCmd::Status => {
            ctrl.get_status();
            print_ctrl(&ctrl.status_output, &ctrl.error_message)
        }
        HeatswitchCmd::Position => {
            ctrl.get_position();
            print_ctrl(&ctrl.status_output, &ctrl.error_message)
        }
        HeatswitchCmd::Open => {
            ctrl.open()?;
            println!("Open command sent (CW {} steps). Motor is moving.", HEATSWITCH_TRAVEL_STEPS);
            Ok(())
        }
        HeatswitchCmd::Close => {
            ctrl.close()?;
            println!("Close command sent (CCW {} steps). Motor is moving.", HEATSWITCH_TRAVEL_STEPS);
            Ok(())
        }
        HeatswitchCmd::Home => {
            ctrl.home()?;
            println!("Home command sent.");
            Ok(())
        }
        HeatswitchCmd::Reset => {
            ctrl.reset()?;
            println!("Reset (home) command sent.");
            Ok(())
        }
        HeatswitchCmd::Stop => {
            ctrl.stop()?;
            println!("Stop command sent.");
            Ok(())
        }
        HeatswitchCmd::Estop => {
            ctrl.emergency_stop()?;
            println!("Emergency stop sent (3x).");
            Ok(())
        }
        HeatswitchCmd::MoveAbs { position } => {
            ctrl.move_absolute(position)?;
            println!("Move absolute {} sent.", position);
            Ok(())
        }
        HeatswitchCmd::MoveRel { steps } => {
            ctrl.move_relative(steps)?;
            println!("Move relative {} sent.", steps);
            Ok(())
        }
        HeatswitchCmd::Cw { steps } => {
            ctrl.rotate_cw(steps)?;
            println!("CW {} steps sent.", steps);
            Ok(())
        }
        HeatswitchCmd::Ccw { steps } => {
            ctrl.rotate_ccw(steps)?;
            println!("CCW {} steps sent.", steps);
            Ok(())
        }
        HeatswitchCmd::SafeCw { steps } => {
            ctrl.safe_cw(steps)?;
            println!("Safe CW {} steps sent.", steps.clamp(1, 1000));
            Ok(())
        }
        HeatswitchCmd::SafeCcw { steps } => {
            ctrl.safe_ccw(steps)?;
            println!("Safe CCW {} steps sent.", steps.clamp(1, 1000));
            Ok(())
        }
        HeatswitchCmd::MoveVel { velocity } => {
            ctrl.move_velocity(velocity)?;
            println!("Moving at velocity {}. Run 'frost heatswitch stop' to halt.", velocity);
            Ok(())
        }
    }
}

// ── LakeShore 625 dispatch ────────────────────────────────────
fn run_lakeshore625(ctrl: &mut LakeShore625Controller, cmd: Lakeshore625Cmd) -> Result<(), String> {
    match cmd {
        Lakeshore625Cmd::Identify => {
            ctrl.get_identification();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore625Cmd::Baud => {
            ctrl.get_baud_rate();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore625Cmd::Field => {
            let v = ctrl.get_field()?;
            println!("Field:   {} T", v);
            Ok(())
        }
        Lakeshore625Cmd::Current => {
            let v = ctrl.get_current()?;
            println!("Current: {} A", v);
            Ok(())
        }
        Lakeshore625Cmd::Voltage => {
            let v = ctrl.get_voltage()?;
            println!("Voltage: {} V", v);
            Ok(())
        }
        Lakeshore625Cmd::All => {
            ctrl.get_all_readings();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore625Cmd::SetCurrent { current } => {
            ctrl.set_current(current)?;
            println!("Target current set to {} A.", current);
            Ok(())
        }
        Lakeshore625Cmd::GetRate => {
            ctrl.get_ramp_rate();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore625Cmd::SetRate { rate } => {
            ctrl.set_ramp_rate(rate)?;
            println!("Ramp rate set to {} A/s.", rate);
            Ok(())
        }
        Lakeshore625Cmd::StartRamp => {
            ctrl.start_ramp()?;
            println!("Ramp started.");
            Ok(())
        }
        Lakeshore625Cmd::StopRamp => {
            ctrl.stop_ramp()?;
            println!("Ramp stopped.");
            Ok(())
        }
        Lakeshore625Cmd::GetCompliance => {
            ctrl.get_compliance_voltage();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore625Cmd::SetCompliance { voltage } => {
            ctrl.set_compliance_voltage(voltage)?;
            println!("Compliance voltage set to {} V.", voltage);
            Ok(())
        }
        Lakeshore625Cmd::GetLimits => {
            ctrl.get_limits();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore625Cmd::SetLimits { current, voltage, rate } => {
            ctrl.set_limits(current, voltage, rate)?;
            println!("Limits set: current={} A, voltage={} V, rate={} A/s.", current, voltage, rate);
            Ok(())
        }
        Lakeshore625Cmd::QuenchStatus => {
            ctrl.get_quench_status();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore625Cmd::EnableQuench => {
            ctrl.set_quench_enable(true)?;
            println!("Quench detection enabled.");
            Ok(())
        }
        Lakeshore625Cmd::DisableQuench => {
            ctrl.set_quench_enable(false)?;
            println!("Quench detection disabled.");
            Ok(())
        }
        Lakeshore625Cmd::SetQuench { enable, step_limit } => {
            ctrl.set_quench_detection(enable != 0, step_limit)?;
            println!("Quench set: enable={}, step_limit={} A/s.", enable, step_limit);
            Ok(())
        }
        Lakeshore625Cmd::ErrorStatus => {
            ctrl.get_error_status();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore625Cmd::Raw { command } => {
            let cmd_str = command.join(" ");
            ctrl.raw_command(&cmd_str);
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
    }
}

// ── LakeShore 370 dispatch ───────────────────────────────────
fn run_lakeshore370(ctrl: &mut LakeShore370Controller, cmd: Lakeshore370Cmd) -> Result<(), String> {
    match cmd {
        Lakeshore370Cmd::Identify => {
            ctrl.get_identification();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore370Cmd::Baud => {
            ctrl.get_baud_rate();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore370Cmd::SetBaud { code } => {
            ctrl.set_baud_rate(code)?;
            println!("Baud rate code set to {code}.");
            Ok(())
        }
        Lakeshore370Cmd::Kelvin { input } => {
            let v = ctrl.read_kelvin(input)?;
            println!("Input {input} temperature: {v} K");
            Ok(())
        }
        Lakeshore370Cmd::Resistance { input } => {
            let v = ctrl.read_resistance(input)?;
            println!("Input {input} resistance: {v} Ω");
            Ok(())
        }
        Lakeshore370Cmd::Power { input } => {
            let v = ctrl.read_excitation_power(input)?;
            println!("Input {input} excitation power: {v} W");
            Ok(())
        }
        Lakeshore370Cmd::InputStatus { input } => {
            let v = ctrl.read_status(input)?;
            println!("Input {input} status: {v}");
            Ok(())
        }
        Lakeshore370Cmd::All { input } => {
            ctrl.get_all_readings(input);
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore370Cmd::GetRange { input } => {
            ctrl.get_resistance_range(input);
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore370Cmd::SetRange { input, mode, excitation, range, autorange, cs_off } => {
            ctrl.set_resistance_range(input, mode, excitation, range, autorange, cs_off)?;
            println!("Resistance range set for input {input}.");
            Ok(())
        }
        Lakeshore370Cmd::GetHeater => {
            ctrl.get_heater_output();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore370Cmd::SetHeater { percent } => {
            ctrl.set_heater_output(percent)?;
            println!("Heater output set to {percent:.3}%.");
            Ok(())
        }
        Lakeshore370Cmd::GetHeaterRange => {
            ctrl.get_heater_range();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore370Cmd::SetHeaterRange { range } => {
            ctrl.set_heater_range(range)?;
            println!("Heater range set to {range}.");
            Ok(())
        }
        Lakeshore370Cmd::HeaterStatus => {
            ctrl.get_heater_status();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore370Cmd::GetAnalog { channel } => {
            ctrl.get_analog_config(channel);
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore370Cmd::AnalogOutput { channel } => {
            ctrl.get_analog_output(channel);
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore370Cmd::SetAnalogOff { channel } => {
            ctrl.set_analog_off(channel)?;
            println!("Analog output {channel} turned off.");
            Ok(())
        }
        Lakeshore370Cmd::SetAnalogChannel { channel, polarity, input, data_source, high_value, low_value } => {
            ctrl.set_analog_channel_mode(channel, polarity, input, data_source, high_value, low_value)?;
            println!("Analog output {channel} set to channel-monitor mode.");
            Ok(())
        }
        Lakeshore370Cmd::SetAnalogManual { channel, polarity, manual_value } => {
            ctrl.set_analog_manual_mode(channel, polarity, manual_value)?;
            println!("Analog output {channel} set to manual mode ({manual_value}).");
            Ok(())
        }
        Lakeshore370Cmd::SetAnalogStill { polarity } => {
            ctrl.set_analog_still_mode(polarity)?;
            println!("Analog output 2 set to still-heater mode.");
            Ok(())
        }
        Lakeshore370Cmd::Raw { command } => {
            let cmd_str = command.join(" ");
            ctrl.raw_command(&cmd_str);
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
    }
}

// ── LakeShore 350 dispatch ────────────────────────────────────
fn run_lakeshore350(ctrl: &mut LakeShore350Controller, cmd: Lakeshore350Cmd) -> Result<(), String> {
    match cmd {
        Lakeshore350Cmd::Identify => {
            ctrl.get_identification();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore350Cmd::Read { input } => {
            ctrl.read_input_intelligent(&input);
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore350Cmd::All => {
            ctrl.get_all_readings();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore350Cmd::DisplayShow { input } => {
            ctrl.get_display_name(&input);
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore350Cmd::DisplayShowAll => {
            ctrl.get_all_display_names();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore350Cmd::DisplaySetName { input, name } => {
            ctrl.set_display_name(&input, &name)?;
            println!("Display name for input {} set to '{}'.", input, name);
            Ok(())
        }
        Lakeshore350Cmd::SetOutput { output, percent } => {
            ctrl.set_output_percent(output, percent);
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore350Cmd::QueryOutput { output } => {
            ctrl.query_output(output);
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore350Cmd::QueryAllOutputs => {
            ctrl.query_all_outputs();
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore350Cmd::OutputsSetParams { output, params } => {
            ctrl.set_output_params(output, &params);
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore350Cmd::OutputsSetRange { output, range } => {
            ctrl.set_output_range(output, range);
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
        Lakeshore350Cmd::Raw { command } => {
            let cmd_str = command.join(" ");
            ctrl.raw_command(&cmd_str);
            print_ctrl(&ctrl.output, &ctrl.error_message)
        }
    }
}

// ── Shared helper ─────────────────────────────────────────────
fn print_ctrl(output: &str, error: &Option<String>) -> Result<(), String> {
    if let Some(e) = error {
        return Err(e.clone());
    }
    print!("{}", output);
    Ok(())
}
// ── RecordTemps dispatch ────────────────────────────────────────
fn run_record_temps(cmd: RecordTempsCmd) -> Result<(), String> {
    // Use the fixed defaults from each controller module
    let mut ls350 = LakeShore350Controller::default();
    let mut ls370 = LakeShore370Controller::default();
    const OUTPUT_DIR: &str = "temps";

    match cmd {
        RecordTempsCmd::Snapshot => {
            let msg = crate::record_temps::record_single_snapshot(
                &mut ls350, &mut ls370, OUTPUT_DIR,
            )?;
            println!("{}", msg);
            let record = crate::record_temps::take_snapshot(&mut ls350, &mut ls370);
            print!("{}", record.to_display());
            Ok(())
        }
        RecordTempsCmd::Loop { interval } => {
            crate::record_temps::run_recording_loop(
                &ls350.port, ls350.baud_rate,
                &ls370.port, ls370.baud_rate,
                interval,
                OUTPUT_DIR,
            );
            Ok(())
        }
    }
}