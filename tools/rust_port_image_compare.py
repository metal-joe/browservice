#!/usr/bin/env python3
"""Captures window images of a running Browservice server (with the
retrojsvice plugin) for UI scenarios, to compare the Rust port with the C++
binary pixel by pixel. Coordinates are window coordinates for a 640x480
window with the default control bar layout (quality selector and clipboard
button shown).

Usage (start the server with --start-page=about:blank and the options of the
scenario, run the same against the C++ binary with another prefix):

  python3 tools/rust_port_image_compare.py http://127.0.0.1:18081 /tmp/rs SCENARIO [URI]
  python3 tools/rust_port_image_compare.py compare /tmp/rs_X.png /tmp/cpp_X.png ...

Scenarios (images written to PREFIX_<name>.png):
  static    one image (e.g. with --show-soft-navigation-buttons=true or
            --show-control-bar=false)
  hover     mouse over the go button, find button pressed, find bar opened
            and "ex" typed, find bar closed with Esc
  address   click on the address field (text selected), type "abc", Left,
            Shift+Left (selection)
  quality   press and release the quality selector down button
  goto URI  navigate to URI (e.g. an error page)
  download  navigate to an attachment served on 127.0.0.1:18082, image
            with the download button, button pressed, after release
"""
import sys, time
sys.path.insert(0, __import__("os").path.dirname(__import__("os").path.abspath(__file__)))
import rust_port_smoke_test as t

t.Y = 0  # raw window coordinates
def compare(a_path, b_path):
    from PIL import Image, ImageChops
    a = Image.open(a_path).convert("RGB")
    b = Image.open(b_path).convert("RGB")
    if a.size != b.size:
        print(f"{a_path} vs {b_path}: sizes differ {a.size} {b.size}")
        return False
    diff = ImageChops.difference(a, b)
    n = sum(1 for x in range(a.size[0]) for y in range(a.size[1]) if diff.getpixel((x, y)) != (0, 0, 0))
    print(f"{a_path} vs {b_path}: size {a.size}, differing pixels {n}, bbox {diff.getbbox()}")
    return n == 0


if sys.argv[1] == "compare":
    sys.exit(0 if all([compare(a, b) for a, b in zip(sys.argv[2::2], sys.argv[3::2])]) else 1)

base, outp, scen = sys.argv[1], sys.argv[2], sys.argv[3]
s = t.Session(base)
time.sleep(3)
s.image(); time.sleep(1)
def snap(name, events=(), wait=1.5):
    img, ctype, size = s.image(events)
    time.sleep(wait)
    img, ctype, size = s.image()
    img.save(f"{outp}_{name}.png")
    print(name, img.size, ctype, size)
def key(k):  # KPR for printable key
    s.image((s.key_event(k),))
def kdn(k):  # special key down (k negative key code)
    idx = s.event_idx
    s.image((f"KDN_{(-k) ^ s.cipher[idx % len(s.cipher)]}",))
def kup(k):
    idx = s.event_idx
    s.image((f"KUP_{(-k) ^ s.cipher[idx % len(s.cipher)]}",))
if scen == "static":
    snap("static")
elif scen == "hover":
    snap("hover_go", ("MMO_439_10",))
    snap("press_find", ("MMO_509_10", "MDN_509_10_0"))
    s.image(("MUP_509_10_0",)); time.sleep(1)
    s.image(("MMO_300_300",)); time.sleep(1)
    for c in "ex": key(ord(c)); time.sleep(0.3)
    snap("findbar_typed", (), 0.2)
    kdn(-27); kup(-27); time.sleep(1)
    snap("findbar_closed", ("MMO_300_300",))
elif scen == "address":
    s.image(("MMO_200_12", "MDN_200_12_0", "MUP_200_12_0"))
    snap("addr_selected", (), 0.1)
    for c in "abc": key(ord(c)); time.sleep(0.2)
    kdn(-37); kup(-37); kdn(-16); kdn(-37); kup(-37); kup(-16)
    snap("addr_typed", (), 0.1)
elif scen == "quality":
    snap("q_down", ("MMO_630_16", "MDN_630_16_0"), 0.3)
    s.image(("MUP_630_16_0",)); time.sleep(1.5)
    snap("q_after", ("MMO_300_300",))
elif scen == "goto":
    s.goto(sys.argv[4]); time.sleep(3)
    snap("goto")
if scen == "download":
    import http.server, threading
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 18082), t.DownloadHandler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    s.goto("http://127.0.0.1:18082/smoke.bin"); time.sleep(3)
    snap("dl_pending")
    s.image(("MMO_596_10", "MDN_596_10_0"))
    snap("dl_pressed", (), 0.5)
    s.image(("MUP_596_10_0",)); time.sleep(2)
    snap("dl_after", ("MMO_300_300",))
    srv.shutdown()
