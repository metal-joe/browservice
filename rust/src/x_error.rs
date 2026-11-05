//! Ignoring non-fatal X errors, like the `XSetErrorHandler` and
//! `XSetIOErrorHandler` calls in `main` in `src/main.cpp` (not used on
//! Windows). libX11 is linked by `rust/build.rs` (`-lX11` as in the
//! Makefile); the two functions are declared here by hand.

#![cfg(not(windows))]

use std::os::raw::{c_int, c_void};

type XErrorHandler = Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int>;
type XIOErrorHandler = Option<unsafe extern "C" fn(*mut c_void) -> c_int>;

unsafe extern "C" {
    fn XSetErrorHandler(handler: XErrorHandler) -> XErrorHandler;
    fn XSetIOErrorHandler(handler: XIOErrorHandler) -> XIOErrorHandler;
}

unsafe extern "C" fn ignore_x_error(_display: *mut c_void, _event: *mut c_void) -> c_int {
    0
}

unsafe extern "C" fn ignore_x_io_error(_display: *mut c_void) -> c_int {
    0
}

/// Installs X error handlers that ignore non-fatal X errors (the default
/// Xlib handlers print the error and exit the process).
pub fn ignore_non_fatal_x_errors() {
    unsafe {
        XSetErrorHandler(Some(ignore_x_error));
        XSetIOErrorHandler(Some(ignore_x_io_error));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handlers_are_installed() {
        type ErrFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
        type IoFn = unsafe extern "C" fn(*mut c_void) -> c_int;
        let err: ErrFn = ignore_x_error;
        let io: IoFn = ignore_x_io_error;
        ignore_non_fatal_x_errors();
        // XSetErrorHandler returns the previous handler, i.e. ours.
        let prev = unsafe { XSetErrorHandler(Some(err)) };
        assert_eq!(prev.map(|f| f as usize), Some(err as usize));
        let prev = unsafe { XSetIOErrorHandler(Some(io)) };
        assert_eq!(prev.map(|f| f as usize), Some(io as usize));
    }

    unsafe extern "C" {
        fn XOpenDisplay(name: *const std::os::raw::c_char) -> *mut c_void;
        fn XCloseDisplay(display: *mut c_void) -> c_int;
        fn XMapWindow(display: *mut c_void, window: std::os::raw::c_ulong) -> c_int;
        fn XSync(display: *mut c_void, discard: c_int) -> c_int;
    }

    /// With the default Xlib handler, a protocol error (here BadWindow) exits
    /// the process. Needs an X server; skipped when DISPLAY is not set (run
    /// with `xvfb-run -a cargo test ...`).
    #[test]
    fn protocol_error_is_ignored() {
        if std::env::var_os("DISPLAY").is_none() {
            eprintln!("DISPLAY not set, skipping");
            return;
        }
        ignore_non_fatal_x_errors();
        unsafe {
            let display = XOpenDisplay(std::ptr::null());
            assert!(!display.is_null(), "XOpenDisplay failed");
            XMapWindow(display, 0x7fff_fff0);
            XSync(display, 0);
            XCloseDisplay(display);
        }
    }
}
