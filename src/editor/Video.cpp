#include "Video.h"
#include "Gif.h"
#include "Ui.h"
#include "Icons.h"
#include <mfapi.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <mferror.h>
#include <wrl/client.h>
#include <cmath>
#include <deque>

using Microsoft::WRL::ComPtr;

namespace snap::ed {

static constexpr LONGLONG kHns = 10000000;

// ---------------------------------------------------------------- MFSampleHelper
bool IsInsideAnyCut(const std::vector<Interval>& cuts, LONGLONG t) {
    for (auto& c : cuts) if (t >= c.start && t < c.end) return true;
    return false;
}
bool IsCutInBetweenFrames(const std::vector<Interval>& cuts, LONGLONG t) {
    for (auto& c : cuts) if (c.end <= t) return true;
    return false;
}
LONGLONG PrecedingCutsDuration(const std::vector<Interval>& cuts, LONGLONG t) {
    LONGLONG s = 0;
    for (auto& c : cuts) if (c.end <= t) s += c.end - c.start;
    return s;
}

static std::vector<Interval> Normalize(std::vector<Interval> cuts) {
    std::sort(cuts.begin(), cuts.end(), [](const Interval& a, const Interval& b) { return a.start < b.start; });
    std::vector<Interval> out;
    for (auto& c : cuts) {
        if (c.end <= c.start) continue;
        if (!out.empty() && c.start <= out.back().end) out.back().end = std::max(out.back().end, c.end);
        else out.push_back(c);
    }
    return out;
}

// copies the frame (2D, any pitch sign) to Image top-down. The decoder often delivers rows
// wider than the video (width aligned to 16/32/64 px), so the stride is never assumed to be w * 4.
static bool SampleToImage(IMFSample* s, int w, int h, Image& out) {
    out = Image(w, h);
    auto copy2d = [&](IMFMediaBuffer* b) {
        ComPtr<IMF2DBuffer> b2;
        BYTE* scan0; LONG pitch;
        if (FAILED(b->QueryInterface(IID_PPV_ARGS(&b2))) || FAILED(b2->Lock2D(&scan0, &pitch))) return false;
        bool ok = std::abs(pitch) >= w * 4;
        if (ok) for (int y = 0; y < h; y++) memcpy(out.row(y), scan0 + (LONGLONG)pitch * y, (size_t)w * 4);
        b2->Unlock2D();
        return ok;
    };
    DWORD n = 0;
    ComPtr<IMFMediaBuffer> buf;
    if (SUCCEEDED(s->GetBufferCount(&n)) && n == 1 && SUCCEEDED(s->GetBufferByIndex(0, &buf)) && copy2d(buf.Get())) {
        out.MakeOpaque();
        return true;
    }
    buf.Reset();
    if (FAILED(s->ConvertToContiguousBuffer(&buf))) return false;
    if (!copy2d(buf.Get())) {
        BYTE* p; DWORD len;
        if (FAILED(buf->Lock(&p, nullptr, &len))) return false;
        // linear buffer: the stride comes from the size (the height may be aligned too)
        DWORD pitch = 0;
        for (int a : { 1, 2, 8, 16, 32, 64 }) {
            DWORD hh = (DWORD)((h + a - 1) / a * a);
            if (len % hh == 0 && len / hh >= (DWORD)w * 4 && len / hh < (DWORD)w * 4 + 512) { pitch = len / hh; break; }
        }
        if (pitch) for (int y = 0; y < h; y++) memcpy(out.row(y), p + (size_t)y * pitch, (size_t)w * 4);
        buf->Unlock();
        if (!pitch) return false;
    }
    out.MakeOpaque();
    return true;
}

// ---------------------------------------------------------------- reader
bool VideoReader::Open(const std::wstring& path) {
    Close();
    if (FAILED(MFStartup(MF_VERSION, MFSTARTUP_FULL))) return false;
    mfStarted_ = true;
    ComPtr<IMFAttributes> attr;
    MFCreateAttributes(&attr, 1);
    attr->SetUINT32(MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, TRUE);
    ComPtr<IMFSourceReader> rd;
    if (FAILED(MFCreateSourceReaderFromURL(path.c_str(), attr.Get(), &rd))) return false;
    rd->SetStreamSelection((DWORD)MF_SOURCE_READER_ALL_STREAMS, FALSE);
    rd->SetStreamSelection((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, TRUE);
    ComPtr<IMFMediaType> mt;
    MFCreateMediaType(&mt);
    mt->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    mt->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_RGB32);
    if (FAILED(rd->SetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, nullptr, mt.Get()))) return false;
    ComPtr<IMFMediaType> cur;
    rd->GetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, &cur);
    UINT32 w = 0, h = 0, num = 30, den = 1;
    MFGetAttributeSize(cur.Get(), MF_MT_FRAME_SIZE, &w, &h);
    MFGetAttributeRatio(cur.Get(), MF_MT_FRAME_RATE, &num, &den);
    width = (int)w; height = (int)h;
    fps = den ? (double)num / den : 30;
    PROPVARIANT pv; PropVariantInit(&pv);
    if (SUCCEEDED(rd->GetPresentationAttribute((DWORD)MF_SOURCE_READER_MEDIASOURCE, MF_PD_DURATION, &pv))) duration = (LONGLONG)pv.uhVal.QuadPart;
    PropVariantClear(&pv);
    reader_ = rd.Detach();
    return width > 0 && height > 0;
}

void VideoReader::Close() {
    if (reader_) { ((IMFSourceReader*)reader_)->Release(); reader_ = nullptr; }
    if (mfStarted_) { MFShutdown(); mfStarted_ = false; }
}

