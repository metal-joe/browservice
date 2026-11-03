//! Port of `src/find_bar.{hpp,cpp}`: the find bar of the control bar (text
//! field and next/previous/close buttons).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::image_slice::ImageSlice;
use crate::menu_button::{MenuButton, MenuButtonEventHandler, MenuButtonIcon, Palette};
use crate::text_field::{TextField, TextFieldEventHandler};
use crate::ui::post_task;
use crate::widget::{Widget, WidgetBase, WidgetExt, WidgetParent};

macro_rules! require {
    ($cond:expr) => {
        if !($cond) {
            $crate::panic_log!("Requirement '{}' failed", stringify!($cond));
        }
    };
}

pub trait FindBarEventHandler {
    fn on_find_bar_close(&self);
    fn on_find(&self, text: String, forward: bool, find_next: bool);
    fn on_stop_find(&self, clear_selection: bool);
}

const DOWN_ICON_PATTERN: &[&str] = &[
    "................",
    "................",
    "................",
    "................",
    "......55555.....",
    "......5XX#3.....",
    "......5X#s3.....",
    "......5X#s3.....",
    "......5X#s3.....",
    "......5X#s3.....",
    "...4555X#s3553..",
    "....3XXX###s2...",
    ".....3###zs2....",
    "......3#zs2.....",
    ".......3s2......",
    "........3.......",
    "................",
    "................",
    "................",
];

const UP_ICON_PATTERN: &[&str] = &[
    "................",
    "................",
    "................",
    "................",
    "........4.......",
    ".......5X3......",
    "......5X##3.....",
    ".....5X####3....",
    "....5X###sss3...",
    "...4444##s3333..",
    "......5X#s3.....",
    "......5X#s3.....",
    "......5X#s3.....",
    "......5X#s3.....",
    "......5Xss3.....",
    "......53333.....",
    "................",
    "................",
    "................",
];

const CLOSE_ICON_PATTERN: &[&str] = &[
    "................",
    "................",
    "................",
    "................",
    "................",
    ".....3.....3....",
    "....3X2...3X2...",
    "...3Xx#2.3X#z2..",
    "....2###3X#s1...",
    ".....2####s1....",
    "......3###2.....",
    ".....3X####2....",
    "....3X#s2###2...",
    "...3X#s1.2#zs2..",
    "....2z1...2s1...",
    ".....2.....2....",
    "................",
    "................",
    "................",
];

const ACTIVE_ARROW_COLORS: Palette = &[
    ('.', [192, 192, 192]),
    ('#', [232, 232, 0]),
    ('X', [255, 255, 64]),
    ('5', [102, 102, 0]),
    ('4', [68, 68, 0]),
    ('3', [42, 42, 0]),
    ('2', [0, 0, 0]),
    ('s', [206, 206, 0]),
    ('z', [220, 220, 0]),
];

const PASSIVE_ARROW_COLORS: Palette = &[
    ('.', [192, 192, 192]),
    ('#', [204, 204, 204]),
    ('X', [224, 224, 224]),
    ('5', [90, 90, 90]),
    ('4', [56, 56, 56]),
    ('3', [34, 34, 34]),
    ('2', [0, 0, 0]),
    ('s', [174, 174, 174]),
    ('z', [190, 190, 190]),
];

const CLOSE_ACTIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('#', [255, 128, 128]),
    ('X', [255, 176, 176]),
    ('x', [255, 144, 144]),
    ('1', [0, 0, 0]),
    ('2', [22, 10, 10]),
    ('3', [44, 22, 22]),
    ('s', [192, 96, 96]),
    ('z', [216, 108, 108]),
];

const CLOSE_PASSIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('#', [204, 204, 204]),
    ('X', [224, 224, 224]),
    ('x', [224, 210, 210]),
    ('1', [0, 0, 0]),
    ('2', [20, 20, 20]),
    ('3', [40, 40, 40]),
    ('s', [156, 156, 156]),
    ('z', [174, 174, 174]),
];

const BTN_WIDTH: i32 = 19;

pub struct FindBar {
    base: WidgetBase,
    event_handler: Weak<dyn FindBarEventHandler>,

    text_field: Rc<TextField>,
    down_button: Rc<MenuButton>,
    up_button: Rc<MenuButton>,
    close_button: Rc<MenuButton>,

