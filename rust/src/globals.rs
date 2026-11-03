//! Paths and directories, ported from `src/globals.cpp`.

use std::ffi::CStr;
use std::path::Path;

fn home_dir_path() -> String {
    if let Ok(path) = std::env::var("HOME") {
        return path;
    }
    unsafe {
        let uid = libc::getuid();
        let mut buf_size = match libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) {
            n if n > 0 => n as usize,
            _ => 1,
        };
        loop {
            let mut pwd: libc::passwd = std::mem::zeroed();
            let mut buf = vec![0 as libc::c_char; buf_size];
            let mut result: *mut libc::passwd = std::ptr::null_mut();
            let code = libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf_size, &mut result);
            if code == 0 && std::ptr::eq(result, &pwd) {
                return CStr::from_ptr(pwd.pw_dir).to_string_lossy().into_owned();
            }
            if !(code == libc::ERANGE && result.is_null()) {
                crate::panic_log!(
                    "Requirement 'resultCode == ERANGE && resultPtr == nullptr' failed"
                );
            }
            buf_size *= 2;
        }
    }
}

/// `~/.browservice`, the directory for Browservice's own files.
pub fn dot_dir_path() -> String {
    format!("{}/.browservice", home_dir_path())
}

/// Creates the directory (mode 0700) unless it already exists as a directory.
pub fn ensure_dir_exists(path: &str) -> bool {
    use std::os::unix::fs::DirBuilderExt;
    match std::fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Path::new(path).is_dir(),
        Err(_) => false,
    }
}
