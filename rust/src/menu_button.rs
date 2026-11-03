//! Port of `src/menu_button.{hpp,cpp}`: the icon buttons of the control bar
//! and the find bar (a passive icon normally, an active icon with a frame
//! when the mouse is over the button).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::image_slice::ImageSlice;
use crate::key::keys;
use crate::ui::post_task;
use crate::widget::{Widget, WidgetBase, WidgetExt, WidgetParent};

/// (active icon, passive icon) (`MenuButtonIcon`).
#[derive(Clone)]
pub struct MenuButtonIcon(pub ImageSlice, pub ImageSlice);

/// A color palette for [`ImageSlice::create_image_from_strings`].
pub type Palette = &'static [(char, [u8; 3])];

impl MenuButtonIcon {
    /// Creates the icon pair from one pattern and the active and passive
    /// color palettes.
    pub fn from_pattern(pattern: &[&str], active: Palette, passive: Palette) -> MenuButtonIcon {
        MenuButtonIcon(
            ImageSlice::create_image_from_strings(pattern, active),
            ImageSlice::create_image_from_strings(pattern, passive),
        )
    }
}

pub trait MenuButtonEventHandler {
    fn on_menu_button_pressed(&self, button: Weak<MenuButton>);

    // Event forwarding used by FindBar (as in the C++ version).
    fn on_menu_button_enter_key_down(&self) {}
    fn on_menu_button_esc_key_down(&self) {}
}

pub struct MenuButton {
    base: WidgetBase,
    self_weak: Weak<MenuButton>,
    icon: RefCell<MenuButtonIcon>,
    event_handler: Weak<dyn MenuButtonEventHandler>,
    mouse_over: Cell<bool>,
    mouse_down: Cell<bool>,
}

crate::impl_widget_parent_for_widget!(MenuButton);

impl MenuButton {
    pub fn new(
        icon: MenuButtonIcon,
        widget_parent: Weak<dyn WidgetParent>,
        event_handler: Weak<dyn MenuButtonEventHandler>,
    ) -> Rc<MenuButton> {
        Rc::new_cyclic(|self_weak| MenuButton {
            base: WidgetBase::new(widget_parent),
            self_weak: self_weak.clone(),
            icon: RefCell::new(icon),
            event_handler,
            mouse_over: Cell::new(false),
            mouse_down: Cell::new(false),
        })
    }

    pub fn set_icon(&self, icon: MenuButtonIcon) {
        *self.icon.borrow_mut() = icon;
        self.signal_view_dirty();
    }

    /// True if `button` (from an event) is this button.
    pub fn is(self: &Rc<Self>, button: &Weak<MenuButton>) -> bool {
        button.upgrade().is_some_and(|b| Rc::ptr_eq(&b, self))
    }

    fn mouse_move(&self, x: i32, y: i32) {
        let viewport = self.viewport();
        let new_mouse_over = x >= 0 && x < viewport.width() && y >= 0 && y < viewport.height();
        if new_mouse_over != self.mouse_over.get() {
            self.mouse_over.set(new_mouse_over);
            self.signal_view_dirty();
        }
    }

    fn post_event(&self, func: impl FnOnce(&dyn MenuButtonEventHandler) + 'static) {
        let handler = self.event_handler.clone();
        post_task(move || {
            if let Some(handler) = handler.upgrade() {
                func(&*handler);
            }
        });
    }
}

impl Widget for MenuButton {
    fn base(&self) -> &WidgetBase {
        &self.base
    }

    fn widget_render(&self) {
        let viewport = self.viewport();
        let icon = self.icon.borrow().clone();

        let width = icon.0.width() + 3;
        let height = icon.0.height() + 3;

        // Background
        viewport.fill(0, width, 0, height, 192);

        if self.mouse_over.get() {
            let down = self.mouse_down.get();
            let p = |a: u8, b: u8| if down { a } else { b };

            // Frame
            viewport.fill(0, width - 1, 0, 1, p(128, 255));
            viewport.fill(0, 1, 1, height - 1, p(128, 255));
            viewport.fill(0, width - 1, height - 1, height, p(255, 128));
            viewport.fill(width - 1, width, 0, height, p(255, 128));

            // Icon
            let d = down as i32;
            viewport.put_image(&icon.0, 1 + d, 1 + d);
        } else {
            viewport.put_image(&icon.1, 1, 1);
        }
    }

    fn widget_mouse_down_event(&self, _x: i32, _y: i32, button: i32) {
        if button == 0 {
            self.mouse_down.set(true);
            self.signal_view_dirty();
        }
    }

    fn widget_mouse_up_event(&self, _x: i32, _y: i32, button: i32) {
        if button == 0 {
            if self.mouse_down.get() && self.mouse_over.get() {
                let self_weak = self.self_weak.clone();
                self.post_event(move |h| h.on_menu_button_pressed(self_weak));
            }
            self.mouse_down.set(false);
            self.signal_view_dirty();
        }
    }

    fn widget_mouse_move_event(&self, x: i32, y: i32) {
        self.mouse_move(x, y);
    }

    fn widget_mouse_enter_event(&self, x: i32, y: i32) {
        self.mouse_move(x, y);
    }

    fn widget_mouse_leave_event(&self, _x: i32, _y: i32) {
        if self.mouse_over.get() {
            self.mouse_over.set(false);
            self.signal_view_dirty();
        }
    }

    fn widget_key_down_event(&self, key: i32) {
        if key == keys::ENTER {
            self.post_event(|h| h.on_menu_button_enter_key_down());
        }
        if key == keys::ESC {
            self.post_event(|h| h.on_menu_button_esc_key_down());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::test_util::test_parent;

    struct NoHandler;
    impl MenuButtonEventHandler for NoHandler {
        fn on_menu_button_pressed(&self, _button: Weak<MenuButton>) {}
    }

    #[test]
    fn render_passive_and_active() {
        let (_top, parent) = test_parent();
        let icon = MenuButtonIcon::from_pattern(
            &["ab", "ba"],
            &[('a', [10, 20, 30]), ('b', [40, 50, 60])],
            &[('a', [1, 1, 1]), ('b', [2, 2, 2])],
        );
        let handler: Rc<dyn MenuButtonEventHandler> = Rc::new(NoHandler);
        let button = MenuButton::new(icon, parent, Rc::downgrade(&handler));
        let image = ImageSlice::create_image(8, 8, 7);
        button.set_viewport(image.sub_rect(1, 6, 1, 6));
        button.render();
        // Background 5x5 (icon + 3), passive icon at (1, 1).
        assert_eq!(image.get_pixel(1, 1), (192, 192, 192));
        assert_eq!(image.get_pixel(2, 2), (1, 1, 1));
        assert_eq!(image.get_pixel(3, 2), (2, 2, 2));
        assert_eq!(image.get_pixel(5, 5), (192, 192, 192));
        assert_eq!(image.get_pixel(6, 6), (7, 7, 7));

        button.send_mouse_enter_event(2, 2);
        button.render();
        // Frame: top/left white, bottom/right gray; active icon at (1, 1).
        assert_eq!(image.get_pixel(1, 1), (255, 255, 255));
        assert_eq!(image.get_pixel(5, 1), (128, 128, 128));
        assert_eq!(image.get_pixel(1, 4), (255, 255, 255));
        assert_eq!(image.get_pixel(1, 5), (128, 128, 128));
        assert_eq!(image.get_pixel(2, 5), (128, 128, 128));
        assert_eq!(image.get_pixel(2, 2), (10, 20, 30));
    }
}
