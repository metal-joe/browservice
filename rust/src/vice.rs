//! Vice plugins through the C API declared in `vice_plugin_api.h`: port of
//! `VicePlugin`, `ViceFileUpload` and `ViceContext` in `src/vice.{hpp,cpp}`.
//!
//! A [`VicePlugin`] is the loaded shared library. A [`ViceContext`] is an
//! initialized plugin context; once started, it relays the plugin callbacks
//! (which the plugin may only call from `vicePluginAPI_pumpEvents`, except
//! `eventNotify`) to a [`ViceContextEventHandler`] (the server). Contexts are
//! used on the CEF UI thread only.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use libloading::Library;

use crate::download_manager::CompletedDownload;
use crate::temp_dir::TempDir;

pub const API_VERSION: u64 = 2_000_000;

pub const LOG_LEVEL_INFO: c_int = 100;
pub const LOG_LEVEL_WARNING: c_int = 200;
pub const LOG_LEVEL_ERROR: c_int = 300;

const MOUSE_CURSOR_NORMAL: c_int = 0;
const MOUSE_CURSOR_HAND: c_int = 1;
const MOUSE_CURSOR_TEXT: c_int = 2;

const ZOOM_INPUT_COMMAND_ZOOM_IN: c_int = 0;
const ZOOM_INPUT_COMMAND_ZOOM_OUT: c_int = 1;
const ZOOM_INPUT_COMMAND_ZOOM_RESET: c_int = 2;

type Ctx = c_void;

type LogCallback = unsafe extern "C" fn(*mut c_void, c_int, *const c_char, *const c_char);
type PanicCallback = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char);
type DestructorCallback = unsafe extern "C" fn(*mut c_void);
type OptionDocsCallback =
    unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char, *const c_char);
type PutImageFunc = unsafe extern "C" fn(*mut c_void, *const u8, usize, usize, usize);
type CleanupFunc = unsafe extern "C" fn(*mut c_void);

