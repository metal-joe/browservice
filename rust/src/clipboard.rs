//! Port of `src/clipboard.{hpp,cpp}`: the Chromium clipboard, accessed
//! through the accessor functions of the Browservice Chromium patch (no-op
//! in-process stubs with the test-only `stub-patch` feature).

pub fn copy_to_clipboard(text: &str) {
    crate::chromium_patch::clipboard_copy(text);
}

pub fn paste_from_clipboard() -> String {
    crate::chromium_patch::clipboard_paste()
}
