//! Port of `src/temp_dir.{hpp,cpp}`: a temporary directory created with
//! `mkdtemp` and removed (it must be empty by then) when dropped.

use std::ffi::{CStr, CString};

pub struct TempDir {
    path: String,
}

impl TempDir {
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
        assert!(path.starts_with("/tmp/browservicetmp_"));
        assert!(std::path::Path::new(&path).is_dir());
        drop(dir);
        assert!(!std::path::Path::new(&path).exists());
    }
}
