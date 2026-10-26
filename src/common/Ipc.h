// Notification channel between the capturer and the editor — ds_pseudocode §4 / ds_arch §19.8 / ed_comp_14 §3
//   4 KiB mapping + mutex + event per channel + acknowledgement (ack) event
//   message = [u32 code][u32 kind][payload]   kind 0 = int (4 B) | 1 = 16 B | 2 = NUL-terminated UTF-16
#pragma once
#include "Common.h"

namespace snap::ipc {

enum Channel : uint32_t { kCapture = 0, kEditor = 1 };
enum Kind : uint32_t { kInt = 0, kBlob = 1, kString = 2 };

// Codes (same numbering as the original where the meaning is known)
enum Code : uint32_t {
    // capturer → editor (CMainFrame::OnIPCNotify)
    ShowEditor            = 0x06,   // show/activate the Editor
    CloseEditorIfHidden   = 0x0A,   // close if hidden
    ReloadConfig          = 0x0C,   // reload configuration
    ShowEditorWithParam   = 0x0E,
    NewImageFromClipboard = 0x1E,   // "Snip detected. New Image From Clipboard commencing."
    OpenFile              = 0x30,   // string: path of the file to open (Snapnab)
    LibraryChanged        = 0x31,   // library changed → refresh the tray
    RestartEditor         = 0x32,   // language changed in Options: reopen the Editor
    // editor → capturer (CSnagItUIFrame::OnIPCNotify)
    LibraryDocOpened      = 0x02,   // blob: GUID
    LibraryDocReleased    = 0x03,   // blob: GUID
    EditorStartCapture    = 0x12,   // start a capture from the Editor (int: 0 image, 1 video)
    EditorClosed          = 0x13,
    EditorOptionChanged   = 0x17,
};

struct Message {
    uint32_t code = 0, kind = 0;
    int32_t ivalue = 0;
    uint8_t blob[16]{};
    std::wstring text;
};

class Notify {
public:
    using Listener = std::function<void(const Message&)>;
    bool Start(Channel self, Listener listener);
    void Stop();
    // waitForAck: waits for the peer to consume it (up to 10 s if its window is not hung)
    bool Send(Channel dest, uint32_t code, uint32_t kind, const void* payload, uint32_t len, bool waitForAck = false);
    bool SendInt(Channel dest, uint32_t code, int32_t v, bool wait = false) { return Send(dest, code, kInt, &v, 4, wait); }
    bool SendString(Channel dest, uint32_t code, const std::wstring& s, bool wait = false) {
        return Send(dest, code, kString, s.c_str(), (uint32_t)((s.size() + 1) * 2), wait);
    }
    ~Notify() { Stop(); }
private:
    void ListenerThread();
    Channel self_ = kCapture;
    Listener listener_;
    std::thread thread_;
    std::atomic<bool> running_{ false };
    HANDLE event_ = nullptr, ack_ = nullptr, stopEvent_ = nullptr, aliveMutex_ = nullptr, mutex_ = nullptr, map_ = nullptr;
};

} // namespace snap::ipc
