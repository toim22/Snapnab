# Arquitetura

O Snapnab é uma implementação em C++20/Win32 (x64) feita a partir de documentos de referência de arquitetura e
pseudocódigo que não fazem parte deste repositório (as citações `§N`, `comp_N` e `ed_comp_N` no código apontam para eles). São **dois executáveis** que cooperam, mais uma DLL:

| Binário | Equivale a | Papel |
|---|---|---|
| `Snapnab.exe` | `Snagit32.exe` | bandeja, janela de captura, presets, atalhos globais, motor de captura, gravação de vídeo, saídas |
| `SnapnabEditor.exe` | `SnagitEditor.exe` | edição de imagens (objetos vetoriais + raster), efeitos, OCR, PDF, edição de vídeo e GIF, biblioteca |
| `SnapEx64.dll` | `SnagEx64.dll` | captura de janela estendida (hook `WH_CALLWNDPROC` + `WM_GETMINMAXINFO` 19500×19500) |

## Código-fonte

| Pasta | Conteúdo |
|---|---|
| `src/common/` | biblioteca estática compartilhada: imagem (WIC/GDI+), XML, IPC, formato `.snap`, biblioteca de capturas, efeitos, visão computacional, GIF, OCR, formulários, ícones e a interface comum aos dois programas: barra de título com menus (`CaptionBar.*`), cores da cor de destaque (`Theme.*`) e controles modernos (`Widgets.*`) |
| `src/capture/` | `Snapnab.exe`: janela de captura (`main.cpp`), presets, atalhos, mira (`SelectOverlay.*`), motor de captura, rolagem panorâmica (`Panoramic.*`), gravador de vídeo e HUD, webcam (`Webcam.*`) e One Click (`OneClick.*`) |
| `src/editor/` | `SnapnabEditor.exe`: documento e desfazer, renderização dos objetos, canvas e ferramentas, painéis, vídeo, PDF, OCR |
| `src/snapex/` | `SnapEx64.dll` |
| `src/resources/` | ícones e manifesto (DPI PerMonitorV2, Common Controls 6) |
| `tests/` | testes de integração (`SnapnabTests.exe`, rodados pelo CTest) |
| `lang/` | traduções da interface: `<idioma>.po` (gettext) e o modelo `snapnab.pot`, gerado por `scripts/i18n.py` |

A versão do produto fica só em `project(VERSION …)` no `CMakeLists.txt`; o CMake gera `Version.h`, usado pelo código
e pelos recursos `VERSIONINFO` dos executáveis.

## Como a arquitetura de referência foi seguida

