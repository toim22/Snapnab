// Gravação de tela — equivalente de TSCRec3 + AVFileWriter + AVStreamEncoder (ds_arch §12, comp_3)
//   vídeo: BitBlt da área → amostras RGB32 → IMFSinkWriter (H.264, MP4), codificação por hardware opcional
//   áudio: WASAPI (microfone + loopback do sistema) → mixer com mudo separado → AAC
#pragma once
#include "Image.h"

namespace snap {

struct RecordSettings {
    RECT rect{};                 // área de tela
    bool cursor = true;
    bool microphone = true;
    bool systemAudio = true;
    bool micMuted = false, sysMuted = false;
    int fps = 30;
    int quality = 2;             // 0..3
    bool downscale1080 = true;
    bool hardware = true;
    std::wstring path;
};

class Recorder {
public:
    enum State { Idle, Recording, Paused, Stopping };   // espelha os estados de VideoRecording (comp_3 §4.1)
    bool Start(const RecordSettings& s, HWND notify, UINT notifyMsg);
    void Pause();
    void Resume();
    bool Stop();                 // finaliza o MP4
    void Cancel();               // para e apaga
    State state() const { return state_; }
    double Elapsed() const;      // segundos gravados (sem pausas)
    void SetMicMuted(bool m) { micMuted_ = m; }
    void SetSysMuted(bool m) { sysMuted_ = m; }
    bool micMuted() const { return micMuted_; }
    bool sysMuted() const { return sysMuted_; }
    bool hasMic() const { return hasMic_; }
    bool hasSys() const { return hasSys_; }
    float micLevel() const { return micLevel_; }
    float sysLevel() const { return sysLevel_; }
    const std::wstring& path() const { return settings_.path; }
    Image TakeSnapshot();        // imagem da área durante a gravação
    Image firstFrame;
    std::wstring lastError;
    ~Recorder();

    // notificações enviadas para notifyMsg (wParam)
    enum Note { NoteDiskLow = 1, NoteDiskFull = 2, NoteError = 3 };
private:
    struct AudioSource;
    void VideoThread();
    void AudioMixThread();
    LONGLONG Now100ns() const;   // relógio da gravação (exclui pausas)
    bool WriteSample(DWORD stream, const void* data, DWORD len, LONGLONG time, LONGLONG dur);

    RecordSettings settings_;
    HWND notify_ = nullptr; UINT notifyMsg_ = 0;
    std::atomic<State> state_{ Idle };
    std::atomic<bool> running_{ false };
    std::atomic<bool> micMuted_{ false }, sysMuted_{ false };
    std::atomic<float> micLevel_{ 0 }, sysLevel_{ 0 };
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
