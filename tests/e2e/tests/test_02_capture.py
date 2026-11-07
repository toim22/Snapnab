"""Image capture: every selection mode, the outputs (Editor, clipboard, file, program, text) and the capture engine
state machine, driven through presets written to Presets1.xml and run with `Snapnab.exe /preset "name"`."""
import time

import psutil
import pytest
import win32api
from PIL import Image

from snapnab_e2e import apps, library, presets, regs, win
from snapnab_e2e.apps import CaptureCmd
from snapnab_e2e.config import EDITOR_EXE, LIBRARY_DIR
from snapnab_e2e.logs import state

OVERLAY = "SnapnabSelectWindowClass"


def virtual_screen():
    return (win32api.GetSystemMetrics(76), win32api.GetSystemMetrics(77),
            win32api.GetSystemMetrics(78), win32api.GetSystemMetrics(79))


def start_with(capture, *preset_xml):
    presets.write(*preset_xml)
    capture.start("/hide")
    return capture


def run_preset(capture, name):
    assert capture.send("/preset", name) == 0


def overlay():
    h = win.wait_window(OVERLAY)
    apps.wait_quiet(0.3)    # the crosshair paints the frozen screen first
    return h


def wait_idle(logs, timeout=15):
    logs.capture.wait(r"CaptureEngine: \w+ → Idle", timeout)


def near(a, b, tol):
    return abs(a - b) <= tol


# ---------- selection modes ----------

@pytest.mark.smoke
def test_full_screen(capture, logs, lib):
    start_with(capture, presets.preset("e2e full", "FullScreen", editor=False))
    run_preset(capture, "e2e full")
    for a, b in [("Idle", "Activated"), ("Capturing", "HaveInput"), ("HaveInput", "Outputting"), ("Outputting", "Idle")]:
        logs.capture.wait(state(a, b))
    doc = library.dump(lib.wait_new()[-1])
    assert library.size(doc) == virtual_screen()[2:]


def test_monitor(capture, lib):
    start_with(capture, presets.preset("e2e monitor", "Monitor", editor=False))
    win32api.SetCursorPos((10, 10))           # the monitor under the cursor: the primary one
    run_preset(capture, "e2e monitor")
    doc = library.dump(lib.wait_new()[-1])
    assert library.size(doc) == win.screen_size()


def test_region_drag(capture, lib, target):
    start_with(capture, presets.preset("e2e region", "Region", editor=False))
    run_preset(capture, "e2e region")
    overlay()
    l, t, _, _ = target.rect
    win.drag([(l + 50, t + 60), (l + 350, t + 260)])
    doc = library.dump(lib.wait_new()[-1])
    w, h = library.size(doc)
    assert near(w, 300, 2) and near(h, 200, 2), (w, h)


def test_region_escape_cancels(capture, logs, lib):
    start_with(capture, presets.preset("e2e region", "Region", editor=False))
    run_preset(capture, "e2e region")
    win.foreground(overlay())
    win.keys("{ESC}")
    win.wait_no_window(OVERLAY)
    wait_idle(logs)
    apps.wait_quiet(1)
    assert not lib.new("*.snap"), "a canceled selection must not add anything to the library"


def test_window_pick(capture, lib, target):
    start_with(capture, presets.preset("e2e window", "Window", editor=False))
    run_preset(capture, "e2e window")
    overlay()
    x, y = target.client_center()
    win32api.SetCursorPos((x, y))
    apps.wait_quiet(0.4)
    win.click(x, y)
    doc = library.dump(lib.wait_new()[-1])
    l, t, r, b = target.rect
    w, h = library.size(doc)
    # the window frame (DWM extended bounds) can be a few pixels smaller than GetWindowRect
    assert near(w, r - l, 20) and near(h, b - t, 20), ((w, h), (r - l, b - t))


def test_fixed_size_region_follows_click(capture, lib, target):
    start_with(capture, presets.preset("e2e fixed", "FixedRegion", editor=False, fixed=(320, 180)))
    run_preset(capture, "e2e fixed")
    overlay()
    x, y = target.client_center()
    win32api.SetCursorPos((x, y))
    apps.wait_quiet(0.3)
    win.click(x, y)
    assert library.size(library.dump(lib.wait_new()[-1])) == (320, 180)


def test_fixed_region_with_position_needs_no_input(capture, lib, target):
    l, t, _, _ = target.rect
    start_with(capture, presets.preset("e2e fixedpos", "Region", editor=False, fixed=(240, 160), region_fixed=True,
                                       region_pos=(l + 20, t + 40)))
    run_preset(capture, "e2e fixedpos")
    doc = library.dump(lib.wait_new()[-1])
    assert library.size(doc) == (240, 160)
    assert not win.find_window(OVERLAY, visible_only=True)


