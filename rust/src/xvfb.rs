//! Port of `src/xvfb.{hpp,cpp}`: a private Xvfb X server run as a child
//! process (not used on Windows).
//!
//! The server is started with `-displayfd`, so that it picks a free display
//! number itself and reports it through a pipe. Access is restricted with a
//! random xauth cookie in an `.Xauthority` file in a temporary directory.
//! `setup_env` points `DISPLAY` and `XAUTHORITY` to the server, and the
//! server is shut down with SIGTERM (and waited for) by `shutdown` or when the
//! object is dropped.

#![cfg(not(windows))]

use std::ffi::CString;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::temp_dir::TempDir;
use crate::{info_log, panic_log, warning_log};

fn parse_display(display_str: &[u8]) -> Option<i32> {
    let s = display_str.strip_suffix(b"\n")?;
    let s = std::str::from_utf8(s).ok()?;
    // `parseString<int>` in the C++ code: the whole string must be an integer.
    let display: i32 = s.parse().ok()?;
    if display >= 0 { Some(display) } else { None }
}

fn generate_cookie() -> String {
    let mut bytes = [0u8; 32];
    crate::common::random_bytes(&mut bytes);
    bytes
        .iter()
        .map(|b| char::from_digit((b % 16) as u32, 16).unwrap())
        .collect()
}

fn add_cookie_to_xauth_file(path: &str, display: i32, cookie: &str) {
    // Like popen(3) in the C++ code: run the command through /bin/sh.
    let xauth_cmd = format!("xauth -f {path} source -");
    let mut proc = match Command::new("/bin/sh")
        .arg("-c")
        .arg(&xauth_cmd)
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(proc) => proc,
        Err(_) => panic_log!("Running xauth failed"),
    };

    let input = format!("add :{display} . {cookie}\n");
    let write_ok = proc
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(input.as_bytes()).is_ok())
        .unwrap_or(false);
    if !write_ok {
        panic_log!("Requirement 'fwrite(input.data(), 1, input.size(), proc) == input.size()' failed");
    }
    match proc.wait() {
        Ok(status) if status.success() => {}
        _ => panic_log!("Running xauth failed (nonzero exit status)"),
    }
}

/// Xvfb X server child process.
pub struct Xvfb {
    x_auth_path: String,
    pid: libc::pid_t,
    display: i32,
    running: bool,
    // Dropped after `Drop::drop` has removed the `.Xauthority` file.
    _temp_dir: TempDir,
}

impl Xvfb {
    pub fn create() -> Xvfb {
        info_log!("Starting Xvfb X server as child process");

        let temp_dir = TempDir::create();
        let x_auth_path = format!("{}/.Xauthority", temp_dir.path());

        // Create empty .Xauthority file
        if std::fs::File::create(&x_auth_path).is_err() {
            panic_log!("Requirement 'fp.good()' failed");
        }

        // Add dummy cookie to stop server from accepting all connections
        add_cookie_to_xauth_file(&x_auth_path, 0, &generate_cookie());

        // Pipe through which Xvfb sends us the display number
        let mut display_fds = [0 as libc::c_int; 2];
        if unsafe { libc::pipe(display_fds.as_mut_ptr()) } != 0 {
            panic_log!("Requirement '!pipe(displayFds)' failed");
        }
        let read_display_fd = display_fds[0];
        let write_display_fd = display_fds[1];

        // Everything the child needs is prepared before fork, because only
        // async-signal-safe functions may be called in the child.
        let write_display_fd_str = CString::new(write_display_fd.to_string()).unwrap();
        let x_auth_path_c = CString::new(x_auth_path.as_str()).unwrap();
        let args: [CString; 8] = [
            c"Xvfb".into(),
            c"-displayfd".into(),
            write_display_fd_str,
            c"-auth".into(),
            x_auth_path_c,
            c"-screen".into(),
            c"0".into(),
            c"640x480x24".into(),
        ];
        let mut argv: Vec<*const libc::c_char> = args.iter().map(|a| a.as_ptr()).collect();
        argv.push(std::ptr::null());
        let child_panic_msg = format!(
            "PANIC @ {}:{}: Requirement 'false' failed\n",
            file!(),
            line!()
        );

        let pid = unsafe { libc::fork() };
        if pid == -1 {
            panic_log!("Requirement 'pid_ != -1' failed");
        }
        if pid == 0 {
            // Xvfb subprocess:
            unsafe {
                libc::close(read_display_fd);

                // Move the X server process to its own process group, as
                // otherwise Ctrl+C sent to the parent would stop the X server
                // before we have time to shut the parent down
                libc::setpgid(0, 0);

                libc::execvp(argv[0], argv.as_ptr());

                // If exec succeeded, this should not be reachable
                libc::write(2, child_panic_msg.as_ptr().cast(), child_panic_msg.len());
                libc::abort();
            }
        }

        // Parent process:
        if unsafe { libc::close(write_display_fd) } != 0 {
            panic_log!("Requirement '!close(writeDisplayFd)' failed");
        }

        let mut display_str = Vec::new();
        let mut buf = [0u8; 64];
        loop {
            let read_count =
                unsafe { libc::read(read_display_fd, buf.as_mut_ptr().cast(), buf.len()) };
            if read_count == 0 {
                break;
            }
            if read_count < 0 {
                if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                    panic_log!("Requirement 'errno == EINTR' failed");
                }
                continue;
            }
            display_str.extend_from_slice(&buf[..read_count as usize]);
        }
        if unsafe { libc::close(read_display_fd) } != 0 {
            panic_log!("Requirement '!close(readDisplayFd)' failed");
        }

