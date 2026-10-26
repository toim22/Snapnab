// Animated GIF (V2AGNative::GifCreator / GIFMaker::Writer, ds_pseudocode §11.5)
//   bitsPerPixel 4 (16 colors, the original's default) or 8; dithering; delta frames; infinite loop
#pragma once
#include "Image.h"

namespace snap {

class GifWriter {
public:
    bool Open(const std::wstring& path, int width, int height, double fps, int bitsPerPixel, bool dithering,
              bool deltaFrames, const std::string& comment = "Snapnab");
    bool AddFrame(const Image& frame, int delayCentiseconds = -1);
    bool Close();
    bool failed() const { return failed_; }
    ~GifWriter() { if (file_ != INVALID_HANDLE_VALUE) Close(); }
private:
    void Put(const void* d, size_t n);
    void PutByte(uint8_t b) { Put(&b, 1); }
    void PutWord(uint16_t w) { Put(&w, 2); }
    HANDLE file_ = INVALID_HANDLE_VALUE;
    int w_ = 0, h_ = 0, bpp_ = 8, delay_ = 10;
    bool dither_ = false, delta_ = false, failed_ = false;
    std::vector<uint32_t> prev_;     // last frame shown (for delta frames)
    std::vector<uint8_t> buf_;
};

} // namespace snap
