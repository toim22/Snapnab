# Como compilar

## Requisitos

- Windows 10 2004+ ou Windows 11, x64
- Visual Studio 2022 ou mais novo com a carga **Desenvolvimento para desktop com C++**, que já traz:
  MSVC, Windows SDK, **C++ CMake tools for Windows** (CMake + Ninja)

Não há dependências externas: tudo usa APIs do Windows (Win32, GDI+, WIC, Media Foundation, WASAPI, Windows.Media.Ocr).

## Linha de comando

```bat
build.bat            :: Release + pacote do produto em dist\
build.bat test       :: Release + testes de integração
build.bat debug      :: Debug em out\build\debug\bin
build.bat clean      :: apaga out\ e dist\
```

O `build.bat` encontra o Visual Studio sozinho (`vswhere`) e chama os presets do CMake. Num *Developer Command Prompt*
dá para usar o CMake direto:

```bat
cmake --preset release
cmake --build --preset release
ctest --preset release
cpack --preset release
```

## Visual Studio / VS Code / CLion

Abra a pasta do repositório (*Arquivo > Abrir > Pasta*). O `CMakePresets.json` é reconhecido e oferece as configurações
**Release (x64)** e **Debug (x64)**. Para depurar, escolha `Snapnab.exe` ou `SnapnabEditor.exe` como item de inicialização.

## Onde fica cada coisa

| Pasta | O quê | Versionada? |
|---|---|---|
| `out/build/<preset>/` | árvore de build do CMake; executáveis em `bin/` | não |
| `dist/Snapnab/` | o programa como o usuário recebe (`Snapnab.exe`, `SnapnabEditor.exe`, `SnapEx64.dll`, `LEIA-ME.txt`) | não |
| `dist/Snapnab-<versão>-win-x64.zip` | pacote portátil para distribuir (por exemplo, anexado a uma *release* do GitHub) | não |
| `dist/symbols/` | PDBs do Release, para analisar falhas; não vão no pacote | não |

## Instalador

`packaging/installer/Snapnab.iss` é um script do [Inno Setup 6](https://jrsoftware.org/isinfo.php) que empacota
`dist\Snapnab\` num instalador (`dist\Snapnab-<versão>-setup.exe`): instala em *Arquivos de Programas*, cria atalhos no
menu Iniciar e desinstala pelo Painel de Controle. Depois do `build.bat`:

```bat
"%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" packaging\installer\Snapnab.iss
```

## Versão

A versão fica só em `project(VERSION …)` no `CMakeLists.txt`. Ao mudar, atualize também `CHANGELOG.md` e a linha
`#define AppVersion` do script do instalador.

## Ícones

Os ícones em `src/resources/` são gerados por `python scripts/make_icons.py` (sem dependências).
