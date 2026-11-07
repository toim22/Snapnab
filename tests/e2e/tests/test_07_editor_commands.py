"""Editor menus (rust/snapedit/src/app/commands.rs): Image geometry and effects, object arrangement, clipboard, OCR, documents and
view commands. Results come from the saved .snap and from the composed image."""
import pytest
import win32con
from PIL import Image

from snapnab_e2e import apps, library, regs, uia, win
from snapnab_e2e.apps import EditorCmd as M
from snapnab_e2e.editor import BLUE, GREEN, WHITE, EditorSession, close, make_picture
from snapnab_e2e.library import LibraryWatcher


@pytest.fixture
def s(editor, tmp_path):
    sess = EditorSession(editor)
    sess.open_picture(make_picture(tmp_path / "e2e-picture.png"))
    return sess


def two_shapes(s):
    s.tool("Shape")
    s.drag((250, 250), (450, 400))
    s.drag((900, 600), (1200, 800))
    s.tool("Select")
    s.cmd(M.M_SELECTALL)


def diff_count(a, b, tol=25):
    if a.size != b.size:
        return a.width * a.height
    return sum(1 for p, q in zip(a.getdata(), b.getdata()) if not close(p, q, tol))


# ---------- Image: geometry ----------

@pytest.mark.parametrize("cmd,size", [(M.M_ROT_CW, (1000, 1600)), (M.M_ROT_CCW, (1000, 1600)), (M.M_ROT_180, (1600, 1000)),
                                      (M.M_FLIP_H, (1600, 1000)), (M.M_FLIP_V, (1600, 1000))],
                         ids=["rot-cw", "rot-ccw", "rot-180", "flip-h", "flip-v"])
def test_rotate_and_flip(s, cmd, size):
    s.cmd(cmd)
    assert s.size() == size
    img = s.composed()
    if cmd == M.M_FLIP_H:
        assert close(img.getpixel((1600 - 450, 400)), BLUE), "the blue block moves to the right"
    if cmd == M.M_ROT_180:
        assert close(img.getpixel((1600 - 450, 1000 - 400)), BLUE)


def test_rotate_keeps_objects_on_the_image(s):
    s.tool("Arrow")
    s.drag((300, 300), (500, 400))
    s.cmd(M.M_ROT_CW)
    o = s.objects()[0]
    assert float(o.get("X1")) <= 1000 and float(o.get("Y1")) <= 1600, o.attrib


def test_resize_by_scale(s):
    s.post(M.M_RESIZE)
    form = s.form()
    win.form_set(form, 2, 50)                                 # "or scale (%)"
    win.form_ok(form)
    win.wait_until(lambda: s.size() == (800, 500), what="half size")


def test_canvas_size(s):
    s.post(M.M_CANVAS)
    form = s.form()
    win.form_set(form, 0, 2000)
    win.form_set(form, 1, 1200)
    win.form_ok(form)
    win.wait_until(lambda: s.size() == (2000, 1200), what="bigger canvas")
    img = s.composed()
    assert close(img.getpixel((200 + 450, 100 + 400)), BLUE), "anchored in the center"


def test_free_rotation(s):
    s.post(M.M_ROT_FREE)
    form = s.form()
    win.form_set(form, 0, 90)
    win.form_ok(form)
    w, h = win.wait_until(lambda: (lambda z: z if z[0] < z[1] else None)(s.size()), what="rotated by 90°")
    assert abs(w - 1000) <= 4 and abs(h - 1600) <= 4


def test_trim_removes_uniform_borders(s):
    s.cmd(M.M_TRIM)
    w, h = s.size()
    assert w < 1600 and h < 1000, "the white margin is uniform"
    assert w >= 1200 and h >= 600, "the content stays"


def test_crop_to_selection_without_selection_explains(s):
    s.post(M.M_CROP_SEL)
    box = win.wait_until(lambda: s.popups(), what="message box")[0]
    win.post_command(box, win32con.IDOK)
    assert s.size() == (1600, 1000)


# ---------- Image: effects panel (preview, then Apply) ----------

