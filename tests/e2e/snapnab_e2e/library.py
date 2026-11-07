"""The capture library (%LOCALAPPDATA%\\Snapnab\\Library) and the .snap documents in it."""
import subprocess
import time
import xml.etree.ElementTree as ET
from pathlib import Path

from . import win
from .config import ACTION_TIMEOUT, LIBRARY_DIR, TOOL_EXE


def files(pattern="*"):
    return set(LIBRARY_DIR.glob(pattern)) if LIBRARY_DIR.exists() else set()


class LibraryWatcher:
    def __init__(self):
        self.before = {p: p.stat().st_mtime for p in files()}

    def new(self, pattern="*"):
        return sorted((p for p in files(pattern) if p not in self.before), key=lambda p: p.stat().st_mtime)

    def changed(self, pattern="*"):
        out = []
        for p in files(pattern):
            try:
                if p not in self.before or p.stat().st_mtime != self.before[p]:
                    out.append(p)
            except FileNotFoundError:
                pass
        return out

    def wait_new(self, pattern="*.snap", count=1, timeout=ACTION_TIMEOUT):
        return win.wait_until(lambda: (lambda n: n if len(n) >= count else None)(self.new(pattern)), timeout,
                              what=f"{count} new {pattern} in the library")


def dump(path) -> ET.Element:
    """XML of a .snap through 'SnapnabTool.exe dump' (the same reader the program uses)."""
    assert TOOL_EXE.exists(), f"{TOOL_EXE} not found: run build.bat, or set SNAPNAB_TOOL_EXE"
    for _ in range(20):   # the program may be writing the file right now
        r = subprocess.run([str(TOOL_EXE), "dump", str(path)], capture_output=True, timeout=30)
        out = r.stdout.decode("utf-8", "replace")
        if r.returncode == 0 and "<Document" in out:
            return ET.fromstring(out[out.index("<?xml"):] if "<?xml" in out else out)
        time.sleep(0.25)
    raise AssertionError(f"could not read {path}: {out[:300]}")


def objects(doc: ET.Element):
    objs = doc.find("Objects")
    return list(objs) if objs is not None else []


def object_tags(doc):
    return [o.tag for o in objects(doc)]


def size(doc):
    return int(doc.get("Width")), int(doc.get("Height"))


def wait_doc(path, cond, timeout=ACTION_TIMEOUT, what="document state"):
    """Re-reads the .snap until cond(doc) holds (the Editor writes it on Save/autosave)."""
    last = {}

    def check():
        last["doc"] = dump(path)
        return cond(last["doc"])

    try:
        win.wait_until(check, timeout, interval=0.4, what=what)
    except AssertionError:
        d = last.get("doc")
        raise AssertionError(f"{what} not reached in {Path(path).name}; last XML:\n" +
                             (ET.tostring(d, encoding="unicode")[:2000] if d is not None else "<none>"))
    return last["doc"]