bool VideoReader::Seek(LONGLONG t) {
    if (!reader_) return false;
    PROPVARIANT pv; PropVariantInit(&pv);
    pv.vt = VT_I8; pv.hVal.QuadPart = std::max<LONGLONG>(0, t);
    HRESULT hr = ((IMFSourceReader*)reader_)->SetCurrentPosition(GUID_NULL, pv);
    return SUCCEEDED(hr);
}

bool VideoReader::ReadFrame(Image& out, LONGLONG& ts) {
    if (!reader_) return false;
    for (int guard = 0; guard < 1000; guard++) {
        DWORD idx, flags; LONGLONG t; ComPtr<IMFSample> s;
        if (FAILED(((IMFSourceReader*)reader_)->ReadSample((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, 0, &idx, &flags, &t, &s))) return false;
        if (flags & MF_SOURCE_READERF_ENDOFSTREAM) return false;
        if (!s) continue;
        ts = t;
        return SampleToImage(s.Get(), width, height, out);
    }
    return false;
}

// ---------------------------------------------------------------- transcoding with cuts
bool TranscodeWithCuts(const std::wstring& src, const std::wstring& dst, std::vector<Interval> cuts, Progress progress, std::wstring& err) {
    cuts = Normalize(cuts);
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    MFStartup(MF_VERSION, MFSTARTUP_FULL);
    bool ok = false;
    do {
        ComPtr<IMFAttributes> ra;
        MFCreateAttributes(&ra, 1);
        ra->SetUINT32(MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, TRUE);
        // separate readers for video and audio (reading both in a single reader with ANY_STREAM fails intermittently
        // when video processing is enabled); the samples are interleaved by timestamp when writing
        ComPtr<IMFSourceReader> rd, rdA;
        if (FAILED(MFCreateSourceReaderFromURL(src.c_str(), ra.Get(), &rd))) { err = Tr(L"Não foi possível abrir o vídeo."); break; }
        rd->SetStreamSelection((DWORD)MF_SOURCE_READER_ALL_STREAMS, FALSE);
        rd->SetStreamSelection((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, TRUE);
        // video → RGB32
        ComPtr<IMFMediaType> vt; MFCreateMediaType(&vt);
        vt->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video); vt->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_RGB32);
        if (FAILED(rd->SetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, nullptr, vt.Get()))) { err = Tr(L"Formato de vídeo não suportado."); break; }
        ComPtr<IMFMediaType> vcur; rd->GetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, &vcur);
        UINT32 w, h, fn = 30, fd = 1; MFGetAttributeSize(vcur.Get(), MF_MT_FRAME_SIZE, &w, &h);
        MFGetAttributeRatio(vcur.Get(), MF_MT_FRAME_RATE, &fn, &fd);
        // audio → 16-bit PCM (if present)
        bool hasAudio = false;
        UINT32 rate = 48000, ch = 2;
        if (SUCCEEDED(MFCreateSourceReaderFromURL(src.c_str(), nullptr, &rdA))) {
            rdA->SetStreamSelection((DWORD)MF_SOURCE_READER_ALL_STREAMS, FALSE);
            rdA->SetStreamSelection((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, TRUE);
            ComPtr<IMFMediaType> at; MFCreateMediaType(&at);
            at->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Audio); at->SetGUID(MF_MT_SUBTYPE, MFAudioFormat_PCM);
            at->SetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
            if (SUCCEEDED(rdA->SetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, nullptr, at.Get()))) {
                ComPtr<IMFMediaType> acur; rdA->GetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, &acur);
                rate = MFGetAttributeUINT32(acur.Get(), MF_MT_AUDIO_SAMPLES_PER_SECOND, 48000);
                ch = MFGetAttributeUINT32(acur.Get(), MF_MT_AUDIO_NUM_CHANNELS, 2);
                hasAudio = (rate == 44100 || rate == 48000) && (ch == 1 || ch == 2);
            }
        }
        PROPVARIANT pv; PropVariantInit(&pv);
        LONGLONG duration = 0;
        if (SUCCEEDED(rd->GetPresentationAttribute((DWORD)MF_SOURCE_READER_MEDIASOURCE, MF_PD_DURATION, &pv))) duration = (LONGLONG)pv.uhVal.QuadPart;
        PropVariantClear(&pv);

        DeleteFileW(dst.c_str());
        ComPtr<IMFAttributes> wa; MFCreateAttributes(&wa, 1);
        wa->SetUINT32(MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, FALSE);
        ComPtr<IMFSinkWriter> wr;
        if (FAILED(MFCreateSinkWriterFromURL(dst.c_str(), nullptr, wa.Get(), &wr))) { err = Tr(L"Não foi possível criar o arquivo de saída."); break; }
        DWORD vs = 0, as = 0;
        UINT32 ow = w & ~1u, oh = h & ~1u;
        ComPtr<IMFMediaType> vo; MFCreateMediaType(&vo);
        vo->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video); vo->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_H264);
        vo->SetUINT32(MF_MT_AVG_BITRATE, (UINT32)std::clamp(0.12 * ow * oh * (fd ? (double)fn / fd : 30), 500000.0, 40000000.0));
        vo->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
        MFSetAttributeSize(vo.Get(), MF_MT_FRAME_SIZE, ow, oh);
        MFSetAttributeRatio(vo.Get(), MF_MT_FRAME_RATE, fn, fd);
        MFSetAttributeRatio(vo.Get(), MF_MT_PIXEL_ASPECT_RATIO, 1, 1);
        if (FAILED(wr->AddStream(vo.Get(), &vs))) { err = Tr(L"Codificador H.264 indisponível."); break; }
        ComPtr<IMFMediaType> vi; MFCreateMediaType(&vi);
        vi->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video); vi->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_RGB32);
        vi->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
        MFSetAttributeSize(vi.Get(), MF_MT_FRAME_SIZE, ow, oh);
        MFSetAttributeRatio(vi.Get(), MF_MT_FRAME_RATE, fn, fd);
        MFSetAttributeRatio(vi.Get(), MF_MT_PIXEL_ASPECT_RATIO, 1, 1);
        if (FAILED(wr->SetInputMediaType(vs, vi.Get(), nullptr))) { err = Tr(L"Entrada de vídeo recusada."); break; }
        if (hasAudio) {
            ComPtr<IMFMediaType> ao; MFCreateMediaType(&ao);
            ao->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Audio); ao->SetGUID(MF_MT_SUBTYPE, MFAudioFormat_AAC);
            ao->SetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE, 16); ao->SetUINT32(MF_MT_AUDIO_SAMPLES_PER_SECOND, rate);
            ao->SetUINT32(MF_MT_AUDIO_NUM_CHANNELS, ch); ao->SetUINT32(MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24000);
            ComPtr<IMFMediaType> ai; MFCreateMediaType(&ai);
            ai->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Audio); ai->SetGUID(MF_MT_SUBTYPE, MFAudioFormat_PCM);
            ai->SetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE, 16); ai->SetUINT32(MF_MT_AUDIO_SAMPLES_PER_SECOND, rate);
            ai->SetUINT32(MF_MT_AUDIO_NUM_CHANNELS, ch); ai->SetUINT32(MF_MT_AUDIO_BLOCK_ALIGNMENT, ch * 2);
            ai->SetUINT32(MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * ch * 2);
            if (FAILED(wr->AddStream(ao.Get(), &as)) || FAILED(wr->SetInputMediaType(as, ai.Get(), nullptr))) { err = Tr(L"Codificador AAC indisponível."); break; }
        }
        if (FAILED(wr->BeginWriting())) { err = Tr(L"Falha ao iniciar a gravação."); break; }

        bool videoDone = false, audioDone = !hasAudio, canceled = false;
        std::vector<BYTE> flip((size_t)ow * oh * 4);
        int lastPct = -1;
        // next pending sample of each reader
        struct Pending { ComPtr<IMFSample> s; LONGLONG ts = 0; bool have = false; };
        Pending pv2, pa;
        auto fetch = [&](IMFSourceReader* r, DWORD stream, Pending& p, bool& done) -> bool {
            while (!done && !p.have) {
                DWORD idx, flags; LONGLONG ts; ComPtr<IMFSample> s;
                HRESULT rhr = r->ReadSample(stream, 0, &idx, &flags, &ts, &s);
                if (FAILED(rhr)) { err = Format(Tr(L"Falha ao ler o vídeo (0x%08X)."), rhr); return false; }
                if (flags & MF_SOURCE_READERF_ENDOFSTREAM) { done = true; break; }
                if (s) { p.s = s; p.ts = ts; p.have = true; }
            }
            return true;
        };
        while (!(videoDone && audioDone)) {
            if (!fetch(rd.Get(), (DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, pv2, videoDone)) break;
            if (hasAudio && !fetch(rdA.Get(), (DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, pa, audioDone)) {
                // unreadable audio: continue with video only
                Log(L"Cut: unreadable audio (%s), continuing without audio", err.c_str());
                err.clear(); audioDone = true; pa.have = false;
            }
            if (!pv2.have && !pa.have) continue;
            bool isVideo = pv2.have && (!pa.have || pv2.ts <= pa.ts);
            Pending& cur = isVideo ? pv2 : pa;
            ComPtr<IMFSample> s = cur.s;
            LONGLONG ts = cur.ts;
            cur.have = false; cur.s.Reset();
            // cut range: discard; otherwise rewrite the PTS minus the previous cuts
            if (IsInsideAnyCut(cuts, ts)) continue;
            LONGLONG removed = PrecedingCutsDuration(cuts, ts);
            LONGLONG dur = 0; s->GetSampleDuration(&dur);
            if (isVideo) {
                Image img;
                if (!SampleToImage(s.Get(), (int)w, (int)h, img)) continue;
                // sink writer input RGB32: bottom-up
                for (UINT32 y = 0; y < oh; y++) memcpy(flip.data() + (size_t)(oh - 1 - y) * ow * 4, img.row((int)y), (size_t)ow * 4);
                ComPtr<IMFMediaBuffer> b; MFCreateMemoryBuffer((DWORD)flip.size(), &b);
                BYTE* p; b->Lock(&p, nullptr, nullptr); memcpy(p, flip.data(), flip.size()); b->Unlock();
                b->SetCurrentLength((DWORD)flip.size());
                ComPtr<IMFSample> o; MFCreateSample(&o); o->AddBuffer(b.Get());
                o->SetSampleTime(ts - removed); o->SetSampleDuration(dur);
                if (FAILED(wr->WriteSample(vs, o.Get()))) { err = Tr(L"Falha ao codificar quadro."); break; }
                if (duration > 0 && progress) {
                    int pct = (int)(ts * 100 / duration);
                    if (pct != lastPct) { lastPct = pct; if (!progress(pct)) { canceled = true; break; } }
                }
            } else {
                s->SetSampleTime(ts - removed);
                if (FAILED(wr->WriteSample(as, s.Get()))) { err = Tr(L"Falha ao codificar áudio."); break; }
            }
        }
        if (!err.empty() || canceled) { wr.Reset(); DeleteFileW(dst.c_str()); if (canceled) err = Tr(L"Cancelado."); break; }
        if (FAILED(wr->Finalize())) { err = Tr(L"Falha ao finalizar o MP4."); break; }
        ok = true;
    } while (false);
    MFShutdown();
    CoUninitialize();
    return ok;
}

