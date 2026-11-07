"""Editor files and video: Save as in every format, opening files, and the video view (frame, cut, MP4, GIF)."""
import time

import pytest
from PIL import Image

from snapnab_e2e import apps, filedialog, library, presets, uia, win
from snapnab_e2e import video as v
from snapnab_e2e.apps import EditorCmd as M
from snapnab_e2e.editor import BLUE, EditorSession, close, make_picture
from snapnab_e2e.library import LibraryWatcher

# video view buttons (rust/snapedit/src/app/video.rs)
# the video view's buttons: WM_COMMAND to the Editor's main window (the view is drawn inside it)
B_PLAY, B_PREV, B_NEXT, B_MARKIN, B_MARKOUT, B_CUT, B_UNDOCUT, B_SAVEMP4, B_SAVEFRAME, B_GIF, B_PLAYER = range(2001, 2012)


@pytest.fixture
def s(editor, tmp_path):
    sess = EditorSession(editor)
    sess.open_picture(make_picture(tmp_path / "e2e-picture.png"))
    return sess


@pytest.mark.parametrize("ext,magic", [("png", b"\x89PNG"), ("jpg", b"\xff\xd8"), ("bmp", b"BM"), ("gif", b"GIF8"),
                                       ("tif", (b"II*\x00", b"MM\x00*")), ("pdf", b"%PDF")])
def test_save_as_formats(s, tmp_path, ext, magic):
    s.tool("Arrow")
    s.drag((300, 300), (600, 450))                            # the annotations go into the exported file
    out = tmp_path / f"exported.{ext}"
    s.post(M.M_SAVEAS)
    filedialog.fill(s.ed.pid, out)
    if ext == "jpg":
        form = s.form()                                       # JPEG quality
        win.form_set(form, 0, 85)
        win.form_ok(form)
    win.wait_until(lambda: out.exists() and out.stat().st_size > 0, 15, what=f"{ext} written")
    head = out.read_bytes()[:8]
    assert any(head.startswith(m) for m in (magic if isinstance(magic, tuple) else (magic,))), head
    if ext not in ("pdf",):
        with Image.open(out) as im:
            assert im.size == (1600, 1000)
            if ext in ("png", "bmp", "tif"):
                assert close(im.convert("RGB").getpixel((450, 600)), BLUE)


def test_save_as_snap_copy(s, tmp_path):
    s.tool("Shape")
    s.drag((300, 300), (600, 450))
    out = tmp_path / "copy.snap"
    s.post(M.M_SAVEAS)
    filedialog.fill(s.ed.pid, out)
    win.wait_until(lambda: out.exists() and out.stat().st_size > 0, 15, what=".snap written")
    doc = library.dump(out)
    assert library.size(doc) == (1600, 1000) and library.object_tags(doc) == ["Shape"]


def test_save_as_names_an_unnamed_library_capture(s, tmp_path):
    win.set_clipboard_image(Image.new("RGB", (200, 100), (1, 1, 1)))
    lib = LibraryWatcher()
    s.cmd(M.M_NEW_CLIPBOARD)
    new = lib.wait_new()[-1]
    s.path = new
    out = tmp_path / "Nome E2E.png"
    s.post(M.M_SAVEAS)
    filedialog.fill(s.ed.pid, out)
    win.wait_until(lambda: out.exists(), what="png")
    names = library.LIBRARY_DIR / "Names.xml"
    win.wait_until(lambda: names.exists() and "Nome E2E" in names.read_text(encoding="utf-8"), what="capture named")


def test_open_dialog(s, tmp_path):
    other = tmp_path / "opened.png"
    Image.new("RGB", (300, 300), (255, 0, 0)).save(other)
    lib = LibraryWatcher()
    s.post(M.M_OPEN)
    filedialog.fill(s.ed.pid, other)
    assert library.size(library.dump(lib.wait_new()[-1])) == (300, 300)
    win.wait_until(lambda: "opened" in s.ed.title(), what="opened document is current")


