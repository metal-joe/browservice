//! Port of `src/window.{hpp,cpp}`: one browser window, consisting of a CEF
//! browser with windowless (off-screen) rendering, the widget tree that
//! renders into the window image and a download manager.
//!
//! Lifecycle (as in the C++ version): a window is Open after creation; after
//! `close()` (or the browser closing by itself, signaled with
//! `on_window_close`) it is Closed; when the CEF browser has been closed, it
//! signals `on_window_cleanup_complete` and enters CleanupComplete.
//!
//! The CEF handler objects only store the window handle; they find the window
//! through a registry on the CEF UI thread. This keeps the non-thread-safe
//! `Rc` state out of objects that CEF may reference from other threads.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use cef::*;

use crate::browser_area::{BrowserArea, BrowserAreaEventHandler, EVENTFLAG_SHIFT_DOWN};
use crate::data_url::{
    create_signed_data_url, generate_data_url_sign_key, read_signed_data_url, userfree_to_string,
};
use crate::download_manager::{
    CompletedDownload, DownloadManager, DownloadManagerEventHandler, WindowDownloadHandler,
};
use crate::globals::globals;
use crate::image_slice::ImageSlice;
use crate::key::{is_valid_key, keys};
use crate::control_bar::{ControlBarEventHandler, SecurityStatus};
use crate::root_widget::RootWidget;
use crate::timeout::Timeout;
use crate::vice::ViceFileUpload;
use crate::widget::{
    CURSOR_TYPE_COUNT, GlobalHotkey, HAND_CURSOR, NORMAL_CURSOR, TEXT_CURSOR, WidgetExt,
    WidgetParent,
};

pub trait WindowEventHandler {
    fn on_window_close(&self, handle: u64);
    fn on_window_cleanup_complete(&self, handle: u64);
    fn on_window_view_image_changed(&self, handle: u64);
    fn on_window_title_changed(&self, handle: u64);
    fn on_window_cursor_changed(&self, handle: u64, cursor: i32);
    fn on_window_quality_selector_query(&self, handle: u64) -> Option<(Vec<String>, usize)>;
    fn on_window_quality_changed(&self, handle: u64, idx: usize);
    fn on_window_needs_clipboard_button_query(&self, handle: u64) -> bool;
    fn on_window_clipboard_button_pressed(&self, handle: u64);
    fn on_window_download_completed(&self, handle: u64, file: Arc<CompletedDownload>);
    fn on_window_start_file_upload(&self, handle: u64) -> bool;
    fn on_window_cancel_file_upload(&self, handle: u64);
    /// To accept the popup, the implementation calls `accept` once with the
    /// handle of the new window before returning; `accept` returns the new
    /// window (using the same event handler) or None if creation failed.
    fn on_window_create_popup_request(
        &self,
        handle: u64,
        accept: &mut dyn FnMut(u64) -> Option<Rc<Window>>,
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Open,
    Closed,
    CleanupComplete,
}

/// Only works for fully qualified and normalized URLs, such as the ones from
/// `CefRequest::GetURL`.
fn is_local_file_request_url(url: &str) -> bool {
    url.len() >= 5 && url.as_bytes()[..5].eq_ignore_ascii_case(b"file:")
}

// Adjust zoom in steps of sixth root of 2, at most by factor 4.
const ZOOM_FACTOR_STEP: f64 = 1.122462048309373;
const MIN_ZOOM_FACTOR: f64 = 0.25;
const MAX_ZOOM_FACTOR: f64 = 4.0;
const ZOOM_LEVEL_EPSILON: f64 = 1e-6;

/// Zoom factor to the Chromium logarithmic zoom level.
fn zoom_factor_to_zoom_level(zoom: f64) -> f64 {
    let zoom = if zoom.is_finite() { zoom } else { 1.0 };
    zoom.clamp(MIN_ZOOM_FACTOR, MAX_ZOOM_FACTOR).ln() / 1.2f64.ln()
}

fn zoom_level_step() -> f64 {
    zoom_factor_to_zoom_level(ZOOM_FACTOR_STEP) - zoom_factor_to_zoom_level(1.0)
}

fn extract_domain_from_https_url(url: &str) -> Option<String> {
    let prefix = "https://";
    if url.len() <= prefix.len() || !url.starts_with(prefix) {
        return None;
    }
    let rest = &url[prefix.len()..];
    let domain = match rest.find('/') {
        Some(end) => &rest[..end],
        None => rest,
    };
    if domain.is_empty() {
        None
    } else {
        Some(domain.to_string())
    }
}

const CERT_STATUS_NON_ERROR: u32 = 65536 | 131072 | 524288 | 1048576;
const SSL_CONTENT_NORMAL_CONTENT: u32 = 0;

/// Reads the certificate and content status bit masks. The bindings model
/// them as Rust enums, which cannot hold combinations of flags, so the raw
/// C functions are called with integer return types.
fn ssl_status_bits(ssl: &Sslstatus) -> (u32, u32) {
    unsafe {
        let raw = ssl.get_raw();
        let cert: unsafe extern "C" fn(*mut sys::_cef_sslstatus_t) -> u32 =
            std::mem::transmute((*raw).get_cert_status.unwrap());
        let content: unsafe extern "C" fn(*mut sys::_cef_sslstatus_t) -> u32 =
            std::mem::transmute((*raw).get_content_status.unwrap());
        (cert(raw), content(raw))
    }
}

thread_local! {
    static WINDOW_REGISTRY: RefCell<HashMap<u64, Weak<Window>>> = RefCell::new(HashMap::new());
}

fn lookup_window(handle: u64) -> Option<Rc<Window>> {
    WINDOW_REGISTRY.with(|r| r.borrow().get(&handle).and_then(|w| w.upgrade()))
}

/// The download manager of the window with the given handle, if it exists.
pub fn download_manager_of(handle: u64) -> Option<Rc<DownloadManager>> {
    lookup_window(handle).map(|w| w.download_manager.clone())
}

pub struct Window {
    handle: u64,
    self_weak: Weak<Window>,
    state: Cell<State>,
    event_handler: RefCell<Option<Rc<dyn WindowEventHandler>>>,
    request_context: RequestContext,
    show_soft_navigation_buttons: bool,
    image_changed: Cell<bool>,
    title_changed: Cell<bool>,
    title: RefCell<String>,
    browser: RefCell<Option<Browser>>,
    root_viewport: RefCell<ImageSlice>,
    root_widget: Rc<RootWidget>,
    download_manager: Rc<DownloadManager>,
    watchdog_timeout: Rc<Timeout>,
    file_upload_callback: RefCell<Option<FileDialogCallback>>,
    retained_uploads: RefCell<Vec<Rc<ViceFileUpload>>>,
    zoom_level: Cell<f64>,

