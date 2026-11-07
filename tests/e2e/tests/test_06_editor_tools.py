"""Editor drawing tools (rust/snapedit/src/canvas.rs and tools.rs): every tool is used on the canvas with the real mouse and the result is
read from the saved .snap (objects) or from the composed image (raster tools)."""
import pytest
import win32con

from snapnab_e2e import library, win
from snapnab_e2e.apps import EditorCmd as M
from snapnab_e2e.editor import BLUE, BLUE_BOX, GREEN, GREEN_BOX, WHITE, EditorSession, close, make_picture


@pytest.fixture
def s(editor, tmp_path):
    sess = EditorSession(editor)
    sess.open_picture(make_picture(tmp_path / "e2e-picture.png"))
    assert sess.tags() == [], "a new import has no objects"
    return sess


# drag-created objects: tool → element written to the .snap
@pytest.mark.parametrize("tool,tag", [
    ("Arrow", "Arrow"), ("Line", "Line"), ("Shape", "Shape"), ("Pen", "Pen"), ("Highlighter", "Highlight"),
    ("Blur", ("Blur", "Pixelate")), ("Spotlight", "Spotlight"), ("Magnify", "Magnify"), ("Stamp", "Stamp"),
])
def test_drag_tool_creates_object(s, tool, tag):
    s.tool(tool)
    s.drag((300, 300), (650, 520), (800, 650))
    tags = s.tags()
    assert len(tags) == 1 and tags[0] in (tag if isinstance(tag, tuple) else (tag,)), tags


@pytest.mark.smoke
def test_text_tool(s):
    s.tool("Text")
    s.drag((250, 250), (900, 400))
    s.keys("Hello E2E")
    s.keys("{ESC}")
    objs = s.objects()
    assert [o.tag for o in objs] == ["Text"]
    assert "Hello E2E" in (objs[0].get("Text") or "".join(objs[0].itertext())), objs[0].attrib


def test_callout_tool(s):
    s.tool("Callout")
    s.drag((300, 300), (800, 500))
    s.keys("Callout E2E")
    s.keys("{ESC}")
    objs = s.objects()
    assert [o.tag for o in objs] == ["Callout"]
    assert "Callout E2E" in (objs[0].get("Text") or "".join(objs[0].itertext()))


def test_step_tool_numbers_in_sequence(s):
    s.tool("Step")
    for x in (300, 500, 700):
        s.click(x, 820)
    objs = s.objects()
    assert [o.tag for o in objs] == ["Step"] * 3
    values = [o.get("Value") or o.get("StepValue") or o.get("Text") for o in objs]
    assert values == ["1", "2", "3"], [o.attrib for o in objs]


def test_fill_tool_paints_the_clicked_region(s):
    s.tool("Fill")
    s.click(450, 400)                                         # inside the blue block
    img = s.composed()
    assert not close(img.getpixel((450, 400)), BLUE), "the blue block should be filled with the tool color"
    assert close(img.getpixel((1100, 400)), GREEN), "the green block is untouched"
    assert close(img.getpixel((100, 100)), WHITE)


def test_eraser_erases_where_the_mouse_passes(s):
    s.tool("Eraser")
    s.drag((250, 400), (650, 400))          # the mouse reports 6 positions, 80 image px apart
    img = s.composed()
    assert not close(img.getpixel((250, 400)), BLUE) and not close(img.getpixel((330, 400)), BLUE)
    assert close(img.getpixel((450, 250)), BLUE), "only along the path"


def test_eraser_stroke_is_continuous(s):
    """A quick stroke reports mouse positions far apart; the eraser must cover the segment between them, as the pen
    does (Canvas::EraseAt is only called at each WM_MOUSEMOVE position)."""
    s.tool("Eraser")
    s.drag((250, 400), (650, 400))
    img = s.composed()
    gaps = [x for x in range(260, 640, 10) if close(img.getpixel((x, 400)), BLUE)]
    assert not gaps, f"the eraser left the image intact between the mouse positions, at x = {gaps[:8]}…"


def test_eraser_shift_erases_a_straight_line(s):
    """Shift while erasing keeps the stroke on the axis the mouse moved along first (here horizontal), however the
    hand wobbles."""
    s.tool("Eraser")
    with win.held(win32con.VK_SHIFT):
        s.drag((250, 400), (400, 450), (550, 350), (650, 400))
    img = s.composed()
    gaps = [x for x in range(260, 640, 10) if close(img.getpixel((x, 400)), BLUE)]
    assert not gaps, f"the straight line was not erased at x = {gaps[:8]}…"
    for p in ((400, 450), (550, 350)):
        assert close(img.getpixel(p), BLUE), f"{p}: the eraser followed the mouse off the horizontal line"


def test_crop_tool(s):
    s.tool("Crop")
    s.drag((200, 200), (700, 600))                            # exactly the blue block
    s.keys("{ENTER}")
    w, h = s.size()
    assert abs(w - 500) <= 6 and abs(h - 400) <= 6, (w, h)


