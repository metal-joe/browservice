//! Port of `src/server.{hpp,cpp}`: the root object of the browser server,
//! connecting the vice plugin context to the browser windows. Before quitting
//! the CEF message loop, call [`Server::shutdown`] and wait for
//! [`ServerEventHandler::on_server_shutdown_complete`].

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use cef::RequestContext;

use crate::clipboard::{copy_to_clipboard, paste_from_clipboard};
use crate::download_manager::CompletedDownload;
use crate::globals::globals;
use crate::image_slice::ImageSlice;
use crate::vice::{ViceContext, ViceContextEventHandler, ViceFileUpload};
use crate::window::{Window, WindowEventHandler};

pub trait ServerEventHandler {
    fn on_server_shutdown_complete(&self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Running,
    WaitWindows,
    WaitViceContext,
    ShutdownComplete,
}

pub struct Server {
    self_weak: Weak<Server>,
    event_handler: Weak<dyn ServerEventHandler>,
    next_window_handle: Cell<u64>,
    state: Cell<State>,
    vice_ctx: Rc<ViceContext>,
    open_windows: RefCell<BTreeMap<u64, Rc<Window>>>,
    cleanup_windows: RefCell<BTreeMap<u64, Rc<Window>>>,
    clipboard_content_requested: Cell<bool>,
    request_context: RequestContext,
}

macro_rules! require {
    ($cond:expr) => {
        if !($cond) {
            crate::panic_log!("Requirement '{}' failed", stringify!($cond));
        }
    };
}

impl Server {
    pub fn create(
        event_handler: Weak<dyn ServerEventHandler>,
        vice_ctx: Rc<ViceContext>,
        request_context: RequestContext,
    ) -> Rc<Server> {
        crate::ui::require_ui_thread();
        let server = Rc::new_cyclic(|weak| Server {
            self_weak: weak.clone(),
            event_handler,
            next_window_handle: Cell::new(1),
            state: Cell::new(State::Running),
            vice_ctx,
            open_windows: RefCell::new(BTreeMap::new()),
            cleanup_windows: RefCell::new(BTreeMap::new()),
            clipboard_content_requested: Cell::new(false),
            request_context,
        });
        let handler: Rc<dyn ViceContextEventHandler> = server.clone();
        server.vice_ctx.start(handler);
        server
    }

    fn rc(&self) -> Rc<Server> {
        self.self_weak.upgrade().expect("server is alive")
    }

    /// Shuts the server down if it is not already shut down.
    pub fn shutdown(&self) {
        crate::ui::require_ui_thread();
        if self.state.get() != State::Running {
            return;
        }
        self.state.set(State::WaitWindows);
        crate::info_log!("Shutting down server");

        let windows = std::mem::take(&mut *self.open_windows.borrow_mut());
        for (handle, window) in windows {
            window.close();
            self.vice_ctx.close_window(handle);
            let prev = self.cleanup_windows.borrow_mut().insert(handle, window);
            require!(prev.is_none());
        }
        self.check_cleanup_complete();
    }

    fn check_cleanup_complete(&self) {
        if self.state.get() == State::WaitWindows && self.cleanup_windows.borrow().is_empty() {
            require!(self.open_windows.borrow().is_empty());
            self.state.set(State::WaitViceContext);
            self.vice_ctx.shutdown();
        }
    }

    fn require_not_shut_down(&self) {
        crate::ui::require_ui_thread();
        require!(self.state.get() != State::ShutdownComplete);
    }

    fn open_window(&self, handle: u64) -> Rc<Window> {
        self.require_not_shut_down();
        match self.open_windows.borrow().get(&handle) {
            Some(w) => w.clone(),
            None => crate::panic_log!("Requirement 'it != openWindows_.end()' failed"),
        }
    }

    fn require_open(&self, handle: u64) {
        self.require_not_shut_down();
        require!(self.open_windows.borrow().contains_key(&handle));
    }

    fn window_count(&self) -> usize {
        self.open_windows.borrow().len() + self.cleanup_windows.borrow().len()
    }

