# Rust port of Browservice: status

The C++ sources (`src/`, `viceplugins/`) are still in place and remain the
reference implementation. The Rust port lives in `rust/` with `Cargo.toml` at
the repository root (crate `browservice`, binary `browservice`).

**Status after phase 2:** the Rust binary runs the browser server. It starts
the vice plugin, creates windowless CEF browsers for the windows the plugin
asks for, serves their rendered pages to the plugin as window images and
forwards mouse and keyboard input to CEF. With the C++ `retrojsvice.so` plugin,
pages are shown and can be used through the plugin's HTTP interface (verified
end to end, see below). **The control bar (address field, navigation buttons,
find bar, quality selector, download button) and all text rendering are not
ported**, so the window shows only the browser view, as with
`--show-control-bar=false` in the C++ build.

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
| `rust/src/browser_area.rs` | `src/browser_area.{hpp,cpp}` | Complete except the error text: the render handler copies dirty rectangles of the off-screen view and popup buffers (e.g. `<select>` dropdowns) into the window image with change detection; mouse (buttons, double click, wheel, enter/leave), key (RAWKEYDOWN + CHAR, KEYUP, modifier tracking, AltGr workaround, Ctrl+X/C/V through the focused frame) and focus events go to the CEF browser host. In error mode the view is white and the message is written to the log instead of being drawn (text rendering is not ported). |
| `rust/src/widget.rs` | `src/widget.{hpp,cpp}` | Complete widget base: viewport, render, focus and mouse-over routing, button/key state cleanup, cursors, global hotkeys. |
| `rust/src/root_widget.rs` | `src/root_widget.{hpp,cpp}`, interface of `src/control_bar.hpp` | Root widget with the browser area over the whole viewport. `ControlBar` is only a state holder (address, title, loading, security status, quality selector, clipboard button, downloads, find result) that the window updates; it is not drawn and takes no space. |
| `rust/src/image_slice.rs` | `src/image_slice.{hpp,cpp}`, `src/rect.hpp` | Complete (shared BGRx buffers, sub-slices, fill, put, clone). |
| `rust/src/key.rs` | `src/key.{hpp,cpp}`, `src/key_codes.hpp` | Complete. |
| `rust/src/download_manager.rs` | `src/download_manager.{hpp,cpp}` | Complete: pending queue, accept, progress, cancel, `CompletedDownload` that deletes the file (and releases the temporary directory) when the plugin calls the cleanup function from any thread. |
| `rust/src/scheme.rs` | `src/scheme.{hpp,cpp}` | `browservice://bookmarks/` and the 404 page (see the deviation below). |
| `rust/src/bookmarks.rs` | `src/bookmarks.{hpp,cpp}` | Complete: same file format and location (`~/.browservice/bookmarks`), atomic save, cache by URL, page with removal form. |
| `rust/src/temp_dir.rs` | `src/temp_dir.{hpp,cpp}` | Complete (Linux). |
| `rust/src/data_url.rs`, `rust/src/sha3.rs` | `src/data_url.{hpp,cpp}`, `src/tiny_sha3` | Complete; base64 and URI coding through the CEF functions, SHA3-256 ported from tiny_sha3. |
| `rust/src/clipboard.rs` | `src/clipboard.{hpp,cpp}` | Complete, through the patch symbols. |
| `rust/src/timeout.rs` | `src/timeout.{hpp,cpp}` | Complete. |
| `rust/src/ui.rs` | `postTask` in `src/common.cpp` | Posting closures to the CEF UI thread (UI-thread closures and `Send` closures from any thread). |
| `rust/src/globals.rs` | `src/globals.{hpp,cpp}` | Configuration and dot directory, readable from any thread (the text render context belongs to the control bar port). |
| `rust/src/cef_app.rs`, `rust/src/main.rs` | `App` and `main` in `src/main.cpp` | Plugin load and context init before CEF, `CefInitialize`, scheme handler factory, request context (incognito or `--data-dir`), server creation in `OnContextInitialized`, 200 ms termination signal polling, shutdown before quitting the message loop, `CefShutdown`. |

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
- Error messages of the browser area are logged (`Browser area shows error:
  ...`) instead of drawn.

## Not ported yet

- **Control bar and text rendering**: `src/control_bar.cpp`, `text.cpp`
  (Pango), `widget` subclasses `text_field.cpp`, `button.cpp`,
  `menu_button.cpp`, `quality_selector.cpp`, `find_bar.cpp`. Consequences: no
  address field, buttons, find bar, quality selector, bookmark toggle,
  clipboard or download buttons inside the window image; pending downloads
  cannot be accepted (the C++ build also needs the control bar for that), and
  the plugin's quality and clipboard button information is only stored.
  `--show-control-bar=true` (the default) prints a warning and behaves like
  `false`.
- Dedicated Xvfb (`src/xvfb.cpp`). The `--use-dedicated-xvfb` option is parsed,
  but the Rust binary prints a warning and uses the `DISPLAY` of the
  environment.
- `XSetErrorHandler`/`XSetIOErrorHandler` (ignoring non-fatal X errors) and
  panicking through CEF's `LOG(FATAL)` after initialization; panics abort the
  process directly.
- Windows-specific code paths (Windows option defaults are mirrored in the
  config, but the Windows build is not done or tested).
- The vice plugin `viceplugins/retrojsvice` stays in C++ and is loaded as a
  shared library through `vice_plugin_api.h`.

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
   | default options | `file:///etc/hostname` | log `Browser area shows error: Access to files through the file:// URI scheme is blocked ...`, white view |
   | `--block-file-scheme=false` | `file:///etc/hostname` | file content (`vm`) rendered in the window image |
   | default options, local HTTPS server with a self-signed certificate on 127.0.0.1:18443 | `https://127.0.0.1:18443/` | log `Invalid certificate for domain '127.0.0.1:18443', canceling request. ...` and `Browser area shows error: Loading URL failed due to error: ERR_CERT_AUTHORITY_INVALID` (the C++ binary draws the same message) |
   | `--certificate-check-exceptions=127.0.0.1:18443` | `https://127.0.0.1:18443/` | the page (green background, "TLS OK") rendered |
   | `--window-limit=2`, third `GET /` | | plugin answers `ERROR: Could not create window, reason: Maximum number of concurrent windows exceeded`; log `Denying window creation due to window limit` |

Not verified end to end: accepting downloads (needs the control bar), popup
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
| `cargo test --release --features stub-patch` | 36 passed, 0 failed (phase 2) |
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
