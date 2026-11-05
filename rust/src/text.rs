//! Port of `src/text.{hpp,cpp}`: text layout and rendering with Pango and
//! FreeType (the PangoFT2 backend), using the same system libraries, font
//! (`Verdana 11` at 72 DPI, resolved through fontconfig), FreeType settings
//! (TrueType interpreter version 35, no antialiasing, no autohinter, hinting
//! enabled) and graymap thresholding as the C++ version, so that the glyphs
//! and metrics are the same.
//!
//! The Pango/PangoFT2/fontconfig functions are declared by hand in [`ffi`]
//! (only the dozen functions used here). The gtk-rs `pango` crate does not
//! cover the PangoFT2 backend (there is no maintained `pangoft2` crate), and
//! the C++ code depends on PangoFT2 rendering into a FreeType graymap; going
//! through cairo instead would change the rasterization. The libraries are
//! found with `pkg-config` in `rust/build.rs` (`pangoft2` and `fontconfig`),
//! like the C++ Makefile (`pkg-config --libs pangoft2`).
//!
//! All objects are used on one thread (the CEF UI thread); they are not
//! `Send`.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::rc::Rc;

use crate::image_slice::{ImageSlice, Rect};

#[allow(non_camel_case_types)]
mod ffi {
    use std::ffi::{c_char, c_double, c_int, c_uchar, c_uint, c_ushort, c_void};

    pub type gboolean = c_int;

    #[repr(C)]
    pub struct PangoFontMap {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct PangoContext {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct PangoFontDescription {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct PangoLanguage {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct PangoLayout {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct PangoLayoutLine {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct FcPattern {
        _private: [u8; 0],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default, Debug)]
    pub struct PangoRectangle {
        pub x: c_int,
        pub y: c_int,
        pub width: c_int,
        pub height: c_int,
    }

    #[repr(C)]
    pub struct FT_Bitmap {
        pub rows: c_uint,
        pub width: c_uint,
        pub pitch: c_int,
        pub buffer: *mut c_uchar,
        pub num_grays: c_ushort,
        pub pixel_mode: c_uchar,
        pub palette_mode: c_uchar,
        pub palette: *mut c_void,
    }

