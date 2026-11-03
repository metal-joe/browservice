//! Port of `src/widget.{hpp,cpp}`: the widget tree base, routing input events
//! to the focused / mouse-over widget and tracking cursors and view dirtiness.
//!
//! A widget type implements [`Widget`] (the `widget*_` hooks of the C++
//! class); the public API of the C++ `Widget` base class is provided for every
//! widget by the blanket trait [`WidgetExt`]. Widgets are used through
//! `Rc<...>` on the CEF UI thread and use interior mutability; no `RefCell`
//! borrow is held across calls into other widgets or event handlers.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::{Rc, Weak};

use crate::image_slice::ImageSlice;
use crate::key::{is_valid_key, keys};

pub const HAND_CURSOR: i32 = 0;
pub const NORMAL_CURSOR: i32 = 1;
pub const TEXT_CURSOR: i32 = 2;
pub const CURSOR_TYPE_COUNT: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalHotkey {
    Address,
    Find,
    FindNext,
    Refresh,
    ZoomReset,
    ZoomIn,
    ZoomOut,
}

/// Identity of a widget for `on_widget_take_focus` (address of the object).
pub type WidgetId = *const ();

pub trait WidgetParent {
    // Called directly (not through a posted task), like in the C++ version;
    // implementations must take care of re-entrancy.
    fn on_widget_view_dirty(&self);
    fn on_widget_cursor_changed(&self);
    fn on_widget_take_focus(&self, _child: WidgetId) {}
    fn on_global_hotkey_pressed(&self, key: GlobalHotkey);
}

/// State of the C++ `Widget` base class.
pub struct WidgetBase {
    parent: Weak<dyn WidgetParent>,
    viewport: RefCell<ImageSlice>,
    view_dirty: Cell<bool>,
    focus_child: RefCell<Option<Rc<dyn Widget>>>,
    mouse_over_child: RefCell<Option<Rc<dyn Widget>>>,
    mouse_over: Cell<bool>,
    focused: Cell<bool>,
    last_mouse_x: Cell<i32>,
    last_mouse_y: Cell<i32>,
    mouse_buttons_down: RefCell<BTreeSet<i32>>,
    keys_down: RefCell<BTreeSet<i32>>,
    cursor: Cell<i32>,
    my_cursor: Cell<i32>,
}

impl WidgetBase {
    pub fn new(parent: Weak<dyn WidgetParent>) -> WidgetBase {
        WidgetBase {
            parent,
            viewport: RefCell::new(ImageSlice::default()),
            view_dirty: Cell::new(false),
            focus_child: RefCell::new(None),
            mouse_over_child: RefCell::new(None),
            mouse_over: Cell::new(false),
            focused: Cell::new(false),
            last_mouse_x: Cell::new(-1),
            last_mouse_y: Cell::new(-1),
            mouse_buttons_down: RefCell::new(BTreeSet::new()),
            keys_down: RefCell::new(BTreeSet::new()),
            cursor: Cell::new(NORMAL_CURSOR),
            my_cursor: Cell::new(NORMAL_CURSOR),
        }
    }

    fn parent(&self) -> Option<Rc<dyn WidgetParent>> {
        self.parent.upgrade()
    }
}

/// Hooks implemented by widget types (the virtual `widget*_` functions).
/// Mouse coordinates passed to the event hooks are local to the viewport.
pub trait Widget: 'static {
    fn base(&self) -> &WidgetBase;

    fn widget_viewport_updated(&self) {}
    fn widget_render(&self) {}
    fn widget_list_children(&self) -> Vec<Rc<dyn Widget>> {
        Vec::new()
    }

    fn widget_mouse_down_event(&self, _x: i32, _y: i32, _button: i32) {}
    fn widget_mouse_up_event(&self, _x: i32, _y: i32, _button: i32) {}
    fn widget_mouse_double_click_event(&self, _x: i32, _y: i32) {}
    fn widget_mouse_wheel_event(&self, _x: i32, _y: i32, _delta: i32) {}
    fn widget_mouse_move_event(&self, _x: i32, _y: i32) {}
    fn widget_mouse_enter_event(&self, _x: i32, _y: i32) {}
    fn widget_mouse_leave_event(&self, _x: i32, _y: i32) {}
    fn widget_key_down_event(&self, _key: i32) {}
    fn widget_key_up_event(&self, _key: i32) {}
    fn widget_gain_focus_event(&self, _x: i32, _y: i32) {}
    fn widget_lose_focus_event(&self) {}
}

