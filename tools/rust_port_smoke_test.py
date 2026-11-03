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

Checks:
  1. GET / returns 200 and an HTML page (the window redirect page).
  2. The window image shows the test page (blue square on red background).
  3. A mouse click on the square (MDN/MUP events) runs its onclick handler,
     which turns the background green.
  4. Clicking the text field and typing "ab" (KPR events, obfuscated with the
     key cipher of the main page) turns the background yellow.
  5. Navigating to browservice://bookmarks/ (through the /goto/ path of the
     plugin, i.e. the URINavigation extension) shows the bookmark page.
  6. Clipboard text set through the plugin's /clipboard/ page is returned
     by a later get (copy and paste callbacks of the server).
Requires Pillow (PIL) for decoding the images.
"""

import html
import io
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
    "</body></html>"
)

W, H = 640, 480
PASSED = []


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
    """Pixel value, or None if outside the image (images served before the
    window has been resized to the requested size may be smaller)."""
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


def main():
    if len(sys.argv) == 2 and sys.argv[1] == "--print-start-page":
        print(start_page())
        return
    if len(sys.argv) != 2:
        print(__doc__)
        sys.exit(2)
    s = Session(sys.argv[1].rstrip("/"))

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

    s.goto("browservice://bookmarks/")
    def bookmarks_shown(im):
        dark = sum(
            1
            for x in range(0, 300, 2)
            for y in range(0, 80, 2)
            if sum(px(im, x, y) or (255, 255, 255)) < 200
        )
        return near(px(im, 500, 300), (255, 255, 255)) and dark > 20
    wait_for(s, bookmarks_shown, "browservice://bookmarks/ page (white page with heading text)")
    check_clipboard(s.base)
    print("ALL CHECKS PASSED")


if __name__ == "__main__":
    main()