    pub const FT_PIXEL_MODE_GRAY: c_uchar = 2;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub union FcValueUnion {
        pub s: *const c_uchar,
        pub i: c_int,
        pub b: c_int,
        pub d: c_double,
        pub p: *const c_void,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct FcValue {
        pub type_: c_int,
        pub u: FcValueUnion,
    }

    pub const FC_TYPE_BOOL: c_int = 4;
    pub const FC_FALSE: c_int = 0;
    pub const FC_TRUE: c_int = 1;

    pub const PANGO_SCALE: c_int = 1024;
    pub const PANGO_DIRECTION_LTR: c_int = 0;
    pub const G_MAXINT: c_int = c_int::MAX;

    pub type PangoFT2SubstituteFunc = unsafe extern "C" fn(*mut FcPattern, *mut c_void);
    pub type GDestroyNotify = unsafe extern "C" fn(*mut c_void);

    unsafe extern "C" {
        pub fn pango_ft2_font_map_new() -> *mut PangoFontMap;
        pub fn pango_ft2_font_map_set_default_substitute(
            fontmap: *mut PangoFontMap,
            func: Option<PangoFT2SubstituteFunc>,
            data: *mut c_void,
            notify: Option<GDestroyNotify>,
        );
        pub fn pango_ft2_font_map_set_resolution(
            fontmap: *mut PangoFontMap,
            dpi_x: c_double,
            dpi_y: c_double,
        );
        pub fn pango_ft2_render_layout(
            bitmap: *mut FT_Bitmap,
            layout: *mut PangoLayout,
            x: c_int,
            y: c_int,
        );

        pub fn pango_font_map_create_context(fontmap: *mut PangoFontMap) -> *mut PangoContext;
        pub fn pango_context_set_base_dir(context: *mut PangoContext, direction: c_int);
        pub fn pango_context_set_language(context: *mut PangoContext, language: *mut PangoLanguage);
        pub fn pango_language_from_string(language: *const c_char) -> *mut PangoLanguage;
        pub fn pango_font_description_from_string(s: *const c_char) -> *mut PangoFontDescription;
        pub fn pango_font_description_free(desc: *mut PangoFontDescription);

        pub fn pango_layout_new(context: *mut PangoContext) -> *mut PangoLayout;
        pub fn pango_layout_set_font_description(
            layout: *mut PangoLayout,
            desc: *const PangoFontDescription,
        );
        pub fn pango_layout_set_auto_dir(layout: *mut PangoLayout, auto_dir: gboolean);
        pub fn pango_layout_set_single_paragraph_mode(layout: *mut PangoLayout, setting: gboolean);
        pub fn pango_layout_set_text(layout: *mut PangoLayout, text: *const c_char, length: c_int);
        pub fn pango_layout_get_text(layout: *mut PangoLayout) -> *const c_char;
        pub fn pango_layout_get_line_readonly(
            layout: *mut PangoLayout,
            line: c_int,
        ) -> *mut PangoLayoutLine;
        pub fn pango_layout_line_x_to_index(
            line: *mut PangoLayoutLine,
            x_pos: c_int,
            index: *mut c_int,
            trailing: *mut c_int,
        ) -> gboolean;
        pub fn pango_layout_get_cursor_pos(
            layout: *mut PangoLayout,
            index: c_int,
            strong_pos: *mut PangoRectangle,
            weak_pos: *mut PangoRectangle,
        );
        pub fn pango_layout_move_cursor_visually(
            layout: *mut PangoLayout,
            strong: gboolean,
            old_index: c_int,
            old_trailing: c_int,
            direction: c_int,
            new_index: *mut c_int,
            new_trailing: *mut c_int,
        );
        pub fn pango_layout_get_pixel_extents(
            layout: *mut PangoLayout,
            ink_rect: *mut PangoRectangle,
            logical_rect: *mut PangoRectangle,
        );

        pub fn g_object_unref(object: *mut c_void);

        pub fn FcPatternAdd(
            p: *mut FcPattern,
            object: *const c_char,
            value: FcValue,
            append: c_int,
        ) -> c_int;
    }
}

macro_rules! require {
    ($cond:expr) => {
        if !($cond) {
            $crate::panic_log!("Requirement '{}' failed", stringify!($cond));
        }
    };
}

/// Advances `count` UTF-8 characters from byte index `idx` (`jumpUTF8Chars`).
fn jump_utf8_chars(s: &[u8], mut idx: usize, mut count: i32) -> usize {
    require!(idx <= s.len());
    require!(count >= 0);
    while count > 0 {
        count -= 1;
        require!(idx < s.len());
        let c = s[idx] as i32;
        if c & 0x80 != 0 {
            require!(c & 0x40 != 0);
            let bytes = if c & 0x20 == 0 {
                2
            } else if c & 0x10 == 0 {
                3
            } else {
                require!(c & 0x8 == 0);
                4
            };
            require!(idx + bytes <= s.len());
            idx += 1;
            for _ in 0..bytes - 1 {
                require!(s[idx] & 0xC0 == 0x80);
                idx += 1;
            }
        } else {
            idx += 1;
        }
    }
    require!(idx <= s.len());
    if idx != s.len() {
        require!(s[idx] & 0xC0 != 0x80);
    }
    idx
}

/// Sets `FREETYPE_PROPERTIES=truetype:interpreter-version=35` while alive and
/// restores the previous value when dropped (`FreeType2SetEnv`). FreeType only
/// reads the TrueType interpreter version from this environment variable when
/// the library is initialized by the PangoFT2 font map.
#[cfg(not(windows))]
struct FreeType2SetEnv {
    old_value: Option<std::ffi::OsString>,
}

const FREETYPE_PROPERTIES: &str = "FREETYPE_PROPERTIES";

#[cfg(not(windows))]
impl FreeType2SetEnv {
    fn new() -> FreeType2SetEnv {
        let old_value = std::env::var_os(FREETYPE_PROPERTIES);
        // SAFETY: like the C++ version, this runs once while creating the text
        // render context, before CEF is initialized (only CEF's own threads
        // could read the environment concurrently).
        unsafe { std::env::set_var(FREETYPE_PROPERTIES, "truetype:interpreter-version=35") };
        FreeType2SetEnv { old_value }
    }
}

#[cfg(not(windows))]
impl Drop for FreeType2SetEnv {
    fn drop(&mut self) {
        // SAFETY: see FreeType2SetEnv::new.
        unsafe {
            match &self.old_value {
                Some(v) => std::env::set_var(FREETYPE_PROPERTIES, v),
                None => std::env::remove_var(FREETYPE_PROPERTIES),
            }
        }
    }
}

// On Windows, FreeType reads the variable with the C runtime's `getenv`,
// which does not see changes made with `SetEnvironmentVariableW` (used by
// `std::env::set_var`), so the C runtime functions are used as in C++
// (`_putenv_s`; setting an empty value removes the variable).
#[cfg(windows)]
unsafe extern "C" {
    fn getenv(name: *const c_char) -> *const c_char;
    fn _putenv_s(name: *const c_char, value: *const c_char) -> c_int;
}

#[cfg(windows)]
struct FreeType2SetEnv {
    old_value: Option<std::ffi::CString>,
}

#[cfg(windows)]
impl FreeType2SetEnv {
    fn new() -> FreeType2SetEnv {
        let _ = FREETYPE_PROPERTIES;
        unsafe {
            let old = getenv(c"FREETYPE_PROPERTIES".as_ptr());
            let old_value = (!old.is_null()).then(|| CStr::from_ptr(old).to_owned());
            if _putenv_s(c"FREETYPE_PROPERTIES".as_ptr(), c"truetype:interpreter-version=35".as_ptr()) != 0 {
                crate::panic_log!(
                    "Requirement '!_putenv_s(\"FREETYPE_PROPERTIES\", \"truetype:interpreter-version=35\")' failed"
                );
            }
            FreeType2SetEnv { old_value }
        }
    }
}

#[cfg(windows)]
impl Drop for FreeType2SetEnv {
    fn drop(&mut self) {
        let value = match &self.old_value {
            Some(v) => v.as_ptr(),
            None => c"".as_ptr(),
        };
        if unsafe { _putenv_s(c"FREETYPE_PROPERTIES".as_ptr(), value) } != 0 {
            crate::panic_log!("Requirement '!_putenv_s(\"FREETYPE_PROPERTIES\", ...)' failed");
        }
    }
}

unsafe extern "C" fn default_substitute(pattern: *mut ffi::FcPattern, _data: *mut c_void) {
    let add = |name: &CStr, value: c_int| {
        let v = ffi::FcValue {
            type_: ffi::FC_TYPE_BOOL,
            u: ffi::FcValueUnion { b: value },
        };
        // SAFETY: pattern is the valid pattern given by PangoFT2.
        unsafe { ffi::FcPatternAdd(pattern, name.as_ptr(), v, ffi::FC_FALSE) };
    };
    add(c"antialias", ffi::FC_FALSE);
    add(c"autohint", ffi::FC_FALSE);
    add(c"hinting", ffi::FC_TRUE);
}

unsafe extern "C" fn noop_destroy(_data: *mut c_void) {}

/// Common text rendering library context for multiple [`TextLayout`]s
/// (`TextRenderContext`). Normally the global one returned by
/// [`global_text_render_context`] is used.
pub struct TextRenderContext {
    font_map: *mut ffi::PangoFontMap,
    pango_ctx: *mut ffi::PangoContext,
    font_desc: *mut ffi::PangoFontDescription,
}

impl TextRenderContext {
    pub fn new() -> Rc<TextRenderContext> {
        let _set_env = FreeType2SetEnv::new();

        // SAFETY: plain calls into Pango with valid arguments; results are
        // checked for NULL.
        unsafe {
            let font_map = ffi::pango_ft2_font_map_new();
            require!(!font_map.is_null());

            ffi::pango_ft2_font_map_set_default_substitute(
                font_map,
                Some(default_substitute),
                std::ptr::null_mut(),
                Some(noop_destroy),
            );
            ffi::pango_ft2_font_map_set_resolution(font_map, 72.0, 72.0);

            let pango_ctx = ffi::pango_font_map_create_context(font_map);
            require!(!pango_ctx.is_null());

            ffi::pango_context_set_base_dir(pango_ctx, ffi::PANGO_DIRECTION_LTR);
            ffi::pango_context_set_language(
                pango_ctx,
                ffi::pango_language_from_string(c"en-US".as_ptr()),
            );

            let font_desc = ffi::pango_font_description_from_string(c"Verdana 11".as_ptr());
            require!(!font_desc.is_null());

            Rc::new(TextRenderContext {
                font_map,
                pango_ctx,
                font_desc,
            })
        }
    }
}

impl Drop for TextRenderContext {
    fn drop(&mut self) {
        // SAFETY: the objects were created in new() and are owned by self.
        unsafe {
            ffi::pango_font_description_free(self.font_desc);
            ffi::g_object_unref(self.pango_ctx as *mut c_void);
            ffi::g_object_unref(self.font_map as *mut c_void);
        }
    }
}

thread_local! {
    static GLOBAL_CONTEXT: RefCell<Option<Rc<TextRenderContext>>> = const { RefCell::new(None) };
}

/// The global text render context of the calling thread
/// (`globals->textRenderContext`), created on first use. `main` creates it
/// at the same point as the C++ version (together with the globals, before
/// CEF is initialized).
pub fn global_text_render_context() -> Rc<TextRenderContext> {
    GLOBAL_CONTEXT.with(|ctx| {
        ctx.borrow_mut()
            .get_or_insert_with(TextRenderContext::new)
            .clone()
    })
}

struct Graymap {
    width: i32,
    height: i32,
    buffer: Vec<u8>,
}

impl Graymap {
    fn new(width: i32, height: i32) -> Graymap {
        require!(width >= 1);
        require!(height >= 1);
        let limit = i32::MAX / 9;
        require!(width < limit / height);
        Graymap {
            width,
            height,
            buffer: vec![0; (width * height) as usize],
        }
    }
}

/// Layout of a single line of text (`TextLayout`).
pub struct TextLayout {
    _ctx: Rc<TextRenderContext>,
    layout: *mut ffi::PangoLayout,
    text: RefCell<String>,
    graymap: RefCell<Option<Graymap>>,
}

impl TextLayout {
    /// Uses the global context [`global_text_render_context`].
    pub fn new() -> TextLayout {
        TextLayout::with_context(global_text_render_context())
    }

