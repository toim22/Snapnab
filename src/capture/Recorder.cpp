#include "Recorder.h"
#include <mfapi.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <mferror.h>
#include <mmdeviceapi.h>
#include <audioclient.h>
#include <wrl/client.h>
#include <deque>
#include <future>
#include <cmath>

using Microsoft::WRL::ComPtr;

namespace snap {

static constexpr int kRate = 48000;     // taxa do mixer/AAC
static constexpr LONGLONG kHns = 10000000;

// ---------------------------------------------------------------- fonte de áudio WASAPI
struct Recorder::AudioSource {
    bool loopback = false;
    std::atomic<bool> run{ false };
    std::atomic<float>* level = nullptr;
    std::thread th;
    std::mutex m;
    std::deque<float> buf;               // estéreo intercalado a 48 kHz
    std::wstring deviceName;
    // reamostragem linear
    double pos = 0; float prevL = 0, prevR = 0;
    std::promise<bool> ready_;

    bool Start(bool lb, std::atomic<float>* lvl) {
        loopback = lb; level = lvl; run = true;
        auto fut = ready_.get_future();
        th = std::thread([this] { Loop(ready_); });
        bool ok = fut.get();
        if (!ok && th.joinable()) th.join();
        return ok;
    }
    void Stop() { run = false; if (th.joinable()) th.join(); }

    size_t Available() { std::lock_guard<std::mutex> lk(m); return buf.size() / 2; }

    void Loop(std::promise<bool>& ready) {
        CoInitializeEx(nullptr, COINIT_MULTITHREADED);
        bool signalled = false;
        auto fail = [&](const wchar_t* what, HRESULT hr) {
            Log(L"Áudio (%s): %s falhou 0x%08X", loopback ? L"sistema" : L"microfone", what, hr);
            if (!signalled) { ready.set_value(false); signalled = true; }
        };
        do {
            ComPtr<IMMDeviceEnumerator> en;
            HRESULT hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL, IID_PPV_ARGS(&en));
            if (FAILED(hr)) { fail(L"MMDeviceEnumerator", hr); break; }
            ComPtr<IMMDevice> dev;
            hr = en->GetDefaultAudioEndpoint(loopback ? eRender : eCapture, eConsole, &dev);
            if (FAILED(hr)) { fail(L"GetDefaultAudioEndpoint", hr); break; }
            ComPtr<IAudioClient> client;
            hr = dev->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr, (void**)client.GetAddressOf());
            if (FAILED(hr)) { fail(L"Activate", hr); break; }
            WAVEFORMATEX* wf = nullptr;
            hr = client->GetMixFormat(&wf);
            if (FAILED(hr)) { fail(L"GetMixFormat", hr); break; }
            hr = client->Initialize(AUDCLNT_SHAREMODE_SHARED, loopback ? AUDCLNT_STREAMFLAGS_LOOPBACK : 0, 2000000, 0, wf, nullptr);
            if (FAILED(hr)) {
                // alguns drivers recusam o próprio formato de mixagem: tenta float estéreo 48 kHz com conversão do WASAPI
                Log(L"Áudio (%s): formato de mixagem recusado (0x%08X), tentando conversão automática", loopback ? L"sistema" : L"microfone", hr);
                client.Reset();
                if (FAILED(dev->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr, (void**)client.GetAddressOf()))) { CoTaskMemFree(wf); fail(L"Activate", hr); break; }
                WAVEFORMATEX* std = (WAVEFORMATEX*)CoTaskMemAlloc(sizeof(WAVEFORMATEX));
                *std = {};
                std->wFormatTag = WAVE_FORMAT_IEEE_FLOAT; std->nChannels = 2; std->nSamplesPerSec = 48000; std->wBitsPerSample = 32;
                std->nBlockAlign = 8; std->nAvgBytesPerSec = 48000 * 8;
                hr = client->Initialize(AUDCLNT_SHAREMODE_SHARED, (loopback ? AUDCLNT_STREAMFLAGS_LOOPBACK : 0) |
                                        AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, 2000000, 0, std, nullptr);
                CoTaskMemFree(wf);
                wf = std;
                if (FAILED(hr)) { CoTaskMemFree(wf); fail(L"Initialize", hr); break; }
            }
            ComPtr<IAudioCaptureClient> cap;
            hr = client->GetService(IID_PPV_ARGS(&cap));
            if (FAILED(hr)) { CoTaskMemFree(wf); fail(L"GetService", hr); break; }
            bool isFloat = wf->wFormatTag == WAVE_FORMAT_IEEE_FLOAT;
            if (wf->wFormatTag == WAVE_FORMAT_EXTENSIBLE)
                isFloat = ((WAVEFORMATEXTENSIBLE*)wf)->SubFormat == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
            int ch = wf->nChannels, bits = wf->wBitsPerSample, rate = wf->nSamplesPerSec, blk = wf->nBlockAlign;
            Log(L"Áudio (%s): %d Hz, %d canais, %d bits%s", loopback ? L"sistema" : L"microfone", rate, ch, bits, isFloat ? L" float" : L"");
            CoTaskMemFree(wf);
            hr = client->Start();
            if (FAILED(hr)) { fail(L"Start", hr); break; }
            ready.set_value(true); signalled = true;

