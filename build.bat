@echo off
rem ===========================================================================
rem  Snapnab — build and package (MSVC x64 + CMake + Visual Studio's Ninja)
rem
rem    build.bat             Release + product package in dist\
rem    build.bat test        Release + integration tests (ctest)
rem    build.bat debug       Debug (out\build\debug\bin)
rem    build.bat clean       deletes out\ and dist\
rem
rem  Release output:
rem    dist\Snapnab\                      program folder (Snapnab.exe, SnapnabEditor.exe, SnapEx64.dll, lang\, help\)
rem    dist\Snapnab-<version>-win-x64.zip portable package for distribution
rem    dist\Snapnab-<version>-setup.exe   installer (only if Inno Setup 6 is installed:
rem                                       winget install JRSoftware.InnoSetup)
rem    dist\symbols\                      PDBs (crash debugging; do not distribute)
rem ===========================================================================
setlocal
set "ROOT=%~dp0"
cd /d "%ROOT%"

if /i "%~1"=="clean" (
  if exist out rmdir /s /q out
  if exist dist rmdir /s /q dist
  echo Clean.
  exit /b 0
)

rem ---- Visual Studio environment (cl, rc, cmake, ninja)
if defined VCINSTALLDIR goto :havevc
set VSWHERE="%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist %VSWHERE% goto :novs
for /f "usebackq tokens=*" %%i in (`%VSWHERE% -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSDIR=%%i"
if not defined VSDIR goto :novs
call "%VSDIR%\VC\Auxiliary\Build\vcvars64.bat" >nul || goto :novs
:havevc
where cmake >nul 2>nul || (echo [error] CMake not found. Install the "C++ CMake tools for Windows" component of Visual Studio. & exit /b 1)

set PRESET=release
if /i "%~1"=="debug" set PRESET=debug

cmake --preset %PRESET% || goto :fail
cmake --build --preset %PRESET% || goto :fail

if /i "%~1"=="debug" (
  echo.
  echo Debug build OK: out\build\debug\bin
  exit /b 0
)
if /i "%~1"=="test" (
  ctest --preset release || goto :fail
  exit /b 0
)

rem ---- product package
if exist dist\Snapnab rmdir /s /q dist\Snapnab
if exist dist\symbols rmdir /s /q dist\symbols
cmake --install out\build\release --prefix dist\Snapnab --component Runtime || goto :fail
cmake --install out\build\release --prefix dist\symbols --component Symbols || goto :fail
cpack --preset release || goto :fail
if exist dist\_CPack_Packages rmdir /s /q dist\_CPack_Packages

rem ---- installer (Inno Setup 6) with the version from CMakeLists.txt
if exist dist\Snapnab-*-setup.exe del /q dist\Snapnab-*-setup.exe
set "ISCC="
for %%p in ("%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe" "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" "%ProgramFiles%\Inno Setup 6\ISCC.exe") do if not defined ISCC if exist "%%~p" set "ISCC=%%~p"
if not defined ISCC for /f "delims=" %%p in ('where iscc 2^>nul') do if not defined ISCC set "ISCC=%%p"
if not defined ISCC goto :noinno
set "VER="
for /f "tokens=2 delims==" %%v in ('findstr /b /c:"CMAKE_PROJECT_VERSION:" out\build\release\CMakeCache.txt') do set "VER=%%v"
"%ISCC%" /Q /DAppVersion=%VER% packaging\installer\Snapnab.iss || goto :fail
:noinno

echo.
echo Build OK
echo   Program:    dist\Snapnab\Snapnab.exe
for %%z in (dist\Snapnab-*-win-x64.zip) do echo   Package:    %%z
for %%s in (dist\Snapnab-*-setup.exe) do echo   Installer:  %%s
if not defined ISCC echo   Installer:  Inno Setup 6 not found - install it with: winget install JRSoftware.InnoSetup
exit /b 0

:novs
echo [error] Visual Studio 2022 or later with "Desktop development with C++" not found.
exit /b 1
:fail
echo.
echo [error] build failed
exit /b 1