    // State of the C++ Window::Client (one client per window).
    last_find_id: Cell<i32>,
    last_certificate_error_url: RefCell<Option<String>>,
    certificate_error_page_sign_key: Vec<u8>,
    file_scheme_blocked_page_sign_key: Vec<u8>,
}

macro_rules! require {
    ($cond:expr) => {
        if !($cond) {
            crate::panic_log!("Requirement '{}' failed", stringify!($cond));
        }
    };
}

impl Window {
    /// Creates the window object in the Open state (`create` + `init_`).
    fn new_window(
        event_handler: Rc<dyn WindowEventHandler>,
        request_context: RequestContext,
        handle: u64,
        show_soft_navigation_buttons: bool,
    ) -> Rc<Window> {
        require!(handle != 0);
        let window = Rc::new_cyclic(|weak: &Weak<Window>| {
            let widget_parent: Weak<dyn WidgetParent> = weak.clone();
            let control_bar_handler: Weak<dyn ControlBarEventHandler> = weak.clone();
            let browser_area_handler: Weak<dyn BrowserAreaEventHandler> = weak.clone();
            let download_handler: Weak<dyn DownloadManagerEventHandler> = weak.clone();
            Window {
                handle,
                self_weak: weak.clone(),
                state: Cell::new(State::Open),
                event_handler: RefCell::new(Some(event_handler)),
                request_context,
                show_soft_navigation_buttons,
                image_changed: Cell::new(false),
                title_changed: Cell::new(false),
                title: RefCell::new("Browservice".to_string()),
                browser: RefCell::new(None),
                root_viewport: RefCell::new(ImageSlice::create_image(800, 600, 255)),
                root_widget: RootWidget::create(
                    widget_parent,
                    control_bar_handler,
                    browser_area_handler,
                    show_soft_navigation_buttons,
                ),
                download_manager: Rc::new(DownloadManager::new(download_handler)),
                watchdog_timeout: Timeout::create(250),
                file_upload_callback: RefCell::new(None),
                retained_uploads: RefCell::new(Vec::new()),
                zoom_level: Cell::new(zoom_factor_to_zoom_level(globals().config.initial_zoom)),
                last_find_id: Cell::new(-1),
                last_certificate_error_url: RefCell::new(None),
                certificate_error_page_sign_key: generate_data_url_sign_key(),
                file_scheme_blocked_page_sign_key: generate_data_url_sign_key(),
            }
        });
        WINDOW_REGISTRY.with(|r| r.borrow_mut().insert(handle, Rc::downgrade(&window)));
        let viewport = window.root_viewport.borrow().clone();
        window.root_widget.set_viewport(viewport);
        window
    }

    fn rc(&self) -> Rc<Window> {
        self.self_weak.upgrade().expect("window is alive")
    }

    fn handler(&self) -> Rc<dyn WindowEventHandler> {
        match self.event_handler.borrow().clone() {
            Some(h) => h,
            None => crate::panic_log!("Requirement 'eventHandler_' failed"),
        }
    }

    fn browser(&self) -> Option<Browser> {
        self.browser.borrow().clone()
    }

    fn host(&self) -> Option<BrowserHost> {
        self.browser().and_then(|b| b.host())
    }

    fn browser_area(&self) -> Rc<BrowserArea> {
        self.root_widget.browser_area()
    }

    pub fn handle(&self) -> u64 {
        self.handle
    }

    /// Creates a window and its CEF browser. Returns None if creating the CEF
    /// browser fails.
    pub fn try_create(
        event_handler: Rc<dyn WindowEventHandler>,
        request_context: RequestContext,
        handle: u64,
        uri: Option<String>,
        show_soft_navigation_buttons: bool,
    ) -> Option<Rc<Window>> {
        crate::ui::require_ui_thread();
        crate::info_log!("Creating window {}", handle);

        let window = Window::new_window(
            event_handler,
            request_context.clone(),
            handle,
            show_soft_navigation_buttons,
        );
        let mut client = WindowClient::create(handle);

        let window_info = WindowInfo::default().set_as_windowless(0);
        let browser_settings = BrowserSettings {
            background_color: 0xFFFFFFFF,
            ..Default::default()
        };

        let url = match uri {
            Some(uri) if !uri.is_empty() => uri,
            _ => globals().config.start_page.clone(),
        };
        let mut request_context = request_context;
        if browser_host_create_browser(
            Some(&window_info),
            Some(&mut client),
            Some(&CefString::from(url.as_str())),
            Some(&browser_settings),
            None,
            Some(&mut request_context),
        ) == 0
        {
            crate::warning_log!(
                "Opening CEF browser for window {} failed, aborting window creation",
                handle
            );
            window.create_failed();
            return None;
        }

        window.create_successful();
        Some(window)
    }

    pub fn close(&self) {
        crate::ui::require_ui_thread();
        require!(self.state.get() == State::Open);

        crate::info_log!("Closing window {}", self.handle);
        self.state.set(State::Closed);
        self.after_close();

        // If the browser has been created, start closing it; otherwise closing
        // is deferred to on_after_created.
        if let Some(browser) = self.browser() {
            crate::ui::post_task(move || {
                if let Some(host) = browser.host() {
                    host.close_browser(1);
                }
            });
        }
    }