bool CreateGifFromVideo(const std::wstring& src, const std::wstring& dst, LONGLONG from, LONGLONG to, int fps, int width,
                        int bpp, bool dither, bool delta, Progress progress, std::wstring& err) {
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    bool ok = false;
    {
        VideoReader rd;
        if (!rd.Open(src)) { err = Tr(L"Não foi possível abrir o vídeo."); }
        else {
            if (to <= from) to = rd.duration;
            int ow = std::clamp(width, 16, rd.width);
            int oh = std::max(16, (int)std::lround((double)rd.height * ow / rd.width));
            GifWriter gw;
            if (!gw.Open(dst, ow, oh, fps, bpp, dither, delta)) err = Tr(L"Não foi possível criar o GIF.");
            else {
                rd.Seek(from);
                LONGLONG step = kHns / std::max(1, fps), next = from;
                Image img, pending; LONGLONG ts;
                int frames = 0;
                bool canceled = false;
                while (rd.ReadFrame(img, ts)) {
                    if (ts < next - step / 2) continue;
                    if (ts > to) break;
                    // repeated frames increase the delay instead of duplicating the frame
                    int reps = 1;
                    next += step;
                    while (ts >= next + step / 2) { next += step; reps++; }
                    gw.AddFrame(ScaleImage(img, ow, oh), std::max(2, (int)std::lround(100.0 * reps / fps)));
                    frames++;
                    if (progress && to > from && !progress((int)((ts - from) * 100 / (to - from)))) { canceled = true; break; }
                }
                ok = gw.Close() && frames > 0 && !canceled;
                if (canceled) { err = Tr(L"Cancelado."); DeleteFileW(dst.c_str()); }
                else if (!frames) err = Tr(L"Nenhum quadro no intervalo.");
            }
        }
    }
    CoUninitialize();
    return ok;
}

