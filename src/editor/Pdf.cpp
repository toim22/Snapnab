// PDF export (≙ CPaintDocument::SaveAs → PDF::PDFPage::SavePDFDocument, ds_pseudocode §10.1)
//   A4 or Letter page according to the measurement system (USSystem), orientation from the image shape,
//   margin 0, image centered and fitted (ImageLayout 0); image written as JPEG (DCTDecode)
//   Links captured with the image (UI Automation) become /Link annotations with a /URI action
#include "Document.h"

namespace snap::ed {

// URI as a PDF literal string: ASCII, with the characters outside it percent-encoded and ( ) \ escaped
static std::string PdfUri(const std::wstring& url) {
    std::string u = WToUtf8(url), out;
    for (unsigned char c : u) {
        if (c < 0x21 || c > 0x7E) out += FormatA("%%%02X", c);
        else if (c == '(' || c == ')' || c == '\\') { out += '\\'; out += (char)c; }
        else out += (char)c;
    }
    return out;
}

bool SavePdf(const std::wstring& path, const Image& imgIn, std::wstring& err, const std::vector<PdfLink>& links) {
    if (imgIn.empty()) { err = Tr(L"Imagem vazia."); return false; }
    std::vector<BYTE> jpg;
    if (!EncodeToMemory(imgIn, ImageFormat::JPEG, jpg, 92)) { err = Tr(L"Falha ao codificar a imagem."); return false; }

    DWORD measure = 0;
    GetLocaleInfoEx(LOCALE_NAME_USER_DEFAULT, LOCALE_IMEASURE | LOCALE_RETURN_NUMBER, (LPWSTR)&measure, sizeof(measure) / 2);
    bool us = measure == 1;
    double pw = us ? 612 : 595.28, ph = us ? 792 : 841.89;           // Letter : A4 (points)
    if (imgIn.w > imgIn.h) std::swap(pw, ph);                         // landscape
    double s = std::min(pw / imgIn.w, ph / imgIn.h);
    double iw = imgIn.w * s, ih = imgIn.h * s;
    double ix = (pw - iw) / 2, iy = (ph - ih) / 2;

    std::string out = "%PDF-1.4\n%\xE2\xE3\xCF\xD3\n";
    std::vector<size_t> offs;
    auto obj = [&](const std::string& body) { offs.push_back(out.size()); out += FormatA("%zu 0 obj\n", offs.size()) + body + "\nendobj\n"; };
    // link annotations (objects 6…): only the ones inside the image and with a web or e-mail address
    std::vector<PdfLink> valid;
    for (auto& l : links) {
        bool web = l.url.rfind(L"http://", 0) == 0 || l.url.rfind(L"https://", 0) == 0 || l.url.rfind(L"mailto:", 0) == 0;
        Gdiplus::RectF r = l.r;
        Gdiplus::RectF::Intersect(r, r, Gdiplus::RectF(0, 0, (float)imgIn.w, (float)imgIn.h));
        if (web && r.Width >= 1 && r.Height >= 1) valid.push_back({ r, l.url });
    }
    std::string annots;
    for (size_t i = 0; i < valid.size(); i++) annots += FormatA("%s%zu 0 R", i ? " " : "", 6 + i);
    obj("<< /Type /Catalog /Pages 2 0 R >>");
    obj("<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    obj(FormatA("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %.2f %.2f] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R%s >>", pw, ph,
                annots.empty() ? "" : (" /Annots [" + annots + "]").c_str()));
    offs.push_back(out.size());
    out += FormatA("4 0 obj\n<< /Type /XObject /Subtype /Image /Width %d /Height %d /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length %zu >>\nstream\n",
                   imgIn.w, imgIn.h, jpg.size());
    out.append((const char*)jpg.data(), jpg.size());
    out += "\nendstream\nendobj\n";
    std::string content = FormatA("q\n%.3f 0 0 %.3f %.3f %.3f cm\n/Im0 Do\nQ\n", iw, ih, ix, iy);
    obj(FormatA("<< /Length %zu >>\nstream\n", content.size()) + content + "endstream");
    for (auto& l : valid) {                     // image pixels → page points (origin at the bottom left)
        double x0 = ix + l.r.X * s, x1 = ix + l.r.GetRight() * s, y1 = iy + (imgIn.h - l.r.Y) * s, y0 = iy + (imgIn.h - l.r.GetBottom()) * s;
        obj(FormatA("<< /Type /Annot /Subtype /Link /Rect [%.2f %.2f %.2f %.2f] /Border [0 0 0] /A << /S /URI /URI (%s) >> >>", x0, y0, x1, y1, PdfUri(l.url).c_str()));
    }
    std::string title = WToUtf8(FileNameOf(path));
    for (auto& c : title) if (c == '(' || c == ')' || c == '\\') c = '_';
    obj("<< /Producer (Snapnab) /Title (" + title + ") >>");
    size_t xref = out.size();
    out += FormatA("xref\n0 %zu\n0000000000 65535 f \n", offs.size() + 1);
    for (size_t o : offs) out += FormatA("%010zu 00000 n \n", o);
    out += FormatA("trailer\n<< /Size %zu /Root 1 0 R /Info %zu 0 R >>\nstartxref\n%zu\n%%%%EOF\n", offs.size() + 1, offs.size(), xref);

    HANDLE h = CreateFileW(path.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h == INVALID_HANDLE_VALUE) { err = Tr(L"Não foi possível criar o arquivo."); return false; }
    DWORD wr = 0;
    BOOL ok = WriteFile(h, out.data(), (DWORD)out.size(), &wr, nullptr);
    CloseHandle(h);
    if (!ok || wr != out.size()) { err = Tr(L"Falha ao gravar o arquivo."); return false; }
    return true;
}

} // namespace snap::ed