@pytest.mark.parametrize("cmd,grows", [
    (M.M_FX_BORDER, True), (M.M_FX_SHADOW, True), (M.M_FX_CAPTION, True), (M.M_FX_EDGE, False), (M.M_FX_GRAY, False),
    (M.M_FX_SEPIA, False), (M.M_FX_INVERT, False), (M.M_FX_MONO, False), (M.M_FX_BC, False), (M.M_FX_HS, False),
    (M.M_FX_REPLACE, False), (M.M_FX_WATERMARK, False), (M.M_FX_PERSPECTIVE, False), (M.M_FX_PAGECURL, False),
    (M.M_FX_SPOTLIGHT, False), (M.M_FX_FILTER, False), (M.M_FX_ADJUST, False),
], ids=lambda v: {getattr(M, n): n[5:].lower() for n in dir(M) if n.startswith("M_FX_")}.get(v, str(v)))
def test_effect_applies(s, cmd, grows):
    before = s.composed()
    s.apply_effect(cmd)
    after = s.composed()
    if grows:
        assert after.width > before.width or after.height > before.height, (before.size, after.size)
    else:
        assert diff_count(before, after) > 1000, "the effect should change the picture"
    s.cmd(M.M_UNDO)                                           # one undo step
    assert diff_count(before, s.composed()) < 50, "undo restores the picture"


def test_grayscale_values(s):
    s.apply_effect(M.M_FX_GRAY)
    p = s.composed().getpixel((450, 400))
    assert max(p) - min(p) <= 4, p


def test_invert_values(s):
    s.apply_effect(M.M_FX_INVERT)
    p = s.composed().getpixel((450, 400))
    assert close(p, tuple(255 - c for c in BLUE), 15), p


# ---------- objects ----------

def test_group_and_ungroup(s):
    two_shapes(s)
    s.cmd(M.M_GROUP)
    objs = s.objects()
    assert [o.tag for o in objs] == ["Group"] and len(list(objs[0])) == 2
    s.cmd(M.M_UNGROUP)
    assert s.tags() == ["Shape", "Shape"]


def test_align_left(s):
    two_shapes(s)
    s.cmd(M.M_ALIGN_LEFT)
    xs = [min(float(o.get("X1")), float(o.get("X2"))) for o in s.objects()]
    assert abs(xs[0] - xs[1]) < 1.5, xs


def test_distribute_horizontally(s):
    s.tool("Shape")
    for x0 in (200, 500, 1300):
        s.drag((x0, 300), (x0 + 100, 400))
    s.tool("Select")
    s.cmd(M.M_SELECTALL)
    s.cmd(M.M_DIST_H)
    xs = sorted(min(float(o.get("X1")), float(o.get("X2"))) for o in s.objects())
    assert abs((xs[1] - xs[0]) - (xs[2] - xs[1])) < 3, xs


def test_bring_to_front_and_send_to_back(s):
    s.tool("Arrow")
    s.drag((200, 200), (400, 300))
    s.tool("Line")
    s.drag((200, 500), (600, 500))                            # the line is selected (last drawn)
    s.cmd(M.M_BACK)
    assert s.tags() == ["Line", "Arrow"]
    s.cmd(M.M_FRONT)
    assert s.tags() == ["Arrow", "Line"]


def test_lock_and_unlock(s):
    s.tool("Shape")
    s.drag((300, 300), (500, 450))
    s.cmd(M.M_LOCK)
    assert s.objects()[0].get("Locked") == "1"
    s.cmd(M.M_DELETE_SEL)
    assert s.tags() == ["Shape"], "a locked object cannot be deleted"
    s.cmd(M.M_UNLOCK)
    assert s.objects()[0].get("Locked") in (None, "0")


def test_delete_selected_object(s):
    s.tool("Shape")
    s.drag((300, 300), (500, 450))
    s.cmd(M.M_DELETE_SEL)
    assert s.tags() == []


