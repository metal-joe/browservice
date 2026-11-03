//! Port of `src/button.{hpp,cpp}`: a push button with a text label (used for
//! the download button of the control bar).

use std::cell::Cell;
use std::rc::{Rc, Weak};

use crate::text::TextLayout;
use crate::ui::post_task;
use crate::widget::{Widget, WidgetBase, WidgetExt, WidgetParent};

pub trait ButtonEventHandler {
    fn on_button_pressed(&self);
}

pub struct Button {
    base: WidgetBase,
    event_handler: Weak<dyn ButtonEventHandler>,
    enabled: Cell<bool>,
    mouse_down: Cell<bool>,
    pressed: Cell<bool>,
    text_layout: TextLayout,
}

crate::impl_widget_parent_for_widget!(Button);

impl Button {
    pub fn new(
        widget_parent: Weak<dyn WidgetParent>,
        event_handler: Weak<dyn ButtonEventHandler>,
    ) -> Rc<Button> {
        Rc::new(Button {
            base: WidgetBase::new(widget_parent),
            event_handler,
            enabled: Cell::new(false),
            mouse_down: Cell::new(false),
            pressed: Cell::new(false),
            text_layout: TextLayout::new(),
        })
    }

    pub fn set_enabled(&self, enabled: bool) {
        if enabled != self.enabled.get() {
            self.enabled.set(enabled);
            self.mouse_down.set(false);
            self.pressed.set(false);
            self.signal_view_dirty();
        }
    }

    pub fn set_text(&self, text: String) {
        self.text_layout.set_text(text);
        self.signal_view_dirty();
    }
}

impl Widget for Button {
    fn base(&self) -> &WidgetBase {
        &self.base
    }

    fn widget_render(&self) {
        let viewport = self.viewport();
        let width = viewport.width();
        let height = viewport.height();
        let pressed = self.enabled.get() && self.pressed.get();
        let p = |a: u8, b: u8| if pressed { a } else { b };

        // Frame
        viewport.fill(0, width - 1, 0, 1, p(128, 255));
        viewport.fill(0, 1, 1, height - 1, p(128, 255));
        viewport.fill(0, width, height - 1, height, p(255, 0));
        viewport.fill(width - 1, width, 0, height - 1, p(255, 0));
        viewport.fill(1, width - 2, 1, 2, p(0, 192));
        viewport.fill(1, 2, 2, height - 2, p(0, 192));
        viewport.fill(1, width - 1, height - 2, height - 1, p(192, 128));
        viewport.fill(width - 2, width - 1, 1, height - 2, p(192, 128));

        // Background
        viewport.fill(2, width - 2, 2, height - 2, 192);

        // Text
        let mut offset_x = (width - self.text_layout.width()) / 2;
        let mut offset_y = 7 - (height + 1) / 2;

        if self.enabled.get() {
            if self.pressed.get() {
                offset_x += 1;
                offset_y += 1;
            }
            self.text_layout.render(&viewport, offset_x, offset_y, 0);
        } else {
            self.text_layout.render(&viewport, offset_x + 1, offset_y + 1, 255);
            self.text_layout.render(&viewport, offset_x, offset_y, 128);
        }
    }

    fn widget_mouse_down_event(&self, _x: i32, _y: i32, _button: i32) {
        self.mouse_down.set(true);
        self.pressed.set(true);
        self.signal_view_dirty();
    }

    fn widget_mouse_up_event(&self, x: i32, y: i32, button: i32) {
        if button == 0 {
            self.widget_mouse_move_event(x, y);

            if self.pressed.get() && self.enabled.get() {
                let handler = self.event_handler.clone();
                post_task(move || {
                    if let Some(handler) = handler.upgrade() {
                        handler.on_button_pressed();
                    }
                });
            }

            self.mouse_down.set(false);
            self.pressed.set(false);
            self.signal_view_dirty();
        }
    }

    fn widget_mouse_move_event(&self, x: i32, y: i32) {
        if self.mouse_down.get() {
            let viewport = self.viewport();
            let inside = x >= 0 && y >= 0 && x < viewport.width() && y < viewport.height();
            if inside != self.pressed.get() {
                self.pressed.set(inside);
                self.signal_view_dirty();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_slice::ImageSlice;
    use crate::widget::test_util::test_parent;

    struct NoHandler;
    impl ButtonEventHandler for NoHandler {
        fn on_button_pressed(&self) {}
    }

    #[test]
    fn render_disabled_and_enabled() {
        let (_top, parent) = test_parent();
        let handler: Rc<dyn ButtonEventHandler> = Rc::new(NoHandler);
        let button = Button::new(parent, Rc::downgrade(&handler));
        button.set_text("Download".to_string());
        let image = ImageSlice::create_image(88, 22, 0);
        button.set_viewport(image.clone());
        button.render();
        assert_eq!(image.get_pixel(0, 0), (255, 255, 255));
        assert_eq!(image.get_pixel(87, 0), (0, 0, 0));
        assert_eq!(image.get_pixel(1, 1), (192, 192, 192));
        assert_eq!(image.get_pixel(86, 20), (128, 128, 128));
        let count = |c| {
            (0..22)
                .flat_map(|y| (0..88).map(move |x| (x, y)))
                .filter(|&(x, y)| image.get_pixel(x, y) == c)
                .count()
        };
        // Disabled: gray text with a white shadow, no black text.
        let gray_disabled = count((128, 128, 128));
        assert!(gray_disabled > 100 + 20);
        button.set_enabled(true);
        button.render();
        assert!(count((128, 128, 128)) < gray_disabled);
        let black_inside = (2..20)
            .flat_map(|y| (2..86).map(move |x| (x, y)))
            .filter(|&(x, y)| image.get_pixel(x, y) == (0, 0, 0))
            .count();
        assert!(black_inside > 20);
    }
}
