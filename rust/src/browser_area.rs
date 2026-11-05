//! Port of `src/browser_area.{hpp,cpp}`: the widget that shows the CEF
//! browser. Its CEF render handler copies the off-screen paint buffer (view
//! and popup widgets such as `<select>` dropdowns) into the widget viewport,
//! which is a part of the window image slice served to the vice plugin, and
//! input events are forwarded to the CEF browser host. In error mode, the
//! view is white with the error message drawn at the top.

use std::cell::{Cell, RefCell};
use std::rc::Weak;

use cef::*;

use crate::image_slice::Rect;
use crate::key::{is_valid_key, key_codes, keys};
use crate::text::TextLayout;
use crate::widget::{Widget, WidgetBase, WidgetExt, WidgetParent};

pub trait BrowserAreaEventHandler {
    fn on_browser_area_view_dirty(&self);
}

pub const EVENTFLAG_SHIFT_DOWN: u32 = 1 << 1;
const EVENTFLAG_CONTROL_DOWN: u32 = 1 << 2;
const EVENTFLAG_ALT_DOWN: u32 = 1 << 3;
const EVENTFLAG_LEFT_MOUSE_BUTTON: u32 = 1 << 4;
const EVENTFLAG_MIDDLE_MOUSE_BUTTON: u32 = 1 << 5;
const EVENTFLAG_RIGHT_MOUSE_BUTTON: u32 = 1 << 6;

fn create_mouse_event(x: i32, y: i32, modifiers: u32) -> MouseEvent {
    MouseEvent { x, y, modifiers }
}

fn mouse_button_info(button: i32) -> (MouseButtonType, u32) {
    match button {
        0 => (MouseButtonType::LEFT, EVENTFLAG_LEFT_MOUSE_BUTTON),
        1 => (MouseButtonType::MIDDLE, EVENTFLAG_MIDDLE_MOUSE_BUTTON),
        _ => (MouseButtonType::RIGHT, EVENTFLAG_RIGHT_MOUSE_BUTTON),
    }
}

/// Builds the CEF key event for a key (`createKeyEvent`). The event type is
/// set by the caller.
pub fn create_key_event(key: i32, event_modifiers: u32) -> KeyEvent {
    if !is_valid_key(key) {
        crate::panic_log!("Requirement 'isValidKey(key)' failed");
    }
    let mut event = KeyEvent {
        windows_key_code: 0,
        native_key_code: 0,
        modifiers: event_modifiers,
        is_system_key: (event_modifiers & EVENTFLAG_ALT_DOWN != 0) as i32,
        unmodified_character: 0,
        ..Default::default()
    };

    if let Some((win, native)) = key_codes(key) {
        event.windows_key_code = win;
        // The native (X11 keycode) value is only used on Linux, as in C++.
        #[cfg(not(windows))]
        {
            event.native_key_code = native;
        }
        #[cfg(windows)]
        let _ = native;
    }

    if key < 0 {
        if key == keys::ENTER {
            event.unmodified_character = '\r' as u16;
        }
        if key == keys::SPACE {
            event.unmodified_character = ' ' as u16;
        }
    } else {
        // Like the C++ version, which assigns the int to a char16_t.
        event.unmodified_character = key as u16;
    }
    event.character = event.unmodified_character;

    // Hack to avoid breaking AltGr keys.
    let is_alnum = (key >= 'a' as i32 && key <= 'z' as i32)
        || (key >= 'A' as i32 && key <= 'Z' as i32)
        || (key >= '0' as i32 && key <= '9' as i32);
    if key > 0
        && (event.modifiers & EVENTFLAG_CONTROL_DOWN != 0)
        && (event.modifiers & EVENTFLAG_ALT_DOWN != 0)
        && !is_alnum
    {
        event.modifiers &= !EVENTFLAG_CONTROL_DOWN;
        event.modifiers &= !EVENTFLAG_ALT_DOWN;
        event.is_system_key = 0;
    }

    event
}

fn key_modifier_flag(key: i32) -> u32 {
    match key {
        keys::SHIFT => EVENTFLAG_SHIFT_DOWN,
        keys::CONTROL => EVENTFLAG_CONTROL_DOWN,
        keys::ALT => EVENTFLAG_ALT_DOWN,
        _ => 0,
    }
}

pub struct BrowserArea {
    base: WidgetBase,
    event_handler: Weak<dyn BrowserAreaEventHandler>,
    browser: RefCell<Option<Browser>>,
    popup_open: Cell<bool>,
    popup_rect: Cell<Rect>,
    event_modifiers: Cell<u32>,
    error_active: Cell<bool>,
    error_layout: TextLayout,
}