    pub fn resize(&self, width: i32, height: i32) {
        crate::ui::require_ui_thread();
        require!(self.state.get() == State::Open);

        let width = width.clamp(64, 4096);
        let height = height.clamp(64, 4096);
        let (cur_w, cur_h) = {
            let vp = self.root_viewport.borrow();
            (vp.width(), vp.height())
        };
        if cur_w != width || cur_h != height {
            let viewport = ImageSlice::create_image(width, height, 255);
            *self.root_viewport.borrow_mut() = viewport.clone();
            self.root_widget.set_viewport(viewport);
        }
    }

    pub fn fetch_view_image(&self) -> ImageSlice {
        crate::ui::require_ui_thread();
        require!(self.state.get() == State::Open);
        self.image_changed.set(false);
        self.root_viewport.borrow().clone()
    }

    pub fn fetch_title(&self) -> String {
        crate::ui::require_ui_thread();
        require!(self.state.get() == State::Open);
        self.title_changed.set(false);
        self.title.borrow().clone()
    }

    /// -1 = back, 0 = refresh, 1 = forward.
    pub fn navigate(&self, direction: i32) {
        crate::ui::require_ui_thread();
        require!(self.state.get() == State::Open);
        require!((-1..=1).contains(&direction));
        if let Some(browser) = self.browser() {
            match direction {
                -1 => browser.go_back(),
                0 => browser.reload(),
                _ => browser.go_forward(),
            }
        }
    }

    pub fn navigate_to_uri(&self, uri: &str) {
        crate::ui::require_ui_thread();
        require!(self.state.get() == State::Open);
        if !uri.is_empty()
            && let Some(frame) = self.browser().and_then(|b| b.main_frame())
        {
            frame.load_url(Some(&CefString::from(uri)));
            self.browser_area().take_focus();
        }
    }

    pub fn upload_file(&self, file: Rc<ViceFileUpload>) {
        crate::ui::require_ui_thread();
        require!(self.state.get() == State::Open);
        let callback = self.file_upload_callback.borrow_mut().take();
        let Some(callback) = callback else {
            crate::panic_log!("Requirement 'fileUploadCallback_' failed");
        };
        let mut paths = CefStringList::new();
        paths.append(file.path());
        callback.cont(Some(&mut paths));

        // All uploads are retained until the window cleanup is complete, as
        // we cannot know how long CEF uses them.
        self.retained_uploads.borrow_mut().push(file);
    }

    pub fn cancel_file_upload(&self) {
        crate::ui::require_ui_thread();
        require!(self.state.get() == State::Open);
        let callback = self.file_upload_callback.borrow_mut().take();
        let Some(callback) = callback else {
            crate::panic_log!("Requirement 'fileUploadCallback_' failed");
        };
        callback.cancel();
    }

    fn clamp_mouse_coords(&self, x: i32, y: i32) -> (i32, i32) {
        let vp = self.root_viewport.borrow();
        (x.max(-1000).min(vp.width() + 1000), y.max(-1000).min(vp.height() + 1000))
    }

    pub fn send_mouse_down_event(&self, x: i32, y: i32, button: i32) {
        require!(self.state.get() == State::Open);
        if (0..=2).contains(&button) {
            let (x, y) = self.clamp_mouse_coords(x, y);
            self.root_widget.send_mouse_down_event(x, y, button);
        }
    }

    pub fn send_mouse_up_event(&self, x: i32, y: i32, button: i32) {
        require!(self.state.get() == State::Open);
        if (0..=2).contains(&button) {
            let (x, y) = self.clamp_mouse_coords(x, y);
            self.root_widget.send_mouse_up_event(x, y, button);
        }
    }

    pub fn send_mouse_move_event(&self, x: i32, y: i32) {
        require!(self.state.get() == State::Open);
        let (x, y) = self.clamp_mouse_coords(x, y);
        self.root_widget.send_mouse_move_event(x, y);
    }

    pub fn send_mouse_double_click_event(&self, x: i32, y: i32, button: i32) {
        require!(self.state.get() == State::Open);
        if button == 0 {
            let (x, y) = self.clamp_mouse_coords(x, y);
            self.root_widget.send_mouse_double_click_event(x, y);
        }
    }

    pub fn send_mouse_wheel_event(&self, x: i32, y: i32, _dx: i32, dy: i32) {
        require!(self.state.get() == State::Open);
        let (x, y) = self.clamp_mouse_coords(x, y);
        let delta = (-(dy as i64)).clamp(-180, 180) as i32;
        self.root_widget.send_mouse_wheel_event(x, y, delta);
    }

    pub fn send_mouse_leave_event(&self, x: i32, y: i32) {
        require!(self.state.get() == State::Open);
        let (x, y) = self.clamp_mouse_coords(x, y);
        self.root_widget.send_mouse_leave_event(x, y);
    }

    pub fn send_key_down_event(&self, key: i32) {
        require!(self.state.get() == State::Open);
        if is_valid_key(key) {
            self.root_widget.send_key_down_event(key);
        }
    }

    pub fn send_key_up_event(&self, key: i32) {
        require!(self.state.get() == State::Open);
        if is_valid_key(key) {
            self.root_widget.send_key_up_event(key);
        }
    }

    pub fn send_lose_focus_event(&self) {
        require!(self.state.get() == State::Open);
        self.root_widget.send_lose_focus_event();
    }

    pub fn zoom_in(&self) {
        require!(self.state.get() == State::Open);
        self.zoom_level.set((self.zoom_level.get() + zoom_level_step()).clamp(
            zoom_factor_to_zoom_level(MIN_ZOOM_FACTOR),
            zoom_factor_to_zoom_level(MAX_ZOOM_FACTOR),
        ));
        self.update_zoom();
    }

    pub fn zoom_out(&self) {
        require!(self.state.get() == State::Open);
        self.zoom_level.set((self.zoom_level.get() - zoom_level_step()).clamp(
            zoom_factor_to_zoom_level(MIN_ZOOM_FACTOR),
            zoom_factor_to_zoom_level(MAX_ZOOM_FACTOR),
        ));
        self.update_zoom();
    }