            std::vector<float> st;   // quadros estéreo na taxa original
            while (run) {
                Sleep(10);
                UINT32 pkt = 0;
                while (SUCCEEDED(cap->GetNextPacketSize(&pkt)) && pkt) {
                    BYTE* data; UINT32 frames; DWORD flags;
                    if (FAILED(cap->GetBuffer(&data, &frames, &flags, nullptr, nullptr))) break;
                    st.resize((size_t)frames * 2);
                    float peak = 0;
                    for (UINT32 f = 0; f < frames; f++) {
                        float s[2] = { 0, 0 };
                        if (!(flags & AUDCLNT_BUFFERFLAGS_SILENT)) {
                            for (int c = 0; c < std::min(ch, 2); c++) {
                                const BYTE* p = data + (size_t)f * blk + (size_t)c * (bits / 8);
                                float v = 0;
                                if (isFloat && bits == 32) v = *(const float*)p;
                                else if (bits == 16) v = *(const int16_t*)p / 32768.0f;
                                else if (bits == 24) v = ((int)((p[0] << 8) | (p[1] << 16) | (p[2] << 24)) >> 8) / 8388608.0f;
                                else if (bits == 32) v = *(const int32_t*)p / 2147483648.0f;
                                s[c] = v;
                            }
                            if (ch == 1) s[1] = s[0];
                        }
                        st[(size_t)f * 2] = s[0]; st[(size_t)f * 2 + 1] = s[1];
                        peak = std::max(peak, std::max(std::fabs(s[0]), std::fabs(s[1])));
                    }
                    cap->ReleaseBuffer(frames);
                    if (level) level->store(std::max(peak, level->load() * 0.85f));
                    Push(st, frames, rate);
                }
                if (level) level->store(level->load() * 0.9f);
            }
            client->Stop();
        } while (false);
        if (!signalled) ready.set_value(false);
        CoUninitialize();
    }

    void Push(const std::vector<float>& s, UINT32 n, int rate) {
        std::lock_guard<std::mutex> lk(m);
        if (rate == kRate) { buf.insert(buf.end(), s.begin(), s.begin() + (size_t)n * 2); }
        else {
            double ratio = (double)rate / kRate;
            while (pos <= (double)n - 1) {
                int i = (int)std::floor(pos);
                double fr = pos - i;
                float aL = i < 0 ? prevL : s[(size_t)i * 2], aR = i < 0 ? prevR : s[(size_t)i * 2 + 1];
                float bL = s[(size_t)(i + 1) * 2], bR = s[(size_t)(i + 1) * 2 + 1];
                buf.push_back((float)(aL + (bL - aL) * fr));
                buf.push_back((float)(aR + (bR - aR) * fr));
                pos += ratio;
            }
            pos -= n;
            if (n) { prevL = s[(size_t)(n - 1) * 2]; prevR = s[(size_t)(n - 1) * 2 + 1]; }
        }
        size_t maxFrames = kRate * 5;                 // nunca acumula mais que 5 s
        if (buf.size() > maxFrames * 2) buf.erase(buf.begin(), buf.begin() + (buf.size() - maxFrames * 2));
    }

    // retira até n quadros; devolve quantos vieram
    size_t Pop(float* out, size_t n) {
        std::lock_guard<std::mutex> lk(m);
        size_t avail = buf.size() / 2;
        // deriva: se o buffer cresceu demais (> n + 200 ms), descarta o excesso mais antigo
        if (avail > n + kRate / 5) {
            size_t drop = avail - n - kRate / 20;
            buf.erase(buf.begin(), buf.begin() + drop * 2);
            avail -= drop;
        }
        size_t take = std::min(n, avail);
        for (size_t i = 0; i < take * 2; i++) out[i] = buf[i];
        buf.erase(buf.begin(), buf.begin() + take * 2);
        return take;
    }
    void Clear() { std::lock_guard<std::mutex> lk(m); buf.clear(); }
};

