#!/usr/bin/env python3
"""End-to-end smoke test for a running Browservice server with the retrojsvice
plugin: talks the plugin's HTTP protocol like the browser-side JavaScript of
retrojsvice (viceplugins/retrojsvice/html/main.html) and checks the pixels of
the served window images.

Start the server with the test page as start page, for example:

  PAGE=$(python3 tools/rust_port_smoke_test.py --print-start-page)
  xvfb-run -a ./browservice --use-dedicated-xvfb=no --chromium-args=no-sandbox \
      --vice-opt-http-listen-addr=127.0.0.1:18081 --start-page="$PAGE"
  python3 tools/rust_port_smoke_test.py http://127.0.0.1:18081

If the server runs with --show-control-bar=false, pass --no-control-bar
after the URL (the page then starts at the top of the window instead of
below the 27 pixel control bar).

Checks:
  0. The control bar is drawn at the top (frame, background, "Address"
     label, address field showing the start page URL) unless
     --no-control-bar is given.
  1. GET / returns 200 and an HTML page (the window redirect page).
  2. The window image shows the test page (blue square on red background).
  3. A mouse click on the square (MDN/MUP events) runs its onclick handler,
     which turns the background green.
  4. Clicking the text field and typing "ab" (KPR events, obfuscated with the
     key cipher of the main page) turns the background yellow.
  5. Clicking a file input and posting a file to the plugin's upload page
     delivers the file to the page (its onchange handler sees the name).
  6. Navigating to browservice://bookmarks/ (through the /goto/ path of the
     plugin, i.e. the URINavigation extension) shows the bookmark page.
  7. Clipboard text set through the plugin's /clipboard/ page is returned
     by a later get (copy and paste callbacks of the server).
  8. (With the control bar.) Navigating to a file served as an attachment
     by a local HTTP server started by this script (127.0.0.1:18082) shows
     the "Download (1)" button in the control bar; clicking it accepts the
     download, and the plugin then offers the file through its iframe and
     download pages with the original content.
Requires Pillow (PIL) for decoding the images.
"""

import html
import http.server
import io
import threading
import re
import sys
import time
import urllib.parse
import urllib.request

PAGE = (
    "<html><head><title>SmokeTest</title></head>"
    "<body style='margin:0;background:rgb(255,0,0)'>"
    "<div style='position:absolute;left:0;top:0;width:200px;height:200px;"
    "background:rgb(0,0,255)' "
    "onclick=\"document.body.style.background='rgb(0,255,0)'\"></div>"
    "<input id='t' style='position:absolute;left:300px;top:0;width:300px;height:40px' "
    "oninput=\"if(this.value=='ab')document.body.style.background='rgb(255,255,0)'\">"
    "<input type='file' style='position:absolute;left:0;top:250px;width:300px;height:100px' "
    "onchange=\"if(this.files.length==1&&this.files[0].name=='up.txt')"
    "document.body.style.background='rgb(255,0,255)'\">"
    "</body></html>"
)

W, H = 640, 480
PASSED = []
# Height of the control bar (ControlBar::Height); set to 0 by --no-control-bar.
Y = 27


def start_page():
    return "data:text/html," + urllib.parse.quote(PAGE)


def get(url, timeout=20):
    with urllib.request.urlopen(url, timeout=timeout) as resp:
        return resp.status, resp.headers.get("Content-Type", ""), resp.read()


def fail(msg):
    print("FAIL:", msg)
    sys.exit(1)


class Session:
    def __init__(self, base):
        self.base = base
        status, ctype, body = get(base + "/")
        print(f"GET / -> {status} {ctype} ({len(body)} bytes)")
        if status != 200 or b"<html>" not in body:
            fail("root page is not HTML with status 200")
        self.enter(body.decode())

    def enter(self, text):
        """Follows the JavaScript redirects to the main page of the window."""
        base = self.base
        for _ in range(10):
            m = re.search(r'window\.location\.href = \\*"([^"\\]+)\\*"', text)
            if "/image/" in text or not m:
                break
            status, ctype, body = get(base + m.group(1))
            text = body.decode()
        m = re.search(r'"(/\d+/[A-Za-z0-9]+)/image/" \+\s*"(\d+)/"', text)
        if not m:
            fail("main page with image URLs not found")
        self.prefix = m.group(1)
        self.main_idx = int(m.group(2))
        cipher = []
        for arr in re.findall(r"pushSnakeOil\(new Array\(([^)]*)\)\)", text):
            cipher += [int(x) for x in arr.split(",") if x.strip()]
        self.cipher = cipher
        # The plugin restarts image and event numbering for each main page.
        self.img_idx = 0
        self.event_idx = 0
        print(f"main page: prefix={self.prefix} mainIdx={self.main_idx} "
              f"cipher length={len(cipher)}")

    def goto(self, uri):
        """Navigates the window (URINavigation extension of the plugin)."""
        status, ctype, body = get(self.base + self.prefix + "/goto/" + uri)
        print(f"GET goto/{uri} -> {status}")
        self.enter(body.decode())

    def image(self, events=()):
        self.img_idx += 1
        path = (f"{self.prefix}/image/{self.main_idx}/{self.img_idx}/1/{W}/{H}/"
                f"{self.event_idx}/")
        for e in events:
            m = re.match(r"(MMO|MDN|MUP|MDBL|MWH)_(-?\d+)_(-?\d+)(.*)", e)
            if m:
                e = f"{m.group(1)}_{m.group(2)}_{int(m.group(3)) + Y}{m.group(4)}"
            path += e + "/"
        self.event_idx += len(events)
        status, ctype, body = get(self.base + path)
        if status != 200:
            fail(f"image request returned {status}")
        from PIL import Image
        return Image.open(io.BytesIO(body)).convert("RGB"), ctype, len(body)

    def key_event(self, key):
        idx = self.event_idx  # caller sends one event per request
        return f"KPR_{key ^ self.cipher[idx % len(self.cipher)]}"