/// `VicePluginAPI_Callbacks` (field order as in `vice_plugin_api.h`).
#[repr(C)]
#[derive(Clone, Copy)]
struct Callbacks {
    event_notify: Option<unsafe extern "C" fn(*mut c_void)>,
    shutdown_complete: Option<unsafe extern "C" fn(*mut c_void)>,
    create_window: Option<unsafe extern "C" fn(*mut c_void, *mut *mut c_char) -> u64>,
    close_window: Option<unsafe extern "C" fn(*mut c_void, u64)>,
    resize_window: Option<unsafe extern "C" fn(*mut c_void, u64, usize, usize)>,
    fetch_window_image: Option<unsafe extern "C" fn(*mut c_void, u64, PutImageFunc, *mut c_void)>,
    mouse_down: Option<unsafe extern "C" fn(*mut c_void, u64, c_int, c_int, c_int)>,
    mouse_up: Option<unsafe extern "C" fn(*mut c_void, u64, c_int, c_int, c_int)>,
    mouse_move: Option<unsafe extern "C" fn(*mut c_void, u64, c_int, c_int)>,
    mouse_double_click: Option<unsafe extern "C" fn(*mut c_void, u64, c_int, c_int, c_int)>,
    mouse_wheel: Option<unsafe extern "C" fn(*mut c_void, u64, c_int, c_int, c_int, c_int)>,
    mouse_leave: Option<unsafe extern "C" fn(*mut c_void, u64, c_int, c_int)>,
    key_down: Option<unsafe extern "C" fn(*mut c_void, u64, c_int)>,
    key_up: Option<unsafe extern "C" fn(*mut c_void, u64, c_int)>,
    lose_focus: Option<unsafe extern "C" fn(*mut c_void, u64)>,
    navigate: Option<unsafe extern "C" fn(*mut c_void, u64, c_int)>,
    copy_to_clipboard: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    request_clipboard_content: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
    upload_file: Option<
        unsafe extern "C" fn(*mut c_void, u64, *const c_char, *const c_char, CleanupFunc, *mut c_void),
    >,
    cancel_file_upload: Option<unsafe extern "C" fn(*mut c_void, u64)>,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UriNavigationCallbacks {
    create_window_with_uri:
        Option<unsafe extern "C" fn(*mut c_void, *mut *mut c_char, *const c_char) -> u64>,
    navigate_window_to_uri: Option<unsafe extern "C" fn(*mut c_void, u64, *const c_char)>,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct WindowTitleCallbacks {
    get_window_title: Option<unsafe extern "C" fn(*mut c_void, u64) -> *mut c_char>,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ZoomInputCallbacks {
    zoom_command: Option<unsafe extern "C" fn(*mut c_void, u64, c_int)>,
}

/// Function pointers of the plugin API (`VicePlugin::APIFuncs`).
struct ApiFuncs {
    is_api_version_supported: unsafe extern "C" fn(u64) -> c_int,
    create_version_string: unsafe extern "C" fn() -> *mut c_char,
    create_credits_string: unsafe extern "C" fn() -> *mut c_char,
    malloc: unsafe extern "C" fn(usize) -> *mut c_void,
    free: unsafe extern "C" fn(*mut c_void),
    init_context: unsafe extern "C" fn(
        u64,
        *const *const c_char,
        *const *const c_char,
        usize,
        *const c_char,
        *mut *mut c_char,
    ) -> *mut Ctx,
    destroy_context: unsafe extern "C" fn(*mut Ctx),
    start: unsafe extern "C" fn(*mut Ctx, Callbacks, *mut c_void),
    shutdown: unsafe extern "C" fn(*mut Ctx),
    pump_events: unsafe extern "C" fn(*mut Ctx),
    create_popup_window: unsafe extern "C" fn(*mut Ctx, u64, u64, *mut *mut c_char) -> c_int,
    close_window: unsafe extern "C" fn(*mut Ctx, u64),
    notify_window_view_changed: unsafe extern "C" fn(*mut Ctx, u64),
    set_window_cursor: unsafe extern "C" fn(*mut Ctx, u64, c_int),
    window_quality_selector_query:
        unsafe extern "C" fn(*mut Ctx, u64, *mut *mut c_char, *mut usize) -> c_int,
    window_quality_changed: unsafe extern "C" fn(*mut Ctx, u64, usize),
    window_needs_clipboard_button_query: unsafe extern "C" fn(*mut Ctx, u64) -> c_int,
    window_clipboard_button_pressed: unsafe extern "C" fn(*mut Ctx, u64),
    put_clipboard_content: unsafe extern "C" fn(*mut Ctx, *const c_char),
    put_file_download: unsafe extern "C" fn(
        *mut Ctx,
        u64,
        *const c_char,
        *const c_char,
        CleanupFunc,
        *mut c_void,
    ),
    start_file_upload: unsafe extern "C" fn(*mut Ctx, u64) -> c_int,
    cancel_file_upload: unsafe extern "C" fn(*mut Ctx, u64),
    get_option_docs: unsafe extern "C" fn(u64, OptionDocsCallback, *mut c_void),
    set_global_log_callback:
        unsafe extern "C" fn(u64, Option<LogCallback>, *mut c_void, Option<DestructorCallback>),
    set_global_panic_callback:
        unsafe extern "C" fn(u64, Option<PanicCallback>, *mut c_void, Option<DestructorCallback>),
    is_extension_supported: unsafe extern "C" fn(u64, *const c_char) -> c_int,

    uri_navigation_enable: Option<unsafe extern "C" fn(*mut Ctx, UriNavigationCallbacks)>,
    plugin_navigation_control_support_query: Option<unsafe extern "C" fn(*mut Ctx) -> c_int>,
    window_title_enable: Option<unsafe extern "C" fn(*mut Ctx, WindowTitleCallbacks)>,
    window_title_notify_window_title_changed: Option<unsafe extern "C" fn(*mut Ctx, u64)>,
    zoom_input_enable: Option<unsafe extern "C" fn(*mut Ctx, ZoomInputCallbacks)>,
}

/// All API functions that a plugin must export (FOREACH_REQUIRED_VICE_API_FUNC
/// in `src/vice.cpp`).
const REQUIRED_FUNCS: &[&str] = &[
    "isAPIVersionSupported",
    "createVersionString",
    "createCreditsString",
    "malloc",
    "free",
    "initContext",
    "destroyContext",
    "start",
    "shutdown",
    "pumpEvents",
    "createPopupWindow",
    "closeWindow",
    "notifyWindowViewChanged",
    "setWindowCursor",
    "windowQualitySelectorQuery",
    "windowQualityChanged",
    "windowNeedsClipboardButtonQuery",
    "windowClipboardButtonPressed",
    "putClipboardContent",
    "putFileDownload",
    "startFileUpload",
    "cancelFileUpload",
    "getOptionDocs",
    "setGlobalLogCallback",
    "setGlobalPanicCallback",
    "isExtensionSupported",
];

/// Optional extensions and their functions, loaded only if the plugin reports
/// support for the extension.
const EXTENSIONS: &[(&str, &[&str])] = &[
    ("URINavigation", &["URINavigation_enable"]),
    (
        "PluginNavigationControlSupportQuery",
        &["PluginNavigationControlSupportQuery_query"],
    ),
    (
        "WindowTitle",
        &["WindowTitle_enable", "WindowTitle_notifyWindowTitleChanged"],
    ),
    ("ZoomInput", &["ZoomInput_enable"]),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionDocsItem {
    pub name: String,
    pub val_spec: String,
    pub desc: String,
    pub default_val_str: String,
}

pub struct VicePlugin {
    filename: String,
    api: ApiFuncs,
    supported_extensions: Vec<&'static str>,
    // Must be dropped last (declared last); the function pointers above point
    // into this library.
    _lib: Library,
}

fn c_str_lossy(ptr: *const c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }
}

fn c_str_bytes(ptr: *const c_char) -> Vec<u8> {
    if ptr.is_null() {
        Vec::new()
    } else {
        unsafe { CStr::from_ptr(ptr) }.to_bytes().to_vec()
    }
}

/// CString from a Rust string, truncated at the first NUL byte (like passing
/// `std::string::c_str()` in C++).
fn to_cstring(s: &str) -> CString {
    let bytes = s.as_bytes();
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    CString::new(&bytes[..end]).unwrap()
}

unsafe extern "C" fn log_callback(
    filename_ptr: *mut c_void,
    log_level: c_int,
    location: *const c_char,
    msg: *const c_char,
) {
    let filename = unsafe { &*(filename_ptr as *const String) };
    let level = match log_level {
        LOG_LEVEL_ERROR => "ERROR",
        LOG_LEVEL_WARNING => "WARNING",
        _ => {
            if log_level != LOG_LEVEL_INFO {
                crate::warning_log!(
                    "Incoming log message from vice plugin {}with unknown log level, defaulting to INFO",
                    filename
                );
            }
            "INFO"
        }
    };
    crate::common::write_log(
        level,
        &format!("{} {}", filename, c_str_lossy(location)),
        &c_str_lossy(msg),
    );
}

unsafe extern "C" fn panic_callback(
    filename_ptr: *mut c_void,
    location: *const c_char,
    msg: *const c_char,
) {
    let filename = unsafe { &*(filename_ptr as *const String) };
    crate::common::panic_at(
        &format!("{} {}", filename, c_str_lossy(location)),
        &c_str_lossy(msg),
    );
}

unsafe extern "C" fn destructor_callback(filename_ptr: *mut c_void) {
    drop(unsafe { Box::from_raw(filename_ptr as *mut String) });
}

unsafe extern "C" fn option_docs_callback(
    data: *mut c_void,
    name: *const c_char,
    val_spec: *const c_char,
    desc: *const c_char,
    default_val_str: *const c_char,
) {
    let ret = unsafe { &mut *(data as *mut Vec<OptionDocsItem>) };
    ret.push(OptionDocsItem {
        name: c_str_lossy(name),
        val_spec: c_str_lossy(val_spec),
        desc: c_str_lossy(desc),
        default_val_str: c_str_lossy(default_val_str),
    });
}

fn open_library(filename: &str) -> Result<Library, String> {
    #[cfg(unix)]
    {
        let flags = libc::RTLD_NOW | libc::RTLD_LOCAL;
        #[cfg(all(target_os = "linux", not(target_env = "musl")))]
        let flags = flags | libc::RTLD_DEEPBIND;
        unsafe { libloading::os::unix::Library::open(Some(filename), flags) }
            .map(Library::from)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        unsafe { Library::new(filename) }.map_err(|e| e.to_string())
    }
}

/// Resolves `vicePluginAPI_<name>` from the library as a raw pointer.
fn load_sym(lib: &Library, filename: &str, name: &str) -> Option<*const c_void> {
    let sym_name = format!("vicePluginAPI_{name}\0");
    match unsafe { lib.get::<*const c_void>(sym_name.as_bytes()) } {
        Ok(sym) => Some(*sym),
        Err(e) => {
            crate::error_log!(
                "Loading symbol 'vicePluginAPI_{}' from vice plugin {} failed: {}",
                name,
                filename,
                e
            );
            None
        }
    }
}

impl VicePlugin {
    /// Loads the plugin shared library. Returns None (after logging the error)
    /// if loading fails.
    pub fn load(filename: &str) -> Option<VicePlugin> {
        let lib = match open_library(filename) {
            Ok(lib) => lib,
            Err(err) => {
                crate::error_log!("Loading vice plugin library '{}' failed: {}", filename, err);
                return None;
            }
        };

        let mut syms = std::collections::HashMap::new();
        for &name in REQUIRED_FUNCS {
            syms.insert(name, load_sym(&lib, filename, name)?);
        }

        // SAFETY: the symbols are declared with these signatures in
        // vice_plugin_api.h.
        #[allow(clippy::missing_transmute_annotations)]
        let mut api = unsafe {
            use std::mem::transmute as t;
            ApiFuncs {
                is_api_version_supported: t(syms["isAPIVersionSupported"]),
                create_version_string: t(syms["createVersionString"]),
                create_credits_string: t(syms["createCreditsString"]),
                malloc: t(syms["malloc"]),
                free: t(syms["free"]),
                init_context: t(syms["initContext"]),
                destroy_context: t(syms["destroyContext"]),
                start: t(syms["start"]),
                shutdown: t(syms["shutdown"]),
                pump_events: t(syms["pumpEvents"]),
                create_popup_window: t(syms["createPopupWindow"]),
                close_window: t(syms["closeWindow"]),
                notify_window_view_changed: t(syms["notifyWindowViewChanged"]),
                set_window_cursor: t(syms["setWindowCursor"]),
                window_quality_selector_query: t(syms["windowQualitySelectorQuery"]),
                window_quality_changed: t(syms["windowQualityChanged"]),
                window_needs_clipboard_button_query: t(syms["windowNeedsClipboardButtonQuery"]),
                window_clipboard_button_pressed: t(syms["windowClipboardButtonPressed"]),
                put_clipboard_content: t(syms["putClipboardContent"]),
                put_file_download: t(syms["putFileDownload"]),
                start_file_upload: t(syms["startFileUpload"]),
                cancel_file_upload: t(syms["cancelFileUpload"]),
                get_option_docs: t(syms["getOptionDocs"]),
                set_global_log_callback: t(syms["setGlobalLogCallback"]),
                set_global_panic_callback: t(syms["setGlobalPanicCallback"]),
                is_extension_supported: t(syms["isExtensionSupported"]),
                uri_navigation_enable: None,
                plugin_navigation_control_support_query: None,
                window_title_enable: None,
                window_title_notify_window_title_changed: None,
                zoom_input_enable: None,
            }
        };

        if unsafe { (api.is_api_version_supported)(API_VERSION) } == 0 {
            crate::error_log!(
                "Vice plugin {} does not support API version {}",
                filename,
                API_VERSION
            );
            return None;
        }

        unsafe {
            (api.set_global_log_callback)(
                API_VERSION,
                Some(log_callback),
                Box::into_raw(Box::new(filename.to_string())).cast(),
                Some(destructor_callback),
            );
            (api.set_global_panic_callback)(
                API_VERSION,
                Some(panic_callback),
                Box::into_raw(Box::new(filename.to_string())).cast(),
                Some(destructor_callback),
            );
        }

        let mut supported_extensions = Vec::new();
        for &(ext, funcs) in EXTENSIONS {
            let ext_c = CString::new(ext).unwrap();
            if unsafe { (api.is_extension_supported)(API_VERSION, ext_c.as_ptr()) } != 0 {
                for &func in funcs {
                    let sym = load_sym(&lib, filename, func)?;
                    #[allow(clippy::missing_transmute_annotations)]
                    unsafe {
                        use std::mem::transmute as t;
                        match func {
                            "URINavigation_enable" => api.uri_navigation_enable = Some(t(sym)),
                            "PluginNavigationControlSupportQuery_query" => {
                                api.plugin_navigation_control_support_query = Some(t(sym))
                            }
                            "WindowTitle_enable" => api.window_title_enable = Some(t(sym)),
                            "WindowTitle_notifyWindowTitleChanged" => {
                                api.window_title_notify_window_title_changed = Some(t(sym))
                            }
                            "ZoomInput_enable" => api.zoom_input_enable = Some(t(sym)),
                            _ => unreachable!(),
                        }
                    }
                }
                supported_extensions.push(ext);
            }
        }

        Some(VicePlugin {
            filename: filename.to_string(),
            api,
            supported_extensions,
            _lib: lib,
        })
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn supported_extensions(&self) -> &[&'static str] {
        &self.supported_extensions
    }

    fn take_plugin_string(&self, raw: *mut c_char) -> String {
        let val = c_str_lossy(raw);
        unsafe { (self.api.free)(raw.cast()) };
        val
    }

    pub fn version_string(&self) -> String {
        let raw = unsafe { (self.api.create_version_string)() };
        self.take_plugin_string(raw)
    }

    pub fn credits_string(&self) -> String {
        let raw = unsafe { (self.api.create_credits_string)() };
        self.take_plugin_string(raw)
    }

    pub fn option_docs(&self) -> Vec<OptionDocsItem> {
        let mut ret: Vec<OptionDocsItem> = Vec::new();
        unsafe {
            (self.api.get_option_docs)(
                API_VERSION,
                option_docs_callback,
                (&mut ret as *mut Vec<OptionDocsItem>).cast(),
            );
        }
        ret
    }

    /// A string allocated with the plugin's malloc (`createMallocString`).
    fn create_malloc_string(&self, val: &str) -> *mut c_char {
        let c = to_cstring(val);
        let bytes = c.as_bytes_with_nul();
        let ret = unsafe { (self.api.malloc)(bytes.len()) } as *mut u8;
        if ret.is_null() {
            crate::panic_log!("Requirement 'ret != nullptr' failed");
        }
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ret, bytes.len()) };
        ret.cast()
    }
}

impl Drop for VicePlugin {
    fn drop(&mut self) {
        // Revert the plugin to its default logging so that it does not call
        // back into our callbacks after the library handle is gone; the plugin
        // calls the destructor callbacks for the filename boxes.
        unsafe {
            (self.api.set_global_log_callback)(API_VERSION, None, std::ptr::null_mut(), None);
            (self.api.set_global_panic_callback)(API_VERSION, None, std::ptr::null_mut(), None);
        }
    }
}

// ---------------------------------------------------------------------------
// File uploads

fn sanitize_filename(src: &str) -> String {
    let mut ret: String = src.chars().filter(|&c| c != '/' && c != '\\' && c != '\0').collect();
    if ret.len() > 200 {
        let mut end = 200;
        while !ret.is_char_boundary(end) {
            end -= 1;
        }
        ret.truncate(end);
    }
    if ret == "." || ret == ".." {
        ret += "_file.bin";
    }
    if ret.is_empty() {
        ret = "file.bin".to_string();
    }
    ret
}

/// A file uploaded through the plugin, exposed to CEF through a symbolic link
/// with the sanitized original name in a temporary directory. Dropping it
/// removes the link and calls the cleanup function of the plugin.
pub struct ViceFileUpload {
    _temp_dir: Arc<TempDir>,
    link_dir: String,
    link_path: String,
    src_cleanup: Option<Box<dyn FnOnce()>>,
}

impl ViceFileUpload {
    fn create(
        temp_dir: Arc<TempDir>,
        upload_idx: u64,
        name: &str,
        src_path: &str,
        src_cleanup: Box<dyn FnOnce()>,
    ) -> Rc<ViceFileUpload> {
        let name = sanitize_filename(name);
        let link_dir = format!("{}/{}", temp_dir.path(), upload_idx);
        let link_path = format!("{link_dir}/{name}");

        use std::os::unix::fs::DirBuilderExt;
        if std::fs::DirBuilder::new().mode(0o777).create(&link_dir).is_err() {
            crate::panic_log!("Requirement 'mkdir(linkDir_.c_str(), 0777) == 0' failed");
        }
        if std::os::unix::fs::symlink(src_path, &link_path).is_err() {
            crate::panic_log!("Requirement 'symlink(srcPath_.c_str(), linkPath_.c_str()) == 0' failed");
        }

        Rc::new(ViceFileUpload {
            _temp_dir: temp_dir,
            link_dir,
            link_path,
            src_cleanup: Some(src_cleanup),
        })
    }

    pub fn path(&self) -> &str {
        &self.link_path
    }
}

impl Drop for ViceFileUpload {
    fn drop(&mut self) {
        if std::fs::remove_file(&self.link_path).is_err() {
            crate::warning_log!("Unlinking temporary symlink {} failed", self.link_path);
        }
        if std::fs::remove_dir(&self.link_dir).is_err() {
            crate::warning_log!("Deleting temporary directory {} failed", self.link_dir);
        }
        if let Some(cleanup) = self.src_cleanup.take() {
            cleanup();
        }
    }
}

// ---------------------------------------------------------------------------
// Contexts

/// Event handler of a running context (implemented by the server).
/// Implementations may NOT call functions of the [`ViceContext`] directly.
pub trait ViceContextEventHandler {
    /// To deny window creation, return 0 and optionally set `reason`.
    fn on_vice_context_create_window_request(&self, reason: &mut String, uri: Option<String>) -> u64;
    fn on_vice_context_close_window(&self, window: u64);
    fn on_vice_context_resize_window(&self, window: u64, width: i32, height: i32);
    /// Must call `put_image(image, width, height, pitch)` exactly once.
    fn on_vice_context_fetch_window_image(
        &self,
        window: u64,
        put_image: &mut dyn FnMut(*const u8, usize, usize, usize),
    );
    fn on_vice_context_mouse_down(&self, window: u64, x: i32, y: i32, button: i32);
    fn on_vice_context_mouse_up(&self, window: u64, x: i32, y: i32, button: i32);
    fn on_vice_context_mouse_move(&self, window: u64, x: i32, y: i32);
    fn on_vice_context_mouse_double_click(&self, window: u64, x: i32, y: i32, button: i32);
    fn on_vice_context_mouse_wheel(&self, window: u64, x: i32, y: i32, dx: i32, dy: i32);
    fn on_vice_context_mouse_leave(&self, window: u64, x: i32, y: i32);
    fn on_vice_context_key_down(&self, window: u64, key: i32);
    fn on_vice_context_key_up(&self, window: u64, key: i32);
    fn on_vice_context_lose_focus(&self, window: u64);
    fn on_vice_context_navigate(&self, window: u64, direction: i32);
    fn on_vice_context_navigate_to_uri(&self, window: u64, uri: String);
    fn on_vice_context_copy_to_clipboard(&self, text: String);
    fn on_vice_context_request_clipboard_content(&self);
    fn on_vice_context_upload_file(&self, window: u64, file: Rc<ViceFileUpload>);
    fn on_vice_context_cancel_file_upload(&self, window: u64);
    fn on_vice_context_window_title_query(&self, window: u64) -> String;
    fn on_vice_context_zoom_in(&self, window: u64);
    fn on_vice_context_zoom_out(&self, window: u64);
    fn on_vice_context_zoom_reset(&self, window: u64);
    fn on_vice_context_shutdown_complete(&self);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Pending,
    Running,
    ShutdownComplete,
}

/// Flags that `eventNotify` (any thread) may touch.
#[derive(Default)]
struct SharedFlags {
    pump_events_in_queue: AtomicBool,
    shutdown_complete: AtomicBool,
}

/// The callback data pointer given to the plugin. It is intentionally leaked
/// (like the weak pointer in debug builds of the C++ version) so that a
/// misbehaving plugin calling callbacks after shutdown is detected instead of
/// causing use-after-free.
struct CallbackData {
    flags: Arc<SharedFlags>,
    /// Only accessed on the CEF UI thread.
    ctx: Weak<ViceContext>,
}

struct SendPtr(*const CallbackData);
unsafe impl Send for SendPtr {}

thread_local! {
    static THREAD_ACTIVE_PUMP_EVENTS_CONTEXT: Cell<*const ViceContext> =
        const { Cell::new(std::ptr::null()) };
}

pub struct ViceContext {
    plugin: Rc<VicePlugin>,
    ctx: *mut Ctx,
    has_navigation_controls: Option<bool>,
    state: Cell<State>,
    shutdown_pending: Cell<bool>,
    flags: Arc<SharedFlags>,
    event_handler: RefCell<Option<Rc<dyn ViceContextEventHandler>>>,
    /// Keeps a running context alive.
    self_: RefCell<Option<Rc<ViceContext>>>,
    open_windows: RefCell<BTreeSet<u64>>,
    upload_mode_windows: RefCell<BTreeSet<u64>>,
    upload_temp_dir: Arc<TempDir>,
    next_upload_idx: Cell<u64>,
}

macro_rules! require {
    ($cond:expr) => {
        if !($cond) {
            crate::panic_log!("Requirement '{}' failed", stringify!($cond));
        }
    };
}

/// Resolves the callback data to the context (`getContext_`, with the checks
/// of the C++ debug build).
unsafe fn context_from_data(data: *mut c_void) -> Rc<ViceContext> {
    if data.is_null() {
        crate::panic_log!("Vice plugin sent unexpected NULL pointer as callback data");
    }
    let data = unsafe { &*(data as *const CallbackData) };
    match data.ctx.upgrade() {
        Some(ctx) => {
            if ctx.state.get() == State::ShutdownComplete {
                crate::panic_log!("Vice plugin called a callback for a context that has shut down");
            }
            require!(ctx.state.get() == State::Running);
            ctx
        }
        None => crate::panic_log!("Vice plugin called a callback for a context that has been destroyed"),
    }
}

/// Like [`context_from_data`], additionally checking that the callback is
/// called from `vicePluginAPI_pumpEvents` (CTX_CALLBACK in `src/vice.cpp`).
unsafe fn pumped_context(data: *mut c_void) -> Rc<ViceContext> {
    let ctx = unsafe { context_from_data(data) };
    let active = THREAD_ACTIVE_PUMP_EVENTS_CONTEXT.with(|c| c.get());
    if !std::ptr::eq(active, Rc::as_ptr(&ctx)) {
        crate::panic_log!(
            "Vice plugin unexpectedly called a callback in a thread that is not currently \
             executing vicePluginAPI_pumpEvents"
        );
    }
    crate::ui::require_ui_thread();
    ctx
}

impl ViceContext {
    fn handler(&self) -> Rc<dyn ViceContextEventHandler> {
        match self.event_handler.borrow().clone() {
            Some(h) => h,
            None => crate::panic_log!("Requirement 'eventHandler_' failed"),
        }
    }

    fn require_open(&self, window: u64) {
        if !self.open_windows.borrow().contains(&window) {
            crate::panic_log!("Requirement 'self->openWindows_.count(window)' failed");
        }
    }

    /// Initializes a plugin context. Returns None if initialization failed
    /// (the reason is written to stderr).
    pub fn init(plugin: Rc<VicePlugin>, options: &[(String, String)]) -> Option<Rc<ViceContext>> {
        let names: Vec<CString> = options.iter().map(|(n, _)| to_cstring(n)).collect();
        let values: Vec<CString> = options.iter().map(|(_, v)| to_cstring(v)).collect();
        let name_ptrs: Vec<*const c_char> = names.iter().map(|s| s.as_ptr()).collect();
        let value_ptrs: Vec<*const c_char> = values.iter().map(|s| s.as_ptr()).collect();
        let program_name = CString::new("Browservice").unwrap();

        let mut init_error_msg: *mut c_char = std::ptr::null_mut();
        let ctx = unsafe {
            (plugin.api.init_context)(
                API_VERSION,
                name_ptrs.as_ptr(),
                value_ptrs.as_ptr(),
                options.len(),
                program_name.as_ptr(),
                &mut init_error_msg,
            )
        };

        if ctx.is_null() {
            require!(!init_error_msg.is_null());
            eprintln!(
                "ERROR: Vice plugin {} initialization failed: {}",
                plugin.filename,
                c_str_lossy(init_error_msg)
            );
            unsafe { (plugin.api.free)(init_error_msg.cast()) };
            return None;
        }

        let has_navigation_controls = plugin
            .api
            .plugin_navigation_control_support_query
            .map(|query| {
                let value = unsafe { query(ctx) };
                require!(value == 0 || value == 1);
                value == 1
            });

        Some(Rc::new(ViceContext {
            plugin,
            ctx,
            has_navigation_controls,
            state: Cell::new(State::Pending),
            shutdown_pending: Cell::new(false),
            flags: Arc::new(SharedFlags::default()),
            event_handler: RefCell::new(None),
            self_: RefCell::new(None),
            open_windows: RefCell::new(BTreeSet::new()),
            upload_mode_windows: RefCell::new(BTreeSet::new()),
            upload_temp_dir: Arc::new(TempDir::create()),
            next_upload_idx: Cell::new(1),
        }))
    }

    /// Does the plugin implement navigation controls? (None if the plugin does
    /// not support the PluginNavigationControlSupportQuery extension.)
    pub fn has_navigation_controls(&self) -> Option<bool> {
        self.has_navigation_controls
    }

    /// Starts running the context. Before quitting the CEF message loop, call
    /// [`ViceContext::shutdown`] and wait for the shutdown complete event.
    pub fn start(self: &Rc<Self>, event_handler: Rc<dyn ViceContextEventHandler>) {
        crate::ui::require_ui_thread();
        require!(self.state.get() == State::Pending);

        self.state.set(State::Running);
        *self.event_handler.borrow_mut() = Some(event_handler);
        *self.self_.borrow_mut() = Some(self.clone());

        let callback_data = Box::into_raw(Box::new(CallbackData {
            flags: self.flags.clone(),
            ctx: Rc::downgrade(self),
        }))
        .cast::<c_void>();

        let api = &self.plugin.api;

        if let Some(enable) = api.uri_navigation_enable {
            unsafe {
                enable(
                    self.ctx,
                    UriNavigationCallbacks {
                        create_window_with_uri: Some(cb_create_window_with_uri),
                        navigate_window_to_uri: Some(cb_navigate_window_to_uri),
                    },
                )
            };
        }
        if let Some(enable) = api.window_title_enable {
            unsafe {
                enable(
                    self.ctx,
                    WindowTitleCallbacks {
                        get_window_title: Some(cb_get_window_title),
                    },
                )
            };
        }
        if let Some(enable) = api.zoom_input_enable {
            crate::info_log!("Initializing ZoomInput plugin");
            unsafe {
                enable(
                    self.ctx,
                    ZoomInputCallbacks {
                        zoom_command: Some(cb_zoom_command),
                    },
                )
            };
        }

        let callbacks = Callbacks {
            event_notify: Some(cb_event_notify),
            shutdown_complete: Some(cb_shutdown_complete),
            create_window: Some(cb_create_window),
            close_window: Some(cb_close_window),
            resize_window: Some(cb_resize_window),
            fetch_window_image: Some(cb_fetch_window_image),
            mouse_down: Some(cb_mouse_down),
            mouse_up: Some(cb_mouse_up),
            mouse_move: Some(cb_mouse_move),
            mouse_double_click: Some(cb_mouse_double_click),
            mouse_wheel: Some(cb_mouse_wheel),
            mouse_leave: Some(cb_mouse_leave),
            key_down: Some(cb_key_down),
            key_up: Some(cb_key_up),
            lose_focus: Some(cb_lose_focus),
            navigate: Some(cb_navigate),
            copy_to_clipboard: Some(cb_copy_to_clipboard),
            request_clipboard_content: Some(cb_request_clipboard_content),
            upload_file: Some(cb_upload_file),
            cancel_file_upload: Some(cb_cancel_file_upload),
        };
        unsafe { (api.start)(self.ctx, callbacks, callback_data) };
    }

    pub fn shutdown(&self) {
        crate::ui::require_ui_thread();
        require!(self.state.get() == State::Running);
        require!(!self.shutdown_pending.get());
        self.shutdown_pending.set(true);
        unsafe { (self.plugin.api.shutdown)(self.ctx) };
    }

    fn running_context_func_checks(&self) {
        crate::ui::require_ui_thread();
        require!(self.state.get() == State::Running);
        if !THREAD_ACTIVE_PUMP_EVENTS_CONTEXT.with(|c| c.get()).is_null() {
            crate::panic_log!("Requirement 'threadActivePumpEventsContext == nullptr' failed");
        }
    }

    /// Asks the plugin to create a popup window. On denial returns
    /// Err(reason).
    pub fn request_create_popup(&self, parent_window: u64, popup_window: u64) -> Result<(), String> {
        self.running_context_func_checks();
        self.require_open(parent_window);
        require!(popup_window != 0);
        require!(!self.open_windows.borrow().contains(&popup_window));

        let mut msg: *mut c_char = std::ptr::null_mut();
        let result = unsafe {
            (self.plugin.api.create_popup_window)(self.ctx, parent_window, popup_window, &mut msg)
        };
        require!(result == 0 || result == 1);
        if result == 1 {
            require!(msg.is_null());
            self.open_windows.borrow_mut().insert(popup_window);
            Ok(())
        } else {
            require!(!msg.is_null());
            let reason = c_str_lossy(msg);
            unsafe { (self.plugin.api.free)(msg.cast()) };
            Err(reason)
        }
    }

    pub fn close_window(&self, window: u64) {
        self.running_context_func_checks();
        if !self.open_windows.borrow_mut().remove(&window) {
            crate::panic_log!("Requirement 'openWindows_.erase(window)' failed");
        }
        self.upload_mode_windows.borrow_mut().remove(&window);
        unsafe { (self.plugin.api.close_window)(self.ctx, window) };
    }

    pub fn notify_window_view_changed(&self, window: u64) {
        self.running_context_func_checks();
        self.require_open(window);
        unsafe { (self.plugin.api.notify_window_view_changed)(self.ctx, window) };
    }

    pub fn notify_window_title_changed(&self, window: u64) {
        self.running_context_func_checks();
        self.require_open(window);
        if self.plugin.api.window_title_enable.is_some() {
            let Some(notify) = self.plugin.api.window_title_notify_window_title_changed else {
                crate::panic_log!(
                    "Requirement 'WindowTitle_notifyWindowTitleChanged != nullptr' failed"
                );
            };
            unsafe { notify(self.ctx, window) };
        }
    }

    pub fn set_window_cursor(&self, window: u64, cursor: i32) {
        self.running_context_func_checks();
        self.require_open(window);
        use crate::widget::{HAND_CURSOR, NORMAL_CURSOR, TEXT_CURSOR};
        let api_cursor = match cursor {
            HAND_CURSOR => MOUSE_CURSOR_HAND,
            TEXT_CURSOR => MOUSE_CURSOR_TEXT,
            NORMAL_CURSOR => MOUSE_CURSOR_NORMAL,
            _ => crate::panic_log!("Requirement 'cursor == NormalCursor' failed"),
        };
        unsafe { (self.plugin.api.set_window_cursor)(self.ctx, window, api_cursor) };
    }

    pub fn window_quality_selector_query(&self, window: u64) -> Option<(Vec<String>, usize)> {
        self.running_context_func_checks();
        self.require_open(window);

        let mut quality_list: *mut c_char = std::ptr::null_mut();
        let mut current_quality: usize = usize::MAX;
        let result = unsafe {
            (self.plugin.api.window_quality_selector_query)(
                self.ctx,
                window,
                &mut quality_list,
                &mut current_quality,
            )
        };
        require!(result == 0 || result == 1);
        if result == 0 {
            return None;
        }

        require!(!quality_list.is_null());
        let label_str = c_str_bytes(quality_list);
        unsafe { (self.plugin.api.free)(quality_list.cast()) };

        let labels = parse_quality_labels(&label_str);
        require!(!labels.is_empty());
        require!(current_quality < labels.len());
        Some((labels, current_quality))
    }

    pub fn window_quality_changed(&self, window: u64, idx: usize) {
        self.running_context_func_checks();
        self.require_open(window);
        unsafe { (self.plugin.api.window_quality_changed)(self.ctx, window, idx) };
    }

    pub fn window_needs_clipboard_button_query(&self, window: u64) -> bool {
        self.running_context_func_checks();
        self.require_open(window);
        let result =
            unsafe { (self.plugin.api.window_needs_clipboard_button_query)(self.ctx, window) };
        require!(result == 0 || result == 1);
        result == 1
    }

    pub fn window_clipboard_button_pressed(&self, window: u64) {
        self.running_context_func_checks();
        self.require_open(window);
        unsafe { (self.plugin.api.window_clipboard_button_pressed)(self.ctx, window) };
    }

    pub fn put_clipboard_content(&self, text: &str) {
        self.running_context_func_checks();
        let text = to_cstring(text);
        unsafe { (self.plugin.api.put_clipboard_content)(self.ctx, text.as_ptr()) };
    }

    pub fn put_file_download(&self, window: u64, file: Arc<CompletedDownload>) {
        self.running_context_func_checks();
        self.require_open(window);

        let name = to_cstring(file.name());
        let path = to_cstring(file.path());

        unsafe extern "C" fn cleanup(data: *mut c_void) {
            if data.is_null() {
                crate::panic_log!("Requirement 'cleanupData != nullptr' failed");
            }
            drop(unsafe { Box::from_raw(data as *mut Arc<CompletedDownload>) });
        }

        unsafe {
            (self.plugin.api.put_file_download)(
                self.ctx,
                window,
                name.as_ptr(),
                path.as_ptr(),
                cleanup,
                Box::into_raw(Box::new(file)).cast(),
            )
        };
    }

    pub fn start_file_upload(&self, window: u64) -> bool {
        self.running_context_func_checks();
        self.require_open(window);
        require!(!self.upload_mode_windows.borrow().contains(&window));
        let result = unsafe { (self.plugin.api.start_file_upload)(self.ctx, window) };
        require!(result == 0 || result == 1);
        if result == 1 {
            self.upload_mode_windows.borrow_mut().insert(window);
        }
        result == 1
    }

    pub fn cancel_file_upload(&self, window: u64) {
        self.running_context_func_checks();
        self.require_open(window);
        if !self.upload_mode_windows.borrow_mut().remove(&window) {
            crate::panic_log!("Requirement 'uploadModeWindows_.erase(window)' failed");
        }
        unsafe { (self.plugin.api.cancel_file_upload)(self.ctx, window) };
    }

    fn pump_events(&self) {
        crate::ui::require_ui_thread();
        if !THREAD_ACTIVE_PUMP_EVENTS_CONTEXT.with(|c| c.get()).is_null() {
            crate::panic_log!("Requirement 'threadActivePumpEventsContext == nullptr' failed");
        }
        if self.state.get() != State::Running {
            return;
        }
        self.flags.pump_events_in_queue.store(false, Ordering::SeqCst);

        THREAD_ACTIVE_PUMP_EVENTS_CONTEXT.with(|c| c.set(self as *const ViceContext));
        unsafe { (self.plugin.api.pump_events)(self.ctx) };
        THREAD_ACTIVE_PUMP_EVENTS_CONTEXT.with(|c| c.set(std::ptr::null()));
    }

    fn shutdown_complete(&self) {
        crate::ui::require_ui_thread();
        require!(self.state.get() == State::Running);
        require!(self.shutdown_pending.get());

        self.state.set(State::ShutdownComplete);
        self.shutdown_pending.set(false);
        self.flags.shutdown_complete.store(true, Ordering::SeqCst);

        let handler = self.handler();
        crate::ui::post_task(move || handler.on_vice_context_shutdown_complete());
        *self.event_handler.borrow_mut() = None;

        // Dropping the self reference must happen last, as it may destroy
        // the context. Defer it so that it does not happen while the plugin
        // is still executing vicePluginAPI_pumpEvents for this context.
        let self_ref = self.self_.borrow_mut().take();
        crate::ui::post_task(move || drop(self_ref));
    }

    fn create_window_impl(&self, msg: *mut *mut c_char, uri: Option<String>) -> u64 {
        let mut reason = "Unknown reason".to_string();
        let window = self
            .handler()
            .on_vice_context_create_window_request(&mut reason, uri);
        if window != 0 {
            if !self.open_windows.borrow_mut().insert(window) {
                crate::panic_log!("Requirement 'self->openWindows_.insert(window).second' failed");
            }
            window
        } else {
            if !msg.is_null() {
                unsafe { *msg = self.plugin.create_malloc_string(&reason) };
            }
            0
        }
    }
}

impl Drop for ViceContext {
    fn drop(&mut self) {
        let state = self.state.get();
        if !(state == State::Pending || state == State::ShutdownComplete) {
            crate::panic_log!("Requirement 'state_ == Pending || state_ == ShutdownComplete' failed");
        }
        unsafe { (self.plugin.api.destroy_context)(self.ctx) };
    }
}

/// Parses the newline-terminated quality label list of
/// `vicePluginAPI_windowQualitySelectorQuery`, panicking on invalid input like
/// the C++ version.
fn parse_quality_labels(label_str: &[u8]) -> Vec<String> {
    let mut labels = Vec::new();
    let mut i = 0;
    while i < label_str.len() {
        let mut j = i;
        loop {
            require!(j < label_str.len());
            if label_str[j] == b'\n' {
                break;
            }
            j += 1;
        }
        let len = j - i;
        require!((1..=3).contains(&len));
        let label = &label_str[i..j];
        for &c in label {
            require!((33..=126).contains(&c));
        }
        labels.push(String::from_utf8(label.to_vec()).unwrap());
        i = j + 1;
    }
    labels
}

// --- Plugin callbacks -------------------------------------------------------

unsafe extern "C" fn cb_event_notify(data: *mut c_void) {
    if data.is_null() {
        crate::panic_log!("Vice plugin sent unexpected NULL pointer as callback data");
    }
    let cd = unsafe { &*(data as *const CallbackData) };
    require!(!cd.flags.shutdown_complete.load(Ordering::SeqCst));
    if !cd.flags.pump_events_in_queue.swap(true, Ordering::SeqCst) {
        let ptr = SendPtr(data as *const CallbackData);
        crate::ui::post_task_from_any_thread(move || {
            let ptr = ptr;
            let cd = unsafe { &*ptr.0 };
            if let Some(ctx) = cd.ctx.upgrade() {
                ctx.pump_events();
            }
        });
    }
}

unsafe extern "C" fn cb_shutdown_complete(data: *mut c_void) {
    let ctx = unsafe { pumped_context(data) };
    ctx.shutdown_complete();
}

unsafe extern "C" fn cb_create_window(data: *mut c_void, msg: *mut *mut c_char) -> u64 {
    let ctx = unsafe { pumped_context(data) };
    ctx.create_window_impl(msg, None)
}

unsafe extern "C" fn cb_create_window_with_uri(
    data: *mut c_void,
    msg: *mut *mut c_char,
    uri: *const c_char,
) -> u64 {
    let ctx = unsafe { pumped_context(data) };
    let uri = crate::common::sanitize_utf8_string(&c_str_bytes(uri));
    ctx.create_window_impl(msg, Some(uri))
}

unsafe extern "C" fn cb_navigate_window_to_uri(data: *mut c_void, window: u64, uri: *const c_char) {
    let ctx = unsafe { pumped_context(data) };
    ctx.require_open(window);
    let uri = crate::common::sanitize_utf8_string(&c_str_bytes(uri));
    ctx.handler().on_vice_context_navigate_to_uri(window, uri);
}

unsafe extern "C" fn cb_get_window_title(data: *mut c_void, window: u64) -> *mut c_char {
    let ctx = unsafe { pumped_context(data) };
    ctx.require_open(window);
    let title = ctx.handler().on_vice_context_window_title_query(window);
    ctx.plugin.create_malloc_string(&title)
}

unsafe extern "C" fn cb_zoom_command(data: *mut c_void, window: u64, command: c_int) {
    let ctx = unsafe { pumped_context(data) };
    ctx.require_open(window);
    let handler = ctx.handler();
    match command {
        ZOOM_INPUT_COMMAND_ZOOM_IN => handler.on_vice_context_zoom_in(window),
        ZOOM_INPUT_COMMAND_ZOOM_OUT => handler.on_vice_context_zoom_out(window),
        ZOOM_INPUT_COMMAND_ZOOM_RESET => handler.on_vice_context_zoom_reset(window),
        _ => crate::panic_log!(
            "Requirement 'command == VICE_PLUGIN_API_ZOOM_INPUT_COMMAND_ZOOM_RESET' failed"
        ),
    }
}

unsafe extern "C" fn cb_close_window(data: *mut c_void, window: u64) {
    let ctx = unsafe { pumped_context(data) };
    if !ctx.open_windows.borrow_mut().remove(&window) {
        crate::panic_log!("Requirement 'self->openWindows_.erase(window)' failed");
    }
    ctx.upload_mode_windows.borrow_mut().remove(&window);
    ctx.handler().on_vice_context_close_window(window);
}

unsafe extern "C" fn cb_resize_window(data: *mut c_void, window: u64, width: usize, height: usize) {
    let ctx = unsafe { pumped_context(data) };
    ctx.require_open(window);
    require!(width != 0);
    require!(height != 0);
    ctx.handler().on_vice_context_resize_window(
        window,
        width.min(16384) as i32,
        height.min(16384) as i32,
    );
}

unsafe extern "C" fn cb_fetch_window_image(
    data: *mut c_void,
    window: u64,
    put_image_func: PutImageFunc,
    put_image_func_data: *mut c_void,
) {
    let ctx = unsafe { pumped_context(data) };
    ctx.require_open(window);
    let mut put_image_called = false;
    ctx.handler().on_vice_context_fetch_window_image(
        window,
        &mut |image: *const u8, width: usize, height: usize, pitch: usize| {
            require!(!put_image_called);
            put_image_called = true;
            require!(width != 0);
            require!(height != 0);
            unsafe { put_image_func(put_image_func_data, image, width, height, pitch) };
        },
    );
    require!(put_image_called);
}

macro_rules! forward_input_event {
    ($cb:ident, $method:ident, ($($arg:ident: $ty:ty),*)) => {
        unsafe extern "C" fn $cb(data: *mut c_void, window: u64 $(, $arg: $ty)*) {
            let ctx = unsafe { pumped_context(data) };
            ctx.require_open(window);
            ctx.handler().$method(window $(, $arg)*);
        }
    };
}

forward_input_event!(cb_mouse_down, on_vice_context_mouse_down, (x: c_int, y: c_int, button: c_int));
forward_input_event!(cb_mouse_up, on_vice_context_mouse_up, (x: c_int, y: c_int, button: c_int));
forward_input_event!(cb_mouse_move, on_vice_context_mouse_move, (x: c_int, y: c_int));
forward_input_event!(
    cb_mouse_double_click,
    on_vice_context_mouse_double_click,
    (x: c_int, y: c_int, button: c_int)
);
forward_input_event!(
    cb_mouse_wheel,
    on_vice_context_mouse_wheel,
    (x: c_int, y: c_int, dx: c_int, dy: c_int)
);
forward_input_event!(cb_mouse_leave, on_vice_context_mouse_leave, (x: c_int, y: c_int));
forward_input_event!(cb_key_down, on_vice_context_key_down, (key: c_int));
forward_input_event!(cb_key_up, on_vice_context_key_up, (key: c_int));
forward_input_event!(cb_lose_focus, on_vice_context_lose_focus, ());

unsafe extern "C" fn cb_navigate(data: *mut c_void, window: u64, direction: c_int) {
    let ctx = unsafe { pumped_context(data) };
    ctx.require_open(window);
    require!((-1..=1).contains(&direction));
    ctx.handler().on_vice_context_navigate(window, direction);
}

unsafe extern "C" fn cb_copy_to_clipboard(data: *mut c_void, text: *const c_char) {
    let ctx = unsafe { pumped_context(data) };
    require!(!text.is_null());
    let text = crate::common::sanitize_utf8_string(&c_str_bytes(text));
    ctx.handler().on_vice_context_copy_to_clipboard(text);
}

unsafe extern "C" fn cb_request_clipboard_content(data: *mut c_void) -> c_int {
    let ctx = unsafe { pumped_context(data) };
    ctx.handler().on_vice_context_request_clipboard_content();
    1
}

unsafe extern "C" fn cb_upload_file(
    data: *mut c_void,
    window: u64,
    name: *const c_char,
    path: *const c_char,
    cleanup: CleanupFunc,
    cleanup_data: *mut c_void,
) {
    let ctx = unsafe { pumped_context(data) };
    require!(!path.is_null());
    ctx.require_open(window);
    if !ctx.upload_mode_windows.borrow_mut().remove(&window) {
        crate::panic_log!("Requirement 'self->uploadModeWindows_.erase(window)' failed");
    }

    let cleanup_data = cleanup_data as usize;
    let cleanup_func: Box<dyn FnOnce()> =
        Box::new(move || unsafe { cleanup(cleanup_data as *mut c_void) });
    let idx = ctx.next_upload_idx.get();
    ctx.next_upload_idx.set(idx + 1);
    let file = ViceFileUpload::create(
        ctx.upload_temp_dir.clone(),
        idx,
        &String::from_utf8_lossy(&c_str_bytes(name)),
        &String::from_utf8_lossy(&c_str_bytes(path)),
        cleanup_func,
    );
    ctx.handler().on_vice_context_upload_file(window, file);
}

unsafe extern "C" fn cb_cancel_file_upload(data: *mut c_void, window: u64) {
    let ctx = unsafe { pumped_context(data) };
    ctx.require_open(window);
    if !ctx.upload_mode_windows.borrow_mut().remove(&window) {
        crate::panic_log!("Requirement 'self->uploadModeWindows_.erase(window)' failed");
    }
    ctx.handler().on_vice_context_cancel_file_upload(window);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames() {
        assert_eq!(sanitize_filename("a/b\\c"), "abc");
        assert_eq!(sanitize_filename(".."), ".._file.bin");
        assert_eq!(sanitize_filename("//"), "file.bin");
        assert_eq!(sanitize_filename(&"x".repeat(300)).len(), 200);
    }

    #[test]
    fn quality_labels() {
        assert_eq!(parse_quality_labels(b"10\n20\nPNG\n"), vec!["10", "20", "PNG"]);
        assert!(parse_quality_labels(b"").is_empty());
    }
}
