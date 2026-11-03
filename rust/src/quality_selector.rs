//! Port of `src/quality_selector.{hpp,cpp}`: the image quality spin box of
//! the control bar (a text field with up/down buttons, keys and mouse wheel).

use std::cell::Cell;
use std::rc::{Rc, Weak};

use crate::key::keys;
use crate::text_field::{TextField, TextFieldEventHandler};
use crate::timeout::Timeout;
use crate::ui::post_task;
use crate::widget::{Widget, WidgetBase, WidgetExt, WidgetParent};

macro_rules! require {
    ($cond:expr) => {
        if !($cond) {
            $crate::panic_log!("Requirement '{}' failed", stringify!($cond));
        }
    };
}

pub trait QualitySelectorEventHandler {
    fn on_quality_changed(&self, idx: usize);
}

pub struct QualitySelector {
    base: WidgetBase,
    self_weak: Weak<QualitySelector>,
    event_handler: Weak<dyn QualitySelectorEventHandler>,

    text_field: Rc<TextField>,

    long_mouse_repeat_timeout: Rc<Timeout>,
    short_mouse_repeat_timeout: Rc<Timeout>,

    labels: Vec<String>,
    choice_idx: Cell<usize>,

    has_focus: Cell<bool>,
    up_key_pressed: Cell<bool>,
    down_key_pressed: Cell<bool>,
    up_button_pressed: Cell<bool>,
    down_button_pressed: Cell<bool>,
}

crate::impl_widget_parent_for_widget!(QualitySelector);

impl QualitySelector {
    pub const WIDTH: i32 = 48;
    pub const HEIGHT: i32 = 22;

    pub fn new(
        widget_parent: Weak<dyn WidgetParent>,
        event_handler: Weak<dyn QualitySelectorEventHandler>,
        labels: Vec<String>,
        choice_idx: usize,
    ) -> Rc<QualitySelector> {
        require!(!labels.is_empty());
        require!(choice_idx < labels.len());

        let selector = Rc::new_cyclic(|self_weak: &Weak<QualitySelector>| {
            let parent: Weak<dyn WidgetParent> = self_weak.clone();
            let text_handler: Weak<dyn TextFieldEventHandler> = self_weak.clone();
            QualitySelector {
                base: WidgetBase::new(widget_parent),
                self_weak: self_weak.clone(),
                event_handler,
                text_field: TextField::new(parent, text_handler),
                long_mouse_repeat_timeout: Timeout::create(500),
                short_mouse_repeat_timeout: Timeout::create(50),
                labels,
                choice_idx: Cell::new(choice_idx),
                has_focus: Cell::new(false),
                up_key_pressed: Cell::new(false),
                down_key_pressed: Cell::new(false),
                up_button_pressed: Cell::new(false),
                down_button_pressed: Cell::new(false),
            }
        });
        selector.update_text_field();
        selector
    }

    fn set_quality_str(&self, quality_str: &str) {
        if let Some(i) = self.labels.iter().position(|l| l == quality_str) {
            self.set_quality(i);
            return;
        }
        let reduce = |s: &str| -> String {
            s.bytes()
                .filter(|&c| c != b' ')
                .map(|c| c.to_ascii_lowercase() as char)
                .collect()
        };
        let quality_str = reduce(quality_str);
        if let Some(i) = self.labels.iter().position(|l| reduce(l) == quality_str) {
            self.set_quality(i);
            return;
        }
        self.update_text_field();
    }

    fn set_quality(&self, choice_idx: usize) {
        require!(choice_idx < self.labels.len());

        if choice_idx != self.choice_idx.get() {
            self.choice_idx.set(choice_idx);
            let handler = self.event_handler.clone();
            post_task(move || {
                if let Some(handler) = handler.upgrade() {
                    handler.on_quality_changed(choice_idx);
                }
            });
            self.signal_view_dirty();
        }

        self.update_text_field();
    }

