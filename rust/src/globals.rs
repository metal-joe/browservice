//! Paths and directories, ported from `src/globals.cpp`.

#[cfg(not(windows))]
use std::ffi::CStr;
use std::path::Path;

/// The roaming application data folder (`FOLDERID_RoamingAppData`).
#[cfg(windows)]
fn app_data_path() -> String {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_RoamingAppData, SHGetKnownFolderPath};
    unsafe {
        let mut path: windows_sys::core::PWSTR = std::ptr::null_mut();
        if SHGetKnownFolderPath(&FOLDERID_RoamingAppData, 0, std::ptr::null_mut(), &mut path) != 0 {
            crate::panic_log!(
                "Requirement 'SHGetKnownFolderPath(FOLDERID_RoamingAppData, 0, nullptr, &str) == S_OK' failed"
            );
        }
        let mut len = 0;
        while *path.add(len) != 0 {
            len += 1;
        }
        let ret = String::from_utf16_lossy(std::slice::from_raw_parts(path, len));
        CoTaskMemFree(path as *const std::ffi::c_void);
        ret
    }
}

/// `%APPDATA%\Browservice`, the directory for Browservice's own files.
#[cfg(windows)]
pub fn dot_dir_path() -> String {
    format!("{}\\Browservice", app_data_path())
}

/// Creates the directory unless it already exists as a directory.
#[cfg(windows)]
pub fn ensure_dir_exists(path: &str) -> bool {
    match std::fs::create_dir(path) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Path::new(path).is_dir(),
        Err(_) => false,
    }
}

#[cfg(not(windows))]
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
#[cfg(not(windows))]
pub fn dot_dir_path() -> String {
    format!("{}/.browservice", home_dir_path())
}

/// Creates the directory (mode 0700) unless it already exists as a directory.
#[cfg(not(windows))]
pub fn ensure_dir_exists(path: &str) -> bool {
    use std::os::unix::fs::DirBuilderExt;
    match std::fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Path::new(path).is_dir(),
        Err(_) => false,
    }
}

/// Global state shared by the whole program (`Globals` in
/// `src/globals.{hpp,cpp}`). The text render context is not `Send` and is
/// kept per thread instead (`text::global_text_render_context`, created by
/// `main` on the UI thread). Set once before CEF is initialized; may be read
/// from any thread.
pub struct Globals {
    pub config: crate::config::Config,
    pub dot_dir_path: String,
}

static GLOBALS: std::sync::OnceLock<Globals> = std::sync::OnceLock::new();

/// Initializes the globals, panicking if the dot directory or the data
/// directory cannot be created (like the C++ `Globals` constructor).
pub fn init_globals(config: crate::config::Config) {
    let dot_dir_path = dot_dir_path();
    if !ensure_dir_exists(&dot_dir_path) {
        crate::panic_log!(
            "Directory '{}' does not exist and creating it failed",
            dot_dir_path
        );
    }
    if !config.data_dir.is_empty() && !ensure_dir_exists(&config.data_dir) {
        crate::panic_log!(
            "Data directory '{}' does not exist and creating it failed",
            config.data_dir
        );
    }
    if GLOBALS
        .set(Globals {
            config,
            dot_dir_path,
        })
        .is_err()
    {
        crate::panic_log!("Globals initialized twice");
    }
}

pub fn globals() -> &'static Globals {
    match GLOBALS.get() {
        Some(g) => g,
        None => crate::panic_log!("Globals used before initialization"),
    }
}

/// For unit tests: the globals if they have been initialized.
pub fn try_globals() -> Option<&'static Globals> {
    GLOBALS.get()
}