// ---------------------------------------------------------------- Recorder
Recorder::~Recorder() { if (running_) Cancel(); }

LONGLONG Recorder::Now100ns() const {
    std::lock_guard<std::mutex> lk(clockMutex_);
    LARGE_INTEGER now; QueryPerformanceCounter(&now);
    LONGLONG t = (now.QuadPart - qpcStart_.QuadPart) * kHns / qpcFreq_.QuadPart;
    LONGLONG paused = pausedTotal_;
    if (state_ == Paused) paused += t - pauseStart_;
    return std::max<LONGLONG>(0, t - paused);
}

double Recorder::Elapsed() const { return state_ == Idle ? 0 : Now100ns() / (double)kHns; }

static HRESULT CreateWriter(const RecordSettings& s, int w, int h, bool hw, bool audio, ComPtr<IMFSinkWriter>& out,
                            DWORD& vStream, DWORD& aStream) {
    ComPtr<IMFAttributes> attr;
    MFCreateAttributes(&attr, 3);
    attr->SetUINT32(MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, hw ? TRUE : FALSE);
    attr->SetGUID(MF_TRANSCODE_CONTAINERTYPE, MFTranscodeContainerType_MPEG4);
    ComPtr<IMFSinkWriter> wr;
    HRESULT hr = MFCreateSinkWriterFromURL(s.path.c_str(), nullptr, attr.Get(), &wr);
    if (FAILED(hr)) return hr;

    static const double bppTable[4] = { 0.04, 0.07, 0.10, 0.16 };
    UINT32 bitrate = (UINT32)std::clamp(bppTable[std::clamp(s.quality, 0, 3)] * w * h * s.fps, 400000.0, 40000000.0);
    ComPtr<IMFMediaType> vout, vin;
    MFCreateMediaType(&vout);
    vout->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    vout->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_H264);
    vout->SetUINT32(MF_MT_AVG_BITRATE, bitrate);
    vout->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    MFSetAttributeSize(vout.Get(), MF_MT_FRAME_SIZE, w, h);
    MFSetAttributeRatio(vout.Get(), MF_MT_FRAME_RATE, s.fps, 1);
    MFSetAttributeRatio(vout.Get(), MF_MT_PIXEL_ASPECT_RATIO, 1, 1);
    if (FAILED(hr = wr->AddStream(vout.Get(), &vStream))) return hr;
    MFCreateMediaType(&vin);
    vin->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    vin->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_RGB32);
    vin->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    MFSetAttributeSize(vin.Get(), MF_MT_FRAME_SIZE, w, h);
    MFSetAttributeRatio(vin.Get(), MF_MT_FRAME_RATE, s.fps, 1);
    MFSetAttributeRatio(vin.Get(), MF_MT_PIXEL_ASPECT_RATIO, 1, 1);
    if (FAILED(hr = wr->SetInputMediaType(vStream, vin.Get(), nullptr))) return hr;

    if (audio) {
        ComPtr<IMFMediaType> aout, ain;
        MFCreateMediaType(&aout);
        aout->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Audio);
        aout->SetGUID(MF_MT_SUBTYPE, MFAudioFormat_AAC);
        aout->SetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
        aout->SetUINT32(MF_MT_AUDIO_SAMPLES_PER_SECOND, kRate);
        aout->SetUINT32(MF_MT_AUDIO_NUM_CHANNELS, 2);
        aout->SetUINT32(MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24000);
        aout->SetUINT32(MF_MT_AAC_PAYLOAD_TYPE, 0);
        if (FAILED(hr = wr->AddStream(aout.Get(), &aStream))) return hr;
        MFCreateMediaType(&ain);
        ain->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Audio);
        ain->SetGUID(MF_MT_SUBTYPE, MFAudioFormat_PCM);
        ain->SetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
        ain->SetUINT32(MF_MT_AUDIO_SAMPLES_PER_SECOND, kRate);
        ain->SetUINT32(MF_MT_AUDIO_NUM_CHANNELS, 2);
        ain->SetUINT32(MF_MT_AUDIO_BLOCK_ALIGNMENT, 4);
        ain->SetUINT32(MF_MT_AUDIO_AVG_BYTES_PER_SECOND, kRate * 4);
        if (FAILED(hr = wr->SetInputMediaType(aStream, ain.Get(), nullptr))) return hr;
    }
    if (FAILED(hr = wr->BeginWriting())) return hr;
    out = wr;
    return S_OK;
}

