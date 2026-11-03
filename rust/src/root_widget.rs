//! Port of `src/root_widget.{hpp,cpp}` and the state interface of
//! `src/control_bar.hpp`.
//!
//! The control bar widget (address field, buttons, find bar, quality
//! selector, download button; `src/control_bar.cpp` and its child widgets
//! with Pango text rendering) is NOT ported yet. [`ControlBar`] only keeps
//! the state that the window pushes to it, so that the window logic can be
//! ported faithfully; it does not draw anything and takes no space. The root
//! widget therefore always lays out the browser area over the whole window,
//! like the C++ version with `--show-control-bar=false`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::browser_area::{BrowserArea, BrowserAreaEventHandler};
use crate::widget::{Widget, WidgetBase, WidgetExt, WidgetParent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityStatus {
    Secure,
    Warning,
    Insecure,
}

/// Events of the control bar widget (`ControlBarEventHandler`). Nothing
/// raises them until the control bar widget is ported.
pub trait ControlBarEventHandler {
    fn on_address_submitted(&self, url: String);
    fn on_quality_changed(&self, idx: usize);
    fn on_pending_download_accepted(&self);
    fn on_find(&self, text: String, forward: bool, find_next: bool);
    fn on_stop_find(&self, clear_selection: bool);
    fn on_clipboard_button_pressed(&self);
    fn on_open_bookmarks_button_pressed(&self);
    /// -1 = back, 0 = refresh, 1 = forward.
    fn on_navigation_button_pressed(&self, direction: i32);
    fn on_home_button_pressed(&self);
}

/// State holder standing in for the control bar widget (see module docs).
pub struct ControlBar {
    _event_handler: Weak<dyn ControlBarEventHandler>,
    pub quality_selector: RefCell<Option<(Vec<String>, usize)>>,
    pub clipboard_button_enabled: Cell<bool>,
    pub security_status: Cell<SecurityStatus>,
    pub address: RefCell<String>,
    pub page_title: RefCell<String>,
    pub loading: Cell<bool>,
    pub pending_download_count: Cell<i32>,
    pub download_progress: RefCell<Vec<i32>>,
    pub find_result: Cell<Option<bool>>,
}

impl ControlBar {
    /// Height of the C++ control bar in pixels.
    pub const HEIGHT: i32 = 27;

    fn new(event_handler: Weak<dyn ControlBarEventHandler>) -> ControlBar {
        ControlBar {
            _event_handler: event_handler,
            quality_selector: RefCell::new(None),
            clipboard_button_enabled: Cell::new(false),
            security_status: Cell::new(SecurityStatus::Insecure),
            address: RefCell::new(String::new()),
            page_title: RefCell::new(String::new()),
            loading: Cell::new(false),
            pending_download_count: Cell::new(0),
            download_progress: RefCell::new(Vec::new()),
            find_result: Cell::new(None),
        }
    }

    pub fn enable_quality_selector(&self, labels: Vec<String>, choice_idx: usize) {
        *self.quality_selector.borrow_mut() = Some((labels, choice_idx));
    }
    pub fn enable_clipboard_button(&self) {
        self.clipboard_button_enabled.set(true);
    }
    pub fn set_security_status(&self, value: SecurityStatus) {
        self.security_status.set(value);
    }
    pub fn set_address(&self, addr: String) {
        *self.address.borrow_mut() = addr;
    }
    pub fn set_page_title(&self, title: String) {
        *self.page_title.borrow_mut() = title;
    }
    pub fn set_loading(&self, loading: bool) {
        self.loading.set(loading);
    }
    pub fn set_pending_download_count(&self, count: i32) {
        self.pending_download_count.set(count);
    }
    pub fn set_download_progress(&self, progress: Vec<i32>) {
        *self.download_progress.borrow_mut() = progress;
    }
    pub fn set_find_result(&self, found: bool) {
        self.find_result.set(Some(found));
    }
    pub fn activate_address(&self) {}
    pub fn open_find_bar(&self) {}
    pub fn find_next(&self) {}
}

pub struct RootWidget {
    base: WidgetBase,
    control_bar: Rc<ControlBar>,
    browser_area: Rc<BrowserArea>,
}

crate::impl_widget_parent_for_widget!(RootWidget);

impl RootWidget {
    pub fn create(
        widget_parent: Weak<dyn WidgetParent>,
        control_bar_event_handler: Weak<dyn ControlBarEventHandler>,
        browser_area_event_handler: Weak<dyn BrowserAreaEventHandler>,
        _show_soft_navigation_buttons: bool,
    ) -> Rc<RootWidget> {
        Rc::new_cyclic(|weak: &Weak<RootWidget>| {
            let self_parent: Weak<dyn WidgetParent> = weak.clone();
            RootWidget {
                base: WidgetBase::new(widget_parent),
                control_bar: Rc::new(ControlBar::new(control_bar_event_handler)),
                browser_area: Rc::new(BrowserArea::new(self_parent, browser_area_event_handler)),
            }
        })
    }

    pub fn control_bar(&self) -> Rc<ControlBar> {
        self.control_bar.clone()
    }

    pub fn browser_area(&self) -> Rc<BrowserArea> {
        self.browser_area.clone()
    }
}

impl Widget for RootWidget {
    fn base(&self) -> &WidgetBase {
        &self.base
    }

    fn widget_viewport_updated(&self) {
        // The control bar is not ported: the browser area gets the whole
        // viewport (as with --show-control-bar=false in the C++ version).
        self.browser_area.set_viewport(self.viewport());
    }

    fn widget_list_children(&self) -> Vec<Rc<dyn Widget>> {
        vec![self.browser_area.clone() as Rc<dyn Widget>]
    }
}