    pub fn zoom_reset(&self) {
        require!(self.state.get() == State::Open);
        self.zoom_level
            .set(zoom_factor_to_zoom_level(globals().config.initial_zoom));
        self.update_zoom();
    }

    fn create_successful(&self) {
        crate::ui::require_ui_thread();
        let this = self.rc();
        crate::ui::post_task(move || this.watchdog());

        let this = self.rc();
        crate::ui::post_task(move || {
            if this.state.get() != State::Open {
                return;
            }
            let handler = this.handler();
            if let Some((labels, idx)) = handler.on_window_quality_selector_query(this.handle) {
                this.root_widget.control_bar().enable_quality_selector(labels, idx);
            }
            if handler.on_window_needs_clipboard_button_query(this.handle) {
                this.root_widget.control_bar().enable_clipboard_button();
            }
        });
    }

    fn create_failed(&self) {
        self.state.set(State::CleanupComplete);
        *self.event_handler.borrow_mut() = None;
        WINDOW_REGISTRY.with(|r| r.borrow_mut().remove(&self.handle));
    }

    fn after_close(&self) {
        require!(self.state.get() == State::Closed);
        self.watchdog_timeout.clear(false);
        let callback = self.file_upload_callback.borrow_mut().take();
        if let Some(callback) = callback {
            callback.cancel();
        }
    }

    /// Called every 0.25 s for an Open window for various checks.
    fn watchdog(&self) {
        if self.state.get() != State::Open {
            return;
        }
        // Make sure that the security status is not incorrect for extended
        // periods of time in case the event handlers miss changes.
        self.update_security_status();
        // The browser typically resets the zoom during initialization, so the
        // zoom level is kept correct here.
        self.update_zoom();

        if !self.watchdog_timeout.is_active() {
            let weak = self.self_weak.clone();
            self.watchdog_timeout.set(move || {
                if let Some(this) = weak.upgrade() {
                    this.watchdog();
                }
            });
        }
    }

    fn update_security_status(&self) {
        if self.state.get() != State::Open {
            return;
        }
        let mut status = SecurityStatus::Insecure;
        if let Some(host) = self.host()
            && let Some(nav) = host.visible_navigation_entry()
            && let Some(ssl) = nav.sslstatus()
            && ssl.is_secure_connection() != 0
        {
            let (cert_status, content_status) = ssl_status_bits(&ssl);
            if cert_status & !CERT_STATUS_NON_ERROR == 0 {
                status = if content_status == SSL_CONTENT_NORMAL_CONTENT {
                    SecurityStatus::Secure
                } else {
                    SecurityStatus::Warning
                };
            }
        }
        self.root_widget.control_bar().set_security_status(status);
    }

    fn update_zoom(&self) {
        if self.state.get() != State::Open {
            return;
        }
        if let Some(host) = self.host()
            && (host.zoom_level() - self.zoom_level.get()).abs() > ZOOM_LEVEL_EPSILON
        {
            host.set_zoom_level(self.zoom_level.get());
        }
    }

    fn signal_image_changed(&self) {
        if self.state.get() == State::Open && !self.image_changed.get() {
            self.image_changed.set(true);
            self.handler().on_window_view_image_changed(self.handle);
        }
    }

    fn signal_title_changed(&self) {
        if self.state.get() == State::Open && !self.title_changed.get() {
            self.title_changed.set(true);
            self.handler().on_window_title_changed(self.handle);
        }
    }

    /// Popup creation for both CEF popups and the bookmark popup; returns true
    /// if the popup was denied.
    fn request_popup(&self, accept: &mut dyn FnMut(u64) -> Option<Rc<Window>>) {
        let mut allow_accept_call = true;
        let mut accept_called = false;
        let handler = self.handler();
        handler.on_window_create_popup_request(self.handle, &mut |new_handle| {
            require!(allow_accept_call);
            require!(!accept_called);
            accept_called = true;
            require!(new_handle != 0);
            require!(new_handle != self.handle);
            accept(new_handle)
        });
        allow_accept_call = false;
        let _ = allow_accept_call;
    }

    // --- CEF client callbacks (called by the handler objects below). ---

    fn on_before_popup(
        &self,
        window_info: Option<&mut WindowInfo>,
        client: Option<&mut Option<Client>>,
        settings: Option<&mut BrowserSettings>,
    ) -> bool {
        if self.state.get() != State::Open {
            return true;
        }
        crate::info_log!("CEF browser of window {} is requesting a popup window", self.handle);

        let mut popup_denied = true;
        let mut window_info = window_info;
        let mut client = client;
        let mut settings = settings;
        let event_handler = self.handler();
        self.request_popup(&mut |new_handle| {
            crate::info_log!("Creating window {} (popup of window {})", new_handle, self.handle);
            let new_window = Window::new_window(
                event_handler.clone(),
                self.request_context.clone(),
                new_handle,
                self.show_soft_navigation_buttons,
            );
            if let Some(info) = window_info.as_deref_mut() {
                *info = info.clone().set_as_windowless(0);
            }
            if let Some(settings) = settings.as_deref_mut() {
                settings.background_color = 0xFFFFFFFF;
            }
            if let Some(client) = client.as_deref_mut() {
                *client = Some(WindowClient::create(new_handle));
            }
            new_window.create_successful();
            popup_denied = false;
            Some(new_window)
        });
        popup_denied
    }

    fn on_after_created(&self, browser: &Browser) {
        require!(self.state.get() == State::Open || self.state.get() == State::Closed);
        require!(self.browser.borrow().is_none());

        crate::info_log!("CEF browser for window {} created", self.handle);
        *self.browser.borrow_mut() = Some(browser.clone());
        self.browser_area().set_browser(Some(browser.clone()));
        self.update_security_status();

        if self.state.get() == State::Closed {
            // Browser close deferred from close().
            let browser = browser.clone();
            crate::ui::post_task(move || {
                if let Some(host) = browser.host() {
                    host.close_browser(1);
                }
            });
        }
    }

