"""The controls of an egui window through UI Automation.

egui draws its widgets itself (there are no child windows); AccessKit publishes them to UI Automation, each with its
AutomationId set to the name of the C++ control it replaces ("IDC_SELECTION"), and the items of a list or drop-down
with "<name>.<index>" ("IDC_SELECTION.2"). The tests click them with the real mouse, as a user would.
"""
import re

from pywinauto import Desktop

from . import win


def window(hwnd):
    return Desktop(backend="uia").window(handle=hwnd)


def _all(hwnd):
    try:
        return window(hwnd).descendants()
    except Exception:
        return []


def _aid(e):
    try:
        return e.element_info.automation_id or ""
    except Exception:
        return ""


def query(hwnd, aid):
    """The element with this AutomationId, or None."""
    return next((e for e in _all(hwnd) if _aid(e) == aid), None)


def find(hwnd, aid, timeout=10):
    return win.wait_until(lambda: query(hwnd, aid), timeout, what=f"UI Automation element {aid}")


def exists(hwnd, aid):
    return query(hwnd, aid) is not None


def items(hwnd, name):
    """The items "<name>.<index>" shown now (a drop-down that is open, the preset list)."""
    pat = re.compile(re.escape(name) + r"\.\d+$")
    return [e for e in _all(hwnd) if pat.match(_aid(e))]


def _stable_rect(hwnd, aid, timeout):
    """The element's rectangle once it stops moving (the window may still be resizing to its content)."""
    last = None

    def same():
        nonlocal last
        r = find(hwnd, aid, timeout).rectangle()
        r = (r.left, r.top, r.right, r.bottom)
        ok = r == last and r[2] > r[0]
        last = r
        return r if ok else None
    return win.wait_until(same, timeout, interval=0.1, what=f"{aid} in place")


def click(hwnd, aid, timeout=10):
    win.foreground(hwnd)
    l, t, r, b = _stable_rect(hwnd, aid, timeout)
    win.click((l + r) // 2, (t + b) // 2)


def toggled(hwnd, aid):
    return find(hwnd, aid).get_toggle_state() == 1


def value(hwnd, aid):
    e = find(hwnd, aid)
    try:
        return e.iface_value.CurrentValue
    except Exception:
        return e.window_text()