    fn change_quality(&self, d: i32) {
        let choice_idx = self.choice_idx.get();
        let len = self.labels.len();
        if d > 0 {
            let ds = d as usize;
            if len - choice_idx <= ds {
                self.set_quality(len - 1);
            } else {
                self.set_quality(choice_idx + ds);
            }
        }
        if d < 0 {
            let ds = (-d) as usize;
            self.set_quality(if ds > choice_idx { 0 } else { choice_idx - ds });
        }
    }

    fn update_text_field(&self) {
        let choice_idx = self.choice_idx.get();
        require!(choice_idx < self.labels.len());
        self.text_field.set_text(self.labels[choice_idx].clone());
    }

    fn mouse_repeat(&self, direction: i32, first: bool) {
        self.change_quality(direction);

        let self_weak = self.self_weak.clone();
        let timeout = if first {
            &self.long_mouse_repeat_timeout
        } else {
            &self.short_mouse_repeat_timeout
        };
        timeout.set(move || {
            if let Some(this) = self_weak.upgrade() {
                this.mouse_repeat(direction, false);
            }
        });
    }
}

impl TextFieldEventHandler for QualitySelector {
    fn on_text_field_submitted(&self, text: String) {
        self.set_quality_str(&text);
    }

    fn on_text_field_lost_focus_after_edit(&self) {
        self.set_quality_str(&self.text_field.text());
    }

    fn on_text_field_ud_key_down(&self, down: bool) {
        self.widget_key_down_event(if down { keys::DOWN } else { keys::UP });
    }

    fn on_text_field_ud_key_up(&self, down: bool) {
        self.widget_key_up_event(if down { keys::DOWN } else { keys::UP });
    }

    fn on_text_field_wheel_event(&self, delta: i32) {
        if delta != 0 && (self.has_focus.get() || self.text_field.has_focus()) {
            self.change_quality(if delta > 0 { 1 } else { -1 });
        }
    }
}

impl Widget for QualitySelector {
    fn base(&self) -> &WidgetBase {
        &self.base
    }

    fn widget_viewport_updated(&self) {
        let viewport = self.viewport();
        self.text_field
            .set_viewport(viewport.sub_rect(4, Self::WIDTH - 19, 2, Self::HEIGHT - 4));
    }

