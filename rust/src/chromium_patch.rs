//! Bindings to the C functions that the Browservice Chromium patch adds to
//! `libcef.so` (see `src/clipboard.cpp` and `src/browser_font_render_mode.cpp`
//! in the original C++ code).
//!
//! A stock CEF distribution does not export these symbols. For linking and
//! running against stock CEF during development and testing, the
//! `stub-patch` cargo feature compiles [`test_stub_patch`], which defines
//! no-op implementations. That feature must never be used for real builds.

use std::ffi::{CStr, CString, c_char, c_int};

unsafe extern "C" {
    fn cef_chromiumBrowserviceClipboardPaste() -> *mut c_char;
    fn cef_chromiumBrowserviceClipboardFreePasteResult(str: *mut c_char);
    fn cef_chromiumBrowserviceClipboardCopy(str: *const c_char);

    fn cef_chromiumBrowserviceFontRenderParamsSetAntialiasingEnabled(enabled: c_int);
    fn cef_chromiumBrowserviceFontRenderParamsSetSubpixelPositioningEnabled(enabled: c_int);
    fn cef_chromiumBrowserviceFontRenderParamsSetAutohinterEnabled(enabled: c_int);
    fn cef_chromiumBrowserviceFontRenderParamsSetUseBitmapsEnabled(enabled: c_int);
    fn cef_chromiumBrowserviceFontRenderParamsSetHinting(val: c_int);
    fn cef_chromiumBrowserviceFontRenderParamsSetSubpixelRendering(val: c_int);
}

/// True if this binary was built with the test-only stub patch symbols.
pub const USING_TEST_STUB: bool = cfg!(feature = "stub-patch");

/// Copies text to the Chromium clipboard. Interior NUL bytes truncate the text
/// (like the C++ version, which passes `c_str()`).
pub fn clipboard_copy(text: &str) {
    let bytes = text.as_bytes();
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let cstr = CString::new(&bytes[..end]).expect("NUL bytes were removed");
    unsafe { cef_chromiumBrowserviceClipboardCopy(cstr.as_ptr()) }
}

/// Reads the text content of the Chromium clipboard.
pub fn clipboard_paste() -> String {
    unsafe {
        let buf = cef_chromiumBrowserviceClipboardPaste();
        if buf.is_null() {
            crate::panic_log!("Requirement 'buf != nullptr' failed");
        }
        let ret = CStr::from_ptr(buf).to_string_lossy().into_owned();
        cef_chromiumBrowserviceClipboardFreePasteResult(buf);
        ret
    }
}

pub mod font_render_params {
    use super::*;

    pub fn set_antialiasing_enabled(enabled: bool) {
        unsafe { cef_chromiumBrowserviceFontRenderParamsSetAntialiasingEnabled(enabled as c_int) }
    }
    pub fn set_subpixel_positioning_enabled(enabled: bool) {
        unsafe {
            cef_chromiumBrowserviceFontRenderParamsSetSubpixelPositioningEnabled(enabled as c_int)
        }
    }
    pub fn set_autohinter_enabled(enabled: bool) {
        unsafe { cef_chromiumBrowserviceFontRenderParamsSetAutohinterEnabled(enabled as c_int) }
    }
    pub fn set_use_bitmaps_enabled(enabled: bool) {
        unsafe { cef_chromiumBrowserviceFontRenderParamsSetUseBitmapsEnabled(enabled as c_int) }
    }
    pub fn set_hinting(val: c_int) {
        unsafe { cef_chromiumBrowserviceFontRenderParamsSetHinting(val) }
    }
    pub fn set_subpixel_rendering(val: c_int) {
        unsafe { cef_chromiumBrowserviceFontRenderParamsSetSubpixelRendering(val) }
    }
}

/// TEST/DEV ONLY stub implementations of the patched CEF symbols. Compiled only
/// with the `stub-patch` feature so that the program can be linked against a
/// stock CEF build. The clipboard stub keeps the text in process memory and
/// the font render parameter setters do nothing.
#[cfg(feature = "stub-patch")]
#[allow(clippy::missing_safety_doc)]
pub mod test_stub_patch {
    use std::ffi::{CStr, CString, c_char, c_int};
    use std::sync::Mutex;

    static CLIPBOARD: Mutex<Vec<u8>> = Mutex::new(Vec::new());

    #[unsafe(no_mangle)]
    pub extern "C" fn cef_chromiumBrowserviceClipboardPaste() -> *mut c_char {
        let data = CLIPBOARD.lock().unwrap().clone();
        CString::new(data).unwrap_or_default().into_raw()
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn cef_chromiumBrowserviceClipboardFreePasteResult(str: *mut c_char) {
        if !str.is_null() {
            drop(unsafe { CString::from_raw(str) });
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn cef_chromiumBrowserviceClipboardCopy(str: *const c_char) {
        if !str.is_null() {
            let bytes = unsafe { CStr::from_ptr(str) }.to_bytes().to_vec();
            *CLIPBOARD.lock().unwrap() = bytes;
        }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn cef_chromiumBrowserviceFontRenderParamsSetAntialiasingEnabled(_: c_int) {}
    #[unsafe(no_mangle)]
    pub extern "C" fn cef_chromiumBrowserviceFontRenderParamsSetSubpixelPositioningEnabled(
        _: c_int,
    ) {
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn cef_chromiumBrowserviceFontRenderParamsSetAutohinterEnabled(_: c_int) {}
    #[unsafe(no_mangle)]
    pub extern "C" fn cef_chromiumBrowserviceFontRenderParamsSetUseBitmapsEnabled(_: c_int) {}
    #[unsafe(no_mangle)]
    pub extern "C" fn cef_chromiumBrowserviceFontRenderParamsSetHinting(_: c_int) {}
    #[unsafe(no_mangle)]
    pub extern "C" fn cef_chromiumBrowserviceFontRenderParamsSetSubpixelRendering(_: c_int) {}
}

#[cfg(all(test, feature = "stub-patch"))]
mod tests {
    #[test]
    fn stub_clipboard_roundtrip() {
        super::clipboard_copy("hello\0ignored");
        assert_eq!(super::clipboard_paste(), "hello");
    }
}
