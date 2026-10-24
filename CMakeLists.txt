# Snapnab — captura de tela, gravação de vídeo e editor de imagens para Windows
#
#   cmake --preset release && cmake --build --preset release      (ou simplesmente: build.bat)
#
# Produtos: Snapnab.exe (captura), SnapnabEditor.exe (editor) e SnapEx64.dll (janela estendida),
# sempre lado a lado na mesma pasta — os dois processos se localizam pela pasta do executável.
cmake_minimum_required(VERSION 3.25)

project(Snapnab
    VERSION 1.0.0
    DESCRIPTION "Captura de tela, gravação de vídeo e editor de imagens"
    LANGUAGES CXX RC)

if(NOT MSVC OR NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
    message(FATAL_ERROR "Snapnab requer MSVC (Visual Studio 2022 ou mais novo) e x64.")
endif()

# titular dos direitos autorais (aviso de copyright do código, dos executáveis e do instalador)
set(SNAPNAB_COPYRIGHT "Copyright © 2026 Antonio Rezende")

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# CRT estático: os executáveis rodam sem instalar o Visual C++ Redistributable
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
# PDB também no Release (os símbolos vão para dist/symbols, fora do pacote)
set(CMAKE_MSVC_DEBUG_INFORMATION_FORMAT "ProgramDatabase")

add_compile_definitions(UNICODE _UNICODE NOMINMAX _CRT_SECURE_NO_WARNINGS)
add_compile_options("$<$<COMPILE_LANGUAGE:CXX>:/utf-8;/permissive-;/W3;/EHsc>")
# o manifesto (DPI PerMonitorV2, Common Controls 6, asInvoker) vem dos arquivos .rc
add_link_options(/MANIFEST:NO "$<$<CONFIG:Release>:/DEBUG;/OPT:REF;/OPT:ICF>")

# executáveis e DLL na mesma pasta também na árvore de build (out/build/<preset>/bin)
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/lib)
set(CMAKE_PDB_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)

add_subdirectory(src)

option(SNAPNAB_BUILD_TESTS "Compila os testes de integração" ON)
if(SNAPNAB_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

# ---------------------------------------------------------------- instalação / pacote
# Componente Runtime = o que o usuário recebe; Symbols = PDBs para depurar falhas.
install(TARGETS Snapnab SnapnabEditor SnapEx64
    RUNTIME DESTINATION . COMPONENT Runtime)
install(FILES packaging/LEIA-ME.txt CHANGELOG.md LICENSE
    DESTINATION . COMPONENT Runtime)
install(FILES $<TARGET_PDB_FILE:Snapnab> $<TARGET_PDB_FILE:SnapnabEditor> $<TARGET_PDB_FILE:SnapEx64>
    DESTINATION . COMPONENT Symbols)

set(CPACK_PACKAGE_NAME "Snapnab")
set(CPACK_PACKAGE_VENDOR "Antonio Rezende")
set(CPACK_PACKAGE_DESCRIPTION_SUMMARY "${PROJECT_DESCRIPTION}")
set(CPACK_PACKAGE_FILE_NAME "Snapnab-${PROJECT_VERSION}-win-x64")
set(CPACK_PACKAGE_DIRECTORY "${CMAKE_SOURCE_DIR}/dist")
set(CPACK_GENERATOR "ZIP")
set(CPACK_RESOURCE_FILE_LICENSE "${CMAKE_SOURCE_DIR}/LICENSE")
set(CPACK_INSTALL_CMAKE_PROJECTS "${CMAKE_BINARY_DIR};${PROJECT_NAME};Runtime;/")   # só o componente Runtime
set(CPACK_INCLUDE_TOPLEVEL_DIRECTORY ON)
include(CPack)