    pub fn with_context(ctx: Rc<TextRenderContext>) -> TextLayout {
        // SAFETY: the Pango context is valid while ctx is alive (we keep it).
        let layout = unsafe {
            let layout = ffi::pango_layout_new(ctx.pango_ctx);
            require!(!layout.is_null());
            ffi::pango_layout_set_font_description(layout, ctx.font_desc);
            ffi::pango_layout_set_auto_dir(layout, 0);
            ffi::pango_layout_set_single_paragraph_mode(layout, 1);
            layout
        };
        TextLayout {
            _ctx: ctx,
            layout,
            text: RefCell::new(String::new()),
            graymap: RefCell::new(None),
        }
    }

    /// Sets the text to be laid out.
    pub fn set_text(&self, text: String) {
        *self.graymap.borrow_mut() = None;
        require!(text.len() <= i32::MAX as usize);

        // SAFETY: text is valid UTF-8 of the given length; the layout copies it.
        unsafe {
            ffi::pango_layout_set_text(self.layout, text.as_ptr() as *const c_char, text.len() as c_int);
            // Check that Pango agrees that the text is valid UTF-8 (it
            // replaces invalid input). Like strcmp in the C++ version, only
            // the part before a possible NUL byte is compared.
            let stored = CStr::from_ptr(ffi::pango_layout_get_text(self.layout)).to_bytes();
            let expected = text.as_bytes().split(|&c| c == 0).next().unwrap_or_default();
            require!(stored == expected);
        }
        *self.text.borrow_mut() = text;
    }

