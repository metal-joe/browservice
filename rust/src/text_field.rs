//! Port of `src/text_field.{hpp,cpp}`: a single line text field with a
//! blinking caret, selection, clipboard shortcuts and horizontal scrolling
//! (used for the address field, the find bar and the quality selector).

use std::cell::Cell;
use std::rc::{Rc, Weak};

use crate::clipboard::{copy_to_clipboard, paste_from_clipboard};
use crate::key::{is_valid_key, key_to_utf8, keys};
use crate::text::OverflowTextLayout;
use crate::timeout::Timeout;
use crate::ui::post_task;
use crate::widget::{TEXT_CURSOR, Widget, WidgetBase, WidgetExt, WidgetParent};

macro_rules! require {
    ($cond:expr) => {
        if !($cond) {
            $crate::panic_log!("Requirement '{}' failed", stringify!($cond));
        }
    };
}

pub trait TextFieldEventHandler {
    fn on_text_field_submitted(&self, _text: String) {}
    fn on_text_field_lost_focus_after_edit(&self) {}
    fn on_text_field_text_changed(&self) {}

    // Event forwarding used by QualitySelector (as in the C++ version).
    fn on_text_field_ud_key_down(&self, _down: bool) {}
    fn on_text_field_esc_key_down(&self) {}
    fn on_text_field_ud_key_up(&self, _down: bool) {}
    fn on_text_field_wheel_event(&self, _delta: i32) {}
}

pub struct TextField {
    base: WidgetBase,
    self_weak: Weak<TextField>,
    event_handler: Weak<dyn TextFieldEventHandler>,
    text_layout: OverflowTextLayout,

    remove_caret_on_submit: Cell<bool>,
    allow_empty_submit: Cell<bool>,

    has_focus: Cell<bool>,
    left_mouse_button_down: Cell<bool>,
    shift_key_down: Cell<bool>,
    control_key_down: Cell<bool>,

    caret_active: Cell<bool>,
    caret_start: Cell<i32>,
    caret_end: Cell<i32>,
    caret_blink_state: Cell<bool>,

    caret_blink_timeout: Rc<Timeout>,
}

crate::impl_widget_parent_for_widget!(TextField);

impl TextField {
    pub fn new(
        widget_parent: Weak<dyn WidgetParent>,
        event_handler: Weak<dyn TextFieldEventHandler>,
    ) -> Rc<TextField> {
        let field = Rc::new_cyclic(|self_weak| TextField {
            base: WidgetBase::new(widget_parent),
            self_weak: self_weak.clone(),
            event_handler,
            text_layout: OverflowTextLayout::new(),
            remove_caret_on_submit: Cell::new(true),
            allow_empty_submit: Cell::new(true),
            has_focus: Cell::new(false),
            left_mouse_button_down: Cell::new(false),
            shift_key_down: Cell::new(false),
            control_key_down: Cell::new(false),
            caret_active: Cell::new(false),
            caret_start: Cell::new(0),
            caret_end: Cell::new(0),
            caret_blink_state: Cell::new(false),
            caret_blink_timeout: Timeout::create(500),
        });
        field.set_cursor_(TEXT_CURSOR);
        field
    }

    pub fn set_text(&self, text: String) {
        self.unset_caret();
        self.text_layout.set_text(text);
        self.text_layout.set_offset(0);
        self.signal_view_dirty();
    }

    pub fn text(&self) -> String {
        self.text_layout.text()
    }

    /// Takes the focus and selects all text.
    pub fn activate(&self) {
        self.take_focus();
        self.set_caret(0, self.text_layout.text_len());
    }

    pub fn has_focus(&self) -> bool {
        self.has_focus.get()
    }

    pub fn set_remove_caret_on_submit(&self, value: bool) {
        self.remove_caret_on_submit.set(value);
    }

    pub fn set_allow_empty_submit(&self, value: bool) {
        self.allow_empty_submit.set(value);
    }

    fn post_event(&self, func: impl FnOnce(&dyn TextFieldEventHandler) + 'static) {
        let handler = self.event_handler.clone();
        post_task(move || {
            if let Some(handler) = handler.upgrade() {
                func(&*handler);
            }
        });
    }

    fn unset_caret(&self) {
        if self.caret_active.get() {
            self.caret_active.set(false);
            self.caret_blink_timeout.clear(false);
            self.signal_view_dirty();
        }
    }

    fn set_caret(&self, start: i32, end: i32) {
        let len = self.text_layout.text_len();
        require!(start >= 0 && start <= len);
        require!(end >= 0 && end <= len);

        if !self.caret_active.get() || self.caret_start.get() != start || self.caret_end.get() != end {
            self.caret_active.set(true);
            self.caret_start.set(start);
            self.caret_end.set(end);
            self.caret_blink_state.set(true);

            self.text_layout.make_visible(end);

            self.schedule_blink_caret();

            self.signal_view_dirty();
        }
    }

