# Alvos do Snapnab
set(SNAPNAB_WIN32_LIBS
    user32 gdi32 gdiplus ole32 oleaut32 uuid shell32 shlwapi comctl32 comdlg32
    windowscodecs advapi32 dwmapi msimg32 winmm rpcrt4)
set(SNAPNAB_MF_LIBS mfplat mfreadwrite mfuuid mf)

# versão única do produto (project(VERSION …)) para o código e os recursos .rc
configure_file(common/Version.h.in ${CMAKE_CURRENT_BINARY_DIR}/generated/Version.h @ONLY)

# ---- biblioteca comum (imagem, IPC, .snap, biblioteca, efeitos, visão computacional, UI)
file(GLOB COMMON_SOURCES CONFIGURE_DEPENDS common/*.cpp common/*.h)
add_library(SnapnabCommon STATIC ${COMMON_SOURCES})
target_include_directories(SnapnabCommon PUBLIC common ${CMAKE_CURRENT_BINARY_DIR}/generated)
target_link_libraries(SnapnabCommon PUBLIC ${SNAPNAB_WIN32_LIBS})

# ---- SnapEx64.dll: hook da captura de janela estendida
add_library(SnapEx64 SHARED snapex/SnapEx64.cpp snapex/SnapEx64.def)
target_link_libraries(SnapEx64 PRIVATE user32)

# ---- Snapnab.exe: bandeja, janela de captura, atalhos, motor de captura e gravação
file(GLOB CAPTURE_SOURCES CONFIGURE_DEPENDS capture/*.cpp capture/*.h)
add_executable(Snapnab WIN32 ${CAPTURE_SOURCES} capture/Snapnab.rc)
target_link_libraries(Snapnab PRIVATE SnapnabCommon ${SNAPNAB_MF_LIBS})
add_dependencies(Snapnab SnapEx64)

# ---- SnapnabEditor.exe: editor de imagens e vídeo
file(GLOB EDITOR_SOURCES CONFIGURE_DEPENDS editor/*.cpp editor/*.h)
add_executable(SnapnabEditor WIN32 ${EDITOR_SOURCES} editor/SnapnabEditor.rc)
target_link_libraries(SnapnabEditor PRIVATE SnapnabCommon ${SNAPNAB_MF_LIBS} winspool windowsapp)
