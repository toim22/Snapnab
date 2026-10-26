// Interface language — equivalent of the SnagItEditorRes.dll chosen by the language folder (ed_comp_1 §3, step 5):
// at startup each program loads lang\<code>.po (gettext format) from the executable's folder; without the catalog of
// the requested language it falls back to English and, without that, to the Portuguese in the code itself.
//
// The strings in the code are Brazilian Portuguese; Tr(L"...") returns the translation for the current language. Static
// tables (initialized before the catalog) mark their strings with TR_NOOP and call Tr() where they are shown.
// scripts/i18n.py extracts the strings to lang/snapnab.pot and checks the catalogs.
#pragma once
#include <string>
#include <vector>

#define TR_NOOP(s) s

namespace snap {

const wchar_t* Tr(const wchar_t* pt);
std::wstring   Tr(const std::wstring& pt);

namespace i18n {

struct Language {
    const wchar_t* code;        // "en", "zh"… (primary BCP 47 subtag)
    const wchar_t* name;        // name in the language itself
};

const std::vector<Language>& Languages();   // the 12 languages, Portuguese first
bool Available(const std::wstring& code);    // Portuguese always; the others if lang\<code>.po exists
std::wstring Configured();                   // chosen in Options; "" = the Windows language
void SetConfigured(const std::wstring& code);
std::wstring Resolve();                      // what Init() will use: the chosen one or the Windows one, if there is a catalog
void Init();                                 // loads the catalog (call at the start of WinMain)
const std::wstring& Current();               // language in use
bool Rtl();                                  // Arabic and Urdu: right-to-left reading
const wchar_t* UiFontFace();                 // interface font with the language's characters
wchar_t AltKey(const std::wstring& name, wchar_t fallback);   // Alt letter of a menu: the Latin initial of the translated name

// Reads a .po catalog (only translated, non-"fuzzy" entries); returns false if the file does not open
bool LoadPo(const std::wstring& path, std::vector<std::pair<std::wstring, std::wstring>>& out);

} // namespace i18n
} // namespace snap