    fn schedule_blink_caret(&self) {
        self.caret_blink_timeout.clear(false);

        let self_weak = self.self_weak.clone();
        self.caret_blink_timeout.set(move || {
            if let Some(this) = self_weak.upgrade()
                && this.caret_active.get()
            {
                this.caret_blink_state.set(!this.caret_blink_state.get());
                this.signal_view_dirty();
                this.schedule_blink_caret();
            }
        });
    }

    fn caret_range(&self) -> (usize, usize) {
        let (a, b) = (self.caret_start.get(), self.caret_end.get());
        (a.min(b) as usize, a.max(b) as usize)
    }

    fn type_text(&self, text: &[u8]) {
        if self.caret_active.get() {
            let (idx1, idx2) = self.caret_range();
            self.unset_caret();

            let old_text = self.text_layout.text();
            require!(idx2 <= old_text.len());

            let mut new_text = Vec::with_capacity(old_text.len() + text.len());
            new_text.extend_from_slice(&old_text.as_bytes()[..idx1]);
            new_text.extend_from_slice(text);
            new_text.extend_from_slice(&old_text.as_bytes()[idx2..]);
            // The C++ version passes the bytes to Pango and requires that
            // they are valid UTF-8; the caret indices are always character
            // boundaries and the inserted text is valid UTF-8.
            let Ok(new_text) = String::from_utf8(new_text) else {
                crate::panic_log!("Requirement 'valid UTF-8' failed");
            };
            self.text_layout.set_text(new_text);

            let idx = (idx1 + text.len()) as i32;
            self.set_caret(idx, idx);

            self.post_event(|h| h.on_text_field_text_changed());
        }
    }

    fn type_character(&self, key: i32) {
        if self.caret_active.get() {
            self.type_text(&key_to_utf8(key));
        }
    }

    fn erase_range(&self) {
        if self.caret_active.get() {
            let (idx1, idx2) = self.caret_range();
            self.unset_caret();

            let old_text = self.text_layout.text();
            require!(idx2 <= old_text.len());
            let new_text = format!("{}{}", &old_text[..idx1], &old_text[idx2..]);
            self.text_layout.set_text(new_text);

            self.set_caret(idx1 as i32, idx1 as i32);

            self.post_event(|h| h.on_text_field_text_changed());
        }
    }

    fn paste_from_clipboard(&self) {
        if self.caret_active.get() {
            let self_weak = self.self_weak.clone();
            let text = paste_from_clipboard();
            post_task(move || {
                if let Some(this) = self_weak.upgrade() {
                    this.type_text(text.as_bytes());
                }
            });
        }
    }

    fn copy_to_clipboard(&self) {
        if self.caret_active.get() {
            let (idx1, idx2) = self.caret_range();
            if idx1 < idx2 {
                let text = self.text_layout.text();
                require!(idx2 <= text.len());
                copy_to_clipboard(&text[idx1..idx2]);
            }
        }
    }
}

impl Widget for TextField {
    fn base(&self) -> &WidgetBase {
        &self.base
    }

    fn widget_viewport_updated(&self) {
        self.text_layout.set_width(self.viewport().width());
    }

    fn widget_render(&self) {
        let viewport = self.viewport();

        self.text_layout.render(&viewport, 0);

        let caret_start_y = viewport.height() - 14;
        let caret_end_y = viewport.height();

        if self.caret_active.get() {
            let start_x = self.text_layout.index_to_x_coord(self.caret_start.get());
            let end_x = self.text_layout.index_to_x_coord(self.caret_end.get());

            let fill_slice = if start_x < end_x {
                viewport.sub_rect(start_x, end_x, caret_start_y, caret_end_y)
            } else if start_x > end_x {
                viewport.sub_rect(end_x + 1, start_x, caret_start_y, caret_end_y)
            } else {
                Default::default()
            };

            // Inverts the selection: black text becomes white, everything
            // else the selection color. As in the C++ version, the bytes are
            // written in buffer order (B, G, R), so the color is (0, 0, 128).
            for y in 0..fill_slice.height() {
                let ptr = fill_slice.pixel_ptr(0, y);
                for x in 0..fill_slice.width() {
                    // SAFETY: (x, y) is inside fill_slice.
                    unsafe {
                        let p = ptr.add(4 * x as usize);
                        if *p == 0 && *p.add(1) == 0 && *p.add(2) == 0 {
                            *p = 255;
                            *p.add(1) = 255;
                            *p.add(2) = 255;
                        } else {
                            *p = 128;
                            *p.add(1) = 0;
                            *p.add(2) = 0;
                        }
                    }
                }
            }

            if self.caret_blink_state.get() {
                viewport.fill(end_x, end_x + 1, caret_start_y, caret_end_y, 0);
            }
        }
    }

    fn widget_mouse_down_event(&self, x: i32, _y: i32, button: i32) {
        if button == 0 {
            if self.caret_active.get() {
                self.left_mouse_button_down.set(true);
                let idx = self.text_layout.x_coord_to_index(x);
                if self.shift_key_down.get() {
                    self.set_caret(self.caret_start.get(), idx);
                } else {
                    self.set_caret(idx, idx);
                }
            } else {
                self.set_caret(0, self.text_layout.text_len());
            }
        }
    }

