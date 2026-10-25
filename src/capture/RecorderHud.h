// HUD de gravação (RecorderUI / snagitminirecordinghud, comp_3 §3.3) + contagem regressiva + moldura da área
#pragma once
#include "Recorder.h"

namespace snap {

struct HudCallbacks {
    std::function<void()> onRecord;        // iniciar (quando ocioso) / pausar-retomar
    std::function<void()> onStop;
    std::function<void()> onRestart;
    std::function<void()> onCancel;
    std::function<void()> onSnapshot;
    std::function<void(bool)> onMicMute;
    std::function<void(bool)> onSysMute;
    std::function<void(bool)> onWebcam;    // mostrar/ocultar a webcam (ToggleWebcamCommand)
};

class RecorderHud {
public:
    void Show(RECT area, Recorder* rec, HudCallbacks cb, bool micOn, bool sysOn);
    void Hide();
    void Refresh();
    void Countdown(int seconds, std::function<void()> then);
    void SkipCountdown();                  // "Pular" na contagem (ou o botão de gravar durante ela)
    bool counting() const { return count_ != nullptr; }
    HWND hwnd() const { return hwnd_; }
    bool micOn = true, sysOn = true;
    bool camAvailable = false, camOn = false;   // botão da webcam: só aparece se a webcam foi ligada no preset
private:
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    static LRESULT CALLBACK CountProc(HWND, UINT, WPARAM, LPARAM);
    void Paint(HDC dc);
    int HitButton(POINT p);
    void Layout();
    struct Button { int id; RECT r; };
    std::vector<Button> buttons_;
    HWND hwnd_ = nullptr, border_ = nullptr, count_ = nullptr;
    RECT area_{};
    Recorder* rec_ = nullptr;
    HudCallbacks cb_;
    int hover_ = -1;
    void RenderCount();
    RECT SkipRect() const;
    int countLeft_ = 0;
    bool skipHot_ = false;
    std::function<void()> countThen_;
};

} // namespace snap
