"""Win32 helpers: windows, messages, input, clipboard and waits.

Snapnab's controls are custom HWNDs (SnapnabToggle, SnapnabButton, SnapnabCombo) that speak the standard control
messages and its menus are real HMENUs, so the suite drives it through control IDs and WM_COMMAND, with real mouse
input only where the program reads the mouse (crosshair, canvas).
"""
import contextlib
import ctypes
import ctypes.wintypes as wt
import io
import time

# per-monitor DPI awareness before anything reads coordinates, so the mouse lands where GetWindowRect says
try:
    ctypes.windll.user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
except Exception:
    pass

import win32api
import win32clipboard
import win32con
import win32gui
import win32process
from PIL import Image
from pywinauto import keyboard, mouse

from .config import ACTION_TIMEOUT

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

BM_GETCHECK = 0x00F0
CB_GETCURSEL = 0x0147
CB_SETCURSEL = 0x014E
CB_GETCOUNT = 0x0146
CBN_SELCHANGE = 1
BN_CLICKED = 0


class Timeout(AssertionError):
    pass


def wait_until(fn, timeout=ACTION_TIMEOUT, interval=0.15, what="condition"):
    """Polls fn() until it returns something truthy and returns it."""
    deadline = time.time() + timeout
    last_exc = None
    while time.time() < deadline:
        try:
            v = fn()
            if v:
                return v
        except Exception as e:  # the window may be half-built while polling
            last_exc = e
        time.sleep(interval)
    raise Timeout(f"timed out after {timeout}s waiting for {what}" + (f" (last error: {last_exc})" if last_exc else ""))


# ---------- windows ----------

def top_windows(cls=None, pid=None, visible_only=False):
    out = []

    def cb(h, _):
        if cls and win32gui.GetClassName(h) != cls:
            return True
        if visible_only and not win32gui.IsWindowVisible(h):
            return True
        if pid is not None and win32process.GetWindowThreadProcessId(h)[1] != pid:
            return True
        out.append(h)
        return True

    win32gui.EnumWindows(cb, None)
    return out


def find_window(cls, visible_only=False, pid=None):
    w = top_windows(cls, pid=pid, visible_only=visible_only)
    return w[0] if w else 0


def wait_window(cls, timeout=ACTION_TIMEOUT, visible_only=True, pid=None):
    return wait_until(lambda: find_window(cls, visible_only, pid), timeout, what=f"window {cls}")


def wait_no_window(cls, timeout=ACTION_TIMEOUT, visible_only=True):
    return wait_until(lambda: not find_window(cls, visible_only), timeout, what=f"window {cls} to close")


def window_pid(h):
    return win32process.GetWindowThreadProcessId(h)[1]


def is_visible(h):
    return bool(h) and bool(win32gui.IsWindow(h)) and bool(win32gui.IsWindowVisible(h))


def title(h):
    return win32gui.GetWindowText(h)


def rect(h):
    return win32gui.GetWindowRect(h)


def child_by_id(parent, ctl_id):
    try:
        return win32gui.GetDlgItem(parent, ctl_id)
    except Exception:
        return 0


def children(parent, cls=None):
    out = []

    def cb(h, _):
        if cls is None or win32gui.GetClassName(h) == cls:
            out.append(h)
        return True

    try:
        win32gui.EnumChildWindows(parent, cb, None)
    except Exception:
        pass
    return out


def owned_popups(pid, cls=None, visible_only=True):
    """Top-level windows of a process (forms, popups, message boxes)."""
    return top_windows(cls, pid=pid, visible_only=visible_only)


def post_command(h, cmd_id, notify=0, ctl=0):
    win32gui.PostMessage(h, win32con.WM_COMMAND, (notify << 16) | cmd_id, ctl)


def send_command(h, cmd_id, notify=0, ctl=0):
    return win32gui.SendMessage(h, win32con.WM_COMMAND, (notify << 16) | cmd_id, ctl)


def send(h, msg, wp=0, lp=0):
    return win32gui.SendMessage(h, msg, wp, lp)


def get_check(h):
    return send(h, BM_GETCHECK) == 1


def combo_sel(h):
    return send(h, CB_GETCURSEL)


def combo_count(h):
    return send(h, CB_GETCOUNT)


def set_combo(parent, ctl_id, index):
    """Selects an item the way a user would: CB_SETCURSEL + CBN_SELCHANGE to the parent."""
    h = child_by_id(parent, ctl_id)
    send(h, CB_SETCURSEL, index)
    send_command(parent, ctl_id, CBN_SELCHANGE, h)


def click_control(h):
    """BN_CLICKED to the parent, as the custom buttons and toggles notify it."""
    parent = win32gui.GetParent(h)
    send_command(parent, win32gui.GetDlgCtrlID(h), BN_CLICKED, h)


def close_window(h):
    if h and win32gui.IsWindow(h):
        win32gui.PostMessage(h, win32con.WM_CLOSE, 0, 0)