    fn show_soft_navigation_buttons(&self) -> bool {
        match globals().config.show_soft_navigation_buttons {
            Some(value) => value,
            None => !matches!(self.vice_ctx.has_navigation_controls(), Some(true)),
        }
    }
}

impl ViceContextEventHandler for Server {
    fn on_vice_context_create_window_request(&self, reason: &mut String, uri: Option<String>) -> u64 {
        self.require_not_shut_down();
        if self.state.get() != State::Running {
            *reason = "Server is shutting down".to_string();
            return 0;
        }

        crate::info_log!("Got request for new window from vice plugin");

        if self.window_count() as i64 >= globals().config.window_limit as i64 {
            crate::info_log!("Denying window creation due to window limit");
            *reason = "Maximum number of concurrent windows exceeded".to_string();
            return 0;
        }

        let handle = self.next_window_handle.get();
        self.next_window_handle.set(handle + 1);
        require!(handle != 0);

        let handler: Rc<dyn WindowEventHandler> = self.rc();
        match Window::try_create(
            handler,
            self.request_context.clone(),
            handle,
            uri,
            self.show_soft_navigation_buttons(),
        ) {
            Some(window) => {
                let prev = self.open_windows.borrow_mut().insert(handle, window);
                require!(prev.is_none());
                handle
            }
            None => {
                *reason = "Creating CEF browser for window failed".to_string();
                0
            }
        }
    }

    fn on_vice_context_close_window(&self, handle: u64) {
        let window = self.open_window(handle);
        self.open_windows.borrow_mut().remove(&handle);
        window.close();
        let prev = self.cleanup_windows.borrow_mut().insert(handle, window);
        require!(prev.is_none());
    }

    fn on_vice_context_resize_window(&self, handle: u64, width: i32, height: i32) {
        self.open_window(handle).resize(width, height);
    }

    fn on_vice_context_fetch_window_image(
        &self,
        handle: u64,
        put_image: &mut dyn FnMut(*const u8, usize, usize, usize),
    ) {
        let mut image = self.open_window(handle).fetch_view_image();
        if image.width() < 1 || image.height() < 1 {
            image = ImageSlice::create_image(1, 1, 255);
        }
        put_image(
            image.buf_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.pitch() as usize,
        );
    }

    fn on_vice_context_mouse_down(&self, window: u64, x: i32, y: i32, button: i32) {
        self.open_window(window).send_mouse_down_event(x, y, button);
    }
    fn on_vice_context_mouse_up(&self, window: u64, x: i32, y: i32, button: i32) {
        self.open_window(window).send_mouse_up_event(x, y, button);
    }
    fn on_vice_context_mouse_move(&self, window: u64, x: i32, y: i32) {
        self.open_window(window).send_mouse_move_event(x, y);
    }
    fn on_vice_context_mouse_double_click(&self, window: u64, x: i32, y: i32, button: i32) {
        self.open_window(window).send_mouse_double_click_event(x, y, button);
    }
    fn on_vice_context_mouse_wheel(&self, window: u64, x: i32, y: i32, dx: i32, dy: i32) {
        self.open_window(window).send_mouse_wheel_event(x, y, dx, dy);
    }
    fn on_vice_context_mouse_leave(&self, window: u64, x: i32, y: i32) {
        self.open_window(window).send_mouse_leave_event(x, y);
    }
    fn on_vice_context_key_down(&self, window: u64, key: i32) {
        self.open_window(window).send_key_down_event(key);
    }
    fn on_vice_context_key_up(&self, window: u64, key: i32) {
        self.open_window(window).send_key_up_event(key);
    }
    fn on_vice_context_lose_focus(&self, window: u64) {
        self.open_window(window).send_lose_focus_event();
    }

    fn on_vice_context_navigate(&self, window: u64, direction: i32) {
        self.open_window(window).navigate(direction);
    }

    fn on_vice_context_navigate_to_uri(&self, window: u64, uri: String) {
        self.open_window(window).navigate_to_uri(&uri);
    }

    fn on_vice_context_copy_to_clipboard(&self, text: String) {
        self.require_not_shut_down();
        copy_to_clipboard(&text);
    }

    fn on_vice_context_request_clipboard_content(&self) {
        self.require_not_shut_down();
        if self.clipboard_content_requested.get() {
            return;
        }
        self.clipboard_content_requested.set(true);

        // The response is posted as a task because the vice context functions
        // may not be called from its callbacks.
        let text = paste_from_clipboard();
        let server = self.rc();
        crate::ui::post_task(move || {
            if server.state.get() == State::Running {
                server.clipboard_content_requested.set(false);
                server.vice_ctx.put_clipboard_content(&text);
            }
        });
    }

    fn on_vice_context_upload_file(&self, window: u64, file: Rc<ViceFileUpload>) {
        self.open_window(window).upload_file(file);
    }

    fn on_vice_context_cancel_file_upload(&self, window: u64) {
        self.open_window(window).cancel_file_upload();
    }

    fn on_vice_context_window_title_query(&self, window: u64) -> String {
        self.open_window(window).fetch_title()
    }

