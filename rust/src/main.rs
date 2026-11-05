//! Browservice entry point. Mirrors `main` in `src/main.cpp`; the program
//! itself is `browservice::app_main::run_main`.
//!
//! On Windows the distributed build does not use this executable: like the
//! C++ build, it is a DLL (`rust/windll`) exporting `RunConsoleMain`, loaded
//! by CEF's `bootstrapc.exe`, which initializes the sandbox (see
//! `RUST_PORT.md`). This executable passes no sandbox information.

use std::process::ExitCode;

fn main() -> ExitCode {
    let code = browservice::app_main::run_main(std::ptr::null_mut());
    ExitCode::from(code.clamp(0, 255) as u8)
}