    pub fn text(&self) -> String {
        self.text.borrow().clone()
    }

    fn extents(&self) -> ffi::PangoRectangle {
        let mut extents = ffi::PangoRectangle::default();
        // SAFETY: valid layout and output pointer.
        unsafe { ffi::pango_layout_get_pixel_extents(self.layout, std::ptr::null_mut(), &mut extents) };
        extents.width = extents.width.max(1);
        extents.height = extents.height.max(1);
        extents
    }

    /// The logical width of the current text when rendered.
    pub fn width(&self) -> i32 {
        self.extents().width
    }

    /// The logical height of the current text when rendered.
    pub fn height(&self) -> i32 {
        self.extents().height
    }

    /// Byte index of the character boundary closest to the given X coordinate.
    pub fn x_coord_to_index(&self, x: i32) -> i32 {
        let mut idx: c_int = 0;
        let mut trailing: c_int = 0;
        // SAFETY: valid layout; line 0 always exists.
        unsafe {
            let line = ffi::pango_layout_get_line_readonly(self.layout, 0);
            require!(!line.is_null());
            ffi::pango_layout_line_x_to_index(
                line,
                x.wrapping_mul(ffi::PANGO_SCALE),
                &mut idx,
                &mut trailing,
            );
        }
        let text = self.text.borrow();
        require!(idx >= 0);
        let idx = jump_utf8_chars(text.as_bytes(), idx as usize, trailing);
        require!(idx <= text.len());
        idx as i32
    }