    fn on_vice_context_zoom_in(&self, window: u64) {
        self.open_window(window).zoom_in();
    }
    fn on_vice_context_zoom_out(&self, window: u64) {
        self.open_window(window).zoom_out();
    }
    fn on_vice_context_zoom_reset(&self, window: u64) {
        self.open_window(window).zoom_reset();
    }

    fn on_vice_context_shutdown_complete(&self) {
        crate::ui::require_ui_thread();
        require!(self.state.get() == State::WaitViceContext);
        self.state.set(State::ShutdownComplete);
        crate::info_log!("Server shutdown complete");
        let handler = self.event_handler.clone();
        crate::ui::post_task(move || {
            if let Some(handler) = handler.upgrade() {
                handler.on_server_shutdown_complete();
            }
        });
    }
}

impl WindowEventHandler for Server {
    fn on_window_close(&self, handle: u64) {
        let window = self.open_window(handle);
        self.open_windows.borrow_mut().remove(&handle);
        let prev = self.cleanup_windows.borrow_mut().insert(handle, window);
        require!(prev.is_none());
        self.vice_ctx.close_window(handle);
    }

    fn on_window_cleanup_complete(&self, handle: u64) {
        self.require_not_shut_down();
        let removed = self.cleanup_windows.borrow_mut().remove(&handle);
        require!(removed.is_some());
        // The window may still be executing the callback that called this, so
        // it is dropped in a later task.
        crate::ui::post_task(move || drop(removed));
        self.check_cleanup_complete();
    }

    fn on_window_view_image_changed(&self, handle: u64) {
        self.require_open(handle);
        self.vice_ctx.notify_window_view_changed(handle);
    }

    fn on_window_title_changed(&self, handle: u64) {
        self.require_open(handle);
        self.vice_ctx.notify_window_title_changed(handle);
    }

    fn on_window_cursor_changed(&self, handle: u64, cursor: i32) {
        self.require_open(handle);
        self.vice_ctx.set_window_cursor(handle, cursor);
    }

    fn on_window_quality_selector_query(&self, handle: u64) -> Option<(Vec<String>, usize)> {
        self.require_open(handle);
        self.vice_ctx.window_quality_selector_query(handle)
    }

    fn on_window_quality_changed(&self, handle: u64, idx: usize) {
        self.require_open(handle);
        self.vice_ctx.window_quality_changed(handle, idx);
    }

    fn on_window_needs_clipboard_button_query(&self, handle: u64) -> bool {
        self.require_open(handle);
        self.vice_ctx.window_needs_clipboard_button_query(handle)
    }

    fn on_window_clipboard_button_pressed(&self, handle: u64) {
        self.require_open(handle);
        self.vice_ctx.window_clipboard_button_pressed(handle);
    }

    fn on_window_download_completed(&self, handle: u64, file: Arc<CompletedDownload>) {
        self.require_open(handle);
        self.vice_ctx.put_file_download(handle, file);
    }

    fn on_window_start_file_upload(&self, handle: u64) -> bool {
        self.require_open(handle);
        self.vice_ctx.start_file_upload(handle)
    }

    fn on_window_cancel_file_upload(&self, handle: u64) {
        self.require_open(handle);
        self.vice_ctx.cancel_file_upload(handle);
    }

    fn on_window_create_popup_request(
        &self,
        handle: u64,
        accept: &mut dyn FnMut(u64) -> Option<Rc<Window>>,
    ) {
        self.require_open(handle);

        if self.state.get() != State::Running {
            crate::info_log!("Denying popup window request because the server is shutting down");
            return;
        }
        if self.window_count() as i64 >= globals().config.window_limit as i64 {
            crate::info_log!("Denying popup window request due to window limit");
            return;
        }

        let new_handle = self.next_window_handle.get();
        self.next_window_handle.set(new_handle + 1);
        require!(new_handle != 0);

        crate::info_log!(
            "Sending request for the creation of popup window {} (opened by existing window {}) \
             to the vice plugin",
            new_handle,
            handle
        );

        match self.vice_ctx.request_create_popup(handle, new_handle) {
            Ok(()) => {
                crate::info_log!("Popup window creation {} accepted by the vice plugin", new_handle);
                match accept(new_handle) {
                    Some(new_window) => {
                        let prev = self.open_windows.borrow_mut().insert(new_handle, new_window);
                        require!(prev.is_none());
                    }
                    None => {
                        crate::warning_log!(
                            "Creating popup window {} failed, closing it in vice plugin",
                            new_handle
                        );
                        self.vice_ctx.close_window(new_handle);
                    }
                }
            }
            Err(msg) => {
                crate::info_log!(
                    "Popup window {} creation denied by the vice plugin for reason: {}",
                    new_handle,
                    msg
                );
            }
        }
    }
}
