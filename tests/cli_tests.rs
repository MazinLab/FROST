// cli_tests.rs — CLI argument parsing tests for FROST
//
// These tests verify that clap correctly parses all subcommands and their
// arguments. No serial I/O occurs; `try_parse_from` only parses args.
//
// Run with: cargo test --test cli_tests

use clap::Parser;
use frost::cli::{
    AdrCmd, Cli, CompressorCmd, Device, Gl7Cmd, HeatswitchCmd, Lakeshore350Cmd, Lakeshore370Cmd,
    Lakeshore625Cmd, RecordTempsCmd,
};

// ── Helpers ────────────────────────────────────────────────────────────────

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).expect("parse should succeed")
}

fn parse_fails(args: &[&str]) {
    assert!(
        Cli::try_parse_from(args).is_err(),
        "expected parse failure for: {:?}",
        args
    );
}

// ── Default ports and baud rates ───────────────────────────────────────────

#[test]
fn ls625_default_port_is_ttyusb0() {
    let cli = parse(&["frost", "lakeshore625", "identify"]);
    let Device::Lakeshore625 { port, .. } = cli.device else { panic!() };
    assert_eq!(port, "/dev/ttyUSB0");
}

#[test]
fn ls370_default_port_is_ttyusb1() {
    let cli = parse(&["frost", "lakeshore370", "identify"]);
    let Device::Lakeshore370 { port, baud, .. } = cli.device else { panic!() };
    assert_eq!(port, "/dev/ttyUSB1");
    assert_eq!(baud, 9600);
}

#[test]
fn ls350_default_port_is_ttyusb2_and_baud_57600() {
    let cli = parse(&["frost", "lakeshore350", "identify"]);
    let Device::Lakeshore350 { port, baud, .. } = cli.device else { panic!() };
    assert_eq!(port, "/dev/ttyUSB2");
    assert_eq!(baud, 57600);
}

#[test]
fn compressor_default_port_is_ttyusb3() {
    let cli = parse(&["frost", "compressor", "status"]);
    let Device::Compressor { port, baud, addr, .. } = cli.device else { panic!() };
    assert_eq!(port, "/dev/ttyUSB3");
    assert_eq!(baud, 115200);
    assert_eq!(addr, 16);
}

#[test]
fn heatswitch_default_port_is_ttyusb4() {
    let cli = parse(&["frost", "heatswitch", "status"]);
    let Device::Heatswitch { port, baud, device_id, .. } = cli.device else { panic!() };
    assert_eq!(port, "/dev/ttyUSB4");
    assert_eq!(baud, 9600);
    assert_eq!(device_id, 1);
}

// ── Custom port / baud overrides ───────────────────────────────────────────

#[test]
fn ls625_custom_port_overrides_default() {
    let cli = parse(&["frost", "lakeshore625", "--port", "/dev/ttyUSB9", "identify"]);
    let Device::Lakeshore625 { port, .. } = cli.device else { panic!() };
    assert_eq!(port, "/dev/ttyUSB9");
}

#[test]
fn ls370_custom_port_and_baud() {
    let cli = parse(&["frost", "lakeshore370", "--port", "/dev/ttyACM0", "--baud", "4800", "baud"]);
    let Device::Lakeshore370 { port, baud, .. } = cli.device else { panic!() };
    assert_eq!(port, "/dev/ttyACM0");
    assert_eq!(baud, 4800);
}

#[test]
fn ls350_custom_port_and_baud() {
    let cli = parse(&["frost", "lakeshore350", "--port", "/dev/ttyACM1", "--baud", "9600", "identify"]);
    let Device::Lakeshore350 { port, baud, .. } = cli.device else { panic!() };
    assert_eq!(port, "/dev/ttyACM1");
    assert_eq!(baud, 9600);
}

// ── GUI ────────────────────────────────────────────────────────────────────

#[test]
fn gui_subcommand_parses() {
    let cli = parse(&["frost", "gui"]);
    assert!(matches!(cli.device, Device::Gui));
}

// ── ADR ────────────────────────────────────────────────────────────────────

#[test]
fn adr_ramp_parses_rate_and_current() {
    let cli = parse(&["frost", "adr", "ramp", "0.05", "8.0"]);
    let Device::Adr { command } = cli.device else { panic!() };
    let AdrCmd::Ramp { rate, current, soak_mins } = command else { panic!() };
    assert_eq!(rate, 0.05);
    assert_eq!(current, 8.0);
    assert_eq!(soak_mins, 45); // default
}