    /// X coordinate of a character boundary given as byte index.
    pub fn index_to_x_coord(&self, idx: i32) -> i32 {
        require!(idx >= 0 && idx as usize <= self.text.borrow().len());
        let mut rect = ffi::PangoRectangle::default();
        // SAFETY: valid layout, index in range.
        unsafe { ffi::pango_layout_get_cursor_pos(self.layout, idx, &mut rect, std::ptr::null_mut()) };
        rect.x / ffi::PANGO_SCALE
    }

    /// The previous/next visual character boundary from the given byte index,
    /// clamped to the beginning/end.
    pub fn visual_move_idx(&self, idx: i32, forward: bool) -> i32 {
        let len = self.text.borrow().len();
        require!(idx >= 0 && idx as usize <= len);

        let mut new_idx: c_int = 0;
        let mut trailing: c_int = 0;
        // SAFETY: valid layout, index in range.
        unsafe {
            ffi::pango_layout_move_cursor_visually(
                self.layout,
                1,
                idx,
                0,
                if forward { 1 } else { -1 },
                &mut new_idx,
                &mut trailing,
            );
        }

        if new_idx == -1 {
            0
        } else if new_idx == ffi::G_MAXINT {
            len as i32
        } else {
            require!(new_idx >= 0 && new_idx as usize <= len);
            jump_utf8_chars(self.text.borrow().as_bytes(), new_idx as usize, trailing) as i32
        }
    }

    fn ensure_graymap_rendered(&self) {
        if self.graymap.borrow().is_some() {
            return;
        }
        let extents = self.extents();
        let mut graymap = Graymap::new(extents.width, extents.height);
        let mut bitmap = ffi::FT_Bitmap {
            rows: graymap.height as c_uint,
            width: graymap.width as c_uint,
            pitch: graymap.width,
            buffer: graymap.buffer.as_mut_ptr(),
            num_grays: 256,
            pixel_mode: ffi::FT_PIXEL_MODE_GRAY,
            palette_mode: 0,
            palette: std::ptr::null_mut(),
        };
        // SAFETY: the bitmap describes the graymap buffer, which outlives the call.
        unsafe { ffi::pango_ft2_render_layout(&mut bitmap, self.layout, -extents.x, -extents.y) };
        *self.graymap.borrow_mut() = Some(graymap);
    }

    /// Renders the text with color (r, g, b) to `dest`. The offset (x, y)
    /// moves the text; with (0, 0), the bottom left corners of the logical
    /// text rectangle and `dest` are aligned.
    pub fn render_rgb(&self, dest: &ImageSlice, offset_x: i32, mut offset_y: i32, r: u8, g: u8, b: u8) {
        self.ensure_graymap_rendered();
        let graymap = self.graymap.borrow();
        let graymap = graymap.as_ref().unwrap();

        offset_y += dest.height() - graymap.height;

        let rect = Rect::intersection(
            Rect::new(0, graymap.width, 0, graymap.height),
            Rect::translate(Rect::new(0, dest.width(), 0, dest.height()), -offset_x, -offset_y),
        );
        if rect.is_empty() {
            return;
        }
        for y in rect.start_y..rect.end_y {
            let src_row = &graymap.buffer[(y * graymap.width) as usize..];
            let dest_ptr = dest.pixel_ptr(rect.start_x + offset_x, y + offset_y);
            for x in rect.start_x..rect.end_x {
                if src_row[x as usize] >= 128 {
                    // SAFETY: (x + offset_x, y + offset_y) is inside dest by
                    // the intersection above.
                    unsafe {
                        let p = dest_ptr.add(4 * (x - rect.start_x) as usize);
                        *p = b;
                        *p.add(1) = g;
                        *p.add(2) = r;
                    }
                }
            }
        }
    }

