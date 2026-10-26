#include "Links.h"
#include "SelectOverlay.h"
#include <uiautomation.h>
#include <dwmapi.h>
#include <wrl/client.h>
#include <future>

using Microsoft::WRL::ComPtr;

namespace snap {

namespace {
// programs whose windows show web pages and documents with links
bool IsLinkHost(HWND h) {
    static const wchar_t* apps[] = { L"chrome.exe", L"msedge.exe", L"firefox.exe", L"brave.exe", L"opera.exe", L"vivaldi.exe",
                                     L"iexplore.exe", L"arc.exe", L"winword.exe", L"powerpnt.exe", L"excel.exe", L"outlook.exe",
                                     L"onenote.exe", L"acrord32.exe", L"acrobat.exe", L"sumatrapdf.exe", L"thunderbird.exe" };
    std::wstring exe = ToLower(WindowProcessName(h));
    for (auto a : apps) if (exe == a) return true;
    return false;
}

bool Cloaked(HWND h) {
    DWORD c = 0;
    return SUCCEEDED(DwmGetWindowAttribute(h, DWMWA_CLOAKED, &c, sizeof c)) && c;
}

struct Shared { std::mutex m; std::vector<CapturedLink> links; std::atomic<bool> cancel{ false }; };

std::wstring VariantText(const VARIANT& v) { return v.vt == VT_BSTR && v.bstrVal ? std::wstring(v.bstrVal) : std::wstring(); }

void Search(std::shared_ptr<Shared> sh, std::vector<HWND> wins, RECT area) {
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    {
        ComPtr<IUIAutomation> uia;
        if (FAILED(CoCreateInstance(CLSID_CUIAutomation, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&uia)))) { CoUninitialize(); return; }
        VARIANT type; type.vt = VT_I4; type.lVal = UIA_HyperlinkControlTypeId;
        ComPtr<IUIAutomationCondition> cond;
        ComPtr<IUIAutomationCacheRequest> cache;
        if (FAILED(uia->CreatePropertyCondition(UIA_ControlTypePropertyId, type, &cond)) || FAILED(uia->CreateCacheRequest(&cache))) { CoUninitialize(); return; }
        cache->AddProperty(UIA_BoundingRectanglePropertyId);
        cache->AddProperty(UIA_ValueValuePropertyId);
        cache->AddProperty(UIA_LegacyIAccessibleValuePropertyId);
        cache->AddProperty(UIA_IsOffscreenPropertyId);
        for (HWND w : wins) {
            for (int attempt = 0; attempt < 2 && !sh->cancel; attempt++) {
                ComPtr<IUIAutomationElement> root;
                ComPtr<IUIAutomationElementArray> arr;
                if (FAILED(uia->ElementFromHandle(w, &root)) || !root) break;
                if (FAILED(root->FindAllBuildCache(TreeScope_Descendants, cond.Get(), cache.Get(), &arr)) || !arr) break;
                int n = 0; arr->get_Length(&n);
                int found = 0;
                for (int i = 0; i < n && !sh->cancel; i++) {
                    ComPtr<IUIAutomationElement> el;
                    if (FAILED(arr->GetElement(i, &el)) || !el) continue;
                    RECT rc{};
                    if (FAILED(el->get_CachedBoundingRectangle(&rc)) || rc.right <= rc.left || rc.bottom <= rc.top) continue;
                    VARIANT off; VariantInit(&off);
                    if (SUCCEEDED(el->GetCachedPropertyValue(UIA_IsOffscreenPropertyId, &off)) && off.vt == VT_BOOL && off.boolVal) { VariantClear(&off); continue; }
                    VariantClear(&off);
                    VARIANT val; VariantInit(&val);
                    std::wstring url;
                    if (SUCCEEDED(el->GetCachedPropertyValue(UIA_ValueValuePropertyId, &val))) url = VariantText(val);
                    VariantClear(&val);
                    if (url.empty() && SUCCEEDED(el->GetCachedPropertyValue(UIA_LegacyIAccessibleValuePropertyId, &val))) url = VariantText(val);
                    VariantClear(&val);
                    if (url.rfind(L"http://", 0) != 0 && url.rfind(L"https://", 0) != 0 && url.rfind(L"mailto:", 0) != 0) continue;
                    RECT vis;
                    if (!IntersectRect(&vis, &rc, &area)) continue;
                    // only where the window is really visible (not under another window)
                    POINT c{ (vis.left + vis.right) / 2, (vis.top + vis.bottom) / 2 };
                    if (GetAncestor(WindowFromPoint(c), GA_ROOT) != w) continue;
                    found++;
                    std::lock_guard<std::mutex> lk(sh->m);
                    bool dup = std::any_of(sh->links.begin(), sh->links.end(), [&](const CapturedLink& l) { return EqualRect(&l.r, &vis) && l.url == url; });
                    if (!dup) sh->links.push_back({ vis, url });
                }
                if (found || attempt) break;
                Sleep(250);                     // browsers build their accessibility tree when it is first asked for
            }
        }
    }
    CoUninitialize();
}
}

std::vector<CapturedLink> FindLinks(RECT area, DWORD budgetMs) {
    std::vector<HWND> wins;
    struct Ctx { RECT area; std::vector<HWND>* out; } ctx{ area, &wins };
    EnumWindows([](HWND h, LPARAM p) -> BOOL {
        auto* c = (Ctx*)p;
        if (!IsWindowVisible(h) || IsIconic(h) || Cloaked(h)) return TRUE;
        DWORD pid = 0; GetWindowThreadProcessId(h, &pid);
        if (pid == GetCurrentProcessId()) return TRUE;
        RECT r, x;
        if (!GetWindowRect(h, &r) || !IntersectRect(&x, &r, &c->area)) return TRUE;
        if (IsLinkHost(h)) c->out->push_back(h);
        return TRUE;
    }, (LPARAM)&ctx);
    if (wins.empty()) return {};
    auto sh = std::make_shared<Shared>();
    std::packaged_task<void()> task([sh, wins, area] { Search(sh, wins, area); });
    auto done = task.get_future();
    std::thread t(std::move(task));
    if (done.wait_for(std::chrono::milliseconds(budgetMs)) == std::future_status::ready) t.join();
    else { sh->cancel = true; t.detach(); Log(L"Links: search stopped after %u ms", budgetMs); }
    std::lock_guard<std::mutex> lk(sh->m);
    Log(L"Links: %zu link(s) in the captured area", sh->links.size());
    return sh->links;
}

} // namespace snap
