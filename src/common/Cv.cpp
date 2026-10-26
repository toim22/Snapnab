#include "Cv.h"
#include <queue>
#include <complex>
#include <cmath>
#include <array>
#include <climits>

namespace snap::cv {

// ---------------------------------------------------------------- magic wand
Mask WandSelect(const Image& img, POINT seed, float tolerance, bool global) {
    Mask m((size_t)img.w * img.h, 0);
    if (img.empty() || seed.x < 0 || seed.y < 0 || seed.x >= img.w || seed.y >= img.h) return m;
    if (!(0.0f < tolerance && tolerance <= 100.0f)) tolerance = std::clamp(tolerance, 0.01f, 100.0f);
    const uint32_t s = img.at(seed.x, seed.y);
    const int limit = (int)std::lround(tolerance * 255.0f / 100.0f);
    // per-channel comparison scaled by the tolerance (the original's eq lambda)
    auto cost = [&](uint32_t p) {
        return std::max({ std::abs(CR(p) - CR(s)), std::abs(CG(p) - CG(s)), std::abs(CB(p) - CB(s)), std::abs(CA(p) - CA(s)) });
    };
    if (global) {
        for (size_t i = 0; i < img.px.size(); i++) if (cost(img.px[i]) <= limit) m[i] = 255;
        return m;
    }
    // queue ordered by cost (std::set<PixelCost> in the original)
    using Item = std::pair<int, int>;      // (cost, index)
    std::priority_queue<Item, std::vector<Item>, std::greater<Item>> pq;
    std::vector<uint8_t> seen(m.size(), 0);
    int si = seed.y * img.w + seed.x;
    pq.push({ 0, si }); seen[si] = 1;
    while (!pq.empty()) {
        auto [c, i] = pq.top(); pq.pop();
        m[i] = 255;
        int x = i % img.w, y = i / img.w;
        const int nx[4] = { x - 1, x + 1, x, x }, ny[4] = { y, y, y - 1, y + 1 };
        for (int k = 0; k < 4; k++) {
            if (nx[k] < 0 || ny[k] < 0 || nx[k] >= img.w || ny[k] >= img.h) continue;
            int j = ny[k] * img.w + nx[k];
            if (seen[j]) continue;
            seen[j] = 1;
            int cj = cost(img.px[j]);
            if (cj <= limit) pq.push({ cj, j });
        }
    }
    return m;
}

Mask DoWand(const Image& img, POINT pt, float tol, bool global) {
    if (pt.x < 0 || pt.y < 0 || pt.x >= img.w || pt.y >= img.h) return {};
    if (tol >= 0.01f && tol <= 100.0f) {
        if (tol == 100.0f) return Mask((size_t)img.w * img.h, 255);      // whole layer
    } else tol = tol < 0.01f ? 0.01f : 100.0f;
    return WandSelect(img, pt, tol, global);
}

// ---------------------------------------------------------------- masks
Mask RectMask(int w, int h, RECT r) {
    Mask m((size_t)w * h, 0);
    for (int y = std::max<LONG>(0, r.top); y < std::min<LONG>(h, r.bottom); y++)
        for (int x = std::max<LONG>(0, r.left); x < std::min<LONG>(w, r.right); x++) m[(size_t)y * w + x] = 255;
    return m;
}

Mask EllipseMask(int w, int h, RECT r) {
    Mask m((size_t)w * h, 0);
    double cx = (r.left + r.right) / 2.0, cy = (r.top + r.bottom) / 2.0;
    double rx = std::max(0.5, (r.right - r.left) / 2.0), ry = std::max(0.5, (r.bottom - r.top) / 2.0);
    for (int y = std::max<LONG>(0, r.top); y < std::min<LONG>(h, r.bottom); y++)
        for (int x = std::max<LONG>(0, r.left); x < std::min<LONG>(w, r.right); x++) {
            double dx = (x + 0.5 - cx) / rx, dy = (y + 0.5 - cy) / ry;
            if (dx * dx + dy * dy <= 1.0) m[(size_t)y * w + x] = 255;
        }
    return m;
}

Mask PolygonMask(int w, int h, const std::vector<POINT>& pts) {
    Mask m((size_t)w * h, 0);
    if (pts.size() < 3) return m;
    std::vector<double> xs;
    for (int y = 0; y < h; y++) {
        double sy = y + 0.5;
        xs.clear();
        for (size_t i = 0, j = pts.size() - 1; i < pts.size(); j = i++) {
            double y0 = pts[j].y, y1 = pts[i].y;
            if ((y0 <= sy && y1 > sy) || (y1 <= sy && y0 > sy))
                xs.push_back(pts[j].x + (sy - y0) / (y1 - y0) * (pts[i].x - pts[j].x));
        }
        std::sort(xs.begin(), xs.end());
        for (size_t k = 0; k + 1 < xs.size(); k += 2) {
            int x0 = std::max(0, (int)std::ceil(xs[k] - 0.5)), x1 = std::min(w - 1, (int)std::floor(xs[k + 1] - 0.5));
            for (int x = x0; x <= x1; x++) m[(size_t)y * w + x] = 255;
        }
    }
    return m;
}

RECT MaskBounds(const Mask& m, int w, int h) {
    RECT r{ w, h, 0, 0 };
    for (int y = 0; y < h; y++)
        for (int x = 0; x < w; x++)
            if (m[(size_t)y * w + x]) {
                r.left = std::min<LONG>(r.left, x); r.right = std::max<LONG>(r.right, x + 1);
                r.top = std::min<LONG>(r.top, y); r.bottom = std::max<LONG>(r.bottom, y + 1);
            }
    if (r.right <= r.left) r = { 0, 0, 0, 0 };
    return r;
}

bool MaskEmpty(const Mask& m) {
    for (uint8_t v : m) if (v) return false;
    return true;
}

Mask Dilate(const Mask& m, int w, int h, int k) {
    // separable dilation by a k×k rectangular element (getStructuringElement(MORPH_RECT))
    int r = std::max(1, k / 2);
    Mask t(m.size(), 0), o(m.size(), 0);
    for (int y = 0; y < h; y++) {
        int last = -1000000;
        for (int x = 0; x < w; x++) if (m[(size_t)y * w + x]) last = x; else if (x - last <= r) t[(size_t)y * w + x] = 255;
        last = 1000000;
        for (int x = w - 1; x >= 0; x--) if (m[(size_t)y * w + x]) { last = x; t[(size_t)y * w + x] = 255; } else if (last - x <= r) t[(size_t)y * w + x] = 255;
    }
    for (int x = 0; x < w; x++) {
        int last = -1000000;
        for (int y = 0; y < h; y++) if (t[(size_t)y * w + x]) { last = y; o[(size_t)y * w + x] = 255; } else if (y - last <= r) o[(size_t)y * w + x] = 255;
        last = 1000000;
        for (int y = h - 1; y >= 0; y--) if (t[(size_t)y * w + x]) last = y; else if (last - y <= r) o[(size_t)y * w + x] = 255;
    }
    return o;
}

// ---------------------------------------------------------------- inpainting
bool Dingo7Inpaint(Image& img, const Mask& mask, int kernel, uint32_t* dominantOut, double* shareOut) {
    Mask grown = Dilate(mask, img.w, img.h, kernel);
    std::map<uint32_t, int> hist;           // key = packed BGRA
    int total = 0;
    for (size_t i = 0; i < grown.size(); i++)
        if (grown[i] && !mask[i]) { hist[img.px[i]]++; total++; }  // only the surrounding ring (excludes the object itself)
    if (hist.empty()) return false;
    uint32_t dom = 0; int best = -1, bestBright = -1;
    for (auto& [c, n] : hist) {
        int bright = CA(c) + CR(c) + CG(c) + CB(c);       // tie-break by the sum of the 4 bytes
        if (n > best || (n == best && bright > bestBright)) { best = n; dom = c; bestBright = bright; }
    }
    for (size_t i = 0; i < mask.size(); i++) if (mask[i]) img.px[i] = dom;   // setTo(dominant, mask)
    if (dominantOut) *dominantOut = dom;
    if (shareOut) *shareOut = total ? (double)best / total : 0;
    return true;
}

void DiffusionInpaint(Image& img, const Mask& mask, int iterations) {
    const int w = img.w, h = img.h;
    RECT b = MaskBounds(mask, w, h);
    if (b.right <= b.left) return;
    std::vector<float> c[4];
    for (auto& v : c) v.assign((size_t)w * h, 0);
    std::vector<uint8_t> known((size_t)w * h, 1);
    for (size_t i = 0; i < mask.size(); i++) {
        if (mask[i]) { known[i] = 0; continue; }
        uint32_t p = img.px[i];
        c[0][i] = (float)CR(p); c[1][i] = (float)CG(p); c[2][i] = (float)CB(p); c[3][i] = (float)CA(p);
    }
    // "onion peel": fills from the border inward with the average of the known neighbors
    std::vector<int> frontier, next;
    for (int y = b.top; y < b.bottom; y++)
        for (int x = b.left; x < b.right; x++) {
            size_t i = (size_t)y * w + x;
            if (known[i]) continue;
            frontier.push_back((int)i);
        }
    while (!frontier.empty()) {
        next.clear();
        std::vector<std::pair<int, std::array<float, 4>>> upd;
        for (int i : frontier) {
            int x = i % w, y = i / w;
            float s[4] = { 0, 0, 0, 0 }; int n = 0;
            const int nx[8] = { x - 1, x + 1, x, x, x - 1, x + 1, x - 1, x + 1 }, ny[8] = { y, y, y - 1, y + 1, y - 1, y - 1, y + 1, y + 1 };
            for (int k = 0; k < 8; k++) {
                if (nx[k] < 0 || ny[k] < 0 || nx[k] >= w || ny[k] >= h) continue;
                size_t j = (size_t)ny[k] * w + nx[k];
                if (!known[j]) continue;
                for (int q = 0; q < 4; q++) s[q] += c[q][j];
                n++;
            }
            if (n) upd.push_back({ i, { s[0] / n, s[1] / n, s[2] / n, s[3] / n } });
            else next.push_back(i);
        }
        if (upd.empty()) break;
        for (auto& [i, v] : upd) { for (int q = 0; q < 4; q++) c[q][i] = v[q]; known[i] = 1; }
        frontier.swap(next);
    }
    // smoothing (Jacobi) only inside the mask
    if (iterations <= 0) iterations = std::clamp((int)std::max(b.right - b.left, b.bottom - b.top) / 2, 20, 150);
    for (int it = 0; it < iterations; it++)
        for (int y = std::max<LONG>(1, b.top); y < std::min<LONG>(h - 1, b.bottom); y++)
            for (int x = std::max<LONG>(1, b.left); x < std::min<LONG>(w - 1, b.right); x++) {
                size_t i = (size_t)y * w + x;
                if (!mask[i]) continue;
                for (int q = 0; q < 4; q++) c[q][i] = (c[q][i - 1] + c[q][i + 1] + c[q][i - w] + c[q][i + w]) * 0.25f;
            }
    for (size_t i = 0; i < mask.size(); i++)
        if (mask[i]) img.px[i] = ARGB((int)(c[3][i] + .5f), (int)(c[0][i] + .5f), (int)(c[1][i] + .5f), (int)(c[2][i] + .5f));
}

void SmartFill(Image& img, const Mask& mask) {
    Image probe = img;
    uint32_t dom; double share = 0;
    if (Dingo7Inpaint(probe, mask, 7, &dom, &share) && share >= 0.6) { img = std::move(probe); return; }
    DiffusionInpaint(img, mask);
}

// ---------------------------------------------------------------- Smart Move
float SmartMoveAutoTolerance(const Image& m) {
    if (m.empty()) return 10.0f;
    double sum = 0, sum2 = 0; size_t n = m.px.size();
    for (uint32_t p : m.px) { double l = (CR(p) + CG(p) + CB(p)) / 3.0; sum += l; sum2 += l * l; }
    float mean = (float)(sum / n);
    float sigma = (float)std::sqrt(std::max(0.0, sum2 / n - (sum / n) * (sum / n)));
    return ((sigma * 14.0f / 255.0f) + 1.0f) * (((255.0f - mean) * 3.0f / 255.0f) + 2.0f);   // range [2; 75]
}

float SmartMoveToleranceScaling(float t, float inMin, float inMax, float outMin, float outMax) {
    if (inMin != inMax) return outMin + (t - inMin) * (outMax - outMin) / (inMax - inMin);
    return inMin;
}

// ---------------------------------------------------------------- phase correlation
using cpx = std::complex<double>;

static void Fft1D(cpx* a, int n, bool inv, int stride) {
    for (int i = 1, j = 0; i < n; i++) {
        int bit = n >> 1;
        for (; j & bit; bit >>= 1) j ^= bit;
        j ^= bit;
        if (i < j) std::swap(a[i * stride], a[j * stride]);
    }
    for (int len = 2; len <= n; len <<= 1) {
        double ang = 2 * 3.14159265358979323846 / len * (inv ? 1 : -1);
        cpx wl(std::cos(ang), std::sin(ang));
        for (int i = 0; i < n; i += len) {
            cpx w(1);
            for (int j = 0; j < len / 2; j++) {
                cpx u = a[(i + j) * stride], v = a[(i + j + len / 2) * stride] * w;
                a[(i + j) * stride] = u + v;
                a[(i + j + len / 2) * stride] = u - v;
                w *= wl;
            }
        }
    }
    if (inv) for (int i = 0; i < n; i++) a[i * stride] /= n;
}

static void Fft2D(std::vector<cpx>& a, int W, int H, bool inv) {
    for (int y = 0; y < H; y++) Fft1D(&a[(size_t)y * W], W, inv, 1);
    for (int x = 0; x < W; x++) Fft1D(&a[x], H, inv, W);
}

static int NextPow2(int v) { int p = 1; while (p < v) p <<= 1; return p; }

POINT DetectTranslation(const Image& a, const Image& b, double* confidence) {
    if (a.empty() || b.empty() || a.w != b.w || a.h != b.h) return { 0, 0 };
    int f = 1;
    while (std::max(a.w, a.h) / f > 512) f *= 2;
    int sw = a.w / f, sh = a.h / f;
    int W = NextPow2(sw), H = NextPow2(sh);
    std::vector<cpx> A((size_t)W * H), B((size_t)W * H);
    auto gray = [&](const Image& im, std::vector<cpx>& out) {
        double mean = 0;
        std::vector<double> g((size_t)sw * sh);
        for (int y = 0; y < sh; y++)
            for (int x = 0; x < sw; x++) {
                double s = 0;
                for (int yy = 0; yy < f; yy++)
                    for (int xx = 0; xx < f; xx++) {
                        uint32_t p = im.at(x * f + xx, y * f + yy);
                        s += 0.114 * CB(p) + 0.587 * CG(p) + 0.299 * CR(p);   // BGR2GRAY
                    }
                g[(size_t)y * sw + x] = s / (f * f); mean += g[(size_t)y * sw + x];
            }
        mean /= g.size();
        for (int y = 0; y < sh; y++) {
            double wy = 0.5 - 0.5 * std::cos(2 * 3.14159265 * y / std::max(1, sh - 1));     // Hann window
            for (int x = 0; x < sw; x++) {
                double wx = 0.5 - 0.5 * std::cos(2 * 3.14159265 * x / std::max(1, sw - 1));
                out[(size_t)y * W + x] = (g[(size_t)y * sw + x] - mean) * wx * wy;
            }
        }
    };
    gray(a, A); gray(b, B);
    Fft2D(A, W, H, false); Fft2D(B, W, H, false);
    for (size_t i = 0; i < A.size(); i++) {
        cpx c = A[i] * std::conj(B[i]);                  // mulSpectrums(A, conj(B))
        double mag = std::abs(c);
        A[i] = c / (mag > 0 ? mag : 2.220446049250313e-16);   // DBL_EPSILON avoids division by zero
    }
    Fft2D(A, W, H, true);
    // correlation peaks (cv::minMaxIdx gives only the largest; we keep the 6 largest local maxima as candidates)
    double sum = 0;
    for (auto& c : A) sum += std::fabs(c.real());
    struct Peak { double v; int x, y; };
    std::vector<Peak> peaks;
    for (int y = 0; y < H; y++)
        for (int x = 0; x < W; x++) {
            double v = A[(size_t)y * W + x].real();
            bool localMax = true;
            for (int k = -1; k <= 1 && localMax; k++)
                for (int j = -1; j <= 1; j++) {
                    if (!k && !j) continue;
                    if (A[(size_t)((y + k + H) % H) * W + (x + j + W) % W].real() > v) { localMax = false; break; }
                }
            if (!localMax) continue;
            peaks.push_back({ v, x, y });
        }
    std::sort(peaks.begin(), peaks.end(), [](const Peak& p, const Peak& q) { return p.v > q.v; });
    if (peaks.size() > 6) peaks.resize(6);
    if (confidence) *confidence = (sum > 0 && !peaks.empty()) ? peaks[0].v / (sum / A.size()) : 0;

    // full-resolution SAD over a central band: b(p + d) = a(p)
    const int x0 = a.w / 4, x1 = a.w * 3 / 4;
    auto sadAt = [&](int dx, int dy) -> long long {
        int y0 = std::max(0, -dy), y1 = std::min(a.h, a.h - dy);
        if (y1 - y0 < a.h / 8 || std::abs(dx) >= a.w / 2) return LLONG_MAX;     // minimum overlap
        long long s = 0; int n = 0;
        int ys = std::max(1, (y1 - y0) / 400), xs = std::max(1, (x1 - x0) / 200);
        for (int y = y0; y < y1; y += ys)
            for (int x = std::max(x0, -dx); x < std::min(x1, a.w - dx); x += xs) {
                uint32_t p = a.at(x, y), q = b.at(x + dx, y + dy);
                s += std::abs(CR(p) - CR(q)) + std::abs(CG(p) - CG(q)) + std::abs(CB(p) - CB(q));
                n++;
            }
        return n > 50 ? s * 1000 / n : LLONG_MAX;
    };
    std::vector<POINT> cand;
    for (auto& p : peaks) {
        int px = p.x > W / 2 ? p.x - W : p.x, py = p.y > H / 2 ? p.y - H : p.y;
        cand.push_back({ -px * f, -py * f });          // Translation(-maxLoc.x, -maxLoc.y), at full scale
    }
    // extra candidates from the row profile (vertical scrolling): average luminance per row
    {
        std::vector<double> ra(a.h), rb(b.h);
        for (int y = 0; y < a.h; y++) {
            double sa = 0, sb = 0; int n = 0;
            for (int x = x0; x < x1; x += 2) {
                uint32_t p = a.at(x, y), q = b.at(x, y);
                sa += CR(p) + CG(p) + CB(p); sb += CR(q) + CG(q) + CB(q); n++;
            }
            ra[y] = sa / std::max(1, n); rb[y] = sb / std::max(1, n);
        }
        std::vector<std::pair<double, int>> sc;
        for (int d = -(a.h * 7 / 8); d <= a.h * 7 / 8; d++) {
            int y0 = std::max(0, -d), y1 = std::min(a.h, a.h - d);
            double s = 0;
            for (int y = y0; y < y1; y++) s += std::fabs(ra[y] - rb[y + d]);
            sc.push_back({ s / std::max(1, y1 - y0), d });
        }
        std::sort(sc.begin(), sc.end());
        for (size_t i = 0; i < sc.size() && i < 4; i++) cand.push_back({ 0, sc[i].second });
    }
    long long best = LLONG_MAX; POINT bestP = cand.empty() ? POINT{ 0, 0 } : cand[0];
    for (auto c : cand)
        for (int dy = c.y - f; dy <= c.y + f; dy++)
            for (int dx = c.x - (c.x ? f : 0); dx <= c.x + (c.x ? f : 0); dx++) {
                long long s = sadAt(dx, dy);
                // tie (periodic content): prefers the smallest offset
                bool tie = best != LLONG_MAX && std::llabs(s - best) * 100 <= best;
                if (s < best && !(tie && std::abs(dy) + std::abs(dx) > std::abs(bestP.y) + std::abs(bestP.x))) { best = s; bestP = { dx, dy }; }
                else if (tie && std::abs(dy) + std::abs(dx) < std::abs(bestP.y) + std::abs(bestP.x)) { best = std::min(best, s); bestP = { dx, dy }; }
            }
    return bestP;
}

} // namespace snap::cv
