//! Port of `src/timeout.{hpp,cpp}`: a timeout that runs a callback on the CEF
//! UI thread after a fixed delay, unless cleared.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

pub type TimeoutFunc = Box<dyn FnOnce()>;

pub struct Timeout {
    delay_ms: i64,
    active: Cell<bool>,
    func: RefCell<Option<TimeoutFunc>>,
    func_time: Cell<Option<Instant>>,
    delayed_task_scheduled: Cell<bool>,
    delayed_task_time: Cell<Option<Instant>>,
}

impl Timeout {
    pub fn create(delay_ms: i64) -> Rc<Timeout> {
        Rc::new(Timeout {
            delay_ms: delay_ms.max(1),
            active: Cell::new(false),
            func: RefCell::new(None),
            func_time: Cell::new(None),
            delayed_task_scheduled: Cell::new(false),
            delayed_task_time: Cell::new(None),
        })
    }

    /// Sets `func` to be run in `delay_ms` milliseconds or when cleared with
    /// `run_func` set to true. Calling when the timeout is active is an error.
    pub fn set(self: &Rc<Self>, func: impl FnOnce() + 'static) {
        crate::ui::require_ui_thread();
        if self.active.get() {
            crate::panic_log!("Requirement '!active_' failed");
        }

        self.active.set(true);
        *self.func.borrow_mut() = Some(Box::new(func));
        let func_time = Instant::now() + Duration::from_millis(self.delay_ms as u64);
        self.func_time.set(Some(func_time));

        if !self.delayed_task_scheduled.get() {
            self.delayed_task_scheduled.set(true);
            self.delayed_task_time.set(Some(func_time));
            let this = self.clone();
            crate::ui::post_delayed_task(self.delay_ms, move || this.delayed_task());
        }
    }

    /// If the timeout is active, stops it. If `run_func` is true, the
    /// associated function is called immediately.
    pub fn clear(&self, run_func: bool) {
        crate::ui::require_ui_thread();
        if !self.active.get() {
            return;
        }
        self.active.set(false);
        let func = self.func.borrow_mut().take();
        if run_func && let Some(func) = func {
            func();
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.get()
    }

    fn delayed_task(self: Rc<Self>) {
        if !self.delayed_task_scheduled.get() {
            crate::panic_log!("Requirement 'delayedTaskScheduled_' failed");
        }
        self.delayed_task_scheduled.set(false);

        if !self.active.get() {
            return;
        }

        let func_time = self.func_time.get();
        let task_time = self.delayed_task_time.get();
        if func_time == task_time {
            self.active.set(false);
            let func = self.func.borrow_mut().take();
            if let Some(func) = func {
                func();
            }
        } else {
            let add_delay_ms = match (func_time, task_time) {
                (Some(f), Some(t)) => f.saturating_duration_since(t).as_millis() as i64,
                _ => 1,
            }
            .max(1);
            self.delayed_task_scheduled.set(true);
            self.delayed_task_time.set(func_time);
            let this = self.clone();
            crate::ui::post_delayed_task(add_delay_ms, move || this.delayed_task());
        }
    }
}
