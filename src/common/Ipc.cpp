#include "Ipc.h"

namespace snap::ipc {

static const wchar_t kMapName[]   = L"SnapnabIPCNotifyMemMapFile";
static const wchar_t kMutexName[] = L"SnapnabIPCNotifyMutex";
static const DWORD   kMapSize     = 0x1000;     // 4 KiB

static std::wstring EventName(uint32_t ch)  { return Format(L"SnapnabIPCNotifyEvent%u", ch); }
static std::wstring AckName(uint32_t ch)    { return Format(L"SnapnabIPCNotifyWaitUntilMessageSentEvent%u", ch); }
static std::wstring AliveName(uint32_t ch)  { return Format(L"SnapnabIPCNotifyThreadStartedMutex%u", ch); }

static HANDLE OpenMap() {
    return CreateFileMappingW(INVALID_HANDLE_VALUE, nullptr, PAGE_READWRITE, 0, kMapSize, kMapName);
}

bool Notify::Start(Channel self, Listener listener) {
    self_ = self;
    listener_ = std::move(listener);
    event_ = CreateEventW(nullptr, TRUE, FALSE, EventName(self).c_str());
    ack_ = CreateEventW(nullptr, TRUE, FALSE, AckName(self).c_str());
    stopEvent_ = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    mutex_ = CreateMutexW(nullptr, FALSE, kMutexName);
    map_ = OpenMap();
    if (!event_ || !ack_ || !mutex_ || !map_) { Log(L"IPC: failed to create objects (%u)", GetLastError()); return false; }
    running_ = true;
    thread_ = std::thread([this] { ListenerThread(); });
    return true;
}

void Notify::Stop() {
    if (!running_) return;
    running_ = false;
    SetEvent(stopEvent_);
    if (thread_.joinable()) thread_.join();
    for (HANDLE* h : { &event_, &ack_, &stopEvent_, &mutex_, &map_ }) if (*h) { CloseHandle(*h); *h = nullptr; }
}

void Notify::ListenerThread() {
    // "receiver thread alive" — the sender waits for this named mutex before sending
    aliveMutex_ = CreateMutexW(nullptr, TRUE, AliveName(self_).c_str());
    HANDLE waits[2] = { event_, stopEvent_ };
    while (running_) {
        DWORD r = WaitForMultipleObjects(2, waits, FALSE, INFINITE);
        if (r != WAIT_OBJECT_0) break;
        ResetEvent(event_);

        Message m;
        WaitForSingleObject(mutex_, 5000);
        if (auto* p = (const uint8_t*)MapViewOfFile(map_, FILE_MAP_READ, 0, 0, kMapSize)) {
            memcpy(&m.code, p, 4);
            memcpy(&m.kind, p + 4, 4);
            switch (m.kind) {
            case kInt: memcpy(&m.ivalue, p + 8, 4); break;
            case kBlob: memcpy(m.blob, p + 8, 16); break;
            case kString: {
                const wchar_t* t = (const wchar_t*)(p + 8);
                m.text.assign(t, wcsnlen(t, (kMapSize - 8) / 2));
                break;
            }
            }
            UnmapViewOfFile(p);
        }
        ReleaseMutex(mutex_);
        SetEvent(ack_);                         // releases the sender
        Log(L"IPC: recebido code=0x%02X kind=%u", m.code, m.kind);
        if (listener_) listener_(m);
    }
    if (aliveMutex_) { ReleaseMutex(aliveMutex_); CloseHandle(aliveMutex_); aliveMutex_ = nullptr; }
}

bool Notify::Send(Channel dest, uint32_t code, uint32_t kind, const void* payload, uint32_t len, bool waitForAck) {
    if (dest == self_) return false;                                 // never sends to itself
    const wchar_t* peerMutex = dest == kEditor ? kEditorMutex : kCaptureMutex;
    if (!IsProcessRunning(peerMutex)) return false;                  // nobody on the other side
    // waits for the peer's receiver thread: up to 75 × 200 ms = 15 s
    bool ready = false;
    for (int i = 0; i < 75; i++) {
        HANDLE a = OpenMutexW(SYNCHRONIZE, FALSE, AliveName(dest).c_str());
        if (a) { CloseHandle(a); ready = true; break; }
        Sleep(200);
    }
    if (!ready) { Log(L"IPC: peer %u did not respond", dest); return false; }
    if (len > kMapSize - 8) len = kMapSize - 8;

    HANDLE ack = CreateEventW(nullptr, TRUE, FALSE, AckName(dest).c_str());
    ResetEvent(ack);
    HANDLE m = CreateMutexW(nullptr, FALSE, kMutexName);
    WaitForSingleObject(m, 5000);
    HANDLE map = OpenMap();
    if (auto* p = (uint8_t*)MapViewOfFile(map, FILE_MAP_WRITE, 0, 0, kMapSize)) {
        memcpy(p, &code, 4);
        memcpy(p + 4, &kind, 4);
        if (payload && len) memcpy(p + 8, payload, len);
        FlushViewOfFile(p, 0);
        UnmapViewOfFile(p);
    }
    ReleaseMutex(m);
    HANDLE ev = CreateEventW(nullptr, TRUE, FALSE, EventName(dest).c_str());
    SetEvent(ev);
    if (waitForAck) {
        HWND w = FindWindowW(dest == kEditor ? kEditorWndClass : kCaptureWndClass, nullptr);
        if (w && !IsHungAppWindow(w)) WaitForSingleObject(ack, 10000);   // does not block if the peer is hung
        ResetEvent(ack);
    }
    CloseHandle(ev); CloseHandle(map); CloseHandle(m); CloseHandle(ack);
    Log(L"IPC: enviado code=0x%02X kind=%u → %u", code, kind, dest);
    return true;
}

} // namespace snap::ipc