// ---------------------------------------------------------------- progress window
bool RunWithProgress(HWND owner, const std::wstring& title, std::function<bool(Progress)> work) {
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = DefWindowProcW; wc.hInstance = GetModuleHandleW(nullptr);
        wc.hbrBackground = GetSysColorBrush(COLOR_WINDOW); wc.lpszClassName = L"SnapnabProgress"; wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
        RegisterClassW(&wc); reg = true;
    }
    int dpi = DpiForWindow(owner);
    RECT orc; GetWindowRect(owner, &orc);
    int W = ui::Scale(380, dpi), H = ui::Scale(140, dpi);
    HWND h = CreateWindowExW(WS_EX_DLGMODALFRAME | WS_EX_TOPMOST, L"SnapnabProgress", title.c_str(), WS_POPUP | WS_CAPTION,
                             (orc.left + orc.right - W) / 2, (orc.top + orc.bottom - H) / 2, W, H, owner, nullptr, GetModuleHandleW(nullptr), nullptr);
    HWND bar = CreateWindowExW(0, PROGRESS_CLASSW, nullptr, WS_CHILD | WS_VISIBLE, ui::Scale(16, dpi), ui::Scale(20, dpi), W - ui::Scale(48, dpi),
                               ui::Scale(20, dpi), h, nullptr, GetModuleHandleW(nullptr), nullptr);
    HWND cancel = CreateWindowExW(0, L"BUTTON", Tr(L"Cancelar"), WS_CHILD | WS_VISIBLE, W - ui::Scale(130, dpi), ui::Scale(56, dpi),
                                  ui::Scale(100, dpi), ui::Scale(28, dpi), h, (HMENU)2, GetModuleHandleW(nullptr), nullptr);
    SendMessageW(cancel, WM_SETFONT, (WPARAM)ui::UiFont(dpi), TRUE);
    SendMessageW(bar, PBM_SETRANGE32, 0, 100);
    EnableWindow(owner, FALSE);
    ShowWindow(h, SW_SHOW);
    std::atomic<int> pct{ 0 };
    std::atomic<bool> cancelFlag{ false }, done{ false };
    bool result = false;
    std::thread t([&] {
        result = work([&](int p) { pct = p; return !cancelFlag.load(); });
        done = true;
    });
    MSG msg;
    while (!done) {
        while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) {
            if ((msg.hwnd == cancel && msg.message == WM_LBUTTONUP) || (msg.message == WM_KEYDOWN && msg.wParam == VK_ESCAPE)) cancelFlag = true;
            TranslateMessage(&msg); DispatchMessageW(&msg);
        }
        SendMessageW(bar, PBM_SETPOS, pct.load(), 0);
        MsgWaitForMultipleObjects(0, nullptr, FALSE, 50, QS_ALLINPUT);
    }
    t.join();
    EnableWindow(owner, TRUE);
    DestroyWindow(h);
    SetForegroundWindow(owner);
    return result;
}

// ---------------------------------------------------------------- preview audio (waveOut)
// Reads the audio track as 16-bit PCM on its own thread and plays it through waveOut, skipping the cut ranges.
struct VideoView::AudioPlayer {
    std::thread th;
    std::atomic<bool> run{ false };

    void Start(const std::wstring& path, LONGLONG from, std::vector<Interval> cuts) {
        run = true;
        th = std::thread([this, path, from, cuts = std::move(cuts)] {
            CoInitializeEx(nullptr, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_LITE);
            Play(path, from, cuts);
            MFShutdown();
            CoUninitialize();
        });
    }
    void Stop() { run = false; if (th.joinable()) th.join(); }
    ~AudioPlayer() { Stop(); }

