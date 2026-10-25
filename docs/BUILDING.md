# Como compilar

## Requisitos

- Windows 10 2004+ ou Windows 11, x64
- Visual Studio 2022 ou mais novo com a carga **Desenvolvimento para desktop com C++**, que já traz:
  MSVC, Windows SDK, **C++ CMake tools for Windows** (CMake + Ninja)
- Para gerar o instalador: [Inno Setup 6.5+](https://jrsoftware.org/isinfo.php) (`winget install JRSoftware.InnoSetup`).
  Sem ele, o `build.bat` gera só a pasta e o zip.

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
| `dist/Snapnab/` | o programa como o usuário recebe (`Snapnab.exe`, `SnapnabEditor.exe`, `SnapEx64.dll`, `lang/`, `LEIA-ME.txt`) | não |
| `dist/Snapnab-<versão>-win-x64.zip` | pacote portátil para distribuir (por exemplo, anexado a uma *release* do GitHub) | não |
| `dist/Snapnab-<versão>-setup.exe` | instalador (gerado quando o Inno Setup está instalado) | não |
| `dist/symbols/` | PDBs do Release, para analisar falhas; não vão no pacote | não |

## Instalador

`packaging/installer/Snapnab.iss` é um script do [Inno Setup 6](https://jrsoftware.org/isinfo.php) que empacota
`dist\Snapnab\` num instalador (`dist\Snapnab-<versão>-setup.exe`). O `build.bat` o compila sozinho quando acha o
`ISCC.exe` (em `%LOCALAPPDATA%\Programs`, `Arquivos de Programas` ou no `PATH`), passando a versão do `CMakeLists.txt`.
Para compilar à mão depois do `build.bat`:

```bat
ISCC.exe /DAppVersion=0.2.0 packaging\installer\Snapnab.iss
```

O que o instalador faz:

- pergunta se instala **só para o usuário** (padrão, sem pedir administrador, em `%LOCALAPPDATA%\Programs\Snapnab`) ou
  **para todos os usuários** (em *Arquivos de Programas*, pede administrador); se, no modo do usuário, a pessoa
  escolher uma pasta em que não pode gravar (como *Arquivos de Programas*), oferece reiniciar como administrador com
  essa pasta;
- assistente nos mesmos 12 idiomas do programa, escolhido pelo idioma do Windows: 7 traduções oficiais do Inno Setup e
  5 em `packaging/installer/Languages/` (chinês, hindi, bengali, indonésio e urdu); acompanha o tema claro/escuro
  do Windows e usa o ícone do Snapnab (`WizardLarge.png`, `WizardSmall.png`);
- atalhos no menu Iniciar (Snapnab e Snapnab Editor), atalho na área de trabalho opcional e "iniciar com o Windows"
  (o mesmo valor que *Opções › Iniciar com o Windows* grava);
- fecha o Snapnab se ele estiver aberto, e numa atualização instala por cima mantendo as configurações;
- desinstala por *Configurações › Aplicativos*; capturas, presets e configurações ficam no perfil do usuário.

O executável não é assinado digitalmente, então o SmartScreen pode avisar na primeira execução até o arquivo ganhar
reputação; para assinar, use `SignTool` do Windows SDK com um certificado de assinatura de código.

## Versão

A versão fica só em `project(VERSION …)` no `CMakeLists.txt` (o `build.bat` a repassa ao instalador). Ao mudar,
atualize também o `CHANGELOG.md`.

## Ícones

Os ícones em `src/resources/` são gerados por `python scripts/make_icons.py` (sem dependências).
