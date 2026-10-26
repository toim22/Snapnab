# Snapnab — screen capture, video recording and image editor for Windows
#
#   cmake --preset release && cmake --build --preset release      (or simply: build.bat)
#
# Products: Snapnab.exe (capture), SnapnabEditor.exe (editor) and SnapEx64.dll (extended window),
# always side by side in the same folder — the two processes find each other through the executable's folder.
cmake_minimum_required(VERSION 3.25)

project(Snapnab
    VERSION 0.3.0
    DESCRIPTION "Screen capture, video recording and image editor"
    LANGUAGES CXX RC)

if(NOT MSVC OR NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
    message(FATAL_ERROR "Snapnab requires MSVC (Visual Studio 2022 or later) and x64.")
endif()

# copyright holder (copyright notice of the code, the executables and the installer)
set(SNAPNAB_COPYRIGHT "Copyright © 2026 Antonio Rezende")

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# static CRT: the executables run without installing the Visual C++ Redistributable
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
# PDB in Release too (the symbols go to dist/symbols, outside the package)
set(CMAKE_MSVC_DEBUG_INFORMATION_FORMAT "ProgramDatabase")

add_compile_definitions(UNICODE _UNICODE NOMINMAX _CRT_SECURE_NO_WARNINGS)
add_compile_options("$<$<COMPILE_LANGUAGE:CXX>:/utf-8;/permissive-;/W3;/EHsc>")
# the manifest (DPI PerMonitorV2, Common Controls 6, asInvoker) comes from the .rc files
add_link_options(/MANIFEST:NO "$<$<CONFIG:Release>:/DEBUG;/OPT:REF;/OPT:ICF>")

# executables and DLL in the same folder in the build tree too (out/build/<preset>/bin)
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/lib)
set(CMAKE_PDB_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)

add_subdirectory(src)

# translation catalogs (lang\<code>.po) next to the executables, in the build tree and in the package
file(GLOB SNAPNAB_PO CONFIGURE_DEPENDS ${CMAKE_SOURCE_DIR}/lang/*.po)
add_custom_target(SnapnabLang ALL
    COMMAND ${CMAKE_COMMAND} -E make_directory ${CMAKE_BINARY_DIR}/bin/lang
    COMMAND ${CMAKE_COMMAND} -E copy_if_different ${SNAPNAB_PO} ${CMAKE_BINARY_DIR}/bin/lang
    DEPENDS ${SNAPNAB_PO}
    VERBATIM)

# user manual (help\Snapnab-Manual.html, opened by Help › Manual and F1) next to the executables
file(GLOB SNAPNAB_HELP CONFIGURE_DEPENDS ${CMAKE_SOURCE_DIR}/help/*)
add_custom_target(SnapnabHelp ALL
    COMMAND ${CMAKE_COMMAND} -E make_directory ${CMAKE_BINARY_DIR}/bin/help
    COMMAND ${CMAKE_COMMAND} -E copy_if_different ${SNAPNAB_HELP} ${CMAKE_BINARY_DIR}/bin/help
    DEPENDS ${SNAPNAB_HELP}
    VERBATIM)

option(SNAPNAB_BUILD_TESTS "Build the integration tests" ON)
if(SNAPNAB_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

# ---------------------------------------------------------------- install / package
# Runtime component = what the user gets; Symbols = PDBs to debug crashes.
install(TARGETS Snapnab SnapnabEditor SnapEx64
    RUNTIME DESTINATION . COMPONENT Runtime)
install(FILES packaging/README.txt CHANGELOG.md LICENSE
    DESTINATION . COMPONENT Runtime)
install(FILES ${SNAPNAB_PO} DESTINATION lang COMPONENT Runtime)
install(FILES ${SNAPNAB_HELP} DESTINATION help COMPONENT Runtime)
install(FILES $<TARGET_PDB_FILE:Snapnab> $<TARGET_PDB_FILE:SnapnabEditor> $<TARGET_PDB_FILE:SnapEx64>
    DESTINATION . COMPONENT Symbols)

set(CPACK_PACKAGE_NAME "Snapnab")
set(CPACK_PACKAGE_VENDOR "Antonio Rezende")
set(CPACK_PACKAGE_DESCRIPTION_SUMMARY "${PROJECT_DESCRIPTION}")
set(CPACK_PACKAGE_FILE_NAME "Snapnab-${PROJECT_VERSION}-win-x64")
set(CPACK_PACKAGE_DIRECTORY "${CMAKE_SOURCE_DIR}/dist")
set(CPACK_GENERATOR "ZIP")
set(CPACK_RESOURCE_FILE_LICENSE "${CMAKE_SOURCE_DIR}/LICENSE")
set(CPACK_INSTALL_CMAKE_PROJECTS "${CMAKE_BINARY_DIR};${PROJECT_NAME};Runtime;/")   # only the Runtime component
set(CPACK_INCLUDE_TOPLEVEL_DIRECTORY ON)
include(CPack)