        let Some(display) = parse_display(&display_str) else {
            panic_log!("Starting Xvfb failed");
        };

        info_log!("Xvfb X server :{} successfully started", display);

        // Now that we know the display number, we can add the xauth rule we
        // actually use
        add_cookie_to_xauth_file(&x_auth_path, display, &generate_cookie());

        Xvfb {
            x_auth_path,
            pid,
            display,
            running: true,
            _temp_dir: temp_dir,
        }
    }

    pub fn display(&self) -> i32 {
        self.display
    }

    pub fn pid(&self) -> libc::pid_t {
        self.pid
    }

    /// Sets the DISPLAY and XAUTHORITY environment variables to point to
    /// this X server.
    pub fn setup_env(&self) {
        // SAFETY: called from the main thread before CEF is initialized,
        // like `setenv` in the C++ code.
        unsafe {
            std::env::set_var("DISPLAY", format!(":{}", self.display));
            std::env::set_var("XAUTHORITY", &self.x_auth_path);
        }
    }

    /// Shuts down the X server. Run automatically when dropped.
    pub fn shutdown(&mut self) {
        if !self.running {
            return;
        }

        info_log!("Sending SIGTERM to the Xvfb X server child process to shut it down");
        if unsafe { libc::kill(self.pid, libc::SIGTERM) } != 0 {
            warning_log!("Could not send SIGTERM signal to Xvfb, maybe it has already shut down?");
        }

        info_log!("Waiting for Xvfb child process to shut down");
        loop {
            let ret = unsafe { libc::waitpid(self.pid, std::ptr::null_mut(), 0) };
            if ret == self.pid {
                break;
            }
            if ret < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            panic_log!("Requirement 'waitpid(pid_, nullptr, 0) == pid_' failed");
        }

        info_log!("Successfully shut down Xvfb X server");

        if std::fs::remove_file(&self.x_auth_path).is_err() {
            warning_log!("Unlinking file {} failed", self.x_auth_path);
        }

        self.running = false;
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        if self.running {
            self.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_display_like_cpp() {
        assert_eq!(parse_display(b"0\n"), Some(0));
        assert_eq!(parse_display(b"42\n"), Some(42));
        assert_eq!(parse_display(b"42"), None);
        assert_eq!(parse_display(b""), None);
        assert_eq!(parse_display(b"\n"), None);
        assert_eq!(parse_display(b"-1\n"), None);
        assert_eq!(parse_display(b"4x\n"), None);
    }

    #[test]
    fn cookie_format() {
        let cookie = generate_cookie();
        assert_eq!(cookie.len(), 32);
        assert!(cookie.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    }
}