    is_open: Cell<bool>,
    find_result: Cell<bool>,
    text: RefCell<Option<String>>,
    last_dir_forward: Cell<bool>,
}

crate::impl_widget_parent_for_widget!(FindBar);

impl FindBar {
    pub const WIDTH: i32 = 180;
    pub const HEIGHT: i32 = 22;

    pub fn new(
        widget_parent: Weak<dyn WidgetParent>,
        event_handler: Weak<dyn FindBarEventHandler>,
    ) -> Rc<FindBar> {
        Rc::new_cyclic(|self_weak: &Weak<FindBar>| {
            let parent: Weak<dyn WidgetParent> = self_weak.clone();
            let text_handler: Weak<dyn TextFieldEventHandler> = self_weak.clone();
            let button_handler: Weak<dyn MenuButtonEventHandler> = self_weak.clone();

            let text_field = TextField::new(parent.clone(), text_handler);
            text_field.set_remove_caret_on_submit(false);

            let icon = |pattern, active, passive| MenuButtonIcon::from_pattern(pattern, active, passive);
            let down_icon = icon(DOWN_ICON_PATTERN, ACTIVE_ARROW_COLORS, PASSIVE_ARROW_COLORS);
            let up_icon = icon(UP_ICON_PATTERN, ACTIVE_ARROW_COLORS, PASSIVE_ARROW_COLORS);
            let close_icon = icon(CLOSE_ICON_PATTERN, CLOSE_ACTIVE, CLOSE_PASSIVE);

            FindBar {
                base: WidgetBase::new(widget_parent),
                event_handler,
                text_field,
                down_button: MenuButton::new(down_icon, parent.clone(), button_handler.clone()),
                up_button: MenuButton::new(up_icon, parent.clone(), button_handler.clone()),
                close_button: MenuButton::new(close_icon, parent, button_handler),
                is_open: Cell::new(false),
                find_result: Cell::new(true),
                text: RefCell::new(None),
                last_dir_forward: Cell::new(true),
            }
        })
    }

    fn post_event(&self, func: impl FnOnce(&dyn FindBarEventHandler) + 'static) {
        let handler = self.event_handler.clone();
        post_task(move || {
            if let Some(handler) = handler.upgrade() {
                func(&*handler);
            }
        });
    }

    pub fn open(&self) {
        if !self.is_open.get() {
            self.is_open.set(true);
            self.find_result.set(true);
            *self.text.borrow_mut() = None;
            self.text_field.set_text(String::new());
            self.last_dir_forward.set(true);
        }
    }

    pub fn close(&self) {
        if self.is_open.get() {
            self.is_open.set(false);
            self.post_event(|h| h.on_stop_find(false));
            self.post_event(|h| h.on_find_bar_close());
        }
    }

    pub fn activate(&self) {
        self.text_field.activate();
    }

    pub fn find_next(&self) {
        if self.is_open.get() {
            self.find(self.text_field.text(), self.last_dir_forward.get());
        }
    }

    pub fn set_find_result(&self, found: bool) {
        if self.is_open.get() && self.find_result.get() != found {
            self.find_result.set(found);
            self.signal_view_dirty();
        }
    }

    fn update_text(&self, text: String) -> bool {
        require!(self.is_open.get());

        if text.is_empty() {
            if self.text.borrow().is_some() {
                self.post_event(|h| h.on_stop_find(true));
                *self.text.borrow_mut() = None;
            }
            self.set_find_result(true);
            true
        } else if self.text.borrow().as_deref() == Some(text.as_str()) {
            false
        } else {
            *self.text.borrow_mut() = Some(text.clone());
            self.post_event(move |h| h.on_find(text, true, false));
            true
        }
    }

    fn find(&self, text: String, forward: bool) {
        require!(self.is_open.get());

        self.last_dir_forward.set(forward);

        if !self.update_text(text.clone()) {
            self.post_event(move |h| h.on_find(text, forward, true));
        }
    }
}

impl TextFieldEventHandler for FindBar {
    fn on_text_field_text_changed(&self) {
        if !self.is_open.get() {
            return;
        }
        self.update_text(self.text_field.text());
    }

    fn on_text_field_submitted(&self, text: String) {
        if !self.is_open.get() {
            return;
        }
        self.find(text, self.last_dir_forward.get());
    }

