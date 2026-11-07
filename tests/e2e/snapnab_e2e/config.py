"""Paths and timeouts for the E2E suite (overridable through environment variables)."""
import os
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
E2E_DIR = Path(__file__).resolve().parents[1]

# The installed program is tested by default; SNAPNAB_DIR=dist\Snapnab tests a fresh build instead
APP_DIR = Path(os.environ.get("SNAPNAB_DIR", r"C:\Program Files\Snapnab"))
CAPTURE_EXE = APP_DIR / "Snapnab.exe"
EDITOR_EXE = APP_DIR / "SnapnabEditor.exe"
# SnapnabTool.exe (rust/snaptool): "dump file.snap" prints the document XML, the oracle of the Editor tests, and
# "audio file.mp4" reads a recording's audio track
TOOL_EXE = Path(os.environ.get("SNAPNAB_TOOL_EXE", REPO / "rust" / "target" / "release" / "SnapnabTool.exe"))
RUST_DIR = REPO / "rust"

LOCALAPPDATA = Path(os.environ["LOCALAPPDATA"])
DATA_DIR = LOCALAPPDATA / "Snapnab"
LIBRARY_DIR = DATA_DIR / "Library"
LOGS_DIR = DATA_DIR / "Logs"
CAPTURE_LOG = LOGS_DIR / "Snapnab.log"
EDITOR_LOG = LOGS_DIR / "SnapnabEditor.log"
PICTURES_DIR = Path(os.environ["USERPROFILE"]) / "Pictures" / "Snapnab"
REG_KEY = r"HKCU\Software\Snapnab"

ARTIFACTS = E2E_DIR / "artifacts"
LAUNCH_TIMEOUT = float(os.environ.get("SNAPNAB_LAUNCH_TIMEOUT", "20"))
ACTION_TIMEOUT = float(os.environ.get("SNAPNAB_ACTION_TIMEOUT", "10"))

# window classes and single-instance mutexes (rust/snapcore/src/ipc.rs)
CAPTURE_CLASS = "SnapnabUI"
EDITOR_CLASS = "SnapnabEditor"
CAPTURE_MUTEX = "Snapnab-Capture-{41F1BF9C-5A4E-4B6F-9C33-1D2E3F405A61}"
EDITOR_MUTEX = "Snapnab-Editor-{800F2EB3-7C1D-4E2A-A3B4-C5D6E7F80912}"