#[test]
fn adr_ramp_custom_soak_mins() {
    let cli = parse(&["frost", "adr", "ramp", "0.05", "8.0", "--soak-mins", "60"]);
    let Device::Adr { command } = cli.device else { panic!() };
    let AdrCmd::Ramp { soak_mins, .. } = command else { panic!() };
    assert_eq!(soak_mins, 60);
}

#[test]
fn adr_ramp_missing_current_fails() {
    parse_fails(&["frost", "adr", "ramp", "0.05"]);
}

#[test]
fn adr_ramp_missing_both_args_fails() {
    parse_fails(&["frost", "adr", "ramp"]);
}

#[test]
fn adr_logging_parses() {
    let cli = parse(&["frost", "adr", "logging"]);
    let Device::Adr { command } = cli.device else { panic!() };
    assert!(matches!(command, AdrCmd::Logging));
}

// ── Compressor ─────────────────────────────────────────────────────────────

#[test]
fn compressor_all_zero_arg_subcommands_parse() {
    let cmds = ["status", "start", "stop", "temperature", "pressure", "system", "all", "clear-min-max"];
    for cmd in cmds {
        parse(&["frost", "compressor", cmd]);
    }
}

#[test]
fn compressor_clear_min_max_parses() {
    let cli = parse(&["frost", "compressor", "clear-min-max"]);
    let Device::Compressor { command, .. } = cli.device else { panic!() };
    assert!(matches!(command, CompressorCmd::ClearMinMax));
}

// ── Heatswitch ─────────────────────────────────────────────────────────────

#[test]
fn heatswitch_zero_arg_subcommands_parse() {
    let cmds = ["status", "position", "open", "close", "home", "reset", "stop", "estop"];
    for cmd in cmds {
        parse(&["frost", "heatswitch", cmd]);
    }
}

#[test]
fn heatswitch_move_abs_parses() {
    let cli = parse(&["frost", "heatswitch", "move-abs", "115200"]);
    let Device::Heatswitch { command, .. } = cli.device else { panic!() };
    let HeatswitchCmd::MoveAbs { position } = command else { panic!() };
    assert_eq!(position, 115200);
}

#[test]
fn heatswitch_move_rel_parses_positive() {
    let cli = parse(&["frost", "heatswitch", "move-rel", "500"]);
    let Device::Heatswitch { command, .. } = cli.device else { panic!() };
    let HeatswitchCmd::MoveRel { steps } = command else { panic!() };
    assert_eq!(steps, 500);
}

#[test]
fn heatswitch_move_rel_negative_requires_double_dash() {
    // clap treats bare negative numbers as unknown flags; -- is required
    parse_fails(&["frost", "heatswitch", "move-rel", "-500"]);
    let cli = parse(&["frost", "heatswitch", "move-rel", "--", "-500"]);
    let Device::Heatswitch { command, .. } = cli.device else { panic!() };
    let HeatswitchCmd::MoveRel { steps } = command else { panic!() };
    assert_eq!(steps, -500);
}

#[test]
fn heatswitch_cw_parses() {
    let cli = parse(&["frost", "heatswitch", "cw", "1000"]);
    let Device::Heatswitch { command, .. } = cli.device else { panic!() };
    let HeatswitchCmd::Cw { steps } = command else { panic!() };
    assert_eq!(steps, 1000);
}

#[test]
fn heatswitch_ccw_parses() {
    let cli = parse(&["frost", "heatswitch", "ccw", "500"]);
    let Device::Heatswitch { command, .. } = cli.device else { panic!() };
    let HeatswitchCmd::Ccw { steps } = command else { panic!() };
    assert_eq!(steps, 500);
}

#[test]
fn heatswitch_safe_cw_parses() {
    let cli = parse(&["frost", "heatswitch", "safe-cw", "100"]);
    let Device::Heatswitch { command, .. } = cli.device else { panic!() };
    let HeatswitchCmd::SafeCw { steps } = command else { panic!() };
    assert_eq!(steps, 100);
}

