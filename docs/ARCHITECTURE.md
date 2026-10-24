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
| `src/common/` | biblioteca estática compartilhada: imagem (WIC/GDI+), XML, IPC, formato `.snap`, biblioteca de capturas, efeitos, visão computacional, GIF, formulários e ícones |
| `src/capture/` | `Snapnab.exe`: janela de captura (`main.cpp`), controles modernos (`Widgets.*`), presets, atalhos, mira (`SelectOverlay.*`), motor de captura, gravador de vídeo e HUD |
| `src/editor/` | `SnapnabEditor.exe`: documento e desfazer, renderização dos objetos, canvas e ferramentas, painéis, vídeo, PDF, OCR |
| `src/snapex/` | `SnapEx64.dll` |
| `src/resources/` | ícones e manifesto (DPI PerMonitorV2, Common Controls 6) |
| `tests/` | testes de integração (`SnapnabTests.exe`, rodados pelo CTest) |

A versão do produto fica só em `project(VERSION …)` no `CMakeLists.txt`; o CMake gera `Version.h`, usado pelo código
e pelos recursos `VERSIONINFO` dos executáveis.

## Como a arquitetura de referência foi seguida

| Documento | Implementação |
|---|---|
| Modelo de processos (pseudocódigo §1–§2) | instância única por mutex + janela (`SnapnabUI` / `SnapnabEditor`); a 2ª instância repassa a linha de comando por `WM_COPYDATA` (`dwData 0x8010`, GUID + tamanho + UTF-16 ≤ 10 000 bytes); o Editor inicia o capturador se ele não estiver rodando |
| Máquina de estados `CCaptureEngine` (§3) | `src/capture/CaptureEngine.*`: `Idle → Activated → InputSelecting → InputSelected → Capturing → HaveInput → Outputting`, `Suspended` na pausa do vídeo (tudo registrado no log) |
| IPC (§4, §19.8, ed_comp_14) | `src/common/Ipc.*`: mapeamento de 4 KiB + mutex + `…Event%d` + ack `…WaitUntilMessageSentEvent%d`; mensagem `[code][kind][payload]` (int / 16 B / UTF-16); espera do par 75×200 ms; ack de 10 s só se o par não estiver travado (`IsHungAppWindow`). Códigos: `0x06` mostrar, `0x0A` fechar se oculto, `0x0C` recarregar, `0x1E` nova imagem do clipboard, `0x02/0x03` documento aberto/liberado, `0x12` captura pedida pelo Editor, `0x13` Editor fechou |
| Janela estendida (§6) | `src/snapex/SnapEx64.cpp` (`Snap5000`/`Snap5001`) |
| Rolagem / panorâmica (§3.2, §9.5) | rolagem por roda do mouse + `PhaseCorrelationTranslationDetector` próprio (FFT 2D, `conj(B)`, `DBL_EPSILON`, janela de Hann, pico negado) com verificação por SAD e costura |
| Visão computacional (§9) | `src/common/Cv.*`: varinha ordenada por custo, seleção global, `DoWand` (clamp 0,01–100, atalho 100), Dingo7 (cor dominante da vizinhança dilatada), difusão (papel do Navier-Stokes), tolerância automática do Smart Move (fórmula de §9.6) |
| Formato `.snap` (ed_comp_2 §3) | `src/common/SnapFile.*`: cabeçalho de 24 bytes + arquivo composto OLE sobre `ILockBytes` com deslocamento; fluxos `Index` (XML), `Frame%d/Document`, `Frame%d/ImageStorage/Image_N.png` |
| Biblioteca (DataStore) | `%LOCALAPPDATA%\Snapnab\Library` (`<GUID>.snap`, `Video_*.mp4`, `Thumbnails\`); a pasta é monitorada pelo Editor |
| Presets (comp_7) | `Presets1.xml` com captura, efeitos, destinos, áudio/vídeo e atalho no mesmo objeto; importar/exportar `.snappresets` |
| Atalhos (comp_7 §4) | `RegisterHotKey` com detecção de conflito interno; conflito externo (ex.: Print Screen do Windows) cai para gancho `WH_KEYBOARD_LL`; Esc registrado como atalho id 3 durante a mira |
| Vídeo (§11, comp_3) | `src/capture/Recorder.*`: BitBlt → `IMFSinkWriter` H.264/MP4 (hardware opcional, cai para software), WASAPI microfone + loopback do sistema, mixer com mudo separado → AAC, pausa sem buraco no tempo, redução para 1080p, cursor, limites de disco, impede suspensão |
| Corte MP4 (§11.4) | `TranscodeWithCuts` com `IsInsideAnyCut` / `PrecedingCutsDuration` (reescrita de PTS) |
| GIF (§11.5) | `src/common/Gif.*`: LZW próprio, 4 bpp (16 cores) ou 8 bpp, dithering Floyd–Steinberg, quadros delta |
| PDF (§10.1) | página A4 ou Carta conforme o sistema de medidas, orientação pela imagem, margem 0 |

## Janela de captura

Compacta e de tamanho fixo: abas Imagem/Vídeo na vertical, campos (seleção, efeitos, arquivo, atraso ou quadros/qualidade),
opções, botão de captura com os atalhos e barra inferior com Presets (a lista abre abaixo), Novo, Opções e Abrir Editor.
As cores puxam de leve para a cor de destaque do Windows (Personalização > Cores) e acompanham a troca na hora.

Os controles são próprios (`src/capture/Widgets.*`) e falam as mensagens dos controles padrão (`BM_GETCHECK`, `CB_ADDSTRING`,
`CBN_SELCHANGE`…): chaves liga/desliga, botões planos ou só com ícone e caixas com lista suspensa. A caixa **Efeitos** lista
Borda, Filtros, Sombra, Informações da captura, Substituição de cor, Redimensionar imagem, Bordas especiais, Cortar bordas,
Ajuste de cor, Girar imagem, Marca d'água e Resolução da imagem; escolher um abre os ajustes e o liga, com
"Adicionar outro efeito" e "Remover este efeito" para combinar vários.

## Diferenças deliberadas em relação à referência

- **Não implementado (fora do escopo ou dependente de serviços de terceiros):** licenciamento, telemetria, envio de falhas para
  a nuvem, atualizador, conta/login, feature switches em JWT, plugins de nuvem (`.tscplugin`), automação COM, impressora
  virtual, processo auxiliar com `uiAccess`, integração com celular, modelos/drop zones, Simplify, corretor ortográfico,
  tradução XLIFF, vídeo a partir de imagens e webcam.
- **Substituições de componentes de terceiros:** LEADTOOLS → WIC; ABBYY → OCR do Windows (`Windows.Media.Ocr`, depende dos
  idiomas instalados); SQL CE → a própria pasta da biblioteca; OpenCV → algoritmos próprios em `Cv.cpp`; Cairo/Pango → GDI+.
- `SnapEx64` instala o hook só no thread da janela-alvo (a referência usa hook global), para não injetar a DLL em todo o desktop.
- O Dingo7 conta só o anel em volta da seleção (o pseudocódigo inclui os pixels do próprio objeto, o que puxaria a cor dele).
- O efeito "Resolução da imagem" só vale para arquivos salvos pela captura; a imagem que vai ao Editor não guarda o DPI.

## Testes

`SnapnabTests.exe` (via `build.bat test` ou `ctest --preset release`) cobre: correlação de fase (37/120/250 px), varinha e
inpainting, ida e volta do `.snap`, renderização de todos os objetos, PDF, GIF 4/8 bpp, efeitos (incluindo os 12 da captura
juntos), IPC entre emissor e receptor, gravação MP4 real (orientação dos quadros e pausa excluída da duração), corte de MP4,
GIF a partir de vídeo e OCR.

Modos auxiliares: `SnapnabTests.exe dump arquivo.snap` imprime o XML do documento; `SnapnabTests.exe audio arquivo.mp4`
lê a faixa de áudio.
