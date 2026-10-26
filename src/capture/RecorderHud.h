// Recording HUD (RecorderUI / snagitminirecordinghud, comp_3 §3.3) + countdown + area frame
#pragma once
#include "Recorder.h"

namespace snap {

struct HudCallbacks {
    std::function<void()> onRecord;        // start (when idle) / pause-resume
    std::function<void()> onStop;
    std::function<void()> onRestart;
    std::function<void()> onCancel;
    std::function<void()> onSnapshot;
    std::function<void(bool)> onMicMute;
    std::function<void(bool)> onSysMute;
    std::function<void(bool)> onWebcam;    // show/hide the webcam (ToggleWebcamCommand)
    std::function<void(int mic, int sys, bool autoGain)> onVolume;   // volume sliders changed (saved in the preset)
};

class RecorderHud {
public:
    void Show(RECT area, Recorder* rec, HudCallbacks cb, bool micOn, bool sysOn);
    void Hide();
    void Refresh();
    void Countdown(int seconds, std::function<void()> then);
    void SkipCountdown();                  // "Skip" during the countdown (or the record button during it)
    bool counting() const { return count_ != nullptr; }
    HWND hwnd() const { return hwnd_; }
    bool micOn = true, sysOn = true;
    bool camAvailable = false, camOn = false;   // webcam button: only shown if the webcam was turned on in the preset
    int micVolume = 100, sysVolume = 100;  // volume panel (before and during the recording)
    bool autoGain = true;
private:
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    static LRESULT CALLBACK CountProc(HWND, UINT, WPARAM, LPARAM);
    static LRESULT CALLBACK VolumeProc(HWND, UINT, WPARAM, LPARAM);
    void ToggleVolume();
    void PlaceVolume();                    // below the HUD (above when there is no room)
    void PaintVolume(HDC dc);
    RECT VolumeTrack(int which) const;     // 0 microphone, 1 system (track of the slider in the panel)
    RECT VolumeCheck() const;
    void SetVolumeFromX(int which, int x);
    HWND vol_ = nullptr;
    int volDrag_ = -1;
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