#[test]
fn heatswitch_safe_ccw_parses() {
    let cli = parse(&["frost", "heatswitch", "safe-ccw", "200"]);
    let Device::Heatswitch { command, .. } = cli.device else { panic!() };
    let HeatswitchCmd::SafeCcw { steps } = command else { panic!() };
    assert_eq!(steps, 200);
}

#[test]
fn heatswitch_move_vel_parses() {
    let cli = parse(&["frost", "heatswitch", "move-vel", "2000"]);
    let Device::Heatswitch { command, .. } = cli.device else { panic!() };
    let HeatswitchCmd::MoveVel { velocity } = command else { panic!() };
    assert_eq!(velocity, 2000);
}

#[test]
fn heatswitch_move_abs_missing_arg_fails() {
    parse_fails(&["frost", "heatswitch", "move-abs"]);
}

// ── LakeShore 625 ──────────────────────────────────────────────────────────

#[test]
fn ls625_zero_arg_subcommands_parse() {
    let cmds = [
        "identify", "baud", "field", "current", "voltage", "all",
        "get-current", "get-rate", "start-ramp", "stop-ramp",
        "get-compliance", "get-limits", "quench-status",
        "enable-quench", "disable-quench", "error-status", "logging",
    ];
    for cmd in cmds {
        parse(&["frost", "lakeshore625", cmd]);
    }
}

#[test]
fn ls625_set_current_parses() {
    let cli = parse(&["frost", "lakeshore625", "set-current", "9.5"]);
    let Device::Lakeshore625 { command, .. } = cli.device else { panic!() };
    let Lakeshore625Cmd::SetCurrent { current } = command else { panic!() };
    assert_eq!(current, 9.5);
}

#[test]
fn ls625_set_rate_parses() {
    let cli = parse(&["frost", "lakeshore625", "set-rate", "0.05"]);
    let Device::Lakeshore625 { command, .. } = cli.device else { panic!() };
    let Lakeshore625Cmd::SetRate { rate } = command else { panic!() };
    assert_eq!(rate, 0.05);
}

#[test]
fn ls625_set_compliance_parses() {
    let cli = parse(&["frost", "lakeshore625", "set-compliance", "3.5"]);
    let Device::Lakeshore625 { command, .. } = cli.device else { panic!() };
    let Lakeshore625Cmd::SetCompliance { voltage } = command else { panic!() };
    assert_eq!(voltage, 3.5);
}

#[test]
fn ls625_set_limits_parses_all_three() {
    let cli = parse(&["frost", "lakeshore625", "set-limits", "60.0", "5.0", "1.5"]);
    let Device::Lakeshore625 { command, .. } = cli.device else { panic!() };
    let Lakeshore625Cmd::SetLimits { current, voltage, rate } = command else { panic!() };
    assert_eq!(current, 60.0);
    assert_eq!(voltage, 5.0);
    assert_eq!(rate, 1.5);
}

#[test]
fn ls625_set_quench_parses() {
    let cli = parse(&["frost", "lakeshore625", "set-quench", "1", "0.5"]);
    let Device::Lakeshore625 { command, .. } = cli.device else { panic!() };
    let Lakeshore625Cmd::SetQuench { enable, step_limit } = command else { panic!() };
    assert_eq!(enable, 1);
    assert_eq!(step_limit, 0.5);
}

#[test]
fn ls625_raw_single_token_parses() {
    let cli = parse(&["frost", "lakeshore625", "raw", "*IDN?"]);
    let Device::Lakeshore625 { command, .. } = cli.device else { panic!() };
    let Lakeshore625Cmd::Raw { command: tokens } = command else { panic!() };
    assert_eq!(tokens, vec!["*IDN?"]);
}

#[test]
fn ls625_raw_multi_token_parses() {
    let cli = parse(&["frost", "lakeshore625", "raw", "SETI", "5.0"]);
    let Device::Lakeshore625 { command, .. } = cli.device else { panic!() };
    let Lakeshore625Cmd::Raw { command: tokens } = command else { panic!() };
    assert_eq!(tokens, vec!["SETI", "5.0"]);
}

#[test]
fn ls625_set_current_missing_arg_fails() {
    parse_fails(&["frost", "lakeshore625", "set-current"]);
}

#[test]
fn ls625_set_limits_too_few_args_fails() {
    parse_fails(&["frost", "lakeshore625", "set-limits", "60.0", "5.0"]);
}

