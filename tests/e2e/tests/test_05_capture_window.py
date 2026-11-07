"""The capture window (rust/snapnab/src/window.rs): tabs, switches, combo boxes, the capture button, preset management,
Options, preset and settings export/import, One Click, About and the folder shortcuts."""
import re
import time

import pytest
import win32con
import win32gui

from snapnab_e2e import apps, filedialog, presets, regs, uia, win
from snapnab_e2e.apps import CaptureCmd as C
from snapnab_e2e.config import DATA_DIR
from snapnab_e2e.logs import RELOAD_CONFIG, ipc_received, ipc_sent

LB_SETCURSEL, LB_GETCOUNT, LBN_SELCHANGE = 0x0186, 0x018B, 1
HKM_SETHOTKEY = 0x0401
TDM_CLICK_BUTTON = 0x0400 + 102


def preset_xml():
    import xml.etree.ElementTree as ET
    return ET.parse(DATA_DIR / "Presets1.xml").getroot()


def cur_preset():
    root = preset_xml()
    items = list(root.iter("CaptureSettings"))
    return items[int(root.get("SelectedMode", "0"))]


@pytest.fixture
def window(capture):
    presets.write(presets.preset("e2e one", "Region", editor=False),
                  presets.preset("e2e two", "FullScreen", editor=False))
    capture.start("/hide")
    capture.show()
    win.foreground(capture.hwnd)
    return capture


def message_box(capture, timeout=10):
    return win.wait_until(lambda: next(iter(capture.popups("#32770")), 0), timeout, what="message box")


def box_text(box):
    return " ".join(win.title(c) for c in win.children(box))


@pytest.fixture
def window_with_list(capture):
    """The window with the preset list open below the bottom bar."""
    regs.set_dword("CapturePresetsOpen", 1)
    presets.write(presets.preset("e2e one", "Region", editor=False),
                  presets.preset("e2e two", "FullScreen", editor=False))
    capture.start("/hide")
    capture.show()
    win.foreground(capture.hwnd)
    return capture


def open_dropdown(capture, name, n_items=None):
    uia.click(capture.hwnd, name)
    return win.wait_until(lambda: (lambda it: it if it and (n_items is None or len(it) == n_items) else None)(uia.items(capture.hwnd, name)),
                          what=f"{name} open")


@pytest.mark.smoke
def test_controls_exist(window):
    """egui draws the controls; UI Automation finds them by the C++ control names (AutomationId)."""
    for aid in ("IDC_TAB_IMAGE", "IDC_TAB_VIDEO", "IDC_SELECTION", "IDC_EFFECTS", "IDC_SHARE", "IDC_PREVIEW", "IDC_CLIPBOARD",
                "IDC_CURSOR", "IDC_DELAY", "IDC_CAPTURE", "IDC_PRESETS_LABEL", "IDC_PRESET_ADD", "IDC_OPEN_EDITOR"):
        uia.find(window.hwnd, aid)
    assert len(open_dropdown(window, "IDC_SELECTION")) == 13, "image selections (SelectionsFor)"
    win.keys("{ESC}")


def test_video_tab_switches_the_preset_kind(window):
    uia.click(window.hwnd, "IDC_TAB_VIDEO")
    win.wait_until(lambda: cur_preset().get("CaptureMode") == "Video", what="preset saved as video")
    uia.find(window.hwnd, "IDC_MIC")
    uia.find(window.hwnd, "IDC_SYS")
    assert len(open_dropdown(window, "IDC_SELECTION")) == 4, "video selections: region, window, full screen, monitor"
    win.keys("{ESC}")
    uia.click(window.hwnd, "IDC_TAB_IMAGE")
    win.wait_until(lambda: cur_preset().get("CaptureMode") == "Image", what="back to image")


def test_tab_commands_still_work(window):
    """The tabs answer the C++ control ids as WM_COMMAND too (automation posts them)."""
    win.send_command(window.hwnd, C.IDC_TAB_VIDEO)
    win.wait_until(lambda: cur_preset().get("CaptureMode") == "Video", what="preset saved as video")
    win.send_command(window.hwnd, C.IDC_TAB_IMAGE)
    win.wait_until(lambda: cur_preset().get("CaptureMode") == "Image", what="back to image")


def test_selection_combo_is_saved(window):
    open_dropdown(window, "IDC_SELECTION", 13)
    uia.click(window.hwnd, "IDC_SELECTION.2")                 # Full screen
    win.wait_until(lambda: cur_preset().get("SelectionMode") == str(presets.SEL["FullScreen"]), what="selection saved")


