//! Logging and panicking helpers, mirroring `LogWriter` and `Panicker` in the
//! original `src/common.hpp`.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

/// Writes a log message in the format `SEVERITY @ LOCATION -- MSG` to stderr.
pub fn write_log(severity: &str, location: &str, msg: &str) {
    let line = format!("{severity} @ {location} -- {msg}\n");
    let _ = std::io::stderr().write_all(line.as_bytes());
}

/// Ends the program after writing `PANIC @ LOCATION: MSG` to stderr.
pub fn panic_at(location: &str, msg: &str) -> ! {
    let mut line = format!("PANIC @ {location}");
    if !msg.is_empty() {
        line.push_str(": ");
        line.push_str(msg);
    }
    line.push('\n');
    let mut stderr = std::io::stderr();
    let _ = stderr.write_all(line.as_bytes());
    let _ = stderr.flush();
    std::process::abort();
}

#[macro_export]
macro_rules! info_log {
    ($($arg:tt)*) => {
        $crate::common::write_log(
            "INFO", &format!("{}:{}", file!(), line!()), &format!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! warning_log {
    ($($arg:tt)*) => {
        $crate::common::write_log(
            "WARNING", &format!("{}:{}", file!(), line!()), &format!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! error_log {
    ($($arg:tt)*) => {
        $crate::common::write_log(
            "ERROR", &format!("{}:{}", file!(), line!()), &format!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! panic_log {
    ($($arg:tt)*) => {
        $crate::common::panic_at(&format!("{}:{}", file!(), line!()), &format!($($arg)*))
    };
}

static TERM_SIGNAL_RECEIVED: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_term_signal(signal_id: libc::c_int) {
    // Only async-signal-safe operations are allowed here, so the message is
    // written with a single write(2) call without allocating.
    let _ = signal_id;
    const MSG: &[u8] = b"INFO @ signal handler -- Got termination signal, initiating shutdown\n";
    unsafe {
        libc::write(2, MSG.as_ptr().cast(), MSG.len());
    }
    TERM_SIGNAL_RECEIVED.store(true, Ordering::SeqCst);
}

/// Registers handlers for SIGINT and SIGTERM that set a flag polled by the
/// main loop (see `src/main.cpp`).
pub fn register_term_signal_handler() {
    #[cfg(unix)]
    unsafe {
        let handler = handle_term_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
}

pub fn term_signal_received() -> bool {
    TERM_SIGNAL_RECEIVED.load(Ordering::SeqCst)
}
