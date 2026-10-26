// Source formatting of clipboard text (paste keeping the formatting, ed_comp_4 §5.2 CInPlaceRichEditControl): the
// text objects have a single style, so "keeping the formatting" takes the font, size, color and bold/italic/underline
// of the first run of the copied text — from "Rich Text Format" (Word, WordPad) or "HTML Format" (browsers, VS Code)
#include "Document.h"

namespace snap::ed {

static std::string Trim(const std::string& s) {
    size_t a = s.find_first_not_of(" \t\r\n"), b = s.find_last_not_of(" \t\r\n");
    return a == std::string::npos ? std::string() : s.substr(a, b - a + 1);
}

// ---------------------------------------------------------------- RTF
bool ParseRtfFormat(const std::string& rtf, Style& out) {
    if (rtf.rfind("{\\rtf", 0) != 0) return false;
    enum Dest { Normal, FontTable, ColorTable, Skip };
    struct State { Dest dest = Normal; bool b = false, i = false, ul = false; int fs = 24, f = 0, cf = 0; };
    std::vector<State> stack{ State{} };
    std::map<int, std::string> fonts;
    std::vector<uint32_t> colors;
    int curFont = 0, red = -1, green = -1, blue = -1, deff = 0;
    std::string fontName;
    bool groupStart = false;
    size_t i = 0;
    while (i < rtf.size()) {
        char c = rtf[i];
        State& st = stack.back();
        if (c == '{') { stack.push_back(st); groupStart = true; i++; continue; }
        if (c == '}') {
            if (stack.back().dest == FontTable && stack.size() >= 2 && stack[stack.size() - 2].dest == FontTable && !fontName.empty()) {
                fonts[curFont] = fontName; fontName.clear();
            }
            if (stack.size() > 1) stack.pop_back();
            groupStart = false; i++;
            continue;
        }
        if (c == '\\') {
            i++;
            if (i >= rtf.size()) break;
            char n = rtf[i];
            if (n == '*') { st.dest = Skip; i++; continue; }
            if (n == '\'') { i += 3; if (st.dest == Normal) goto text; continue; }     // hex character: counts as text
            if (!isalpha((unsigned char)n)) {                                          // control symbol (\\ \{ \} \~ …)
                i++;
                if (st.dest == FontTable) fontName += n;
                else if (st.dest == Normal && (n == '\\' || n == '{' || n == '}' || n == '~')) goto text;
                continue;
            }
            std::string word;
            while (i < rtf.size() && isalpha((unsigned char)rtf[i])) word += rtf[i++];
            bool hasNum = false; int num = 0, sign = 1;
            if (i < rtf.size() && rtf[i] == '-') { sign = -1; i++; }
            while (i < rtf.size() && isdigit((unsigned char)rtf[i])) { num = num * 10 + (rtf[i++] - '0'); hasNum = true; }
            num *= sign;
            if (i < rtf.size() && rtf[i] == ' ') i++;
            bool first = groupStart;
            groupStart = false;
            if (word == "fonttbl") { st.dest = FontTable; continue; }
            if (word == "colortbl") { st.dest = ColorTable; continue; }
            if (first && (word == "stylesheet" || word == "info" || word == "pict" || word == "header" || word == "footer" ||
                          word == "listtable" || word == "listoverridetable" || word == "rsidtbl" || word == "generator" ||
                          word == "themedata" || word == "colorschememapping" || word == "latentstyles" || word == "datastore" ||
                          word == "xmlnstbl" || word == "mmathPr" || word == "fldinst")) { st.dest = Skip; continue; }
            if (st.dest == FontTable) { if (word == "f") curFont = num; continue; }
            if (st.dest == ColorTable) {
                if (word == "red") red = num; else if (word == "green") green = num; else if (word == "blue") blue = num;
                continue;
            }
            if (st.dest != Normal) continue;
            if (word == "deff") deff = num;
            else if (word == "plain") { st.b = st.i = st.ul = false; st.fs = 24; st.f = deff; st.cf = 0; }
            else if (word == "b") st.b = !hasNum || num != 0;
            else if (word == "i") st.i = !hasNum || num != 0;
            else if (word == "ul") st.ul = !hasNum || num != 0;
            else if (word == "ulnone") st.ul = false;
            else if (word == "fs" && hasNum) st.fs = num;
            else if (word == "f" && hasNum) st.f = num;
            else if (word == "cf" && hasNum) st.cf = num;
            else if (word == "par" || word == "line" || word == "tab") { /* not text yet */ }
            continue;
        }
        if (c == '\r' || c == '\n') { i++; continue; }
        if (st.dest == FontTable) {
            if (c == ';') { fonts[curFont] = fontName; fontName.clear(); }
            else fontName += c;
            i++;
            continue;
        }
        if (st.dest == ColorTable) {
            if (c == ';') { colors.push_back(red < 0 ? 0 : 0xFF000000u | (uint32_t)std::clamp(red, 0, 255) << 16 | (uint32_t)std::clamp(green, 0, 255) << 8 | (uint32_t)std::clamp(blue, 0, 255)); red = green = blue = -1; }
            i++;
            continue;
        }
        if (st.dest != Normal || c == ' ') { i++; continue; }
    text: {
            const State& f = stack.back();
            auto it = fonts.find(f.f);
            if (it == fonts.end()) it = fonts.find(deff);
            if (it != fonts.end()) {
                std::string name = Trim(it->second);
                if (!name.empty()) out.font = Utf8ToW(name);
            }
            out.fontSize = std::clamp(f.fs / 2.0f * 4 / 3, 4.0f, 400.0f);
            if (f.cf > 0 && f.cf < (int)colors.size() && colors[f.cf]) out.textColor = colors[f.cf];
            out.bold = f.b; out.italic = f.i; out.underline = f.ul;
            return true;
        }
    }
    return false;
}

// ---------------------------------------------------------------- HTML
static std::string Lower(std::string s) { for (auto& c : s) c = (char)tolower((unsigned char)c); return s; }

static bool ParseCssColor(std::string v, uint32_t& out) {
    v = Lower(Trim(v));
    if (v.rfind("#", 0) == 0) {
        std::string h = v.substr(1);
        if (h.size() == 3) h = { h[0], h[0], h[1], h[1], h[2], h[2] };
        if (h.size() < 6) return false;
        out = 0xFF000000u | (uint32_t)strtoul(h.substr(0, 6).c_str(), nullptr, 16);
        return true;
    }
    if (v.rfind("rgb", 0) == 0) {
        int r = 0, g = 0, b = 0;
        size_t p = v.find('(');
        if (p == std::string::npos || sscanf_s(v.c_str() + p + 1, "%d , %d , %d", &r, &g, &b) < 3) return false;
        out = 0xFF000000u | (uint32_t)std::clamp(r, 0, 255) << 16 | (uint32_t)std::clamp(g, 0, 255) << 8 | (uint32_t)std::clamp(b, 0, 255);
        return true;
    }
    static const std::map<std::string, uint32_t> names = { { "black", 0xFF000000 }, { "white", 0xFFFFFFFF }, { "red", 0xFFFF0000 },
        { "green", 0xFF008000 }, { "blue", 0xFF0000FF }, { "gray", 0xFF808080 }, { "grey", 0xFF808080 }, { "orange", 0xFFFFA500 },
        { "purple", 0xFF800080 }, { "yellow", 0xFFFFFF00 } };
    auto it = names.find(v);
    if (it == names.end()) return false;
    out = it->second;
    return true;
}

static void ApplyCss(const std::string& css, Style& s, bool& any) {
    size_t i = 0;
    while (i < css.size()) {
        size_t semi = css.find(';', i);
        std::string decl = css.substr(i, semi == std::string::npos ? std::string::npos : semi - i);
        i = semi == std::string::npos ? css.size() : semi + 1;
        size_t colon = decl.find(':');
        if (colon == std::string::npos) continue;
        std::string k = Lower(Trim(decl.substr(0, colon))), v = Trim(decl.substr(colon + 1));
        if (k == "font-family") {
            std::string fam = v.substr(0, v.find(','));
            fam.erase(std::remove(fam.begin(), fam.end(), '\''), fam.end());
            fam.erase(std::remove(fam.begin(), fam.end(), '"'), fam.end());
            fam = Trim(fam);
            static const char* generic[] = { "monospace", "serif", "sans-serif", "cursive", "fantasy", "system-ui" };
            if (!fam.empty() && std::none_of(std::begin(generic), std::end(generic), [&](const char* g) { return Lower(fam) == g; })) { s.font = Utf8ToW(fam); any = true; }
        } else if (k == "font-size") {
            float n = (float)atof(v.c_str());
            std::string lv = Lower(v);
            if (n > 0) { s.fontSize = std::clamp(lv.find("pt") != std::string::npos ? n * 4 / 3 : n, 4.0f, 400.0f); any = true; }
        } else if (k == "color") { if (ParseCssColor(v, s.textColor)) any = true; }
        else if (k == "font-weight") { std::string lv = Lower(v); s.bold = lv == "bold" || lv == "bolder" || atoi(lv.c_str()) >= 600; any = true; }
        else if (k == "font-style") { s.italic = Lower(v).find("italic") != std::string::npos; any = true; }
        else if (k == "text-decoration" || k == "text-decoration-line") { s.underline = Lower(v).find("underline") != std::string::npos; any = true; }
    }
}

static std::string Attr(const std::string& tag, const char* name) {
    std::string lt = Lower(tag);
    size_t p = 0;
    std::string key = std::string(name) + "=";
    while ((p = lt.find(key, p)) != std::string::npos) {
        if (p > 0 && !isspace((unsigned char)lt[p - 1])) { p++; continue; }
        size_t v = p + key.size();
        if (v >= tag.size()) return "";
        char q = tag[v];
        if (q == '"' || q == '\'') { size_t e = tag.find(q, v + 1); return tag.substr(v + 1, e == std::string::npos ? std::string::npos : e - v - 1); }
        size_t e = v; while (e < tag.size() && !isspace((unsigned char)tag[e]) && tag[e] != '>') e++;
        return tag.substr(v, e - v);
    }
    return "";
}

bool ParseHtmlFormat(const std::string& html, Style& out) {
    size_t a = html.find("<!--StartFragment-->"), b = html.find("<!--EndFragment-->");
    std::string frag = a != std::string::npos ? html.substr(a + 20, b != std::string::npos && b > a ? b - a - 20 : std::string::npos) : html;
    // open tags up to the first visible text; the innermost ones win
    std::vector<std::string> open;
    size_t i = 0;
    while (i < frag.size()) {
        if (frag[i] == '<') {
            size_t e = frag.find('>', i);
            if (e == std::string::npos) break;
            std::string tag = frag.substr(i + 1, e - i - 1);
            i = e + 1;
            if (tag.empty() || tag[0] == '!' || tag[0] == '?') continue;
            if (tag[0] == '/') { if (!open.empty()) open.pop_back(); continue; }
            std::string name = Lower(tag.substr(0, tag.find_first_of(" \t\r\n/")));
            if (name == "br" || name == "img" || name == "meta" || name == "hr" || tag.back() == '/') continue;
            if (name == "style" || name == "script") { size_t c = Lower(frag).find("</" + name, i); i = c == std::string::npos ? frag.size() : c; continue; }
            open.push_back(tag);
            continue;
        }
        if (isspace((unsigned char)frag[i])) { i++; continue; }
        Style s = out;
        bool any = false;
        for (auto& tag : open) {
            std::string name = Lower(tag.substr(0, tag.find_first_of(" \t\r\n")));
            if (name == "b" || name == "strong") { s.bold = true; any = true; }
            if (name == "i" || name == "em") { s.italic = true; any = true; }
            if (name == "u") { s.underline = true; any = true; }
            if (name == "font") {
                std::string face = Attr(tag, "face"), col = Attr(tag, "color");
                if (!face.empty()) { s.font = Utf8ToW(Trim(face.substr(0, face.find(',')))); any = true; }
                if (!col.empty() && ParseCssColor(col, s.textColor)) any = true;
            }
            std::string css = Attr(tag, "style");
            if (!css.empty()) ApplyCss(css, s, any);
        }
        if (any) out = s;
        return any;
    }
    return false;
}

} // namespace snap::ed
