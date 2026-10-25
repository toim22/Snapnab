#include "Common.h"
#include <fstream>
#include <sstream>
#include <string_view>
#include <unordered_map>

namespace snap {

namespace {
struct ViewHash {
    using is_transparent = void;
    size_t operator()(std::wstring_view s) const { return std::hash<std::wstring_view>{}(s); }
};
std::unordered_map<std::wstring, std::wstring, ViewHash, std::equal_to<>> g_catalog;     // português → idioma em uso
std::wstring g_current = L"pt";
} // namespace

const wchar_t* Tr(const wchar_t* pt) {
    if (!pt || g_catalog.empty()) return pt;
    auto it = g_catalog.find(std::wstring_view(pt));
    return it != g_catalog.end() ? it->second.c_str() : pt;
}

std::wstring Tr(const std::wstring& pt) {
    if (g_catalog.empty()) return pt;
    auto it = g_catalog.find(std::wstring_view(pt));
    return it != g_catalog.end() ? it->second : pt;
}

namespace i18n {

const std::vector<Language>& Languages() {
    // os 12 idiomas mais falados (total de falantes), cada um com o próprio nome
    static const std::vector<Language> list = {
        { L"pt", L"Português" }, { L"en", L"English" }, { L"zh", L"中文（简体）" }, { L"hi", L"हिन्दी" },
        { L"es", L"Español" }, { L"ar", L"العربية" }, { L"fr", L"Français" }, { L"bn", L"বাংলা" },
        { L"ru", L"Русский" }, { L"id", L"Bahasa Indonesia" }, { L"ur", L"اردو" }, { L"de", L"Deutsch" },
    };
    return list;
}

static bool Known(const std::wstring& code) {
    for (auto& l : Languages()) if (code == l.code) return true;
    return false;
}

static std::wstring CatalogPath(const std::wstring& code) { return ExeDir() + L"\\lang\\" + code + L".po"; }
bool Available(const std::wstring& code) { return code == L"pt" || (Known(code) && FileExists(CatalogPath(code))); }

std::wstring Configured() {
    std::wstring c = RegGetString(L"Language", L"");
    return Known(c) ? c : L"";
}

void SetConfigured(const std::wstring& code) { RegSetString(L"Language", code); }

// idiomas de exibição do Windows em ordem de preferência ("pt-BR", "en-US"…)
static std::vector<std::wstring> WindowsLanguages() {
    std::vector<std::wstring> out;
    ULONG n = 0, cch = 0;
    if (GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &n, nullptr, &cch) && cch) {
        std::wstring buf(cch, L'\0');
        if (GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &n, buf.data(), &cch))
            for (const wchar_t* p = buf.c_str(); *p; p += wcslen(p) + 1) out.push_back(p);
    }
    wchar_t name[LOCALE_NAME_MAX_LENGTH];
    if (out.empty() && LCIDToLocaleName(MAKELCID(GetUserDefaultUILanguage(), SORT_DEFAULT), name, LOCALE_NAME_MAX_LENGTH, 0))
        out.push_back(name);
    return out;
}

std::wstring Resolve() {
    std::wstring c = Configured();
    if (!c.empty() && Available(c)) return c;
    for (auto& tag : WindowsLanguages()) {
        std::wstring primary = ToLower(tag.substr(0, tag.find(L'-')));
        if (Available(primary)) return primary;
    }
    return Available(L"en") ? L"en" : L"pt";
}

void Init() {
    g_current = Resolve();
    g_catalog.clear();
    if (g_current == L"pt") { Log(L"Idioma: pt (textos do próprio programa)"); return; }
    std::vector<std::pair<std::wstring, std::wstring>> entries;
    if (g_current != L"en") LoadPo(CatalogPath(L"en"), entries);      // o que faltar no catálogo fica em inglês
    bool ok = LoadPo(CatalogPath(g_current), entries);
    for (auto& e : entries) g_catalog[e.first] = e.second;
    if (Rtl())
        for (auto& [pt, text] : g_catalog) {
            if (text.find(L'|') != std::wstring::npos) continue;        // filtros de arquivo: os padrões ficam intactos
            std::wstring out;
            size_t start = 0;
            while (start <= text.size()) {
                size_t end = text.find(L'\n', start);
                std::wstring line = text.substr(start, end == std::wstring::npos ? std::wstring::npos : end - start);
                if (!line.empty()) out += L"\u202B" + line + L"\u202C";
                if (end == std::wstring::npos) break;
                out += L'\n';
                start = end + 1;
            }
            text = out;
        }
    Log(L"Idioma: %s (%s, %zu textos)", g_current.c_str(), ok ? L"catálogo carregado" : L"catálogo não abriu", g_catalog.size());
}