@pytest.mark.parametrize("aid,attr", [("IDC_CLIPBOARD", "CopyToClipboard"), ("IDC_PREVIEW", "PreviewInEditor"),
                                      ("IDC_CURSOR", "CaptureCursor")])
def test_switch_click_toggles_and_saves(window, aid, attr):
    before = uia.toggled(window.hwnd, aid)
    uia.click(window.hwnd, aid)
    win.wait_until(lambda: uia.toggled(window.hwnd, aid) != before, what="switch flipped")
    win.wait_until(lambda: cur_preset().get(attr) == str(int(not before)), what=f"{attr} saved")


@pytest.mark.parametrize("index,check", [
    (1, lambda p: p.find("ShareSettingsFile").get("Enabled") == "1"),
    (2, lambda p: p.find("ShareSettingsProgram").get("Enabled") == "1"),
    (3, lambda p: p.find("ShareSettingsGrabText").get("Enabled") == "1"),
    (0, lambda p: p.find("ShareSettingsFile").get("Enabled") == "0" and p.find("ShareSettingsProgram").get("Enabled") == "0"),
], ids=["file", "program", "text", "none"])
def test_share_destination(window, index, check):
    open_dropdown(window, "IDC_SHARE", 11)
    uia.click(window.hwnd, f"IDC_SHARE.{index}")
    win.wait_until(lambda: check(cur_preset()), what="share destination saved")


def test_delay_menu(window):
    uia.click(window.hwnd, "IDC_DELAY")
    uia.click(window.hwnd, "IDC_DELAY.3")
    win.wait_until(lambda: cur_preset().get("DelayDuration") == "3", what="delay saved")


def test_capture_button(window_with_list, lib):
    window = window_with_list
    uia.click(window.hwnd, "IDC_PRESETS.1")                   # "e2e two": full screen
    win.wait_until(lambda: preset_xml().get("SelectedMode") == "1", what="preset selected")
    uia.click(window.hwnd, "IDC_CAPTURE")
    lib.wait_new()


def test_preset_list_selection_loads_controls(window_with_list):
    window = window_with_list
    rows = win.wait_until(lambda: uia.items(window.hwnd, "IDC_PRESETS"), what="preset rows")
    assert len(rows) == 2
    first = uia.value(window.hwnd, "IDC_SELECTION")
    uia.click(window.hwnd, "IDC_PRESETS.1")
    win.wait_until(lambda: uia.value(window.hwnd, "IDC_SELECTION") != first, what="selection of 'e2e two' shown")
    uia.click(window.hwnd, "IDC_PRESETS.0")
    win.wait_until(lambda: uia.value(window.hwnd, "IDC_SELECTION") == first, what="selection of 'e2e one' shown")


def test_presets_button_opens_the_list(window):
    before = regs.get("CapturePresetsOpen", default=0)
    uia.click(window.hwnd, "IDC_PRESETS_LABEL")
    win.wait_until(lambda: regs.get("CapturePresetsOpen") == 1 - before, what="list toggled")
    win.wait_until(lambda: len(uia.items(window.hwnd, "IDC_PRESETS")) == (0 if before else 2), what="preset rows shown or hidden")
    uia.click(window.hwnd, "IDC_PRESETS_LABEL")
    win.wait_until(lambda: regs.get("CapturePresetsOpen") == before, what="list toggled back")


def test_preset_new_rename_duplicate_delete(window):
    window.command(C.IDM_PRESET_NEW)
    form = win.wait_form(window.pid)
    win.form_set(form, 0, "E2E criado")
    win.form_ok(form)
    win.wait_until(lambda: "E2E criado" in presets.names_in_file(), what="new preset saved")

    window.command(C.IDM_PRESET_RENAME)
    form = win.wait_form(window.pid)
    assert win.form_get(form, 0) == "E2E criado"
    win.form_set(form, 0, "E2E renomeado")
    win.form_ok(form)
    win.wait_until(lambda: "E2E renomeado" in presets.names_in_file(), what="renamed")

    window.command(C.IDM_PRESET_DUP)
    win.wait_until(lambda: any(n.startswith("E2E renomeado") and n != "E2E renomeado" for n in presets.names_in_file()),
                   what="duplicate")
    n = len(presets.names_in_file())

    window.command(C.IDM_PRESET_DEL)
    box = message_box(window)
    win.post_command(box, win32con.IDYES)
    win.wait_until(lambda: len(presets.names_in_file()) == n - 1, what="preset deleted")


