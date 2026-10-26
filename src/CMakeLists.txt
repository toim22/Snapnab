# Snapnab targets
set(SNAPNAB_WIN32_LIBS
    user32 gdi32 gdiplus ole32 oleaut32 uuid shell32 shlwapi comctl32 comdlg32
    windowscodecs advapi32 dwmapi msimg32 winmm rpcrt4 d2d1 dwrite)
set(SNAPNAB_MF_LIBS mfplat mfreadwrite mfuuid mf)

# single product version (project(VERSION …)) for the code and the .rc resources
configure_file(common/Version.h.in ${CMAKE_CURRENT_BINARY_DIR}/generated/Version.h @ONLY)

# ---- common library (image, IPC, .snap, library, effects, computer vision, UI)
file(GLOB COMMON_SOURCES CONFIGURE_DEPENDS common/*.cpp common/*.h)
add_library(SnapnabCommon STATIC ${COMMON_SOURCES})
target_include_directories(SnapnabCommon PUBLIC common ${CMAKE_CURRENT_BINARY_DIR}/generated)
target_link_libraries(SnapnabCommon PUBLIC ${SNAPNAB_WIN32_LIBS} windowsapp)     # windowsapp: OCR (Windows.Media.Ocr)

# ---- SnapEx64.dll: extended window capture hook
add_library(SnapEx64 SHARED snapex/SnapEx64.cpp snapex/SnapEx64.def)
target_link_libraries(SnapEx64 PRIVATE user32)

# ---- Snapnab.exe: tray, capture window, hotkeys, capture engine and recording
file(GLOB CAPTURE_SOURCES CONFIGURE_DEPENDS capture/*.cpp capture/*.h)
add_executable(Snapnab WIN32 ${CAPTURE_SOURCES} capture/Snapnab.rc)
target_link_libraries(Snapnab PRIVATE SnapnabCommon ${SNAPNAB_MF_LIBS} uiautomationcore)   # uiautomationcore: links of the captured area
add_dependencies(Snapnab SnapEx64)

# ---- SnapnabEditor.exe: image and video editor
file(GLOB EDITOR_SOURCES CONFIGURE_DEPENDS editor/*.cpp editor/*.h)
add_executable(SnapnabEditor WIN32 ${EDITOR_SOURCES} editor/SnapnabEditor.rc)
target_link_libraries(SnapnabEditor PRIVATE SnapnabCommon ${SNAPNAB_MF_LIBS} winspool)
