"""Pinned images (Pins): pin a file, the clipboard (an image, a color, a text) or a new capture; close one with Esc
(it goes to the history) and reopen it; the tray's Pins commands. Then the floating panels: the quick access overlay
after a capture and One Click. Driven with the command line and WM_COMMAND."""
import win32con
from PIL import Image

from snapnab_e2e import apps, presets, regs, win
from snapnab_e2e.config import DATA_DIR

PIN = "SnapnabPin"
SELECT = "SnapnabSelectWindowClass"
IDM_PIN_CLIPBOARD, IDM_PIN_FILE, IDM_PINS_SHOW, IDM_PINS_HIDE, IDM_PINS_CLOSE, IDM_PINS_REOPEN = range(1029, 1035)
IDM_CAPTURE_PIN = 1040
IDM_ONECLICK = 1014
QUICK = "SnapnabQuickOverlay"


def history():
    d = DATA_DIR / "Pins"
    return sorted(d.glob("pin_*.png")) if d.exists() else []


def pins():
    return win.top_windows(PIN, visible_only=True)


def wait_pins(n):
    return win.wait_until(lambda: len(pins()) == n and pins(), what=f"{n} pin(s) on the screen")


def picture(tmp_path, size=(271, 133), color=(200, 30, 60)):
    f = tmp_path / "pin-me.png"
    Image.new("RGB", size, color).save(f)
    return f


def size(h):
    l, t, r, b = win.rect(h)
    return r - l, b - t


def test_pin_file_esc_goes_to_history_and_reopens(capture, tmp_path):
    capture.start("/hide")
    before = len(history())
    capture.send("/pin", str(picture(tmp_path)))
    h = wait_pins(1)[0]
    w, hh = size(h)
    assert w - hh == 271 - 133, "the pin is the image plus the same margin on every side"
    win.send(h, win32con.WM_KEYDOWN, win32con.VK_ESCAPE)
    win.wait_until(lambda: not pins(), what="the pin closed")
    assert len(history()) == min(before + 1, 10), "a closed pin is kept in the history"
    capture.command(IDM_PINS_REOPEN)
    h = wait_pins(1)[0]
    assert size(h)[0] - size(h)[1] == 271 - 133


def test_pin_clipboard_image_color_and_text(capture):
    capture.start("/hide")
    win.set_clipboard_image(Image.new("RGB", (150, 90), (10, 120, 200)))
    capture.command(IDM_PIN_CLIPBOARD)
    h = wait_pins(1)[0]
    assert size(h)[0] - size(h)[1] == 60
    win.set_clipboard_text("#FF8000")
    capture.command(IDM_PIN_CLIPBOARD)
    wait_pins(2)
    assert any(win.title(p) == "#FF8000" for p in pins()), [win.title(p) for p in pins()]
    win.set_clipboard_text("a note to keep on the screen")
    capture.send("/pinclipboard")
    wait_pins(3)


def test_tray_hide_show_and_close_all(capture, tmp_path):
    capture.start("/hide")
    capture.send("/pin", str(picture(tmp_path)))
    wait_pins(1)
    capture.send("/pin", str(picture(tmp_path)))
    wait_pins(2)
    capture.command(IDM_PINS_HIDE)
    win.wait_until(lambda: not pins(), what="the pins hidden")
    capture.command(IDM_PINS_SHOW)
    wait_pins(2)
    capture.command(IDM_PINS_CLOSE)
    win.wait_until(lambda: not win.top_windows(PIN), what="every pin closed")


def test_capture_and_pin_keeps_the_place(capture, target):
    capture.start("/hide")
    capture.command(IDM_CAPTURE_PIN)
    win.wait_window(SELECT)
    apps.wait_quiet(0.3)
    l, t, _, _ = target.rect
    win.drag([(l + 50, t + 60), (l + 350, t + 260)])
    h = wait_pins(1)[0]
    w, hh = size(h)
    assert abs((w - hh) - 100) <= 2, (w, hh)
    pl, pt, _, _ = win.rect(h)
    margin = (w - 300) // 2
    assert abs(pl + margin - (l + 50)) <= 3 and abs(pt + margin - (t + 60)) <= 3, "the pin opens where it was captured"


def test_quick_overlay_after_capture_closes_by_itself(capture, lib):
    regs.set_dword("AfterCaptureMode", 1)       # the overlay instead of the Editor
    regs.set_dword("OverlaySeconds", 2)
    try:
        presets.write(presets.preset("e2e quick", "FullScreen", editor=True))
        capture.start("/hide")
        assert capture.send("/preset", "e2e quick") == 0
        lib.wait_new()
        win.wait_window(QUICK)
        assert not win.find_window("SnapnabEditor", visible_only=True), "the overlay comes instead of the Editor"
        win.wait_no_window(QUICK, timeout=8)
    finally:
        regs.delete("AfterCaptureMode")
        regs.delete("OverlaySeconds")


def test_one_click_toggle(capture):
    regs.set_dword("OneClickEnabled", 0)
    try:
        capture.start("/hide")
        capture.command(IDM_ONECLICK)
        win.wait_window("SnapnabOneClick")
        assert regs.get("OneClickEnabled") == 1
        capture.command(IDM_ONECLICK)
        win.wait_no_window("SnapnabOneClick")
        assert regs.get("OneClickEnabled") == 0
    finally:
        regs.delete("OneClickEnabled")
