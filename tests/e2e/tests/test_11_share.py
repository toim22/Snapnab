"""The upload and Office outputs. The upload goes to a fake HTTP server on 127.0.0.1 through a custom uploader in a
temporary Uploaders.xml (SNAPNAB_UPLOADERS_FILE): no real service, no Credential Manager, the user's destinations
untouched. Office runs only when Word / PowerPoint are installed and not already open (so a document of the user's is
never touched); the test documents are closed without saving."""
import http.server
import io
import json
import os
import threading
import time

import psutil
import pytest
from PIL import Image

from snapnab_e2e import presets, win
from snapnab_e2e.config import LIBRARY_DIR


class _Server(http.server.ThreadingHTTPServer):
    def __init__(self):
        self.bodies = []
        super().__init__(("127.0.0.1", 0), _Handler)


class _Handler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(n)
        self.server.bodies.append((self.path, self.headers.get("Content-Type", ""), body))
        out = json.dumps({"data": {"link": f"http://127.0.0.1:{self.server.server_port}/i/e2e.png"}}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)

    def log_message(self, *a):
        pass


@pytest.fixture
def server():
    s = _Server()
    t = threading.Thread(target=s.serve_forever, daemon=True)
    t.start()
    yield s
    s.shutdown()
    s.server_close()


@pytest.fixture
def uploaders(tmp_path, server):
    sxcu = {"Version": "14.0.0", "Name": "e2e", "DestinationType": "ImageUploader", "RequestMethod": "POST",
            "RequestURL": f"http://127.0.0.1:{server.server_port}/upload", "Body": "MultipartFormData",
            "FileFormName": "file", "URL": "{json:data.link}"}
    f = tmp_path / "Uploaders.xml"
    from xml.sax.saxutils import escape
    f.write_text('<?xml version="1.0" encoding="UTF-8"?>\n<Uploaders Destination="7" CopyLink="1">'
                 f"<Custom>{escape(json.dumps(sxcu))}</Custom></Uploaders>\n", encoding="utf-8")
    old = os.environ.get("SNAPNAB_UPLOADERS_FILE")
    os.environ["SNAPNAB_UPLOADERS_FILE"] = str(f)
    yield f
    if old is None:
        os.environ.pop("SNAPNAB_UPLOADERS_FILE", None)
    else:
        os.environ["SNAPNAB_UPLOADERS_FILE"] = old


def png_in(body):
    i = body.find(b"\x89PNG")
    assert i >= 0, "a PNG in the request"
    return Image.open(io.BytesIO(body[i:]))


def test_upload_from_the_command_line_copies_the_link(capture, server, uploaders):
    capture.start("/hide")
    win.clear_clipboard()
    capture.send("/region", "40,50,210,130", "/upload")
    win.wait_until(lambda: server.bodies, timeout=20, what="the upload request")
    path, ctype, body = server.bodies[0]
    assert path == "/upload" and ctype.startswith("multipart/form-data"), (path, ctype)
    assert png_in(body).size == (210, 130)
    link = f"http://127.0.0.1:{server.server_port}/i/e2e.png"
    win.wait_until(lambda: win.clipboard_text() == link, what="the link on the clipboard")
    uploads = LIBRARY_DIR / "Uploads.xml"
    win.wait_until(lambda: uploads.exists() and link in uploads.read_text(encoding="utf-8", errors="ignore"),
                   what="the link kept with the library item")


def test_upload_preset(capture, server, uploaders):
    presets.write(presets.preset("e2e upload", "FullScreen", editor=False, upload=True))
    capture.start("/hide")
    capture.send("/preset", "e2e upload")
    win.wait_until(lambda: server.bodies, timeout=20, what="the upload request")
    assert png_in(server.bodies[0][2]).size[0] > 200


def _office_app(name):
    import win32com.client
    return win32com.client.GetActiveObject(name)


def _need(exe, prog_id):
    import winreg
    try:
        winreg.OpenKey(winreg.HKEY_CLASSES_ROOT, prog_id + "\\CLSID").Close()
    except OSError:
        pytest.skip(f"{prog_id} is not installed")
    if any(p.info["name"] and p.info["name"].lower() == exe for p in psutil.process_iter(["name"])):
        pytest.skip(f"{exe} is open: the test does not touch the user's documents")


def _quit(exe, prog_id, close):
    try:
        app = _office_app(prog_id)
        close(app)
        app.Quit()
    except Exception:
        pass
    deadline = time.time() + 15
    while time.time() < deadline and any(p.info["name"] and p.info["name"].lower() == exe
                                         for p in psutil.process_iter(["name"])):
        time.sleep(0.3)


def test_word_gets_the_picture(capture):
    _need("winword.exe", "Word.Application")
    presets.write(presets.preset("e2e word", "FullScreen", editor=False, office=0))
    try:
        capture.start("/hide")
        capture.send("/preset", "e2e word")

        def shapes():
            try:
                app = _office_app("Word.Application")
                return app.Documents.Count > 0 and app.ActiveDocument.InlineShapes.Count
            except Exception:
                return 0
        win.wait_until(shapes, timeout=60, what="the picture in a Word document")
    finally:
        _quit("winword.exe", "Word.Application", lambda a: [d.Close(0) for d in list(a.Documents)])


def test_powerpoint_gets_the_picture_on_a_new_slide(capture):
    _need("powerpnt.exe", "PowerPoint.Application")
    presets.write(presets.preset("e2e ppt", "FullScreen", editor=False, office=1))
    try:
        capture.start("/hide")
        capture.send("/preset", "e2e ppt")

        def pictures():
            try:
                app = _office_app("PowerPoint.Application")
                p = app.ActivePresentation
                return p.Slides.Count >= 1 and sum(s.Shapes.Count for s in p.Slides)
            except Exception:
                return 0
        win.wait_until(pictures, timeout=60, what="the picture on a slide")
    finally:
        def close(a):
            for p in list(a.Presentations):
                p.Saved = True
                p.Close()
        _quit("powerpnt.exe", "PowerPoint.Application", close)