    void Play(const std::wstring& path, LONGLONG from, const std::vector<Interval>& cuts) {
        ComPtr<IMFSourceReader> rd;
        if (FAILED(MFCreateSourceReaderFromURL(path.c_str(), nullptr, &rd))) return;
        rd->SetStreamSelection((DWORD)MF_SOURCE_READER_ALL_STREAMS, FALSE);
        if (FAILED(rd->SetStreamSelection((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, TRUE))) return;   // video without audio
        ComPtr<IMFMediaType> mt;
        MFCreateMediaType(&mt);
        mt->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Audio); mt->SetGUID(MF_MT_SUBTYPE, MFAudioFormat_PCM);
        mt->SetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
        if (FAILED(rd->SetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, nullptr, mt.Get()))) return;
        ComPtr<IMFMediaType> cur;
        rd->GetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, &cur);
        WAVEFORMATEX wf{};
        wf.wFormatTag = WAVE_FORMAT_PCM;
        wf.nChannels = (WORD)MFGetAttributeUINT32(cur.Get(), MF_MT_AUDIO_NUM_CHANNELS, 2);
        wf.nSamplesPerSec = MFGetAttributeUINT32(cur.Get(), MF_MT_AUDIO_SAMPLES_PER_SECOND, 48000);
        wf.wBitsPerSample = 16;
        wf.nBlockAlign = wf.nChannels * 2;
        wf.nAvgBytesPerSec = wf.nSamplesPerSec * wf.nBlockAlign;
        PROPVARIANT pv; PropVariantInit(&pv);
        pv.vt = VT_I8; pv.hVal.QuadPart = from;
        rd->SetCurrentPosition(GUID_NULL, pv);
        HANDLE ev = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        HWAVEOUT wo = nullptr;
        if (waveOutOpen(&wo, WAVE_MAPPER, &wf, (DWORD_PTR)ev, 0, CALLBACK_EVENT) != MMSYSERR_NOERROR) { CloseHandle(ev); return; }
        std::deque<WAVEHDR*> queue;
        auto recycle = [&](bool all) {
            while (!queue.empty() && (all || (queue.front()->dwFlags & WHDR_DONE))) {
                WAVEHDR* h = queue.front(); queue.pop_front();
                waveOutUnprepareHeader(wo, h, sizeof *h);
                delete[] h->lpData; delete h;
            }
        };
        bool eos = false;
        while (run && !(eos && queue.empty())) {
            recycle(false);
            if (eos || queue.size() >= 10) { WaitForSingleObject(ev, 30); continue; }
            DWORD idx, flags; LONGLONG ts; ComPtr<IMFSample> s;
            if (FAILED(rd->ReadSample((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, 0, &idx, &flags, &ts, &s)) ||
                (flags & MF_SOURCE_READERF_ENDOFSTREAM)) { eos = true; continue; }
            if (!s || ts < from - kHns / 20 || IsInsideAnyCut(cuts, ts)) continue;
            ComPtr<IMFMediaBuffer> b;
            BYTE* p; DWORD len;
            if (FAILED(s->ConvertToContiguousBuffer(&b)) || FAILED(b->Lock(&p, nullptr, &len))) continue;
            auto* h = new WAVEHDR{};
            h->lpData = new char[len];
            memcpy(h->lpData, p, len);
            h->dwBufferLength = len;
            b->Unlock();
            waveOutPrepareHeader(wo, h, sizeof *h);
            waveOutWrite(wo, h, sizeof *h);
            queue.push_back(h);
        }
        waveOutReset(wo);
        recycle(true);
        waveOutClose(wo);
        CloseHandle(ev);
    }
};

// ---------------------------------------------------------------- video view
enum { B_PLAY = 1, B_PREV, B_NEXT, B_MARKIN, B_MARKOUT, B_CUT, B_UNDOCUT, B_SAVEMP4, B_SAVEFRAME, B_GIF, B_PLAYER };

static std::wstring FmtTime(LONGLONG t) {
    double s = t / (double)kHns;
    int m = (int)(s / 60);
    return Format(L"%02d:%05.2f", m, s - m * 60);
}

void VideoView::Create(HWND parent) {
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = Proc; wc.hInstance = GetModuleHandleW(nullptr); wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
        wc.lpszClassName = L"SnapnabVideoView";
        RegisterClassW(&wc); reg = true;
    }
    hwnd = CreateWindowExW(0, L"SnapnabVideoView", nullptr, WS_CHILD | WS_CLIPCHILDREN, 0, 0, 10, 10, parent, nullptr,
                           GetModuleHandleW(nullptr), this);
    struct B { int id; const wchar_t* t; } bs[] = {
        { B_PLAY, Tr(L"▶ Reproduzir") }, { B_PREV, Tr(L"◀ Quadro") }, { B_NEXT, Tr(L"Quadro ▶") }, { B_MARKIN, Tr(L"[ Início") }, { B_MARKOUT, Tr(L"Fim ]") },
        { B_CUT, Tr(L"✂ Cortar seleção") }, { B_UNDOCUT, Tr(L"Desfazer corte") }, { B_SAVEFRAME, Tr(L"Salvar quadro") }, { B_GIF, Tr(L"Criar GIF…") },
        { B_SAVEMP4, Tr(L"Salvar MP4…") }, { B_PLAYER, Tr(L"Abrir no player") } };
    int dpi = DpiForWindow(parent);
    for (auto& b : bs) {
        HWND h = CreateWindowExW(0, L"BUTTON", b.t, WS_CHILD | WS_VISIBLE | WS_TABSTOP, 0, 0, 10, 10, hwnd, (HMENU)(INT_PTR)b.id,
                                 GetModuleHandleW(nullptr), nullptr);
        SendMessageW(h, WM_SETFONT, (WPARAM)ui::UiFont(dpi), TRUE);
        btn_[b.id] = h;
    }
}

bool VideoView::Open(const std::wstring& p) {
    Close();
    path = p;
    if (!reader_.Open(p)) { open_ = false; InvalidateRect(hwnd, nullptr, FALSE); return false; }
    open_ = true;
    pos_ = 0; selA_ = selB_ = -1; cuts_.clear(); cutUndo_.clear();
    ShowAt(0);
    return true;
}