    fn on_before_close(&self) {
        if self.state.get() == State::Open {
            // The window closed on its own (not triggered by close()).
            crate::info_log!(
                "Closing window {} because the CEF browser is closing",
                self.handle
            );
            self.state.set(State::Closed);
            self.after_close();
            self.handler().on_window_close(self.handle);
        }

        require!(self.state.get() == State::Closed);
        let handler = self.handler();

        crate::info_log!("Cleanup of CEF browser for window {} complete", self.handle);

        self.state.set(State::CleanupComplete);
        *self.browser.borrow_mut() = None;
        self.retained_uploads.borrow_mut().clear();
        self.browser_area().set_browser(None);
        WINDOW_REGISTRY.with(|r| r.borrow_mut().remove(&self.handle));
        handler.on_window_cleanup_complete(self.handle);
        *self.event_handler.borrow_mut() = None;
    }

    fn on_load_start(&self, frame: &Frame) {
        if self.state.get() == State::Open && frame.is_main() != 0 {
            let url = userfree_to_string(&frame.url());
            let area = self.browser_area();
            if read_signed_data_url(&url, &self.certificate_error_page_sign_key).is_some() {
                area.show_error("Loading URL failed due to a certificate error".to_string());
            } else if read_signed_data_url(&url, &self.file_scheme_blocked_page_sign_key).is_some()
            {
                area.show_error(
                    "Access to files through the file:// URI scheme is blocked \
                     (do NOT rely on this block for security, as there may be ways around it)"
                        .to_string(),
                );
            } else {
                area.clear_error();
            }
            // Make sure that the loaded page gets the correct idea about the
            // focus and mouse over status.
            area.refresh_status_events();
        }
    }

    fn on_loading_state_change(&self, is_loading: bool) {
        if self.state.get() == State::Open {
            self.root_widget.control_bar().set_loading(is_loading);
            self.update_security_status();
        }
    }

    fn on_load_error(&self, frame: &Frame, error_code: Errorcode, error_text: String, failed_url: String) {
        if self.state.get() != State::Open || frame.is_main() == 0 {
            return;
        }
        let aborted = error_code == Errorcode::ABORTED;
        let cert_error_url = self.last_certificate_error_url.borrow().clone();
        if aborted && cert_error_url.as_deref() == Some(failed_url.as_str()) {
            let url = create_signed_data_url(&failed_url, &self.certificate_error_page_sign_key);
            frame.load_url(Some(&CefString::from(url.as_str())));
        } else if aborted
            && globals().config.block_file_scheme
            && is_local_file_request_url(&failed_url)
        {
            let url = create_signed_data_url(&failed_url, &self.file_scheme_blocked_page_sign_key);
            frame.load_url(Some(&CefString::from(url.as_str())));
        } else if !aborted {
            self.browser_area()
                .show_error(format!("Loading URL failed due to error: {error_text}"));
            self.root_widget.control_bar().set_address(failed_url);
        }
    }

    fn on_address_change(&self, url: String) {
        if self.state.get() != State::Open {
            return;
        }
        let control_bar = self.root_widget.control_bar();
        if let Some(u) = read_signed_data_url(&url, &self.certificate_error_page_sign_key) {
            control_bar.set_address(u);
        } else if let Some(u) = read_signed_data_url(&url, &self.file_scheme_blocked_page_sign_key)
        {
            control_bar.set_address(u);
        } else {
            control_bar.set_address(url);
        }
        self.update_security_status();
    }

    fn on_title_change(&self, orig_title: String) {
        if self.state.get() != State::Open {
            return;
        }
        let mut title = orig_title.clone();
        let mut bookmark_title = orig_title;
        if read_signed_data_url(&title, &self.certificate_error_page_sign_key).is_some()
            || read_signed_data_url(&title, &self.file_scheme_blocked_page_sign_key).is_some()
        {
            // Do not show error message data URLs as titles.
            title = "Error".to_string();
            bookmark_title = String::new();
        }

        if *self.title.borrow() != title {
            *self.title.borrow_mut() = title;
            let this = self.rc();
            crate::ui::post_task(move || {
                if this.state.get() == State::Open {
                    this.signal_title_changed();
                }
            });
        }
        self.root_widget.control_bar().set_page_title(bookmark_title);
    }

    fn on_cursor_change(&self, type_: CursorType) {
        if self.state.get() != State::Open {
            return;
        }
        let cursor = if type_ == CursorType::HAND {
            HAND_CURSOR
        } else if type_ == CursorType::IBEAM {
            TEXT_CURSOR
        } else {
            NORMAL_CURSOR
        };
        self.browser_area().set_cursor(cursor);
    }

    fn on_certificate_error(&self, request_url: String, callback: Option<&mut Callback>) -> bool {
        if let Some(domain) = extract_domain_from_https_url(&request_url) {
            if globals().config.certificate_check_exceptions.contains(&domain) {
                let Some(callback) = callback else {
                    crate::panic_log!("Requirement 'callback' failed");
                };
                callback.cont();
                return true;
            }
            crate::info_log!(
                "Invalid certificate for domain '{}', canceling request. To add an exception, \
                 use the --certificate-check-exceptions command line option.",
                domain
            );
        }
        *self.last_certificate_error_url.borrow_mut() = Some(request_url);
        false
    }

    fn on_find_result(&self, identifier: i32, count: i32, final_update: bool) {
        if self.state.get() == State::Open && identifier >= self.last_find_id.get() {
            if count > 0 || final_update {
                self.root_widget.control_bar().set_find_result(count > 0);
            }
            self.last_find_id.set(identifier);
        }
    }

    fn on_pre_key_event(&self, event: &KeyEvent) -> bool {
        if self.state.get() == State::Open
            && event.type_ == KeyEventType::RAWKEYDOWN
            && event.windows_key_code == -keys::BACKSPACE
            && event.focus_on_editable_field == 0
        {
            self.navigate(if event.modifiers & EVENTFLAG_SHIFT_DOWN != 0 { 1 } else { -1 });
            return true;
        }
        false
    }

