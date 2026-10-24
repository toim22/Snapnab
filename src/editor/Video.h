// Vídeo no Editor (CVideoDoc / CMFVideoPlayer / MFMP4Encoder / GifCreator — ds_pseudocode §11, ed_comp_9)
#pragma once
#include "Image.h"

namespace snap::ed {

struct Interval { LONGLONG start, end; };      // trecho REMOVIDO (16 bytes, como no original)

// MFSampleHelper (§11.4)
bool IsInsideAnyCut(const std::vector<Interval>& cuts, LONGLONG t);
bool IsCutInBetweenFrames(const std::vector<Interval>& cuts, LONGLONG t);   // ∃ intervalo com end <= t
LONGLONG PrecedingCutsDuration(const std::vector<Interval>& cuts, LONGLONG t);

class VideoReader {
public:
    bool Open(const std::wstring& path);
    void Close();
    bool Seek(LONGLONG t);
    bool ReadFrame(Image& out, LONGLONG& ts);     // próximo quadro de vídeo (RGB32 → Image)
    int width = 0, height = 0;
    LONGLONG duration = 0;                         // 100 ns
    double fps = 30;
    ~VideoReader() { Close(); }
private:
    void* reader_ = nullptr;                       // IMFSourceReader*
    bool mfStarted_ = false;
};

using Progress = std::function<bool(int percent)>;   // devolve false para cancelar

// MFMP4Encoder::Transcode::ToFileWithCuts
bool TranscodeWithCuts(const std::wstring& src, const std::wstring& dst, std::vector<Interval> cuts, Progress progress, std::wstring& err);
// V2AGNative::GifCreator::CreateFromVideoFile
bool CreateGifFromVideo(const std::wstring& src, const std::wstring& dst, LONGLONG from, LONGLONG to, int fps, int width,
                        int bitsPerPixel, bool dithering, bool deltaFrames, Progress progress, std::wstring& err);

// executa trabalho longo numa thread com janela de progresso cancelável
bool RunWithProgress(HWND owner, const std::wstring& title, std::function<bool(Progress)> work);

class VideoView {
public:
    HWND hwnd = nullptr;
    void Create(HWND parent);
    bool Open(const std::wstring& path);
    void Close();
    bool IsOpen() const { return open_; }
    std::function<void(const Image&)> onSaveFrame;         // abre o quadro como imagem nova
    std::function<void(const std::wstring&)> onNewVideo;    // vídeo exportado (atualiza a biblioteca)
    std::wstring path;
private:
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    LRESULT Handle(UINT m, WPARAM w, LPARAM l);
    void Layout();
    void Paint(HDC dc);
    void ShowAt(LONGLONG t);
    void Step(int dir);
    void TogglePlay();
    void StopPlay();
    void SaveVideo();
    void SaveGif();
    RECT TimelineRect() const;
    LONGLONG TimeFromX(int x) const;
    VideoReader reader_;
    Image frame_;
    LONGLONG pos_ = 0, selA_ = -1, selB_ = -1;
    std::vector<Interval> cuts_;
    std::vector<std::vector<Interval>> cutUndo_;
    bool open_ = false, playing_ = false, scrubbing_ = false;
    std::map<int, HWND> btn_;
};

} // namespace snap::ed