void VideoView::Close() {
    StopPlay();
    reader_.Close();
    open_ = false;
    frame_ = Image();
}

RECT VideoView::TimelineRect() const {
    RECT cr; GetClientRect(hwnd, &cr);
    int dpi = DpiForWindow(hwnd);
    return { ui::Scale(16, dpi), cr.bottom - ui::Scale(112, dpi), cr.right - ui::Scale(16, dpi), cr.bottom - ui::Scale(76, dpi) };
}

LONGLONG VideoView::TimeFromX(int x) const {
    RECT t = TimelineRect();
    double f = std::clamp((double)(x - t.left) / std::max(1L, t.right - t.left), 0.0, 1.0);
    return (LONGLONG)(f * reader_.duration);
}

void VideoView::Layout() {
    RECT cr; GetClientRect(hwnd, &cr);
    int dpi = DpiForWindow(hwnd);
    int x = ui::Scale(16, dpi), y = cr.bottom - ui::Scale(46, dpi), h = ui::Scale(30, dpi);
    int widths[] = { 110, 80, 80, 74, 64, 130, 110, 100, 90, 100, 110 };
    int ids[] = { B_PLAY, B_PREV, B_NEXT, B_MARKIN, B_MARKOUT, B_CUT, B_UNDOCUT, B_SAVEFRAME, B_GIF, B_SAVEMP4, B_PLAYER };
    for (int i = 0; i < 11; i++) {
        int w = ui::Scale(widths[i], dpi);
        MoveWindow(btn_[ids[i]], x, y, w, h, TRUE);
        x += w + ui::Scale(6, dpi);
    }
}

void VideoView::ShowAt(LONGLONG t) {
    if (!open_) return;
    t = std::clamp<LONGLONG>(t, 0, std::max<LONGLONG>(0, reader_.duration - 1));
    reader_.Seek(t);
    Image img; LONGLONG ts = 0;
    // decodes from the key frame until it reaches t
    for (int i = 0; i < 600; i++) {
        if (!reader_.ReadFrame(img, ts)) break;
        frame_ = img;
        if (ts + (LONGLONG)(kHns / reader_.fps) > t) break;
    }
    pos_ = std::max(t, ts);
    InvalidateRect(hwnd, nullptr, FALSE);
}

void VideoView::Step(int dir) {
    StopPlay();
    ShowAt(pos_ + dir * (LONGLONG)(kHns / reader_.fps));
}

void VideoView::TogglePlay() {
    if (playing_) { StopPlay(); return; }
    if (!open_) return;
    if (pos_ >= reader_.duration - kHns / 10) ShowAt(0);
    playing_ = true;
    reader_.Seek(pos_);
    pending_ = Image(); pendingTs_ = -1;
    playFrom_ = pos_;
    playTick_ = GetTickCount64();
    audio_ = { new AudioPlayer, [](AudioPlayer* a) { delete a; } };
    audio_->Start(path, pos_, Normalize(cuts_));
    SetWindowTextW(btn_[B_PLAY], Tr(L"❚❚ Pausar"));
    SetTimer(hwnd, 1, std::max(10, (int)(500 / reader_.fps)), nullptr);
}

void VideoView::StopPlay() {
    if (!playing_) return;
    playing_ = false;
    KillTimer(hwnd, 1);
    audio_.reset();
    pending_ = Image(); pendingTs_ = -1;
    SetWindowTextW(btn_[B_PLAY], Tr(L"▶ Reproduzir"));
}

// wall clock converted to source video time, skipping the cut ranges (like the audio)
LONGLONG VideoView::PlayClock() const {
    LONGLONG t = playFrom_ + (LONGLONG)(GetTickCount64() - playTick_) * 10000;
    for (auto& c : Normalize(cuts_))
        if (c.end > playFrom_ && c.start <= t) t += c.end - std::max(c.start, playFrom_);
    return t;
}