    fn on_file_dialog(&self, mode: FileDialogMode, callback: &FileDialogCallback) {
        if self.state.get() != State::Open
            || (mode != FileDialogMode::OPEN && mode != FileDialogMode::OPEN_MULTIPLE)
        {
            callback.cancel();
            return;
        }
        if self.file_upload_callback.borrow().is_some() {
            crate::warning_log!(
                "Cannot upload in window {} because the window is already in upload mode",
                self.handle
            );
            callback.cancel();
            return;
        }
        if self.handler().on_window_start_file_upload(self.handle) {
            *self.file_upload_callback.borrow_mut() = Some(callback.clone());
        } else {
            crate::warning_log!(
                "Cannot upload in window {} because the vice plugin does not allow it",
                self.handle
            );
            callback.cancel();
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        if self.state.get() != State::CleanupComplete {
            crate::panic_log!("Requirement 'state_ == CleanupComplete' failed");
        }
    }
}

// --- Widget / control bar / browser area / download manager events. ---

impl WidgetParent for Window {
    fn on_widget_view_dirty(&self) {
        if self.state.get() != State::Open {
            return;
        }
        let this = self.rc();
        crate::ui::post_task(move || {
            if this.state.get() == State::Open {
                this.root_widget.render();
                this.signal_image_changed();
            }
        });
    }

    fn on_widget_cursor_changed(&self) {
        if self.state.get() != State::Open {
            return;
        }
        let this = self.rc();
        crate::ui::post_task(move || {
            if this.state.get() == State::Open {
                let cursor = this.root_widget.cursor();
                require!((0..CURSOR_TYPE_COUNT).contains(&cursor));
                this.handler().on_window_cursor_changed(this.handle, cursor);
            }
        });
    }

    fn on_global_hotkey_pressed(&self, key: GlobalHotkey) {
        if self.state.get() != State::Open {
            return;
        }
        let this = self.rc();
        crate::ui::post_task(move || {
            if this.state.get() != State::Open {
                return;
            }
            let control_bar = this.root_widget.control_bar();
            match key {
                GlobalHotkey::Address => control_bar.activate_address(),
                GlobalHotkey::Find => control_bar.open_find_bar(),
                GlobalHotkey::FindNext => control_bar.find_next(),
                GlobalHotkey::Refresh => this.navigate(0),
                GlobalHotkey::ZoomIn => this.zoom_in(),
                GlobalHotkey::ZoomOut => this.zoom_out(),
                GlobalHotkey::ZoomReset => this.zoom_reset(),
            }
        });
    }
}

impl ControlBarEventHandler for Window {
    fn on_address_submitted(&self, url: String) {
        if self.state.get() != State::Open || url.is_empty() {
            return;
        }
        if let Some(frame) = self.browser().and_then(|b| b.main_frame()) {
            frame.load_url(Some(&CefString::from(url.as_str())));
            self.browser_area().take_focus();
        }
    }

    fn on_quality_changed(&self, idx: usize) {
        if self.state.get() == State::Open {
            self.handler().on_window_quality_changed(self.handle, idx);
        }
    }

    fn on_pending_download_accepted(&self) {
        if self.state.get() == State::Open {
            self.download_manager.accept_pending_download();
        }
    }

    fn on_find(&self, text: String, forward: bool, find_next: bool) {
        if self.state.get() == State::Open
            && let Some(host) = self.host()
        {
            host.find(
                Some(&CefString::from(text.as_str())),
                forward as i32,
                0,
                find_next as i32,
            );
        }
    }

    fn on_stop_find(&self, clear_selection: bool) {
        if self.state.get() == State::Open
            && let Some(host) = self.host()
        {
            host.stop_finding(clear_selection as i32);
        }
    }

    fn on_clipboard_button_pressed(&self) {
        if self.state.get() == State::Open {
            self.handler().on_window_clipboard_button_pressed(self.handle);
        }
    }

    fn on_open_bookmarks_button_pressed(&self) {
        if self.state.get() != State::Open {
            return;
        }
        let mut open_popup = true;
        if let Some(frame) = self.browser().and_then(|b| b.main_frame()) {
            let url = userfree_to_string(&frame.url());
            if url == "about:blank" || url == "browservice://bookmarks/" {
                open_popup = false;
            }
        }

        if open_popup {
            crate::info_log!(
                "Bookmark button pressed in window {}, opening bookmark popup",
                self.handle
            );
            let mut popup_denied = true;
            let event_handler = self.handler();
            self.request_popup(&mut |new_handle| {
                crate::info_log!("Creating bookmark popup window {}", new_handle);
                popup_denied = false;
                Window::try_create(
                    event_handler.clone(),
                    self.request_context.clone(),
                    new_handle,
                    Some("browservice://bookmarks/".to_string()),
                    self.show_soft_navigation_buttons,
                )
            });
            if popup_denied {
                crate::warning_log!(
                    "Creating bookmark popup window failed because request was denied"
                );
            }
        } else {
            crate::info_log!(
                "Bookmark button pressed in window {}, navigating to bookmarks",
                self.handle
            );
            self.navigate_to_uri("browservice://bookmarks/");
        }
    }

    fn on_navigation_button_pressed(&self, direction: i32) {
        if self.state.get() == State::Open {
            self.navigate(direction);
        }
    }

    fn on_home_button_pressed(&self) {
        if self.state.get() == State::Open {
            self.navigate_to_uri(&globals().config.start_page);
        }
    }
}

impl BrowserAreaEventHandler for Window {
    fn on_browser_area_view_dirty(&self) {
        if self.state.get() == State::Open {
            self.signal_image_changed();
        }
    }
}

impl DownloadManagerEventHandler for Window {
    fn on_pending_download_count_changed(&self, count: i32) {
        self.root_widget.control_bar().set_pending_download_count(count);
    }

    fn on_download_progress_changed(&self, progress: Vec<i32>) {
        self.root_widget.control_bar().set_download_progress(progress);
    }