def test_freehand(capture, lib, target):
    start_with(capture, presets.preset("e2e free", "Freehand", editor=False))
    run_preset(capture, "e2e free")
    overlay()
    l, t, _, _ = target.rect
    win.drag([(l + 100, t + 100), (l + 300, t + 90), (l + 320, t + 280), (l + 90, t + 300), (l + 100, t + 100)])
    w, h = library.size(library.dump(lib.wait_new()[-1]))
    assert 180 <= w <= 260 and 170 <= h <= 240, (w, h)


def test_multiple_areas(capture, lib, target):
    start_with(capture, presets.preset("e2e multi", "MultiArea", editor=False))
    run_preset(capture, "e2e multi")
    win.foreground(overlay())
    l, t, _, _ = target.rect
    win.drag([(l + 20, t + 20), (l + 120, t + 100)])
    win.drag([(l + 300, t + 200), (l + 420, t + 320)])
    win.keys("{ENTER}")
    w, h = library.size(library.dump(lib.wait_new()[-1]))
    assert w >= 390 and h >= 290, "the result spans the bounding box of all the areas"


def test_object_under_cursor(capture, lib, target):
    start_with(capture, presets.preset("e2e object", "Object", editor=False))
    run_preset(capture, "e2e object")
    overlay()
    x, y = target.client_center()
    win32api.SetCursorPos((x, y))
    apps.wait_quiet(0.5)
    win.click(x, y)
    w, h = library.size(library.dump(lib.wait_new()[-1]))
    assert 10 < w <= 620 and 10 < h <= 420


def pick_target_background(target):
    """A point of the window itself, away from the centered label (a child control of its own)."""
    l, t, _, _ = target.rect
    x, y = l + 40, t + 90
    win32api.SetCursorPos((x, y))
    apps.wait_quiet(0.4)
    win.click(x, y)


def test_extended_window_grows_beyond_its_size(capture, lib, target):
    l, t, r, b = target.rect
    start_with(capture, presets.preset("e2e ext", "ExtendedWindow", editor=False))
    run_preset(capture, "e2e ext")
    overlay()
    pick_target_background(target)
    form = win.wait_form(capture.pid)             # "Extended window": new width and height
    win.form_set(form, 1, 700)
    win.form_set(form, 2, 1500)
    win.form_ok(form)
    w, h = library.size(library.dump(lib.wait_new(timeout=30)[-1]))
    assert h > (b - t) + 200, f"captured {w}x{h}, window is {r - l}x{b - t}: it should have been extended"
    win.wait_until(lambda: abs((target.rect[3] - target.rect[1]) - (b - t)) <= 2, what="window restored to its size")


def test_scrolling_window(capture, lib, target):
    start_with(capture, presets.preset("e2e scroll", "Scrolling", editor=False))
    run_preset(capture, "e2e scroll")
    overlay()
    pick_target_background(target)
    w, h = library.size(library.dump(lib.wait_new(timeout=30)[-1]))
    l, t, r, b = target.rect
    # nothing to scroll: the result is the visible window
    assert w >= (r - l) * 0.8 and h >= (b - t) * 0.6, (w, h)



def pano_region(capture, target):
    start_with(capture, presets.preset("e2e pano", "Panoramic", editor=False))
    run_preset(capture, "e2e pano")
    overlay()
    l, t, _, _ = target.rect
    win.drag([(l + 50, t + 60), (l + 350, t + 260)])
    win.wait_window("SnapnabPanoBar", 10)            # the bar below the region, with the orange frame around it
    win.wait_window("SnapnabPanoBorder", 5)


def test_panoramic_without_scrolling_is_the_region(capture, lib, target):
    pano_region(capture, target)
    win.keys("{ENTER}")                               # Enter finishes
    w, h = library.size(library.dump(lib.wait_new()[-1]))
    assert near(w, 300, 2) and near(h, 200, 2), (w, h)
    assert not win.find_window("SnapnabPanoBar", visible_only=True)


def test_panoramic_esc_cancels(capture, logs, lib, target):
    pano_region(capture, target)
    win.keys("{ESC}")
    wait_idle(logs)
    assert not lib.new("*.snap")

def test_menu_without_open_menu_falls_back_to_region(capture, logs):
    start_with(capture, presets.preset("e2e menu", "Menu", editor=False))
    run_preset(capture, "e2e menu")
    win.foreground(overlay())
    win.keys("{ESC}")
    wait_idle(logs)


