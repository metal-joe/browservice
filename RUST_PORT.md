# Rust port of Browservice: status

The C++ sources (`src/`, `viceplugins/`) are still in place and are still the
working implementation. The Rust port lives in `rust/` with `Cargo.toml` at the
repository root (crate `browservice`, binary `browservice`).

**The browser does not work in the Rust port yet.** Phase 1 covers the command
line, vice plugin loading and a CEF start/stop cycle only.

## Ported (phase 1)

| Rust module | C++ origin | Status |
|---|---|---|
| `rust/src/config.rs` | `src/config.cpp`, `src/config_defs.hpp`, `src/config.hpp` | All options with the same names, value specs, defaults, parsers, validation, deprecated aliases (`--default-quality`, `--http-auth`, `--http-listen-addr`), `--vice-opt-*` pass-through, duplicate detection, error messages and exit codes. `--help` (including the help line wrapping algorithm and the vice plugin option list), `--version` and `--credits` print the same text as the C++ build. |
| `rust/src/credits.rs` | `src/credits.cpp` | Verbatim copy. |
| `rust/src/vice.rs` | `VicePlugin` in `src/vice.cpp` | Loads the plugin with `dlopen(RTLD_NOW \| RTLD_LOCAL \| RTLD_DEEPBIND)` through `libloading`, resolves every required API symbol, checks API version 2000000, installs log/panic callbacks, loads extension symbols, queries version, credits and option docs. |
| `rust/src/browser_font_render_mode.rs` | `src/browser_font_render_mode.cpp` | Complete. |
| `rust/src/chromium_patch.rs` | extern declarations in `src/clipboard.cpp`, `src/browser_font_render_mode.cpp` | `extern "C"` declarations of all symbols added by the Chromium patch, with safe wrappers. |
| `rust/src/cef_app.rs`, `rust/src/main.rs` | `App` and `main` in `src/main.cpp` | `CefExecuteProcess` subprocess dispatch, command line switches (`disable-smooth-scrolling`, ANGLE/SwiftShader, `--chromium-args`), `browservice` custom scheme registration, `CefInitialize` with windowless rendering enabled and `command_line_args_disabled`, user agent and cache paths (incognito `~/.browservice/cef` or `--data-dir`), SIGINT/SIGTERM handling, message loop and `CefShutdown`. Since there is no server yet, the browser process quits the message loop as soon as the CEF context is initialized. |
| `rust/src/globals.rs` | `getDotDirPath`/`ensureDirExists` in `src/globals.cpp` | Complete. |
| `rust/src/common.rs` | logging/panic in `src/common.{hpp,cpp}` | Same message formats (`INFO @ location -- msg`, `PANIC @ location: msg`); locations are Rust source locations. |

## Not ported yet

- Server, windows, browser area, control bar and all widgets, text rendering
  (Pango), image slices, key handling, bookmarks, downloads/uploads, find bar,
  data URLs, `browservice:` scheme handler, temp dirs, timeouts.
- `ViceContext` (plugin context lifecycle and callbacks). Only the global plugin
  functions are used.
- Dedicated Xvfb (`src/xvfb.cpp`). The `--use-dedicated-xvfb` option is parsed,
  but in normal mode the Rust binary prints a warning and uses the `DISPLAY` of
  the environment.
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

Environment: Linux x86_64, cargo 1.97.0, stock CEF 156.0.15 standard
distribution prepared with `tools/prepare_cef_for_rust.sh`, `retrojsvice.so`
built from `viceplugins/retrojsvice` copied into `target/release/`.

| Command | Result |
|---|---|
| `cargo build --release --features stub-patch` | builds (about 50 s from clean) |
| `cargo build --release` (stock CEF, no stub) | link fails as expected: `undefined symbol: cef_chromiumBrowserviceFontRenderParams...` |
| `cargo test --release --features stub-patch` | 16 passed, 0 failed |
| `cargo test --release` | 15 passed, 0 failed (the stub clipboard test needs the feature) |
| `./browservice --version` | exit 0, output below |
| `./browservice --help` | exit 0 |
| `./browservice --credits` | exit 0 |
| `./browservice --bogus` | exit 1 |
| `./browservice --window-limit=0` | exit 1 |
| `./browservice --start-page` | exit 1 |
| `xvfb-run -a ./browservice --use-dedicated-xvfb=no --chromium-args=no-sandbox` | exit 0: CEF initialized with windowless rendering, subprocesses (e.g. `--type=zygote`) started by re-executing the binary, context initialized, message loop quit, `CefShutdown` completed |

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