    fn on_download_completed(&self, file: Arc<CompletedDownload>) {
        if self.state.get() == State::Open {
            self.handler().on_window_download_completed(self.handle, file);
        }
    }
}

// --- CEF handler objects. ---

/// Looks up the window and performs BROWSER_EVENT_HANDLER_CHECKS: UI thread,
/// window Open or Closed, and `browser` is the browser of the window.
fn checked_window(handle: u64, browser: Option<&mut Browser>) -> Option<(Rc<Window>, Browser)> {
    crate::ui::require_ui_thread();
    let window = lookup_window(handle)?;
    let state = window.state.get();
    require!(state == State::Open || state == State::Closed);
    let Some(browser) = browser else {
        crate::panic_log!("Requirement 'browser' failed");
    };
    let Some(mut own) = window.browser() else {
        crate::panic_log!("Requirement 'window_->browser_' failed");
    };
    require!(browser.is_same(Some(&mut own)) != 0);
    Some((window, browser.clone()))
}

wrap_life_span_handler! {
    struct WindowLifeSpanHandler {
        handle: u64,
    }

    impl LifeSpanHandler {
        fn on_before_popup(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            _target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            window_info: Option<&mut WindowInfo>,
            client: Option<&mut Option<Client>>,
            settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            match checked_window(self.handle, browser) {
                Some((window, _)) => window.on_before_popup(window_info, client, settings) as i32,
                None => 1,
            }
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            crate::ui::require_ui_thread();
            let Some(browser) = browser else {
                crate::panic_log!("Requirement 'browser' failed");
            };
            if let Some(window) = lookup_window(self.handle) {
                window.on_after_created(browser);
            }
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some((window, _)) = checked_window(self.handle, browser) {
                window.on_before_close();
            }
        }
    }
}

wrap_load_handler! {
    struct WindowLoadHandler {
        handle: u64,
    }

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            browser: Option<&mut Browser>,
            is_loading: ::std::os::raw::c_int,
            _can_go_back: ::std::os::raw::c_int,
            _can_go_forward: ::std::os::raw::c_int,
        ) {
            if let Some((window, _)) = checked_window(self.handle, browser) {
                window.on_loading_state_change(is_loading != 0);
            }
        }

        fn on_load_start(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _transition_type: TransitionType,
        ) {
            if let Some((window, _)) = checked_window(self.handle, browser)
                && let Some(frame) = frame
            {
                window.on_load_start(frame);
            }
        }

        fn on_load_error(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            if let Some((window, _)) = checked_window(self.handle, browser)
                && let Some(frame) = frame
            {
                window.on_load_error(
                    frame,
                    error_code,
                    error_text.map(|s| s.to_string()).unwrap_or_default(),
                    failed_url.map(|s| s.to_string()).unwrap_or_default(),
                );
            }
        }
    }
}

wrap_display_handler! {
    struct WindowDisplayHandler {
        handle: u64,
    }

    impl DisplayHandler {
        fn on_address_change(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            if let Some((window, _)) = checked_window(self.handle, browser) {
                window.on_address_change(url.map(|s| s.to_string()).unwrap_or_default());
            }
        }

        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            if let Some((window, _)) = checked_window(self.handle, browser) {
                window.on_title_change(title.map(|s| s.to_string()).unwrap_or_default());
            }
        }

        fn on_cursor_change(
            &self,
            browser: Option<&mut Browser>,
            _cursor: ::std::os::raw::c_ulong,
            type_: CursorType,
            _custom_cursor_info: Option<&CursorInfo>,
        ) -> ::std::os::raw::c_int {
            if let Some((window, _)) = checked_window(self.handle, browser) {
                window.on_cursor_change(type_);
            }
            1
        }
    }
}

wrap_request_handler! {
    struct WindowRequestHandler {
        handle: u64,
    }

    impl RequestHandler {
        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: ::std::os::raw::c_int,
            _is_download: ::std::os::raw::c_int,
            _request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut ::std::os::raw::c_int>,
        ) -> Option<ResourceRequestHandler> {
            // IO thread: only the handle is used here.
            let handle = self.handle;
            crate::ui::post_task_from_any_thread(move || {
                if let Some(window) = lookup_window(handle) {
                    window.update_security_status();
                }
            });
            None
        }

        fn on_before_browse(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: ::std::os::raw::c_int,
            _is_redirect: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            checked_window(self.handle, browser);
            let Some(request) = request else {
                crate::panic_log!("Requirement 'request' failed");
            };
            // Block local file requests (on_load_error redirects to an error
            // message) if blocking is enabled.
            let url = userfree_to_string(&request.url());
            (globals().config.block_file_scheme && is_local_file_request_url(&url)) as i32
        }

        fn on_certificate_error(
            &self,
            browser: Option<&mut Browser>,
            _cert_error: Errorcode,
            request_url: Option<&CefString>,
            _ssl_info: Option<&mut Sslinfo>,
            callback: Option<&mut Callback>,
        ) -> ::std::os::raw::c_int {
            match checked_window(self.handle, browser) {
                Some((window, _)) => window.on_certificate_error(
                    request_url.map(|s| s.to_string()).unwrap_or_default(),
                    callback,
                ) as i32,
                None => 0,
            }
        }
    }
}

wrap_find_handler! {
    struct WindowFindHandler {
        handle: u64,
    }

    impl FindHandler {
        fn on_find_result(
            &self,
            browser: Option<&mut Browser>,
            identifier: ::std::os::raw::c_int,
            count: ::std::os::raw::c_int,
            _selection_rect: Option<&cef::Rect>,
            _active_match_ordinal: ::std::os::raw::c_int,
            final_update: ::std::os::raw::c_int,
        ) {
            if let Some((window, _)) = checked_window(self.handle, browser) {
                window.on_find_result(identifier, count, final_update != 0);
            }
        }
    }
}