#[test]
fn ls625_raw_no_args_fails() {
    parse_fails(&["frost", "lakeshore625", "raw"]);
}

// ── LakeShore 370 ──────────────────────────────────────────────────────────

#[test]
fn ls370_zero_arg_subcommands_parse() {
    let cmds = [
        "identify", "baud", "get-heater", "get-heater-range", "heater-status",
    ];
    for cmd in cmds {
        parse(&["frost", "lakeshore370", cmd]);
    }
}

#[test]
fn ls370_set_baud_parses() {
    let cli = parse(&["frost", "lakeshore370", "set-baud", "2"]);
    let Device::Lakeshore370 { command, .. } = cli.device else { panic!() };
    let Lakeshore370Cmd::SetBaud { code } = command else { panic!() };
    assert_eq!(code, 2);
}

#[test]
fn ls370_single_input_subcommands_parse() {
    let cmds = ["kelvin", "resistance", "power", "input-status", "all", "get-range"];
    for cmd in cmds {
        let cli = parse(&["frost", "lakeshore370", cmd, "5"]);
        let Device::Lakeshore370 { command, .. } = cli.device else { panic!() };
        match command {
            Lakeshore370Cmd::Kelvin { input }
            | Lakeshore370Cmd::Resistance { input }
            | Lakeshore370Cmd::Power { input }
            | Lakeshore370Cmd::InputStatus { input }
            | Lakeshore370Cmd::All { input }
            | Lakeshore370Cmd::GetRange { input } => assert_eq!(input, 5, "failed for {cmd}"),
            _ => panic!("wrong variant for {cmd}"),
        }
    }
}

#[test]
fn ls370_set_heater_parses() {
    let cli = parse(&["frost", "lakeshore370", "set-heater", "75.5"]);
    let Device::Lakeshore370 { command, .. } = cli.device else { panic!() };
    let Lakeshore370Cmd::SetHeater { percent } = command else { panic!() };
    assert_eq!(percent, 75.5);
}

#[test]
fn ls370_set_heater_range_parses() {
    let cli = parse(&["frost", "lakeshore370", "set-heater-range", "4"]);
    let Device::Lakeshore370 { command, .. } = cli.device else { panic!() };
    let Lakeshore370Cmd::SetHeaterRange { range } = command else { panic!() };
    assert_eq!(range, 4);
}

#[test]
fn ls370_set_range_parses_all_six_args() {
    let cli = parse(&["frost", "lakeshore370", "set-range", "3", "1", "5", "10", "0", "0"]);
    let Device::Lakeshore370 { command, .. } = cli.device else { panic!() };
    let Lakeshore370Cmd::SetRange { input, mode, excitation, range, autorange, cs_off } = command
    else {
        panic!()
    };
    assert_eq!(input, 3);
    assert_eq!(mode, 1);
    assert_eq!(excitation, 5);
    assert_eq!(range, 10);
    assert_eq!(autorange, 0);
    assert_eq!(cs_off, 0);
}

#[test]
fn ls370_single_channel_subcommands_parse() {
    let cmds = ["get-analog", "analog-output", "set-analog-off"];
    for cmd in cmds {
        let cli = parse(&["frost", "lakeshore370", cmd, "2"]);
        let Device::Lakeshore370 { command, .. } = cli.device else { panic!() };
        match command {
            Lakeshore370Cmd::GetAnalog { channel }
            | Lakeshore370Cmd::AnalogOutput { channel }
            | Lakeshore370Cmd::SetAnalogOff { channel } => {
                assert_eq!(channel, 2, "failed for {cmd}")
            }
            _ => panic!("wrong variant for {cmd}"),
        }
    }
}

#[test]
fn ls370_set_analog_channel_parses_all_args() {
    let cli = parse(&[
        "frost", "lakeshore370", "set-analog-channel",
        "1", "0", "3", "1", "300.0", "0.0",
    ]);
    let Device::Lakeshore370 { command, .. } = cli.device else { panic!() };
    let Lakeshore370Cmd::SetAnalogChannel {
        channel,
        polarity,
        input,
        data_source,
        high_value,
        low_value,
    } = command
    else {
        panic!()
    };
    assert_eq!(channel, 1);
    assert_eq!(polarity, 0);
    assert_eq!(input, 3);
    assert_eq!(data_source, 1);
    assert_eq!(high_value, 300.0);
    assert_eq!(low_value, 0.0);
}

