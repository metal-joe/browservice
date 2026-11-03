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
