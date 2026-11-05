//! Windows entry point for CEF's `bootstrapc.exe`, the Rust counterpart of
//! `RunConsoleMain` in `src/main.cpp` (`#ifdef _WIN32`).
//!
//! Since CEF 138 the Windows sandbox broker is part of `bootstrap.exe` /
//! `bootstrapc.exe` from the CEF distribution; there is no `cef_sandbox.lib`
//! to link into an application executable any more. A sandboxed application
//! is therefore a DLL with the entry point declared in
//! `include/cef_sandbox_win.h`:
//!
//! ```c
//! CEF_BOOTSTRAP_EXPORT int RunConsoleMain(int argc, char* argv[],
//!                                         void* sandbox_info,
//!                                         cef_version_info_t* version_info);
//! ```
//!
//! `bootstrapc.exe` (console subsystem), renamed to `browservice.exe`, loads
//! `browservice.dll` from its directory and calls this function in the
//! browser process and in every CEF subprocess, passing the sandbox
//! information object that `CefExecuteProcess` and `CefInitialize` need.
#![cfg(windows)]

use std::ffi::{c_char, c_int, c_void};

/// Called by `bootstrapc.exe`. Like the C++ code, `argc`/`argv` and
/// `version_info` are not used: the command line is read with
/// `GetCommandLineW`/`CommandLineToArgvW` (`common::command_line_args`) and
/// the module handle with `GetModuleHandle(nullptr)` (`cef::args::Args`).
///
/// # Safety
///
/// Must only be called by `bootstrapc.exe` with a valid (or null) sandbox
/// information pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn RunConsoleMain(
    _argc: c_int,
    _argv: *mut *mut c_char,
    sandbox_info: *mut c_void,
    _version_info: *mut c_void,
) -> c_int {
    browservice::app_main::run_main(sandbox_info)
}
