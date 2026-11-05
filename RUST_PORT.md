# Rust port of Browservice: status

The C++ sources (`src/`, `viceplugins/`) are still in place and remain the
reference implementation. The Rust port lives in `rust/` with `Cargo.toml` at
the repository root (crate `browservice`, binary `browservice`).

**Status after phase 3:** the Rust binary runs the browser server. It starts
the vice plugin, creates windowless CEF browsers for the windows the plugin
asks for, serves their rendered pages to the plugin as window images and
forwards mouse and keyboard input to CEF. With the C++ `retrojsvice.so` plugin,
pages are shown and can be used through the plugin's HTTP interface (verified
end to end, see below). Since phase 3 the window has the same default layout
as the C++ build: the control bar (address field with security icon and
loading bar, soft navigation buttons, bookmark buttons, find button and find
bar, clipboard button, quality selector, download button with progress bars)
is drawn at the top unless `--show-control-bar=false`, text is rendered with
Pango/FreeType like the C++ build, and browser area error messages are drawn
in the window again. The window images of the Rust and C++ binaries were
compared pixel by pixel for the scenarios listed under "Phase 3" below and
are identical.

**Phase 4:** the dedicated Xvfb X server (`--use-dedicated-xvfb`, on by
default on Linux), the X error handlers, the PANIC path through CEF's
`LOG(FATAL)` and the Windows-specific code paths are ported. The Windows code
is only compile-checked (see "Windows status" below); it has never been run.

