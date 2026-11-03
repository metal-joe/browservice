//! CEF application: the `App` class and the CEF parts of `main` in
//! `src/main.cpp`. In the browser process, the server is created when the
//! CEF context has been initialized and the message loop runs until the
//! server has shut down (after SIGINT/SIGTERM).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use cef::sys::{cef_resultcode_t, cef_scheme_options_t};
use cef::*;

use crate::server::{Server, ServerEventHandler};
use crate::vice::ViceContext;

/// Configuration needed by the CEF application callbacks in the browser
/// process. In subprocesses the app is created without it.
#[derive(Clone, Default)]
pub struct AppState {
    pub chromium_args: Vec<(String, Option<String>)>,
    /// True when the app is used in the browser process after the
    /// configuration has been read (`initialized_` in the C++ App).
    pub initialized: bool,
    /// Cache path of the request context of the browser windows (empty for
    /// incognito mode).
    pub request_context_cache_path: String,
    /// Set when the global request context has been initialized.
    pub context_initialized: bool,
}

pub type SharedAppState = Arc<Mutex<AppState>>;

// Browser process state that lives on the CEF UI thread (which is the main
// thread of the program).
thread_local! {
    static PENDING_VICE_CTX: RefCell<Option<Rc<ViceContext>>> = const { RefCell::new(None) };
    static SERVER: RefCell<Option<Rc<Server>>> = const { RefCell::new(None) };
    static SHUTDOWN_REQUESTED: Cell<bool> = const { Cell::new(false) };
    static QUIT_MESSAGE_LOOP_CALLED: Cell<bool> = const { Cell::new(false) };
    static SERVER_EVENT_HANDLER: Rc<AppServerEventHandler> = Rc::new(AppServerEventHandler);
}

struct AppServerEventHandler;

impl ServerEventHandler for AppServerEventHandler {
    fn on_server_shutdown_complete(&self) {
        crate::info_log!("Quitting CEF message loop");
        QUIT_MESSAGE_LOOP_CALLED.with(|c| c.set(true));
        quit_message_loop();
    }
}

/// Gives the initialized vice plugin context to the app; the server is
/// started with it once the CEF context is initialized.
pub fn set_vice_context(ctx: Rc<ViceContext>) {
    PENDING_VICE_CTX.with(|p| *p.borrow_mut() = Some(ctx));
}

/// Shuts down the server, or marks the shutdown to happen right after the
/// server is created (`App::shutdown`).
fn app_shutdown() {
    let server = SERVER.with(|s| s.borrow().clone());
    match server {
        Some(server) => server.shutdown(),
        None => SHUTDOWN_REQUESTED.with(|s| s.set(true)),
    }
}

/// Polls the termination signal flag every 200 ms (`pollTermSignal`).
fn poll_term_signal() {
    if crate::common::term_signal_received() {
        app_shutdown();
    } else {
        crate::ui::post_delayed_task(200, poll_term_signal);
    }
}

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
            crate::ui::require_ui_thread();
            state.context_initialized = true;
            let cache_path = state.request_context_cache_path.clone();
            drop(state);

            if SERVER.with(|s| s.borrow().is_some()) {
                crate::panic_log!("Requirement '!server_' failed");
            }

            crate::scheme::register_browservice_scheme_handler_factory();

            let settings = RequestContextSettings {
                cache_path: CefString::from(cache_path.as_str()),
                ..Default::default()
            };
            let Some(request_context) = request_context_create_context(Some(&settings), None)
            else {
                crate::panic_log!("Requirement 'requestContext' failed");
            };
            crate::scheme::register_browservice_scheme_handler_factory_for(&request_context);

            let Some(vice_ctx) = PENDING_VICE_CTX.with(|p| p.borrow_mut().take()) else {
                crate::panic_log!("Vice context missing when CEF context was initialized");
            };
            let handler = SERVER_EVENT_HANDLER.with(|h| h.clone());
            let handler: Rc<dyn ServerEventHandler> = handler;
            let server = Server::create(Rc::downgrade(&handler), vice_ctx, request_context);
            SERVER.with(|s| *s.borrow_mut() = Some(server.clone()));
            if SHUTDOWN_REQUESTED.with(|s| s.get()) {
                server.shutdown();
            }
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

/// Initializes CEF with windowless rendering, runs the message loop until the
/// server has shut down and shuts CEF down. Panics if initialization fails.
pub fn run_browser_process(args: &args::Args, app: &mut App, params: &InitParams) {
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
        crate::panic_log!("Initializing CEF failed");
    }

    // Re-register termination handlers as CEF initialization may have
    // interfered with the previous registrations.
    crate::common::register_term_signal_handler();

    crate::ui::post_task(poll_term_signal);

    run_message_loop();

    if !QUIT_MESSAGE_LOOP_CALLED.with(|c| c.get()) {
        crate::panic_log!("Requirement 'cefQuitMessageLoopCalled' failed");
    }

    shutdown();

    // Like `app = nullptr` after CefShutdown in the C++ version.
    SERVER.with(|s| s.borrow_mut().take());
}
