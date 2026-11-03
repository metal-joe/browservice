//! Vice plugin loading through the C API declared in `vice_plugin_api.h`.
//!
//! Port of the non-context part of `VicePlugin` in `src/vice.{hpp,cpp}`: the
//! library is loaded, the required API symbols are resolved and the API version
//! is checked; version/credits strings and option documentation can be
//! queried. Context handling (`ViceContext`) is not ported yet.

use std::ffi::{CStr, CString, c_char, c_int, c_void};

use libloading::Library;

pub const API_VERSION: u64 = 2_000_000;

pub const LOG_LEVEL_INFO: c_int = 100;
pub const LOG_LEVEL_WARNING: c_int = 200;
pub const LOG_LEVEL_ERROR: c_int = 300;

type LogCallback = unsafe extern "C" fn(*mut c_void, c_int, *const c_char, *const c_char);
type PanicCallback = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char);
type DestructorCallback = unsafe extern "C" fn(*mut c_void);
type OptionDocsCallback =
    unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char, *const c_char);

/// Function pointers to the global (non-context) API functions used so far.
struct ApiFuncs {
    is_api_version_supported: unsafe extern "C" fn(u64) -> c_int,
    create_version_string: unsafe extern "C" fn() -> *mut c_char,
    create_credits_string: unsafe extern "C" fn() -> *mut c_char,
    free: unsafe extern "C" fn(*mut c_void),
    get_option_docs: unsafe extern "C" fn(u64, OptionDocsCallback, *mut c_void),
    set_global_log_callback:
        unsafe extern "C" fn(u64, Option<LogCallback>, *mut c_void, Option<DestructorCallback>),
    set_global_panic_callback:
        unsafe extern "C" fn(u64, Option<PanicCallback>, *mut c_void, Option<DestructorCallback>),
    is_extension_supported: unsafe extern "C" fn(u64, *const c_char) -> c_int,
}

/// All API functions that a plugin must export (the same list as
/// FOREACH_REQUIRED_VICE_API_FUNC in `src/vice.cpp`). Every one of them is
/// resolved at load time so that incompatible plugins are rejected early, even
/// though the context functions are not used by the Rust port yet.
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

/// Optional extension functions, loaded only if the plugin reports support for
/// the extension.
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
        let api = unsafe {
            use std::mem::transmute as t;
            ApiFuncs {
                is_api_version_supported: t(syms["isAPIVersionSupported"]),
                create_version_string: t(syms["createVersionString"]),
                create_credits_string: t(syms["createCreditsString"]),
                free: t(syms["free"]),
                get_option_docs: t(syms["getOptionDocs"]),
                set_global_log_callback: t(syms["setGlobalLogCallback"]),
                set_global_panic_callback: t(syms["setGlobalPanicCallback"]),
                is_extension_supported: t(syms["isExtensionSupported"]),
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
                    load_sym(&lib, filename, func)?;
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
