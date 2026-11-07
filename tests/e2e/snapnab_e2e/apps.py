"""Starts, drives and stops Snapnab.exe (capturer) and SnapnabEditor.exe (editor)."""
import subprocess
import time

import psutil
import win32con
import win32event
import win32gui

from . import win
from .config import (ACTION_TIMEOUT, CAPTURE_CLASS, CAPTURE_EXE, CAPTURE_MUTEX, EDITOR_CLASS, EDITOR_EXE, EDITOR_MUTEX,
                     LAUNCH_TIMEOUT)

SYNCHRONIZE = 0x00100000

# command IDs (rust/snapnab/src/lib.rs, mod id)
class CaptureCmd:
    IDC_PRESETS, IDC_TAB_IMAGE, IDC_TAB_VIDEO, IDC_SEL_LABEL, IDC_SELECTION, IDC_EFFECTS_TITLE, IDC_EFFECTS = range(100, 107)
    (IDC_SHARE_LABEL, IDC_SHARE, IDC_QUALITY_LABEL, IDC_QUALITY, IDC_WEBCAM_LABEL, IDC_WEBCAM, IDC_PREVIEW, IDC_CLIPBOARD,
     IDC_CURSOR, IDC_DELAY, IDC_MIC, IDC_SYS, IDC_CAPTURE, IDC_PRESETS_LABEL, IDC_PRESET_ADD, IDC_OPEN_EDITOR) = range(107, 123)
    (IDM_EXIT, IDM_OPEN_EDITOR, IDM_CAPTURE_IMAGE, IDM_CAPTURE_VIDEO, IDM_CAPTURE_CLIPBOARD, IDM_CAPTURE_FILE, IDM_REPEAT,
     IDM_OPTIONS, IDM_PRESET_IMPORT, IDM_PRESET_EXPORT, IDM_ABOUT, IDM_LIBRARY_FOLDER, IDM_LOGS, IDM_SHOW, IDM_ONECLICK,
     IDM_SHOW_PRESETS, IDM_PRESET_FROM_CURRENT, IDM_PRESET_NEW, IDM_PRESET_HELP, IDM_PRESET_RUN, IDM_PRESET_RENAME,
     IDM_PRESET_DUP, IDM_PRESET_HOTKEY, IDM_PRESET_DEL, IDM_SETTINGS_EXPORT, IDM_SETTINGS_IMPORT,
     IDM_SETTINGS_BACKUPS) = range(1000, 1027)
    IDM_PRESET_BASE, IDM_DELAY_BASE, IDM_WEBCAM_OFF = 2000, 3000, 3100
    IDM_REGION_INTERACTIVE, IDM_REGION_FIXED, IDM_WINDOW_OBJECTS, IDM_CAPTURE_LINKS = range(3400, 3404)


# command IDs (rust/snapedit/src/ids.rs)
class EditorCmd:
    T_CAPTURE, T_RECORD, T_COPYALL, T_SAVE, T_SHARE = range(1, 6)
    T_TOOL_BASE = 100
    _names = """M_NEW_CAPTURE M_NEW_VIDEO M_NEW_CLIPBOARD M_NEW_IMAGE M_OPEN M_SAVE M_SAVEAS M_DUPLICATE M_DELETE M_PRINT
    M_EMAIL M_OPEN_PROGRAM M_LIBRARY M_CLOSE M_EXIT
    M_UNDO M_REDO M_UNDOALL M_CUT M_COPY M_COPYALL M_PASTE M_DELETE_SEL M_SELECTALL M_FLATTEN M_FRONT M_BACK M_OCR
    M_CROP_SEL M_ROT_CW M_ROT_CCW M_ROT_180 M_ROT_FREE M_FLIP_H M_FLIP_V M_RESIZE M_CANVAS M_CANVAS_COLOR M_TRIM
    M_FX_BORDER M_FX_SHADOW M_FX_EDGE M_FX_GRAY M_FX_INVERT M_FX_SEPIA M_FX_BC M_FX_HS M_FX_REPLACE M_FX_WATERMARK M_FX_CAPTION
    M_FX_MONO
    M_ZOOM_IN M_ZOOM_OUT M_ZOOM_100 M_ZOOM_FIT M_BG_COLOR M_SHOW_TRAY M_ABOUT M_SHORTCUTS M_SHOW_EFFECTS
    M_FX_PERSPECTIVE M_FX_PAGECURL M_FX_FILTER M_FX_ADJUST M_FX_SPOTLIGHT M_SHOW_LIBRARY
    M_BRING_FORWARD M_SEND_BACKWARD M_ALIGN_LEFT M_ALIGN_CENTER M_ALIGN_RIGHT M_ALIGN_TOP M_ALIGN_MIDDLE M_ALIGN_BOTTOM
    M_GROUP M_UNGROUP M_FLATTEN_SEL M_APPLY_TEMPLATE M_SNAPPING M_TAG
    M_DIST_H M_DIST_V M_PASTE_INPLACE M_LOCK M_UNLOCK M_LIFT M_DROP M_RULERS M_EDGESNAP M_PLAINPASTE M_RENAME_OBJ
    M_SHOW_LAYERS M_CLEAR_GUIDES M_OPEN_LINK""".split()
    M_TB_CUSTOMIZE, M_TB_RESET, M_MORE_TOOL_BASE = 1500, 1501, 1600