void VideoView::Paint(HDC hdc) {
    RECT cr; GetClientRect(hwnd, &cr);
    HDC mem = CreateCompatibleDC(hdc);
    HBITMAP bb = CreateCompatibleBitmap(hdc, std::max(1L, cr.right), std::max(1L, cr.bottom));
    HGDIOBJ ob = SelectObject(mem, bb);
    HBRUSH bg = CreateSolidBrush(RGB(32, 33, 36)); FillRect(mem, &cr, bg); DeleteObject(bg);
    int dpi = DpiForWindow(hwnd);
    RECT tl = TimelineRect();
    RECT area{ ui::Scale(16, dpi), ui::Scale(12, dpi), cr.right - ui::Scale(16, dpi), tl.top - ui::Scale(26, dpi) };
    if (!open_) {
        ui::DrawTextCentered(mem, Tr(L"Não foi possível abrir o vídeo (somente MP4)."), cr, RGB(220, 220, 220), ui::UiFont(dpi, 11));
    } else {
        if (!frame_.empty() && area.right > area.left && area.bottom > area.top) {
            double s = std::min((double)(area.right - area.left) / frame_.w, (double)(area.bottom - area.top) / frame_.h);
            int w = (int)(frame_.w * s), h = (int)(frame_.h * s);
            frame_.DrawTo(mem, (area.left + area.right - w) / 2, (area.top + area.bottom - h) / 2, w, h, true);
            if (IsInsideAnyCut(cuts_, pos_)) {
                RECT tag{ area.left + 8, area.top + 8, area.left + ui::Scale(160, dpi), area.top + ui::Scale(34, dpi) };
                ui::FillRoundRect(mem, tag, 8, RGB(200, 40, 40), RGB(200, 40, 40));
                ui::DrawTextCentered(mem, Tr(L"trecho cortado"), tag, RGB(255, 255, 255), ui::UiFont(dpi, 9, true));
            }
        }
        // timeline
        HBRUSH tb = CreateSolidBrush(RGB(70, 72, 78)); FillRect(mem, &tl, tb); DeleteObject(tb);
        auto xOf = [&](LONGLONG t) { return tl.left + (int)((double)t / std::max<LONGLONG>(1, reader_.duration) * (tl.right - tl.left)); };
        for (auto& c : Normalize(cuts_)) {
            RECT r{ xOf(c.start), tl.top, xOf(c.end), tl.bottom };
            HBRUSH cb = CreateHatchBrush(HS_BDIAGONAL, RGB(230, 70, 70));
            SetBkColor(mem, RGB(120, 30, 30));
            FillRect(mem, &r, cb); DeleteObject(cb);
        }
        if (selA_ >= 0 || selB_ >= 0) {
            LONGLONG a = selA_ >= 0 ? selA_ : 0, b = selB_ >= 0 ? selB_ : reader_.duration;
            if (b < a) std::swap(a, b);
            RECT r{ xOf(a), tl.top - 4, xOf(b), tl.top };
            HBRUSH sb = CreateSolidBrush(RGB(60, 150, 255)); FillRect(mem, &r, sb); DeleteObject(sb);
        }
        int px = xOf(pos_);
        RECT ph{ px - 1, tl.top - 6, px + 2, tl.bottom + 6 };
        HBRUSH pb = CreateSolidBrush(RGB(255, 255, 255)); FillRect(mem, &ph, pb); DeleteObject(pb);
        RECT tt{ tl.left, tl.bottom + 2, tl.right, tl.bottom + ui::Scale(20, dpi) };
        LONGLONG kept = reader_.duration;
        for (auto& c : Normalize(cuts_)) kept -= c.end - c.start;
        std::wstring info = FmtTime(pos_) + L" / " + FmtTime(reader_.duration) + Format(Tr(L"   •   %d×%d @ %.0f qps"), reader_.width, reader_.height, reader_.fps);
        if (!cuts_.empty()) info += Tr(L"   •   duração após cortes: ") + FmtTime(kept);
        if (selA_ >= 0 || selB_ >= 0) info += Tr(L"   •   seleção: ") + FmtTime(std::max<LONGLONG>(0, selA_)) + L" – " + FmtTime(selB_ >= 0 ? selB_ : reader_.duration);
        HGDIOBJ of = SelectObject(mem, ui::UiFont(dpi, 9));
        SetBkMode(mem, TRANSPARENT); SetTextColor(mem, RGB(220, 220, 220));
        DrawTextW(mem, info.c_str(), -1, &tt, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
        SelectObject(mem, of);
    }
    BitBlt(hdc, 0, 0, cr.right, cr.bottom, mem, 0, 0, SRCCOPY);
    SelectObject(mem, ob); DeleteObject(bb); DeleteDC(mem);
}

void VideoView::SaveVideo() {
    StopPlay();
    std::wstring f = Tr(L"Vídeo MP4 (*.mp4)|*.mp4|"); std::replace(f.begin(), f.end(), L'|', L'\0');
    std::wstring out = StripExt(path) + L"_editado.mp4"; int fi = 1;
    if (!ui::SaveFileDialog(GetAncestor(hwnd, GA_ROOT), f, out, fi, L"mp4", Tr(L"Salvar vídeo"))) return;
    if (ToLower(out) == ToLower(path)) { MessageBoxW(hwnd, Tr(L"Escolha um arquivo diferente do original."), L"Snapnab", MB_ICONWARNING); return; }
    std::wstring src = path, err;
    auto cuts = cuts_;
    bool ok = RunWithProgress(GetAncestor(hwnd, GA_ROOT), Tr(L"Salvando vídeo…"), [&](Progress p) { return TranscodeWithCuts(src, out, cuts, p, err); });
    if (!ok) { MessageBoxW(hwnd, (Tr(L"Não foi possível salvar o vídeo.\n") + err).c_str(), L"Snapnab", MB_ICONWARNING); return; }
    if (onNewVideo) onNewVideo(out);
}

void VideoView::SaveGif() {
    StopPlay();
    std::vector<ui::Field> f = {
        ui::Field::MakeLabel(selA_ >= 0 || selB_ >= 0 ? Tr(L"Intervalo: a seleção marcada.") : Tr(L"Intervalo: o vídeo inteiro (marque início/fim para limitar).")),
        ui::Field::MakeInt(Tr(L"Quadros por segundo"), 10, 1, 30),
        ui::Field::MakeInt(Tr(L"Largura (px)"), std::min(640, reader_.width), 32, reader_.width),
        ui::Field::MakeCombo(Tr(L"Cores"), { Tr(L"256 cores"), Tr(L"16 cores (arquivo menor)") }, 0),
        ui::Field::MakeCheck(Tr(L"Pontilhado (dithering)"), true),
        ui::Field::MakeCheck(Tr(L"Quadros delta (só o que mudou)"), true),
    };
    if (!ui::RunForm(GetAncestor(hwnd, GA_ROOT), Tr(L"Criar GIF animado"), f)) return;
    std::wstring flt = Tr(L"GIF animado (*.gif)|*.gif|"); std::replace(flt.begin(), flt.end(), L'|', L'\0');
    std::wstring out = StripExt(path) + L".gif"; int fi = 1;
    if (!ui::SaveFileDialog(GetAncestor(hwnd, GA_ROOT), flt, out, fi, L"gif", Tr(L"Salvar GIF"))) return;
    LONGLONG a = selA_ >= 0 ? selA_ : 0, b = selB_ >= 0 ? selB_ : reader_.duration;
    if (b < a) std::swap(a, b);
    std::wstring src = path, err;
    int fps = f[1].value, w = f[2].value, bpp = f[3].value == 1 ? 4 : 8;
    bool dither = f[4].value, delta = f[5].value;
    bool ok = RunWithProgress(GetAncestor(hwnd, GA_ROOT), Tr(L"Criando GIF…"), [&](Progress p) {
        return CreateGifFromVideo(src, out, a, b, fps, w, bpp, dither, delta, p, err);
    });
    if (!ok) MessageBoxW(hwnd, (Tr(L"Não foi possível criar o GIF.\n") + err).c_str(), L"Snapnab", MB_ICONWARNING);
    else ShellExecuteW(nullptr, L"open", L"explorer.exe", (L"/select,\"" + out + L"\"").c_str(), nullptr, SW_SHOWNORMAL);
}

LRESULT CALLBACK VideoView::Proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    VideoView* self = (VideoView*)GetWindowLongPtrW(h, GWLP_USERDATA);
    if (m == WM_NCCREATE) {
        self = (VideoView*)((CREATESTRUCTW*)l)->lpCreateParams;
        SetWindowLongPtrW(h, GWLP_USERDATA, (LONG_PTR)self);
        self->hwnd = h;
    }
    return self ? self->Handle(m, w, l) : DefWindowProcW(h, m, w, l);
}

