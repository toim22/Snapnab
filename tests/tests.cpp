// Integration tests of Snapnab's critical pieces (console executable, not part of the product)
#include "Common.h"
#include "Image.h"
#include "Cv.h"
#include "Gif.h"
#include "SnapFile.h"
#include "Xml.h"
#include "Ipc.h"
#include "Effects.h"
#include "Icons.h"
#include "Ocr.h"
#include "Settings.h"
#include "../src/capture/Recorder.h"
#include "../src/capture/Panoramic.h"
#include "../src/capture/Webcam.h"
#include "../src/editor/Video.h"
#include "../src/editor/Document.h"
#include "../src/editor/QuickStyles.h"
#include <cstdio>
#include <mfapi.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <cmath>

using namespace snap;

static int g_fail = 0;
#define CHECK(c, ...) do { if (c) { printf("  ok   "); printf(__VA_ARGS__); printf("\n"); } else { printf("  FAIL "); printf(__VA_ARGS__); printf("\n"); g_fail++; } } while (0)

static Image Pattern(int w, int h, int seed) {
    Image img(w, h);
    for (int y = 0; y < h; y++)
        for (int x = 0; x < w; x++) {
            uint32_t hsh = (uint32_t)(x * 73856093u) ^ (uint32_t)(y * 19349663u) ^ (uint32_t)(seed * 83492791u);
            hsh ^= hsh >> 13; hsh *= 0x5bd1e995u; hsh ^= hsh >> 15;
            int v = (int)(hsh & 255);
            // smooth blocks + noise: similar to text/icons on a page
            int blk = ((x / 24 + (y / 18) * 7 + seed) * 2654435761u >> 24) & 255;
            img.at(x, y) = ARGB(255, (blk + v / 4) & 255, (blk * 3 + v / 8) & 255, (255 - blk + v / 6) & 255);
        }
    return img;
}

static std::wstring Tmp(const wchar_t* name) {
    wchar_t t[MAX_PATH]; GetTempPathW(MAX_PATH, t);
    return std::wstring(t) + L"snapnab_test_" + name;
}

