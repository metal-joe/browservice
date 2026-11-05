//! Logging and panicking helpers, mirroring `LogWriter` and `Panicker` in the
//! original `src/common.hpp`.

use std::io::Write;
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicBool, Ordering};

/// Writes a log message in the format `SEVERITY @ LOCATION -- MSG` to stderr.
pub fn write_log(severity: &str, location: &str, msg: &str) {
    let line = format!("{severity} @ {location} -- {msg}\n");
    let _ = std::io::stderr().write_all(line.as_bytes());
}

/// Ends the program after writing `PANIC @ LOCATION: MSG` to stderr
/// (`Panicker::panic_` in `src/common.cpp`): with `abort()`, or through CEF's
/// `LOG(FATAL)` once [`enable_panic_using_cef_fatal_error`] has been called.
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

    if PANIC_USING_CEF_FATAL_ERROR.load(Ordering::SeqCst) {
        // LOG(FATAL) in the C++ code: CEF writes a FATAL log message and
        // crashes the process.
        unsafe {
            cef_log(c"rust/src/common.rs".as_ptr(), line!() as c_int, LOG_FATAL, c"".as_ptr());
        }
    }

    std::process::abort();
}

// Declared in `include/internal/cef_logging_internal.h` and exported by
// libcef; not part of the generated bindings.
unsafe extern "C" {
    fn cef_log(file: *const c_char, line: c_int, severity: c_int, message: *const c_char);
}

/// `cef::logging::LOG_FATAL` in `include/base/cef_logging.h` (the severity
/// that the `LOG(FATAL)` macro passes to `cef_log`).
const LOG_FATAL: c_int = 3;

static PANIC_USING_CEF_FATAL_ERROR: AtomicBool = AtomicBool::new(false);

/// After this call, panics end the program through CEF's fatal error
/// handling (`LOG(FATAL)`) instead of `abort()`, like
/// `enablePanicUsingCEFFatalError` in the C++ code. Should only be called
/// after CEF has been initialized successfully.
pub fn enable_panic_using_cef_fatal_error() {
    PANIC_USING_CEF_FATAL_ERROR.store(true, Ordering::SeqCst);
}

/// Routes Rust panics (failed `assert!`, `unwrap`, `expect`, ...) to
/// [`panic_at`], so that they are reported in the same `PANIC @ LOCATION: MSG`
/// format and end the program the same way as failed `REQUIRE` checks.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = match info.location() {
            Some(loc) => format!("{}:{}", loc.file(), loc.line()),
            None => "unknown location".to_string(),
        };
        let payload = info.payload();
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            String::new()
        };
        panic_at(&location, &msg);
    }));
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

/// Path separator used when building paths (`PathSep` in `src/common.hpp`).
pub const PATH_SEP: char = if cfg!(windows) { '\\' } else { '/' };

static TERM_SIGNAL_RECEIVED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
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

/// Console control handler (Ctrl+C, Ctrl+Break, console close, logoff,
/// shutdown) of the Windows build (`handleTermSignal` in `src/main.cpp`).
#[cfg(windows)]
unsafe extern "system" fn handle_term_signal(ctrl_type: u32) -> windows_sys::core::BOOL {
    crate::info_log!("Got control signal {}, initiating shutdown", ctrl_type);
    TERM_SIGNAL_RECEIVED.store(true, Ordering::SeqCst);
    1
}

/// Registers handlers for SIGINT and SIGTERM (on Windows, a console control
/// handler) that set a flag polled by the main loop (see `src/main.cpp`).
pub fn register_term_signal_handler() {
    #[cfg(unix)]
    unsafe {
        let handler = handle_term_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
    #[cfg(windows)]
    {
        let ok = unsafe {
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(handle_term_signal), 1)
        };
        if ok == 0 {
            crate::panic_log!("Requirement 'SetConsoleCtrlHandler(handleTermSignal, TRUE)' failed");
        }
    }
}

/// The command line arguments of the program. On Windows they are read from
/// the wide-character command line with `GetCommandLineW` and
/// `CommandLineToArgvW` and converted to UTF-8, like `RunConsoleMain` and
/// `Config::read(int, const wchar_t* const*, bool&)` in the C++ code. Returns
/// None if the command line cannot be read.
pub fn command_line_args() -> Option<Vec<String>> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::System::Environment::GetCommandLineW;
        use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

        let command_line = GetCommandLineW();
        if command_line.is_null() {
            return None;
        }
        let mut argc: i32 = 0;
        let argv = CommandLineToArgvW(command_line, &mut argc);
        if argv.is_null() {
            return None;
        }
        let mut args = Vec::with_capacity(argc.max(0) as usize);
        for i in 0..argc.max(0) as usize {
            let arg = *argv.add(i);
            let mut len = 0;
            while *arg.add(len) != 0 {
                len += 1;
            }
            args.push(String::from_utf16_lossy(std::slice::from_raw_parts(arg, len)));
        }
        LocalFree(argv.cast());
        Some(args)
    }
    #[cfg(not(windows))]
    {
        Some(std::env::args().collect())
    }
}

pub fn term_signal_received() -> bool {
    TERM_SIGNAL_RECEIVED.load(Ordering::SeqCst)
}

