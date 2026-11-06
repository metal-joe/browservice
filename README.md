# Browservice: Browser as a Service

Browservice is a web proxy that lets historical browsers use the modern web. A Chromium browser runs on the proxy server and renders its view into images. A small JavaScript application in the client browser shows these images and sends keyboard and mouse input back to the proxy.

Browservice was created by **Topi Talvitie** ([@ttalvitie](https://github.com/ttalvitie/browservice)), who wrote the original C++ version. This repository continues that work: the server is ported to **Rust**, and the embedded browser is updated to **CEF 156 / Chromium 156.0.8078.80**. The original project is MIT licensed, and so is this fork.

> **Status:** actively developed, but still early. See [Status](#status) for what is tested and what is not.

## Screenshots

> The screenshots below are placeholders. Add your own images under `docs/screenshots/` with these file names.

| | |
| --- | --- |
| ![Browser view with control bar](docs/screenshots/control-bar.png) | ![Find bar in use](docs/screenshots/find-bar.png) |
| Control bar and browser view | Text search |
| ![Download confirmation](docs/screenshots/download.png) | ![Browservice running on a historical client](docs/screenshots/client.png) |
| Downloads are confirmed on the control bar | Client on a historical browser |

## Features

- View and use web pages rendered by Chromium, with keyboard and mouse input forwarded to the page
- Several browser windows at the same time (`--window-limit`)
- Address bar, back, forward and reload handled on the proxy
- Text search within the current page
- Text clipboard shared by all windows
- File downloads, confirmed on the control bar before they are delivered to the client
- File uploads
- Bookmarks page (`browservice://bookmarks/`)
- Image quality chosen on the client (JPEG levels or PNG)
- Page zoom (`--initial-zoom`, plus hotkeys)
- Font render settings for the browser view (`--browser-font-render-mode`)
- Certificate check exceptions for specific domains (`--certificate-check-exceptions`)
- Hidden control bar for kiosk use (`--show-control-bar=false`)

## Background

Modern browsers do not run on old operating systems, and old browsers should not be connected to the Internet directly, because they have unpatched security holes. Browservice moves the web rendering to a proxy server with a current browser. The client only has to show images and forward input.

Sending the whole browser view as a new image is not efficient on paper, but it works well on a fast local network. Even older hardware can reach a usable frame rate with a low JPEG quality and a small window.

The idea of rendering pages to images on a proxy is also used by [WRP (Web Rendering Proxy)](https://github.com/tenox7/wrp). Browservice uses JavaScript on the client, so it gives a smoother experience but needs a newer client browser than WRP.

### Security

Browservice reduces the attack surface: untrusted web content reaches the client only as images, explicit downloads or clipboard text. The address bar is also drawn on the proxy, so the client never receives URLs as text.

This is no guarantee. The proxy must be kept up to date, and the connection between proxy and client must be on a trusted network. Client and proxy trust each other: a client controls a browser that runs on the proxy.

## Supported client browsers

The original project tested many old operating system and browser combinations. The full table is in the [original README](https://github.com/ttalvitie/browservice/blob/v0.9.12.2/README.md#supported-client-browsers). Modern browsers also work as clients. The `fig/` directory contains screenshots from the original project on historical clients.

## Build from source

Browservice needs a patched CEF binary distribution. The patch lets the embedded Chromium use a text clipboard that Browservice can read and write, and changes font rendering. The test build below does not need the patch, but production builds do.

### Requirements (Linux)

- Rust stable (tested with cargo 1.97.0)
- `pkg-config`, `libpango1.0-dev`, `libfontconfig-dev`, `libx11-dev`
- For the default dedicated X server: `Xvfb` and `xauth`
- An extracted CEF 156 distribution (`156.0.15+gcf671c0+chromium-156.0.8078.80`)

### Preparing CEF

The Rust crates expect a flat CEF layout. Convert a standard distribution with the helper script:

```sh
tools/prepare_cef_for_rust.sh /path/to/cef_binary_..._linux64 /path/to/cef-rust
export CEF_PATH=/path/to/cef-rust
```

### Test build (stock CEF)

This build runs without the patch, using stub symbols for the patched functions. Do not distribute it.

```sh
cargo build --release --features stub-patch
cargo test --release --features stub-patch
```

### Production build (patched CEF)

The patched CEF is built with the C++ tooling in this repository. See [BUILD.md](BUILD.md).

```sh
cargo build --release
```

The binary is written to `target/release/browservice`. The CEF runtime files are copied next to it.

### Windows

On Windows, `tools/build_windows_rust.ps1` downloads CEF, builds the sandboxed DLL and packages a release folder. Run it from PowerShell in the repository:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools\build_windows_rust.ps1 -StubPatch
```

Use `-CefTarball <path>` with a patched CEF archive for production builds. The Windows build needs Visual Studio 2022 with C++ tools, the MSVC Rust toolchain for x64 (`stable-x86_64-pc-windows-msvc`), Git and Python 3.

## Running

Start the proxy with default settings:

```sh
./target/release/browservice
```

By default the HTTP server listens on `127.0.0.1:8080`, so only local connections are accepted. Open `http://127.0.0.1:8080/` in a browser on the same machine.

Show all options with:

```sh
./target/release/browservice --help
```

Options used most often:

| Option | Purpose |
| --- | --- |
| `--vice-opt-http-listen-addr=ADDR:PORT` | Address the HTTP server listens on |
| `--data-dir=PATH` | Keep cookies, cache and sessions in a directory (default: incognito) |
| `--start-page=URL` | Page opened in new windows |
| `--window-limit=N` | Maximum number of concurrent windows |
| `--block-file-scheme=false` | Allow `file://` URLs (blocked by default) |
| `--certificate-check-exceptions=DOMAIN,...` | Skip certificate checks for listed domains |
| `--show-control-bar=false` | Hide the control bar (kiosk mode) |
| `--initial-zoom=PERCENT` | Initial zoom of the browser view |
| `--chromium-args=SWITCH[=VALUE]` | Extra switches for Chromium (for example `--chromium-args=use-gl=desktop`) |

> **Warning:** Binding to `0.0.0.0` lets anyone on the network use the browser, including its logged-in sessions. Restrict the listen address and use a firewall. Browservice does not encrypt traffic between proxy and client, so use only a trusted or tunnelled network.

## Browser usage

Set the proxy address as the home page of the client browser, for example `http://192.168.56.1:8080/`. Use the control bar to navigate, search, zoom, change the image quality and accept downloads.

## Status

What has been verified:

- The Rust server runs with CEF 156 on Linux. It serves pages, handles input, text search, uploads, downloads, bookmarks and certificate exceptions.
- The control bar and error messages are pixel-identical to the C++ version in the scenarios compared.
- A dedicated X server, X error handling and the panic path are implemented.
- The Windows build is compile-checked for x64 targets.

What is not verified yet:

- The patched Chromium build has not been built end-to-end. Clipboard and font settings were only tested with stub symbols.
- Windows: the sandboxed DLL and packaging script have not been run on a Windows machine.
- Some UI paths are not exercised by automated tests: download progress bars, the loading animation, the bookmark toggle and the hover state of icon buttons.

Known differences from the original:

- `browservice://bookmarks/` works in this version. The original failed with `ERR_UNKNOWN_URL_SCHEME`.

## Issues

Bug reports and feature requests are welcome in the [issue tracker](https://github.com/metal-joe/browservice/issues) of this repository.

## Credits

- **Topi Talvitie** ([@ttalvitie](https://github.com/ttalvitie)) wrote the original Browservice and its Retrojsvice plugin. Thank you for the project.
- The Chromium Embedded Framework is developed by the CEF project.
- The text rendering uses Pango and FreeType.

## License

MIT. See [LICENSE](LICENSE).