    fn widget_render(&self) {
        let viewport = self.viewport();
        let (w, h) = (Self::WIDTH, Self::HEIGHT);

        // Frame
        viewport.fill(0, w - 1, 0, 1, 128);
        viewport.fill(0, 1, 1, h - 1, 128);
        viewport.fill(0, w, h - 1, h, 255);
        viewport.fill(w - 1, w, 0, h - 1, 255);
        viewport.fill(1, w - 2, 1, 2, 0);
        viewport.fill(1, 2, 2, h - 2, 0);
        viewport.fill(1, w - 1, h - 2, h - 1, 192);
        viewport.fill(w - 2, w - 1, 1, h - 2, 192);

        // Text field background
        viewport.fill(2, w - 17, 2, h - 2, 255);

        // Buttons
        let draw_button = |start_y: i32, up: bool, pressed: bool, enabled: bool| {
            let pressed = enabled && pressed;
            let p = |a: u8, b: u8| if pressed { a } else { b };

            let start_x = w - 17;
            let end_x = w - 2;
            let end_y = start_y + 9;

            viewport.fill(start_x, end_x - 1, start_y, start_y + 1, p(128, 192));
            viewport.fill(start_x, start_x + 1, start_y + 1, end_y - 1, p(128, 192));
            viewport.fill(start_x, end_x, end_y - 1, end_y, p(255, 0));
            viewport.fill(end_x - 1, end_x, start_y, end_y - 1, p(255, 0));
            viewport.fill(start_x + 1, end_x - 2, start_y + 1, start_y + 2, p(0, 255));
            viewport.fill(start_x + 1, start_x + 2, start_y + 2, end_y - 2, p(0, 255));
            viewport.fill(start_x + 1, end_x - 1, end_y - 2, end_y - 1, p(192, 128));
            viewport.fill(end_x - 2, end_x - 1, start_y + 1, end_y - 2, p(192, 128));

            viewport.fill(start_x + 2, end_x - 2, start_y + 2, end_y - 2, 192);

            let arrow_x = (start_x + end_x) / 2 + pressed as i32;
            let arrow_y = start_y + 4 + pressed as i32;
            let dy = if up { -1 } else { 1 };
            if !enabled {
                viewport.fill(arrow_x + 1, arrow_x + 2, arrow_y + 1 + dy, arrow_y + 2 + dy, 255);
                viewport.fill(arrow_x, arrow_x + 3, arrow_y + 1, arrow_y + 2, 255);
                viewport.fill(arrow_x - 1, arrow_x + 4, arrow_y + 1 - dy, arrow_y + 2 - dy, 255);
            }
            let c = if enabled { 0 } else { 128 };
            viewport.fill(arrow_x, arrow_x + 1, arrow_y + dy, arrow_y + 1 + dy, c);
            viewport.fill(arrow_x - 1, arrow_x + 2, arrow_y, arrow_y + 1, c);
            viewport.fill(arrow_x - 2, arrow_x + 3, arrow_y - dy, arrow_y + 1 - dy, c);
        };
        let choice_idx = self.choice_idx.get();
        draw_button(
            2,
            true,
            self.up_key_pressed.get() || self.up_button_pressed.get(),
            choice_idx < self.labels.len() - 1,
        );
        draw_button(
            11,
            false,
            self.down_key_pressed.get() || self.down_button_pressed.get(),
            choice_idx > 0,
        );
    }

    fn widget_list_children(&self) -> Vec<Rc<dyn Widget>> {
        vec![self.text_field.clone()]
    }

    fn widget_mouse_down_event(&self, x: i32, y: i32, button: i32) {
        let w = Self::WIDTH;
        if button == 0 && x >= w - 17 && x <= w - 2 && (2..20).contains(&y) {
            self.long_mouse_repeat_timeout.clear(false);
            self.short_mouse_repeat_timeout.clear(false);
            self.down_button_pressed.set(false);
            self.up_button_pressed.set(false);

            let direction = if y < 11 {
                self.up_button_pressed.set(true);
                1
            } else {
                self.down_button_pressed.set(true);
                -1
            };
            self.mouse_repeat(direction, true);
            self.signal_view_dirty();
        }
    }

    fn widget_mouse_up_event(&self, _x: i32, _y: i32, button: i32) {
        if button == 0 {
            self.long_mouse_repeat_timeout.clear(false);
            self.short_mouse_repeat_timeout.clear(false);
            self.down_button_pressed.set(false);
            self.up_button_pressed.set(false);
            self.signal_view_dirty();
        }
    }

    fn widget_mouse_wheel_event(&self, _x: i32, _y: i32, delta: i32) {
        if delta != 0 && (self.has_focus.get() || self.text_field.has_focus()) {
            self.change_quality(if delta > 0 { 1 } else { -1 });
        }
    }

    fn widget_key_down_event(&self, key: i32) {
        if key == keys::DOWN || key == keys::UP {
            self.down_key_pressed.set(key == keys::DOWN);
            self.up_key_pressed.set(key == keys::UP);

            self.change_quality(if key == keys::DOWN { -1 } else { 1 });
            self.signal_view_dirty();
        }
    }

    fn widget_key_up_event(&self, key: i32) {
        if key == keys::DOWN || key == keys::UP {
            self.down_key_pressed.set(false);
            self.up_key_pressed.set(false);
            self.signal_view_dirty();
        }
    }

    fn widget_gain_focus_event(&self, _x: i32, _y: i32) {
        self.has_focus.set(true);
    }

    fn widget_lose_focus_event(&self) {
        self.has_focus.set(false);
    }
}