fn same_widget(a: &Option<Rc<dyn Widget>>, b: &Option<Rc<dyn Widget>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b)),
        _ => false,
    }
}

/// Public API and event routing of the C++ `Widget` class.
pub trait WidgetExt: Widget {
    fn widget_id(&self) -> WidgetId {
        self as *const Self as *const ()
    }

    fn set_viewport(&self, viewport: ImageSlice) {
        *self.base().viewport.borrow_mut() = viewport;
        self.widget_viewport_updated();
        self.signal_view_dirty();
    }

    fn viewport(&self) -> ImageSlice {
        self.base().viewport.borrow().clone()
    }

    fn render(&self) {
        self.base().view_dirty.set(false);
        self.widget_render();
        for child in self.widget_list_children() {
            child.render();
        }
    }

    fn cursor(&self) -> i32 {
        self.base().cursor.get()
    }

    /// Makes this widget the focused widget in the widget tree.
    fn take_focus(&self) {
        let b = self.base();
        if b.focused.get() && b.focus_child.borrow().is_none() {
            return;
        }
        if b.focused.get() {
            self.clear_event_state(b.last_mouse_x.get(), b.last_mouse_y.get());
            self.forward_lose_focus_event();
        } else if let Some(parent) = b.parent() {
            parent.on_widget_take_focus(self.widget_id());
        }
        *b.focus_child.borrow_mut() = None;
        b.focused.set(true);
        let vp = self.viewport();
        self.widget_gain_focus_event(vp.width() / 2, vp.height() / 2);
    }

    fn send_mouse_down_event(&self, x: i32, y: i32, button: i32) {
        let b = self.base();
        b.last_mouse_x.set(x);
        b.last_mouse_y.set(y);
        if b.mouse_buttons_down.borrow().contains(&button) {
            return;
        }
        self.update_focus(x, y);
        b.mouse_buttons_down.borrow_mut().insert(button);
        self.forward_mouse_down_event(x, y, button);
    }

    fn send_mouse_up_event(&self, x: i32, y: i32, button: i32) {
        let b = self.base();
        b.last_mouse_x.set(x);
        b.last_mouse_y.set(y);
        if !b.mouse_buttons_down.borrow_mut().remove(&button) {
            return;
        }
        self.forward_mouse_up_event(x, y, button);
        self.update_mouse_over(x, y);
    }

    fn send_mouse_double_click_event(&self, x: i32, y: i32) {
        let b = self.base();
        b.last_mouse_x.set(x);
        b.last_mouse_y.set(y);
        self.forward_mouse_double_click_event(x, y);
    }

    fn send_mouse_wheel_event(&self, x: i32, y: i32, delta: i32) {
        let b = self.base();
        b.last_mouse_x.set(x);
        b.last_mouse_y.set(y);
        self.update_mouse_over(x, y);
        self.forward_mouse_wheel_event(x, y, delta);
    }

    fn send_mouse_move_event(&self, x: i32, y: i32) {
        let b = self.base();
        if !b.mouse_over.get() && x == b.last_mouse_x.get() && y == b.last_mouse_y.get() {
            return;
        }
        b.last_mouse_x.set(x);
        b.last_mouse_y.set(y);
        self.update_mouse_over(x, y);
        self.forward_mouse_move_event(x, y);
    }

    fn send_mouse_enter_event(&self, x: i32, y: i32) {
        let b = self.base();
        b.last_mouse_x.set(x);
        b.last_mouse_y.set(y);
        self.update_mouse_over(x, y);
    }

    fn send_mouse_leave_event(&self, x: i32, y: i32) {
        let b = self.base();
        b.last_mouse_x.set(x);
        b.last_mouse_y.set(y);
        if b.mouse_buttons_down.borrow().is_empty() && b.mouse_over.get() {
            self.forward_mouse_leave_event(x, y);
            *b.mouse_over_child.borrow_mut() = None;
            b.mouse_over.set(false);
            self.update_cursor();
        }
    }

