// Idioma da interface — equivalente da SnagItEditorRes.dll escolhida pela pasta do idioma (ed_comp_1 §3, passo 5):
// no início cada programa carrega lang\<código>.po (formato gettext) da pasta do executável; sem o catálogo do idioma
// pedido fica o inglês e, sem ele, o português do próprio código.
//
// Os textos do código são o português do Brasil; Tr(L"...") devolve a tradução do idioma em uso. Tabelas estáticas
// (inicializadas antes do catálogo) marcam os textos com TR_NOOP e chamam Tr() onde eles aparecem.
// scripts/i18n.py extrai os textos para lang/snapnab.pot e confere os catálogos.
#pragma once
#include <string>
#include <vector>

#define TR_NOOP(s) s

namespace snap {

const wchar_t* Tr(const wchar_t* pt);
std::wstring   Tr(const std::wstring& pt);

namespace i18n {

struct Language {
    const wchar_t* code;        // "en", "zh"… (subtag principal do BCP 47)
    const wchar_t* name;        // nome no próprio idioma
};

const std::vector<Language>& Languages();   // os 12 idiomas, o português primeiro
bool Available(const std::wstring& code);    // o português sempre; os outros se lang\<código>.po existe
std::wstring Configured();                   // escolhido nas Opções; "" = o do Windows
void SetConfigured(const std::wstring& code);
std::wstring Resolve();                      // o que Init() vai usar: o escolhido ou o do Windows, se houver catálogo
void Init();                                 // carrega o catálogo (chamar no início do WinMain)
const std::wstring& Current();               // idioma em uso
bool Rtl();                                  // árabe e urdu: leitura da direita para a esquerda
const wchar_t* UiFontFace();                 // fonte da interface com os caracteres do idioma
wchar_t AltKey(const std::wstring& name, wchar_t fallback);   // letra do Alt de um menu: a inicial latina do nome traduzido

// Lê um catálogo .po (só as entradas traduzidas e sem "fuzzy"); devolve false se o arquivo não abre
bool LoadPo(const std::wstring& path, std::vector<std::pair<std::wstring, std::wstring>>& out);

} // namespace i18n
} // namespace snap
