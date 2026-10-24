// Formato .snap — ed_comp_2 §3: cabeçalho de 24 bytes + arquivo composto OLE (STGFMT_DOCFILE)
//   Index                    fluxo XML <Index><FrameList><Frame Name='Frame0'/></FrameList></Index>
//   Frame%d/Document         fluxo XML do quadro (objetos vetoriais, propriedades)
//   Frame%d/ImageStorage/... fluxos PNG (imagem base e imagens embutidas, por StreamName)
#pragma once
#include "Image.h"
#include "Xml.h"

namespace snap {

struct SnapFrame {
    Image base;                                     // imagem de fundo (Image_0.png)
    XmlNode doc{ "Document" };                      // objetos e metadados
    std::map<std::string, Image> embedded;          // imagens referenciadas pelos objetos (StreamName → imagem)
};

bool SaveSnapFile(const std::wstring& path, const std::vector<SnapFrame>& frames);
bool LoadSnapFile(const std::wstring& path, std::vector<SnapFrame>& frames);
bool IsSnapFile(const std::wstring& path);

} // namespace snap