def test_capture_from_file(capture, lib, tmp_path):
    src = tmp_path / "from-file.png"
    Image.new("RGB", (271, 133), (1, 2, 3)).save(src)
    start_with(capture, presets.preset("e2e file", "File", editor=False))
    capture.command(CaptureCmd.IDM_PRESET_BASE)         # also covers running a preset from the menu
    from snapnab_e2e import filedialog
    filedialog.fill(capture.pid, src)
    assert library.size(library.dump(lib.wait_new()[-1])) == (271, 133)


def test_delay_shows_countdown(capture, logs, lib):
    start_with(capture, presets.preset("e2e delay", "FullScreen", editor=False, delay=2))
    t0 = time.time()
    run_preset(capture, "e2e delay")
    win.wait_window("SnapnabDelay", 5)
    lib.wait_new(timeout=15)
    assert time.time() - t0 >= 1.8, "the capture must wait for the delay"


def test_repeat_last_capture(capture, lib):
    start_with(capture, presets.preset("e2e full", "FullScreen", editor=False))
    run_preset(capture, "e2e full")
    lib.wait_new()
    capture.command(CaptureCmd.IDM_REPEAT)
    lib.wait_new(count=2)


def test_windows_as_objects(capture, lib, target):
    start_with(capture, presets.preset("e2e objs", "FullScreen", editor=False, window_objects=True))
    run_preset(capture, "e2e objs")
    doc = library.dump(lib.wait_new(timeout=30)[-1])
    names = [o.get("Name") for o in library.objects(doc) if o.tag == "Image"]
    assert target.title in names, f"each window becomes an Image object; got {names[:15]}"


# ---------- hotkeys ----------

def test_preset_hotkey(capture, lib):
    p = presets.preset("e2e hotkey", "FullScreen", editor=False)
    p = p.replace('HotKey="0" HotKeyModifiers="0"', 'HotKey="122" HotKeyModifiers="3"')   # Ctrl+Alt+F11
    start_with(capture, p)
    apps.wait_quiet(0.5)
    win.keys("^%{F11}")
    lib.wait_new()


def windows_print_screen_opens_snipping():
    """Windows 11 setting "Use the Print screen key to open screen capture" (on when the value is missing)."""
    import winreg
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Control Panel\Keyboard") as k:
            return winreg.QueryValueEx(k, "PrintScreenKeyForSnippingEnabled")[0] != 0
    except OSError:
        return True


def test_print_screen_runs_first_image_preset(capture, logs, lib):
    regs.set_dword("ImageHotkey", 0x2C)
    start_with(capture, presets.preset("e2e full", "FullScreen", editor=False))
    logs.capture.wait(r"Hotkey registered: id 1 = Print Screen|hook")
    apps.wait_quiet(0.5)
    win.keys("{PRTSC}")
    try:
        lib.wait_new()
    except AssertionError:
        snip = windows_print_screen_opens_snipping()
        win.keys("{ESC}")                    # closes the Snipping Tool overlay if it took the key
        raise AssertionError(
            "Print Screen did not start a Snapnab capture"
            + (" — Windows opened the Snipping Tool instead: RegisterHotKey succeeds while the Windows 11 setting "
               "'Use the Print screen key to open screen capture' is on, so the low-level hook fallback never engages"
               if snip else ""))


# ---------- outputs ----------

def test_output_opens_editor(capture, logs, lib):
    start_with(capture, presets.preset("e2e editor", "FullScreen", editor=True))
    run_preset(capture, "e2e editor")
    new = lib.wait_new()[-1]
    ed = apps.Editor()
    win.wait_until(lambda: win.is_visible(ed.hwnd), 15, what="Editor with the capture")
    logs.editor.wait(r"IPC: sent code=0x02")     # the Editor tells the capturer which library item it holds


def test_output_clipboard(capture, lib):
    win.clear_clipboard()
    start_with(capture, presets.preset("e2e clip", "Monitor", editor=False, clipboard=True))
    win32api.SetCursorPos((10, 10))
    run_preset(capture, "e2e clip")
    lib.wait_new()
    img = win.wait_until(win.clipboard_image, what="image on the clipboard")
    assert img.size == win.screen_size()


def test_after_capture_only_copy(capture, logs, lib):
    """Options › When done › Only copy to the clipboard (#3): neither the Editor nor the quick thumbnail, even with the
    preset's Editor on, and the clipboard even with CopyEveryCapture off."""
    regs.set_dword("AfterCaptureMode", 3)
    regs.set_dword("CopyEveryCapture", 0)
    try:
        win.clear_clipboard()
        start_with(capture, presets.preset("e2e only copy", "Monitor", editor=True))
        win32api.SetCursorPos((10, 10))
        run_preset(capture, "e2e only copy")
        lib.wait_new()
        img = win.wait_until(win.clipboard_image, what="image on the clipboard")
        assert img.size == win.screen_size()
        wait_idle(logs)
        time.sleep(1.5)
        assert not apps.Editor().running, "the Editor opened"
        assert not win.find_window("SnapnabQuickOverlay", visible_only=True), "the quick thumbnail appeared"
    finally:
        regs.delete("AfterCaptureMode")
        regs.delete("CopyEveryCapture")