def near(c, ref, tol=40):
    return c is not None and all(abs(a - b) <= tol for a, b in zip(c, ref))


def px(im, x, y):
    """Pixel value at page coordinates (x, y) (the control bar height is
    added), or None if outside the image (images served before the window has
    been resized to the requested size may be smaller)."""
    y += Y
    if x < im.size[0] and y < im.size[1]:
        return im.getpixel((x, y))
    return None


def wait_for(s, pred, what, events=(), timeout=20.0):
    deadline = time.time() + timeout
    img, ctype, size = s.image(events)
    while not pred(img):
        if time.time() > deadline:
            img.save("/tmp/browservice_smoke_last.png")
            fail(f"timed out waiting for {what}; pixel(500,300)={px(img, 500, 300)} "
                 f"pixel(100,100)={px(img, 100, 100)} (saved image to "
                 f"/tmp/browservice_smoke_last.png)")
        time.sleep(0.3)
        img, ctype, size = s.image()
    img.save(f"/tmp/browservice_smoke_{len(PASSED)}.png")
    PASSED.append(what)
    print(f"OK: {what} ({ctype}, {size} bytes, {img.size[0]}x{img.size[1]}, "
          f"pixel(100,100)={px(img, 100, 100)}, pixel(500,300)={px(img, 500, 300)})")
    return img


def check_clipboard(base):
    """Clipboard through the plugin's /clipboard/ page: 'set' calls the
    copyToClipboard callback, 'get' calls requestClipboardContent and waits
    for putClipboardContent."""
    status, ctype, body = get(base + "/clipboard/")
    m = re.search(rb'name="csrftoken" value="([^"]+)"', body)
    if status != 200 or not m:
        fail("clipboard page or its CSRF token not found")
    token = m.group(1).decode()
    text = "smoke clipboard \u00e4"

    def post(fields):
        data = urllib.parse.urlencode(fields).encode()
        with urllib.request.urlopen(base + "/clipboard/", data=data, timeout=20) as resp:
            return resp.read().decode()

    post({"csrftoken": token, "mode": "set", "text": text})
    page = post({"csrftoken": token, "mode": "get"})
    if text not in html.unescape(page):
        fail("clipboard text did not round-trip: " + page[:300])
    print("OK: clipboard set/get round trip through the server")


def control_bar_shown(im):
    """Frame and background of the control bar (src/control_bar.cpp
    widgetRender_), the "Address" label (black pixels) and the address field
    (white box with black text)."""
    if px(im, 100, 100 - Y) is None:
        return False
    g = lambda x, y: im.getpixel((x, y))
    if g(0, 0) != (255, 255, 255) or g(5, 1) != (192, 192, 192):
        return False
    if g(5, 26) != (0, 0, 0) or g(5, 25) != (128, 128, 128) or g(5, 24) != (255, 255, 255):
        return False
    label = sum(1 for x in range(1, 53) for y in range(1, 23) if g(x, y) == (0, 0, 0))
    field_text = sum(1 for x in range(76, 300) for y in range(3, 19) if g(x, y) == (0, 0, 0))
    return label > 30 and field_text > 30 and g(200, 5) == (255, 255, 255)


DOWNLOAD_CONTENT = b"downloaded content 0123456789\n" * 100


class DownloadHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Disposition", 'attachment; filename="smoke.bin"')
        self.send_header("Content-Length", str(len(DOWNLOAD_CONTENT)))
        self.end_headers()
        self.wfile.write(DOWNLOAD_CONTENT)

    def log_message(self, *args):
        pass