#[test]
fn ls370_set_analog_manual_parses() {
    let cli = parse(&["frost", "lakeshore370", "set-analog-manual", "2", "1", "50.0"]);
    let Device::Lakeshore370 { command, .. } = cli.device else { panic!() };
    let Lakeshore370Cmd::SetAnalogManual { channel, polarity, manual_value } = command
    else {
        panic!()
    };
    assert_eq!(channel, 2);
    assert_eq!(polarity, 1);
    assert_eq!(manual_value, 50.0);
}

#[test]
fn ls370_set_analog_still_parses() {
    let cli = parse(&["frost", "lakeshore370", "set-analog-still", "0"]);
    let Device::Lakeshore370 { command, .. } = cli.device else { panic!() };
    let Lakeshore370Cmd::SetAnalogStill { polarity } = command else { panic!() };
    assert_eq!(polarity, 0);
}

#[test]
fn ls370_raw_parses() {
    let cli = parse(&["frost", "lakeshore370", "raw", "RDGK?", "1"]);
    let Device::Lakeshore370 { command, .. } = cli.device else { panic!() };
    let Lakeshore370Cmd::Raw { command: tokens } = command else { panic!() };
    assert_eq!(tokens, vec!["RDGK?", "1"]);
}

#[test]
fn ls370_set_range_too_few_args_fails() {
    parse_fails(&["frost", "lakeshore370", "set-range", "3", "1", "5"]);
}

#[test]
fn ls370_kelvin_missing_input_fails() {
    parse_fails(&["frost", "lakeshore370", "kelvin"]);
}

// ── LakeShore 350 ──────────────────────────────────────────────────────────

#[test]
fn ls350_zero_arg_subcommands_parse() {
    let cmds = ["identify", "all", "display-show-all", "query-all-outputs"];
    for cmd in cmds {
        parse(&["frost", "lakeshore350", cmd]);
    }
}

#[test]
fn ls350_read_parses_string_input() {
    let inputs = ["A", "B", "C", "D1", "D2", "D3", "D4", "D5"];
    for input in inputs {
        let cli = parse(&["frost", "lakeshore350", "read", input]);
        let Device::Lakeshore350 { command, .. } = cli.device else { panic!() };
        let Lakeshore350Cmd::Read { input: parsed } = command else { panic!() };
        assert_eq!(parsed, input);
    }
}

#[test]
fn ls350_display_show_parses() {
    let cli = parse(&["frost", "lakeshore350", "display-show", "B"]);
    let Device::Lakeshore350 { command, .. } = cli.device else { panic!() };
    let Lakeshore350Cmd::DisplayShow { input } = command else { panic!() };
    assert_eq!(input, "B");
}

#[test]
fn ls350_display_set_name_parses() {
    let cli = parse(&["frost", "lakeshore350", "display-set-name", "A", "3-head"]);
    let Device::Lakeshore350 { command, .. } = cli.device else { panic!() };
    let Lakeshore350Cmd::DisplaySetName { input, name } = command else { panic!() };
    assert_eq!(input, "A");
    assert_eq!(name, "3-head");
}

#[test]
fn ls350_set_output_parses() {
    let cli = parse(&["frost", "lakeshore350", "set-output", "2", "75.0"]);
    let Device::Lakeshore350 { command, .. } = cli.device else { panic!() };
    let Lakeshore350Cmd::SetOutput { output, percent } = command else { panic!() };
    assert_eq!(output, 2);
    assert_eq!(percent, 75.0);
}

#[test]
fn ls350_query_output_parses() {
    let cli = parse(&["frost", "lakeshore350", "query-output", "3"]);
    let Device::Lakeshore350 { command, .. } = cli.device else { panic!() };
    let Lakeshore350Cmd::QueryOutput { output } = command else { panic!() };
    assert_eq!(output, 3);
}

#[test]
fn ls350_outputs_set_range_parses() {
    let cli = parse(&["frost", "lakeshore350", "outputs-set-range", "1", "2"]);
    let Device::Lakeshore350 { command, .. } = cli.device else { panic!() };
    let Lakeshore350Cmd::OutputsSetRange { output, range } = command else { panic!() };
    assert_eq!(output, 1);
    assert_eq!(range, 2);
}