    /// [`TextLayout::render_rgb`] with a gray color.
    pub fn render(&self, dest: &ImageSlice, x: i32, y: i32, rgb: u8) {
        self.render_rgb(dest, x, y, rgb, rgb, rgb);
    }
}

impl Default for TextLayout {
    fn default() -> TextLayout {
        TextLayout::new()
    }
}

impl Drop for TextLayout {
    fn drop(&mut self) {
        // SAFETY: the layout was created in with_context and is owned by self.
        unsafe { ffi::g_object_unref(self.layout as *mut c_void) };
    }
}

/// A [`TextLayout`] clamped to a width, with a horizontal scroll offset
/// (`OverflowTextLayout`).
pub struct OverflowTextLayout {
    text_layout: TextLayout,
    width: Cell<i32>,
    offset: Cell<i32>,
}

impl Default for OverflowTextLayout {
    fn default() -> OverflowTextLayout {
        OverflowTextLayout::new()
    }
}

impl OverflowTextLayout {
    /// Uses the global context [`global_text_render_context`].
    pub fn new() -> OverflowTextLayout {
        OverflowTextLayout::with_context(global_text_render_context())
    }

    pub fn with_context(ctx: Rc<TextRenderContext>) -> OverflowTextLayout {
        OverflowTextLayout {
            text_layout: TextLayout::with_context(ctx),
            width: Cell::new(0),
            offset: Cell::new(0),
        }
    }

    pub fn set_text(&self, text: String) {
        self.text_layout.set_text(text);
        self.clamp_offset();
    }

    pub fn text(&self) -> String {
        self.text_layout.text()
    }

    pub fn text_len(&self) -> i32 {
        self.text_layout.text.borrow().len() as i32
    }

    /// Sets the width to which the text is clamped.
    pub fn set_width(&self, width: i32) {
        require!(width >= 0);
        self.width.set(width);
        self.clamp_offset();
    }

    pub fn width(&self) -> i32 {
        self.width.get()
    }

    /// Logical width of the text without clamping.
    pub fn text_width(&self) -> i32 {
        self.text_layout.width()
    }

    /// Logical height of the text without clamping.
    pub fn text_height(&self) -> i32 {
        self.text_layout.height()
    }

    /// Sets the text offset (clamped to the valid range).
    pub fn set_offset(&self, offset: i32) {
        self.offset.set(offset);
        self.clamp_offset();
    }

    pub fn offset(&self) -> i32 {
        self.offset.get()
    }

    /// Adjusts the offset such that the given character boundary (byte
    /// index) is visible.
    pub fn make_visible(&self, idx: i32) {
        let x = self.text_layout.index_to_x_coord(idx);
        let mut offset = self.offset.get();
        offset = offset.min(x);
        offset = offset.max(x + 1 - self.width.get());
        self.set_offset(offset);
    }

    pub fn x_coord_to_index(&self, x: i32) -> i32 {
        self.text_layout.x_coord_to_index(x + self.offset.get())
    }

    pub fn index_to_x_coord(&self, idx: i32) -> i32 {
        self.text_layout.index_to_x_coord(idx) - self.offset.get()
    }

    pub fn visual_move_idx(&self, idx: i32, forward: bool) -> i32 {
        self.text_layout.visual_move_idx(idx, forward)
    }

    pub fn render_rgb(&self, dest: &ImageSlice, r: u8, g: u8, b: u8) {
        let sub_dest = dest.sub_rect(0, self.width.get(), 0, dest.height());
        self.text_layout.render_rgb(&sub_dest, -self.offset.get(), 0, r, g, b);
    }

    pub fn render(&self, dest: &ImageSlice, rgb: u8) {
        self.render_rgb(dest, rgb, rgb, rgb);
    }