def test_open_snap_file_keeps_objects(s, tmp_path):
    s.tool("Arrow")
    s.drag((300, 300), (600, 450))
    out = tmp_path / "with-arrow.snap"
    s.post(M.M_SAVEAS)
    filedialog.fill(s.ed.pid, out)
    win.wait_until(lambda: out.exists(), what=".snap")
    s.ed.open(out)
    win.wait_until(lambda: "with-arrow" in s.ed.title(), what=".snap opened")


# ---------- video ----------

@pytest.fixture
def video_in_editor(capture, logs):
    presets.write(v.video_preset(editor=True, fps=15))
    capture.start("/hide")
    mp4 = v.record(capture, logs, 4)
    ed = apps.Editor()
    win.wait_until(lambda: win.is_visible(ed.hwnd) and mp4.stem in ed.title(), 20, what="video in the Editor")
    win.wait_until(lambda: uia.exists(ed.hwnd, "SnapnabVideoView"), what="video view")
    view = ed.hwnd
    apps.wait_quiet(1)
    return ed, view, mp4


def vcmd(view, button, wait=0.3):
    win.post_command(view, button)
    time.sleep(wait)


def test_video_save_frame(video_in_editor):
    ed, view, mp4 = video_in_editor
    lib = LibraryWatcher()
    vcmd(view, B_NEXT)
    vcmd(view, B_SAVEFRAME)
    doc = library.dump(lib.wait_new()[-1])
    assert library.size(doc) == v.frame_size(mp4)


def test_video_cut_and_save_mp4(video_in_editor, tmp_path):
    ed, view, mp4 = video_in_editor
    full = v.duration(mp4)
    for _ in range(5):
        vcmd(view, B_NEXT, 0.1)
    vcmd(view, B_MARKIN)
    for _ in range(25):                                       # ~1.7 s at 15 fps
        vcmd(view, B_NEXT, 0.1)
    vcmd(view, B_MARKOUT)
    vcmd(view, B_CUT)
    out = tmp_path / "cut.mp4"
    vcmd(view, B_SAVEMP4)
    filedialog.fill(ed.pid, out)
    win.wait_until(lambda: out.exists() and out.stat().st_size > 1000 and not ed.popups("SnapnabForm"), 60,
                   what="MP4 exported")
    time.sleep(1)
    d = v.duration(out)
    assert d < full - 1.0, f"{d:.2f}s should be shorter than {full:.2f}s after cutting ~1.7 s"


def test_video_cut_needs_a_range(video_in_editor):
    ed, view, mp4 = video_in_editor
    vcmd(view, B_CUT)
    box = win.wait_until(lambda: ed.popups(), what="explanation")[0]
    win.post_command(box, 1)


def test_video_to_animated_gif(video_in_editor, tmp_path):
    ed, view, mp4 = video_in_editor
    vcmd(view, B_GIF)
    form = win.wait_form(ed.pid)
    win.form_set(form, 1, 5)                                  # fps
    win.form_set(form, 2, 320)                                # width
    win.form_ok(form)
    out = tmp_path / "anim.gif"
    filedialog.fill(ed.pid, out)
    win.wait_until(lambda: out.exists() and out.stat().st_size > 1000, 90, what="GIF written")
    time.sleep(1)
    with Image.open(out) as im:
        assert im.width == 320 and getattr(im, "n_frames", 1) > 3, (im.size, getattr(im, "n_frames", 1))


def test_video_play_pause(video_in_editor):
    ed, view, mp4 = video_in_editor
    def label():
        return uia.find(view, f"SnapnabVideoView.{B_PLAY - 2000}").window_text()
    before = label()
    vcmd(view, B_PLAY, 0.8)
    assert label() != before, "the button turns into Pause while playing"
    vcmd(view, B_PLAY, 0.3)
    assert label() == before
