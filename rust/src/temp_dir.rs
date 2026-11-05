//! Port of `src/temp_dir.{hpp,cpp}`: a temporary directory created with
//! `mkdtemp` (on Windows: `browservicetmp_` and 16 random characters in the
//! temporary directory of the user) and removed (it must be empty by then)
//! when dropped.

#[cfg(not(windows))]
use std::ffi::{CStr, CString};

pub struct TempDir {
    path: String,
}

impl TempDir {
    #[cfg(windows)]
    pub fn create() -> TempDir {
        // GetTempPathW, like the C++ code; the result ends with a backslash.
        let mut path = std::env::temp_dir().to_string_lossy().into_owned();
        if !path.ends_with('\\') {
            path.push('\\');
        }
        path.push_str("browservicetmp_");
        const PALETTE: &[u8] = b"abcdefghijklmnopqrstuvABCDEFGHIJKLMNOPQRSTUV0123456789";
        for _ in 0..16 {
            let idx = (crate::common::random_u64() % PALETTE.len() as u64) as usize;
            path.push(PALETTE[idx] as char);
        }
        if std::fs::create_dir(&path).is_err() {
            crate::panic_log!("Requirement 'CreateDirectoryW(path.c_str(), nullptr)' failed");
        }
        TempDir { path }
    }

    #[cfg(not(windows))]
    pub fn create() -> TempDir {
        let mut template = b"/tmp/browservicetmp_XXXXXX\0".to_vec();
        let ret = unsafe { libc::mkdtemp(template.as_mut_ptr().cast()) };
        if ret.is_null() {
            crate::panic_log!("Requirement 'mkdtemp(path) != nullptr' failed");
        }
        let path = CStr::from_bytes_with_nul(&template)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        TempDir { path }
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

impl Drop for TempDir {
    #[cfg(windows)]
    fn drop(&mut self) {
        if std::fs::remove_dir(&self.path).is_err() {
            crate::warning_log!("Deleting temporary directory {} failed", self.path);
        }
    }

    #[cfg(not(windows))]
    fn drop(&mut self) {
        let cpath = CString::new(self.path.as_str()).unwrap();
        if unsafe { libc::rmdir(cpath.as_ptr()) } != 0 {
            crate::warning_log!("Deleting temporary directory {} failed", self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn create_and_remove() {
        let dir = super::TempDir::create();
        let path = dir.path().to_string();
        #[cfg(not(windows))]
        assert!(path.starts_with("/tmp/browservicetmp_"));
        #[cfg(windows)]
        assert!(path.contains("\\browservicetmp_"));
        assert!(std::path::Path::new(&path).is_dir());
        drop(dir);
        assert!(!std::path::Path::new(&path).exists());
    }
}