def foreground(h):
    """Brings a window to the front only if it is not there already.

    No Alt tap to get past the foreground lock: Snapnab's title bar treats Alt as menu activation and would swallow
    the next key or click. Attaching to the foreground thread's input does the same job without a keystroke."""
    try:
        fg = win32gui.GetForegroundWindow()
        if fg == h or (fg and win32gui.GetAncestor(fg, 3) == h):      # GA_ROOTOWNER
            return
        win32gui.ShowWindow(h, win32con.SW_SHOW)
        me = kernel32.GetCurrentThreadId()
        other = user32.GetWindowThreadProcessId(fg, None) if fg else 0
        if other and other != me:
            user32.AttachThreadInput(me, other, True)
        try:
            user32.BringWindowToTop(h)
            win32gui.SetForegroundWindow(h)
        finally:
            if other and other != me:
                user32.AttachThreadInput(me, other, False)
    except Exception:
        pass


def foreground_window():
    return win32gui.GetForegroundWindow()


def class_name(h):
    try:
        return win32gui.GetClassName(h)
    except Exception:
        return ""


# ---------- SnapnabForm (rust/snapui/src/form.rs: field i = control 1000 + i, OK = 1, Cancel = 2) ----------

def wait_form(pid=None, timeout=ACTION_TIMEOUT):
    return wait_window("SnapnabForm", timeout, pid=pid)


def form_set(form, index, text):
    h = child_by_id(form, 1000 + index)
    if class_name(h) != "Edit":         # numeric fields can be an Edit inside a container
        edits = children(h, "Edit")
        h = edits[0] if edits else h
    win32gui.SendMessage(h, win32con.WM_SETTEXT, 0, str(text))


def form_get(form, index):
    return win32gui.GetWindowText(child_by_id(form, 1000 + index))


def form_ok(form):
    post_command(form, 1)


def form_cancel(form):
    post_command(form, 2)


# ---------- input ----------

def screen_size():
    return win32api.GetSystemMetrics(0), win32api.GetSystemMetrics(1)


def virtual_screen_size():
    """All monitors together (SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN)."""
    return win32api.GetSystemMetrics(78), win32api.GetSystemMetrics(79)


def click(x, y, button="left"):
    mouse.click(button=button, coords=(int(x), int(y)))


def drag(points, button="left", step_delay=0.02, hold=0.0):
    """Holds the button along a path of screen points (crosshair selections, canvas drawing); hold keeps the mouse
    still at the end before releasing."""
    pts = [(int(x), int(y)) for x, y in points]
    mouse.move(coords=pts[0])
    time.sleep(0.05)
    mouse.press(button=button, coords=pts[0])
    for a, b in zip(pts, pts[1:]):
        for i in range(1, 6):   # intermediate moves so WM_MOUSEMOVE arrives as from a real hand
            mouse.move(coords=(a[0] + (b[0] - a[0]) * i // 5, a[1] + (b[1] - a[1]) * i // 5))
            time.sleep(step_delay)
    time.sleep(hold)
    mouse.release(button=button, coords=pts[-1])
    time.sleep(0.1)


def keys(spec, pause=0.03):
    keyboard.send_keys(spec, pause=pause, with_spaces=True)


@contextlib.contextmanager
def held(vk):
    """Holds a modifier key down (e.g. VK_SHIFT) around mouse input; released even if the body fails."""
    win32api.keybd_event(vk, 0, 0, 0)
    time.sleep(0.05)
    try:
        yield
    finally:
        win32api.keybd_event(vk, 0, win32con.KEYEVENTF_KEYUP, 0)
        time.sleep(0.05)


# ---------- clipboard ----------

def _open_clipboard():
    wait_until(lambda: _try_open(), 5, what="clipboard")


def _try_open():
    try:
        win32clipboard.OpenClipboard()
        return True
    except Exception:
        return False


def set_clipboard_image(img: Image.Image):
    buf = io.BytesIO()
    img.convert("RGB").save(buf, "BMP")
    dib = buf.getvalue()[14:]          # BMP without the file header = CF_DIB
    _open_clipboard()
    try:
        win32clipboard.EmptyClipboard()
        win32clipboard.SetClipboardData(win32clipboard.CF_DIB, dib)
    finally:
        win32clipboard.CloseClipboard()


def set_clipboard_text(text):
    _open_clipboard()
    try:
        win32clipboard.EmptyClipboard()
        win32clipboard.SetClipboardData(win32clipboard.CF_UNICODETEXT, text)
    finally:
        win32clipboard.CloseClipboard()


def clear_clipboard():
    _open_clipboard()
    try:
        win32clipboard.EmptyClipboard()
    finally:
        win32clipboard.CloseClipboard()


def clipboard_sequence():
    return user32.GetClipboardSequenceNumber()


def clipboard_formats():
    _open_clipboard()
    try:
        out, f = [], 0
        while True:
            f = win32clipboard.EnumClipboardFormats(f)
            if not f:
                return out
            out.append(f)
    finally:
        win32clipboard.CloseClipboard()


def clipboard_text():
    _open_clipboard()
    try:
        if win32clipboard.IsClipboardFormatAvailable(win32clipboard.CF_UNICODETEXT):
            return win32clipboard.GetClipboardData(win32clipboard.CF_UNICODETEXT)
        return None
    finally:
        win32clipboard.CloseClipboard()


def clipboard_image():
    from PIL import ImageGrab
    for _ in range(10):
        try:
            img = ImageGrab.grabclipboard()
            return img if isinstance(img, Image.Image) else None
        except OSError:
            time.sleep(0.2)
    return None