bool Recorder::Start(const RecordSettings& s, HWND notify, UINT notifyMsg) {
    if (state_ != Idle) return false;
    settings_ = s;
    notify_ = notify; notifyMsg_ = notifyMsg;
    micMuted_ = s.micMuted; sysMuted_ = s.sysMuted;
    lastError.clear();
    firstFrame = Image();

    // tamanho de saída: par, ≤ 1080 linhas (opcional) e dentro do limite do codificador H.264 (4096×2304)
    int w = s.rect.right - s.rect.left, h = s.rect.bottom - s.rect.top;
    if (w < 16 || h < 16) { lastError = L"Área de gravação muito pequena."; return false; }
    double sc = 1.0;
    if (s.downscale1080 && h > 1080) sc = 1080.0 / h;
    sc = std::min({ sc, 4096.0 / w, 2304.0 / h });
    outW_ = std::max(32, ((int)(w * sc)) & ~1);
    outH_ = std::max(32, ((int)(h * sc)) & ~1);

    if (FAILED(MFStartup(MF_VERSION, MFSTARTUP_FULL))) { lastError = L"Media Foundation indisponível."; return false; }

    // fontes de áudio
    if (s.microphone) { mic_ = std::make_shared<AudioSource>(); hasMic_ = mic_->Start(false, &micLevel_); if (!hasMic_) mic_.reset(); }
    if (s.systemAudio) { sys_ = std::make_shared<AudioSource>(); hasSys_ = sys_->Start(true, &sysLevel_); if (!hasSys_) sys_.reset(); }
    audioEnabled_ = hasMic_ || hasSys_;

    ComPtr<IMFSinkWriter> wr;
    HRESULT hr = CreateWriter(settings_, outW_, outH_, s.hardware, audioEnabled_, wr, videoStream_, audioStream_);
    if (FAILED(hr) && s.hardware) {
        Log(L"Gravador: codificador por hardware recusado (0x%08X), tentando software", hr);
        DeleteFileW(s.path.c_str());
        hr = CreateWriter(settings_, outW_, outH_, false, audioEnabled_, wr, videoStream_, audioStream_);
    }
    if (FAILED(hr)) {
        lastError = Format(L"Não foi possível iniciar o codificador MP4 (0x%08X).", hr);
        Log(L"Gravador: %s", lastError.c_str());
        if (mic_) mic_->Stop(); if (sys_) sys_->Stop();
        mic_.reset(); sys_.reset();
        MFShutdown();
        return false;
    }
    writer_ = wr.Detach();
    QueryPerformanceFrequency(&qpcFreq_);
    QueryPerformanceCounter(&qpcStart_);
    pausedTotal_ = 0;
    state_ = Recording;
    running_ = true;
    SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED);   // impede o sistema de dormir
    videoThread_ = std::thread([this] { VideoThread(); });
    if (audioEnabled_) audioThread_ = std::thread([this] { AudioMixThread(); });
    Log(L"Gravador: iniciado %dx%d → %dx%d @%d fps, áudio mic=%d sis=%d, %s", w, h, outW_, outH_, s.fps, hasMic_, hasSys_, s.path.c_str());
    return true;
}

void Recorder::Pause() {
    if (state_ != Recording) return;
    std::lock_guard<std::mutex> lk(clockMutex_);
    LARGE_INTEGER now; QueryPerformanceCounter(&now);
    pauseStart_ = (now.QuadPart - qpcStart_.QuadPart) * kHns / qpcFreq_.QuadPart;
    state_ = Paused;
}

void Recorder::Resume() {
    if (state_ != Paused) return;
    {
        std::lock_guard<std::mutex> lk(clockMutex_);
        LARGE_INTEGER now; QueryPerformanceCounter(&now);
        LONGLONG t = (now.QuadPart - qpcStart_.QuadPart) * kHns / qpcFreq_.QuadPart;
        pausedTotal_ += t - pauseStart_;
        state_ = Recording;
    }
    if (mic_) mic_->Clear();
    if (sys_) sys_->Clear();
}

