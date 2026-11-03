//! Running closures on the CEF UI thread: the Rust counterpart of `postTask`
//! in `src/common.{hpp,cpp}` and of `CefPostDelayedTask` uses.
//!
//! Almost all browser state lives on the CEF UI thread and is shared through
//! `std::rc::Rc`. Closures capturing such state are not `Send`, so there are
//! two posting functions:
//!
//! - [`post_task`] for closures created on the UI thread (they are stored in a
//!   thread-local table and the CEF task only carries the table key), and
//! - [`post_task_from_any_thread`] for `Send` closures that may be posted from
//!   any thread (plugin `eventNotify`, IO thread callbacks).
//!
//! Every CEF task runs the closures in the order they were posted, because
//! CEF runs tasks posted to the same thread in FIFO order.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use cef::*;

type LocalFn = Box<dyn FnOnce()>;
type SendFn = Box<dyn FnOnce() + Send>;

thread_local! {
    static LOCAL_TASKS: RefCell<HashMap<u64, LocalFn>> = RefCell::new(HashMap::new());
    static NEXT_LOCAL_TASK_ID: Cell<u64> = const { Cell::new(1) };
}

wrap_task! {
    struct LocalTask {
        id: u64,
    }

    impl Task {
        fn execute(&self) {
            let func = LOCAL_TASKS.with(|tasks| tasks.borrow_mut().remove(&self.id));
            if let Some(func) = func {
                func();
            }
        }
    }
}

wrap_task! {
    struct SendTask {
        func: Arc<Mutex<Option<SendFn>>>,
    }

    impl Task {
        fn execute(&self) {
            let func = self.func.lock().unwrap().take();
            if let Some(func) = func {
                func();
            }
        }
    }
}

/// Returns true if the calling thread is the CEF UI thread.
pub fn on_ui_thread() -> bool {
    currently_on(ThreadId::UI) != 0
}

/// Panics (like `REQUIRE_UI_THREAD` with checks enabled) if not called on the
/// CEF UI thread.
#[track_caller]
pub fn require_ui_thread() {
    if !on_ui_thread() {
        let loc = std::panic::Location::caller();
        crate::common::panic_at(
            &format!("{}:{}", loc.file(), loc.line()),
            "Requirement 'REQUIRE_UI_THREAD()' failed",
        );
    }
}

fn store_local(func: LocalFn) -> u64 {
    let id = NEXT_LOCAL_TASK_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    });
    LOCAL_TASKS.with(|tasks| tasks.borrow_mut().insert(id, func));
    id
}

/// Posts a closure to be run later on the CEF UI thread. Must be called on
/// the UI thread (the closure may capture UI-thread-only state).
#[track_caller]
pub fn post_task(func: impl FnOnce() + 'static) {
    post_delayed_task(0, func);
}

/// Like [`post_task`], but the closure runs after `delay_ms` milliseconds.
#[track_caller]
pub fn post_delayed_task(delay_ms: i64, func: impl FnOnce() + 'static) {
    require_ui_thread();
    let id = store_local(Box::new(func));
    let mut task = LocalTask::new(id);
    let ok = if delay_ms > 0 {
        cef::post_delayed_task(ThreadId::UI, Some(&mut task), delay_ms)
    } else {
        cef::post_task(ThreadId::UI, Some(&mut task))
    };
    if ok == 0 {
        LOCAL_TASKS.with(|tasks| tasks.borrow_mut().remove(&id));
        crate::warning_log!("Posting task to the CEF UI thread failed");
    }
}

/// Posts a `Send` closure to be run on the CEF UI thread. May be called from
/// any thread.
pub fn post_task_from_any_thread(func: impl FnOnce() + Send + 'static) {
    let mut task = SendTask::new(Arc::new(Mutex::new(Some(Box::new(func)))));
    if cef::post_task(ThreadId::UI, Some(&mut task)) == 0 {
        crate::warning_log!("Posting task to the CEF UI thread failed");
    }
}