crate::impl_widget_parent_for_widget!(BrowserArea);

impl BrowserArea {
    pub fn new(
        widget_parent: Weak<dyn WidgetParent>,
        event_handler: Weak<dyn BrowserAreaEventHandler>,
    ) -> BrowserArea {
        BrowserArea {
            base: WidgetBase::new(widget_parent),
            event_handler,
            browser: RefCell::new(None),
            popup_open: Cell::new(false),
            popup_rect: Cell::new(Rect::default()),
            event_modifiers: Cell::new(0),
            error_active: Cell::new(false),
            error_layout: TextLayout::new(),
        }
    }

    fn host(&self) -> Option<BrowserHost> {
        let browser = self.browser.borrow().clone();
        browser.and_then(|b| b.host())
    }

    /// Sets the browser that is kept up to date about size changes; None
    /// unsets it.
    pub fn set_browser(&self, browser: Option<Browser>) {
        *self.browser.borrow_mut() = browser;
        if let Some(host) = self.host() {
            host.was_resized();
        }
    }

    /// Informs the browser again about focus and mouse-over status (called
    /// when loading a new page).
    pub fn refresh_status_events(&self) {
        let Some(host) = self.host() else { return };
        host.set_focus(self.is_focused_() as i32);
        let (x, y) = self.last_mouse_pos_();
        let event = create_mouse_event(x, y, self.event_modifiers.get());
        host.send_mouse_move_event(Some(&event), (!self.is_mouse_over_()) as i32);
    }

    /// Switches to a mode that only shows an error message until
    /// [`BrowserArea::clear_error`] is called.
    pub fn show_error(&self, message: String) {
        self.error_active.set(true);
        self.error_layout.set_text(message);
        if let Some(host) = self.host() {
            host.invalidate(PaintElementType::VIEW);
            host.invalidate(PaintElementType::POPUP);
        }
    }

    pub fn clear_error(&self) {
        self.error_active.set(false);
        self.error_layout.set_text(String::new());
        if let Some(host) = self.host() {
            host.invalidate(PaintElementType::VIEW);
            host.invalidate(PaintElementType::POPUP);
        }
    }

    pub fn error_message(&self) -> Option<String> {
        if self.error_active.get() {
            Some(self.error_layout.text())
        } else {
            None
        }
    }

    /// Notifies the browser area that the browser has changed the cursor.
    pub fn set_cursor(&self, cursor: i32) {
        self.set_cursor_(cursor);
    }

    // --- CefRenderHandler implementation (called by WindowRenderHandler).

    pub fn view_rect(&self) -> (i32, i32) {
        let viewport = self.viewport();
        let width = viewport.width().clamp(64, 4096);
        let height = viewport.height().clamp(64, 4096);
        (width, height)
    }

    pub fn on_popup_show(&self, browser: &Browser, show: bool) {
        self.popup_open.set(show);
        if show {
            self.popup_rect.set(Rect::default());
        } else if let Some(host) = browser.host() {
            host.invalidate(PaintElementType::VIEW);
        }
    }

    pub fn on_popup_size(&self, browser: &Browser, rect: &cef::Rect) {
        self.popup_rect.set(Rect::new(
            rect.x,
            rect.x + rect.width,
            rect.y,
            rect.y + rect.height,
        ));
        if self.popup_open.get()
            && let Some(host) = browser.host()
        {
            host.invalidate(PaintElementType::VIEW);
            host.invalidate(PaintElementType::POPUP);
        }
    }

