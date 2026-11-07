@echo off
rem ===========================================================================
rem  Snapnab — build and package (Rust + MSVC x64)
rem
rem    build.bat             Release + product package in dist\
rem    build.bat test        Release + the workspace tests (cargo test)
rem    build.bat debug       Debug (rust\target\debug)
rem    build.bat clean       deletes rust\target and dist\
rem
rem  Release output:
rem    dist\Snapnab\                      program folder (Snapnab.exe, SnapnabEditor.exe, SnapEx64.dll, lang\, help\)
rem    dist\Snapnab-<version>-win-x64.zip portable package for distribution
rem    dist\Snapnab-<version>-setup.exe   installer (only if Inno Setup 6 is installed:
rem                                       winget install JRSoftware.InnoSetup)
rem    dist\symbols\                      PDBs (crash debugging; do not distribute)
rem
rem  Needs Rust (https://rustup.rs, the x86_64-pc-windows-msvc toolchain) and the Visual Studio C++ build tools
rem  (the linker, rc.exe and the Windows SDK).
rem ===========================================================================
setlocal
set "ROOT=%~dp0"
cd /d "%ROOT%"

if /i "%~1"=="clean" (
  if exist rust\target rmdir /s /q rust\target
  if exist dist rmdir /s /q dist
  echo Clean.
  exit /b 0
)

where cargo >nul 2>nul || (echo [error] cargo not found. Install Rust from https://rustup.rs & exit /b 1)

if /i "%~1"=="debug" (
  cargo build --locked --workspace --manifest-path rust\Cargo.toml || goto :fail
  echo.
  echo Debug build OK: rust\target\debug
  exit /b 0
)
if /i "%~1"=="test" (
  cargo test --locked --release --workspace --manifest-path rust\Cargo.toml || goto :fail
  exit /b 0
)

cargo build --locked --release --workspace --manifest-path rust\Cargo.toml || goto :fail

rem ---- the version (rust\Cargo.toml, [workspace.package])
set "VER="
for /f "tokens=2 delims==" %%v in ('findstr /r /b /c:"version *=" rust\Cargo.toml') do if not defined VER set "VER=%%~v"
set "VER=%VER: =%"
set "VER=%VER:"=%"
if not defined VER (echo [error] no version in rust\Cargo.toml & exit /b 1)

rem ---- program folder and symbols
set "BIN=rust\target\release"
if exist dist\Snapnab rmdir /s /q dist\Snapnab
if exist dist\symbols rmdir /s /q dist\symbols
mkdir dist\Snapnab\lang dist\symbols || goto :fail
for %%f in (Snapnab.exe SnapnabEditor.exe SnapEx64.dll) do copy /y "%BIN%\%%f" dist\Snapnab\ >nul || goto :fail
for %%f in (Snapnab.pdb SnapnabEditor.pdb SnapEx64.pdb) do copy /y "%BIN%\%%f" dist\symbols\ >nul || goto :fail
copy /y packaging\README.txt dist\Snapnab\ >nul || goto :fail
for %%f in (CHANGELOG.md LICENSE THIRD-PARTY-NOTICES.txt) do copy /y %%f dist\Snapnab\ >nul || goto :fail
copy /y lang\*.po dist\Snapnab\lang\ >nul || goto :fail
xcopy /e /i /q /y help dist\Snapnab\help >nul || goto :fail

rem ---- portable package (one top folder, as before)
set "PKG=Snapnab-%VER%-win-x64"
if exist dist\%PKG%.zip del /q dist\%PKG%.zip
if exist dist\_zip rmdir /s /q dist\_zip
xcopy /e /i /q /y dist\Snapnab dist\_zip\%PKG% >nul || goto :fail
powershell -NoProfile -Command "Compress-Archive -Path 'dist\_zip\%PKG%' -DestinationPath 'dist\%PKG%.zip' -CompressionLevel Optimal" || goto :fail
rmdir /s /q dist\_zip

rem ---- installer (Inno Setup 6) with the same version
if exist dist\Snapnab-*-setup.exe del /q dist\Snapnab-*-setup.exe
set "ISCC="
for %%p in ("%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe" "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" "%ProgramFiles%\Inno Setup 6\ISCC.exe") do if not defined ISCC if exist "%%~p" set "ISCC=%%~p"
if not defined ISCC for /f "delims=" %%p in ('where iscc 2^>nul') do if not defined ISCC set "ISCC=%%p"
if not defined ISCC goto :noinno
"%ISCC%" /Q /DAppVersion=%VER% packaging\installer\Snapnab.iss || goto :fail
:noinno

echo.
echo Build OK (version %VER%)
echo   Program:    dist\Snapnab\Snapnab.exe
for %%z in (dist\Snapnab-*-win-x64.zip) do echo   Package:    %%z
for %%s in (dist\Snapnab-*-setup.exe) do echo   Installer:  %%s
if not defined ISCC echo   Installer:  Inno Setup 6 not found - install it with: winget install JRSoftware.InnoSetup
exit /b 0

:fail
echo.
echo [error] build failed
exit /b 1
