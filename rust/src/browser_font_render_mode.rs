//! Port of `src/browser_font_render_mode.{hpp,cpp}`.

use crate::chromium_patch::font_render_params as frp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserFontRenderMode {
    NoAntiAliasing,
    AntiAliasing,
    AntiAliasingSubpixelRGB,
    AntiAliasingSubpixelBGR,
    AntiAliasingSubpixelVRGB,
    AntiAliasingSubpixelVBGR,
    System,
}

const HINTING_FULL: i32 = 3;
const HINTING_MEDIUM: i32 = 2;

const SUBPIXEL_RENDERING_NONE: i32 = 0;
const SUBPIXEL_RENDERING_RGB: i32 = 1;
const SUBPIXEL_RENDERING_BGR: i32 = 2;
const SUBPIXEL_RENDERING_VRGB: i32 = 3;
const SUBPIXEL_RENDERING_VBGR: i32 = 4;

/// Applies the font render mode through the patched CEF accessors. Must be
/// called before CEF is initialized.
pub fn init_browser_font_render_mode(mode: BrowserFontRenderMode) {
    use BrowserFontRenderMode::*;

    if mode == System {
        return;
    }

    frp::set_subpixel_positioning_enabled(false);
    frp::set_autohinter_enabled(false);
    frp::set_use_bitmaps_enabled(false);

    if mode == NoAntiAliasing {
        frp::set_antialiasing_enabled(false);
        frp::set_hinting(HINTING_FULL);
        frp::set_subpixel_rendering(SUBPIXEL_RENDERING_NONE);
    } else {
        frp::set_antialiasing_enabled(true);
        frp::set_hinting(HINTING_MEDIUM);
        frp::set_subpixel_rendering(match mode {
            AntiAliasing => SUBPIXEL_RENDERING_NONE,
            AntiAliasingSubpixelRGB => SUBPIXEL_RENDERING_RGB,
            AntiAliasingSubpixelBGR => SUBPIXEL_RENDERING_BGR,
            AntiAliasingSubpixelVRGB => SUBPIXEL_RENDERING_VRGB,
            AntiAliasingSubpixelVBGR => SUBPIXEL_RENDERING_VBGR,
            NoAntiAliasing | System => unreachable!(),
        });
    }
}

pub fn list_browser_font_render_modes() -> [(BrowserFontRenderMode, &'static str); 7] {
    use BrowserFontRenderMode::*;
    [
        (NoAntiAliasing, "no-antialiasing"),
        (AntiAliasing, "antialiasing"),
        (AntiAliasingSubpixelRGB, "antialiasing-subpixel-rgb"),
        (AntiAliasingSubpixelBGR, "antialiasing-subpixel-bgr"),
        (AntiAliasingSubpixelVRGB, "antialiasing-subpixel-vrgb"),
        (AntiAliasingSubpixelVBGR, "antialiasing-subpixel-vbgr"),
        (System, "system"),
    ]
}

pub fn browser_font_render_mode_description() -> &'static str {
    concat!(
        "'no-antialiasing' for unsmoothed text, ",
        "'antialiasing' for smoothed text, ",
        "'antialiasing-subpixel-X' where X is one of 'rgb', 'bgr', 'vrgb' and 'vbgr' for smoothed text with subpixel rendering, ",
        "and 'system' for the Chromium default for this system"
    )
}