    fn widget_mouse_up_event(&self, _x: i32, _y: i32, button: i32) {
        if button == 0 {
            self.left_mouse_button_down.set(false);
        }
    }

    fn widget_mouse_double_click_event(&self, _x: i32, _y: i32) {
        self.set_caret(0, self.text_layout.text_len());
    }

    fn widget_mouse_wheel_event(&self, _x: i32, _y: i32, delta: i32) {
        self.post_event(move |h| h.on_text_field_wheel_event(delta));
    }

    fn widget_mouse_move_event(&self, x: i32, _y: i32) {
        if self.left_mouse_button_down.get() && self.caret_active.get() {
            let idx = self.text_layout.x_coord_to_index(x);
            self.set_caret(self.caret_start.get(), idx);
        }
    }

    fn widget_key_down_event(&self, key: i32) {
        require!(is_valid_key(key));

        if key == keys::SHIFT {
            self.shift_key_down.set(true);
        }
        if key == keys::CONTROL {
            self.control_key_down.set(true);
        }

        if key == keys::DOWN || key == keys::UP {
            let down = key == keys::DOWN;
            self.post_event(move |h| h.on_text_field_ud_key_down(down));
        }

        if key == keys::ESC {
            self.post_event(|h| h.on_text_field_esc_key_down());
        }

        if (key == keys::LEFT || key == keys::RIGHT) && self.caret_active.get() {
            let idx = self.text_layout.visual_move_idx(self.caret_end.get(), key == keys::RIGHT);
            let start = if self.shift_key_down.get() { self.caret_start.get() } else { idx };
            self.set_caret(start, idx);
        }

        if (key == keys::HOME || key == keys::END) && self.caret_active.get() {
            let idx = if key == keys::HOME { 0 } else { self.text_layout.text_len() };
            let start = if self.shift_key_down.get() { self.caret_start.get() } else { idx };
            self.set_caret(start, idx);
        }

        if key > 0 {
            let control = self.control_key_down.get();
            let is = |c: u8| key == c as i32 || key == c.to_ascii_uppercase() as i32;
            if control && is(b'c') {
                self.copy_to_clipboard();
            } else if control && is(b'x') {
                self.copy_to_clipboard();
                self.erase_range();
            } else if control && is(b'v') {
                self.paste_from_clipboard();
            } else if control && is(b'a') {
                self.set_caret(0, self.text_layout.text_len());
            } else {
                self.type_character(key);
            }
        }
        if key == keys::SPACE {
            self.type_character(b' ' as i32);
        }

        if (key == keys::BACKSPACE || key == keys::DELETE) && self.caret_active.get() {
            if self.caret_start.get() == self.caret_end.get() {
                self.caret_end.set(
                    self.text_layout.visual_move_idx(self.caret_end.get(), key == keys::DELETE),
                );
            }
            self.erase_range();
        }

        if key == keys::ENTER && self.caret_active.get() {
            let text = self.text_layout.text();
            if !text.is_empty() || self.allow_empty_submit.get() {
                if self.remove_caret_on_submit.get() {
                    self.unset_caret();
                }
                self.post_event(move |h| h.on_text_field_submitted(text));
            }
        }
    }

    fn widget_key_up_event(&self, key: i32) {
        require!(is_valid_key(key));

        if key == keys::DOWN || key == keys::UP {
            let down = key == keys::DOWN;
            self.post_event(move |h| h.on_text_field_ud_key_up(down));
        }

        if key == keys::SHIFT {
            self.shift_key_down.set(false);
        }
        if key == keys::CONTROL {
            self.control_key_down.set(false);
        }
    }

    fn widget_gain_focus_event(&self, _x: i32, _y: i32) {
        self.has_focus.set(true);
    }

    fn widget_lose_focus_event(&self) {
        self.has_focus.set(false);

        if self.caret_active.get() {
            self.unset_caret();
            self.post_event(|h| h.on_text_field_lost_focus_after_edit());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_slice::ImageSlice;
    use crate::widget::test_util::test_parent;

    struct NoHandler;
    impl TextFieldEventHandler for NoHandler {}

    #[test]
    fn render_text_without_caret() {
        let (_top, parent) = test_parent();
        let handler: Rc<dyn TextFieldEventHandler> = Rc::new(NoHandler);
        let field = TextField::new(parent, Rc::downgrade(&handler));
        assert_eq!(field.cursor(), TEXT_CURSOR);
        let image = ImageSlice::create_image(60, 16, 255);
        field.set_viewport(image.sub_rect(0, 30, 0, 16));
        field.set_text("http://www.example.com/".to_string());
        field.render();
        let black = |x0: i32, x1: i32| {
            (0..16)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .filter(|&(x, y)| image.get_pixel(x, y) == (0, 0, 0))
                .count()
        };
        // The text is clipped to the 30 pixel wide viewport.
        assert!(black(0, 30) > 10);
        assert_eq!(black(30, 60), 0);
        assert_eq!(field.text(), "http://www.example.com/");
    }
}
