// Testes de integração das peças críticas do Snapnab (executável de console, fora do produto)
#include "Common.h"
#include "Image.h"
#include "Cv.h"
#include "Gif.h"
#include "SnapFile.h"
#include "Xml.h"
#include "Ipc.h"
#include "Effects.h"
#include "../src/capture/Recorder.h"
#include "../src/editor/Video.h"
#include "../src/editor/Document.h"
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
            // blocos suaves + ruído: parecido com texto/ícones numa página
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
    if (argc > 2 && std::wstring(argv[1]) == L"dump") {      // depuração: imprime o XML de um .snap
        std::vector<SnapFrame> fr;
        if (!LoadSnapFile(argv[2], fr)) { printf("falha ao abrir\n"); return 1; }
        printf("%s", XmlSerialize(fr[0].doc).c_str());
        return 0;
    }
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (argc > 2 && std::wstring(argv[1]) == L"audio") {    // depuração: lê só a faixa de áudio
        MFStartup(MF_VERSION);
        IMFSourceReader* rd = nullptr;
        HRESULT hr = MFCreateSourceReaderFromURL(argv[2], nullptr, &rd);
        printf("open 0x%08X\n", hr);
        rd->SetStreamSelection((DWORD)MF_SOURCE_READER_ALL_STREAMS, FALSE);
        rd->SetStreamSelection((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, TRUE);
        IMFMediaType* mt = nullptr; MFCreateMediaType(&mt);
        mt->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Audio); mt->SetGUID(MF_MT_SUBTYPE, MFAudioFormat_PCM); mt->SetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
        printf("settype 0x%08X\n", rd->SetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, nullptr, mt));
        for (int i = 0; i < 10000; i++) {
            DWORD idx, flags; LONGLONG ts; IMFSample* smp = nullptr;
            hr = rd->ReadSample((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, 0, &idx, &flags, &ts, &smp);
            if (FAILED(hr)) { printf("sample %d FAILED 0x%08X\n", i, hr); break; }
            if (flags & MF_SOURCE_READERF_ENDOFSTREAM) { printf("EOS after %d samples, last ts %.3f\n", i, ts / 1e7); break; }
            if (i < 3 || i % 50 == 0) printf("sample %d ts %.3f flags %u\n", i, ts / 1e7, flags);
            if (smp) smp->Release();
        }
        return 0;
    }
    GdiplusSession gdip;

    printf("[1] Correlação de fase (costura da rolagem)\n");
    {
        Image page = Pattern(700, 1600, 3);
        for (int s : { 37, 120, 250 }) {
            Image a = page.Crop({ 0, 100, 700, 600 });
            Image b = page.Crop({ 0, 100 + s, 700, 600 + s });       // conteúdo subiu s px (rolou para baixo)
            double conf = 0;
            POINT t = cv::DetectTranslation(a, b, &conf);
            CHECK(-t.y == s && t.x == 0, "rolagem %d px detectada como %d (dx=%d, conf %.1f)", s, -t.y, t.x, conf);
        }
    }

    printf("[2] Varinha mágica / inpainting\n");
    {
        Image img(100, 80, 0xFFFFFFFF);
        img.FillRect({ 20, 20, 60, 50 }, 0xFFFF0000);
        cv::Mask m = cv::DoWand(img, { 30, 30 }, 10, false);
        RECT b = cv::MaskBounds(m, img.w, img.h);
        CHECK(b.left == 20 && b.top == 20 && b.right == 60 && b.bottom == 50, "varinha seleciona o retângulo vermelho (%ld,%ld)-(%ld,%ld)", b.left, b.top, b.right, b.bottom);
        cv::Mask all = cv::DoWand(img, { 5, 5 }, 100, false);
        CHECK(!cv::MaskEmpty(all) && all.size() == 8000 && std::all_of(all.begin(), all.end(), [](uint8_t v) { return v == 255; }), "tolerância 100 = camada inteira");
        Image f = img;
        cv::SmartFill(f, m);
        CHECK(f.at(40, 35) == 0xFFFFFFFF, "Smart fill pinta o buraco com a cor dominante do entorno (0x%08X)", f.at(40, 35));
        float tol = cv::SmartMoveAutoTolerance(img);
        CHECK(tol >= 2 && tol <= 75, "tolerância automática do Smart Move na faixa [2;75]: %.2f", tol);
    }

    printf("[3] Formato .snap (armazenamento estruturado com cabeçalho)\n");
    {
        std::wstring p = Tmp(L"doc.snap");
        SnapFrame f; f.base = Pattern(321, 123, 9);
        f.doc.setW("CaptureMode", L"Região ç");
        XmlNode& objs = f.doc.add("Objects");
        objs.add("Arrow").setd("X1", 1.5).setd("Y1", 2).setd("X2", 100).setd("Y2", 50).set("PenColor", "#FFE5322D");
        f.embedded["Image_1.png"] = Pattern(10, 10, 1);
        objs.add("Image").set("StreamName", "Image_1.png");
        bool saved = SaveSnapFile(p, { f });
        std::vector<SnapFrame> in;
        bool loaded = LoadSnapFile(p, in);
        CHECK(saved && loaded && IsSnapFile(p), "salvar/abrir .snap");
        CHECK(loaded && in[0].base.px == f.base.px, "imagem base idêntica após ida e volta");
        CHECK(loaded && in[0].doc.getW("CaptureMode") == L"Região ç", "metadados UTF-8 preservados");
        CHECK(loaded && in[0].embedded.count("Image_1.png") && in[0].embedded["Image_1.png"].w == 10, "imagem embutida por StreamName");
        auto d = ed::Document::Load(p);
        CHECK(d && d->st.objs.size() == 2 && d->st.objs[0]->type == ed::ObjType::Arrow, "Document carrega objetos (%zu)", d ? d->st.objs.size() : 0);
        DeleteFileW(p.c_str());
    }

    printf("[4] Renderização de todos os tipos de objeto + PDF\n");
    {
        auto d = ed::Document::FromImage(Pattern(400, 300, 4), L"t");
        for (int t = 0; t <= (int)ed::ObjType::Picture; t++) {
            auto o = std::make_shared<ed::DrawObj>();
            o->type = (ed::ObjType)t; o->p1 = { 20.0f + t * 5, 20 }; o->p2 = { 200.0f, 150.0f + t * 3 };
            o->text = L"Olá"; o->tail = { 250, 250 }; o->pts = { { 10, 10 }, { 50, 60 }, { 90, 20 } };
            if (o->type == ed::ObjType::Picture) o->picture = std::make_shared<Image>(Pattern(30, 30, 2));
            d->st.objs.push_back(o);
        }
        Image r = d->Render();
        CHECK(r.w == 400 && r.h == 300 && r.px != d->st.base->px, "render de %zu objetos", d->st.objs.size());
        std::wstring pdf = Tmp(L"out.pdf"), err;
        bool ok = ed::SavePdf(pdf, r, err);
        CHECK(ok && FileSize(pdf) > 1000, "PDF gravado (%llu bytes)", (unsigned long long)FileSize(pdf));
        DeleteFileW(pdf.c_str());
    }

    printf("[5] GIF animado (LZW, quadros delta, 4 e 8 bpp)\n");
    for (int bpp : { 8, 4 }) {
        std::wstring p = Tmp(L"anim.gif");
        GifWriter gw;
        gw.Open(p, 120, 90, 10, bpp, true, true);
        for (int i = 0; i < 6; i++) { Image fr = Pattern(120, 90, 0); fr.FillRect({ i * 15, 20, i * 15 + 20, 60 }, 0xFF00AA00); gw.AddFrame(fr); }
        bool closed = gw.Close();
        Image dec;
        bool decoded = LoadImageFile(p, dec);
        CHECK(closed && decoded && dec.w == 120 && dec.h == 90, "GIF %d bpp válido e decodificável pelo WIC (%llu bytes)", bpp, (unsigned long long)FileSize(p));
        DeleteFileW(p.c_str());
    }

    printf("[6] Efeitos\n");
    {
        Image s = Pattern(80, 60, 5);
        CHECK(fx::Border(s, 5, 0xFF000000).w == 90, "borda");
        CHECK(fx::Rotate90(s, true).w == 60, "girar 90");
        Image t(50, 40, 0xFFFFFFFF); t.FillRect({ 10, 10, 30, 25 }, 0xFF123456);
        Image tr = fx::AutoTrim(t);
        CHECK(tr.w == 20 && tr.h == 15, "corte automático (%dx%d)", tr.w, tr.h);
        Image e = fx::EdgeEffect(s, fx::Edge::Torn, 10, fx::All, true);
        CHECK(!e.empty() && e.HasTransparency(), "borda rasgada com sombra");
        // todos os efeitos do menu "Efeitos" da janela de captura de uma vez
        fx::CaptureEffects all;
        all.border = all.filter = all.shadow = all.caption = all.replaceColor = all.scale = true;
        all.edge = all.trim = all.adjust = all.rotate = all.watermark = all.resolution = true;
        all.scalePercent = 50; all.rotateKind = 0; all.dpi = 300; all.filterKind = 1;
        Image fxAll = fx::ApplyCaptureEffects(s, all);
        CHECK(!fxAll.empty() && fxAll.dpi == 300.0f, "12 efeitos da captura aplicados (%dx%d)", fxAll.w, fxAll.h);
        std::wstring dp = Tmp(L"dpi.png");
        Image back;
        CHECK(SaveImageFile(dp, fxAll) && LoadImageFile(dp, back) && back.w == fxAll.w, "PNG com resolução gravada");
        DeleteFileW(dp.c_str());
    }

    printf("[7] IPC por memória compartilhada (emissor → receptor com ack)\n");
    {
        HANDLE fakeEditor = CreateMutexW(nullptr, FALSE, kEditorMutex);
        ipc::Notify editor, capture;
        std::atomic<int> got{ 0 }; std::wstring text;
        editor.Start(ipc::kEditor, [&](const ipc::Message& m) { if (m.code == ipc::OpenFile) text = m.text; got++; });
        capture.Start(ipc::kCapture, nullptr);
        bool s1 = capture.SendInt(ipc::kEditor, ipc::ShowEditor, 1, true);
        bool s2 = capture.SendString(ipc::kEditor, ipc::OpenFile, L"C:\\x\\captura.snap", true);
        for (int i = 0; i < 50 && got < 2; i++) Sleep(20);
        CHECK(s1 && s2 && got == 2 && text == L"C:\\x\\captura.snap", "2 mensagens entregues (int e string)");
        editor.Stop(); capture.Stop();
        CloseHandle(fakeEditor);
    }

    printf("[8] Gravação MP4 (H.264/AAC) e orientação dos quadros\n");
    std::wstring mp4 = Tmp(L"rec.mp4");
    {
        // janela de teste com gradiente vertical conhecido, para checar a orientação
        WNDCLASSW wc{}; wc.lpfnWndProc = [](HWND h, UINT m, WPARAM w, LPARAM l) -> LRESULT {
            if (m == WM_PAINT) {
                PAINTSTRUCT ps; HDC dc = BeginPaint(h, &ps); RECT r; GetClientRect(h, &r);
                for (int y = 0; y < r.bottom; y += 4) { RECT b{ 0, y, r.right, y + 4 }; HBRUSH br = CreateSolidBrush(RGB(y * 255 / r.bottom, 40, 255 - y * 255 / r.bottom)); FillRect(dc, &b, br); DeleteObject(br); }
                EndPaint(h, &ps); return 0;
            }
            return DefWindowProcW(h, m, w, l);
        };
        wc.hInstance = GetModuleHandleW(nullptr); wc.lpszClassName = L"SnapnabTestWnd";
        RegisterClassW(&wc);
        HWND w = CreateWindowExW(WS_EX_TOPMOST, L"SnapnabTestWnd", L"t", WS_POPUP | WS_VISIBLE, 100, 100, 640, 360, nullptr, nullptr, wc.hInstance, nullptr);
        UpdateWindow(w);
        MSG msg; for (int i = 0; i < 20; i++) { while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) DispatchMessageW(&msg); Sleep(20); }
        Recorder rec;
        RecordSettings rs; rs.rect = { 100, 100, 740, 460 }; rs.fps = 15; rs.path = mp4; rs.hardware = false;
        rs.microphone = false; rs.systemAudio = true;
        bool started = rec.Start(rs, nullptr, 0);
        CHECK(started, "gravador iniciado %s", started ? "" : WToUtf8(rec.lastError).c_str());
        for (int i = 0; i < 60; i++) { while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) DispatchMessageW(&msg); Sleep(25); }
        rec.Pause(); Sleep(500); rec.Resume();
        for (int i = 0; i < 30; i++) { while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) DispatchMessageW(&msg); Sleep(25); }
        double el = rec.Elapsed();
        bool stopped = rec.Stop();
        DestroyWindow(w);
        CHECK(stopped && FileSize(mp4) > 2000, "MP4 finalizado (%llu bytes, %.2f s gravados, pausa excluída)", (unsigned long long)FileSize(mp4), el);
        ed::VideoReader rd;
        Image f; LONGLONG ts;
        bool opened = rd.Open(mp4) && rd.ReadFrame(f, ts);
        CHECK(opened && rd.width == 640 && rd.height == 360, "MP4 decodificável (%dx%d, %.2f s, %.1f qps)", rd.width, rd.height, rd.duration / 1e7, rd.fps);
        if (opened) {
            // topo deve ser azul (B alto, R baixo); base vermelha
            uint32_t top = f.at(320, 10), bot = f.at(320, 350);
            CHECK(CB(top) > CR(top) && CR(bot) > CB(bot), "orientação correta (topo B=%d R=%d, base R=%d B=%d)", CB(top), CR(top), CR(bot), CB(bot));
            CHECK(rd.duration > 1.0e7 && rd.duration < 3.5e7, "duração coerente com o tempo gravado sem a pausa (%.2f s)", rd.duration / 1e7);
        }
    }

    printf("[9] Corte de vídeo (ToFileWithCuts) e GIF a partir do vídeo\n");
    {
        std::vector<ed::Interval> cuts = { { 3000000, 8000000 } };    // remove 0,3 s – 0,8 s
        CHECK(ed::PrecedingCutsDuration(cuts, 9000000) == 5000000 && ed::IsInsideAnyCut(cuts, 5000000) && !ed::IsInsideAnyCut(cuts, 9000000),
              "PrecedingCutsDuration / IsInsideAnyCut");
        std::wstring out = Tmp(L"cut.mp4"), err;
        bool ok = ed::TranscodeWithCuts(mp4, out, cuts, nullptr, err);
        ed::VideoReader a, b;
        a.Open(mp4); b.Open(out);
        double diff = (a.duration - b.duration) / 1e7;
        CHECK(ok && std::fabs(diff - 0.5) < 0.2, "MP4 cortado %.2f s menor (esperado 0,5) %s", diff, WToUtf8(err).c_str());
        a.Close(); b.Close();
        std::wstring gif = Tmp(L"from_video.gif");
        bool gok = ed::CreateGifFromVideo(mp4, gif, 0, 0, 8, 320, 8, true, true, nullptr, err);
        Image g;
        CHECK(gok && LoadImageFile(gif, g) && g.w == 320, "GIF a partir do vídeo (%llu bytes)", (unsigned long long)FileSize(gif));
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
        bool ok = ed::RunOcr(img, text, err);
        bool match = text.find(L"Snapnab") != std::wstring::npos || text.find(L"OCR") != std::wstring::npos;
        if (!ok) printf("  info OCR indisponível: %s\n", WToUtf8(err).c_str());
        else CHECK(match, "texto reconhecido: \"%s\"", WToUtf8(Trim(text)).c_str());
    }

    printf("\n%s (%d falha(s))\n", g_fail ? "FALHOU" : "TUDO OK", g_fail);
    CoUninitialize();
    return g_fail ? 1 : 0;
}
