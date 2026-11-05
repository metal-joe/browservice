//! Browservice entry point (Rust port, work in progress). Mirrors `main` in
//! `src/main.cpp`: CEF subprocess dispatch, configuration, vice plugin
//! loading and context initialization, CEF initialization with windowless
//! rendering and the server running in the CEF message loop.

use std::process::ExitCode;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use browservice::browser_font_render_mode::init_browser_font_render_mode;
use browservice::cef_app::{self, AppState, InitParams};
use browservice::chromium_patch::USING_TEST_STUB;
use browservice::globals::{globals, init_globals};
use browservice::vice::{ViceContext, VicePlugin};
use browservice::{config, info_log, warning_log};

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

    info_log!("Initializing vice plugin {}", config.vice_plugin);
    let Some(vice_ctx) = ViceContext::init(Rc::new(vice_plugin), &config.vice_opts) else {
        return 1;
    };

    // Dropped (shutting the X server down) when `run` returns, after CEF has
    // been shut down, like `xvfb.reset()` at the end of the C++ main.
    #[cfg(not(windows))]
    let _xvfb = if config.use_dedicated_xvfb {
        let xvfb = browservice::xvfb::Xvfb::create();
        xvfb.setup_env();
        Some(xvfb)
    } else {
        None
    };

    let data_dir = config.data_dir.clone();
    let user_agent = config.user_agent.clone();
    let chromium_args = config.chromium_args.clone();
    init_globals(config);
    // Like the C++ Globals constructor: create the text render context (it
    // sets FREETYPE_PROPERTIES temporarily) before CEF is initialized.
    let _text_render_context = browservice::text::global_text_render_context();
    let dot_dir = globals().dot_dir_path.clone();

    if browservice::common::term_signal_received() {
        return 0;
    }

    // Ignore non-fatal X errors
    #[cfg(not(windows))]
    browservice::x_error::ignore_non_fatal_x_errors();

    let (params, request_context_cache_path) = if data_dir.is_empty() {
        // Incognito mode.
        let root_cache_path = format!("{dot_dir}/cef");
        (
            InitParams {
                user_agent,
                cache_path: root_cache_path.clone(),
                root_cache_path,
            },
            String::new(),
        )
    } else {
        (
            InitParams {
                user_agent,
                root_cache_path: data_dir.clone(),
                cache_path: data_dir.clone(),
            },
            data_dir,
        )
    };

    {
        let mut state = state.lock().unwrap();
        state.chromium_args = chromium_args;
        state.request_context_cache_path = request_context_cache_path;
        state.initialized = true;
    }
    cef_app::set_vice_context(vice_ctx);

    info_log!("Initializing CEF (windowless rendering)");
    cef_app::run_browser_process(&cef_args, &mut app, &params);
    info_log!("CEF shut down cleanly");
    0
}

fn main() -> ExitCode {
    let code = run();
    ExitCode::from(code.clamp(0, 255) as u8)
}
