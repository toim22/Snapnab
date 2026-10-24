// CCaptureEngine — máquina de estados da captura (ds_pseudocode §3, comp_1 §4–§7)
#pragma once
#include "Presets.h"
#include "Image.h"
#include "Ipc.h"
#include "Library.h"
#include "Recorder.h"
#include "RecorderHud.h"
#include "SelectOverlay.h"

namespace snap {

class CaptureEngine {
public:
    enum State { Idle, Activated, InputSelecting, InputSelected, Capturing, HaveInput, Outputting, Suspended };

    void Init(HWND frame, ipc::Notify* ipc);
    bool Activate(const Preset& preset);          // ponto de entrada (atalho, botão, bandeja, linha de comando)
    State state() const { return state_; }
    bool busy() const { return state_ != Idle; }

    // vídeo (atalhos VideoStartPauseResumeKey / VideoStopKey)
    void VideoStartPauseResume();
    void VideoStop();
    void VideoCancel();
    void OnRecorderNote(WPARAM note);

    // saída comum (também usada pela captura da área de transferência e pelo monitor de clipboard)
    void Output(const Image& img, const CaptureInfo& info);
    void DeliverToEditor(const std::wstring& path);

    std::function<void(const std::wstring& title, const std::wstring& text)> notify;   // balão da bandeja
    std::function<void(bool busy)> onBusyChanged;
    Recorder recorder;

private:
    void SetState(State s);
    void BeginInput();
    void OnInputSelected(const SelectResult& r);
    void InputCaptured(Image img, const CaptureInfo& info);
    void InputCanceled();
    void StartVideoHud(RECT area);
    void StartRecording();
    void FinishVideo(bool ok);
    void ShowDelay(int seconds, std::function<void()> then);
    Image FreezeScreen(RECT vs, bool withCursor);
    Image ScrollCapture(HWND top, HWND child, POINT at);
    Image ExtendedWindowCapture(HWND top);
    bool CaptureMenus(Image& out);

    HWND frame_ = nullptr;
    ipc::Notify* ipc_ = nullptr;
    State state_ = Idle;
    Preset preset_;
    Image frozen_;
    RECT vs_{};
    RecorderHud hud_;
    RECT videoArea_{};
    HWND delayWnd_ = nullptr;
};

} // namespace snap
