#include "Stamps.h"
#include <shobjidl.h>

namespace snap::ed {

std::wstring MyStampsDir() {
    std::wstring d = AppDataDir() + L"\\Stamps";
    EnsureDir(d);
    return d;
}

// categorias de fábrica (as mesmas da galeria original: Atividades, Negócios, Comida e bebida, Feriados, Interface,
// Smileys e pessoas, Adesivos, Símbolos, Viagem e lugares); o nome de cada carimbo serve para a busca e é traduzido
// um a um (scripts/i18n.py extrai os nomes destas listas)
static const struct { const wchar_t* cat; const wchar_t* list; } kStock[] = {
    { TR_NOOP(L"Negócios"), L"📎 clipe anexo|🔍 lupa pesquisar|⚙️ engrenagem configurações|🕐 relógio hora|👤 usuário pessoa|👥 equipe pessoas|"
                   L"🔒 cadeado fechado segurança|🔓 cadeado aberto|🔖 marcador|📶 sinal wi-fi|💻 notebook código|⬇️ baixar download|"
                   L"⬆️ enviar upload|🔀 mesclar|🔁 repetir|📊 gráfico barras|📈 crescimento|📉 queda|✅ lista concluído|🔗 link compartilhar|"
                   L"▶️ reproduzir play|⏹️ parar|⏸️ pausar|⏩ avançar|⏪ voltar|💰 dinheiro moedas|💳 cartão pagamento|🌐 globo internet|"
                   L"🏅 prêmio medalha|🖊️ caneta|✏️ lápis|📄 documento|📝 nota|📞 telefone|✉️ e-mail carta|💬 conversa comentário|"
                   L"🖥️ monitor computador|📱 celular|🖱️ mouse|☁️ nuvem|📁 pasta|📅 calendário|💼 maleta trabalho|📌 alfinete|"
                   L"🗑️ lixeira|🔔 sino notificação|💡 ideia lâmpada|🎯 alvo meta|🚀 foguete lançamento|🏆 troféu" },
    { TR_NOOP(L"Interface"), L"✅ concluído certo|❌ errado fechar|⚠️ atenção aviso|ℹ️ informação|❓ pergunta dúvida|❗ exclamação importante|"
                    L"⭐ estrela favorito|❤️ coração|👍 positivo curtir|👎 negativo|➡️ seta direita|⬅️ seta esquerda|⬆️ seta cima|"
                    L"⬇️ seta baixo|↗️ seta diagonal|🔄 atualizar|➕ adicionar mais|➖ remover menos|✔️ marca certo|✖️ multiplicar fechar|"
                    L"🔴 vermelho|🟠 laranja|🟡 amarelo|🟢 verde|🔵 azul|🟣 roxo|⚫ preto|⚪ branco|🟥 quadrado vermelho|"
                    L"🟩 quadrado verde|🟦 quadrado azul|🔘 botão rádio|🔲 caixa|🔳 caixa marcada|🔊 som alto|🔇 sem som mudo|"
                    L"🏠 início casa|🔑 chave senha|🛒 carrinho compras|📥 caixa de entrada|📤 caixa de saída|🔎 zoom pesquisa" },
    { TR_NOOP(L"Atividades"), L"⚽ futebol|🏀 basquete|🏈 futebol americano|⚾ beisebol|🎾 tênis|🏐 vôlei|🏓 pingue-pongue|🏸 badminton|"
                     L"🥊 boxe|🎳 boliche|⛳ golfe|🎣 pesca|🎿 esqui|🏂 snowboard|🏄 surfe|🚴 ciclismo bicicleta|🏊 natação|"
                     L"🏋️ musculação|🎮 videogame jogo|🎲 dado|🧩 quebra-cabeça|🎨 pintura arte|🎭 teatro|🎤 microfone|🎧 fone|"
                     L"🎸 guitarra|🎹 piano|🥁 bateria|🎬 cinema filme|🥇 primeiro lugar|🎟️ ingresso|📸 foto câmera" },
    { TR_NOOP(L"Comida e bebida"), L"🍎 maçã|🍌 banana|🍇 uva|🍓 morango|🍉 melancia|🍒 cereja|🍑 pêssego|🍍 abacaxi|🥑 abacate|🥕 cenoura|"
                          L"🌽 milho|🍞 pão|🧀 queijo|🍔 hambúrguer|🍟 batata frita|🍕 pizza|🌭 cachorro-quente|🌮 taco|🍣 sushi|"
                          L"🍜 lámen sopa|🍩 rosquinha|🍪 biscoito|🎂 bolo aniversário|🍰 torta|🍫 chocolate|🍿 pipoca|☕ café|"
                          L"🍵 chá|🥤 refrigerante|🍺 cerveja|🍷 vinho|🥂 brinde" },
    { TR_NOOP(L"Feriados"), L"🎄 natal árvore|🎅 papai noel|🎁 presente|🎃 abóbora halloween|👻 fantasma|🦃 peru|🐰 coelho páscoa|🥚 ovo|"
                   L"💝 coração presente|💐 flores|🎆 fogos|🎇 estrelinha|🎉 festa|🎊 confete|🎈 balão|🕯️ vela|🌟 estrela brilhante|"
                   L"❄️ neve floco|☃️ boneco de neve|🎀 laço|🧧 envelope|🍀 trevo sorte|🪅 piñata|🕎 menorá" },
    { TR_NOOP(L"Smileys e pessoas"), L"😀 sorriso feliz|😃 alegre|😄 rindo|😁 sorrisão|😆 gargalhada|😅 aliviado|😂 chorando de rir|🙂 sorriso leve|"
                            L"😉 piscada|😊 contente|😍 apaixonado|😘 beijo|😎 legal óculos|🤔 pensando|😐 neutro|😴 dormindo|"
                            L"😮 surpreso|😲 espantado|😢 triste|😭 chorando|😡 bravo|🤯 cabeça explodindo|🥳 comemorando|🤩 encantado|"
                            L"😇 anjo|🙃 de cabeça para baixo|🤗 abraço|🙄 revirando olhos|😬 careta|👋 olá tchau|👏 palmas|🙌 viva|"
                            L"👌 ok|✌️ paz|🤞 dedos cruzados|👉 apontando direita|👈 apontando esquerda|👆 apontando cima|"
                            L"👇 apontando baixo|💪 força|🙏 por favor obrigado|🤝 acordo aperto de mão" },
    { TR_NOOP(L"Adesivos"), L"💯 cem pontos|🔥 fogo|✨ brilho|💥 explosão|💫 tontura|💢 raiva|💤 sono|💦 gotas|🆕 novo|🆗 ok|🆒 legal|"
                   L"🆓 grátis|🆙 up|🔝 topo|🔜 em breve|🌈 arco-íris|👑 coroa|💎 diamante|🚩 bandeira|🏁 chegada|📢 megafone|"
                   L"📣 anúncio|🏷️ etiqueta|🎖️ medalha|🧲 ímã|🔮 bola de cristal|🪄 varinha mágica|🧨 bomba|🍀 trevo" },
    { TR_NOOP(L"Símbolos"), L"↔️ seta dupla horizontal|↕️ seta dupla vertical|🔃 girar|➗ dividir|♾️ infinito|‼️ dupla exclamação|"
                   L"⁉️ exclamação pergunta|🚫 proibido|⛔ entrada proibida|☑️ caixa marcada|⭕ círculo|🔶 losango laranja|"
                   L"🔷 losango azul|♻️ reciclar|⚡ raio energia|☢️ radioativo|☣️ risco biológico|©️ copyright|®️ marca registrada|"
                   L"™️ marca|#️⃣ hashtag|1️⃣ um|2️⃣ dois|3️⃣ três|4️⃣ quatro|5️⃣ cinco|6️⃣ seis|7️⃣ sete|8️⃣ oito|9️⃣ nove|🔟 dez|"
                   L"🅰️ letra a|🅱️ letra b|🆎 ab|🔤 abc|💲 cifrão|💱 câmbio" },
    { TR_NOOP(L"Viagem e lugares"), L"🚗 carro|🚕 táxi|🚌 ônibus|🚑 ambulância|🚒 bombeiros|🚓 polícia|🚲 bicicleta|🛵 moto|🚂 trem|"
                           L"✈️ avião|🛳️ navio|⛵ veleiro|🚁 helicóptero|🗺️ mapa|🧭 bússola|🏔️ montanha|🏖️ praia|🏝️ ilha|"
                           L"🏕️ acampamento|🏙️ cidade|🏢 prédio escritório|🏥 hospital|🏦 banco|🏨 hotel|🏫 escola|⛪ igreja|"
                           L"🗽 estátua|🗼 torre|🌍 terra mundo|⛽ posto combustível|🚦 semáforo|🅿️ estacionamento|🧳 mala|📍 local pino" },
};

static std::vector<StampItem> ParseList(const wchar_t* list) {
    std::vector<StampItem> v;
    std::wstring s = list;
    size_t i = 0;
    while (i < s.size()) {
        size_t j = s.find(L'|', i);
        std::wstring item = s.substr(i, j == std::wstring::npos ? std::wstring::npos : j - i);
        size_t sp = item.find(L' ');
        if (sp != std::wstring::npos) v.push_back({ item.substr(0, sp), Tr(item.substr(sp + 1)) });
        if (j == std::wstring::npos) break;
        i = j + 1;
    }
    return v;
}

static void ListImages(const std::wstring& dir, std::vector<StampItem>& out) {
    WIN32_FIND_DATAW fd;
    HANDLE h = FindFirstFileW((dir + L"\\*").c_str(), &fd);
    if (h == INVALID_HANDLE_VALUE) return;
    do {
        if (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) continue;
        std::wstring ext = FileExt(fd.cFileName);
        if (ext != L".png" && ext != L".jpg" && ext != L".jpeg" && ext != L".gif" && ext != L".bmp" && ext != L".tif" && ext != L".tiff") continue;
        out.push_back({ L"file:" + dir + L"\\" + fd.cFileName, StripExt(fd.cFileName) });
    } while (FindNextFileW(h, &fd));
    FindClose(h);
    std::sort(out.begin(), out.end(), [](auto& a, auto& b) { return _wcsicmp(a.name.c_str(), b.name.c_str()) < 0; });
}

std::vector<StampCategory> StampCategories() {
    std::vector<StampCategory> cats;
    std::wstring dir = MyStampsDir();
    StampCategory mine{ Tr(L"Meus carimbos"), {}, true };
    ListImages(dir, mine.items);
    cats.push_back(mine);
    WIN32_FIND_DATAW fd;
    HANDLE h = FindFirstFileW((dir + L"\\*").c_str(), &fd);
    if (h != INVALID_HANDLE_VALUE) {
        do {
            if (!(fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) || fd.cFileName[0] == L'.') continue;
            StampCategory c{ std::wstring(Tr(L"Meus carimbos › ")) + fd.cFileName, {}, true };
            ListImages(dir + L"\\" + fd.cFileName, c.items);
            cats.push_back(c);
        } while (FindNextFileW(h, &fd));
        FindClose(h);
    }
    for (auto& s : kStock) cats.push_back({ Tr(s.cat), ParseList(s.list), false });
    return cats;
}

std::vector<StampItem> SearchStamps(const std::vector<StampCategory>& cats, const std::wstring& text) {
    std::vector<StampItem> out;
    std::wstring q = ToLower(Trim(text));
    if (q.empty()) return out;
    for (auto& c : cats)
        for (auto& it : c.items)
            if (ToLower(it.name).find(q) != std::wstring::npos || ToLower(c.name).find(q) != std::wstring::npos) out.push_back(it);
    return out;
}

int ImportStamps(HWND owner) {
    IFileOpenDialog* dlg = nullptr;
    if (FAILED(CoCreateInstance(CLSID_FileOpenDialog, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&dlg)))) return 0;
    DWORD opt = 0; dlg->GetOptions(&opt);
    dlg->SetOptions(opt | FOS_ALLOWMULTISELECT | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM);
    dlg->SetTitle(Tr(L"Importar carimbos"));
    COMDLG_FILTERSPEC spec[] = { { Tr(L"Imagens"), L"*.png;*.jpg;*.jpeg;*.gif;*.bmp;*.tif;*.tiff" } };
    dlg->SetFileTypes(1, spec);
    int n = 0;
    if (SUCCEEDED(dlg->Show(owner))) {
        IShellItemArray* arr = nullptr;
        if (SUCCEEDED(dlg->GetResults(&arr))) {
            DWORD count = 0; arr->GetCount(&count);
            for (DWORD i = 0; i < count; i++) {
                IShellItem* it = nullptr;
                if (FAILED(arr->GetItemAt(i, &it))) continue;
                PWSTR p = nullptr;
                if (SUCCEEDED(it->GetDisplayName(SIGDN_FILESYSPATH, &p))) {
                    std::wstring dst = MyStampsDir() + L"\\" + FileNameOf(p);
                    if (CopyFileW(p, dst.c_str(), FALSE)) n++;
                    CoTaskMemFree(p);
                }
                it->Release();
            }
            arr->Release();
        }
    }
    dlg->Release();
    return n;
}

bool AddImageToMyStamps(const Image& img, std::wstring& saved) {
    if (img.empty()) return false;
    saved = MyStampsDir() + L"\\" + TimeStamp(L"Carimbo %04d-%02d-%02d %02d-%02d-%02d") + L".png";
    return SaveImageFile(saved, img);
}

} // namespace snap::ed