for _i, _n in enumerate(EditorCmd._names):
    setattr(EditorCmd, _n, 1000 + _i)

# Tool enum (rust/snapedit/src/canvas.rs)
TOOLS = ["Select", "Arrow", "Line", "Shape", "Pen", "Highlighter", "Text", "Callout", "Step", "Stamp", "Blur",
         "Spotlight", "Magnify", "Fill", "Eraser", "Crop", "Selection", "Cutout", "Eyedropper", "Favorites", "MagicWand"]


def mutex_exists(name):
    try:
        h = win32event.OpenMutex(SYNCHRONIZE, False, name)
        h.Close()
        return True
    except Exception:
        return False


def processes(exe_name):
    out = []
    for p in psutil.process_iter(["name"]):
        if (p.info["name"] or "").lower() == exe_name.lower():
            out.append(p)
    return out


def kill_all():
    for name in ("Snapnab.exe", "SnapnabEditor.exe"):
        for p in processes(name):
            try:
                p.kill()
            except psutil.Error:
                pass
    win.wait_until(lambda: not processes("Snapnab.exe") and not processes("SnapnabEditor.exe"), 10, what="processes to end")
    win.wait_until(lambda: not mutex_exists(CAPTURE_MUTEX) and not mutex_exists(EDITOR_MUTEX), 5, what="mutexes released")


def run(exe, *args, wait=True, timeout=LAUNCH_TIMEOUT):
    p = subprocess.Popen([str(exe), *args])
    if wait:
        p.wait(timeout=timeout)
    return p


class Capture:
    """Snapnab.exe: the main window (class SnapnabUI) exists even when hidden in the tray."""

    def __init__(self):
        self.proc = None

    @property
    def hwnd(self):
        return win.find_window(CAPTURE_CLASS)

    @property
    def running(self):
        return mutex_exists(CAPTURE_MUTEX)

    @property
    def pid(self):
        ps = processes("Snapnab.exe")
        return ps[0].pid if ps else None

    def start(self, *args, hidden=True):
        """First instance; /hide keeps the window in the tray, as when started with Windows."""
        a = list(args) or (["/hide"] if hidden else [])
        self.proc = run(CAPTURE_EXE, *a, wait=False)
        win.wait_until(lambda: self.running and self.hwnd, LAUNCH_TIMEOUT, what="Snapnab.exe to start")
        return self

    def send(self, *args):
        """Second instance: forwards its command line to the first one (WM_COPYDATA 0x8010) and exits."""
        p = run(CAPTURE_EXE, *args, wait=False)
        p.wait(timeout=LAUNCH_TIMEOUT)
        return p.returncode

    def command(self, cmd_id):
        win.post_command(self.hwnd, cmd_id)

    def show(self):
        self.command(CaptureCmd.IDM_SHOW)
        win.wait_until(lambda: win.is_visible(self.hwnd), what="capture window visible")
        return self.hwnd

    def ctl(self, ctl_id):
        return win.child_by_id(self.hwnd, ctl_id)

    def popups(self, cls=None):
        return win.owned_popups(self.pid, cls) if self.pid else []

    def exit(self, timeout=10):
        if not self.running:
            return
        self.send("/exit")
        try:
            win.wait_until(lambda: not processes("Snapnab.exe"), timeout, what="Snapnab.exe to exit")
        except AssertionError:
            for p in processes("Snapnab.exe"):
                p.kill()
            raise


class Editor:
    def __init__(self):
        self.proc = None

    @property
    def hwnd(self):
        return win.find_window(EDITOR_CLASS)

    @property
    def running(self):
        return mutex_exists(EDITOR_MUTEX)

    @property
    def pid(self):
        ps = processes("SnapnabEditor.exe")
        return ps[0].pid if ps else None

    def start(self, *args):
        self.proc = run(EDITOR_EXE, *args, wait=False)
        win.wait_until(lambda: self.running and win.is_visible(self.hwnd), LAUNCH_TIMEOUT, what="Editor window")
        return self

    def open(self, path):
        """Through the command line, the same way Explorer and the capturer open files."""
        run(EDITOR_EXE, "/Open", str(path), wait=False)

    def command(self, cmd_id):
        win.post_command(self.hwnd, cmd_id)

    def send_command(self, cmd_id):
        return win.send_command(self.hwnd, cmd_id)

    def tool(self, name):
        self.send_command(EditorCmd.T_TOOL_BASE + TOOLS.index(name))

    def canvas_rect(self):
        """The canvas area on the screen (egui draws it inside the main window; UI Automation finds it)."""
        from . import uia
        r = uia.find(self.hwnd, "SnapnabCanvas").rectangle()
        return r.left, r.top, r.right, r.bottom

    def popups(self, cls=None):
        return win.owned_popups(self.pid, cls) if self.pid else []

    def title(self):
        return win.title(self.hwnd)

    def exit(self, timeout=10):
        if not self.running:
            return
        h = self.hwnd
        if h:
            win32gui.PostMessage(h, win32con.WM_CLOSE, 0, 0)
        try:
            win.wait_until(lambda: not processes("SnapnabEditor.exe"), timeout, what="Editor to exit")
        except AssertionError:
            for p in processes("SnapnabEditor.exe"):
                p.kill()
            raise


def wait_quiet(seconds=0.4):
    """Lets the message loops settle after a command (only between steps, never as the synchronization itself)."""
    time.sleep(seconds)