LRESULT VideoView::Handle(UINT m, WPARAM w, LPARAM l) {
    switch (m) {
    case WM_SIZE: Layout(); InvalidateRect(hwnd, nullptr, FALSE); return 0;
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: { PAINTSTRUCT ps; HDC dc = BeginPaint(hwnd, &ps); Paint(dc); EndPaint(hwnd, &ps); return 0; }
    case WM_TIMER: {
        if (!playing_) return 0;
        // shows the most recent frame the clock has reached (the audio follows the same clock)
        LONGLONG clock = PlayClock();
        bool shown = false;
        for (int i = 0; i < 16; i++) {
            if (pendingTs_ < 0) {
                Image img; LONGLONG ts;
                if (!reader_.ReadFrame(img, ts)) { if (!shown) StopPlay(); break; }
                bool skipped = false;
                for (auto& c : Normalize(cuts_))      // skips the cut ranges in the preview
                    if (ts >= c.start && ts < c.end) { reader_.Seek(c.end); skipped = true; break; }
                if (skipped) continue;
                pending_ = std::move(img); pendingTs_ = ts;
            }
            if (pendingTs_ > clock) break;
            frame_ = std::move(pending_); pos_ = pendingTs_; pendingTs_ = -1;
            shown = true;
        }
        if (shown) InvalidateRect(hwnd, nullptr, FALSE);
        return 0;
    }
    case WM_LBUTTONDOWN: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        RECT t = TimelineRect(); InflateRect(&t, 0, 10);
        if (open_ && PtInRect(&t, p)) { StopPlay(); scrubbing_ = true; SetCapture(hwnd); ShowAt(TimeFromX(p.x)); }
        SetFocus(hwnd);
        return 0;
    }
    case WM_MOUSEMOVE:
        if (scrubbing_) {
            static ULONGLONG last = 0;
            if (GetTickCount64() - last > 60) { last = GetTickCount64(); ShowAt(TimeFromX(GET_X_LPARAM(l))); }
        }
        return 0;
    case WM_LBUTTONUP:
        if (scrubbing_) { scrubbing_ = false; ReleaseCapture(); ShowAt(TimeFromX(GET_X_LPARAM(l))); }
        return 0;
    case WM_KEYDOWN:
        if (w == VK_SPACE) TogglePlay();
        else if (w == VK_LEFT) Step(-1);
        else if (w == VK_RIGHT) Step(1);
        else if (w == 'I') { selA_ = pos_; InvalidateRect(hwnd, nullptr, FALSE); }
        else if (w == 'O') { selB_ = pos_; InvalidateRect(hwnd, nullptr, FALSE); }
        return 0;
    case WM_COMMAND:
        switch (LOWORD(w)) {
        case B_PLAY: TogglePlay(); break;
        case B_PREV: Step(-1); break;
        case B_NEXT: Step(1); break;
        case B_MARKIN: selA_ = pos_; if (selB_ >= 0 && selB_ < selA_) selB_ = -1; InvalidateRect(hwnd, nullptr, FALSE); break;
        case B_MARKOUT: selB_ = pos_; if (selA_ > selB_) selA_ = -1; InvalidateRect(hwnd, nullptr, FALSE); break;
        case B_CUT: {      // CVidOpCreateCut
            LONGLONG a = selA_ >= 0 ? selA_ : 0, b = selB_ >= 0 ? selB_ : -1;
            if (b <= a) { MessageBoxW(hwnd, Tr(L"Marque o início e o fim do trecho a remover."), L"Snapnab", MB_ICONINFORMATION); break; }
            cutUndo_.push_back(cuts_);
            cuts_.push_back({ a, b });
            cuts_ = Normalize(cuts_);
            selA_ = selB_ = -1;
            InvalidateRect(hwnd, nullptr, FALSE);
            break;
        }
        case B_UNDOCUT: if (!cutUndo_.empty()) { cuts_ = cutUndo_.back(); cutUndo_.pop_back(); InvalidateRect(hwnd, nullptr, FALSE); } break;
        case B_SAVEFRAME: if (!frame_.empty() && onSaveFrame) onSaveFrame(frame_); break;
        case B_GIF: SaveGif(); break;
        case B_SAVEMP4: SaveVideo(); break;
        case B_PLAYER: StopPlay(); ShellExecuteW(nullptr, L"open", path.c_str(), nullptr, nullptr, SW_SHOWNORMAL); break;
        }
        SetFocus(hwnd);
        return 0;
    case WM_GETDLGCODE: return DLGC_WANTARROWS | DLGC_WANTCHARS;
    }
    return DefWindowProcW(hwnd, m, w, l);
}

} // namespace snap::ed