    /// Copies the dirty parts of a CEF paint buffer (BGRA, `buf_width` x
    /// `buf_height`) into the viewport.
    pub fn on_paint(
        &self,
        type_: PaintElementType,
        dirty_rects: &[cef::Rect],
        buffer: *const u8,
        buf_width: i32,
        buf_height: i32,
    ) {
        let viewport = self.viewport();
        let mut updated = false;

        if self.error_active.get() {
            viewport.fill(0, viewport.width(), 0, viewport.height(), 255);
            self.error_layout
                .render_rgb(&viewport.split_y(20).0, 7, 0, 96, 0, 0);
            updated = true;
        } else {
            let mut offset_x = 0;
            let mut offset_y = 0;
            let mut bounds = Rect::new(0, viewport.width(), 0, viewport.height());
            let mut cutout = Rect::default();

            if type_ == PaintElementType::VIEW {
                if self.popup_open.get() {
                    cutout = self.popup_rect.get();
                }
            } else if type_ == PaintElementType::POPUP && self.popup_open.get() {
                let popup_rect = self.popup_rect.get();
                offset_x = popup_rect.start_x;
                offset_y = popup_rect.start_y;
                bounds = Rect::intersection(bounds, popup_rect);
                bounds = Rect::translate(bounds, -offset_x, -offset_y);
            } else {
                return;
            }

            // Never read outside the paint buffer, even if CEF painted with a
            // size that differs from the current viewport.
            bounds = Rect::intersection(bounds, Rect::new(0, buf_width, 0, buf_height));

            let mut copy_range = |y: i32, ax: i32, bx: i32| {
                if ax >= bx {
                    return;
                }
                let count = 4 * (bx - ax) as usize;
                unsafe {
                    let src = buffer.add(4 * (y as usize * buf_width as usize + ax as usize));
                    let dest = viewport.pixel_ptr(ax + offset_x, y + offset_y);
                    if updated {
                        std::ptr::copy_nonoverlapping(src, dest, count);
                    } else {
                        let src_slice = std::slice::from_raw_parts(src, count);
                        let dest_slice = std::slice::from_raw_parts(dest as *const u8, count);
                        if src_slice != dest_slice {
                            updated = true;
                            std::ptr::copy_nonoverlapping(src, dest, count);
                        }
                    }
                }
            };

            for dirty_rect in dirty_rects {
                let rect = Rect::intersection(
                    Rect::new(
                        dirty_rect.x,
                        dirty_rect.x + dirty_rect.width,
                        dirty_rect.y,
                        dirty_rect.y + dirty_rect.height,
                    ),
                    bounds,
                );
                if rect.is_empty() {
                    continue;
                }
                for y in rect.start_y..rect.end_y {
                    if y >= cutout.start_y && y < cutout.end_y {
                        copy_range(y, rect.start_x, rect.end_x.min(cutout.start_x));
                        copy_range(y, rect.start_x.max(cutout.end_x), rect.end_x);
                    } else {
                        copy_range(y, rect.start_x, rect.end_x);
                    }
                }
            }
        }

        if updated {
            let handler = self.event_handler.clone();
            crate::ui::post_task(move || {
                if let Some(handler) = handler.upgrade() {
                    handler.on_browser_area_view_dirty();
                }
            });
        }
    }
}

impl Widget for BrowserArea {
    fn base(&self) -> &WidgetBase {
        &self.base
    }

    fn widget_viewport_updated(&self) {
        if let Some(host) = self.host() {
            host.was_resized();
            host.invalidate(PaintElementType::VIEW);
            host.invalidate(PaintElementType::POPUP);
        }
    }

    fn widget_mouse_down_event(&self, x: i32, y: i32, button: i32) {
        let (button_type, button_flag) = mouse_button_info(button);
        if let Some(host) = self.host() {
            let event = create_mouse_event(x, y, self.event_modifiers.get());
            host.send_mouse_click_event(Some(&event), button_type, 0, 1);
        }
        self.event_modifiers.set(self.event_modifiers.get() | button_flag);
    }

    fn widget_mouse_up_event(&self, x: i32, y: i32, button: i32) {
        let (button_type, button_flag) = mouse_button_info(button);
        if let Some(host) = self.host() {
            let event = create_mouse_event(x, y, self.event_modifiers.get());
            host.send_mouse_click_event(Some(&event), button_type, 1, 1);
        }
        self.event_modifiers.set(self.event_modifiers.get() & !button_flag);
    }

    fn widget_mouse_double_click_event(&self, x: i32, y: i32) {
        let Some(host) = self.host() else { return };
        let event = create_mouse_event(x, y, self.event_modifiers.get());
        host.send_mouse_click_event(Some(&event), MouseButtonType::LEFT, 0, 2);
        host.send_mouse_click_event(Some(&event), MouseButtonType::LEFT, 1, 2);
    }

    fn widget_mouse_wheel_event(&self, x: i32, y: i32, delta: i32) {
        let Some(host) = self.host() else { return };
        let event = create_mouse_event(x, y, self.event_modifiers.get());
        host.send_mouse_wheel_event(Some(&event), 0, delta);
    }

    fn widget_mouse_move_event(&self, x: i32, y: i32) {
        let Some(host) = self.host() else { return };
        let event = create_mouse_event(x, y, self.event_modifiers.get());
        host.send_mouse_move_event(Some(&event), 0);
    }

