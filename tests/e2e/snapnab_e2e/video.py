"""Screen recording through the HUD (rust/snapnab/src/hud.rs): its buttons are clicked like a user would; the
Shift+F9 / Shift+F10 hotkeys are tested separately because another program may own them."""
import ctypes
import subprocess
import time

from . import presets, win
from .config import TOOL_EXE
from .library import LibraryWatcher
from .logs import Logs

HUD = "SnapnabRecorderHud"
# RecorderHud::Layout: 36 px buttons, 4 px gap, 8 px padding, 10 px between groups
_ORDER = [("record", 0), ("stop", 0), ("restart", 0), ("cancel", 0), ("mic", 1), ("sys", 1), ("volume", 1)]


def hud_button(hud, name, webcam=False):
    s = ctypes.windll.user32.GetDpiForWindow(hud) / 96.0
    b, gap, pad, sep = 36 * s, 4 * s, 8 * s, 10 * s
    order = list(_ORDER) + ([("cam", 1)] if webcam else []) + [("snapshot", 2)]
    x = pad
    group = 0
    for n, g in order:
        if g != group:
            x += sep
            group = g
        if n == name:
            l, t, _, _ = win.rect(hud)
            return int(l + x + b / 2), int(t + pad + b / 2)
        x += b + gap
    raise KeyError(name)


def click(hud, name):
    win.click(*hud_button(hud, name))
    time.sleep(0.3)


def video_preset(name="e2e video", editor=False, **kw):
    kw.setdefault("mic", False)
    kw.setdefault("sys_audio", False)
    return presets.preset(name, "FullScreen", "Video", editor=editor, **kw)


def arm(capture, name="e2e video"):
    """Runs the video preset: the HUD opens over the area and waits for the start."""
    assert capture.send("/preset", name) == 0
    hud = win.wait_window(HUD, 15)
    time.sleep(0.5)
    return hud


def start(hud, logs: Logs):
    click(hud, "record")                  # countdown
    win.wait_window("SnapnabCountdown", 5)
    click(hud, "record")                  # record during the countdown = start right away
    logs.capture.wait(r"Recorder: started", 15)


def stop(hud, logs: Logs):
    click(hud, "stop")
    logs.capture.wait(r"Recorder: stopped \(ok\)", 30)


def record(capture, logs: Logs, seconds=2.0, name="e2e video"):
    lib = LibraryWatcher()
    hud = arm(capture, name)
    start(hud, logs)
    time.sleep(seconds)
    stop(hud, logs)
    return lib.wait_new("Video_*.mp4", timeout=20)[-1]


def duration(path):
    """Seconds, from the Windows property system (System.Media.Duration, 100 ns units)."""
    from win32com.propsys import propsys, pscon
    for _ in range(20):
        try:
            store = propsys.SHGetPropertyStoreFromParsingName(str(path))
            v = store.GetValue(pscon.PKEY_Media_Duration).GetValue()
            if v:
                return v / 1e7
        except Exception:
            pass
        time.sleep(0.25)
    raise AssertionError(f"no duration for {path}")


def frame_size(path):
    from win32com.propsys import propsys, pscon
    store = propsys.SHGetPropertyStoreFromParsingName(str(path))
    return store.GetValue(pscon.PKEY_Video_FrameWidth).GetValue(), store.GetValue(pscon.PKEY_Video_FrameHeight).GetValue()


def audio_report(path):
    r = subprocess.run([str(TOOL_EXE), "audio", str(path)], capture_output=True, timeout=60)
    return r.stdout.decode("utf-8", "replace")