def check_download(s):
    """Accepting a pending download with the download button of the control
    bar (src/control_bar.cpp: downloadButton_ -> onPendingDownloadAccepted)."""
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 18082), DownloadHandler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        s.goto("http://127.0.0.1:18082/smoke.bin")

        def download_button(im):
            # The button (88 pixels wide, enabled: black text on gray) is
            # left of the quality selector at the right end of the bar.
            if px(im, 100, 100 - Y) is None:
                return False
            black = sum(1 for x in range(500, 640) for y in range(3, 20)
                        if im.getpixel((x, y)) == (0, 0, 0))
            return black > 150
        wait_for(s, download_button, "download button shown in the control bar")
        # Find the button: its top-left white frame pixel left of the quality
        # selector frame (gray 128).
        img, _, _ = s.image()
        row = [img.getpixel((x, 1)) for x in range(img.size[0])]
        start = max(x for x in range(1, 600) if row[x] == (255, 255, 255) and row[x - 1] == (192, 192, 192))
        bx, by = start + 44, 10
        s.image((f"MMO_{bx}_{by - Y}", f"MDN_{bx}_{by - Y}_0", f"MUP_{bx}_{by - Y}_0"))
        deadline = time.time() + 20
        while time.time() < deadline:
            s.image()
            status, ctype, body = get(f"{s.base}{s.prefix}/iframe/{s.main_idx}/{int(time.time())}/")
            m = re.search(rb'window\.location\.href = "([^"]+)"', body)
            if m:
                path = m.group(1).decode()
                status, ctype, data = get(s.base + path)
                if status != 200 or data != DOWNLOAD_CONTENT:
                    fail(f"downloaded file differs ({status}, {len(data)} bytes)")
                print(f"OK: download accepted from the control bar and served by the "
                      f"plugin ({path}, {len(data)} bytes)")
                return
            time.sleep(0.5)
        fail("download not offered by the plugin after clicking the download button")
    finally:
        server.shutdown()


def main():
    global Y
    if len(sys.argv) == 2 and sys.argv[1] == "--print-start-page":
        print(start_page())
        return
    args = sys.argv[1:]
    if "--no-control-bar" in args:
        args.remove("--no-control-bar")
        Y = 0
    if len(args) != 1:
        print(__doc__)
        sys.exit(2)
    s = Session(args[0].rstrip("/"))

    if Y:
        wait_for(s, control_bar_shown, "control bar drawn above the page")

    img = wait_for(
        s,
        lambda im: near(px(im, 100, 100), (0, 0, 255))
        and near(px(im, 500, 300), (255, 0, 0)),
        "rendered test page (blue square on red)",
    )
    # The plugin may add a border column/row to the requested size.
    if not (W <= img.size[0] <= W + 1 and H <= img.size[1] <= H + 1):
        fail(f"unexpected image size {img.size}")

    wait_for(
        s,
        lambda im: near(px(im, 500, 300), (0, 255, 0)),
        "mouse click handled (background green)",
        events=("MMO_100_100", "MDN_100_100_0", "MUP_100_100_0"),
    )

    s.image(("MDN_450_20_0",))
    s.image(("MUP_450_20_0",))
    time.sleep(0.5)
    s.image((s.key_event(ord("a")),))
    wait_for(
        s,
        lambda im: near(px(im, 500, 300), (255, 255, 0)),
        "key presses handled (background yellow)",
        events=(s.key_event(ord("b")),),
    )

    # File upload: clicking the file input opens the CEF file dialog, which
    # the server turns into an upload request to the plugin; the file is then
    # posted to the plugin's upload page.
    s.image(("MMO_100_300", "MDN_100_300_0", "MUP_100_300_0"))
    token = None
    deadline = time.time() + 20
    while token is None and time.time() < deadline:
        time.sleep(0.5)
        s.image()
        status, ctype, body = get(s.base + s.prefix + "/upload/")
        m = re.search(rb'name="csrftoken" value="([^"]+)"', body)
        if m and b'name="file"' in body:
            token = m.group(1).decode()
    if token is None:
        fail("upload mode not entered after clicking the file input")
    boundary = "----smoketestboundary"
    parts = []
    for name, value in (("csrftoken", token), ("mode", "upload"), ("filename", "")):
        parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n')
    parts.append(
        f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="up.txt"\r\n'
        "Content-Type: text/plain\r\n\r\nuploaded content\r\n"
    )
    parts.append(f"--{boundary}--\r\n")
    req = urllib.request.Request(
        s.base + s.prefix + "/upload/",
        data="".join(parts).encode(),
        headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
    )
    with urllib.request.urlopen(req, timeout=20) as resp:
        resp.read()
    wait_for(
        s,
        lambda im: near(px(im, 500, 200), (255, 0, 255)),
        "file upload delivered to the page (background magenta)",
    )

    s.goto("browservice://bookmarks/")
    def bookmarks_shown(im):
        # The "Bookmarks" heading is large bold black text below the top
        # margin; the error message shown on failure is small dark red text
        # at the very top.
        dark = sum(
            1
            for x in range(0, 300)
            for y in range(25, 70)
            if max(px(im, x, y) or (255, 255, 255)) < 60
        )
        return near(px(im, 500, 300), (255, 255, 255)) and dark > 200
    wait_for(s, bookmarks_shown, "browservice://bookmarks/ page (white page with heading text)")
    check_clipboard(s.base)
    if Y:
        check_download(s)
    print("ALL CHECKS PASSED")


if __name__ == "__main__":
    main()
