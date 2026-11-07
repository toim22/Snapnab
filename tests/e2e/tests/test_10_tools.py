"""The capture app's tools: the screen color picker, the screen focus, the interval capture, the tray's "Open the Editor
after capturing" switch, and the automation command line (an area, a tool and an output combined into one capture:
/region, /window, /monitor with /output)."""
import win32api
import win32con
from PIL import Image

from snapnab_e2e import apps, regs, win
from snapnab_e2e.target import TITLE

PICKER = "SnapnabColorPicker"
FOCUS = "SnapnabScreenFocus"
MENU = "#32768"
IDM_COLOR_PICKER, IDM_INTERVAL, IDM_FOCUS, IDM_AFTER_EDITOR = 1028, 1036, 1037, 1044


def close_menus():
    for _ in range(3):
        if not win.top_windows(MENU, visible_only=True):
            return
        win.keys("{ESC}")
        apps.wait_quiet(0.2)


def test_color_picker_esc_closes(capture):
    capture.start("/hide")
    capture.send("/colorpicker")
    win.wait_window(PICKER)
    apps.wait_quiet(0.3)
    win.keys("{ESC}")
    win.wait_no_window(PICKER)


def test_color_picker_click_copies_the_color(capture, target):
    capture.start("/hide")
    win.clear_clipboard()
    capture.command(IDM_COLOR_PICKER)
    win.wait_window(PICKER)
    apps.wait_quiet(0.4)
    x, y = target.client_center()
    win.click(x, y)
    win.wait_no_window(PICKER)
    try:
        text = win.wait_until(lambda: win.clipboard_text(), what="the color on the clipboard")
        assert "C81E1E" in text.upper().replace(" ", "") or "200, 30, 30" in text or "200,30,30" in text, text
        win.wait_window(MENU)       # the other formats, at the cursor
    finally:
        close_menus()


def test_screen_focus_toggles(capture):
    capture.start("/hide")
    capture.send("/focus")
    win.wait_window(FOCUS)
    capture.command(IDM_FOCUS)
    win.wait_no_window(FOCUS)


def test_interval_capture_saves_into_the_folder_and_stops(capture, logs, tmp_path):
    folder = tmp_path / "interval"
    names = ["IntervalArea", "IntervalSeconds", "IntervalMinutes", "IntervalFormat", "IntervalPrefix", "IntervalFolder",
             "IntervalCursor"]
    try:
        capture.start("/hide")
        capture.command(IDM_INTERVAL)
        form = win.wait_form(capture.pid)
        win.set_combo(form, 1000, 0)            # the full screen
        win.form_set(form, 1, 1)                # every second
        win.form_set(form, 2, 0)                # until stopped
        win.set_combo(form, 1003, 0)            # PNG
        win.form_set(form, 4, "e2eint")
        win.form_set(form, 5, str(folder))
        win.form_ok(form)
        files = win.wait_until(lambda: len(list(folder.glob("e2eint*.png"))) >= 2 and list(folder.glob("e2eint*.png")),
                               timeout=15, what="two interval captures")
        with Image.open(sorted(files)[0]) as im:
            assert im.size == (win32api.GetSystemMetrics(78), win32api.GetSystemMetrics(79))
        assert regs.get("IntervalFolder") == str(folder)
        capture.command(IDM_INTERVAL)           # the tray item stops it now
        logs.capture.wait(r"Interval capture: stopped after \d+ capture")
        n = len(list(folder.glob("*.png")))
        apps.wait_quiet(2.5)
        assert len(list(folder.glob("*.png"))) == n, "nothing saved after the stop"
    finally:
        for n in names:
            regs.delete(n)


def test_interval_from_the_command_line(capture, logs, tmp_path):
    folder = tmp_path / "cmdline"
    regs.set_dword("IntervalSeconds", 1)
    regs.set_string("IntervalFolder", str(folder))
    regs.set_string("IntervalPrefix", "cli")
    try:
        capture.start("/hide")
        capture.send("/interval")
        win.wait_until(lambda: list(folder.glob("cli*.png")), timeout=10, what="an interval capture")
        capture.send("/interval", "stop")
        logs.capture.wait(r"Interval capture: stopped")
    finally:
        for n in ["IntervalSeconds", "IntervalFolder", "IntervalPrefix"]:
            regs.delete(n)


def test_after_capture_editor_switch(capture):
    regs.set_dword("AfterCaptureMode", 1)
    try:
        capture.start("/hide")
        capture.command(IDM_AFTER_EDITOR)
        win.wait_until(lambda: regs.get("AfterCaptureMode") == 3, what="only copy to the clipboard")
        assert regs.get("AfterCaptureModeOn") == 1
        capture.command(IDM_AFTER_EDITOR)
        win.wait_until(lambda: regs.get("AfterCaptureMode") == 1, what="the mode chosen before")
    finally:
        regs.delete("AfterCaptureMode")
        regs.delete("AfterCaptureModeOn")


# ---------- automation: an area plus an output ----------

def image_size(path):
    with Image.open(path) as im:
        return im.size


def test_region_to_a_file(capture, tmp_path):
    out = tmp_path / "region.png"
    capture.start("/hide")
    capture.send("/region", "100,120,321,123", "/output", str(out))
    win.wait_until(out.exists, what="the /output file")
    assert win.wait_until(lambda: out.stat().st_size > 0 and image_size(out) == (321, 123), what="321x123")


def test_region_to_the_clipboard(capture):
    capture.start("/hide")
    win.clear_clipboard()
    capture.send("/region", "50,60,200,150", "/output", "clipboard")
    img = win.wait_until(win.clipboard_image, what="the image on the clipboard")
    assert img.size == (200, 150)
    assert not win.find_window("SnapnabEditor", visible_only=True)


def test_window_by_title_to_a_file(capture, target, tmp_path):
    out = tmp_path / "window.png"
    capture.start("/hide")
    capture.send("/window", TITLE.lower(), "/output", str(out))
    win.wait_until(lambda: out.exists() and out.stat().st_size > 0, what="the /output file")
    l, t, r, b = target.rect
    w, h = image_size(out)
    assert abs(w - (r - l)) <= 16 and abs(h - (b - t)) <= 16, ((w, h), target.rect)
    with Image.open(out) as im:
        px = im.convert("RGB").getpixel((w // 2, h // 2))
    assert abs(px[0] - 0xC8) < 12 and px[1] < 50 and px[2] < 50, px


def test_unknown_window_and_monitor_only_notify(capture, logs, tmp_path):
    out = tmp_path / "none.png"
    capture.start("/hide")
    capture.send("/window", "no window has this title 7f3a", "/output", str(out))
    logs.capture.wait(r"Command line: no window titled")
    capture.send("/monitor", "99", "/output", str(out))
    apps.wait_quiet(1.5)
    assert not out.exists()


def test_monitor_to_a_file(capture, tmp_path):
    out = tmp_path / "monitor.png"
    capture.start("/hide")
    capture.send("/monitor", "1", "/output", str(out))
    win.wait_until(lambda: out.exists() and out.stat().st_size > 0, what="the /output file")
    w, h = image_size(out)
    mons = sorted(win32api.EnumDisplayMonitors(), key=lambda m: (m[2][0], m[2][1]))
    l, t, r, b = mons[0][2]
    assert (w, h) == (r - l, b - t), ((w, h), mons[0][2])
