# Snapnab

**Captura de tela, gravação de vídeo e editor de imagens para Windows.**

![Janela de captura do Snapnab](docs/images/capture-window.png)

O Snapnab fica na área de notificação: aperte **Print Screen**, escolha o que capturar e a imagem abre no
**Snapnab Editor**, pronta para anotar, destacar e compartilhar. Também grava a tela em MP4, com microfone e áudio do sistema.

## Recursos

**Captura**
- Região, janela, tela inteira, monitor, região de tamanho fixo, mão livre, várias áreas, objeto sob o cursor,
  rolagem (página inteira), janela estendida, menus, área de transferência e arquivo
- Lupa e coordenadas na mira, atraso com contagem regressiva, captura do cursor
- Presets com atalhos globais e destinos: Editor, área de transferência, arquivo com nome automático, programa externo
- 12 efeitos aplicados na hora: borda, filtros, sombra, informações da captura, substituição de cor, redimensionar,
  bordas especiais, cortar bordas, ajuste de cor, girar, marca d'água e resolução

**Gravação de vídeo**
- MP4 (H.264 + AAC), microfone e áudio do sistema com mudo separado, pausa sem buraco no vídeo
- Painel de controle e moldura que não aparecem na gravação, redução para 1080p, codificação por hardware

**Editor**
- Seta, linha, formas, caneta, marcador, texto, balões, passos numerados, carimbos, desfoque, destaque, lupa,
  preencher, borracha, conta-gotas, cortar e recortar faixa
- Seleção com Smart Move, varinha mágica e preenchimento inteligente; desfazer/refazer
- Efeitos de imagem, reconhecimento de texto (OCR), salvar em PNG, JPG, BMP, GIF, TIFF, PDF e `.snap`, imprimir e e-mail
- Vídeo: cortar trechos, salvar quadro, criar GIF animado e exportar MP4
- Biblioteca com todas as capturas

A janela de captura acompanha a cor de destaque escolhida em *Configurações > Personalização > Cores*.

## Instalação

Requer **Windows 10 versão 2004 ou mais novo, ou Windows 11 (64 bits)**. Não precisa de nenhum runtime extra.

- **Portátil:** baixe `Snapnab-<versão>-win-x64.zip` na página de *Releases*, extraia em qualquer pasta e abra `Snapnab.exe`.
- **Instalador:** `Snapnab-<versão>-setup.exe` instala, cria atalhos no menu Iniciar e desinstala pelo Windows.

## Uso

| Atalho | Ação |
|---|---|
| `Print Screen` | capturar imagem |
| `Shift+Print Screen` | gravar vídeo (de novo para parar) |
| `Shift+F9` | pausar / retomar a gravação |
| `Shift+F10` | parar a gravação |
| `Esc` | cancelar a seleção |

Os atalhos podem ser trocados em *Opções*. Linha de comando:

```text
Snapnab.exe        /capture | /video | /preset "nome" | /clipboard | /hide | /exit | /help
SnapnabEditor.exe  /Open "arquivo" | /NewFromClipboard | /help
```

### Seus dados

| O quê | Onde |
|---|---|
| Capturas e vídeos | `%LOCALAPPDATA%\Snapnab\Library` |
| Presets | `%LOCALAPPDATA%\Snapnab\Presets1.xml` |
| Logs | `%LOCALAPPDATA%\Snapnab\Logs` |
| Configurações | `HKEY_CURRENT_USER\Software\Snapnab` |

Nada é enviado para a internet.

## Compilar a partir do código

Com o Visual Studio 2022+ (carga *Desenvolvimento para desktop com C++*):

```bat
build.bat
```

O programa sai em `dist\Snapnab\` e o pacote em `dist\Snapnab-<versão>-win-x64.zip`. Detalhes, testes e instalador em
[docs/BUILDING.md](docs/BUILDING.md).

## Estrutura do repositório

```text
src/                    código-fonte (C++20, Win32)
  common/               biblioteca compartilhada pelos dois programas
  capture/              Snapnab.exe — captura e gravação
  editor/               SnapnabEditor.exe — editor
  snapex/               SnapEx64.dll — captura de janela estendida
  resources/            ícones e manifesto
tests/                  testes de integração
packaging/              arquivos do pacote e script do instalador
scripts/                utilitários de desenvolvimento
docs/                   arquitetura, build e imagens
```

A arquitetura e o mapeamento para os documentos de referência estão em [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
O histórico de versões está em [CHANGELOG.md](CHANGELOG.md).

## Licença

Copyright © 2026 Antonio Rezende.

O Snapnab é software livre: você pode usar, estudar, modificar e redistribuir sob os termos da
[GNU General Public License versão 3](LICENSE). Versões modificadas que forem distribuídas também precisam ter o
código aberto sob a mesma licença. O programa é fornecido sem garantia de qualquer tipo.