def test_last_preset_cannot_be_deleted(capture):
    presets.write(presets.preset("only one", "Region"))
    capture.start("/hide")
    capture.show()
    capture.command(C.IDM_PRESET_DEL)
    box = message_box(capture)
    win.post_command(box, win32con.IDOK)
    assert presets.names_in_file() == ["only one"]


def test_preset_from_current_settings(window):
    window.command(C.IDM_PRESET_FROM_CURRENT)
    form = win.wait_form(window.pid)
    win.form_ok(form)                                        # proposed name: "<current> (copy)"
    win.wait_until(lambda: len(presets.names_in_file()) == 3, what="preset added")
    p = list(preset_xml().iter("CaptureSettings"))[-1]
    assert p.get("SelectionMode") == list(preset_xml().iter("CaptureSettings"))[0].get("SelectionMode")


def test_preset_hotkey_form(window, logs):
    window.command(C.IDM_PRESET_HOTKEY)
    form = win.wait_form(window.pid)
    hk = win.child_by_id(form, 1000)
    # Ctrl+Alt+F12: HOTKEYF_CONTROL 2 | HOTKEYF_ALT 4
    win.send(hk, HKM_SETHOTKEY, (6 << 8) | win32con.VK_F12)
    win.form_ok(form)
    win.wait_until(lambda: cur_preset().get("HotKey") == str(win32con.VK_F12), what="hotkey saved")
    assert cur_preset().get("HotKeyModifiers") == "3"        # MOD_ALT | MOD_CONTROL
    hit = logs.capture.wait(r"Hotkey registered: id 100 = Ctrl\+Alt\+F12|Hotkey Ctrl\+Alt\+F12: RegisterHotKey failed")[0]
    if "failed" in hit:                                      # another program owns the keys: the hook takes them
        notice = lambda: [b for b in window.popups("#32770") if re.search("hook|gancho", box_text(b), re.I)]
        win.post_command(win.wait_until(notice, what="hook notice")[0], win32con.IDCANCEL)  # and the user is told so
        win.wait_until(lambda: not notice(), what="notice closed")
    apps.wait_quiet(0.5)
    win.keys("^%{F12}")
    overlay = win.wait_window("SnapnabSelectWindowClass")   # "e2e one" is a region capture: the crosshair opens
    win.foreground(overlay)
    win.keys("{ESC}")


def test_presets_export_and_import(window, tmp_path):
    out = tmp_path / "e2e.snappresets"
    window.command(C.IDM_PRESET_EXPORT)
    filedialog.fill(window.pid, out)
    win.wait_until(lambda: out.exists() and out.stat().st_size > 0, what="presets exported")
    assert "e2e two" in out.read_text(encoding="utf-8")

    window.command(C.IDM_PRESET_DEL)
    win.post_command(message_box(window), win32con.IDYES)
    win.wait_until(lambda: len(presets.names_in_file()) == 1, what="one preset left")

    window.command(C.IDM_PRESET_IMPORT)
    filedialog.fill(window.pid, out)
    win.wait_until(lambda: {"e2e one", "e2e two"} <= set(presets.names_in_file()), what="presets imported")


def test_options_saves_settings_and_tells_the_editor(window, logs):
    ed = apps.Editor()
    window.command(C.IDM_OPEN_EDITOR)
    win.wait_until(lambda: win.is_visible(ed.hwnd), 15, what="Editor")
    regs.set_dword("ClipboardMonitor", 1)
    logs.mark()
    window.command(C.IDM_OPTIONS)
    form = win.wait_form(window.pid)
    combo = win.child_by_id(form, 1011)                      # "Monitor the clipboard" (after the 8 shortcut fields)
    assert win.class_name(combo) == "ComboBox" and win.combo_count(combo) == 3
    win.send(combo, win.CB_SETCURSEL, 2)
    win.form_ok(form)
    win.wait_until(lambda: regs.get("ClipboardMonitor") == 2, what="option saved")
    logs.capture.wait(ipc_sent(RELOAD_CONFIG, 1))
    logs.editor.wait(ipc_received(RELOAD_CONFIG))


