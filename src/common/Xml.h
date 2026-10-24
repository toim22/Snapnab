// XML mínimo (papel do libxml2/MSXML no original): presets, índice do .snap e objetos do documento
#pragma once
#include <string>
#include <vector>
#include <utility>

namespace snap {

struct XmlNode {
    std::string name;
    std::vector<std::pair<std::string, std::string>> attrs;
    std::vector<XmlNode> children;
    std::string text;

    XmlNode() = default;
    explicit XmlNode(std::string n) : name(std::move(n)) {}

    const std::string* attr(const std::string& k) const;
    std::string get(const std::string& k, const std::string& def = "") const;
    std::wstring getW(const std::string& k, const std::wstring& def = L"") const;
    int    geti(const std::string& k, int def = 0) const;
    double getd(const std::string& k, double def = 0) const;
    bool   getb(const std::string& k, bool def = false) const { return geti(k, def ? 1 : 0) != 0; }

    XmlNode& set(const std::string& k, const std::string& v);
    XmlNode& setW(const std::string& k, const std::wstring& v);
    XmlNode& seti(const std::string& k, long long v);
    XmlNode& setd(const std::string& k, double v);

    XmlNode& add(const std::string& childName);
    const XmlNode* child(const std::string& n) const;
    XmlNode* child(const std::string& n);
};

std::string XmlSerialize(const XmlNode& root, bool declaration = true);
bool XmlParse(const std::string& text, XmlNode& root);
bool XmlLoadFile(const std::wstring& path, XmlNode& root);
bool XmlSaveFile(const std::wstring& path, const XmlNode& root);

} // namespace snap