def test_flatten_all(s):
    s.tool("Shape")
    s.drag((300, 300), (500, 450))
    before = s.composed()
    s.cmd(M.M_FLATTEN)
    assert s.tags() == []
    assert diff_count(before, s.composed()) < 50, "the objects become part of the picture"


def test_rename_object(s):
    s.tool("Shape")
    s.drag((300, 300), (500, 450))
    s.post(M.M_RENAME_OBJ)
    form = s.form()
    win.form_set(form, 0, "Caixa E2E")
    win.form_ok(form)
    win.wait_until(lambda: s.objects()[0].get("Name") == "Caixa E2E", what="object renamed")


def test_copy_and_paste_object(s):
    s.tool("Shape")
    s.drag((300, 300), (500, 450))
    s.cmd(M.M_COPY)
    assert win.clipboard_formats(), "something on the clipboard"
    s.cmd(M.M_PASTE)
    assert s.tags() == ["Shape", "Shape"]


def test_paste_in_place_keeps_coordinates(s):
    s.tool("Shape")
    s.drag((300, 300), (500, 450))
    s.cmd(M.M_COPY)
    s.cmd(M.M_PASTE_INPLACE)
    a, b = s.objects()
    assert (a.get("X1"), a.get("Y1")) == (b.get("X1"), b.get("Y1"))


def test_cut_object(s):
    s.tool("Shape")
    s.drag((300, 300), (500, 450))
    s.cmd(M.M_CUT)
    assert s.tags() == []
    s.cmd(M.M_PASTE)
    assert s.tags() == ["Shape"]


def test_paste_image_becomes_an_object(s):
    win.set_clipboard_image(Image.new("RGB", (120, 80), (250, 0, 250)))
    s.focus()
    s.cmd(M.M_PASTE)
    assert s.tags() == ["Image"]


def test_lift_and_drop_a_piece(s):
    s.tool("Selection")
    s.drag((200, 200), (700, 600))
    s.cmd(M.M_LIFT)
    objs = s.objects()
    assert [o.tag for o in objs] == ["Image"] and objs[0].get("Floating") == "1", [o.attrib for o in objs]
    s.cmd(M.M_DROP)
    assert s.tags() == []


def test_tag_image(s):
    s.post(M.M_TAG)
    form = s.form()
    win.form_set(form, 0, "e2e, teste")
    win.form_ok(form)
    win.wait_until(lambda: s.doc().get("Tags") == "e2e, teste", what="tags saved")


# ---------- clipboard, OCR, documents ----------

def test_copy_all_puts_the_composed_image(s):
    img = s.composed()
    assert img.size == (1600, 1000)
    assert close(img.getpixel((450, 400)), BLUE) and close(img.getpixel((1100, 400)), GREEN)


def test_new_image_from_clipboard(s):
    win.set_clipboard_image(Image.new("RGB", (321, 123), (0, 128, 128)))
    lib = LibraryWatcher()
    s.cmd(M.M_NEW_CLIPBOARD)
    assert library.size(library.dump(lib.wait_new()[-1])) == (321, 123)


def test_new_blank_image(s):
    lib = LibraryWatcher()
    s.post(M.M_NEW_IMAGE)
    form = s.form()
    win.form_set(form, 0, 640)
    win.form_set(form, 1, 360)
    win.form_ok(form)
    assert library.size(library.dump(lib.wait_new()[-1])) == (640, 360)


def test_duplicate_document(s):
    s.tool("Arrow")
    s.drag((300, 300), (500, 400))
    s.save()
    lib = LibraryWatcher()
    s.cmd(M.M_DUPLICATE)
    dup = library.dump(lib.wait_new()[-1])
    assert library.size(dup) == (1600, 1000) and library.object_tags(dup) == ["Arrow"]


def test_delete_document_asks_and_recycles(s):
    path = s.path
    s.post(M.M_DELETE)
    box = win.wait_until(lambda: s.popups(), what="confirmation")[0]
    win.post_command(box, win32con.IDNO)
    apps.wait_quiet(0.5)
    assert path.exists(), "No keeps the capture"
    s.post(M.M_DELETE)
    box = win.wait_until(lambda: s.popups(), what="confirmation")[0]
    win.post_command(box, win32con.IDYES)
    win.wait_until(lambda: not path.exists(), what="capture sent to the Recycle Bin")


