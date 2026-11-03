//! Minimal CEF application: subprocess dispatch and a windowless-rendering
//! initialization/shutdown cycle of the browser process. Corresponds to the
//! `App` class and the CEF parts of `main` in `src/main.cpp`.
//!
//! The server (windows, browser area, vice context) is not ported yet, so the
//! browser process currently initializes CEF, waits for the global context to
//! be initialized and then shuts CEF down cleanly.

use std::sync::{Arc, Mutex};

use cef::sys::{cef_resultcode_t, cef_scheme_options_t};
use cef::*;

/// Configuration needed by the CEF application callbacks in the browser
/// process. In subprocesses the app is created without it.
#[derive(Clone, Default)]
pub struct AppState {
    pub chromium_args: Vec<(String, Option<String>)>,
    /// True when the app is used in the browser process after the
    /// configuration has been read (`initialized_` in the C++ App).
    pub initialized: bool,
    /// Set when the global request context has been initialized.
    pub context_initialized: bool,
}

pub type SharedAppState = Arc<Mutex<AppState>>;

wrap_app! {
    pub struct BrowserviceApp {
        state: SharedAppState,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            let state = self.state.lock().unwrap();
            if !state.initialized {
                return;
            }
            let Some(command_line) = command_line else {
                return;
            };

            command_line.append_switch(Some(&CefString::from("disable-smooth-scrolling")));

            // On Linux, use ANGLE/SwiftShader to maximize compatibility.
            #[cfg(not(windows))]
            {
                command_line.append_switch_with_value(
                    Some(&CefString::from("use-gl")),
                    Some(&CefString::from("angle")),
                );
                command_line.append_switch_with_value(
                    Some(&CefString::from("use-angle")),
                    Some(&CefString::from("swiftshader")),
                );
            }

            for (name, value) in &state.chromium_args {
                let name = CefString::from(name.as_str());
                match value {
                    Some(value) => command_line.append_switch_with_value(
                        Some(&name),
                        Some(&CefString::from(value.as_str())),
                    ),
                    None => command_line.append_switch(Some(&name)),
                }
            }
        }

        fn on_register_custom_schemes(&self, registrar: Option<&mut SchemeRegistrar>) {
            if let Some(registrar) = registrar {
                let options = cef_scheme_options_t::CEF_SCHEME_OPTION_STANDARD as i32
                    | cef_scheme_options_t::CEF_SCHEME_OPTION_LOCAL as i32
                    | cef_scheme_options_t::CEF_SCHEME_OPTION_DISPLAY_ISOLATED as i32;
                registrar.add_custom_scheme(Some(&CefString::from("browservice")), options);
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(BrowserviceBrowserProcessHandler::new(self.state.clone()))
        }
    }
}

wrap_browser_process_handler! {
    pub struct BrowserviceBrowserProcessHandler {
        state: SharedAppState,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            let mut state = self.state.lock().unwrap();
            if !state.initialized {
                return;
            }
            state.context_initialized = true;
            drop(state);

            crate::info_log!("CEF context initialized");
            // The server is not ported yet. Quitting the message loop is
            // deferred to a task, because this callback may run before the
            // message loop has started (in which case a quit would be lost).
            let mut task = PollTask::new(self.state.clone());
            post_task(ThreadId::UI, Some(&mut task));
        }

        fn on_already_running_app_relaunch(
            &self,
            _command_line: Option<&mut CommandLine>,
            _current_directory: Option<&CefString>,
        ) -> ::std::os::raw::c_int {
            // Prevent default action.
            1
        }
    }
}

wrap_task! {
    struct PollTask {
        state: SharedAppState,
    }

    impl Task {
        fn execute(&self) {
            // Like pollTermSignal in src/main.cpp, this is re-posted every
            // 200 ms while running. The phase 1 port has no server to run, so
            // the loop is quit as soon as the context has been initialized.
            let context_initialized = self.state.lock().unwrap().context_initialized;
            if crate::common::term_signal_received() {
                crate::info_log!("Termination signal received, quitting CEF message loop");
                quit_message_loop();
            } else if context_initialized {
                crate::info_log!("Server not ported yet, quitting CEF message loop");
                quit_message_loop();
            } else {
                let mut task = PollTask::new(self.state.clone());
                post_delayed_task(ThreadId::UI, Some(&mut task), 200);
            }
        }
    }
}

pub fn new_app(state: SharedAppState) -> App {
    BrowserviceApp::new(state)
}

/// Must be called before any other CEF function to select the CEF API version
/// the bindings were generated for.
pub fn select_api_version() {
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
}

/// Runs `CefExecuteProcess`. Returns `Some(exit_code)` if this is a CEF
/// subprocess that has finished and the program should exit with that code.
pub fn execute_subprocess(args: &args::Args, app: &mut App) -> Option<i32> {
    let code = execute_process(Some(args.as_main_args()), Some(app), std::ptr::null_mut());
    if code >= 0 { Some(code) } else { None }
}

pub struct InitParams {
    pub user_agent: String,
    pub root_cache_path: String,
    pub cache_path: String,
}

/// Initializes CEF with windowless rendering, runs the message loop until it
/// is quit and shuts CEF down. Returns false if initialization failed.
pub fn run_browser_process(args: &args::Args, app: &mut App, params: &InitParams) -> bool {
    let settings = Settings {
        windowless_rendering_enabled: 1,
        command_line_args_disabled: 1,
        user_agent: CefString::from(params.user_agent.as_str()),
        root_cache_path: CefString::from(params.root_cache_path.as_str()),
        cache_path: CefString::from(params.cache_path.as_str()),
        ..Default::default()
    };

    if initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(app),
        std::ptr::null_mut(),
    ) != 1
    {
        if get_exit_code() == cef_resultcode_t::CEF_RESULT_CODE_NORMAL_EXIT_PROCESS_NOTIFIED as i32
        {
            crate::panic_log!(
                "Another Browservice instance is running. Close that instance or specify \
                 different data directory using --data-dir for each instance to support \
                 concurrent instances."
            );
        }
        crate::error_log!("Initializing CEF failed");
        return false;
    }

    crate::common::register_term_signal_handler();

    run_message_loop();
    shutdown();
    true
}