    fn send_key_down_event(&self, key: i32) {
        if !is_valid_key(key) {
            crate::panic_log!("Requirement 'isValidKey(key)' failed");
        }
        let b = self.base();
        let control = b.keys_down.borrow().contains(&keys::CONTROL);
        let is = |c: u8| key == c as i32 || key == c.to_ascii_uppercase() as i32;
        if control && is(b'f') {
            self.on_hotkey(GlobalHotkey::Find);
        } else if control && is(b'l') {
            self.on_hotkey(GlobalHotkey::Address);
        } else if key == keys::F3 {
            self.on_hotkey(GlobalHotkey::FindNext);
        } else if key == keys::F5 {
            self.on_hotkey(GlobalHotkey::Refresh);
        } else if control && is(b'r') {
            self.on_hotkey(GlobalHotkey::Refresh);
        } else if control && is(b'm') {
            self.on_hotkey(GlobalHotkey::ZoomReset);
        } else if control && is(b'k') {
            self.on_hotkey(GlobalHotkey::ZoomIn);
        } else if control && is(b'j') {
            self.on_hotkey(GlobalHotkey::ZoomOut);
        } else {
            b.keys_down.borrow_mut().insert(key);
            self.forward_key_down_event(key);
        }
    }

    fn send_key_up_event(&self, key: i32) {
        if !is_valid_key(key) {
            crate::panic_log!("Requirement 'isValidKey(key)' failed");
        }
        if !self.base().keys_down.borrow_mut().remove(&key) {
            return;
        }
        self.forward_key_up_event(key);
    }

    fn send_gain_focus_event(&self, x: i32, y: i32) {
        let b = self.base();
        b.last_mouse_x.set(x);
        b.last_mouse_y.set(y);
        self.update_focus(x, y);
    }

    fn send_lose_focus_event(&self) {
        let b = self.base();
        if b.focused.get() {
            self.clear_event_state(b.last_mouse_x.get(), b.last_mouse_y.get());
            self.forward_lose_focus_event();
            *b.focus_child.borrow_mut() = None;
            b.focused.set(false);
        }
    }

    // --- WidgetParent behavior of the base class (forwarding from children).

    fn widget_parent_view_dirty(&self) {
        self.signal_view_dirty();
    }

    fn widget_parent_cursor_changed(&self) {
        self.update_cursor();
    }

    fn widget_parent_take_focus(&self, child: WidgetId) {
        let child = self
            .widget_list_children()
            .into_iter()
            .find(|c| std::ptr::addr_eq(Rc::as_ptr(c), child));
        if let Some(child) = child {
            let b = self.base();
            if b.focused.get() {
                self.clear_event_state(b.last_mouse_x.get(), b.last_mouse_y.get());
                self.forward_lose_focus_event();
            } else if let Some(parent) = b.parent() {
                parent.on_widget_take_focus(self.widget_id());
            }
            *b.focus_child.borrow_mut() = Some(child);
            b.focused.set(true);
        }
    }

    fn on_hotkey(&self, key: GlobalHotkey) {
        if let Some(parent) = self.base().parent() {
            parent.on_global_hotkey_pressed(key);
        }
    }

    // --- Protected helpers.

    fn signal_view_dirty(&self) {
        let b = self.base();
        if !b.view_dirty.get() {
            b.view_dirty.set(true);
            if let Some(parent) = b.parent() {
                parent.on_widget_view_dirty();
            }
        }
    }

    fn set_cursor_(&self, new_cursor: i32) {
        if !(0..CURSOR_TYPE_COUNT).contains(&new_cursor) {
            crate::panic_log!(
                "Requirement 'newCursor >= 0 && newCursor < CursorTypeCount' failed"
            );
        }
        self.base().my_cursor.set(new_cursor);
        self.update_cursor();
    }

    fn is_mouse_over_(&self) -> bool {
        self.base().mouse_over.get()
    }

    fn is_focused_(&self) -> bool {
        self.base().focused.get()
    }

    fn last_mouse_pos_(&self) -> (i32, i32) {
        let b = self.base();
        (b.last_mouse_x.get(), b.last_mouse_y.get())
    }

    // --- Private helpers.

