//! Browservice entry point (Rust port, work in progress).

use std::process::ExitCode;

use browservice::config;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let code = match config::read(&args) {
        Ok(_config) => {
            eprintln!("ERROR: Running the browser is not ported to Rust yet");
            1
        }
        Err(code) => code,
    };
    ExitCode::from(code.clamp(0, 255) as u8)
}