    fn widget_mouse_enter_event(&self, x: i32, y: i32) {
        let Some(host) = self.host() else { return };
        let event = create_mouse_event(x, y, self.event_modifiers.get());
        host.send_mouse_move_event(Some(&event), 0);
    }

    fn widget_mouse_leave_event(&self, x: i32, y: i32) {
        let Some(host) = self.host() else { return };
        let event = create_mouse_event(x, y, self.event_modifiers.get());
        host.send_mouse_move_event(Some(&event), 1);
    }

    fn widget_key_down_event(&self, key: i32) {
        if !is_valid_key(key) {
            crate::panic_log!("Requirement 'isValidKey(key)' failed");
        }
        let browser = self.browser.borrow().clone();
        if let Some(browser) = browser {
            let control_down = self.event_modifiers.get() & EVENTFLAG_CONTROL_DOWN != 0;
            let is = |c: char| key == c as i32 || key == c.to_ascii_uppercase() as i32;
            let (x_key, c_key, v_key) = (is('x'), is('c'), is('v'));
            let frame = if control_down && (x_key || c_key || v_key) {
                browser.focused_frame()
            } else {
                None
            };
            if let Some(frame) = frame {
                if x_key {
                    frame.cut();
                }
                if c_key {
                    frame.copy();
                }
                if v_key {
                    frame.paste();
                }
            } else if let Some(host) = browser.host() {
                let mut event = create_key_event(key, self.event_modifiers.get());
                event.type_ = KeyEventType::RAWKEYDOWN;
                host.send_key_event(Some(&event));
                // On Windows, the CHAR event carries the character as its
                // key code and is only sent for keys with a character.
                #[cfg(windows)]
                if event.character != 0 {
                    let mut char_event = event.clone();
                    char_event.type_ = KeyEventType::CHAR;
                    char_event.windows_key_code = char_event.character as i32;
                    host.send_key_event(Some(&char_event));
                }
                #[cfg(not(windows))]
                {
                    event.type_ = KeyEventType::CHAR;
                    host.send_key_event(Some(&event));
                }
            }
        }
        self.event_modifiers.set(self.event_modifiers.get() | key_modifier_flag(key));
    }

    fn widget_key_up_event(&self, key: i32) {
        if !is_valid_key(key) {
            crate::panic_log!("Requirement 'isValidKey(key)' failed");
        }
        if let Some(host) = self.host() {
            let mut event = create_key_event(key, self.event_modifiers.get());
            event.type_ = KeyEventType::KEYUP;
            host.send_key_event(Some(&event));
        }
        self.event_modifiers.set(self.event_modifiers.get() & !key_modifier_flag(key));
    }

    fn widget_gain_focus_event(&self, _x: i32, _y: i32) {
        if let Some(host) = self.host() {
            host.set_focus(1);
        }
    }

    fn widget_lose_focus_event(&self) {
        if let Some(host) = self.host() {
            host.set_focus(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_events() {
        let e = create_key_event('a' as i32, 0);
        #[cfg(not(windows))]
        assert_eq!((e.windows_key_code, e.native_key_code), (65, 38));
        #[cfg(windows)]
        assert_eq!((e.windows_key_code, e.native_key_code), (65, 0));
        assert_eq!((e.character, e.unmodified_character), (97, 97));
        assert_eq!(e.is_system_key, 0);

        let e = create_key_event(keys::ENTER, EVENTFLAG_ALT_DOWN);
        assert_eq!((e.windows_key_code, e.character), (13, '\r' as u16));
        assert_eq!(e.is_system_key, 1);

        // AltGr hack: Ctrl+Alt with a non-alphanumeric character.
        let e = create_key_event('@' as i32, EVENTFLAG_CONTROL_DOWN | EVENTFLAG_ALT_DOWN);
        assert_eq!(e.modifiers, 0);
        assert_eq!(e.is_system_key, 0);
        let e = create_key_event('q' as i32, EVENTFLAG_CONTROL_DOWN | EVENTFLAG_ALT_DOWN);
        assert_eq!(e.modifiers, EVENTFLAG_CONTROL_DOWN | EVENTFLAG_ALT_DOWN);

        let e = create_key_event(keys::LEFT, EVENTFLAG_SHIFT_DOWN);
        #[cfg(not(windows))]
        assert_eq!((e.windows_key_code, e.native_key_code, e.character), (37, 113, 0));
        #[cfg(windows)]
        assert_eq!((e.windows_key_code, e.native_key_code, e.character), (37, 0, 0));
    }
}