    fn update_focus(&self, x: i32, y: i32) {
        let b = self.base();
        let new_focus_child = self.child_by_point(x, y);
        let focus_child = b.focus_child.borrow().clone();
        if !same_widget(&new_focus_child, &focus_child) || !b.focused.get() {
            self.clear_event_state(x, y);
            self.update_mouse_over(x, y);
            if b.focused.get() {
                self.forward_lose_focus_event();
            }
            *b.focus_child.borrow_mut() = new_focus_child;
            b.focused.set(true);
            self.forward_gain_focus_event(x, y);
        } else {
            self.update_mouse_over(x, y);
        }
    }

    fn update_mouse_over(&self, x: i32, y: i32) {
        let b = self.base();
        if !b.mouse_buttons_down.borrow().is_empty() {
            return;
        }
        let new_child = self.child_by_point(x, y);
        let cur_child = b.mouse_over_child.borrow().clone();
        if !same_widget(&new_child, &cur_child) || !b.mouse_over.get() {
            if b.mouse_over.get() {
                self.forward_mouse_leave_event(x, y);
            }
            *b.mouse_over_child.borrow_mut() = new_child;
            b.mouse_over.set(true);
            self.forward_mouse_enter_event(x, y);
            self.update_cursor();
        }
    }

    fn clear_event_state(&self, x: i32, y: i32) {
        let b = self.base();
        loop {
            let button = b.mouse_buttons_down.borrow_mut().pop_first();
            let Some(button) = button else { break };
            self.forward_mouse_up_event(x, y, button);
        }
        loop {
            let key = b.keys_down.borrow_mut().pop_first();
            let Some(key) = key else { break };
            self.forward_key_up_event(key);
        }
    }

    fn child_by_point(&self, x: i32, y: i32) -> Option<Rc<dyn Widget>> {
        self.widget_list_children()
            .into_iter()
            .find(|child| child.base().viewport.borrow().contains_global_point(x, y))
    }

    fn update_cursor(&self) {
        let b = self.base();
        let mouse_over_child = b.mouse_over_child.borrow().clone();
        let new_cursor = match mouse_over_child {
            Some(child) => child.cursor(),
            None => b.my_cursor.get(),
        };
        if new_cursor != b.cursor.get() {
            b.cursor.set(new_cursor);
            if let Some(parent) = b.parent() {
                parent.on_widget_cursor_changed();
            }
        }
    }

    fn to_local(&self, x: i32, y: i32) -> (i32, i32) {
        let vp = self.base().viewport.borrow();
        (x - vp.global_x(), y - vp.global_y())
    }

    fn focus_child(&self) -> Option<Rc<dyn Widget>> {
        self.base().focus_child.borrow().clone()
    }

    fn mouse_over_child(&self) -> Option<Rc<dyn Widget>> {
        self.base().mouse_over_child.borrow().clone()
    }

    fn forward_mouse_down_event(&self, x: i32, y: i32, button: i32) {
        match self.focus_child() {
            Some(c) => c.send_mouse_down_event(x, y, button),
            None => {
                let (x, y) = self.to_local(x, y);
                self.widget_mouse_down_event(x, y, button)
            }
        }
    }

    fn forward_mouse_up_event(&self, x: i32, y: i32, button: i32) {
        match self.focus_child() {
            Some(c) => c.send_mouse_up_event(x, y, button),
            None => {
                let (x, y) = self.to_local(x, y);
                self.widget_mouse_up_event(x, y, button)
            }
        }
    }

    fn forward_mouse_double_click_event(&self, x: i32, y: i32) {
        match self.focus_child() {
            Some(c) => c.send_mouse_double_click_event(x, y),
            None => {
                let (x, y) = self.to_local(x, y);
                self.widget_mouse_double_click_event(x, y)
            }
        }
    }

    fn forward_mouse_wheel_event(&self, x: i32, y: i32, delta: i32) {
        match self.mouse_over_child() {
            Some(c) => c.send_mouse_wheel_event(x, y, delta),
            None => {
                let (x, y) = self.to_local(x, y);
                self.widget_mouse_wheel_event(x, y, delta)
            }
        }
    }

    fn forward_mouse_move_event(&self, x: i32, y: i32) {
        match self.mouse_over_child() {
            Some(c) => c.send_mouse_move_event(x, y),
            None => {
                let (x, y) = self.to_local(x, y);
                self.widget_mouse_move_event(x, y)
            }
        }
    }