bool Recorder::WriteSample(DWORD stream, const void* data, DWORD len, LONGLONG time, LONGLONG dur) {
    ComPtr<IMFMediaBuffer> buf;
    if (FAILED(MFCreateMemoryBuffer(len, &buf))) return false;
    BYTE* p = nullptr;
    buf->Lock(&p, nullptr, nullptr);
    memcpy(p, data, len);
    buf->Unlock();
    buf->SetCurrentLength(len);
    ComPtr<IMFSample> smp;
    MFCreateSample(&smp);
    smp->AddBuffer(buf.Get());
    smp->SetSampleTime(time);
    smp->SetSampleDuration(dur);
    std::lock_guard<std::mutex> lk(writerMutex_);
    HRESULT hr = ((IMFSinkWriter*)writer_)->WriteSample(stream, smp.Get());
    if (FAILED(hr)) Log(L"Gravador: WriteSample(%u) 0x%08X", stream, hr);
    return SUCCEEDED(hr);
}

void Recorder::VideoThread() {
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    timeBeginPeriod(1);
    const RECT r = settings_.rect;
    const int sw = r.right - r.left, sh = r.bottom - r.top;
    HDC screen = GetDC(nullptr);
    HDC mem = CreateCompatibleDC(screen);
    BITMAPINFO bi{};
    bi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    bi.bmiHeader.biWidth = outW_; bi.bmiHeader.biHeight = outH_;     // de baixo para cima = RGB32 padrão do MF
    bi.bmiHeader.biPlanes = 1; bi.bmiHeader.biBitCount = 32; bi.bmiHeader.biCompression = BI_RGB;
    void* bits = nullptr;
    HBITMAP dib = CreateDIBSection(screen, &bi, DIB_RGB_COLORS, &bits, nullptr, 0);
    HGDIOBJ old = SelectObject(mem, dib);
    const bool scaled = outW_ != sw || outH_ != sh;
    SetStretchBltMode(mem, HALFTONE); SetBrushOrgEx(mem, 0, 0, nullptr);
    const double fx = (double)outW_ / sw, fy = (double)outH_ / sh;
    const LONGLONG frameDur = kHns / settings_.fps;
    const DWORD bytes = (DWORD)outW_ * outH_ * 4;
    LONGLONG lastTs = -1;
    ULONGLONG lastDiskCheck = 0;
    bool warnedDisk = false;

    while (running_) {
        if (state_ == Paused) { Sleep(10); continue; }
        LONGLONG t = Now100ns();
        if (scaled) StretchBlt(mem, 0, 0, outW_, outH_, screen, r.left, r.top, sw, sh, SRCCOPY | CAPTUREBLT);
        else BitBlt(mem, 0, 0, outW_, outH_, screen, r.left, r.top, SRCCOPY | CAPTUREBLT);
        if (settings_.cursor) {
            CURSORINFO ci{ sizeof(ci) };
            if (GetCursorInfo(&ci) && (ci.flags & CURSOR_SHOWING)) {
                ICONINFO ii{};
                if (GetIconInfo(ci.hCursor, &ii)) {
                    int cx = (int)((ci.ptScreenPos.x - r.left - (int)ii.xHotspot) * fx);
                    int cy = (int)((ci.ptScreenPos.y - r.top - (int)ii.yHotspot) * fy);
                    int cs = (int)(GetSystemMetrics(SM_CXCURSOR) * std::min(fx, 1.0));
                    DrawIconEx(mem, cx, cy, ci.hCursor, scaled ? cs : 0, scaled ? cs : 0, 0, nullptr, DI_NORMAL);
                    if (ii.hbmMask) DeleteObject(ii.hbmMask);
                    if (ii.hbmColor) DeleteObject(ii.hbmColor);
                }
            }
        }
        GdiFlush();
        // canal alfa do BitBlt é indefinido: o codificador ignora, mas a miniatura precisa de opaco
        if (firstFrame.empty()) {
            Image f(outW_, outH_);
            for (int y = 0; y < outH_; y++) memcpy(f.row(y), (uint32_t*)bits + (size_t)(outH_ - 1 - y) * outW_, outW_ * 4);
            f.MakeOpaque();
            firstFrame = std::move(f);
        }
        LONGLONG ts = std::max(t, lastTs + 1);
        if (!WriteSample(videoStream_, bits, bytes, ts, frameDur)) {
            lastError = L"Falha ao gravar quadro de vídeo.";
            PostMessageW(notify_, notifyMsg_, NoteError, 0);
            break;
        }
        lastTs = ts;
        // limites de espaço em disco (avisa / para)
        ULONGLONG nowTick = GetTickCount64();
        if (nowTick - lastDiskCheck > 2000) {
            lastDiskCheck = nowTick;
            ULARGE_INTEGER freeBytes{};
            if (GetDiskFreeSpaceExW(DirOf(settings_.path).c_str(), &freeBytes, nullptr, nullptr)) {
                if (freeBytes.QuadPart < 200ull * 1024 * 1024) { PostMessageW(notify_, notifyMsg_, NoteDiskFull, 0); }
                else if (!warnedDisk && freeBytes.QuadPart < 1024ull * 1024 * 1024) { warnedDisk = true; PostMessageW(notify_, notifyMsg_, NoteDiskLow, 0); }
            }
        }
        // espera o próximo quadro na grade de tempo
        LONGLONG next = (t / frameDur + 1) * frameDur;
        for (;;) {
            if (!running_ || state_ == Paused) break;
            LONGLONG now = Now100ns();
            if (now >= next) break;
            LONGLONG ms = (next - now) / 10000;
            Sleep((DWORD)std::max<LONGLONG>(1, std::min<LONGLONG>(ms, 50)));
        }
    }
    SelectObject(mem, old);
    DeleteObject(dib); DeleteDC(mem);
    ReleaseDC(nullptr, screen);
    timeEndPeriod(1);
    CoUninitialize();
}

