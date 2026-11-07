"""Reads what the programs write to %LOCALAPPDATA%\\Snapnab\\Logs: the IPC messages
("IPC: sent/received code=0x..") and the capture engine states ("CaptureEngine: A → B") are the oracle of the
cooperation between the two executables."""
import re
import time

from .config import ACTION_TIMEOUT, CAPTURE_LOG, EDITOR_LOG


class LogWatcher:
    """Remembers where a log ended and looks only at what was written after that."""

    def __init__(self, path):
        self.path = path
        self.mark()

    def mark(self):
        self.start = self.path.stat().st_size if self.path.exists() else 0

    def text(self):
        if not self.path.exists():
            return ""
        with open(self.path, "rb") as f:
            f.seek(self.start)
            return f.read().decode("utf-8", "replace")

    def lines(self):
        return [l for l in self.text().splitlines() if l.strip()]

    def find(self, pattern):
        rx = re.compile(pattern)
        return [l for l in self.lines() if rx.search(l)]

    def wait(self, pattern, timeout=ACTION_TIMEOUT, count=1):
        deadline = time.time() + timeout
        while time.time() < deadline:
            hits = self.find(pattern)
            if len(hits) >= count:
                return hits
            time.sleep(0.15)
        tail = "\n".join(self.lines()[-15:])
        raise AssertionError(f"{self.path.name}: '{pattern}' did not appear in {timeout}s. Last lines:\n{tail}")

    def assert_absent(self, pattern):
        hits = self.find(pattern)
        assert not hits, f"{self.path.name}: unexpected '{pattern}':\n" + "\n".join(hits)


def ipc_sent(code, dest=None):
    d = rf" → {dest}" if dest is not None else ""
    return rf"IPC: sent code=0x{code:02X} kind=\d{d}"


def ipc_received(code):
    return rf"IPC: received code=0x{code:02X} kind=\d"


def state(a, b):
    return rf"CaptureEngine: {a} → {b}"


class Logs:
    def __init__(self):
        self.capture = LogWatcher(CAPTURE_LOG)
        self.editor = LogWatcher(EDITOR_LOG)

    def mark(self):
        self.capture.mark()
        self.editor.mark()


# IPC codes (rust/snapcore/src/ipc.rs)
SHOW_EDITOR = 0x06
CLOSE_EDITOR_IF_HIDDEN = 0x0A
RELOAD_CONFIG = 0x0C
NEW_IMAGE_FROM_CLIPBOARD = 0x1E
OPEN_FILE = 0x30
LIBRARY_CHANGED = 0x31
RESTART_EDITOR = 0x32
LIBRARY_DOC_OPENED = 0x02
LIBRARY_DOC_RELEASED = 0x03
EDITOR_START_CAPTURE = 0x12
EDITOR_CLOSED = 0x13