def test_cutout_removes_a_band(s):
    s.tool("Cutout")
    s.drag((50, 650), (1550, 750))                            # horizontal band of 100 px
    w, h = s.size()
    assert w == 1600 and 880 <= h <= 920, (w, h)


def test_selection_and_crop_to_selection(s):
    s.tool("Selection")
    s.drag((900, 250), (1400, 550))                           # the green block
    s.cmd(M.M_CROP_SEL)
    w, h = s.size()
    assert abs(w - 500) <= 6 and abs(h - 300) <= 6, (w, h)


def test_selection_delete_clears_the_area(s):
    s.tool("Selection")
    s.drag((900, 250), (1400, 550))
    s.keys("{DELETE}")
    img = s.composed()
    assert not close(img.getpixel((1150, 400)), GREEN), img.getpixel((1150, 400))


def test_magic_wand_selects_the_block(s):
    s.tool("MagicWand")
    s.click(450, 400)                                         # the blue block, with uniform color
    s.cmd(M.M_CROP_SEL)
    assert s.size() == (BLUE_BOX[2] - BLUE_BOX[0] + 1, BLUE_BOX[3] - BLUE_BOX[1] + 1)


def test_eyedropper_picks_the_color_for_the_next_object(s):
    s.tool("Shape")
    s.drag((100, 100), (180, 160))
    first = s.objects()[0].attrib
    s.tool("Eyedropper")
    s.click(1100, 400)                                        # green
    s.tool("Shape")
    s.drag((1450, 700), (1550, 800))
    second = s.objects()[1].attrib
    changed = {k for k in second if k in first and second[k] != first[k] and "Color" in k}
    assert changed, f"a color attribute should come from the picked green: {second}"


def test_move_tool_moves_an_object(s):
    s.tool("Shape")
    s.drag((300, 300), (500, 450))
    obj = s.objects()[0]
    before = obj.attrib
    s.tool("Select")
    # grabbed by its outline (a hollow shape, as the saved style may be, is not picked by its inside) and away from
    # the handles (a resize): an ellipse (ToolShape 2) at 45°, a rectangle on its top edge
    ellipse = obj.tag == "Ellipse" or before.get("ToolShape") == "2"
    x, y = (469, 322) if ellipse else (350, 300)
    s.drag((x, y), (x + 100, y + 75), (x + 600, y + 325))
    after = s.objects()[0].attrib
    assert float(after["X1"]) > float(before["X1"]) + 200, (obj.tag, before, after)


def test_pen_draws_next_to_its_last_stroke(s):
    """A new stroke started on (or next to) the previous one is drawn; it does not drag the previous stroke."""
    s.tool("Pen")
    s.drag((300, 300), (600, 300), (600, 500))
    first = s.objects()[0].attrib
    s.drag((600, 400), (800, 600))                            # starts right on the first stroke
    s.drag((610, 320), (900, 320))                            # and next to it, near a corner of its box
    objs = s.objects()
    assert [o.tag for o in objs] == ["Pen"] * 3
    assert objs[0].attrib == first, (first, objs[0].attrib)


def test_pen_shift_draws_a_straight_segment(s):
    """Shift held with the pen: a wobbly drag is saved as one straight segment, snapped to 45° (here horizontal)."""
    s.tool("Pen")
    with win.held(win32con.VK_SHIFT):
        s.drag((300, 400), (450, 430), (600, 380), (800, 415))
    objs = s.objects()
    assert [o.tag for o in objs] == ["Pen"]
    pts = [tuple(map(float, p.split(","))) for p in objs[0].get("Points").split()]
    assert len(pts) == 2, pts
    assert abs(pts[1][1] - pts[0][1]) < 1 and pts[1][0] - pts[0][0] > 300, pts


def test_pen_draw_and_hold_makes_a_shape(s):
    """Draw & hold: a rough square drawn with the pen and held still turns into a rectangle; released at once it stays
    a freehand stroke."""
    s.tool("Pen")
    square = [(300, 300), (600, 305), (598, 600), (302, 597), (300, 310)]
    s.drag(*square)
    assert s.tags() == ["Pen"]
    s.drag(*[(x + 400, y) for x, y in square], hold=1.2)
    objs = s.objects()
    assert objs[0].tag == "Pen" and objs[1].tag == "Shape", [o.tag for o in objs]


def test_undo_redo_and_undo_all(s):
    s.tool("Arrow")
    s.drag((200, 200), (400, 300))
    s.tool("Line")
    s.drag((200, 500), (600, 500))
    assert len(s.tags()) == 2
    s.cmd(M.M_UNDO)
    assert s.tags() == ["Arrow"]
    s.cmd(M.M_REDO)
    assert s.tags() == ["Arrow", "Line"]
    s.cmd(M.M_UNDOALL)
    assert s.tags() == []


def test_favorites_tool_bar_opens(s):
    s.tool("Favorites")
    s.tool("Arrow")
    s.drag((200, 200), (500, 300))
    assert s.tags() == ["Arrow"]
