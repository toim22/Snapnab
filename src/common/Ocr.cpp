// Grab Text (OCR) — the original uses ABBYY FineReader Engine (FREngine.dll, ds_arch §13); here the engine is the
// native Windows OCR (Windows.Media.Ocr), called on a dedicated MTA thread. Used by the Editor and by the
// capture window's "Capture text" preset.
#include "Ocr.h"
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Graphics.Imaging.h>
#include <winrt/Windows.Media.Ocr.h>
#include <winrt/Windows.Storage.Streams.h>

namespace snap {

bool RunOcr(const Image& imgIn, std::wstring& text, std::wstring& err) {
    if (imgIn.empty()) { err = Tr(L"Imagem vazia."); return false; }
    // opaque background and scaling: small text is enlarged, huge images are reduced
    Image img(imgIn.w, imgIn.h, 0xFFFFFFFF);
    img.Blit(imgIn, 0, 0, true);
    if (img.h < 500 && img.w < 3000) img = ScaleImage(img, img.w * 2, img.h * 2);
    std::thread t([&] {
        winrt::init_apartment(winrt::apartment_type::multi_threaded);
        try {
            using namespace winrt::Windows::Graphics::Imaging;
            using namespace winrt::Windows::Media::Ocr;
            uint32_t maxDim = OcrEngine::MaxImageDimension();
            Image work = img;
            if ((uint32_t)work.w > maxDim || (uint32_t)work.h > maxDim) {
                double s = std::min((double)maxDim / work.w, (double)maxDim / work.h);
                work = ScaleImage(work, (int)(work.w * s), (int)(work.h * s));
            }
            SoftwareBitmap sb(BitmapPixelFormat::Bgra8, work.w, work.h, BitmapAlphaMode::Premultiplied);
            winrt::Windows::Storage::Streams::Buffer buf((uint32_t)work.px.size() * 4);
            memcpy(buf.data(), work.px.data(), work.px.size() * 4);
            buf.Length((uint32_t)work.px.size() * 4);
            sb.CopyFromBuffer(buf);
            OcrEngine engine = OcrEngine::TryCreateFromUserProfileLanguages();
            if (!engine) { err = Tr(L"Nenhum idioma de OCR instalado no Windows (Configurações › Hora e idioma › Idioma)."); return; }
            OcrResult res = engine.RecognizeAsync(sb).get();
            std::wstring out;
            for (auto const& line : res.Lines()) { out += line.Text().c_str(); out += L"\n"; }
            text = out;
        } catch (winrt::hresult_error const& e) {
            err = e.message().c_str();
        } catch (...) {
            err = Tr(L"Falha desconhecida no OCR.");
        }
        winrt::uninit_apartment();
    });
    t.join();
    return err.empty();
}

} // namespace snap
