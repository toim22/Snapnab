#include "Gif.h"
#include <cmath>

namespace snap {

void GifWriter::Put(const void* d, size_t n) {
    if (file_ == INVALID_HANDLE_VALUE) return;
    const uint8_t* p = (const uint8_t*)d;
    buf_.insert(buf_.end(), p, p + n);
    if (buf_.size() > 1 << 20) {
        DWORD wr; if (!WriteFile(file_, buf_.data(), (DWORD)buf_.size(), &wr, nullptr)) failed_ = true;
        buf_.clear();
    }
}

bool GifWriter::Open(const std::wstring& path, int w, int h, double fps, int bpp, bool dither, bool delta,
                     const std::string& comment) {
    file_ = CreateFileW(path.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file_ == INVALID_HANDLE_VALUE) { failed_ = true; Log(L"Failed to open %s for writing", path.c_str()); return false; }
    w_ = w; h_ = h; bpp_ = bpp <= 4 ? 4 : 8; dither_ = dither; delta_ = delta;
    delay_ = std::max(2, (int)std::lround(100.0 / std::max(1.0, fps)));
    Put("GIF89a", 6);
    PutWord((uint16_t)w); PutWord((uint16_t)h);
    PutByte(0x00); PutByte(0); PutByte(0);                 // no global palette (each frame has its own)
    // NETSCAPE2.0: infinite loop
    const uint8_t loop[] = { 0x21, 0xFF, 0x0B, 'N','E','T','S','C','A','P','E','2','.','0', 0x03, 0x01, 0x00, 0x00, 0x00 };
    Put(loop, sizeof loop);
    if (!comment.empty()) {
        PutByte(0x21); PutByte(0xFE);
        for (size_t i = 0; i < comment.size(); i += 255) {
            size_t n = std::min<size_t>(255, comment.size() - i);
            PutByte((uint8_t)n); Put(comment.data() + i, n);
        }
        PutByte(0);
    }
    return true;
}

namespace {
struct Box { int lo, hi; };   // range in a color vector

// median cut palette
std::vector<uint32_t> MedianCut(std::vector<uint32_t>& cols, int maxColors) {
    std::vector<Box> boxes{ { 0, (int)cols.size() } };
    auto rangeOf = [&](const Box& b, int& ch) {
        int mn[3] = { 255, 255, 255 }, mx[3] = { 0, 0, 0 };
        for (int i = b.lo; i < b.hi; i++) {
            int c[3] = { CR(cols[i]), CG(cols[i]), CB(cols[i]) };
            for (int k = 0; k < 3; k++) { mn[k] = std::min(mn[k], c[k]); mx[k] = std::max(mx[k], c[k]); }
        }
        int best = 0; ch = 0;
        for (int k = 0; k < 3; k++) if (mx[k] - mn[k] > best) { best = mx[k] - mn[k]; ch = k; }
        return best;
    };
    while ((int)boxes.size() < maxColors) {
        int bi = -1, bestR = 0, bestCh = 0;
        for (int i = 0; i < (int)boxes.size(); i++) {
            if (boxes[i].hi - boxes[i].lo < 2) continue;
            int ch; int r = rangeOf(boxes[i], ch);
            if (r > bestR) { bestR = r; bi = i; bestCh = ch; }
        }
        if (bi < 0) break;
        Box b = boxes[bi];
        int shift = 16 - bestCh * 8;
        std::sort(cols.begin() + b.lo, cols.begin() + b.hi, [shift](uint32_t a, uint32_t c) { return ((a >> shift) & 255) < ((c >> shift) & 255); });
        int mid = (b.lo + b.hi) / 2;
        boxes[bi] = { b.lo, mid };
        boxes.push_back({ mid, b.hi });
    }
    std::vector<uint32_t> pal;
    for (auto& b : boxes) {
        long long r = 0, g = 0, bl = 0; int n = b.hi - b.lo;
        if (n <= 0) continue;
        for (int i = b.lo; i < b.hi; i++) { r += CR(cols[i]); g += CG(cols[i]); bl += CB(cols[i]); }
        pal.push_back(ARGB(255, (int)(r / n), (int)(g / n), (int)(bl / n)));
    }
    if (pal.empty()) pal.push_back(0xFF000000);
    return pal;
}

class Lzw {
public:
    Lzw(int minCode, std::vector<uint8_t>& out) : min_(minCode), out_(out) {}
    void Encode(const std::vector<uint8_t>& idx) {
        const int clear = 1 << min_, eoi = clear + 1;
        std::vector<int> dict((size_t)4096 * 256, -1);    // (prefix, symbol) → code
        int next = eoi + 1, size = min_ + 1;
        Emit(clear, size);
        int prefix = idx.empty() ? 0 : idx[0];
        for (size_t i = 1; i < idx.size(); i++) {
            int k = idx[i];
            int& slot = dict[(size_t)prefix * 256 + k];
            if (slot >= 0) { prefix = slot; continue; }
            Emit(prefix, size);
            slot = next++;
            if (next > (1 << size) && size < 12) size++;
            if (next == 4096) {                 // dictionary full: clear and start over
                Emit(clear, size);
                std::fill(dict.begin(), dict.end(), -1);
                next = eoi + 1; size = min_ + 1;
            }
            prefix = k;
        }
        if (!idx.empty()) Emit(prefix, size);
        Emit(eoi, size);
        if (nbits_) out_.push_back((uint8_t)acc_);
    }
private:
    void Emit(int code, int size) {
        acc_ |= (uint32_t)code << nbits_;
        nbits_ += size;
        while (nbits_ >= 8) { out_.push_back((uint8_t)(acc_ & 255)); acc_ >>= 8; nbits_ -= 8; }
    }
    int min_;
    std::vector<uint8_t>& out_;
    uint32_t acc_ = 0; int nbits_ = 0;
};
}

bool GifWriter::AddFrame(const Image& frameIn, int delayCs) {
    if (file_ == INVALID_HANDLE_VALUE || failed_) return false;
    Image frame = (frameIn.w == w_ && frameIn.h == h_) ? frameIn : ScaleImage(frameIn, w_, h_);
    for (auto& p : frame.px) p = BlendOver(0xFFFFFFFF, p);         // GIF has no partial alpha

    // changed rectangle (delta frames)
    RECT rc{ 0, 0, w_, h_ };
    bool useDelta = delta_ && !prev_.empty();
    std::vector<uint8_t> changed;
    if (useDelta) {
        changed.assign((size_t)w_ * h_, 0);
        RECT b{ w_, h_, 0, 0 };
        for (int y = 0; y < h_; y++)
            for (int x = 0; x < w_; x++) {
                size_t i = (size_t)y * w_ + x;
                uint32_t a = frame.px[i], c = prev_[i];
                if (std::abs(CR(a) - CR(c)) + std::abs(CG(a) - CG(c)) + std::abs(CB(a) - CB(c)) > 12) {
                    changed[i] = 1;
                    b.left = std::min<LONG>(b.left, x); b.top = std::min<LONG>(b.top, y);
                    b.right = std::max<LONG>(b.right, x + 1); b.bottom = std::max<LONG>(b.bottom, y + 1);
                }
            }
        if (b.right <= b.left) b = { 0, 0, 1, 1 };   // identical frame: 1 pixel
        rc = b;
    }
    int fw = rc.right - rc.left, fh = rc.bottom - rc.top;
    int ncolors = 1 << bpp_;
    int palColors = useDelta ? ncolors - 1 : ncolors;        // reserves an index for transparency

    // samples colors from the rectangle and builds the palette
    std::vector<uint32_t> sample;
    int step = std::max(1, (int)std::sqrt((double)fw * fh / 40000.0));
    for (int y = rc.top; y < rc.bottom; y += step)
        for (int x = rc.left; x < rc.right; x += step)
            if (!useDelta || changed[(size_t)y * w_ + x]) sample.push_back(frame.at(x, y));
    if (sample.empty()) sample.push_back(frame.at(rc.left, rc.top));
    std::vector<uint32_t> pal = MedianCut(sample, palColors);

    // 15-bit LUT → nearest index
    std::vector<uint8_t> lut(32768);
    for (int i = 0; i < 32768; i++) {
        int r = ((i >> 10) & 31) * 255 / 31, g = ((i >> 5) & 31) * 255 / 31, b = (i & 31) * 255 / 31;
        int best = 0, bd = INT_MAX;
        for (int k = 0; k < (int)pal.size(); k++) {
            int dr = r - CR(pal[k]), dg = g - CG(pal[k]), db = b - CB(pal[k]);
            int d = dr * dr * 2 + dg * dg * 4 + db * db * 3;
            if (d < bd) { bd = d; best = k; }
        }
        lut[i] = (uint8_t)best;
    }
    auto nearest = [&](int r, int g, int b) { return lut[((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3)]; };

    const uint8_t transIdx = (uint8_t)(ncolors - 1);
    std::vector<uint8_t> idx((size_t)fw * fh);
    std::vector<float> err0((fw + 2) * 3, 0), err1((fw + 2) * 3, 0);
    for (int y = 0; y < fh; y++) {
        std::fill(err1.begin(), err1.end(), 0.0f);
        for (int x = 0; x < fw; x++) {
            int gx = rc.left + x, gy = rc.top + y;
            size_t gi = (size_t)gy * w_ + gx;
            if (useDelta && !changed[gi]) { idx[(size_t)y * fw + x] = transIdx; continue; }
            uint32_t p = frame.px[gi];
            int r = CR(p), g = CG(p), b = CB(p);
            if (dither_) {
                r = std::clamp((int)(r + err0[(x + 1) * 3]), 0, 255);
                g = std::clamp((int)(g + err0[(x + 1) * 3 + 1]), 0, 255);
                b = std::clamp((int)(b + err0[(x + 1) * 3 + 2]), 0, 255);
            }
            uint8_t k = nearest(r, g, b);
            idx[(size_t)y * fw + x] = k;
            if (dither_) {          // Floyd–Steinberg
                float e[3] = { (float)(r - CR(pal[k])), (float)(g - CG(pal[k])), (float)(b - CB(pal[k])) };
                for (int c = 0; c < 3; c++) {
                    err0[(x + 2) * 3 + c] += e[c] * 7 / 16;
                    err1[(x) * 3 + c] += e[c] * 3 / 16;
                    err1[(x + 1) * 3 + c] += e[c] * 5 / 16;
                    err1[(x + 2) * 3 + c] += e[c] * 1 / 16;
                }
            }
        }
        std::swap(err0, err1);
    }
    // frame shown after this one (for the next delta)
    if (prev_.empty()) prev_.assign((size_t)w_ * h_, 0);
    for (int y = 0; y < fh; y++)
        for (int x = 0; x < fw; x++) {
            uint8_t k = idx[(size_t)y * fw + x];
            if (useDelta && k == transIdx) continue;
            prev_[(size_t)(rc.top + y) * w_ + rc.left + x] = pal[k];
        }

    // Graphic Control Extension
    PutByte(0x21); PutByte(0xF9); PutByte(4);
    PutByte((uint8_t)((1 << 2) | (useDelta ? 1 : 0)));        // disposal 1 (keep) + transparency flag
    PutWord((uint16_t)(delayCs > 0 ? delayCs : delay_));
    PutByte(useDelta ? transIdx : 0); PutByte(0);
    // Image Descriptor with a local palette
    PutByte(0x2C);
    PutWord((uint16_t)rc.left); PutWord((uint16_t)rc.top); PutWord((uint16_t)fw); PutWord((uint16_t)fh);
    PutByte((uint8_t)(0x80 | (bpp_ - 1)));
    for (int k = 0; k < ncolors; k++) {
        uint32_t c = k < (int)pal.size() ? pal[k] : 0;
        uint8_t rgb[3] = { (uint8_t)CR(c), (uint8_t)CG(c), (uint8_t)CB(c) };
        Put(rgb, 3);
    }
    int minCode = std::max(2, bpp_);
    PutByte((uint8_t)minCode);
    std::vector<uint8_t> data;
    Lzw(minCode, data).Encode(idx);
    for (size_t i = 0; i < data.size(); i += 255) {
        size_t n = std::min<size_t>(255, data.size() - i);
        PutByte((uint8_t)n); Put(data.data() + i, n);
    }
    PutByte(0);
    return !failed_;
}

bool GifWriter::Close() {
    if (file_ == INVALID_HANDLE_VALUE) return false;
    PutByte(0x3B);
    DWORD wr = 0;
    if (!buf_.empty() && !WriteFile(file_, buf_.data(), (DWORD)buf_.size(), &wr, nullptr)) failed_ = true;
    buf_.clear();
    CloseHandle(file_);
    file_ = INVALID_HANDLE_VALUE;
    return !failed_;
}

} // namespace snap