    fn clamp_offset(&self) {
        let mut offset = self.offset.get();
        offset = offset.min(self.text_width() + 1 - self.width.get());
        offset = offset.max(0);
        self.offset.set(offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jump_utf8() {
        let s = "aä€😀b".as_bytes();
        assert_eq!(jump_utf8_chars(s, 0, 1), 1);
        assert_eq!(jump_utf8_chars(s, 1, 1), 3);
        assert_eq!(jump_utf8_chars(s, 3, 1), 6);
        assert_eq!(jump_utf8_chars(s, 6, 1), 10);
        assert_eq!(jump_utf8_chars(s, 0, 5), 11);
        assert_eq!(jump_utf8_chars(s, 11, 0), 11);
    }

    #[test]
    fn layout_metrics_and_rendering() {
        let layout = TextLayout::new();
        layout.set_text(String::new());
        assert_eq!(layout.width(), 1);
        assert!(layout.height() >= 1);

        layout.set_text("Address".to_string());
        let w = layout.width();
        let h = layout.height();
        assert!(w > 20 && w < 100, "width {w}");
        assert!(h > 5 && h < 30, "height {h}");

        assert_eq!(layout.index_to_x_coord(0), 0);
        let end_x = layout.index_to_x_coord(7);
        assert!(end_x > 0 && end_x <= w);
        assert_eq!(layout.x_coord_to_index(-10), 0);
        assert_eq!(layout.x_coord_to_index(1000), 7);
        assert_eq!(layout.visual_move_idx(0, false), 0);
        assert_eq!(layout.visual_move_idx(0, true), 1);
        assert_eq!(layout.visual_move_idx(7, true), 7);

        let image = ImageSlice::create_image(100, 30, 255);
        layout.render(&image, 0, 0, 0);
        let mut black = 0;
        for y in 0..30 {
            for x in 0..100 {
                let p = image.get_pixel(x, y);
                assert!(p == (0, 0, 0) || p == (255, 255, 255));
                if p == (0, 0, 0) {
                    black += 1;
                    assert!(x < w);
                }
            }
        }
        assert!(black > 20);

        layout.set_text("ä€".to_string());
        assert_eq!(layout.visual_move_idx(0, true), 2);
        assert_eq!(layout.visual_move_idx(2, true), 5);
        assert_eq!(layout.visual_move_idx(5, false), 2);
    }

    #[test]
    fn overflow_layout() {
        let layout = OverflowTextLayout::new();
        layout.set_text("http://www.example.com/a/long/path/to/something".to_string());
        layout.set_width(50);
        assert_eq!(layout.offset(), 0);
        layout.make_visible(layout.text_len());
        let max_offset = layout.text_width() + 1 - 50;
        assert_eq!(layout.offset(), max_offset);
        layout.set_offset(1_000_000);
        assert_eq!(layout.offset(), max_offset);
        layout.set_offset(-5);
        assert_eq!(layout.offset(), 0);
    }
}

/// Comparison with the C++ renderer: `TEXT_DUMP_OUT=<file> cargo test --release
/// --features stub-patch text::dump -- --ignored` writes, for each line of
/// `TEXT_DUMP_IN` (or a built-in list), the width, height, cursor positions
/// and the thresholded graymap of the rendered text. The C++ counterpart is
/// `tools/text_dump.cpp` (same output format, built on `src/text.cpp`'s
/// Pango calls).
#[cfg(test)]
mod dump {
    use super::*;
    use std::fmt::Write;

    #[test]
    #[ignore]
    fn dump() {
        let Ok(out) = std::env::var("TEXT_DUMP_OUT") else { return };
        let input = match std::env::var("TEXT_DUMP_IN") {
            Ok(path) => std::fs::read_to_string(path).unwrap(),
            Err(_) => "Address\nQuality\nFind\nDownload\nDownload (1)\n".to_string(),
        };
        let mut s = String::new();
        let layout = TextLayout::new();
        for line in input.lines() {
            layout.set_text(line.to_string());
            let (w, h) = (layout.width(), layout.height());
            writeln!(s, "text {line:?} width {w} height {h}").unwrap();
            let xs: Vec<String> = line
                .char_indices()
                .map(|(i, _)| i)
                .chain([line.len()])
                .map(|i| layout.index_to_x_coord(i as i32).to_string())
                .collect();
            writeln!(s, "cursor {}", xs.join(" ")).unwrap();
            let image = ImageSlice::create_image(w, h, 255);
            layout.render(&image, 0, 0, 0);
            for y in 0..h {
                let row: String = (0..w)
                    .map(|x| if image.get_pixel(x, y) == (0, 0, 0) { '#' } else { '.' })
                    .collect();
                writeln!(s, "{row}").unwrap();
            }
        }
        std::fs::write(out, s).unwrap();
    }
}
