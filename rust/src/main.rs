//! Browservice entry point (Rust port, work in progress). Mirrors `main` in
//! `src/main.cpp` up to the point the port has reached: CEF subprocess
//! dispatch, configuration, vice plugin loading and a CEF windowless
//! initialization/shutdown cycle. The server is not ported yet.

use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use browservice::browser_font_render_mode::init_browser_font_render_mode;
use browservice::cef_app::{self, AppState, InitParams};
use browservice::chromium_patch::USING_TEST_STUB;
use browservice::globals::{dot_dir_path, ensure_dir_exists};
use browservice::vice::VicePlugin;
use browservice::{config, info_log, panic_log, warning_log};

fn run() -> i32 {
    cef_app::select_api_version();

    let cef_args = cef::args::Args::new();
    let state = Arc::new(Mutex::new(AppState::default()));
    let mut app = cef_app::new_app(state.clone());

    // CEF spawns subprocesses (renderer, GPU, utility, zygote) by re-executing
    // this binary; they are dispatched here before anything else happens.
    if let Some(code) = cef_app::execute_subprocess(&cef_args, &mut app) {
        return code;
    }

    browservice::common::register_term_signal_handler();

    let args: Vec<String> = std::env::args().collect();
    let config = match config::read(&args) {
        Ok(config) => config,
        Err(code) => return code,
    };

    if USING_TEST_STUB {
        warning_log!(
            "This binary was built with the test-only 'stub-patch' feature: the Browservice \
             Chromium patch functions (clipboard, font render mode) are no-op stubs"
        );
    }

    init_browser_font_render_mode(config.browser_font_render_mode);

    info_log!("Loading vice plugin {}", config.vice_plugin);
    let Some(vice_plugin) = VicePlugin::load(&config.vice_plugin) else {
        eprintln!("ERROR: Loading vice plugin {} failed", config.vice_plugin);
        return 1;
    };
    info_log!(
        "Loaded vice plugin {}: {}",
        vice_plugin.filename(),
        vice_plugin.version_string()
    );
    // Vice plugin contexts are not ported yet.
    drop(vice_plugin);

    #[cfg(not(windows))]
    if config.use_dedicated_xvfb {
        warning_log!(
            "Dedicated Xvfb is not ported yet; using the X session of the environment \
             (DISPLAY={})",
            std::env::var("DISPLAY").unwrap_or_default()
        );
    }

    let dot_dir = dot_dir_path();
    if !ensure_dir_exists(&dot_dir) {
        panic_log!(
            "Directory '{}' does not exist and creating it failed",
            dot_dir
        );
    }
    if !config.data_dir.is_empty() && !ensure_dir_exists(&config.data_dir) {
        panic_log!(
            "Data directory '{}' does not exist and creating it failed",
            config.data_dir
        );
    }

    if browservice::common::term_signal_received() {
        return 0;
    }

    let params = if config.data_dir.is_empty() {
        // Incognito mode.
        let root_cache_path = format!("{dot_dir}/cef");
        InitParams {
            user_agent: config.user_agent.clone(),
            cache_path: root_cache_path.clone(),
            root_cache_path,
        }
    } else {
        InitParams {
            user_agent: config.user_agent.clone(),
            root_cache_path: config.data_dir.clone(),
            cache_path: config.data_dir.clone(),
        }
    };

    {
        let mut state = state.lock().unwrap();
        state.chromium_args = config.chromium_args.clone();
        state.initialized = true;
    }

    info_log!("Initializing CEF (windowless rendering)");
    if !cef_app::run_browser_process(&cef_args, &mut app, &params) {
        return 1;
    }
    if !state.lock().unwrap().context_initialized {
        panic_log!("CEF message loop exited before the context was initialized");
    }
    info_log!("CEF shut down cleanly");
    0
}

fn main() -> ExitCode {
    let code = run();
    ExitCode::from(code.clamp(0, 255) as u8)
}