    fn forward_mouse_enter_event(&self, x: i32, y: i32) {
        match self.mouse_over_child() {
            Some(c) => c.send_mouse_enter_event(x, y),
            None => {
                let (x, y) = self.to_local(x, y);
                self.widget_mouse_enter_event(x, y)
            }
        }
    }

    fn forward_mouse_leave_event(&self, x: i32, y: i32) {
        match self.mouse_over_child() {
            Some(c) => c.send_mouse_leave_event(x, y),
            None => {
                let (x, y) = self.to_local(x, y);
                self.widget_mouse_leave_event(x, y)
            }
        }
    }

    fn forward_gain_focus_event(&self, x: i32, y: i32) {
        match self.focus_child() {
            Some(c) => c.send_gain_focus_event(x, y),
            None => {
                let (x, y) = self.to_local(x, y);
                self.widget_gain_focus_event(x, y)
            }
        }
    }

    fn forward_key_down_event(&self, key: i32) {
        match self.focus_child() {
            Some(c) => c.send_key_down_event(key),
            None => self.widget_key_down_event(key),
        }
    }

    fn forward_key_up_event(&self, key: i32) {
        match self.focus_child() {
            Some(c) => c.send_key_up_event(key),
            None => self.widget_key_up_event(key),
        }
    }

    fn forward_lose_focus_event(&self) {
        match self.focus_child() {
            Some(c) => c.send_lose_focus_event(),
            None => self.widget_lose_focus_event(),
        }
    }
}

impl<T: Widget + ?Sized> WidgetExt for T {}