int wmain(int argc, wchar_t** argv) {
    SetConsoleOutputCP(CP_UTF8);
    if (argc > 2 && std::wstring(argv[1]) == L"dump") {      // debugging: prints the XML of a .snap
        std::vector<SnapFrame> fr;
        if (!LoadSnapFile(argv[2], fr)) { printf("failed to open\n"); return 1; }
        printf("%s", XmlSerialize(fr[0].doc).c_str());
        return 0;
    }
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (argc > 2 && std::wstring(argv[1]) == L"audio") {    // debugging: reads only the audio track
        MFStartup(MF_VERSION);
        IMFSourceReader* rd = nullptr;
        HRESULT hr = MFCreateSourceReaderFromURL(argv[2], nullptr, &rd);
        printf("open 0x%08X\n", hr);
        rd->SetStreamSelection((DWORD)MF_SOURCE_READER_ALL_STREAMS, FALSE);
        rd->SetStreamSelection((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, TRUE);
        IMFMediaType* mt = nullptr; MFCreateMediaType(&mt);
        mt->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Audio); mt->SetGUID(MF_MT_SUBTYPE, MFAudioFormat_PCM); mt->SetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
        printf("settype 0x%08X\n", rd->SetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, nullptr, mt));
        int peak = 0;
        for (int i = 0; i < 10000; i++) {
            DWORD idx, flags; LONGLONG ts; IMFSample* smp = nullptr;
            hr = rd->ReadSample((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, 0, &idx, &flags, &ts, &smp);
            if (FAILED(hr)) { printf("sample %d FAILED 0x%08X\n", i, hr); break; }
            if (flags & MF_SOURCE_READERF_ENDOFSTREAM) { printf("EOS after %d samples, last ts %.3f\n", i, ts / 1e7); break; }
            if (i < 3 || i % 50 == 0) printf("sample %d ts %.3f flags %u\n", i, ts / 1e7, flags);
            if (smp) {
                IMFMediaBuffer* b = nullptr; BYTE* p; DWORD len;
                if (SUCCEEDED(smp->ConvertToContiguousBuffer(&b)) && SUCCEEDED(b->Lock(&p, nullptr, &len))) {
                    for (DWORD k = 0; k + 1 < len; k += 2) peak = std::max(peak, std::abs((int)*(const int16_t*)(p + k)));
                    b->Unlock();
                }
                if (b) b->Release();
                smp->Release();
            }
        }
        printf("audio peak: %d of 32767 (%.1f%%)\n", peak, peak * 100.0 / 32767);
        return 0;
    }
    if (argc > 3 && std::wstring(argv[1]) == L"frame") {    // debugging: saves the first frame of an MP4 as PNG
        GdiplusSession gdip;
        ed::VideoReader rd;
        Image f; LONGLONG ts;
        if (!rd.Open(argv[2]) || !rd.ReadFrame(f, ts)) { printf("failed to read the video\n"); return 1; }
        printf("%dx%d -> %s\n", f.w, f.h, SaveImageFile(argv[3], f) ? "saved" : "failed");
        return 0;
    }
    GdiplusSession gdip;

    printf("[1] Phase correlation (scroll stitching)\n");
    {
        Image page = Pattern(700, 1600, 3);
        for (int s : { 37, 120, 250 }) {
            Image a = page.Crop({ 0, 100, 700, 600 });
            Image b = page.Crop({ 0, 100 + s, 700, 600 + s });       // content moved up by s px (scrolled down)
            double conf = 0;
            POINT t = cv::DetectTranslation(a, b, &conf);
            CHECK(-t.y == s && t.x == 0, "scroll of %d px detected as %d (dx=%d, conf %.1f)", s, -t.y, t.x, conf);
        }
    }

    printf("[2] Magic wand / inpainting\n");
    {
        Image img(100, 80, 0xFFFFFFFF);
        img.FillRect({ 20, 20, 60, 50 }, 0xFFFF0000);
        cv::Mask m = cv::DoWand(img, { 30, 30 }, 10, false);
        RECT b = cv::MaskBounds(m, img.w, img.h);
        CHECK(b.left == 20 && b.top == 20 && b.right == 60 && b.bottom == 50, "wand selects the red rectangle (%ld,%ld)-(%ld,%ld)", b.left, b.top, b.right, b.bottom);
        cv::Mask all = cv::DoWand(img, { 5, 5 }, 100, false);
        CHECK(!cv::MaskEmpty(all) && all.size() == 8000 && std::all_of(all.begin(), all.end(), [](uint8_t v) { return v == 255; }), "tolerance 100 = whole layer");
        Image f = img;
        cv::SmartFill(f, m);
        CHECK(f.at(40, 35) == 0xFFFFFFFF, "Smart fill paints the hole with the dominant surrounding color (0x%08X)", f.at(40, 35));
        float tol = cv::SmartMoveAutoTolerance(img);
        CHECK(tol >= 2 && tol <= 75, "Smart Move automatic tolerance in the range [2;75]: %.2f", tol);
    }

    printf("[3] .snap format (structured storage with a header)\n");
    {
        std::wstring p = Tmp(L"doc.snap");
        SnapFrame f; f.base = Pattern(321, 123, 9);
        f.doc.setW("CaptureMode", L"Region ç → 中文");    // non-ASCII on purpose: checks UTF-8 metadata
        XmlNode& objs = f.doc.add("Objects");
        objs.add("Arrow").setd("X1", 1.5).setd("Y1", 2).setd("X2", 100).setd("Y2", 50).set("PenColor", "#FFE5322D");
        f.embedded["Image_1.png"] = Pattern(10, 10, 1);
        objs.add("Image").set("StreamName", "Image_1.png");
        bool saved = SaveSnapFile(p, { f });
        std::vector<SnapFrame> in;
        bool loaded = LoadSnapFile(p, in);
        CHECK(saved && loaded && IsSnapFile(p), "save/open .snap");
        CHECK(loaded && in[0].base.px == f.base.px, "base image identical after a round trip");
        CHECK(loaded && in[0].doc.getW("CaptureMode") == L"Region ç → 中文", "UTF-8 metadata preserved");
        CHECK(loaded && in[0].embedded.count("Image_1.png") && in[0].embedded["Image_1.png"].w == 10, "embedded image by StreamName");
        auto d = ed::Document::Load(p);
        CHECK(d && d->st.objs.size() == 2 && d->st.objs[0]->type == ed::ObjType::Arrow, "Document loads objects (%zu)", d ? d->st.objs.size() : 0);
        DeleteFileW(p.c_str());
    }

    printf("[4] Rendering of every object type + PDF\n");
    {
        auto d = ed::Document::FromImage(Pattern(400, 300, 4), L"t");
        for (int t = 0; t < (int)ed::ObjType::Count; t++) {
            auto o = std::make_shared<ed::DrawObj>();
            o->type = (ed::ObjType)t; o->p1 = { 20.0f + t * 5, 20 }; o->p2 = { 200.0f, 150.0f + t * 3 };
            o->text = L"Hello"; o->tail = { 250, 250 }; o->pts = { { 10, 10 }, { 50, 60 }, { 90, 20 } };
            if (o->type == ed::ObjType::Picture) o->picture = std::make_shared<Image>(Pattern(30, 30, 2));
            d->st.objs.push_back(o);
        }
        Image r = d->Render();
        CHECK(r.w == 400 && r.h == 300 && r.px != d->st.base->px, "render of %zu objects", d->st.objs.size());
        // tool variations: 24 callouts, ends, dash patterns, shapes, steps, connected magnifier and quick styles
        auto v = ed::Document::FromImage(Pattern(600, 400, 3), L"v");
        for (int k = 0; k < ed::kCalloutShapes; k++) {
            auto o = std::make_shared<ed::DrawObj>();
            o->type = ed::ObjType::Callout; o->style.calloutShape = k; o->style.fill = 0xFF2080E0; o->style.width = 2;
            o->p1 = { 10.0f + (k % 6) * 95, 10.0f + (k / 6) * 90 }; o->p2 = { o->p1.X + 70, o->p1.Y + 45 }; o->tail = { o->p1.X + 90, o->p1.Y + 80 };
            o->text = L"Hello"; o->style.shaded = k % 2 == 0;
            v->st.objs.push_back(o);
        }
        for (int k = 0; k < ed::CapCount; k++) {
            auto o = std::make_shared<ed::DrawObj>();
            o->type = ed::ObjType::Arrow; o->style.arrowStart = k; o->style.arrowEnd = (k + 3) % ed::CapCount; o->style.dash = k % ed::DashCount;
            o->style.bezier = k % 2 == 1; o->p1 = { 20.0f, 30.0f + k * 40 }; o->p2 = { 300.0f, 50.0f + k * 40 }; o->ctrl = { 160.0f, 10.0f + k * 40 };
            v->st.objs.push_back(o);
        }
        for (int k = 0; k < ed::ShapeCount; k++) {
            auto o = std::make_shared<ed::DrawObj>();
            o->type = ed::ObjType::Shape; o->style.shapeKind = k; o->style.fill = k % 2 ? 0xFF40A040 : 0; o->style.shadowDir = k % 9;
            o->p1 = { 320.0f + (k % 4) * 65, 20.0f + (k / 4) * 70 }; o->p2 = { o->p1.X + 55, o->p1.Y + 50 };
            v->st.objs.push_back(o);
        }
        for (int k = 0; k < ed::StepTypeCount * 4; k++) {
            auto o = std::make_shared<ed::DrawObj>();
            o->type = ed::ObjType::Step; o->style.stepShape = k % 4; o->style.stepType = k / 4; o->stepValue = k + 1;
            o->p1 = o->p2 = { 330.0f + (k % 5) * 50, 200.0f + (k / 5) * 45 };
            v->st.objs.push_back(o);
        }
        auto mg = std::make_shared<ed::DrawObj>();
        mg->type = ed::ObjType::Magnify; mg->style.magnifyLink = true; mg->p1 = { 450, 300 }; mg->p2 = { 560, 390 }; mg->tail = { 380, 340 };
        v->st.objs.push_back(mg);
        auto hl = std::make_shared<ed::DrawObj>();
        hl->type = ed::ObjType::Highlighter; hl->style.stroke = 0xFFFFEB3B; hl->style.opacity = 50; hl->p1 = { 10, 360 }; hl->p2 = { 300, 390 };
        v->st.objs.push_back(hl);
        Image vr = v->Render();
        if (const wchar_t* dump = _wgetenv(L"SNAPNAB_DUMP")) {
            SaveImageFile(std::wstring(dump) + L"\\variacoes.png", vr);
            Image ic(96, 96, 0xFFFFFFFF);                 // enlarged Steps icon, to check the drawing
            { auto bmp = ic.WrapGdip(); Gdiplus::Graphics gi(bmp.get()); icons::Draw(gi, icons::Step, Gdiplus::RectF(0, 0, 96, 96), Gdiplus::Color(255, 56, 56, 62)); }
            SaveImageFile(std::wstring(dump) + L"\\icone_passos.png", ic);
            Image fi(150, 96, 0xFFFFFFFF);                // Fill enlarged and at the toolbar sizes (24 and 20 px)
            {
                auto bmp = fi.WrapGdip(); Gdiplus::Graphics gi(bmp.get());
                Gdiplus::Color ink(255, 56, 56, 62);
                icons::Draw(gi, icons::Fill, Gdiplus::RectF(0, 0, 96, 96), ink);
                icons::Draw(gi, icons::Fill, Gdiplus::RectF(108, 20, 24, 24), ink);
                icons::Draw(gi, icons::Fill, Gdiplus::RectF(110, 60, 20, 20), ink);
            }
            SaveImageFile(std::wstring(dump) + L"\\icone_preencher.png", fi);
        }
        uint32_t under = vr.at(150, 375), base = v->st.base->at(150, 375);
        CHECK(vr.w == 600 && under != base && CA(under) == 255, "%zu object variations rendered; translucent rectangular highlighter", v->st.objs.size());
        {   // group (CPaintOpGroup): renders the children and comes back unchanged from the .snap
            auto gd = ed::Document::FromImage(Pattern(300, 200, 5), L"g");
            auto grp = std::make_shared<ed::DrawObj>();
            grp->type = ed::ObjType::Group;
            ed::DrawObj a; a.type = ed::ObjType::Shape; a.p1 = { 10, 10 }; a.p2 = { 80, 60 };
            ed::DrawObj b2; b2.type = ed::ObjType::Arrow; b2.p1 = { 100, 150 }; b2.p2 = { 200, 40 };
            grp->children = { a, b2 };
            grp->UpdateGroupRect();
            gd->st.objs.push_back(grp);
            gd->path = Tmp(L"group.snap");
            bool saved = gd->Save();
            auto back = ed::Document::Load(gd->path);
            bool ok = saved && back && back->st.objs.size() == 1 && back->st.objs[0]->type == ed::ObjType::Group &&
                      back->st.objs[0]->children.size() == 2 && back->st.objs[0]->children[1].type == ed::ObjType::Arrow;
            Image gr = back ? back->Render() : Image();
            CHECK(ok && gr.px != gd->st.base->px && back->st.objs[0]->HitTest({ 150, 95 }, 3), "group with 2 objects written, read and drawn");
            DeleteFileW(gd->path.c_str());
        }
        int previews = 0;
        for (int t = 0; t < (int)ed::Tool::Count; t++)
            for (auto& st : ed::GenerateStyles((ed::Tool)t, { 0xFFEF4136, 0xFF6A2C91, 0xFF15A589, 0xFFF7A823 }))
                previews += ed::QuickStylePreview((ed::Tool)t, st, 48).w == 48;
        CHECK(previews > 100, "%d quick style thumbnails", previews);
        std::wstring pdf = Tmp(L"out.pdf"), err;
        bool ok = ed::SavePdf(pdf, r, err);
        CHECK(ok && FileSize(pdf) > 1000, "PDF written (%llu bytes)", (unsigned long long)FileSize(pdf));
        DeleteFileW(pdf.c_str());
    }

    printf("[5] Animated GIF (LZW, delta frames, 4 and 8 bpp)\n");
    for (int bpp : { 8, 4 }) {
        std::wstring p = Tmp(L"anim.gif");
        GifWriter gw;
        gw.Open(p, 120, 90, 10, bpp, true, true);
        for (int i = 0; i < 6; i++) { Image fr = Pattern(120, 90, 0); fr.FillRect({ i * 15, 20, i * 15 + 20, 60 }, 0xFF00AA00); gw.AddFrame(fr); }
        bool closed = gw.Close();
        Image dec;
        bool decoded = LoadImageFile(p, dec);
        CHECK(closed && decoded && dec.w == 120 && dec.h == 90, "GIF %d bpp valid and decodable by WIC (%llu bytes)", bpp, (unsigned long long)FileSize(p));
        DeleteFileW(p.c_str());
    }

    printf("[6] Effects\n");
    {
        Image s = Pattern(80, 60, 5);
        CHECK(fx::Border(s, 5, 0xFF000000).w == 90, "border");
        CHECK(fx::Rotate90(s, true).w == 60, "rotate 90");
        Image t(50, 40, 0xFFFFFFFF); t.FillRect({ 10, 10, 30, 25 }, 0xFF123456);
        Image tr = fx::AutoTrim(t);
        CHECK(tr.w == 20 && tr.h == 15, "auto trim (%dx%d)", tr.w, tr.h);
        Image e = fx::EdgeEffect(s, fx::Edge::Torn, 10, fx::All, true);
        CHECK(!e.empty() && e.HasTransparency(), "torn edge with shadow");
        // all effects of the capture window's "Effects" menu at once
        fx::CaptureEffects all;
        all.border = all.filter = all.shadow = all.caption = all.replaceColor = all.scale = true;
        all.edge = all.trim = all.adjust = all.rotate = all.watermark = all.resolution = true;
        all.scalePercent = 50; all.rotateKind = 0; all.dpi = 300; all.filterKind = 1;
        Image fxAll = fx::ApplyCaptureEffects(s, all);
        CHECK(!fxAll.empty() && fxAll.dpi == 300.0f, "12 capture effects applied (%dx%d)", fxAll.w, fxAll.h);
        std::wstring dp = Tmp(L"dpi.png");
        Image back;
        CHECK(SaveImageFile(dp, fxAll) && LoadImageFile(dp, back) && back.w == fxAll.w, "PNG with stored resolution");
        DeleteFileW(dp.c_str());
    }

    printf("[7] Shared memory IPC (sender → receiver with ack)\n");
    {
        HANDLE fakeEditor = CreateMutexW(nullptr, FALSE, kEditorMutex);
        ipc::Notify editor, capture;
        std::atomic<int> got{ 0 }; std::wstring text;
        editor.Start(ipc::kEditor, [&](const ipc::Message& m) { if (m.code == ipc::OpenFile) text = m.text; got++; });
        capture.Start(ipc::kCapture, nullptr);
        bool s1 = capture.SendInt(ipc::kEditor, ipc::ShowEditor, 1, true);
        bool s2 = capture.SendString(ipc::kEditor, ipc::OpenFile, L"C:\\x\\captura.snap", true);
        for (int i = 0; i < 50 && got < 2; i++) Sleep(20);
        CHECK(s1 && s2 && got == 2 && text == L"C:\\x\\captura.snap", "2 messages delivered (int and string)");
        editor.Stop(); capture.Stop();
        CloseHandle(fakeEditor);
    }

    printf("[8] MP4 recording (H.264/AAC) and frame orientation\n");
    std::wstring mp4 = Tmp(L"rec.mp4");
    {
        // test window with a known vertical gradient, to check the orientation
        WNDCLASSW wc{}; wc.lpfnWndProc = [](HWND h, UINT m, WPARAM w, LPARAM l) -> LRESULT {
            if (m == WM_PAINT) {
                // left half: vertical gradient (orientation); right half: white (misaligned rows show up)
                PAINTSTRUCT ps; HDC dc = BeginPaint(h, &ps); RECT r; GetClientRect(h, &r);
                for (int y = 0; y < r.bottom; y += 4) { RECT b{ 0, y, r.right / 2, y + 4 }; HBRUSH br = CreateSolidBrush(RGB(y * 255 / r.bottom, 40, 255 - y * 255 / r.bottom)); FillRect(dc, &b, br); DeleteObject(br); }
                RECT wr{ r.right / 2, 0, r.right, r.bottom }; FillRect(dc, &wr, (HBRUSH)GetStockObject(WHITE_BRUSH));
                EndPaint(h, &ps); return 0;
            }
            return DefWindowProcW(h, m, w, l);
        };
        wc.hInstance = GetModuleHandleW(nullptr); wc.lpszClassName = L"SnapnabTestWnd";
        RegisterClassW(&wc);
        // 854 px: width that is not a multiple of 16 (the decoder delivers padded rows)
        HWND w = CreateWindowExW(WS_EX_TOPMOST, L"SnapnabTestWnd", L"t", WS_POPUP | WS_VISIBLE, 100, 100, 854, 360, nullptr, nullptr, wc.hInstance, nullptr);
        UpdateWindow(w);
        MSG msg; for (int i = 0; i < 20; i++) { while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) DispatchMessageW(&msg); Sleep(20); }
        Recorder rec;
        RecordSettings rs; rs.rect = { 100, 100, 954, 460 }; rs.fps = 15; rs.path = mp4; rs.hardware = false;
        rs.microphone = false; rs.systemAudio = true;
        bool started = rec.Start(rs, nullptr, 0);
        CHECK(started, "recorder started %s", started ? "" : WToUtf8(rec.lastError).c_str());
        for (int i = 0; i < 60; i++) { while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) DispatchMessageW(&msg); Sleep(25); }
        rec.Pause(); Sleep(500); rec.Resume();
        for (int i = 0; i < 30; i++) { while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) DispatchMessageW(&msg); Sleep(25); }
        double el = rec.Elapsed();
        bool stopped = rec.Stop();
        DestroyWindow(w);
        CHECK(stopped && FileSize(mp4) > 2000, "MP4 finalized (%llu bytes, %.2f s recorded, pause excluded)", (unsigned long long)FileSize(mp4), el);
        ed::VideoReader rd;
        Image f; LONGLONG ts;
        bool opened = rd.Open(mp4) && rd.ReadFrame(f, ts);
        CHECK(opened && rd.width == 854 && rd.height == 360, "MP4 decodable (%dx%d, %.2f s, %.1f fps)", rd.width, rd.height, rd.duration / 1e7, rd.fps);
        if (opened) {
            // the top must be blue (high B, low R); the bottom red
            uint32_t top = f.at(200, 10), bot = f.at(200, 350);
            CHECK(CB(top) > CR(top) && CR(bot) > CB(bot), "correct orientation (top B=%d R=%d, bottom R=%d B=%d)", CB(top), CR(top), CR(bot), CB(bot));
            // aligned rows: the gradient | white boundary stays in the middle of the image at every height
            bool aligned = true;
            for (int y : { 20, 180, 340 }) aligned &= CG(f.at(400, y)) < 120 && CG(f.at(460, y)) > 200;
            CHECK(aligned, "frame rows aligned with a width that is not a multiple of 16 (y=340: left G=%d, right G=%d)", CG(f.at(400, 340)), CG(f.at(460, 340)));
            CHECK(rd.duration > 1.0e7 && rd.duration < 3.5e7, "duration consistent with the recorded time without the pause (%.2f s)", rd.duration / 1e7);
        }
    }

    printf("[9] Video cutting (ToFileWithCuts) and GIF from video\n");
    {
        std::vector<ed::Interval> cuts = { { 3000000, 8000000 } };    // removes 0.3 s – 0.8 s
        CHECK(ed::PrecedingCutsDuration(cuts, 9000000) == 5000000 && ed::IsInsideAnyCut(cuts, 5000000) && !ed::IsInsideAnyCut(cuts, 9000000),
              "PrecedingCutsDuration / IsInsideAnyCut");
        std::wstring out = Tmp(L"cut.mp4"), err;
        bool ok = ed::TranscodeWithCuts(mp4, out, cuts, nullptr, err);
        ed::VideoReader a, b;
        a.Open(mp4); b.Open(out);
        double diff = (a.duration - b.duration) / 1e7;
        CHECK(ok && std::fabs(diff - 0.5) < 0.2, "cut MP4 is %.2f s shorter (expected 0.5) %s", diff, WToUtf8(err).c_str());
        a.Close(); b.Close();
        std::wstring gif = Tmp(L"from_video.gif");
        bool gok = ed::CreateGifFromVideo(mp4, gif, 0, 0, 8, 320, 8, true, true, nullptr, err);
        Image g;
        CHECK(gok && LoadImageFile(gif, g) && g.w == 320, "GIF from video (%llu bytes)", (unsigned long long)FileSize(gif));
        DeleteFileW(out.c_str()); DeleteFileW(gif.c_str());
        if (!ok) CopyFileW(mp4.c_str(), Tmp(L"failed.mp4").c_str(), FALSE);
    }
    DeleteFileW(mp4.c_str());

    printf("[10] OCR (Windows.Media.Ocr)\n");
    {
        Image img(600, 120, 0xFFFFFFFF);
        auto bmp = img.WrapGdip();
        { Gdiplus::Graphics gr(bmp.get()); gr.SetTextRenderingHint(Gdiplus::TextRenderingHintAntiAlias);
          Gdiplus::FontFamily ff(L"Segoe UI"); Gdiplus::Font fnt(&ff, 48, Gdiplus::FontStyleRegular, Gdiplus::UnitPixel);
          Gdiplus::SolidBrush br(Gdiplus::Color(255, 0, 0, 0)); gr.DrawString(L"Snapnab OCR 2026", -1, &fnt, Gdiplus::PointF(20, 25), &br); }
        std::wstring text, err;
        bool ok = RunOcr(img, text, err);
        bool match = text.find(L"Snapnab") != std::wstring::npos || text.find(L"OCR") != std::wstring::npos;
        if (!ok) printf("  info OCR unavailable: %s\n", WToUtf8(err).c_str());
        else CHECK(match, "recognized text: \"%s\"", WToUtf8(Trim(text)).c_str());
    }

    printf("[11] Panoramic scrolling (frame stitching)\n");
    {
        auto noise = [](int w, int h) {
            Image img(w, h);
            for (int y = 0; y < h; y++)
                for (int x = 0; x < w; x++) {
                    uint32_t v = (uint32_t)(x * 73856093u ^ y * 19349663u);
                    v ^= v >> 13; v *= 0x5bd1e995u; v ^= v >> 15;
                    img.at(x, y) = 0xFF000000 | (v & 0xFFFFFF);
                }
            return img;
        };
        // vertical, with a move back up in the middle (the move back must not duplicate content)
        Image page = noise(300, 1000);
        PanoramaStitcher v;
        for (int y : { 0, 37, 90, 150, 240, 200, 300, 380, 470, 560, 640, 720, 800 }) v.Add(page.Crop({ 0, y, 300, y + 200 }));
        CHECK(v.axis() == 1 && v.result().w == 300 && v.result().h == 1000 && v.result().px == page.px,
              "vertical: %dx%d rebuilt identical to the page", v.result().w, v.result().h);
        // horizontal
        Image wide = noise(1200, 150);
        PanoramaStitcher hz;
        for (int x = 0; x <= 900; x += 60) hz.Add(wide.Crop({ x, 0, x + 300, 150 }));
        CHECK(hz.axis() == 2 && hz.result().w == 1200 && hz.result().px == wide.px, "horizontal: %dx%d", hz.result().w, hz.result().h);
        // a repeated frame (nothing scrolled) adds nothing
        PanoramaStitcher same;
        same.Add(page.Crop({ 0, 0, 300, 200 }));
        bool added = same.Add(page.Crop({ 0, 0, 300, 200 }));
        CHECK(!added && same.result().h == 200, "an unchanged frame is ignored");
    }

    printf("[12] Webcam chroma key\n");
    {
        COLORREF green = RGB(0, 177, 64);
        uint8_t bg = ChromaAlpha(0xFF00B140, green, 40), bgDark = ChromaAlpha(0xFF00702A, green, 40);
        uint8_t skin = ChromaAlpha(0xFFE0AC90, green, 40), shirt = ChromaAlpha(0xFF2050C0, green, 40);
        CHECK(bg == 0 && bgDark == 0, "green background (lit and in shadow) removed (alpha %d, %d)", bg, bgDark);
        CHECK(skin == 255 && shirt == 255, "skin and blue shirt kept (alpha %d, %d)", skin, shirt);
    }

    printf("[14] Editor batch: shapes, gradients, labels, layers, clipboard, PDF links\n");
    {
        using ed::DrawObj; using ed::ObjType;
        auto solid = [](int w, int h, uint32_t c) { return Image(w, h, c); };
        auto render1 = [&](const Image& base, std::function<void(DrawObj&)> make) {
            auto d = ed::Document::FromImage(base, L"b");
            auto o = std::make_shared<DrawObj>();
            make(*o);
            d->st.objs.push_back(o);
            return d->Render();
        };
        auto similar = [](uint32_t a, uint32_t b, int tol) {
            return std::abs(CR(a) - CR(b)) <= tol && std::abs(CG(a) - CG(b)) <= tol && std::abs(CB(a) - CB(b)) <= tol;
        };
        // highlighter as an ellipse: center tinted, corner of the box untouched
        Image white = solid(200, 160, 0xFFFFFFFF);
        Image he = render1(white, [](DrawObj& o) { o.type = ObjType::Highlighter; o.style.stroke = 0xFFFF0000; o.style.opacity = 100;
                                                     o.style.shadow = false; o.style.areaShape = ed::ShapeEllipse; o.p1 = { 20, 20 }; o.p2 = { 180, 140 }; });
        CHECK(similar(he.at(100, 80), 0xFFFF0000, 10) && similar(he.at(24, 24), 0xFFFFFFFF, 2), "highlighter ellipse: fills the ellipse, not the box corner");
        // polygon highlighter (clicked corners): inside tinted, outside untouched
        Image hp = render1(white, [](DrawObj& o) { o.type = ObjType::Highlighter; o.style.stroke = 0xFF0000FF; o.style.shadow = false;
                                                     o.style.areaShape = ed::AreaPolygon; o.pts = { { 20, 20 }, { 180, 20 }, { 20, 140 } }; o.p1 = { 20, 20 }; o.p2 = { 180, 140 }; });
        CHECK(similar(hp.at(40, 40), 0xFF0000FF, 10) && similar(hp.at(170, 130), 0xFFFFFFFF, 2), "polygon highlighter (triangle) drawn only inside");
        // blur in an ellipse: inside changes, outside the ellipse stays as it was
        Image pat = Pattern(200, 160, 9);
        Image be = render1(pat, [](DrawObj& o) { o.type = ObjType::Blur; o.style.amount = 12; o.style.areaShape = ed::ShapeEllipse; o.p1 = { 20, 20 }; o.p2 = { 180, 140 }; });
        CHECK(be.at(100, 80) != pat.at(100, 80) && be.at(22, 22) == pat.at(22, 22), "blur in an ellipse: corner of the box unchanged");
        // freehand pixelate keeps the outside
        Image pf = render1(pat, [](DrawObj& o) { o.type = ObjType::Pixelate; o.style.amount = 10; o.style.areaShape = ed::AreaFreehand;
                                                   o.pts = { { 60, 30 }, { 140, 40 }, { 150, 120 }, { 70, 130 }, { 40, 80 } }; o.p1 = { 40, 30 }; o.p2 = { 150, 130 }; });
        CHECK(pf.at(5, 5) == pat.at(5, 5) && pf.at(100, 80) != pat.at(100, 80), "freehand pixelate: only inside the outline");
        // round spotlight: inside as is, outside darker
        Image sp = render1(white, [](DrawObj& o) { o.type = ObjType::Spotlight; o.style.amount = 8; o.style.areaShape = ed::ShapeEllipse; o.p1 = { 40, 30 }; o.p2 = { 160, 130 }; });
        CHECK(similar(sp.at(100, 80), 0xFFFFFFFF, 2) && CR(sp.at(42, 32)) < 230, "round spotlight darkens outside the ellipse (also the box corner)");
        // linear gradient fill (0° = left to right) in a shape
        Image gs = render1(white, [](DrawObj& o) { o.type = ObjType::Shape; o.style.width = 0; o.style.shadow = false; o.style.fill = 0xFFFF0000;
                                                     o.style.grad.type = 1; o.style.grad.angle = 0; o.style.grad.stops = { { 0, 0xFFFF0000 }, { 1, 0xFF0000FF } };
                                                     o.p1 = { 10, 10 }; o.p2 = { 190, 150 }; });
        CHECK(CR(gs.at(14, 80)) > 200 && CB(gs.at(14, 80)) < 60 && CB(gs.at(186, 80)) > 200 && CR(gs.at(186, 80)) < 60, "shape with a linear gradient (red → blue)");
        Image fillG = solid(100, 100, 0);
        ed::Gradient rg; rg.type = 2; rg.stops = { { 0, 0xFFFFFFFF }, { 1, 0xFF000000 } };
        ed::FillGradientMask(fillG, {}, { 0, 0, 100, 100 }, rg, 100);
        CHECK(CR(fillG.at(50, 50)) > 240 && CR(fillG.at(1, 1)) < 30, "radial gradient fill (Fill tool / canvas color): white center, dark corners");
        // step with an outline: the ring outside the circle takes the outline color
        Image so = render1(solid(120, 120, 0xFF000000), [](DrawObj& o) { o.type = ObjType::Step; o.style.shadow = false; o.style.stroke = 0xFF2060FF;
                                                                            o.style.fontSize = 40; o.style.stepOutline = 0xFFFFFFFF; o.style.stepOutlineWidth = 6; o.p1 = o.p2 = { 60, 60 }; });
        CHECK(similar(so.at(60, 60 - 29), 0xFFFFFFFF, 40), "step with a white outline (contrast on a dark background)");
        // line spacing: double spacing makes the text block taller
        DrawObj tx; tx.type = ObjType::Text; tx.text = L"one\ntwo\nthree"; tx.style.fontSize = 20;
        float h1 = ed::TextBlockHeight(tx, 300);
        tx.style.lineSpacing = 2.0f;
        float h2 = ed::TextBlockHeight(tx, 300);
        CHECK(h2 > h1 * 1.6f, "line spacing 2× (%.0f px → %.0f px)", h1, h2);
        // arrow label at the tail: drawn beyond the tail, part of the bounds and of the hit test
        DrawObj ar; ar.type = ObjType::Arrow; ar.p1 = { 150, 80 }; ar.p2 = { 280, 80 }; ar.text = L"Label"; ar.style.fontSize = 24; ar.style.shadow = false;
        Gdiplus::RectF lb = ed::ArrowLabelRect(ar);
        CHECK(lb.GetRight() <= 150 && ar.Bounds().X <= lb.X + 1 && ar.HitTest({ lb.X + lb.Width / 2, lb.Y + lb.Height / 2 }, 2), "arrow label beyond the tail (bounds and hit test)");
        Image al = render1(solid(300, 160, 0xFFFFFFFF), [&](DrawObj& o) { o = ar; });
        int inked = 0;
        for (int y = (int)lb.Y; y < (int)lb.GetBottom(); y++) for (int x = (int)lb.X; x < (int)lb.GetRight(); x++) inked += !similar(al.at(x, y), 0xFFFFFFFF, 30);
        CHECK(inked > 30, "arrow label text drawn (%d pixels)", inked);
        // frosted callout: what is behind the body gets blurred even with a transparent fill
        Image fc = render1(pat, [](DrawObj& o) { o.type = ObjType::Callout; o.style.calloutShape = 16; o.style.fill = 0; o.style.width = 0; o.style.shadow = false;
                                                   o.style.bgBlur = true; o.style.bgBlurAmount = 10; o.p1 = { 40, 30 }; o.p2 = { 160, 130 }; });
        CHECK(fc.at(100, 80) != pat.at(100, 80) && fc.at(5, 5) == pat.at(5, 5), "callout background blur (frosted glass)");
        // loupe with a handle: bounds include the handle, which is drawn outside the lens
        DrawObj lp; lp.type = ObjType::Magnify; lp.style.loupe = true; lp.style.loupeHandle = true; lp.style.stroke = 0xFF303030; lp.style.shadow = false;
        lp.p1 = { 20, 20 }; lp.p2 = { 100, 100 };
        Gdiplus::RectF lpb = lp.Bounds();
        Image lr = render1(solid(200, 200, 0xFFFFFFFF), [&](DrawObj& o) { o = lp; });
        CHECK(lpb.GetRight() > 120 && !similar(lr.at(105, 105), 0xFFFFFFFF, 30), "loupe: handle outside the lens drawn and inside the bounds");
        // label stamp: wide badge with the text
        float asp = ed::StampAspect(L"label:2E9E5B:Depois");
        Image ls = ed::StampImage(L"label:2E9E5B:Depois", 96);
        CHECK(asp > 1.5f && !ls.empty() && similar(ls.at(6, ls.h * 3 / 4), 0xFF2E9E5B, 50), "label stamp (aspect %.1f)", asp);
        // hidden objects are not rendered nor exported; locked/name/visibility, guides and links survive the .snap
        auto d = ed::Document::FromImage(solid(120, 90, 0xFFFFFFFF), L"l");
        auto hid = std::make_shared<DrawObj>(); hid->type = ObjType::Shape; hid->style.fill = 0xFF000000; hid->style.shadow = false; hid->p1 = { 10, 10 }; hid->p2 = { 60, 60 };
        hid->hidden = true; hid->locked = true; hid->name = L"Fundo azul ç";
        auto lk = std::make_shared<DrawObj>(); lk->type = ObjType::Link; lk->text = L"https://example.com/a b"; lk->p1 = { 70, 10 }; lk->p2 = { 110, 30 };
        auto pt = std::make_shared<DrawObj>(); pt->type = ObjType::Highlighter; pt->style.areaShape = ed::AreaPolygon; pt->pts = { { 5, 70 }, { 40, 60 }, { 30, 85 } };
        pt->p1 = { 5, 60 }; pt->p2 = { 40, 85 }; pt->floating = true; pt->style.lineSpacing = 1.5f;
        auto st = std::make_shared<DrawObj>(); st->type = ObjType::Step; st->style.stepOutlineWidth = 4; st->style.stepOutline = 0xFF112233; st->p1 = st->p2 = { 90, 70 };
        auto sh = std::make_shared<DrawObj>(); sh->type = ObjType::Shape; sh->style.grad.type = 2; sh->style.grad.stops = { { 0, 0xFFFFFFFF }, { 0.5f, 0xFF00FF00 }, { 1, 0xFF000000 } };
        sh->p1 = { 0, 0 }; sh->p2 = { 5, 5 };
        auto co = std::make_shared<DrawObj>(); co->type = ObjType::Callout; co->style.bgBlur = true; co->style.bgBlurAmount = 13; co->style.lineSpacing = 2.5f; co->text = L"x";
        co->p1 = { 0, 0 }; co->p2 = { 10, 10 };
        auto mg2 = std::make_shared<DrawObj>(); mg2->type = ObjType::Magnify; mg2->style.loupe = true; mg2->style.loupeHandle = false; mg2->style.magnifyShape = ed::MagStar;
        auto arw = std::make_shared<DrawObj>(); arw->type = ObjType::Arrow; arw->text = L"Olá"; arw->style.fontSize = 31; arw->p1 = { 1, 1 }; arw->p2 = { 50, 1 };
        d->st.objs = { hid, lk, pt, st, sh, co, mg2, arw };
        d->guidesX = { 12.5f }; d->guidesY = { 40 };
        Image hr = d->Render();
        CHECK(similar(hr.at(30, 30), 0xFFFFFFFF, 2) && similar(hr.at(80, 15), 0xFFFFFFFF, 2), "hidden object and link areas are not drawn");
        d->path = Tmp(L"batch.snap");
        bool saved = d->Save();
        auto back = ed::Document::Load(d->path);
        bool ok = saved && back && back->st.objs.size() == 8;
        if (ok) {
            auto& o = back->st.objs;
            ok = o[0]->hidden && o[0]->locked && o[0]->name == L"Fundo azul ç" && o[1]->type == ObjType::Link && o[1]->text == L"https://example.com/a b" &&
                 o[2]->style.areaShape == ed::AreaPolygon && o[2]->pts.size() == 3 && o[2]->floating && o[3]->style.stepOutlineWidth == 4 &&
                 o[3]->style.stepOutline == 0xFF112233 && o[4]->style.grad.type == 2 && o[4]->style.grad.stops.size() == 3 &&
                 o[4]->style.grad.stops[1].color == 0xFF00FF00 && o[5]->style.bgBlur && o[5]->style.bgBlurAmount == 13 &&
                 std::fabs(o[5]->style.lineSpacing - 2.5f) < 0.01f && o[6]->style.loupe && !o[6]->style.loupeHandle && o[6]->style.magnifyShape == ed::MagStar &&
                 o[7]->text == L"Olá" && o[7]->style.fontSize == 31 && back->guidesX.size() == 1 && back->guidesY.size() == 1 && back->guidesY[0] == 40;
        }
        CHECK(ok, ".snap round trip of the new properties (name, hidden, locked, link, polygon, outline, gradient, blur, spacing, loupe, label, guides)");
        DeleteFileW(d->path.c_str());
        // objects written by older versions: the new properties take their defaults
        XmlNode old("Rect");
        old.setd("X1", 1).setd("Y1", 2).setd("X2", 30).setd("Y2", 40).set("PenColor", "#FFFF0000").seti("Opacity", 100);
        SnapFrame dummy;
        auto oo = ed::ObjFromXml(old, dummy);
        XmlNode oldHl("Highlight");
        oldHl.setd("X1", 1).setd("Y1", 2).setd("X2", 30).setd("Y2", 40).set("Points", "1.0,2.0 10.0,20.0 30.0,40.0 ");
        auto oh = ed::ObjFromXml(oldHl, dummy);
        CHECK(oo && !oo->hidden && !oo->locked && oo->name.empty() && !oo->style.grad.Active() && oo->style.lineSpacing == 1.0f &&
              oo->style.areaShape == ed::ShapeRect && oo->style.stepOutlineWidth == 0 && !oo->style.bgBlur && !oo->style.loupe &&
              oh && oh->IsPath() && !oh->IsPointShape(), "old objects open with the defaults (old freehand highlighter still a path)");
        // quick styles keep the new properties
        ed::Style qs; qs.lineSpacing = 1.8f; qs.stepOutlineWidth = 2; qs.areaShape = ed::ShapeRoundRect; qs.loupe = true; qs.bgBlur = true;
        qs.grad.type = 1; qs.grad.angle = 45; qs.grad.stops = { { 0, 0xFF000000 }, { 1, 0xFFFFFFFF } };
        ed::Style qb = ed::StyleFromXml(ed::StyleToXml(qs));
        CHECK(ed::SameStyle(qs, qb) && qb.areaShape == ed::ShapeRoundRect && qb.grad.angle == 45, "quick style round trip with spacing, outline, area shape, loupe, blur and gradient");
        // clipboard: objects (with an embedded image) copied in one image and pasted in another
        DrawObj pic; pic.type = ObjType::Picture; pic.picture = std::make_shared<Image>(Pattern(12, 9, 4)); pic.p1 = { 33, 44 }; pic.p2 = { 45, 53 };
        DrawObj tt; tt.type = ObjType::Text; tt.text = L"Copiado"; tt.p1 = { 5, 6 }; tt.p2 = { 100, 40 };
        std::wstring src;
        auto clip = ed::ObjectsFromClip(ed::ObjectsToClip({ pic, tt }, L"C:\\doc.snap"), &src);
        CHECK(clip.size() == 2 && src == L"C:\\doc.snap" && clip[0].picture && clip[0].picture->px == pic.picture->px && clip[0].p1.X == 33 &&
              clip[1].text == L"Copiado", "objects on the clipboard: positions, text and embedded image preserved");
        // source formatting of copied text (paste keeping the formatting)
        ed::Style rf;
        bool rtf = ed::ParseRtfFormat("{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0\\fswiss Arial;}{\\f1\\fmodern Consolas;}}{\\colortbl;\\red255\\green0\\blue0;}"
                                      "\\pard\\f1\\fs32\\b\\cf1 Hello}", rf);
        CHECK(rtf && rf.font == L"Consolas" && std::fabs(rf.fontSize - 21.33f) < 0.1f && rf.bold && rf.textColor == 0xFFFF0000, "RTF: font, size, bold and color of the first run");
        ed::Style hf;
        bool html = ed::ParseHtmlFormat("Version:0.9\r\n<html><body><!--StartFragment--><span style=\"font-family: 'Fira Code', monospace; font-size: 14px; color: #569cd6\"><i>int</i></span><!--EndFragment--></body></html>", hf);
        CHECK(html && hf.font == L"Fira Code" && hf.fontSize == 14 && hf.textColor == 0xFF569CD6 && hf.italic, "HTML: font, size, color and italic of the first text");
        // PDF: the captured links become /Link annotations with the address
        std::wstring lpdf = Tmp(L"links.pdf"), lerr;
        bool pok = ed::SavePdf(lpdf, solid(200, 100, 0xFFFFFFFF), lerr, { { Gdiplus::RectF(10, 10, 50, 20), L"https://example.com/x(1)" } });
        std::string body;
        { HANDLE fh = CreateFileW(lpdf.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr);
          if (fh != INVALID_HANDLE_VALUE) { DWORD n = GetFileSize(fh, nullptr), rd = 0; body.resize(n); ReadFile(fh, body.data(), n, &rd, nullptr); CloseHandle(fh); } }
        CHECK(pok && body.find("/Subtype /Link") != std::string::npos && body.find("(https://example.com/x\\(1\\))") != std::string::npos, "PDF with a clickable link");
        DeleteFileW(lpdf.c_str());
        // the new icons draw something
        int drawn = 0;
        for (auto id : { icons::Lock, icons::Unlock, icons::Eye, icons::EyeOff, icons::Pin, icons::AlignLeft, icons::AlignBottom, icons::DistributeH,
                         icons::DistributeV, icons::Ruler, icons::Link, icons::Group, icons::Sliders }) {
            Image ic(24, 24, 0);
            { auto bmp = ic.WrapGdip(); Gdiplus::Graphics gi(bmp.get()); icons::Draw(gi, id, Gdiplus::RectF(0, 0, 24, 24), Gdiplus::Color(255, 0, 0, 0)); }
            drawn += std::count_if(ic.px.begin(), ic.px.end(), [](uint32_t p) { return CA(p) > 0; }) > 20;
        }
        CHECK(drawn == 13, "new icons (lock, eye, pin, align, distribute, ruler, link, group, volume): %d of 13", drawn);
        // a polygon area whose corners do not exist yet (shape just switched) is drawn as its box, not skipped
        Image hn = render1(white, [](DrawObj& o) { o.type = ObjType::Highlighter; o.style.stroke = 0xFFFF0000; o.style.opacity = 100;
                                                     o.style.shadow = false; o.style.areaShape = ed::AreaPolygon; o.p1 = { 20, 20 }; o.p2 = { 180, 140 }; });
        CHECK(similar(hn.at(30, 30), 0xFFFF0000, 10), "polygon highlighter without corners drawn as its box");
        // settings import with merge: the current items stay, the missing presets, favorites and pinned stamps are added
        {
            std::string cur = "<SavedPresets Version=\"1\"><Presets><CaptureSettings DisplayName=\"A\" SelectionMode=\"0\"/></Presets></SavedPresets>";
            std::string imp = "<SavedPresets Version=\"1\"><Presets><CaptureSettings DisplayName=\"A\" SelectionMode=\"2\"/>"
                              "<CaptureSettings DisplayName=\"B\" SelectionMode=\"1\"/></Presets></SavedPresets>";
            XmlNode m; XmlParse(settings::MergeXml(cur, imp), m);
            const XmlNode* ps = m.child("Presets");
            CHECK(ps && ps->children.size() == 2 && ps->children[0].get("SelectionMode") == "0" && ps->children[1].get("DisplayName") == "B",
                  "settings merge: preset B added inside <Presets>, preset A keeps the current values");
            std::string fcur = "<Favorites><Favorite ToolMode=\"1\"><Style Foreground=\"#FF000000\"/></Favorite></Favorites>";
            std::string fimp = "<Favorites><Favorite ToolMode=\"1\"><Style Foreground=\"#FF000000\"/></Favorite>"
                               "<Favorite ToolMode=\"2\"><Style Foreground=\"#FFFF0000\"/></Favorite></Favorites>";
            XmlNode f; XmlParse(settings::MergeXml(fcur, fimp), f);
            std::string scur = "<StampOrder><Pinned><Item Id=\"a\"/></Pinned></StampOrder>";
            std::string simp = "<StampOrder><Category Key=\"x\"><Item Id=\"b\"/></Category><Pinned><Item Id=\"a\"/><Item Id=\"c\"/></Pinned></StampOrder>";
            XmlNode s; XmlParse(settings::MergeXml(scur, simp), s);
            const XmlNode* pin = s.child("Pinned");
            CHECK(f.children.size() == 2 && s.children.size() == 2 && pin && pin->children.size() == 2,
                  "settings merge: new favorite added once, stamp category and pinned stamp added");
        }
        if (const wchar_t* dump = _wgetenv(L"SNAPNAB_DUMP")) {
            SaveImageFile(std::wstring(dump) + L"\\lote_formas.png", gs);
            SaveImageFile(std::wstring(dump) + L"\\lote_lupa.png", lr);
            SaveImageFile(std::wstring(dump) + L"\\lote_rotulo.png", ls);
            SaveImageFile(std::wstring(dump) + L"\\lote_seta.png", al);
            SaveImageFile(std::wstring(dump) + L"\\lote_passo.png", so);
        }
    }

    printf("[13] Translations (lang\\*.po)\n");
    {
        // sequence of conversions of a printf string (%1$s counts by position): the translation must have the same
        auto specs = [](const std::wstring& s) {
            std::vector<std::pair<int, wchar_t>> v;
            for (size_t i = 0; i < s.size(); i++) {
                if (s[i] != L'%') continue;
                size_t j = i + 1;
                if (j < s.size() && s[j] == L'%') { i = j; continue; }
                int pos = 0;
                size_t k = j;
                while (k < s.size() && iswdigit(s[k])) k++;
                if (k < s.size() && s[k] == L'$' && k > j) { pos = _wtoi(s.substr(j, k - j).c_str()); j = k + 1; }
                while (j < s.size() && wcschr(L"-+ #0", s[j])) j++;
                while (j < s.size() && (iswdigit(s[j]) || s[j] == L'*' || s[j] == L'.')) j++;
                if (s.compare(j, 3, L"I64") == 0) j += 3;
                while (j < s.size() && wcschr(L"hlzjtL", s[j])) j++;
                if (j >= s.size()) break;
                wchar_t c = towlower(s[j]);
                v.push_back({ pos, c == L'i' ? L'd' : c });
                i = j;
            }
            std::stable_sort(v.begin(), v.end(), [](auto& a, auto& b) { return a.first < b.first; });
            std::wstring out;
            for (auto& p : v) out += p.second;
            return out;
        };
        CHECK(Format(L"%2$s, %1$s", L"world", L"hello") == L"hello, world", "Format accepts positional parameters (%%1$s)");
        WIN32_FIND_DATAW fd;
        HANDLE h = FindFirstFileW((ExeDir() + L"\\lang\\*.po").c_str(), &fd);
        size_t full = 0;
        std::vector<std::pair<std::wstring, size_t>> counts;
        if (h != INVALID_HANDLE_VALUE) {
            do {
                std::vector<std::pair<std::wstring, std::wstring>> e;
                bool ok = i18n::LoadPo(ExeDir() + L"\\lang\\" + fd.cFileName, e);
                int bad = 0;
                for (auto& [id, str] : e) if (specs(id) != specs(str)) { if (!bad) printf("       %ls: \"%ls\"\n", fd.cFileName, id.c_str()); bad++; }
                CHECK(ok && !e.empty() && !bad, "%ls: %zu strings, %d with %%d/%%s markers different from the original", fd.cFileName, e.size(), bad);
                counts.push_back({ fd.cFileName, e.size() });
                full = std::max(full, e.size());
            } while (FindNextFileW(h, &fd));
            FindClose(h);
        }
        CHECK(!counts.empty(), "%zu catalogs in the lang folder", counts.size());
        for (auto& [name, n] : counts) CHECK(n == full, "%ls complete (%zu of %zu)", name.c_str(), n, full);
    }

    printf("\n%s (%d failure(s))\n", g_fail ? "FAILED" : "ALL OK", g_fail);
    CoUninitialize();
    return g_fail ? 1 : 0;
}
