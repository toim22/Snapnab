// Screen recording — equivalent of TSCRec3 + AVFileWriter + AVStreamEncoder (ds_arch §12, comp_3)
//   video: BitBlt of the area → RGB32 samples → IMFSinkWriter (H.264, MP4), optional hardware encoding
//   audio: WASAPI (microphone + system loopback) → mixer with separate mute → AAC
#pragma once
#include "Image.h"

namespace snap {

struct RecordSettings {
    RECT rect{};                 // screen area
    bool cursor = true;
    bool microphone = true;
    bool systemAudio = true;
    bool micMuted = false, sysMuted = false;
    bool autoGain = true;        // automatic microphone gain (otherwise only the manual volume)
    int micVolume = 100, sysVolume = 100;   // % (0–200)
    int fps = 30;
    int quality = 2;             // 0..3
    bool downscale1080 = true;
    bool hardware = true;
    std::wstring path;
};

class Recorder {
public:
    enum State { Idle, Recording, Paused, Stopping };   // mirrors the VideoRecording states (comp_3 §4.1)
    bool Start(const RecordSettings& s, HWND notify, UINT notifyMsg);
    void Pause();
    void Resume();
    bool Stop();                 // finalizes the MP4
    void Cancel();               // stops and deletes
    State state() const { return state_; }
    double Elapsed() const;      // seconds recorded (without pauses)
    void SetMicMuted(bool m) { micMuted_ = m; }
    void SetSysMuted(bool m) { sysMuted_ = m; }
    bool micMuted() const { return micMuted_; }
    bool sysMuted() const { return sysMuted_; }
    bool hasMic() const { return hasMic_; }
    bool hasSys() const { return hasSys_; }
    // levels for the meters, after the volume (and the automatic gain) are applied
    float micLevel() const { return std::min(1.0f, micLevel_ * micGainNow_ * micVol_ / 100.0f); }
    float sysLevel() const { return std::min(1.0f, sysLevel_ * sysVol_ / 100.0f); }
    // volume of each source in % (0–200) and automatic microphone gain: they also change during the recording
    void SetMicVolume(int v) { micVol_ = std::clamp(v, 0, 200); }
    void SetSysVolume(int v) { sysVol_ = std::clamp(v, 0, 200); }
    void SetAutoGain(bool on) { autoGain_ = on; }
    int micVolume() const { return micVol_; }
    int sysVolume() const { return sysVol_; }
    bool autoGain() const { return autoGain_; }
    // before the recording: opens the sources only for the meters (nothing is written); Start() closes it
    void StartMonitor(bool mic, bool sys);
    void StopMonitor();
    bool monitoring() const { return monitoring_; }
    const std::wstring& path() const { return settings_.path; }
    Image TakeSnapshot();        // image of the area during the recording
    Image firstFrame;
    std::wstring lastError;
    ~Recorder();

    // notifications sent to notifyMsg (wParam)
    enum Note { NoteDiskLow = 1, NoteDiskFull = 2, NoteError = 3 };
private:
    struct AudioSource;
    void VideoThread();
    void AudioMixThread();
    LONGLONG Now100ns() const;   // recording clock (excludes pauses)
    bool WriteSample(DWORD stream, const void* data, DWORD len, LONGLONG time, LONGLONG dur);

    RecordSettings settings_;
    HWND notify_ = nullptr; UINT notifyMsg_ = 0;
    std::atomic<State> state_{ Idle };
    std::atomic<bool> running_{ false };
    std::atomic<bool> micMuted_{ false }, sysMuted_{ false };
    std::atomic<float> micLevel_{ 0 }, sysLevel_{ 0 };
    std::atomic<int> micVol_{ 100 }, sysVol_{ 100 };
    std::atomic<bool> autoGain_{ true };
    std::atomic<float> micGainNow_{ 1 };          // current automatic gain (for the meter)
    std::atomic<bool> monitoring_{ false };
    std::thread monitorThread_;
    bool hasMic_ = false, hasSys_ = false;
    void* writer_ = nullptr;     // IMFSinkWriter*
    DWORD videoStream_ = 0, audioStream_ = 0;
    bool audioEnabled_ = false;
    std::mutex writerMutex_;
    std::thread videoThread_, audioThread_;
    std::shared_ptr<AudioSource> mic_, sys_;
    int outW_ = 0, outH_ = 0;
    LARGE_INTEGER qpcFreq_{}, qpcStart_{};
    mutable std::mutex clockMutex_;
    LONGLONG pausedTotal_ = 0, pauseStart_ = 0;
};

} // namespace snap
