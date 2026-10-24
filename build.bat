@echo off
rem ===========================================================================
rem  Snapnab — compila e empacota (MSVC x64 + CMake + Ninja do Visual Studio)
rem
rem    build.bat             Release + pacote do produto em dist\
rem    build.bat test        Release + testes de integração (ctest)
rem    build.bat debug       Debug (out\build\debug\bin)
rem    build.bat clean       apaga out\ e dist\
rem
rem  Resultado do Release:
rem    dist\Snapnab\                      pasta do programa (Snapnab.exe, SnapnabEditor.exe, SnapEx64.dll)
rem    dist\Snapnab-<versão>-win-x64.zip  pacote portátil para distribuir
rem    dist\symbols\                      PDBs (depuração de falhas; não distribuir)
rem ===========================================================================
setlocal
set "ROOT=%~dp0"
cd /d "%ROOT%"

if /i "%~1"=="clean" (
  if exist out rmdir /s /q out
  if exist dist rmdir /s /q dist
  echo Limpo.
  exit /b 0
)

rem ---- ambiente do Visual Studio (cl, rc, cmake, ninja)
if defined VCINSTALLDIR goto :havevc
set VSWHERE="%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist %VSWHERE% goto :novs
for /f "usebackq tokens=*" %%i in (`%VSWHERE% -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSDIR=%%i"
if not defined VSDIR goto :novs
call "%VSDIR%\VC\Auxiliary\Build\vcvars64.bat" >nul || goto :novs
:havevc
where cmake >nul 2>nul || (echo [erro] CMake nao encontrado. Instale o componente "C++ CMake tools for Windows" do Visual Studio. & exit /b 1)

set PRESET=release
if /i "%~1"=="debug" set PRESET=debug

cmake --preset %PRESET% || goto :fail
cmake --build --preset %PRESET% || goto :fail

if /i "%~1"=="debug" (
  echo.
  echo Build Debug OK: out\build\debug\bin
  exit /b 0
)
if /i "%~1"=="test" (
  ctest --preset release || goto :fail
  exit /b 0
)

rem ---- pacote do produto
if exist dist\Snapnab rmdir /s /q dist\Snapnab
if exist dist\symbols rmdir /s /q dist\symbols
cmake --install out\build\release --prefix dist\Snapnab --component Runtime || goto :fail
cmake --install out\build\release --prefix dist\symbols --component Symbols || goto :fail
cpack --preset release || goto :fail
if exist dist\_CPack_Packages rmdir /s /q dist\_CPack_Packages

echo.
echo Build OK
echo   Programa: dist\Snapnab\Snapnab.exe
for %%z in (dist\Snapnab-*-win-x64.zip) do echo   Pacote:   %%z
exit /b 0

:novs
echo [erro] Visual Studio 2022 ou mais novo com "Desenvolvimento para desktop com C++" nao encontrado.
exit /b 1
:fail
echo.
echo [erro] build falhou
exit /b 1
