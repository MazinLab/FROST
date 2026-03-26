mod adr_ramping;
mod compressor;
mod heatswitch;
mod lakeshore625;
mod lakeshore370;
mod lakeshore350;
mod record_temps;
mod serial;
mod worker;
mod gui;
mod cli;

fn main() {
    if let Err(e) = cli::run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