**Phase 5:** the Windows sandbox model of the C++ build (a DLL exporting
`RunConsoleMain`, loaded by CEF's `bootstrapc.exe`) is ported as the
workspace member `rust/windll`, and `tools/build_windows_rust.ps1` builds and
packages the Rust port on Windows. Both are compile-checked / reviewed only:
nothing was linked, packaged or run on Windows (see "Windows sandbox" and
"Windows build and packaging" below).

## Ported

Phase 1:

| Rust module | C++ origin | Status |
|---|---|---|
| `rust/src/config.rs` | `src/config.cpp`, `src/config_defs.hpp`, `src/config.hpp` | All options with the same names, value specs, defaults, parsers, validation, deprecated aliases (`--default-quality`, `--http-auth`, `--http-listen-addr`), `--vice-opt-*` pass-through, duplicate detection, error messages and exit codes. `--help` (including the help line wrapping algorithm and the vice plugin option list), `--version` and `--credits` print the same text as the C++ build. |
| `rust/src/credits.rs` | `src/credits.cpp` | Verbatim copy. |
| `rust/src/browser_font_render_mode.rs` | `src/browser_font_render_mode.cpp` | Complete; applied through the patch symbols before CEF is initialized. |
| `rust/src/chromium_patch.rs` | extern declarations in `src/clipboard.cpp`, `src/browser_font_render_mode.cpp` | `extern "C"` declarations of all symbols added by the Chromium patch, with safe wrappers; test-only stubs behind the `stub-patch` feature. |
| `rust/src/common.rs` | `src/common.{hpp,cpp}` | Logging/panic with the same message formats (locations are Rust source locations), signal handling, `sanitizeUTF8String`/`sanitizeUTF8StringToCodePoints`, random numbers. |

Phase 2:

| Rust module | C++ origin | Status |
|---|---|---|
| `rust/src/vice.rs` | `src/vice.{hpp,cpp}` | `VicePlugin` (phase 1) plus `ViceContext` and `ViceFileUpload`: `initContext` with the `--vice-opt-*` options, `start` with the complete `VicePluginAPI_Callbacks` table (window create/close/resize, image fetch, all mouse/key/focus events, navigation, clipboard, upload, cancel upload), the extensions `URINavigation` (create window with URI, navigate to URI), `WindowTitle`, `ZoomInput` and `PluginNavigationControlSupportQuery`, `eventNotify` from any thread with `pumpEvents` scheduling on the UI thread, the check that callbacks only come from `pumpEvents`, all program-to-plugin calls (popups, close, view changed, cursor, quality selector, clipboard button, clipboard content, file downloads, uploads), shutdown and `destroyContext`. The callback data is deliberately leaked so that late callbacks after shutdown are detected (like the C++ debug build). |
| `rust/src/server.rs` | `src/server.{hpp,cpp}` | Complete: window creation with `--window-limit`, soft navigation button decision, popups through the plugin, image fetches (1x1 fallback), input forwarding, navigation, clipboard copy/paste requests, uploads, titles, zoom, the shutdown sequence (close windows, wait for CEF cleanup, shut down the plugin, quit the message loop). |
| `rust/src/window.rs` | `src/window.{hpp,cpp}` | Complete apart from the control bar UI: windowless browser creation (`SetAsWindowless`, opaque white background) with the `--start-page` or the URI from the plugin, the window lifecycle (Open, Closed, CleanupComplete), all client handlers: life span (popups as new windows, deferred close), load (error page detection, refresh of focus/mouse state), display (address, title, cursor), request (`--block-file-scheme`, `--certificate-check-exceptions`, signed data URL error pages, security status updates from the IO thread), find, keyboard (Backspace/Shift+Backspace navigation), file dialog (uploads through the plugin), context menu (cleared), render and download handlers; zoom (`--initial-zoom`, steps of the sixth root of 2) with the 250 ms watchdog; global hotkeys (Ctrl+R/F5 refresh, Ctrl+K/J/M zoom). |
| `rust/src/browser_area.rs` | `src/browser_area.{hpp,cpp}` | Complete: the render handler copies dirty rectangles of the off-screen view and popup buffers (e.g. `<select>` dropdowns) into the window image with change detection; mouse (buttons, double click, wheel, enter/leave), key (RAWKEYDOWN + CHAR, KEYUP, modifier tracking, AltGr workaround, Ctrl+X/C/V through the focused frame) and focus events go to the CEF browser host. In error mode the view is white and the message is drawn at the top in dark red (since phase 3). |
| `rust/src/widget.rs` | `src/widget.{hpp,cpp}` | Complete widget base: viewport, render, focus and mouse-over routing, button/key state cleanup, cursors, global hotkeys. |
| `rust/src/root_widget.rs` | `src/root_widget.{hpp,cpp}` | Complete (since phase 3): control bar (27 pixels) above the browser area, or only the browser area with `--show-control-bar=false`. |
| `rust/src/image_slice.rs` | `src/image_slice.{hpp,cpp}`, `src/rect.hpp` | Complete (shared BGRx buffers, sub-slices, fill, put, clone). |
| `rust/src/key.rs` | `src/key.{hpp,cpp}`, `src/key_codes.hpp` | Complete. |
| `rust/src/download_manager.rs` | `src/download_manager.{hpp,cpp}` | Complete: pending queue, accept, progress, cancel, `CompletedDownload` that deletes the file (and releases the temporary directory) when the plugin calls the cleanup function from any thread. |
| `rust/src/scheme.rs` | `src/scheme.{hpp,cpp}` | `browservice://bookmarks/` and the 404 page (see the deviation below). |
| `rust/src/bookmarks.rs` | `src/bookmarks.{hpp,cpp}` | Complete: same file format and location (`~/.browservice/bookmarks`), atomic save, cache by URL, page with removal form. |
| `rust/src/temp_dir.rs` | `src/temp_dir.{hpp,cpp}` | Complete (Windows variant since phase 4, compile-checked only). |
| `rust/src/data_url.rs`, `rust/src/sha3.rs` | `src/data_url.{hpp,cpp}`, `src/tiny_sha3` | Complete; base64 and URI coding through the CEF functions, SHA3-256 ported from tiny_sha3. |
| `rust/src/clipboard.rs` | `src/clipboard.{hpp,cpp}` | Complete, through the patch symbols. |
| `rust/src/timeout.rs` | `src/timeout.{hpp,cpp}` | Complete. |
| `rust/src/ui.rs` | `postTask` in `src/common.cpp` | Posting closures to the CEF UI thread (UI-thread closures and `Send` closures from any thread). |
| `rust/src/globals.rs` | `src/globals.{hpp,cpp}` | Configuration and dot directory, readable from any thread. The text render context is not `Send`; it is a thread-local in `rust/src/text.rs`, created by `main` where the C++ `Globals` constructor creates it. |
| `rust/src/cef_app.rs`, `rust/src/main.rs` | `App` and `main` in `src/main.cpp` | Plugin load and context init before CEF, `CefInitialize`, scheme handler factory, request context (incognito or `--data-dir`), server creation in `OnContextInitialized`, 200 ms termination signal polling, shutdown before quitting the message loop, `CefShutdown`. |

Phase 3 (control bar and text rendering):

| Rust module | C++ origin | Status |
|---|---|---|
| `rust/src/text.rs` | `src/text.{hpp,cpp}` | Complete: `TextRenderContext` (PangoFT2 font map with the same fontconfig substitutions, 72 DPI, `Verdana 11`, `en-US`, LTR, `FREETYPE_PROPERTIES=truetype:interpreter-version=35` while the font map is created), `TextLayout` (extents, x/index conversion, visual cursor movement, graymap rendering with the 128 threshold) and `OverflowTextLayout`. The global context is created in `main` at the same point as the C++ `Globals` constructor. |
| `rust/src/image_slice.rs` | `ImageSlice::createImageFromStrings` | Added (icons). |
| `rust/src/button.rs` | `src/button.{hpp,cpp}` | Complete (download button). |
| `rust/src/menu_button.rs` | `src/menu_button.{hpp,cpp}` | Complete: passive/active icon, frame on hover/press, pressed event with the button identity, Enter/Esc forwarding. |
| `rust/src/text_field.rs` | `src/text_field.{hpp,cpp}` | Complete: caret with 500 ms blink, selection (inverted, same byte order quirk as C++, i.e. dark blue), mouse/Shift selection, Left/Right/Home/End, Backspace/Delete, typing, Ctrl+A/C/X/V through the clipboard module, Enter submit (remove caret / allow empty options), Up/Down/Esc/wheel forwarding, lost-focus-after-edit. |
| `rust/src/find_bar.rs` | `src/find_bar.{hpp,cpp}` | Complete: open/close/activate, incremental find, find next/previous buttons, Enter/Esc, not-found background. |
| `rust/src/quality_selector.rs` | `src/quality_selector.{hpp,cpp}` | Complete: label matching (exact, then ignoring spaces and case), up/down buttons with 500/50 ms mouse repeat, Up/Down keys, wheel, disabled arrows at the ends. |
| `rust/src/control_bar.rs` | `src/control_bar.{hpp,cpp}` | Complete: all icons (patterns and palettes copied from the C++ source), `Layout` with the same arithmetic, frame/separators/labels/address box/security icon rendering, loading bar animation (30 ms timeout, 5 s period), download button and progress bars, find bar, quality selector, clipboard button, soft navigation buttons, bookmark toggle (writes `~/.browservice/bookmarks`), and all events wired to the window as in C++ (address submit, go, bookmarks page, find, clipboard, back/forward/refresh/home, quality change, pending download accepted). |

Phase 4 (process environment, error handling, Windows):

| Rust module | C++ origin | Status |
|---|---|---|
| `rust/src/xvfb.rs` | `src/xvfb.{hpp,cpp}` | Complete (not on Windows): `.Xauthority` in a temporary directory with a dummy cookie for display 0 (added with `xauth -f <file> source -` through `/bin/sh`, as `popen` does), `Xvfb -displayfd <fd> -auth <file> -screen 0 640x480x24` started with fork/exec in its own process group, display number read from the pipe and parsed like `parseDisplay`, the real cookie added for that display, `DISPLAY`/`XAUTHORITY` set, and on shutdown SIGTERM, `waitpid` and removal of the file and directory. `main` creates it after the vice plugin context and drops it after `CefShutdown`, like the C++ `main`. Same log messages. |
| `rust/src/x_error.rs` | `XSetErrorHandler`/`XSetIOErrorHandler` in `src/main.cpp` | Complete: handlers that return 0, installed right before CEF initialization; `XSetErrorHandler` and `XSetIOErrorHandler` are declared by hand and libX11 is linked with `-lX11` from `rust/build.rs` (as in the Makefile). |
| `rust/src/common.rs` | `Panicker`, `enablePanicUsingCEFFatalError` in `src/common.{hpp,cpp}` | Complete: `PANIC @ LOCATION: MSG` on stderr, then `abort()` before and `cef_log(..., LOG_FATAL, "")` (i.e. `LOG(FATAL)`) after successful CEF initialization. In addition, a panic hook routes Rust panics (`unwrap`, `expect`, `assert!`) to the same output and termination path. |
| `rust/src/cef_app.rs` | `main` in `src/main.cpp` | `CefInitialize` failure with `CEF_RESULT_CODE_NORMAL_EXIT_PROCESS_NOTIFIED` panics with the C++ text "Another Browservice instance is running. Close that instance or specify different data directory using --data-dir for each instance to support concurrent instances." (other failures: "Initializing CEF failed"); `enable_panic_using_cef_fatal_error` right after successful initialization. |
| several modules, `#[cfg(windows)]` | `#ifdef _WIN32` code in `src/*.cpp` | See "Windows status". |

Phase 5 (Windows sandbox and packaging):

| Rust module / file | C++ origin | Status |
|---|---|---|
| `rust/windll/lib.rs` (crate `browservice-windll`, cdylib) | `RunConsoleMain` in `src/main.cpp`, `add_library(browservice SHARED ...)` in `winbuild/CMakeLists.txt` | Exports `RunConsoleMain` for `bootstrapc.exe` and passes the sandbox information on; compile-checked only. |
| `rust/src/app_main.rs` | `main`/`RunConsoleMain` body in `src/main.cpp` | The program, moved from `rust/src/main.rs`; takes the sandbox pointer. |
| `tools/build_windows_rust.ps1`, `tools/windows_rust/retrojsvice/CMakeLists.txt` | `tools/build_windows.py`, `winbuild/CMakeLists.txt` | Written, not run (no Windows, no PowerShell here). |

Text rendering library choice: the Pango, PangoFT2, GObject and fontconfig
functions are declared by hand in `rust/src/text.rs` (module `ffi`, about 25
functions) and linked through `pkg-config --libs pangoft2 fontconfig` in
`rust/build.rs`, i.e. the same system libraries as the C++ build
(`pkg-config --libs pangoft2` in the Makefile). The gtk-rs `pango` crate
does not cover the PangoFT2 backend (there is no maintained `pangoft2`
crate), and rendering through `pangocairo` would rasterize differently from
`pango_ft2_render_layout`. Build requirement: the `pangoft2` development
files (`libpango1.0-dev` on Debian/Ubuntu) and `pkg-config`.

### Design notes

- All browser state lives on the CEF UI thread (`Rc`, `Cell`, `RefCell`).
  No `RefCell` borrow is held across calls into CEF, the plugin or other
  objects, because those calls can re-enter (for example `WasResized` calling
  `GetViewRect`).
- The CEF handler objects of a window only store the window handle and look the
  window up in a UI-thread registry. This keeps `Rc` state out of objects that
  CEF may reference or release on other threads (`GetResourceRequestHandler`
  runs on the IO thread and only posts a task).
- The certificate and SSL content status are bit masks, but the Rust bindings
  model them as enums, so the raw C functions are called with integer return
  types.

### Deliberate deviations from the C++ code

- The `browservice` scheme handler factory is registered with the global
  request context (like the C++ code) **and** with the request context of the
  windows. With CEF 156, the C++ build fails to open `browservice://bookmarks/`
  with `ERR_UNKNOWN_URL_SCHEME`, because the windows use a context created with
  `CefRequestContext::CreateContext` that does not see the global registration
  (verified with the smoke test against the C++ binary, see below).
- The render handler never reads outside the paint buffer if CEF paints with a
  size that differs from the current viewport.

## Not ported yet

- Nothing functional is missing, but the Windows build (sandbox DLL,
  packaging) has not been built or run on Windows yet (see below).
- The vice plugin `viceplugins/retrojsvice` stays in C++ and is loaded as a
  shared library through `vice_plugin_api.h`.

## Windows status

The `#ifdef _WIN32` code paths of the C++ sources are ported with
`#[cfg(windows)]`, using the `windows-sys` crate (Windows-only dependency):

| C++ (`#ifdef _WIN32`) | Rust |
|---|---|
| `SetConsoleCtrlHandler(handleTermSignal)` instead of `signal(SIGINT/SIGTERM)` (`src/main.cpp`) | `common::register_term_signal_handler`, handler logs `Got control signal N, initiating shutdown` |
| `GetCommandLineW` + `CommandLineToArgvW`, wide arguments converted to UTF-8 for `Config::read` (`RunConsoleMain`, `src/config.cpp`) | `common::command_line_args` (`ERROR: Reading command line failed`, exit 1, on failure) |
| `%APPDATA%\Browservice` from `SHGetKnownFolderPath(FOLDERID_RoamingAppData)`, `CreateDirectory` (`src/globals.cpp`) | `globals::dot_dir_path`, `globals::ensure_dir_exists` |
| `GetTempPathW` + `browservicetmp_` + 16 random characters, `CreateDirectoryW`/`RemoveDirectoryW` (`src/temp_dir.cpp`) | `temp_dir::TempDir` |
| `PathSep` `\` | `common::PATH_SEP` (bookmark file, CEF cache directory, download files, upload links) |
| Upload: file copied instead of symlinked (`src/vice.cpp`) | `vice::ViceFileUpload` |
| `MoveFileW`/`ReplaceFileW` for the bookmark file, temporary file kept on failure (`src/bookmarks.cpp`) | `bookmarks::replace_file` |
| `DeleteFileW` messages "Deleting file ... failed" (`src/download_manager.cpp`) | `download_manager` |
| `_putenv_s("FREETYPE_PROPERTIES", ...)` (`src/text.cpp`) | `text::FreeType2SetEnv` (C runtime `getenv`/`_putenv_s`, because FreeType reads the C runtime environment) |
| No `native_key_code`; CHAR event only for keys with a character, with the character as `windows_key_code` (`src/browser_area.cpp`) | `browser_area` |
| No ANGLE/SwiftShader switches, no `--use-dedicated-xvfb`, `retrojsvice.dll` default (`src/main.cpp`, `src/config_defs.hpp`) | already in phases 1 and 2 |
| `random_device` | `BCryptGenRandom` (system preferred RNG) |
| `LoadLibraryA`/`GetProcAddress`/`FreeLibrary` (`src/vice.cpp`) | `libloading` (`LoadLibraryExW`) |

What was checked: compilation only. `cef-dll-sys` supports only the MSVC
Windows targets, and its build script needs a Windows CEF distribution; the
check therefore uses the crate's `dox` feature (no build script, bindings
only). Since phase 5 the checks include the bootstrap DLL (`--workspace`):

```sh
rustup target add x86_64-pc-windows-gnu x86_64-pc-windows-msvc
cargo check --release --workspace --features stub-patch,cef/dox --all-targets --target x86_64-pc-windows-msvc
cargo check --release --workspace --features stub-patch,cef/dox --all-targets --target x86_64-pc-windows-gnu
cargo check --release --workspace --features cef/dox --all-targets --target x86_64-pc-windows-msvc
# all: Finished, no warnings, no errors
```

(Without `cef/dox`, `cargo check --target x86_64-pc-windows-gnu` fails in
the `cef-dll-sys` build script with `Unsupported target triplet:
x86_64-pc-windows-gnu`.) Nothing was linked or run on Windows: the Windows
runtime behavior is **untested**. A real Windows build needs MSVC, the
Windows CEF 156.0.15 distribution in `CEF_PATH` and Pango/fontconfig
through `pkg-config` (e.g. from vcpkg, as `tools/build_windows.py` sets up
for the C++ build; `rust/build.rs` calls `pkg-config` only when building on
a Windows host).

## Windows sandbox

### What the C++ build does

`src/main.cpp` defines, for `_WIN32`, `RunConsoleMain(int, char*[], void*
sandboxInfo, cef_version_info_t*)` instead of `main`, and passes
`sandboxInfo` to `CefExecuteProcess` and `CefInitialize`.
`winbuild/CMakeLists.txt` builds Browservice as a DLL (`add_library(...
SHARED)`), copies `bootstrapc.exe` from the CEF distribution as
`browservice.exe` and embeds `browservice.exe.manifest` +
`compatibility.manifest` into it with `mt.exe`. `bootstrapc.exe` (console
subsystem) loads `browservice.dll` from its directory, initializes the
sandbox and calls `RunConsoleMain`, in the browser process and in every CEF
subprocess.

### What CEF 156 and the cef crate support

- CEF 156 has no other way to get a sandbox on Windows: the Windows
  distribution (`cef_binary_156.0.15+gcf671c0+chromium-156.0.8078.80_windows64.tar.bz2`,
  downloaded and listed for this check) contains `bootstrap.exe`,
  `bootstrapc.exe` and `include/cef_sandbox_win.h`, but no
  `cef_sandbox.lib`; `cef_sandbox_info_create` is not exported by
  `libcef.dll` (it is not in the cef-dll-sys bindings either). In CEF's
  own CMake files, `USE_SANDBOX` on Windows means "client DLL loaded by
  bootstrap.exe" (it defines `CEF_USE_BOOTSTRAP`), and the sample
  `tests/cefsimple` is then built as a DLL exporting `RunWinMain`, next to a
  renamed `bootstrap.exe`. So option (b) of the task ("plain exe with the
  sandbox enabled") does not exist for CEF 156.
- The cef crate 156.3.0 supports exactly this model: its `sandbox` feature
  builds `libcef_dll_wrapper` with `USE_SANDBOX=ON`, its `build_util::win`
  bundler builds the crate's `cdylib` and copies `bootstrap.exe` as
  `<name>.exe`, and the upstream example `examples/cefsimple/src/win.rs`
  (cef-rs repository) is a cdylib exporting `RunWinMain` with
  `#[unsafe(no_mangle)]`. The bindings of `cef::execute_process` and
  `cef::initialize` take the `windows_sandbox_info` pointer.

### Chosen model: DLL + `bootstrapc.exe`, as in the C++ build

- `rust/windll` (package `browservice-windll`, workspace member,
  `crate-type = ["cdylib"]`, output `browservice_windll.dll`) exports
  `RunConsoleMain` with the signature of `cef_sandbox_win.h` and calls
  `browservice::app_main::run_main(sandbox_info)`. Like the C++ function it
  ignores `argc`/`argv`/`version_info`: the command line comes from
  `GetCommandLineW`/`CommandLineToArgvW` and the module handle from
  `GetModuleHandle(nullptr)` (`cef::args::Args::new`). On Windows the
  package enables the cef crate's `sandbox` feature.
- `rust/src/app_main.rs` holds the program (moved from `main.rs`);
  `cef_app::execute_subprocess` and `cef_app::run_browser_process` pass the
  sandbox pointer to `CefExecuteProcess`/`CefInitialize`.
- `bootstrapc.exe` (not `bootstrap.exe`) because Browservice is a console
  program, as in the C++ build; the DLL is copied as `browservice.dll` next
  to the renamed `browservice.exe` by the packaging script.
- The root package stays a plain executable (`rust/src/main.rs`, null
  sandbox pointer). On Linux nothing changes. On Windows the plain
  executable is only a development convenience and is not packaged; it runs
  CEF without the sandbox information.

### Verified here (Linux host)

```sh
cargo check --release --workspace --features stub-patch,cef/dox --target x86_64-pc-windows-msvc --all-targets
#     Finished `release` profile [optimized] target(s) in 12.61s
cargo check --release --workspace --features stub-patch,cef/dox --target x86_64-pc-windows-gnu --all-targets
#     Finished `release` profile [optimized] target(s) in 13.07s
cargo tree -p browservice-windll --target x86_64-pc-windows-msvc -e features -i cef-dll-sys --features cef/dox
# shows cef-dll-sys feature "sandbox" <- cef feature "sandbox" <- browservice-windll
```

The link step of the DLL cannot be done here (no `link.exe` and no Windows
SDK import libraries). Running it with `lld-link` and `-C save-temps`
shows the export definition file rustc generates for the DLL, and the
object file defines the unmangled symbol:

```sh
cargo rustc --release -p browservice-windll --features cef/dox \
    --target x86_64-pc-windows-msvc -- -C linker=lld-link -C save-temps --print link-args
cat .../deps/rustc*/lib.def
# LIBRARY
# EXPORTS
#   RunConsoleMain
llvm-nm .../deps/browservice_windll*.o | grep RunConsoleMain
# 00000000 T RunConsoleMain
# (the link then fails: lld-link: error: could not open 'kernel32.lib')
```

On Linux, `cargo build --release --features stub-patch` (no warnings),
`cargo test --release --features stub-patch` (52 passed, 0 failed, 1
ignored), `./browservice --version`/`--bogus` (same output and exit codes as
before) and the smoke test with the dedicated Xvfb (`ALL CHECKS PASSED`,
exit code 0) still pass after moving the program into `app_main`.

### Not verified

Everything at run time on Windows: that `bootstrapc.exe` loads the Rust DLL
and calls `RunConsoleMain`, that the sandbox is active for the renderer and
GPU processes, and that the Rust standard library behaves in a DLL entered
from `bootstrapc.exe` (expected, but untested). Code signing was not
looked into: like the C++ distribution, the packaged files are unsigned.

## Windows build and packaging

`tools/build_windows_rust.ps1` is the Rust port's counterpart of
`tools/build_windows.py` + `winbuild/CMakeLists.txt`. It runs on Windows
(Windows PowerShell 5.1 or PowerShell 7) and skips steps whose result is
already up to date, so it can be re-run:

1. MSVC environment: imports `vcvars64.bat` (found with `vswhere`) unless
   `cl.exe` is already in `PATH`; checks for `cargo`, `git`, `tar`, `cmake`,
   `ninja`, `mt`, `icacls` and Python 3.
2. CEF 156.0.15+gcf671c0+chromium-156.0.8078.80, Windows 64-bit:
   - `-CefTarball <patched_cef_..._windows64.tar.bz2>`: the patched CEF
     (Browservice Chromium patch), required for a production build, as for
     `tools/build_windows.py`;
   - otherwise the stock standard distribution
     `cef_binary_156.0.15+gcf671c0+chromium-156.0.8078.80_windows64.tar.bz2`
     from `https://cef-builds.spotifycdn.com/` (file name and SHA-1
     `dec540c7236c7c98c033f78618c010e3ae322f82` taken from the `windows64`
     entry of `https://cef-builds.spotifycdn.com/index.json`; the download
     was checked against this SHA-1 here). Stock CEF lacks the patch
     symbols, so this is allowed only together with `-StubPatch`.
   The tarball is extracted with `tar.exe` to `<BuildDir>\cef`, the version
   in `include\cef_version.h` must match the cef crate, and the flat layout
   of cef-dll-sys is created in `<BuildDir>\cef_rust` (`Release\*`,
   `Resources\*`, `include`, `cmake`, `libcef_dll`, `CMakeLists.txt`,
   `archive.json`), which becomes `CEF_PATH`.
3. vcpkg (cloned into `<BuildDir>\vcpkg` or `-VcpkgRoot`), triplet
   `x64-windows`: `pango fontconfig pkgconf` (Rust text renderer, through
   `pkg-config`; `rust/build.rs` now honours `PKG_CONFIG`, set to vcpkg's
   `pkgconf.exe`) and `openssl poco[netssl] libjpeg-turbo` (retrojsvice).
4. `cargo build --release -p browservice-windll` (`--features stub-patch`
   with `-StubPatch`), i.e. `target\release\browservice_windll.dll`. The
   cef-dll-sys build script builds `libcef_dll_wrapper` with CMake + Ninja +
   MSVC.
5. `retrojsvice.dll` (C++, unchanged sources): `gen_html_cpp.py` output is
   written to `<BuildDir>\retrojsvice\gen\html.cpp` (not into
   `viceplugins/`), then the standalone CMake project
   `tools/windows_rust/retrojsvice` (same target as in
   `winbuild/CMakeLists.txt`) is built with the Visual Studio generator and
   the vcpkg MSBuild integration (`Directory.Build.props/.targets`, as in
   `tools/build_windows.py`).
6. `<OutputDir>\<DistName>` (default `release\browservice-<git
   describe>-windows64`): `bootstrapc.exe` as `browservice.exe` with the two
   manifests of `winbuild/` embedded by `mt.exe`, the Rust DLL as
   `browservice.dll`, `retrojsvice.dll`, all DLLs of the vcpkg `bin`
   directory (pango/glib/fontconfig/freetype/harfbuzz, Poco, OpenSSL,
   libjpeg-turbo and their dependencies; the C++ build gets the same through
   vcpkg's app-local copying), the CEF binary files (`chrome_elf.dll`,
   `d3dcompiler_47.dll`, `dxcompiler.dll`, `libcef.dll`,
   `v8_context_snapshot.bin`, `vk_swiftshader.dll`, `vk_swiftshader_icd.json`,
   `vulkan-1.dll`) and resources (`chrome_100_percent.pak`,
   `chrome_200_percent.pak`, `resources.pak`, `icudtl.dat`, `locales\`) as
   listed in the CEF 156 `cmake/cef_variables.cmake`, and `msvcp140.dll`,
   `vcruntime140.dll`, `vcruntime140_1.dll` from the MSVC redistributable
   directory. The LPAC ACL that CEF's `SET_LPAC_ACLS` sets for sandboxed
   samples is granted on the folder (`icacls ... /grant
   *S-1-15-2-2:(OI)(CI)(RX)`); the C++ build does not do this, and ACLs are
   not stored in the zip.
7. `<OutputDir>\<DistName>.zip` (folder at the root of the zip, like
   `tools/build_windows.py`). With `-StubPatch` the folder name ends in
   `-stubpatch-TEST-ONLY` and no zip is created, so the stub never ends up in
   a release package.

### Commands to run on Windows

Requirements: Visual Studio 2022 (or newer) with the workload "Desktop
development with C++" (MSVC x64, Windows SDK, "C++ CMake tools for
Windows", which provide `cmake` and `ninja`), Rust via rustup with the
`stable-x86_64-pc-windows-msvc` toolchain, Git, Python 3. Use a short
checkout path without spaces (e.g. `C:\bs`); the script warns above 40
characters because of the 260 character path limit in the CEF wrapper build.

```powershell
git clone -b <branch of the Rust port> <repository URL> C:\bs
cd C:\bs

# Production build with the patched CEF (Browservice Chromium patch):
powershell -NoProfile -ExecutionPolicy Bypass -File tools\build_windows_rust.ps1 `
    -CefTarball C:\path\to\patched_cef_windows64.tar.bz2
# -> release\browservice-<version>-windows64\ and release\browservice-<version>-windows64.zip

# Test build against the stock CEF (downloads it, about 370 MB):
powershell -NoProfile -ExecutionPolicy Bypass -File tools\build_windows_rust.ps1 -StubPatch
# -> release\browservice-<version>-windows64-stubpatch-TEST-ONLY\ (no zip)

# Try it:
cd release\browservice-<version>-windows64
.\browservice.exe --version
.\browservice.exe --vice-opt-http-listen-addr=127.0.0.1:8080
```

Other options: `-BuildDir` (default `build_windows_rust\`, ignored by git),
`-VcpkgRoot` (existing vcpkg), `-OutputDir` (default `release\`),
`-DistName`, `-CMakeGenerator` (default from the Visual Studio version).
A patched CEF 156 Windows tarball is not part of this repository; it is
built with `tools/build_patched_cef.py` (see `winbuild/README.md`) unless a
matching one is published on the Browservice releases page.

### Verified here and not

- PowerShell is not installed on this Linux host (`pwsh` and `powershell`
  not found; running a downloaded PowerShell was not permitted), so the
  script was **not parsed or run**. It was reviewed by hand for Windows
  PowerShell 5.1 pitfalls (stderr redirection with `ErrorActionPreference
  Stop`, single-element arrays, UTF-8 without BOM for `archive.json`), and
  a rough delimiter balance check (brackets, braces, strings, comments)
  passes.
- The CEF file name and SHA-1 were verified against the CEF build index,
  and the downloaded archive was listed: `Release\bootstrapc.exe`,
  `Release\libcef.lib`, every file of the copy lists above, and
  `include\cef_sandbox_win.h` are present.
- `tools/windows_rust/retrojsvice/CMakeLists.txt` was checked on Linux with
  the Visual Studio generator check removed and Linux link libraries
  added: `retrojsvice` builds from `viceplugins/retrojsvice/src` plus an
  `html.cpp` generated outside `viceplugins/` (the include of
  `../src/html.hpp` resolves through the include directory), and the
  generated `html.cpp` is byte-identical to the output of
  `./gen_html_cpp.py`.
- Not verified: the vcpkg installation, the MSVC link of the Rust DLL
  against vcpkg's Pango import libraries, `mt.exe`, `icacls`, the zip,
  and the resulting distribution at run time.

## Building

Requirements: Rust stable (tested with cargo 1.97.0) and an extracted CEF
`156.0.15+gcf671c0+chromium-156.0.8078.80` binary distribution. The crates
`cef` / `cef-dll-sys` `156.3.0+156.0.15` match this CEF version. On Linux they
do not build the C++ wrapper library; they link `libcef.so` directly.

`cef-dll-sys` expects a flat layout (runtime files, `include/` and an
`archive.json` in one directory). Convert a standard distribution with:

```sh
tools/prepare_cef_for_rust.sh /path/to/cef_binary_..._linux64 /path/to/cef-rust
export CEF_PATH=/path/to/cef-rust
```

(If `CEF_PATH` is unset or not in that layout, `cef-dll-sys` tries to download
CEF itself.)

Production build, against the **patched** CEF (built by the C++ tooling
with the Browservice Chromium patch, which exports the
`cef_chromiumBrowservice*` symbols):

```sh
cargo build --release
```

The text renderer needs `pkg-config` and the PangoFT2 development files
(`pangoft2` and `fontconfig` pkg-config packages), as for the C++ build.
On Linux the binary also links libX11 (`libx11-dev`), and the default
`--use-dedicated-xvfb=yes` needs `Xvfb` and `xauth` in `PATH` at run time.

Development/test build against **stock** CEF:

```sh
cargo build --release --features stub-patch
cargo test --release --features stub-patch
```

The `cef-dll-sys` build script copies `libcef.so`, the `.pak`/`.dat`/`.bin`
files and `locales/` into `target/release/`. The binary is linked with
`RPATH=$ORIGIN` (like the C++ build), so `libcef.so` and `retrojsvice.so` are
found next to the binary. For testing, copy a built `retrojsvice.so` into
`target/release/`. Nothing under `target/` is committed (`/target/` is in
`.gitignore`).

## Test-only stub of the Chromium patch symbols

Stock CEF 156 does not export the functions added by the Browservice Chromium
patch (`cef_chromiumBrowserviceClipboardPaste`,
`cef_chromiumBrowserviceClipboardFreePasteResult`,
`cef_chromiumBrowserviceClipboardCopy`,
`cef_chromiumBrowserviceFontRenderParamsSet{AntialiasingEnabled,AutohinterEnabled,Hinting,SubpixelPositioningEnabled,SubpixelRendering,UseBitmapsEnabled}`).
Without them, linking against stock CEF fails with undefined symbols (verified,
see below).

The module `chromium_patch::test_stub_patch` (file
`rust/src/chromium_patch.rs`) defines no-op stubs (the clipboard stub keeps
text in process memory). It is compiled **only** with the cargo feature
`stub-patch`, which is not a default feature. Do not use it for distributed or
production builds: font render mode settings and the Chromium clipboard do
nothing with it. A binary built with the stub prints a warning at startup in
normal mode.

## Verification results

Phase 5: see "Windows sandbox" and "Windows build and packaging" above.

### Phase 4: dedicated Xvfb, X errors, PANIC path, Windows

Same environment as before (stock CEF 156 prepared with
`tools/prepare_cef_for_rust.sh` into a new directory, `retrojsvice.so` from
the C++ build in `target/release/`). Xvfb 21.1.12 and xauth from the distribution.

```sh
cargo build --release --features stub-patch          # no warnings
cargo test --release --features stub-patch           # 52 passed, 0 failed, 1 ignored
xvfb-run -a cargo test --release --features stub-patch   # 52 passed (X error test active)
```

1. Dedicated Xvfb: started with `DISPLAY` and `XAUTHORITY` removed from the
   environment and without `xvfb-run`:

   ```sh
   cd target/release
   env -u DISPLAY -u XAUTHORITY ./browservice --chromium-args=no-sandbox \
       --vice-opt-http-listen-addr=127.0.0.1:18081 &
   curl -s -o /dev/null -w "status=%{http_code} type=%{content_type} size=%{size_download}\n" \
       http://127.0.0.1:18081/
   pgrep -a -P $! Xvfb
   kill -TERM $!; wait $!; pgrep -a Xvfb
   ```

   Output: `status=200 type=text/html; charset=UTF-8 size=218`; the child
   `Xvfb -displayfd 4 -auth /tmp/browservicetmp_q9rXuq/.Xauthority -screen 0
   640x480x24` in its own process group (PGID = its PID); after SIGTERM the
   browservice exit code is 0, the Xvfb PID no longer exists and `pgrep -a
   Xvfb` finds nothing. Log:

   ```
   INFO @ rust/src/xvfb.rs:77 -- Starting Xvfb X server as child process
   INFO @ rust/src/xvfb.rs:171 -- Xvfb X server :0 successfully started
   ...
   INFO @ rust/src/main.rs:114 -- CEF shut down cleanly
   INFO @ rust/src/xvfb.rs:211 -- Sending SIGTERM to the Xvfb X server child process to shut it down
   INFO @ rust/src/xvfb.rs:216 -- Waiting for Xvfb child process to shut down
   INFO @ rust/src/xvfb.rs:228 -- Successfully shut down Xvfb X server
   ```

   The smoke test (`tools/rust_port_smoke_test.py`) passes with the
   dedicated Xvfb (`ALL CHECKS PASSED`, repeated 6 times) and with
   `xvfb-run -a ... --use-dedicated-xvfb=no` (repeated 6 times). Note: the
   test has to signal only the main browservice process; `pkill -x
   browservice` also kills the CEF subprocesses, after which CEF ends the
   main process with `FATAL: GPU process isn't usable. Goodbye.` (exit 133)
   and the dedicated Xvfb is left running, as it would be for the C++
   build, which also does not clean up on a crash.

   The smoke test's download check was made robust in phase 4: it waited
   for a black pixel count that the "Quality" label and selector text reach
   on their own, so it sometimes looked for the download button before it
   was drawn (`ValueError: max() iterable argument is empty`); it now also
   waits for the button frame.

2. X errors: the unit test `x_error::tests::protocol_error_is_ignored`
   (runs when `DISPLAY` is set) maps a non-existent window, which makes the
   X server return a BadWindow error, and passes. The same request in a
   program with the default Xlib handler prints `X Error of failed request:
   BadWindow (invalid Window parameter)` and exits with code 1 (checked
   with a small ctypes script under `xvfb-run`).

3. PANIC path:

   - After CEF initialization, with the test plugin shim
     `tools/rust_port_panic_test_plugin.c` (build command in the file),
     which calls the `closeWindow` callback from another thread outside
     `vicePluginAPI_pumpEvents` 5 seconds after start:

     ```sh
     xvfb-run -a ./browservice --use-dedicated-xvfb=no --chromium-args=no-sandbox \
         --vice-plugin=panicvice.so --vice-opt-http-listen-addr=127.0.0.1:18081
     ```

     Rust (exit code 133, SIGTRAP from CEF's fatal error handling):

     ```
     PANIC @ rust/src/vice.rs:702: Vice plugin unexpectedly called a callback in a thread that is not currently executing vicePluginAPI_pumpEvents
     [4385:4458:1105/063735.149335:FATAL:rust/src/common.rs:32]
     Trace/breakpoint trap
     ```

     C++ binary, same shim (exit code 133):

     ```
     PANIC @ src/vice.cpp:674: Vice plugin unexpectedly called a callback in a thread that is not currently executing vicePluginAPI_pumpEvents
     [4507:4580:1105/063806.832022:FATAL:src/common.cpp:120]
     Trace/breakpoint trap
     ```

   - Before CEF initialization panics abort (exit code 134). The unit test
     `common::tests::panic_hook_reports_like_panic_macro` runs a child that
     calls `expect` on `None` with the panic hook installed and checks for
     `PANIC @ rust/src/common.rs:...: value missing` and SIGABRT.

   - Second instance with the same root cache path (first instance started
     with default options, second one with another listen address):

     ```
     PANIC @ rust/src/cef_app.rs:238: Another Browservice instance is running. Close that instance or specify different data directory using --data-dir for each instance to support concurrent instances.
     Aborted
     ```

     Exit code 134; the C++ binary prints the same text (`PANIC @
     src/main.cpp:297: ...`) with exit code 134.

4. Windows: compile-checked only, see "Windows status" above.

Not verified in phase 4: anything on Windows at run time, and the patched
CEF (all runs used stock CEF with the `stub-patch` feature).

### Phase 3: control bar and text rendering

Same environment as phase 2. Commands (scratch paths shortened):

```sh
export CEF_PATH=/path/to/cef156rust     # stock CEF 156 prepared as above
cargo build --release --features stub-patch   # no warnings
cargo test --release --features stub-patch    # 46 passed, 0 failed, 1 ignored
cp /path/to/full156/viceplugins/retrojsvice/release/lib/retrojsvice.so target/release/
cd target/release
xvfb-run -a ./browservice --use-dedicated-xvfb=no --chromium-args=no-sandbox \
    --vice-opt-http-listen-addr=127.0.0.1:18081
curl -s -o /dev/null -w "status=%{http_code} type=%{content_type} size=%{size_download}\n" \
    http://127.0.0.1:18081/
# status=200 type=text/html; charset=UTF-8 size=218
```

1. Text rendering: `tools/text_dump.cpp` repeats the Pango/FreeType calls of
   `src/text.cpp` in a standalone program; the ignored unit test
   `text::dump::dump` prints the same format (width, height, cursor x
   positions of every character boundary, thresholded graymap) for the Rust
   renderer:

   ```sh
   g++ -std=c++17 -O2 tools/text_dump.cpp -o text_dump `pkg-config --cflags --libs pangoft2`
   ./text_dump in.txt > cpp.txt
   TEXT_DUMP_IN=in.txt TEXT_DUMP_OUT=rs.txt \
       cargo test --release --features stub-patch text::dump -- --ignored
   cmp cpp.txt rs.txt
   ```

   With nine lines (the control bar labels, "Download (1)", two error
   messages, a URL and a mixed character line) the outputs are identical (144
   lines; e.g. `text "Address" width 44 height 14`, `cursor 0 8 15 22 26 32
   38 44`). (On the test machine fontconfig resolves `Verdana` to the
   installed fallback font, the same for both.)

2. Smoke test (now with the control bar shown by default; it checks the
   control bar frame, label and address text, shifts all page coordinates by
   the 27 pixel bar, and accepts a download through the control bar):

   ```sh
   PAGE=$(python3 tools/rust_port_smoke_test.py --print-start-page)
   xvfb-run -a ./target/release/browservice --use-dedicated-xvfb=no \
       --chromium-args=no-sandbox --vice-opt-http-listen-addr=127.0.0.1:18081 \
       --start-page="$PAGE"
   python3 tools/rust_port_smoke_test.py http://127.0.0.1:18081
   ```

   Output with the Rust binary:

   ```
   GET / -> 200 text/html; charset=UTF-8 (218 bytes)
   main page: prefix=/3/aCbn0DI4jrxuYZySBforfPNtrH55XZV6 mainIdx=1 cipher length=2127
   OK: control bar drawn above the page (image/png, 8010 bytes, 641x481, pixel(100,100)=(0, 0, 255), pixel(500,300)=(255, 0, 0))
   OK: rendered test page (blue square on red) (image/png, 8010 bytes, 641x481, pixel(100,100)=(0, 0, 255), pixel(500,300)=(255, 0, 0))
   OK: mouse click handled (background green) (image/png, 8010 bytes, 641x481, pixel(100,100)=(0, 0, 255), pixel(500,300)=(0, 255, 0))
   OK: key presses handled (background yellow) (image/png, 9129 bytes, 641x482, pixel(100,100)=(0, 0, 255), pixel(500,300)=(255, 255, 0))
   OK: file upload delivered to the page (background magenta) (image/png, 8107 bytes, 640x481, pixel(100,100)=(0, 0, 255), pixel(500,300)=(255, 0, 255))
   GET goto/browservice://bookmarks/ -> 200
   main page: prefix=/3/aCbn0DI4jrxuYZySBforfPNtrH55XZV6 mainIdx=2 cipher length=2234
   OK: browservice://bookmarks/ page (white page with heading text) (image/png, 12729 bytes, 640x481, pixel(100,100)=(255, 255, 255), pixel(500,300)=(255, 255, 255))
   OK: clipboard set/get round trip through the server
   GET goto/http://127.0.0.1:18082/smoke.bin -> 200
   main page: prefix=/3/aCbn0DI4jrxuYZySBforfPNtrH55XZV6 mainIdx=3 cipher length=2430
   OK: download button shown in the control bar (image/png, 12741 bytes, 640x481, pixel(100,100)=(255, 255, 255), pixel(500,300)=(255, 255, 255))
   OK: download accepted from the control bar and served by the plugin (/3/aCbn0DI4jrxuYZySBforfPNtrH55XZV6/download/1/smoke.bin, 3000 bytes)
   ALL CHECKS PASSED
   ```

   The C++ binary (default options, same command) passes the control bar,
   page, click, key and upload checks with PNG images of the same sizes
   (8010, 8010, 9129, 8107 bytes); the images of the click, key and upload
   steps are pixel-identical to the Rust ones, control bar included (the
   first image differed only in the page area because the C++ window had not
   painted the page yet when the bar check passed). It then fails the
   bookmark check as described in phase 2. With `--show-control-bar=false`
   and `--no-control-bar`, the Rust binary passes all checks of phase 2.

3. Pixel comparison of UI states with `tools/rust_port_image_compare.py`
   (Rust and C++ binaries started one after the other with
   `--start-page=about:blank` and the same options, 640x480 window, images
   decoded and compared pixel by pixel with its `compare` mode):

   | Scenario (options) | Images | Result |
   |---|---|---|
   | `static` (default) | bar with address, security icon, go/bookmark buttons, bookmarks/find/clipboard buttons, "Quality" label and PNG selector | identical (0 differing pixels, 641x481, 4047 bytes each) |
   | `static` (`--show-soft-navigation-buttons=true`) | back/forward/refresh/home buttons and separator added | identical |
   | `static` (`--show-control-bar=false`) | browser area only | identical |
   | `hover` | find button pressed (frame and active icon), find bar opened with "ex" typed (layout with find bar, "Find" label, arrow and close icons), find bar closed with Esc | all 4 images identical |
   | `address` | address field clicked (text selected, inverted), "abc" typed, Left, Shift+Left (selection of "b", caret) | both images identical |
   | `quality` | quality selector down button pressed, then quality "100" (JPEG images) | both images identical |
   | `goto file:///etc/hostname` | error message "Access to files through the file:// URI scheme is blocked ..." drawn in dark red below the bar | identical |
   | `goto http://nonexistent.invalid/` | "Loading URL failed due to error: ERR_NAME_NOT_RESOLVED" drawn | identical |
   | `download` | "Download (1)" button next to the quality selector, pressed button, bar after the download was accepted | all 3 images identical |

   The decoded Rust images were also inspected visually (control bar layout,
   icons, text field selection, find bar, error text, download button).

Not verified end to end in phase 3: the download progress bars (the local
test download finishes too fast to catch them), the loading bar animation
(timing dependent, not compared pixel by pixel), the bookmark toggle button
and the find result background (not exercised by the scripts), and the
mouse-over frame of icon buttons without a press (in the `hover` scenario the
first mouse move over the go button did not change the image for either
binary).

### Phase 2: browser server

Environment as in phase 1 (below): stock CEF 156, binary built with
`--features stub-patch`, `retrojsvice.so` from the C++ build copied into
`target/release/`, run as root in a container without network access to the
internet. The C++ binary used for comparison is the C++ build against the same
stock CEF (it links a stub for the patch symbols whose clipboard paste returns
NULL).

1. Server start and window creation:

   ```sh
   xvfb-run -a ./target/release/browservice --use-dedicated-xvfb=no \
       --chromium-args=no-sandbox --vice-opt-http-listen-addr=127.0.0.1:18081
   curl -s -w "status=%{http_code} type=%{content_type} size=%{size_download}\n" \
       http://127.0.0.1:18081/
   ```

   Result: `status=200 type=text/html; charset=UTF-8 size=218`, body:

   ```
   <html>
   <head>
   <title>Browservice</title>
   <script type="text/javascript">
   window.onload = function() {
       window.location.href = "/1/wr8ILRq7lCY47haHiVcr7I7cqrd8vKCe/prev/";
   };
   </script>
   </head>
   <body>
   </body>
   </html>
   ```

   (The same as the C++ binary, apart from the random CSRF token.) The log
   shows `Got request for new window from vice plugin`, `Creating window 1`,
   `CEF browser for window 1 created`. On SIGTERM: `Shutting down server`,
   `Server shutdown complete`, `Quitting CEF message loop`, `CEF shut down
   cleanly`; the temporary directories of the run are removed.

2. Rendering and input, end to end with `tools/rust_port_smoke_test.py`
   (talks the retrojsvice HTTP protocol like its browser-side JavaScript and
   decodes the served PNG window images):

   ```sh
   PAGE=$(python3 tools/rust_port_smoke_test.py --print-start-page)
   xvfb-run -a ./target/release/browservice --use-dedicated-xvfb=no \
       --chromium-args=no-sandbox --vice-opt-http-listen-addr=127.0.0.1:18081 \
       --start-page="$PAGE"
   python3 tools/rust_port_smoke_test.py http://127.0.0.1:18081
   ```

   Output with the Rust binary:

   ```
   GET / -> 200 text/html; charset=UTF-8 (218 bytes)
   main page: prefix=/1/7RJxe93VpVAHijvNCZbcxHnVMfQcaymY mainIdx=1 cipher length=2279
   OK: rendered test page (blue square on red) (image/png, 4628 bytes, 641x481, pixel(100,100)=(0, 0, 255), pixel(500,300)=(255, 0, 0))
   OK: mouse click handled (background green) (image/png, 4628 bytes, 641x481, pixel(100,100)=(0, 0, 255), pixel(500,300)=(0, 255, 0))
   OK: key presses handled (background yellow) (image/png, 5703 bytes, 641x482, pixel(100,100)=(0, 0, 255), pixel(500,300)=(255, 255, 0))
   OK: file upload delivered to the page (background magenta) (image/png, 4702 bytes, 640x481, pixel(100,100)=(0, 0, 255), pixel(500,300)=(255, 0, 255))
   GET goto/browservice://bookmarks/ -> 200
   main page: prefix=/1/7RJxe93VpVAHijvNCZbcxHnVMfQcaymY mainIdx=2 cipher length=2449
   OK: browservice://bookmarks/ page (white page with heading text) (image/png, 10109 bytes, 640x481, pixel(100,100)=(255, 255, 255), pixel(500,300)=(255, 255, 255))
   OK: clipboard set/get round trip through the server
   ALL CHECKS PASSED
   ```

   The C++ binary with `--show-control-bar=false` (same geometry) passes the
   first four checks with images of exactly the same PNG sizes (4628, 4628,
   5703 and 4702 bytes), then fails the bookmark check: its window shows
   `Loading URL failed due to error: ERR_UNKNOWN_URL_SCHEME` (see the
   deviation above). Its clipboard check aborts the C++ process because its
   test stub returns NULL from the paste function. The bookmark page shown by
   the Rust binary listed a bookmark that the C++ binary had written to
   `~/.browservice/bookmarks` earlier, so the file format is compatible.

3. Options, checked with navigation through the plugin's `/goto/` path and
   the log/window image:

   | Run | Navigation | Result |
   |---|---|---|
   | default options | `file:///etc/hostname` | log `Browser area shows error: Access to files through the file:// URI scheme is blocked ...`, white view (phase 2; since phase 3 the message is drawn instead of logged) |
   | `--block-file-scheme=false` | `file:///etc/hostname` | file content (`vm`) rendered in the window image |
   | default options, local HTTPS server with a self-signed certificate on 127.0.0.1:18443 | `https://127.0.0.1:18443/` | log `Invalid certificate for domain '127.0.0.1:18443', canceling request. ...` and `Browser area shows error: Loading URL failed due to error: ERR_CERT_AUTHORITY_INVALID` (the C++ binary draws the same message) |
   | `--certificate-check-exceptions=127.0.0.1:18443` | `https://127.0.0.1:18443/` | the page (green background, "TLS OK") rendered |
   | `--window-limit=2`, third `GET /` | | plugin answers `ERROR: Could not create window, reason: Maximum number of concurrent windows exceeded`; log `Denying window creation due to window limit` |

Not verified end to end in phase 2: accepting downloads (verified in phase 3), popup
windows opened by pages, zoom commands, window titles, and the real Chromium
clipboard and font render settings (they need the patched CEF; only the
in-process test stub was exercised).

### Phase 1

Environment: Linux x86_64, cargo 1.97.0, stock CEF 156.0.15 standard
distribution prepared with `tools/prepare_cef_for_rust.sh`, `retrojsvice.so`
built from `viceplugins/retrojsvice` copied into `target/release/`.

| Command | Result |
|---|---|
| `cargo build --release --features stub-patch` | builds (about 50 s from clean) |
| `cargo build --release` (stock CEF, no stub) | link fails as expected: `undefined symbol: cef_chromiumBrowserviceFontRenderParams...` |
| `cargo test --release --features stub-patch` | 36 passed, 0 failed (phase 2); 46 passed, 1 ignored (phase 3) |
| `cargo test --release` | 35 passed, 0 failed (the stub clipboard test needs the feature) |
| `./browservice --version` | exit 0, output below |
| `./browservice --help` | exit 0 |
| `./browservice --credits` | exit 0 |
| `./browservice --bogus` | exit 1 |
| `./browservice --window-limit=0` | exit 1 |
| `./browservice --start-page` | exit 1 |
| `xvfb-run -a ./browservice --use-dedicated-xvfb=no --chromium-args=no-sandbox` | phase 1 only: exit 0 after a CEF start/stop cycle. Since phase 2 the binary keeps running as a server (see below). |

`./browservice --version` (in `target/release/`):

```
Browservice 0.9.12.3, built with CEF 156.0.15+gcf671c0+chromium-156.0.8078.80
Vice plugin retrojsvice.so: Retrojsvice 0.9.12.3
For copyright information, use --credits
```

`./browservice --bogus`:

```
ERROR: Unrecognized option '--bogus'
Try './browservice --help' for list of supported options
```

`./browservice --window-limit=0`:

```
ERROR: Invalid value '0' given for option --window-limit
See './browservice --help' for more information
```

Comparison with the C++ build (same CEF, same `retrojsvice.so`): stdout and
stderr of `--help`, `--version`, `--credits`, `--bogus`, `--window-limit=0`
and `--start-page` are byte-identical, and the exit codes are the same. With
`--vice-plugin=nonexist.so --version` and `--http-listen-addr=1 --version`
stdout is identical and the log lines on stderr differ only in the source
location (`rust/src/vice.rs:...` instead of `src/vice.cpp:...`). The unit test
`help_output_matches_cpp` checks the Browservice part of `--help` against
`rust/tests/data/help_browservice_part.txt`, captured from the C++ build.

The smoke test runs as root in a container, hence `--chromium-args=no-sandbox`;
CEF logs D-Bus connection errors there, which are harmless.