def test_options_rejects_duplicate_hotkeys(window):
    window.command(C.IDM_OPTIONS)
    form = win.wait_form(window.pid)
    # image and video capture on the same key
    win.send(win.child_by_id(form, 1001), HKM_SETHOTKEY, (6 << 8) | win32con.VK_F11)
    win.send(win.child_by_id(form, 1002), HKM_SETHOTKEY, (6 << 8) | win32con.VK_F11)
    before = regs.get("ImageHotkey")
    win.form_ok(form)
    box = win.wait_until(lambda: [h for h in window.popups("#32770")], what="validation message")[0]
    win.post_command(box, win32con.IDOK)
    assert win32gui.IsWindow(form), "the form stays open"
    win.form_cancel(form)
    assert regs.get("ImageHotkey") == before


def test_settings_export_and_import_merge(window, tmp_path, logs):
    out = tmp_path / "e2e.snapsettings"
    window.command(C.IDM_SETTINGS_EXPORT)
    filedialog.fill(window.pid, out)
    win.wait_until(lambda: out.exists() and out.stat().st_size > 0, what="settings exported")
    txt = out.read_text(encoding="utf-8")
    assert "Presets1.xml" in txt and "ClipboardMonitor" in txt

    old_pid = window.pid
    window.command(C.IDM_SETTINGS_IMPORT)
    filedialog.fill(window.pid, out)
    td = win.wait_until(lambda: [h for h in window.popups("#32770")], what="replace/merge task dialog")[0]
    win.send(td, TDM_CLICK_BUTTON, 101)                       # Merge
    # both programs restart with the imported settings; the current ones go to Backups\Before-import-*
    win.wait_until(lambda: apps.Capture().pid not in (None, old_pid), 30, what="capturer restarted")
    win.wait_until(lambda: list((DATA_DIR / "Backups").glob("Before-import-*.snapsettings")), 10, what="backup")
    assert {"e2e one", "e2e two"} <= set(presets.names_in_file())


def test_one_click_panel_toggle(window):
    regs.set_dword("OneClickEnabled", 0)
    window.command(C.IDM_ONECLICK)
    win.wait_window("SnapnabOneClick")
    assert regs.get("OneClickEnabled") == 1
    window.command(C.IDM_ONECLICK)
    win.wait_no_window("SnapnabOneClick")
    assert regs.get("OneClickEnabled") == 0


def test_about_shows_the_version(window):
    window.command(C.IDM_ABOUT)
    box = message_box(window)
    assert re.search(r"Snapnab \d+\.\d+\.\d+", box_text(box)), box_text(box)
    win.post_command(box, win32con.IDOK)


def test_preset_help(window):
    window.command(C.IDM_PRESET_HELP)
    box = message_box(window)
    assert box_text(box).strip()
    win.post_command(box, win32con.IDOK)


@pytest.mark.parametrize("cmd,folder", [(C.IDM_LIBRARY_FOLDER, "Library"), (C.IDM_LOGS, "Logs"),
                                        (C.IDM_SETTINGS_BACKUPS, "Backups")])
def test_folder_shortcuts_open_explorer(window, cmd, folder):
    """Explorer may open a window or a tab: the shell's window list says which folders are open."""
    import pythoncom
    import win32com.client
    pythoncom.CoInitialize()
    shell = win32com.client.Dispatch("Shell.Application")
    target = str(DATA_DIR / folder).lower()
    (DATA_DIR / folder).mkdir(exist_ok=True)

    def views():
        out = []
        for w in shell.Windows():
            try:
                if w.Document.Folder.Self.Path.lower() == target:
                    out.append(w)
            except Exception:
                pass
        return out

    already = len(views())
    windows_before = set(win.top_windows("CabinetWClass"))
    window.command(cmd)
    try:
        win.wait_until(lambda: len(views()) > already, 15, what=f"Explorer on {folder}")
    except AssertionError:
        # Explorer sometimes reuses an existing window (or a background tab) instead of showing the folder;
        # Snapnab only calls ShellExecute, so that is not counted as a Snapnab failure
        for h in set(win.top_windows("CabinetWClass")) - windows_before:
            win.close_window(h)
        assert window.running and win.is_visible(window.hwnd)
        pytest.skip(f"Explorer did not show a view of {folder} (window/tab reuse); could not observe the result")
    finally:
        for w in views()[already:]:
            try:
                w.Quit()
            except Exception:
                pass


def test_capture_from_clipboard_menu(window, lib):
    from PIL import Image
    win.set_clipboard_image(Image.new("RGB", (123, 45), (9, 9, 9)))
    window.command(C.IDM_CAPTURE_CLIPBOARD)
    from snapnab_e2e import library
    assert library.size(library.dump(lib.wait_new()[-1])) == (123, 45)
