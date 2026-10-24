# Testes de integração (console): CV, .snap, renderização, PDF, GIF, efeitos, IPC, gravação MP4, corte, OCR
set(SRC ${PROJECT_SOURCE_DIR}/src)
add_executable(SnapnabTests
    tests.cpp
    ${SRC}/capture/Recorder.cpp
    ${SRC}/editor/Video.cpp
    ${SRC}/editor/Document.cpp
    ${SRC}/editor/Render.cpp
    ${SRC}/editor/Serialize.cpp
    ${SRC}/editor/Pdf.cpp
    ${SRC}/editor/Ocr.cpp)
target_link_libraries(SnapnabTests PRIVATE SnapnabCommon mfplat mfreadwrite mfuuid mf winspool windowsapp)

add_test(NAME integration COMMAND SnapnabTests)
set_tests_properties(integration PROPERTIES TIMEOUT 600)