    fn on_text_field_esc_key_down(&self) {
        self.close();
    }
}

impl MenuButtonEventHandler for FindBar {
    fn on_menu_button_pressed(&self, button: Weak<MenuButton>) {
        if self.close_button.is(&button) {
            self.close();
        }

        if self.is_open.get() {
            if self.down_button.is(&button) {
                self.find(self.text_field.text(), true);
            }
            if self.up_button.is(&button) {
                self.find(self.text_field.text(), false);
            }
        }
    }

    fn on_menu_button_enter_key_down(&self) {
        if self.is_open.get() {
            self.find(self.text_field.text(), self.last_dir_forward.get());
        }
    }

    fn on_menu_button_esc_key_down(&self) {
        self.close();
    }
}

impl Widget for FindBar {
    fn base(&self) -> &WidgetBase {
        &self.base
    }

    fn widget_viewport_updated(&self) {
        let viewport = self.viewport();
        let (w, h, b) = (Self::WIDTH, Self::HEIGHT, BTN_WIDTH);

        self.text_field.set_viewport(viewport.sub_rect(4, w - 3 * b - 4, 2, h - 4));
        self.down_button.set_viewport(viewport.sub_rect(w - 3 * b, w - 2 * b, 0, h));
        self.up_button.set_viewport(viewport.sub_rect(w - 2 * b, w - b, 0, h));
        self.close_button.set_viewport(viewport.sub_rect(w - b, w, 0, h));
    }

    fn widget_render(&self) {
        if self.is_open.get() {
            let height = Self::HEIGHT;
            let viewport: ImageSlice = self.viewport().sub_rect(0, Self::WIDTH - 3 * BTN_WIDTH, 0, height);
            let width = viewport.width();

            // Text field border
            viewport.fill(0, width - 1, 0, 1, 128);
            viewport.fill(0, 1, 1, height - 1, 128);
            viewport.fill(0, width, height - 1, height, 255);
            viewport.fill(width - 1, width, 0, height - 1, 255);
            viewport.fill(1, width - 2, 1, 2, 0);
            viewport.fill(1, 2, 2, height - 2, 0);
            viewport.fill(1, width - 1, height - 2, height - 1, 192);
            viewport.fill(width - 2, width - 1, 1, height - 2, 192);

            // Text field background
            if self.find_result.get() {
                viewport.fill(2, width - 2, 2, height - 2, 255);
            } else {
                viewport.fill_rgb(2, width - 2, 2, height - 2, 255, 176, 176);
            }
        }
    }

    fn widget_list_children(&self) -> Vec<Rc<dyn Widget>> {
        vec![
            self.text_field.clone(),
            self.down_button.clone(),
            self.up_button.clone(),
            self.close_button.clone(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::test_util::test_parent;

    struct NoHandler;
    impl FindBarEventHandler for NoHandler {
        fn on_find_bar_close(&self) {}
        fn on_find(&self, _text: String, _forward: bool, _find_next: bool) {}
        fn on_stop_find(&self, _clear_selection: bool) {}
    }

    #[test]
    fn render_open() {
        let (_top, parent) = test_parent();
        let handler: Rc<dyn FindBarEventHandler> = Rc::new(NoHandler);
        let bar = FindBar::new(parent, Rc::downgrade(&handler));
        let image = ImageSlice::create_image(FindBar::WIDTH, FindBar::HEIGHT, 7);
        bar.set_viewport(image.clone());
        bar.render();
        // Closed: only the buttons are drawn (passive icons on gray).
        assert_eq!(image.get_pixel(0, 0), (7, 7, 7));
        assert_eq!(image.get_pixel(FindBar::WIDTH - 1, 0), (192, 192, 192));
        bar.open();
        bar.render();
        assert_eq!(image.get_pixel(0, 0), (128, 128, 128));
        assert_eq!(image.get_pixel(1, 1), (0, 0, 0));
        assert_eq!(image.get_pixel(5, 5), (255, 255, 255));
        assert_eq!(image.get_pixel(122, 21), (255, 255, 255));
        // Arrow and close icons (passive palettes) at x = 123 + 1, 142 + 1, 161 + 1.
        assert_eq!(image.get_pixel(124 + 6, 1 + 5), (90, 90, 90));
        assert_eq!(image.get_pixel(162 + 4, 1 + 6), (40, 40, 40));
    }
}