/// Implements [`WidgetParent`] for a widget type with the forwarding
/// behavior of the C++ `Widget` base class.
#[macro_export]
macro_rules! impl_widget_parent_for_widget {
    ($ty:ty) => {
        impl $crate::widget::WidgetParent for $ty {
            fn on_widget_view_dirty(&self) {
                $crate::widget::WidgetExt::widget_parent_view_dirty(self)
            }
            fn on_widget_cursor_changed(&self) {
                $crate::widget::WidgetExt::widget_parent_cursor_changed(self)
            }
            fn on_widget_take_focus(&self, child: $crate::widget::WidgetId) {
                $crate::widget::WidgetExt::widget_parent_take_focus(self, child)
            }
            fn on_global_hotkey_pressed(&self, key: $crate::widget::GlobalHotkey) {
                $crate::widget::WidgetExt::on_hotkey(self, key)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Log(RefCell<Vec<String>>);

    struct Leaf {
        base: WidgetBase,
        name: &'static str,
        log: Rc<Log>,
    }

    impl Widget for Leaf {
        fn base(&self) -> &WidgetBase {
            &self.base
        }
        fn widget_mouse_down_event(&self, x: i32, y: i32, button: i32) {
            self.log.0.borrow_mut().push(format!("{} down {x} {y} {button}", self.name));
        }
        fn widget_mouse_up_event(&self, x: i32, y: i32, button: i32) {
            self.log.0.borrow_mut().push(format!("{} up {x} {y} {button}", self.name));
        }
        fn widget_key_down_event(&self, key: i32) {
            self.log.0.borrow_mut().push(format!("{} key {key}", self.name));
        }
        fn widget_key_up_event(&self, key: i32) {
            self.log.0.borrow_mut().push(format!("{} keyup {key}", self.name));
        }
        fn widget_gain_focus_event(&self, _x: i32, _y: i32) {
            self.log.0.borrow_mut().push(format!("{} focus", self.name));
        }
        fn widget_lose_focus_event(&self) {
            self.log.0.borrow_mut().push(format!("{} blur", self.name));
        }
        fn widget_mouse_enter_event(&self, _x: i32, _y: i32) {
            self.log.0.borrow_mut().push(format!("{} enter", self.name));
        }
        fn widget_mouse_leave_event(&self, _x: i32, _y: i32) {
            self.log.0.borrow_mut().push(format!("{} leave", self.name));
        }
    }
    impl_widget_parent_for_widget!(Leaf);

    struct Root {
        base: WidgetBase,
        children: RefCell<Vec<Rc<Leaf>>>,
    }
    impl Widget for Root {
        fn base(&self) -> &WidgetBase {
            &self.base
        }
        fn widget_viewport_updated(&self) {
            let (a, b) = self.viewport().split_y(10);
            let children = self.children.borrow().clone();
            children[0].set_viewport(a);
            children[1].set_viewport(b);
        }
        fn widget_list_children(&self) -> Vec<Rc<dyn Widget>> {
            self.children
                .borrow()
                .iter()
                .map(|c| c.clone() as Rc<dyn Widget>)
                .collect()
        }
    }
    impl_widget_parent_for_widget!(Root);

    struct TopParent {
        hotkeys: RefCell<Vec<GlobalHotkey>>,
        dirty: Cell<u32>,
    }
    impl WidgetParent for TopParent {
        fn on_widget_view_dirty(&self) {
            self.dirty.set(self.dirty.get() + 1);
        }
        fn on_widget_cursor_changed(&self) {}
        fn on_global_hotkey_pressed(&self, key: GlobalHotkey) {
            self.hotkeys.borrow_mut().push(key);
        }
    }

    #[test]
    fn routing() {
        let log = Rc::new(Log::default());
        let top = Rc::new(TopParent {
            hotkeys: RefCell::new(Vec::new()),
            dirty: Cell::new(0),
        });
        let top_dyn: Rc<dyn WidgetParent> = top.clone();
        let root = Rc::new_cyclic(|weak: &Weak<Root>| {
            let parent: Weak<dyn WidgetParent> = weak.clone();
            Root {
                base: WidgetBase::new(Rc::downgrade(&top_dyn)),
                children: RefCell::new(vec![
                    Rc::new(Leaf {
                        base: WidgetBase::new(parent.clone()),
                        name: "a",
                        log: log.clone(),
                    }),
                    Rc::new(Leaf {
                        base: WidgetBase::new(parent),
                        name: "b",
                        log: log.clone(),
                    }),
                ]),
            }
        });
        root.set_viewport(ImageSlice::create_image(20, 30, 255));
        assert_eq!(top.dirty.get(), 1);

        root.send_mouse_down_event(5, 15, 0);
        root.send_mouse_up_event(6, 16, 0);
        root.send_key_down_event('x' as i32);
        root.send_key_down_event(keys::CONTROL);
        root.send_key_down_event('f' as i32);
        root.send_mouse_down_event(1, 2, 0);
        root.send_lose_focus_event();

        assert_eq!(*top.hotkeys.borrow(), vec![GlobalHotkey::Find]);
        assert_eq!(
            *log.0.borrow(),
            vec![
                "b enter",
                "b focus",
                "b down 5 5 0",
                "b up 6 6 0",
                "b key 120",
                "b key -17",
                "b keyup -17",
                "b keyup 120",
                "b leave",
                "a enter",
                "b blur",
                "a focus",
                "a down 1 2 0",
                "a up 1 2 0",
                "a blur",
            ]
        );
    }
}

/// Helpers for widget unit tests.
#[cfg(test)]
pub mod test_util {
    use super::*;

    /// A top-level parent that counts dirty notifications.
    #[derive(Default)]
    pub struct TestParent {
        pub dirty: Cell<u32>,
        pub hotkeys: RefCell<Vec<GlobalHotkey>>,
    }

    impl WidgetParent for TestParent {
        fn on_widget_view_dirty(&self) {
            self.dirty.set(self.dirty.get() + 1);
        }
        fn on_widget_cursor_changed(&self) {}
        fn on_global_hotkey_pressed(&self, key: GlobalHotkey) {
            self.hotkeys.borrow_mut().push(key);
        }
    }

    pub fn test_parent() -> (Rc<TestParent>, Weak<dyn WidgetParent>) {
        let parent = Rc::new(TestParent::default());
        let dyn_parent: Rc<dyn WidgetParent> = parent.clone();
        let weak = Rc::downgrade(&dyn_parent);
        (parent, weak)
    }

    /// Text rendering of an image region: '#' for pixels equal to `rgb`.
    pub fn ascii(image: &ImageSlice, rgb: (u8, u8, u8)) -> Vec<String> {
        (0..image.height())
            .map(|y| {
                (0..image.width())
                    .map(|x| if image.get_pixel(x, y) == rgb { '#' } else { '.' })
                    .collect()
            })
            .collect()
    }
}