const std::wstring& Current() { return g_current; }
bool Rtl() { return g_current == L"ar" || g_current == L"ur"; }

const wchar_t* UiFontFace() {
    // a Segoe UI liga os ideogramas à Meiryo (desenho japonês) e não tem devanágari nem bengali
    if (g_current == L"zh") return L"Microsoft YaHei UI";
    if (g_current == L"hi" || g_current == L"bn") return L"Nirmala UI";
    return nullptr;
}

wchar_t AltKey(const std::wstring& name, wchar_t fallback) {
    // a barra de título compara a tecla virtual (A–Z); nomes em outros alfabetos ficam com a letra do português
    wchar_t c = name.empty() ? 0 : (wchar_t)towupper(name[0]);
    return c >= L'A' && c <= L'Z' ? c : fallback;
}

// ---------------------------------------------------------------- leitura do .po
static bool ParseQuoted(std::string_view line, std::string& out) {
    size_t a = line.find('"'), b = line.rfind('"');
    if (a == std::string_view::npos || b <= a) return false;
    for (size_t i = a + 1; i < b; i++) {
        char ch = line[i];
        if (ch != '\\' || i + 1 >= b) { out += ch; continue; }
        switch (line[++i]) {
        case 'n': out += '\n'; break;
        case 't': out += '\t'; break;
        case 'r': out += '\r'; break;
        case '"': out += '"'; break;
        case '\\': out += '\\'; break;
        default: out += '\\'; out += line[i]; break;
        }
    }
    return true;
}

bool LoadPo(const std::wstring& path, std::vector<std::pair<std::wstring, std::wstring>>& out) {
    std::ifstream f(path, std::ios::binary);
    if (!f) return false;
    std::stringstream ss; ss << f.rdbuf();
    std::string text = ss.str();
    if (text.compare(0, 3, "\xEF\xBB\xBF") == 0) text.erase(0, 3);
    enum Field { None, Ctxt, Id, Str, Skip } field = None;
    std::string ctxt, id, str;
    bool fuzzy = false;
    auto flush = [&] {
        if (!id.empty() && !str.empty() && !fuzzy)
            out.push_back({ Utf8ToW(ctxt.empty() ? id : ctxt + '\x04' + id), Utf8ToW(str) });
        ctxt.clear(); id.clear(); str.clear(); fuzzy = false; field = None;
    };
    std::istringstream lines(text);
    std::string raw;
    while (std::getline(lines, raw)) {
        std::string_view l(raw);
        while (!l.empty() && (l.back() == '\r' || l.back() == ' ' || l.back() == '\t')) l.remove_suffix(1);
        while (!l.empty() && (l.front() == ' ' || l.front() == '\t')) l.remove_prefix(1);
        if (l.empty()) { flush(); continue; }
        bool starts = l.front() != '"';
        if (starts && (field == Str || field == Skip) && l.rfind("msgstr", 0) != 0) flush();     // entrada seguinte sem linha em branco
        if (l.front() == '#') { if (l.rfind("#,", 0) == 0 && l.find("fuzzy") != std::string_view::npos) fuzzy = true; continue; }
        if (l.rfind("msgctxt", 0) == 0) { field = Ctxt; ParseQuoted(l, ctxt); }
        else if (l.rfind("msgid_plural", 0) == 0) field = Skip;
        else if (l.rfind("msgid", 0) == 0) { field = Id; ParseQuoted(l, id); }
        else if (l.rfind("msgstr[0]", 0) == 0 || (l.rfind("msgstr", 0) == 0 && l.rfind("msgstr[", 0) != 0)) { field = Str; ParseQuoted(l, str); }
        else if (l.rfind("msgstr[", 0) == 0) field = Skip;
        else if (l.front() == '"') {
            if (field == Ctxt) ParseQuoted(l, ctxt);
            else if (field == Id) ParseQuoted(l, id);
            else if (field == Str) ParseQuoted(l, str);
        }
    }
    flush();
    return true;
}

} // namespace i18n
} // namespace snap