def tray_order():
    """The Editor's Recent captures, front first (Tray.bin in the library: UTF-16 lines, hidden ones start with -)."""
    f = LIBRARY_DIR / "Tray.bin"
    text = f.read_bytes().decode("utf-16-le", "replace") if f.exists() else ""
    return [l.strip().lstrip("﻿").lower() for l in text.splitlines() if l.strip() and not l.startswith("-")]


def test_opening_a_tray_capture_keeps_its_place(capture, logs, lib):
    """Clicking an older capture in Recent captures opens it without moving it to the front (#2)."""
    start_with(capture, presets.preset("e2e tray", "Monitor", editor=False))
    apps.Editor().start()
    for n in range(1, 4):
        logs.capture.mark()
        run_preset(capture, "e2e tray")
        lib.wait_new(count=n)
        wait_idle(logs)
    shots = [str(p).lower() for p in lib.new("*.snap")]     # oldest first
    newest_first = shots[::-1]
    win.wait_until(lambda: tray_order()[:3] == newest_first, 15, what="the three captures at the front of the tray")
    logs.editor.mark()
    apps.run(EDITOR_EXE, "/Open", shots[0])                   # same path as a click on the thumbnail
    logs.editor.wait(r"IPC: sent code=0x02")
    apps.wait_quiet(1.0)
    assert tray_order()[:3] == newest_first

@pytest.mark.parametrize("fmt,magic", [("png", b"\x89PNG"), ("jpg", b"\xff\xd8"), ("bmp", b"BM"), ("gif", b"GIF8"),
                                       ("tif", b"II*\x00")])
def test_output_file_formats(capture, lib, tmp_path, target, fmt, magic):
    l, t, _, _ = target.rect
    start_with(capture, presets.preset("e2e save", "Region", editor=False, save_to=tmp_path, file_type=fmt,
                                       prefix="Shot", fixed=(200, 100), region_fixed=True, region_pos=(l + 10, t + 40)))
    run_preset(capture, "e2e save")
    f = win.wait_until(lambda: list(tmp_path.glob(f"Shot*.{fmt}")), what=f"{fmt} file")[0]
    win.wait_until(lambda: f.stat().st_size > 0, what="file written")
    assert f.read_bytes()[:len(magic)] in (magic, b"MM\x00*")
    with Image.open(f) as im:
        assert im.size == (200, 100)


def test_output_file_auto_names_do_not_overwrite(capture, tmp_path):
    start_with(capture, presets.preset("e2e save", "Monitor", editor=False, save_to=tmp_path, prefix="Auto"))
    run_preset(capture, "e2e save")
    win.wait_until(lambda: len(list(tmp_path.glob("Auto*.png"))) == 1, what="first file")
    run_preset(capture, "e2e save")
    win.wait_until(lambda: len(list(tmp_path.glob("Auto*.png"))) == 2, what="second file with a new name")


def test_output_external_program(capture, lib):
    before = {p.pid for p in apps.processes("mspaint.exe")}
    start_with(capture, presets.preset("e2e prog", "Monitor", editor=False, program="mspaint.exe"))
    run_preset(capture, "e2e prog")
    lib.wait_new()
    try:
        p = win.wait_until(lambda: [p for p in apps.processes("mspaint.exe") if p.pid not in before], 20,
                           what="Paint opened with the capture")[0]
        assert any("Snapnab_" in a for a in p.cmdline()), p.cmdline()
    finally:
        for p in apps.processes("mspaint.exe"):
            if p.pid not in before:
                try:
                    p.kill()
                except psutil.Error:
                    pass


def test_grab_text_ocr(capture, target):
    l, t, r, b = target.rect
    start_with(capture, presets.preset("e2e text", "Region", editor=False, grab_text=True, fixed=(r - l - 40, 160),
                                       region_fixed=True, region_pos=(l + 20, (t + b) // 2 - 60)))
    win.set_clipboard_text("")
    run_preset(capture, "e2e text")
    tw = win.wait_window("SnapnabTextWnd", 20)
    text = win.wait_until(win.clipboard_text, 10, what="recognized text on the clipboard")
    win.close_window(tw)
    assert "Target" in text or "E2E" in text, f"OCR text: {text!r}"