#[test]
fn ls350_outputs_set_params_collects_variadic_args() {
    let cli = parse(&[
        "frost", "lakeshore350", "outputs-set-params", "1",
        "50", "1.0", "1.0", "1",
    ]);
    let Device::Lakeshore350 { command, .. } = cli.device else { panic!() };
    let Lakeshore350Cmd::OutputsSetParams { output, params } = command else { panic!() };
    assert_eq!(output, 1);
    assert_eq!(params, vec!["50", "1.0", "1.0", "1"]);
}

#[test]
fn ls350_raw_parses() {
    let cli = parse(&["frost", "lakeshore350", "raw", "*IDN?"]);
    let Device::Lakeshore350 { command, .. } = cli.device else { panic!() };
    let Lakeshore350Cmd::Raw { command: tokens } = command else { panic!() };
    assert_eq!(tokens, vec!["*IDN?"]);
}

#[test]
fn ls350_read_missing_input_fails() {
    parse_fails(&["frost", "lakeshore350", "read"]);
}

#[test]
fn ls350_set_output_missing_percent_fails() {
    parse_fails(&["frost", "lakeshore350", "set-output", "1"]);
}

#[test]
fn ls350_outputs_set_params_missing_params_fails() {
    parse_fails(&["frost", "lakeshore350", "outputs-set-params", "1"]);
}

// ── RecordTemps ────────────────────────────────────────────────────────────

#[test]
fn record_temps_snapshot_parses() {
    let cli = parse(&["frost", "record-temps", "snapshot"]);
    let Device::RecordTemps { command } = cli.device else { panic!() };
    assert!(matches!(command, RecordTempsCmd::Snapshot));
}

#[test]
fn record_temps_loop_default_interval() {
    let cli = parse(&["frost", "record-temps", "loop"]);
    let Device::RecordTemps { command } = cli.device else { panic!() };
    let RecordTempsCmd::Loop { interval } = command else { panic!() };
    assert_eq!(interval, 30);
}

#[test]
fn record_temps_loop_custom_interval() {
    let cli = parse(&["frost", "record-temps", "loop", "--interval", "60"]);
    let Device::RecordTemps { command } = cli.device else { panic!() };
    let RecordTempsCmd::Loop { interval } = command else { panic!() };
    assert_eq!(interval, 60);
}

// ── Top-level failures ─────────────────────────────────────────────────────

#[test]
fn no_subcommand_fails() {
    parse_fails(&["frost"]);
}

#[test]
fn unknown_device_fails() {
    parse_fails(&["frost", "lakeshore999", "identify"]);
}

#[test]
fn unknown_subcommand_fails() {
    parse_fails(&["frost", "lakeshore625", "teleport"]);
}

// ── gl7 subcommands ────────────────────────────────────────────────────────────

#[test]
fn gl7_check_parses_csv() {
    let cli = parse(&["frost", "gl7", "check", "--csv", "/tmp/temps.csv"]);
    let Device::Gl7 { command } = cli.device else { panic!() };
    let Gl7Cmd::Check { csv } = command else { panic!() };
    assert_eq!(csv, "/tmp/temps.csv");
}

#[test]
fn gl7_ramp_pumps_parses_csv() {
    let cli = parse(&["frost", "gl7", "ramp-pumps", "--csv", "/tmp/temps.csv"]);
    let Device::Gl7 { command } = cli.device else { panic!() };
    let Gl7Cmd::RampPumps { csv } = command else { panic!() };
    assert_eq!(csv, "/tmp/temps.csv");
}

#[test]
fn gl7_stabilize_parses_csv() {
    let cli = parse(&["frost", "gl7", "stabilize", "--csv", "/tmp/temps.csv"]);
    let Device::Gl7 { command } = cli.device else { panic!() };
    let Gl7Cmd::Stabilize { csv } = command else { panic!() };
    assert_eq!(csv, "/tmp/temps.csv");
}

#[test]
fn gl7_check_missing_csv_fails() {
    parse_fails(&["frost", "gl7", "check"]);
}

#[test]
fn gl7_stabilize_missing_csv_fails() {
    parse_fails(&["frost", "gl7", "stabilize"]);
}

#[test]
fn gl7_unknown_subcommand_fails() {
    parse_fails(&["frost", "gl7", "cycle"]);
}

