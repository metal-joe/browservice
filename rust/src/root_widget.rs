//! Port of `src/root_widget.{hpp,cpp}`: the root of the widget tree of a
//! window, with the control bar at the top (unless `--show-control-bar=false`)
//! and the browser area below it.

use std::rc::{Rc, Weak};

use crate::browser_area::{BrowserArea, BrowserAreaEventHandler};
use crate::control_bar::{ControlBar, ControlBarEventHandler};
use crate::globals::globals;
use crate::widget::{Widget, WidgetBase, WidgetExt, WidgetParent};

pub struct RootWidget {
    base: WidgetBase,
    show_control_bar: bool,
    control_bar: Rc<ControlBar>,
    browser_area: Rc<BrowserArea>,
}

crate::impl_widget_parent_for_widget!(RootWidget);

impl RootWidget {
    pub fn create(
        widget_parent: Weak<dyn WidgetParent>,
        control_bar_event_handler: Weak<dyn ControlBarEventHandler>,
        browser_area_event_handler: Weak<dyn BrowserAreaEventHandler>,
        show_soft_navigation_buttons: bool,
    ) -> Rc<RootWidget> {
        Rc::new_cyclic(|weak: &Weak<RootWidget>| {
            let self_parent: Weak<dyn WidgetParent> = weak.clone();
            RootWidget {
                base: WidgetBase::new(widget_parent),
                show_control_bar: globals().config.show_control_bar,
                control_bar: ControlBar::new(
                    self_parent.clone(),
                    control_bar_event_handler,
                    show_soft_navigation_buttons,
                ),
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
        let (control_bar_viewport, mut browser_area_viewport) =
            self.viewport().split_y(ControlBar::HEIGHT);
        if !self.show_control_bar {
            browser_area_viewport = self.viewport();
        }
        self.control_bar.set_viewport(control_bar_viewport);
        self.browser_area.set_viewport(browser_area_viewport);
    }

    fn widget_list_children(&self) -> Vec<Rc<dyn Widget>> {
        let mut ret: Vec<Rc<dyn Widget>> = Vec::new();
        if self.show_control_bar {
            ret.push(self.control_bar.clone());
        }
        ret.push(self.browser_area.clone());
        ret
    }
}