void Recorder::AudioMixThread() {
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    LONGLONG written = 0;            // quadros já gravados
    std::vector<float> a, b;
    std::vector<int16_t> out;
    while (running_) {
        Sleep(20);
        if (state_ == Paused) { if (mic_) mic_->Clear(); if (sys_) sys_->Clear(); continue; }
        LONGLONG target = Now100ns() * kRate / kHns;
        LONGLONG n = target - written;
        if (n <= 0) continue;
        if (n > kRate) n = kRate;
        a.assign((size_t)n * 2, 0.0f); b.assign((size_t)n * 2, 0.0f);
        if (mic_) mic_->Pop(a.data(), (size_t)n);
        if (sys_) sys_->Pop(b.data(), (size_t)n);
        out.resize((size_t)n * 2);
        bool mm = micMuted_, sm = sysMuted_;
        for (size_t i = 0; i < out.size(); i++) {
            float v = (mm ? 0.0f : a[i]) + (sm ? 0.0f : b[i]);
            out[i] = (int16_t)std::lround(std::clamp(v, -1.0f, 1.0f) * 32767.0f);
        }
        LONGLONG t0 = written * kHns / kRate, t1 = (written + n) * kHns / kRate;
        WriteSample(audioStream_, out.data(), (DWORD)(out.size() * 2), t0, t1 - t0);
        written += n;
    }
    CoUninitialize();
}

Image Recorder::TakeSnapshot() {
    HDC screen = GetDC(nullptr);
    Image img = Image::FromDC(screen, settings_.rect);
    ReleaseDC(nullptr, screen);
    return img;
}

bool Recorder::Stop() {
    if (state_ == Idle) return false;
    state_ = Stopping;
    running_ = false;
    if (videoThread_.joinable()) videoThread_.join();
    if (audioThread_.joinable()) audioThread_.join();
    if (mic_) mic_->Stop();
    if (sys_) sys_->Stop();
    mic_.reset(); sys_.reset();
    bool ok = false;
    if (writer_) {
        HRESULT hr = ((IMFSinkWriter*)writer_)->Finalize();
        ok = SUCCEEDED(hr);
        if (!ok) { Log(L"Gravador: Finalize 0x%08X", hr); lastError = Format(L"Falha ao finalizar o MP4 (0x%08X).", hr); }
        ((IMFSinkWriter*)writer_)->Release();
        writer_ = nullptr;
    }
    MFShutdown();
    SetThreadExecutionState(ES_CONTINUOUS);
    state_ = Idle;
    micLevel_ = 0; sysLevel_ = 0;
    Log(L"Gravador: parado (%s)", ok ? L"ok" : L"erro");
    return ok;
}

void Recorder::Cancel() {
    Stop();
    DeleteFileW(settings_.path.c_str());
    Log(L"Gravador: cancelado");
}

} // namespace snap