/// Calls `handler(bytes, code_point)` for each valid UTF-8 encoded character
/// in `s`, skipping NUL bytes and invalid sequences (`sanitizeUTF8StringImpl`
/// in `src/common.cpp`).
fn sanitize_utf8_impl(s: &[u8], mut handler: impl FnMut(&[u8], u32)) {
    let mut i = 0;
    while i < s.len() {
        let ch = s[i] as u32;
        if ch == 0 {
            i += 1;
            continue;
        }
        if ch < 128 {
            handler(&s[i..i + 1], ch);
            i += 1;
            continue;
        }

        let length = if (ch & 0xE0) == 0xC0 {
            2
        } else if (ch & 0xF0) == 0xE0 {
            3
        } else if (ch & 0xF8) == 0xF0 {
            4
        } else {
            i += 1;
            continue;
        };

        if i + length > s.len() {
            i += 1;
            continue;
        }

        let mut ok = true;
        let mut point = ch & (0x7F >> length);
        for j in 1..length {
            let ch2 = s[i + j] as u32;
            if (ch2 & 0xC0) != 0x80 {
                ok = false;
                break;
            }
            point = (point << 6) | (ch2 & 0x3F);
        }
        if ok
            && ((length == 2 && (0x80..=0x7FF).contains(&point))
                || (length == 3
                    && ((0x800..=0xD7FF).contains(&point) || (0xE000..=0xFFFF).contains(&point)))
                || (length == 4 && (0x10000..=0x10FFFF).contains(&point)))
        {
            handler(&s[i..i + length], point);
            i += length;
        } else {
            i += 1;
        }
    }
}

/// Removes NUL bytes and invalid UTF-8 sequences (`sanitizeUTF8String`).
pub fn sanitize_utf8_string(s: &[u8]) -> String {
    let mut ret = Vec::with_capacity(s.len());
    sanitize_utf8_impl(s, |bytes, _| ret.extend_from_slice(bytes));
    String::from_utf8(ret).expect("sanitized string is valid UTF-8")
}

/// Code points of the valid characters of `s` (`sanitizeUTF8StringToCodePoints`).
pub fn sanitize_utf8_string_to_code_points(s: &[u8]) -> Vec<u32> {
    let mut ret = Vec::new();
    sanitize_utf8_impl(s, |_, point| ret.push(point));
    ret
}

/// Fills `buf` with random bytes from the operating system.
#[cfg(windows)]
pub fn random_bytes(buf: &mut [u8]) {
    use windows_sys::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };
    for chunk in buf.chunks_mut(u32::MAX as usize) {
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                chunk.as_mut_ptr(),
                chunk.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status != 0 {
            crate::panic_log!("Reading random bytes failed");
        }
    }
}

/// Fills `buf` with random bytes from the operating system.
#[cfg(not(windows))]
pub fn random_bytes(buf: &mut [u8]) {
    let mut filled = 0;
    while filled < buf.len() {
        let ret = unsafe {
            libc::getrandom(
                buf[filled..].as_mut_ptr().cast(),
                buf.len() - filled,
                0,
            )
        };
        if ret < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            crate::panic_log!("Reading random bytes failed");
        }
        filled += ret as usize;
    }
}

pub fn random_u64() -> u64 {
    let mut buf = [0u8; 8];
    random_bytes(&mut buf);
    u64::from_le_bytes(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run in a child process by `panic_hook_reports_like_panic_macro`.
    #[test]
    fn panic_hook_child() {
        if std::env::var_os("BROWSERVICE_PANIC_HOOK_CHILD").is_none() {
            return;
        }
        install_panic_hook();
        let v: Option<u32> = None;
        v.expect("value missing");
    }

    #[cfg(unix)]
    #[test]
    fn panic_hook_reports_like_panic_macro() {
        use std::os::unix::process::ExitStatusExt;
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "common::tests::panic_hook_child", "--nocapture", "--test-threads=1"])
            .env("BROWSERVICE_PANIC_HOOK_CHILD", "1")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("PANIC @ rust/src/common.rs:") && stderr.contains(": value missing\n"),
            "unexpected stderr: {stderr}"
        );
        assert_eq!(output.status.signal(), Some(libc::SIGABRT));
    }

    #[test]
    fn sanitize() {
        assert_eq!(sanitize_utf8_string(b"abc"), "abc");
        assert_eq!(sanitize_utf8_string(b"a\0b"), "ab");
        assert_eq!(sanitize_utf8_string("\u{e4}\u{20ac}\u{1f600}".as_bytes()), "\u{e4}\u{20ac}\u{1f600}");
        // Invalid continuation, overlong encoding, surrogate, truncated sequence.
        assert_eq!(sanitize_utf8_string(b"x\xC3(y"), "x(y");
        assert_eq!(sanitize_utf8_string(b"\xC0\x80z"), "z");
        assert_eq!(sanitize_utf8_string(b"\xED\xA0\x80q"), "q");
        assert_eq!(sanitize_utf8_string(b"w\xE2\x82"), "w");
        assert_eq!(
            sanitize_utf8_string_to_code_points("a\u{e4}".as_bytes()),
            vec![0x61, 0xE4]
        );
    }
}