def test_grab_text_ocr(s):
    s.post(M.M_OCR)
    tw = win.wait_until(lambda: s.popups("SnapnabTextWnd"), 20, what="recognized text window")[0]
    text = uia.value(tw, "SnapnabTextWnd.Text")
    win.close_window(tw)
    assert "Snapnab" in text or "OCR" in text or "2026" in text, text


# ---------- view and settings ----------

@pytest.mark.parametrize("cmd,name,sub", [(M.M_RULERS, "Rulers", "Editor"), (M.M_EDGESNAP, "EdgeSnap", "Editor"),
                                          (M.M_SNAPPING, "CanvasSnapping", "Editor"),
                                          (M.M_PLAINPASTE, "PlainTextPaste", "Editor")])
def test_toggles_are_saved(s, cmd, name, sub):
    s.cmd(cmd)
    first = regs.get(name, sub)
    s.cmd(cmd)
    second = regs.get(name, sub)
    assert {first, second} == {0, 1}


def test_zoom_commands(s):
    for c in (M.M_ZOOM_IN, M.M_ZOOM_IN, M.M_ZOOM_OUT, M.M_ZOOM_100, M.M_ZOOM_FIT):
        s.cmd(c)
    assert win.is_visible(s.ed.hwnd) and s.size() == (1600, 1000)


def test_layers_and_effects_panels(s):
    s.tool("Shape")
    s.drag((300, 300), (500, 450))
    s.cmd(M.M_SHOW_LAYERS)
    win.wait_until(lambda: uia.exists(s.ed.hwnd, "SnapnabLayers"), what="layers panel")
    s.cmd(M.M_SHOW_EFFECTS)
    win.wait_until(s.effects_panel_shown, what="effects panel")


def test_library_view_toggle(s):
    s.cmd(M.M_SHOW_LIBRARY)
    s.cmd(M.M_SHOW_LIBRARY)
    assert s.size() == (1600, 1000)


def test_customize_toolbar_window(s):
    s.post(M.M_TB_CUSTOMIZE)
    w = win.wait_window("SnapnabTbCustomize", pid=s.ed.pid)
    win.close_window(w)
    win.wait_no_window("SnapnabTbCustomize")
    s.cmd(M.M_TB_RESET)


@pytest.mark.parametrize("cmd", [M.M_ABOUT, M.M_SHORTCUTS], ids=["about", "shortcuts"])
def test_help_windows(s, cmd):
    before = set(win.top_windows(pid=s.ed.pid, visible_only=True))
    s.post(cmd)
    new = win.wait_until(lambda: [h for h in win.top_windows(pid=s.ed.pid, visible_only=True) if h not in before],
                         what="help window")
    for h in new:
        if win.class_name(h) == "#32770":
            win.post_command(h, win32con.IDOK)
        else:
            win.close_window(h)


def test_print_dialog_opens_and_cancels(s):
    """Windows 11 shows its modern print dialog from another process: it is found by its title."""
    before = set(win.top_windows(visible_only=True))

    def dialogs():
        return [h for h in win.top_windows(visible_only=True)
                if h not in before and ("Print" in win.title(h) or "Imprimir" in win.title(h))]

    s.post(M.M_PRINT)
    dlg = win.wait_until(dialogs, 20, what="print dialog")[0]
    apps.wait_quiet(1.5)                                      # the XAML dialog finishes loading its printers
    try:
        # the modern dialog is UI Automation-friendly: its Cancel button is invoked directly
        from pywinauto import Desktop
        w = Desktop(backend="uia").window(handle=dlg)
        btn = w.child_window(title_re="Cancel|Cancelar", control_type="Button")
        btn.wait("exists", timeout=10)
        btn.invoke()
    except Exception:
        win.close_window(dlg)
    win.wait_until(lambda: not dialogs(), 15, what="print dialog closed")
    assert s.ed.running and win.is_visible(s.ed.hwnd)