wrap_keyboard_handler! {
    struct WindowKeyboardHandler {
        handle: u64,
    }

    impl KeyboardHandler {
        fn on_pre_key_event(
            &self,
            browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            _os_event: Option<&mut sys::XEvent>,
            _is_keyboard_shortcut: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            match (checked_window(self.handle, browser), event) {
                (Some((window, _)), Some(event)) => window.on_pre_key_event(event) as i32,
                _ => 0,
            }
        }
    }
}

wrap_dialog_handler! {
    struct WindowDialogHandler {
        handle: u64,
    }

    impl DialogHandler {
        fn on_file_dialog(
            &self,
            browser: Option<&mut Browser>,
            mode: FileDialogMode,
            _title: Option<&CefString>,
            _default_file_path: Option<&CefString>,
            _accept_filters: Option<&mut CefStringList>,
            _accept_extensions: Option<&mut CefStringList>,
            _accept_descriptions: Option<&mut CefStringList>,
            callback: Option<&mut FileDialogCallback>,
        ) -> ::std::os::raw::c_int {
            let window = checked_window(self.handle, browser);
            let Some(callback) = callback else {
                crate::panic_log!("Requirement 'callback' failed");
            };
            match window {
                Some((window, _)) => window.on_file_dialog(mode, callback),
                None => callback.cancel(),
            }
            1
        }
    }
}

wrap_context_menu_handler! {
    struct WindowContextMenuHandler {
        handle: u64,
    }

    impl ContextMenuHandler {
        fn on_before_context_menu(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
        ) {
            checked_window(self.handle, browser);
            let Some(model) = model else {
                crate::panic_log!("Requirement 'model' failed");
            };
            model.clear();
        }
    }
}

wrap_render_handler! {
    // `BrowserArea::RenderHandler`: forwards to the browser area of the
    // window.
    struct WindowRenderHandler {
        handle: u64,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut cef::Rect>) {
            crate::ui::require_ui_thread();
            let (width, height) = match lookup_window(self.handle) {
                Some(window) => window.browser_area().view_rect(),
                None => (800, 600),
            };
            if let Some(rect) = rect {
                *rect = cef::Rect { x: 0, y: 0, width, height };
            }
        }

        fn screen_info(
            &self,
            browser: Option<&mut Browser>,
            screen_info: Option<&mut ScreenInfo>,
        ) -> ::std::os::raw::c_int {
            let mut rect = cef::Rect::default();
            self.view_rect(browser, Some(&mut rect));
            if let Some(info) = screen_info {
                info.device_scale_factor = 1.0;
                info.rect = rect.clone();
                info.available_rect = rect;
            }
            1
        }

        fn on_popup_show(&self, browser: Option<&mut Browser>, show: ::std::os::raw::c_int) {
            crate::ui::require_ui_thread();
            if let (Some(window), Some(browser)) = (lookup_window(self.handle), browser) {
                window.browser_area().on_popup_show(browser, show != 0);
            }
        }

        fn on_popup_size(&self, browser: Option<&mut Browser>, rect: Option<&cef::Rect>) {
            crate::ui::require_ui_thread();
            if let (Some(window), Some(browser), Some(rect)) =
                (lookup_window(self.handle), browser, rect)
            {
                window.browser_area().on_popup_size(browser, rect);
            }
        }

        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            dirty_rects: Option<&[cef::Rect]>,
            buffer: *const u8,
            width: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ) {
            crate::ui::require_ui_thread();
            if buffer.is_null() {
                return;
            }
            if let Some(window) = lookup_window(self.handle) {
                window.browser_area().on_paint(
                    type_,
                    dirty_rects.unwrap_or(&[]),
                    buffer,
                    width,
                    height,
                );
            }
        }
    }
}

wrap_client! {
    // `Window::Client`: the CEF client of a window.
    struct WindowClient {
        life_span: LifeSpanHandler,
        load: LoadHandler,
        display: DisplayHandler,
        request: RequestHandler,
        find: FindHandler,
        keyboard: KeyboardHandler,
        dialog: DialogHandler,
        context_menu: ContextMenuHandler,
        render: RenderHandler,
        download: DownloadHandler,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(self.life_span.clone())
        }
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.render.clone())
        }
        fn load_handler(&self) -> Option<LoadHandler> {
            Some(self.load.clone())
        }
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(self.display.clone())
        }
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(self.request.clone())
        }
        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(self.download.clone())
        }
        fn find_handler(&self) -> Option<FindHandler> {
            Some(self.find.clone())
        }
        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(self.keyboard.clone())
        }
        fn dialog_handler(&self) -> Option<DialogHandler> {
            Some(self.dialog.clone())
        }
        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            Some(self.context_menu.clone())
        }
    }
}

impl WindowClient {
    fn create(handle: u64) -> Client {
        WindowClient::new(
            WindowLifeSpanHandler::new(handle),
            WindowLoadHandler::new(handle),
            WindowDisplayHandler::new(handle),
            WindowRequestHandler::new(handle),
            WindowFindHandler::new(handle),
            WindowKeyboardHandler::new(handle),
            WindowDialogHandler::new(handle),
            WindowContextMenuHandler::new(handle),
            WindowRenderHandler::new(handle),
            WindowDownloadHandler::new(handle),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert!(is_local_file_request_url("file:///etc/passwd"));
        assert!(is_local_file_request_url("FILE:///x"));
        assert!(!is_local_file_request_url("https://x/"));
        assert_eq!(
            extract_domain_from_https_url("https://example.com/a/b").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            extract_domain_from_https_url("https://example.com:8443").as_deref(),
            Some("example.com:8443")
        );
        assert_eq!(extract_domain_from_https_url("https://"), None);
        assert_eq!(extract_domain_from_https_url("https:///x"), None);
        assert_eq!(extract_domain_from_https_url("http://a/"), None);
        assert!((zoom_factor_to_zoom_level(1.0)).abs() < 1e-12);
        assert!((zoom_factor_to_zoom_level(1.2) - 1.0).abs() < 1e-12);
        assert_eq!(zoom_factor_to_zoom_level(100.0), zoom_factor_to_zoom_level(4.0));
        assert_eq!(zoom_factor_to_zoom_level(f64::NAN), 0.0);
    }
}