| Documento | Implementação |
|---|---|
| Modelo de processos (pseudocódigo §1–§2) | instância única por mutex + janela (`SnapnabUI` / `SnapnabEditor`); a 2ª instância repassa a linha de comando por `WM_COPYDATA` (`dwData 0x8010`, GUID + tamanho + UTF-16 ≤ 10 000 bytes); o Editor inicia o capturador se ele não estiver rodando |
| Máquina de estados `CCaptureEngine` (§3) | `src/capture/CaptureEngine.*`: `Idle → Activated → InputSelecting → InputSelected → Capturing → HaveInput → Outputting`, `Suspended` na pausa do vídeo (tudo registrado no log) |
| IPC (§4, §19.8, ed_comp_14) | `src/common/Ipc.*`: mapeamento de 4 KiB + mutex + `…Event%d` + ack `…WaitUntilMessageSentEvent%d`; mensagem `[code][kind][payload]` (int / 16 B / UTF-16); espera do par 75×200 ms; ack de 10 s só se o par não estiver travado (`IsHungAppWindow`). Códigos: `0x06` mostrar, `0x0A` fechar se oculto, `0x0C` recarregar, `0x1E` nova imagem do clipboard, `0x02/0x03` documento aberto/liberado, `0x12` captura pedida pelo Editor, `0x13` Editor fechou; próprios do Snapnab: `0x30` abrir arquivo, `0x31` biblioteca mudou, `0x32` reiniciar o Editor (troca de idioma) |
| Janela estendida (§6) | `src/snapex/SnapEx64.cpp` (`Snap5000`/`Snap5001`) |
| Rolagem automática (§3.2, §9.5) | rolagem por roda do mouse + `PhaseCorrelationTranslationDetector` próprio (FFT 2D, `conj(B)`, `DBL_EPSILON`, janela de Hann, pico negado) com verificação por SAD e costura |
| Rolagem panorâmica (comp_1/comp_3) | `src/capture/Panoramic.*`: a região escolhida é copiada ~11 vezes por segundo enquanto o usuário rola; o deslocamento entre quadros sai da comparação das assinaturas de linhas (ou colunas), e só a faixa nova é colada, para cabeçalhos fixos não se repetirem |
| One Click (comp_7 §3) | `src/capture/OneClick.*`: janela em camadas presa a uma borda da tela; recolhe para uma faixa na cor de destaque e abre com o mouse em cima; arrastar solta o painel e ele gruda na borda mais próxima (`OneClickEdge`, `OneClickPos`, `OneClickMonX/Y`, `OneClickEnabled` no registro); some durante a captura e fica fora das capturas (`WDA_EXCLUDEFROMCAPTURE`) |
| Webcam (comp_3 §8) | `src/capture/Webcam.*`: `MFEnumDeviceSources` + `IMFSourceReader` (RGB32 pelo processador de vídeo) numa janela em camadas espelhada dentro da área gravada (círculo, quadrado arredondado, quadrado, 16:9 ou 4:3); como a gravação copia a tela, a webcam entra no vídeo; chroma key por distância de crominância (Cb/Cr) com borda suave e redução do halo; o HUD tem o botão de mostrar/ocultar (`ToggleWebcamCommand`) |
| Grab Text | preset "Capturar texto": a captura passa pelo OCR (`src/common/Ocr.*`), o texto vai para a área de transferência e abre numa janela |
| Visão computacional (§9) | `src/common/Cv.*`: varinha ordenada por custo, seleção global, `DoWand` (clamp 0,01–100, atalho 100), Dingo7 (cor dominante da vizinhança dilatada), difusão (papel do Navier-Stokes), tolerância automática do Smart Move (fórmula de §9.6) |
| Formato `.snap` (ed_comp_2 §3) | `src/common/SnapFile.*`: cabeçalho de 24 bytes + arquivo composto OLE sobre `ILockBytes` com deslocamento; fluxos `Index` (XML), `Frame%d/Document`, `Frame%d/ImageStorage/Image_N.png` |
| Biblioteca (DataStore) | `%LOCALAPPDATA%\Snapnab\Library` (`<GUID>.snap`, `Video_*.mp4`, `Thumbnails\`); a pasta é monitorada pelo Editor |
| Região interativa ou fixa (comp_1 §6, comp_7 §2) | engrenagem da Região: `RegionSelectionType` Interactive/Fixed, `Region{Left,Top,Width,Height}` e `SetPosition`; com posição, a entrada fica pronta sem seletor (`INPUT_REGION_FIXED`); sem posição, a mira mostra um retângulo de tamanho fixo para posicionar |
| Presets (comp_7) | `Presets1.xml` com captura, efeitos, destinos, áudio/vídeo e atalho no mesmo objeto; importar/exportar `.snappresets` |
| Atalhos (comp_7 §4) | `RegisterHotKey` com detecção de conflito interno; conflito externo (ex.: Print Screen do Windows) cai para gancho `WH_KEYBOARD_LL`; Esc registrado como atalho id 3 durante a mira |
| Vídeo (§11, comp_3) | `src/capture/Recorder.*`: BitBlt → `IMFSinkWriter` H.264/MP4 (hardware opcional, cai para software), WASAPI microfone + loopback do sistema, mixer com mudo separado → AAC, pausa sem buraco no tempo, redução para 1080p, cursor, limites de disco, impede suspensão |
| Corte MP4 (§11.4) | `TranscodeWithCuts` com `IsInsideAnyCut` / `PrecedingCutsDuration` (reescrita de PTS) |
| GIF (§11.5) | `src/common/Gif.*`: LZW próprio, 4 bpp (16 cores) ou 8 bpp, dithering Floyd–Steinberg, quadros delta |
| PDF (§10.1) | página A4 ou Carta conforme o sistema de medidas, orientação pela imagem, margem 0 |
| Idioma da interface (ed_comp_1 §3, passo 5) | `src/common/I18n.*`: como a `SnagItEditorRes.dll` da pasta do idioma, cada programa carrega no início `lang\<código>.po` da pasta do executável; o código fica em português e `Tr(L"…")` devolve a tradução (tabelas estáticas marcam com `TR_NOOP` e traduzem ao mostrar). Idioma = o escolhido em *Opções* (`Language` em `HKCU\Software\Snapnab\1`) ou o primeiro idioma de exibição do Windows com catálogo, senão inglês; o que faltar num catálogo cai no inglês. Trocar o idioma reinicia os dois programas (`/restartwait` espera a instância anterior fechar). Nomes de fábrica gravados como dados (presets, temas, efeitos rápidos) ficam em português no arquivo e são traduzidos na exibição. `Format` usa `_vswprintf_p`, então as traduções podem reordenar os valores (`%1$s`) |

## Janela de captura

Compacta e de tamanho fixo, com barra de título própria (ícone e menus Arquivo, Captura, Exibir e Ajuda na mesma faixa,
abertos também por Alt+letra e F10). Abas Imagem/Vídeo na vertical; campos Seleção, Efeitos e Compartilhar (imagem) ou
Seleção, Qualidade e Webcam (vídeo); chaves de opções e Atraso; botão redondo vermelho de captura (o atalho fica na dica).
Barra inferior: ⚙ Presets (abre a lista), + (novo preset com as configurações atuais, novo preset, ajuda) e Abrir Editor.
Cada linha da lista tem "Adicionar atalho" e uma engrenagem (capturar, renomear, atalho, duplicar, excluir).
As cores puxam de leve para a cor de destaque do Windows (Personalização > Cores) e acompanham a troca na hora.

Os controles são próprios (`src/capture/Widgets.*`) e falam as mensagens dos controles padrão (`BM_GETCHECK`, `CB_ADDSTRING`,
`CBN_SELCHANGE`…): chaves liga/desliga, botões planos ou só com ícone, linhas "Rótulo Valor ›" que abrem menu e caixas com
lista suspensa, que podem ter a engrenagem de ajustes dentro (aviso `MUI_GEARCLICK`). A caixa **Efeitos** lista
Borda, Filtros, Sombra, Informações da captura, Substituição de cor, Redimensionar imagem, Bordas especiais, Cortar bordas,
Ajuste de cor, Girar imagem, Marca d'água e Resolução da imagem; escolher um abre os ajustes e o liga, com
"Adicionar outro efeito" e "Remover este efeito" para combinar vários.

## Editor

Mesma barra de título, cores e controles da janela de captura. Em cima, Capturar e Gravar à esquerda, Copiar tudo,
Salvar como e Compartilhar à direita e, no meio, a barra personalizável (TopPanel.ToolBar, ed_comp_3 §3.2): itens dos
tipos `ToolBarItemType` — ferramenta, efeito, saída, comando e espaçador — e o "Mais/Editar" no fim, com as ferramentas
que não estão na barra e a personalização. Automática: Mover, Seleção, Texto, Marcador e Forma, as 4 ferramentas usadas
por último e a Lupa; personalizada: qualquer item, na janela *Personalizar a barra* (disponíveis × na barra) ou
arrastando um item sobre outro. Registro: `ToolbarCustom`, `ToolbarItems` (`t:5,e:1039,o:4,c:1015,s:0`) e
`ToolbarRecentTools`.

À direita, as abas Propriedades e Efeitos. O painel de efeitos (EffectsPanelControls, ed_comp_5 §4) tem a galeria de
efeitos rápidos em cima (`%LOCALAPPDATA%\Snapnab\QuickEffects.xml`; um efeito rápido junta borda, borda especial, dobra
de página e sombra, e "Adicionar" grava os ajustes atuais do painel como um novo) e, embaixo, Borda, Sombra,
Perspectiva, Dobra de página, Bordas especiais, Filtros, Ajuste de cores, Substituir cor, Spotlight, Marca d'água e
Legenda. Clicar num efeito abre os ajustes dentro do painel; cada mudança mostra a prévia ao vivo no lugar da imagem
(PreviewEffect) e *Aplicar* grava o efeito com um passo de desfazer (ApplyEffect). Borda, sombra e legenda mantêm os
objetos editáveis (só deslocados); os demais mesclam os objetos antes, para a prévia e o resultado serem iguais. O menu
*Imagem › Efeitos* e os itens de efeito da barra abrem o mesmo painel. A mesma função gera as miniaturas (com os
valores em pixels escalados para o tamanho da miniatura, refeitas 350 ms depois da última edição).

O Marcador cria objetos `Highlighter` retangulares (`CPaintOpHighlight`, comp_5 §3.2) na cor e na opacidade escolhidas;
marcadores antigos à mão livre continuam abrindo com a cor multiplicada sobre a imagem.

Aba Propriedades (`PropsPanel.cpp`, `ToolProperties.*` + Quick Styles, ed_comp_3 §2.3 e ed_comp_8 §5): em cima, os
estilos rápidos da ferramenta (`QuickStyles.*`): cada tema guarda até 4 cores e os estilos de todas as ferramentas são
gerados a partir delas (GenerateQuickStylesForTheme), mais os que o usuário acrescenta com "+"
(`%LOCALAPPDATA%\Snapnab\ToolThemes.xml`; exportação `.snaptheme`). A estrela favorita o estilo
(`Favorites.xml`); a ferramenta Favoritos mostra todos e, ao escolher um, troca a ferramenta de desenho mantendo a barra
em Favoritos. Na ferramenta Carimbo, a galeria (`Stamps.*`): categorias de emoji coloridos (desenhados com
DirectWrite/Direct2D em `common/Emoji.*`, no tamanho do objeto) e Meus carimbos (`%LOCALAPPDATA%\Snapnab\Stamps`).
Embaixo, as propriedades da ferramenta, desenhadas pelo próprio painel: cores (paleta em `Popups.*`), sombra
(ShadowPropertiesControl: direção na grade 3×3, cor, opacidade, desfoque, distância), forma, pontas e tracejado,
controles deslizantes com caixa de valor, texto avançado. Os objetos são desenhados numa camada própria (`Render.cpp`):
a sombra sai do alfa da camada e a opacidade vale para o objeto inteiro. Os estilos atuais de cada ferramenta ficam em
`ToolStyles.xml`.

Personalizar a barra (CBCGPToolbarCustomize): janela em grade com Ferramentas de desenho, Itens de menu, Destinos de
compartilhamento, Efeitos e Diversos; arrastar um item até a barra o insere onde a barra indica; com a janela aberta a
barra fica em modo de personalização e arrastar um item dela para fora o remove. Cancelar devolve a barra de antes. A
mesma janela escolhe quantas ferramentas a barra automática mostra (6 a 16, `ToolbarToolCount`).

Menu Imagem (como na referência): Ferramentas, Efeitos (itens com ícone, `hbmpItem` 32 bpp), Organizar (ordem um passo
ou até o fim, Alinhar — um objeto em relação à imagem, vários entre si —, Espelhar os objetos selecionados ou a imagem),
Agrupar/Desagrupar, Mesclar os selecionados / Mesclar tudo, Aparar, Redimensionar imagem/canvas, Aplicar modelo
(desabilitado: modelos não implementados), Girar, Encaixe no canvas (`CanvasSnapping`), Cor do canvas e Etiquetar
imagem (atributo `Tags` do documento `.snap`).

Seleção de vários objetos (`Canvas`): o objeto principal (`selected`, o último clicado, cujas propriedades aparecem no
painel) mais os outros selecionados; Ctrl/Shift+clique soma ou tira, o retângulo de seleção do Mover pega os objetos
que ele toca. Agrupar (`CPaintOpGroup`, comp_5 §3.2) troca os selecionados por um `DrawObj` do tipo Group que guarda os
filhos por valor (copiar o grupo copia os filhos, o que mantém o desfazer correto); no `.snap` é `<Group X1… >` com os
elementos dos filhos dentro.

Embaixo, a bandeja de capturas recentes (Tray, ed_comp_3 §5): `Tray.bin` na biblioteca guarda a ordem, até
`RecentTrayDocs` itens (padrão 10); capturas novas e documentos abertos entram na frente, um item excluído abre vaga
para a captura seguinte e "Remover da bandeja" não volta sozinho. Arrastar itens para outro programa
(TrayDragDrop) entrega PNGs com as anotações (gerados em `%TEMP%\Snapnab`) ou o MP4 num `IDataObject` próprio com
`CF_HDROP` e `Shell IDList Array`, efeito preferido "copiar" e "mover" permitido só para as cópias temporárias (o Paint
só aceita "mover"; o MP4 é o arquivo da biblioteca); o `WM_DROPFILES` que chega quando o item volta para o próprio
Editor é descartado ao fim do arraste. Arquivos soltos do Explorer são importados. Segurar o botão num espaço vazio
desenha o retângulo de seleção (Ctrl/Shift somam à seleção). *Biblioteca* (ou *Exibir › Biblioteca*) mostra todas
as capturas em grade no lugar da imagem, com seleção múltipla, exportação e exclusão em lote.

## Diferenças deliberadas em relação à referência

- **Não implementado (fora do escopo ou dependente de serviços de terceiros):** licenciamento, telemetria, envio de falhas para
  a nuvem, atualizador, conta/login, feature switches em JWT, plugins de nuvem (`.tscplugin`), automação COM, impressora
  virtual, processo auxiliar com `uiAccess`, integração com celular, modelos/drop zones, Simplify, corretor ortográfico,
  tradução XLIFF, vídeo a partir de imagens, webcam em tela cheia (o *switcher* que troca a tela pela câmera) e o modo
  All-in-One.
- **Substituições de componentes de terceiros:** LEADTOOLS → WIC; ABBYY → OCR do Windows (`Windows.Media.Ocr`, depende dos
  idiomas instalados); SQL CE → a própria pasta da biblioteca; OpenCV → algoritmos próprios em `Cv.cpp`; Cairo/Pango → GDI+.
- `SnapEx64` instala o hook só no thread da janela-alvo (a referência usa hook global), para não injetar a DLL em todo o desktop.
- O Dingo7 conta só o anel em volta da seleção (o pseudocódigo inclui os pixels do próprio objeto, o que puxaria a cor dele).
- O efeito "Resolução da imagem" só vale para arquivos salvos pela captura; a imagem que vai ao Editor não guarda o DPI.
- Instalador em Inno Setup (`Snapnab-<versão>-setup.exe`, por usuário sem administrador ou para todos) em vez do MSI
  instalado com `msiexec` da referência (comp_9); não há atualizador automático.
- Traduções em catálogos gettext em vez de DLLs de recursos por idioma (mesma escolha no início do programa, mas
  editáveis sem compilar). Em árabe e urdu cada texto é lido da direita para a esquerda (incorporação RLE/PDF), mas a
  disposição das janelas não é espelhada: a interface desenhada com GDI+ não acompanha o espelhamento do GDI.

## Testes

`SnapnabTests.exe` (via `build.bat test` ou `ctest --preset release`) cobre: correlação de fase (37/120/250 px), varinha e
inpainting, ida e volta do `.snap`, renderização de todos os objetos, PDF, GIF 4/8 bpp, efeitos (incluindo os 12 da captura
juntos), IPC entre emissor e receptor, gravação MP4 real (orientação dos quadros e pausa excluída da duração), corte de MP4,
GIF a partir de vídeo, OCR, a junção da rolagem panorâmica (vertical com volta, horizontal e quadro parado) e os
catálogos de tradução (todos completos e com os mesmos marcadores `%d`/`%s` do original).

Modos auxiliares: `SnapnabTests.exe dump arquivo.snap` imprime o XML do documento; `SnapnabTests.exe audio arquivo.mp4`
lê a faixa de áudio.