#[test]
fn gl7_cooldown_parses_csv() {
    let cli = parse(&["frost", "gl7", "cooldown", "--csv", "/tmp/temps.csv"]);
    let Device::Gl7 { command } = cli.device else { panic!() };
    let Gl7Cmd::Cooldown { csv } = command else { panic!() };
    assert_eq!(csv, "/tmp/temps.csv");
}

#[test]
fn gl7_cooldown_missing_csv_fails() {
    parse_fails(&["frost", "gl7", "cooldown"]);
}

#[test]
fn gl7_cycle_4he_parses_csv_with_default_out2() {
    let cli = parse(&["frost", "gl7", "cycle-4he", "--csv", "/tmp/temps.csv"]);
    let Device::Gl7 { command } = cli.device else { panic!() };
    let Gl7Cmd::Cycle4he { csv, out2 } = command else { panic!() };
    assert_eq!(csv, "/tmp/temps.csv");
    assert!((out2 - 18.0).abs() < 1e-9, "default out2 must be 18.0, got {out2}");
}

#[test]
fn gl7_cycle_4he_parses_out2_override() {
    let cli = parse(&["frost", "gl7", "cycle-4he", "--csv", "/tmp/temps.csv", "--out2", "22.5"]);
    let Device::Gl7 { command } = cli.device else { panic!() };
    let Gl7Cmd::Cycle4he { csv, out2 } = command else { panic!() };
    assert_eq!(csv, "/tmp/temps.csv");
    assert!((out2 - 22.5).abs() < 1e-9, "out2 must be 22.5, got {out2}");
}

#[test]
fn gl7_cycle_4he_missing_csv_fails() {
    parse_fails(&["frost", "gl7", "cycle-4he"]);
}

#[test]
fn gl7_cycle_3he_parses_csv_with_default_out3() {
    let cli = parse(&["frost", "gl7", "cycle-3he", "--csv", "/tmp/temps.csv"]);
    let Device::Gl7 { command } = cli.device else { panic!() };
    let Gl7Cmd::Cycle3he { csv, out3 } = command else { panic!() };
    assert_eq!(csv, "/tmp/temps.csv");
    assert!((out3 - 40.0).abs() < 1e-9, "default out3 must be 40.0, got {out3}");
}

#[test]
fn gl7_cycle_3he_parses_out3_override() {
    let cli = parse(&["frost", "gl7", "cycle-3he", "--csv", "/tmp/temps.csv", "--out3", "45.0"]);
    let Device::Gl7 { command } = cli.device else { panic!() };
    let Gl7Cmd::Cycle3he { csv, out3 } = command else { panic!() };
    assert_eq!(csv, "/tmp/temps.csv");
    assert!((out3 - 45.0).abs() < 1e-9, "out3 must be 45.0, got {out3}");
}

#[test]
fn gl7_cycle_3he_missing_csv_fails() {
    parse_fails(&["frost", "gl7", "cycle-3he"]);
}

#[test]
fn gl7_running_parses_csv_with_defaults() {
    let cli = parse(&["frost", "gl7", "running", "--csv", "/tmp/temps.csv"]);
    let Device::Gl7 { command } = cli.device else { panic!() };
    let Gl7Cmd::Running { csv, out3, out4 } = command else { panic!() };
    assert_eq!(csv, "/tmp/temps.csv");
    assert!((out3 - 40.0).abs() < 1e-9, "default out3 must be 40.0, got {out3}");
    assert!((out4 - 40.0).abs() < 1e-9, "default out4 must be 40.0, got {out4}");
}

#[test]
fn gl7_running_parses_out3_and_out4_overrides() {
    let cli = parse(&[
        "frost", "gl7", "running",
        "--csv", "/tmp/temps.csv",
        "--out3", "38.0",
        "--out4", "42.5",
    ]);
    let Device::Gl7 { command } = cli.device else { panic!() };
    let Gl7Cmd::Running { csv, out3, out4 } = command else { panic!() };
    assert_eq!(csv, "/tmp/temps.csv");
    assert!((out3 - 38.0).abs() < 1e-9, "out3 must be 38.0, got {out3}");
    assert!((out4 - 42.5).abs() < 1e-9, "out4 must be 42.5, got {out4}");
}

#[test]
fn gl7_running_missing_csv_fails() {
    parse_fails(&["frost", "gl7", "running"]);
}
