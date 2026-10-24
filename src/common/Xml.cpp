#include "Xml.h"
#include "Common.h"
#include <cstdlib>
#include <cstdio>

namespace snap {

const std::string* XmlNode::attr(const std::string& k) const {
    for (auto& a : attrs) if (a.first == k) return &a.second;
    return nullptr;
}
std::string XmlNode::get(const std::string& k, const std::string& def) const { auto* a = attr(k); return a ? *a : def; }
std::wstring XmlNode::getW(const std::string& k, const std::wstring& def) const { auto* a = attr(k); return a ? Utf8ToW(*a) : def; }
int XmlNode::geti(const std::string& k, int def) const { auto* a = attr(k); return a ? atoi(a->c_str()) : def; }
double XmlNode::getd(const std::string& k, double def) const { auto* a = attr(k); return a ? atof(a->c_str()) : def; }

XmlNode& XmlNode::set(const std::string& k, const std::string& v) {
    for (auto& a : attrs) if (a.first == k) { a.second = v; return *this; }
    attrs.emplace_back(k, v);
    return *this;
}
XmlNode& XmlNode::setW(const std::string& k, const std::wstring& v) { return set(k, WToUtf8(v)); }
XmlNode& XmlNode::seti(const std::string& k, long long v) { return set(k, std::to_string(v)); }
XmlNode& XmlNode::setd(const std::string& k, double v) { char b[64]; snprintf(b, sizeof b, "%.4f", v); return set(k, b); }
XmlNode& XmlNode::add(const std::string& n) { children.emplace_back(n); return children.back(); }
const XmlNode* XmlNode::child(const std::string& n) const { for (auto& c : children) if (c.name == n) return &c; return nullptr; }
XmlNode* XmlNode::child(const std::string& n) { for (auto& c : children) if (c.name == n) return &c; return nullptr; }

static void Escape(std::string& out, const std::string& s) {
    for (char c : s) {
        switch (c) {
        case '&': out += "&amp;"; break;
        case '<': out += "&lt;"; break;
        case '>': out += "&gt;"; break;
        case '"': out += "&quot;"; break;
        case '\'': out += "&apos;"; break;
        case '\n': out += "&#10;"; break;
        case '\r': out += "&#13;"; break;
        case '\t': out += "&#9;"; break;
        default: out += c;
        }
    }
}

static void Write(std::string& out, const XmlNode& n, int depth) {
    out.append(depth * 2, ' ');
    out += '<'; out += n.name;
    for (auto& a : n.attrs) { out += ' '; out += a.first; out += "=\""; Escape(out, a.second); out += '"'; }
    if (n.children.empty() && n.text.empty()) { out += "/>\n"; return; }
    out += '>';
    if (!n.text.empty()) Escape(out, n.text);
    if (!n.children.empty()) {
        out += '\n';
        for (auto& c : n.children) Write(out, c, depth + 1);
        out.append(depth * 2, ' ');
    }
    out += "</"; out += n.name; out += ">\n";
}

std::string XmlSerialize(const XmlNode& root, bool decl) {
    std::string out;
    if (decl) out = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
    Write(out, root, 0);
    return out;
}

namespace {
struct Parser {
    const std::string& s; size_t i = 0;
    explicit Parser(const std::string& t) : s(t) {}
    bool eof() const { return i >= s.size(); }
    void ws() { while (!eof() && (unsigned char)s[i] <= ' ') i++; }
    bool starts(const char* p) const { return s.compare(i, strlen(p), p) == 0; }
    std::string unescape(const std::string& v) {
        std::string r;
        for (size_t k = 0; k < v.size(); k++) {
            if (v[k] != '&') { r += v[k]; continue; }
            size_t e = v.find(';', k);
            if (e == std::string::npos) { r += v[k]; continue; }
            std::string ent = v.substr(k + 1, e - k - 1);
            if (ent == "amp") r += '&'; else if (ent == "lt") r += '<'; else if (ent == "gt") r += '>';
            else if (ent == "quot") r += '"'; else if (ent == "apos") r += '\'';
            else if (!ent.empty() && ent[0] == '#') {
                unsigned long cp = ent.size() > 1 && (ent[1] == 'x' || ent[1] == 'X') ? strtoul(ent.c_str() + 2, nullptr, 16)
                                                                                     : strtoul(ent.c_str() + 1, nullptr, 10);
                std::wstring w; if (cp < 0x10000) w += (wchar_t)cp; else { cp -= 0x10000; w += (wchar_t)(0xD800 + (cp >> 10)); w += (wchar_t)(0xDC00 + (cp & 0x3FF)); }
                r += WToUtf8(w);
            } else { r += '&'; r += ent; r += ';'; }
            k = e;
        }
        return r;
    }
    void skipMisc() {
        for (;;) {
            ws();
            if (starts("<?")) { size_t e = s.find("?>", i); i = e == std::string::npos ? s.size() : e + 2; }
            else if (starts("<!--")) { size_t e = s.find("-->", i); i = e == std::string::npos ? s.size() : e + 3; }
            else if (starts("<!")) { size_t e = s.find('>', i); i = e == std::string::npos ? s.size() : e + 1; }
            else break;
        }
    }
    std::string name() {
        size_t b = i;
        while (!eof() && !strchr(" \t\r\n/>=", s[i])) i++;
        return s.substr(b, i - b);
    }
    bool element(XmlNode& n) {
        skipMisc();
        if (eof() || s[i] != '<') return false;
        i++;
        n.name = name();
        for (;;) {
            ws();
            if (eof()) return false;
            if (s[i] == '/') { i += 2; return true; }
            if (s[i] == '>') { i++; break; }
            std::string k = name();
            ws(); if (eof() || s[i] != '=') return false; i++; ws();
            char q = s[i++];
            size_t e = s.find(q, i);
            if (e == std::string::npos) return false;
            n.attrs.emplace_back(k, unescape(s.substr(i, e - i)));
            i = e + 1;
        }
        for (;;) {
            size_t b = i;
            while (!eof() && s[i] != '<') i++;
            std::string t = s.substr(b, i - b);
            if (t.find_first_not_of(" \t\r\n") != std::string::npos) n.text += unescape(t);
            if (eof()) return false;
            if (starts("</")) { size_t e = s.find('>', i); i = e == std::string::npos ? s.size() : e + 1; return true; }
            if (starts("<!--") || starts("<?")) { skipMisc(); continue; }
            if (starts("<![CDATA[")) { size_t e = s.find("]]>", i); n.text += s.substr(i + 9, e - i - 9); i = e + 3; continue; }
            XmlNode c;
            if (!element(c)) return false;
            n.children.push_back(std::move(c));
        }
    }
};
}

bool XmlParse(const std::string& text, XmlNode& root) {
    Parser p(text);
    if (text.size() >= 3 && (unsigned char)text[0] == 0xEF) p.i = 3;   // BOM
    root = XmlNode();
    return p.element(root);
}

bool XmlLoadFile(const std::wstring& path, XmlNode& root) {
    HANDLE h = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr);
    if (h == INVALID_HANDLE_VALUE) return false;
    DWORD sz = GetFileSize(h, nullptr), rd = 0;
    std::string buf(sz, '\0');
    ReadFile(h, buf.data(), sz, &rd, nullptr);
    CloseHandle(h);
    return XmlParse(buf, root);
}

bool XmlSaveFile(const std::wstring& path, const XmlNode& root) {
    std::string s = XmlSerialize(root);
    HANDLE h = CreateFileW(path.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, 0, nullptr);
    if (h == INVALID_HANDLE_VALUE) return false;
    DWORD wr; WriteFile(h, s.data(), (DWORD)s.size(), &wr, nullptr);
    CloseHandle(h);
    return wr == s.size();
}

} // namespace snap
