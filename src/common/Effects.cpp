#include "Effects.h"
#include <cmath>
#include <random>

namespace snap::fx {

using namespace Gdiplus;

Image Rotate90(const Image& s, bool cw) {
    Image d(s.h, s.w);
    for (int y = 0; y < s.h; y++)
        for (int x = 0; x < s.w; x++) {
            if (cw) d.at(s.h - 1 - y, x) = s.at(x, y);
            else d.at(y, s.w - 1 - x) = s.at(x, y);
        }
    return d;
}

Image Rotate180(const Image& s) {
    Image d(s.w, s.h);
    for (size_t i = 0; i < s.px.size(); i++) d.px[s.px.size() - 1 - i] = s.px[i];
    return d;
}

Image FlipH(const Image& s) {
    Image d(s.w, s.h);
    for (int y = 0; y < s.h; y++)
        for (int x = 0; x < s.w; x++) d.at(s.w - 1 - x, y) = s.at(x, y);
    return d;
}

Image FlipV(const Image& s) {
    Image d(s.w, s.h);
    for (int y = 0; y < s.h; y++) memcpy(d.row(s.h - 1 - y), s.row(y), s.w * 4);
    return d;
}

Image RotateAngle(const Image& s, double deg, uint32_t bg) {
    double rad = deg * 3.14159265358979 / 180.0;
    double c = std::fabs(std::cos(rad)), sn = std::fabs(std::sin(rad));
    int nw = (int)std::ceil(s.w * c + s.h * sn), nh = (int)std::ceil(s.w * sn + s.h * c);
    Image d(nw, nh, bg);
    Image& src = const_cast<Image&>(s);
    auto sb = src.WrapGdip();
    auto db = d.WrapGdip();
    Graphics g(db.get());
    g.SetInterpolationMode(InterpolationModeHighQualityBicubic);
    g.SetSmoothingMode(SmoothingModeAntiAlias);
    g.TranslateTransform(nw / 2.0f, nh / 2.0f);
    g.RotateTransform((REAL)deg);
    g.DrawImage(sb.get(), -s.w / 2.0f, -s.h / 2.0f, (REAL)s.w, (REAL)s.h);
    return d;
}

Image CanvasResize(const Image& s, int nw, int nh, int anchor, uint32_t bg) {
    Image d(std::max(1, nw), std::max(1, nh), bg);
    int ax = anchor % 3, ay = anchor / 3;
    int ox = ax == 0 ? 0 : ax == 1 ? (nw - s.w) / 2 : nw - s.w;
    int oy = ay == 0 ? 0 : ay == 1 ? (nh - s.h) / 2 : nh - s.h;
    d.Blit(s, ox, oy, false);
    return d;
}

static bool Near(uint32_t a, uint32_t b, int tol) {
    return std::abs(CR(a) - CR(b)) <= tol && std::abs(CG(a) - CG(b)) <= tol && std::abs(CB(a) - CB(b)) <= tol &&
           std::abs(CA(a) - CA(b)) <= tol;
}

Image AutoTrim(const Image& s, int tol) {
    if (s.empty()) return s;
    uint32_t ref = s.at(0, 0);
    auto rowUniform = [&](int y) { for (int x = 0; x < s.w; x++) if (!Near(s.at(x, y), ref, tol)) return false; return true; };
    auto colUniform = [&](int x, int y0, int y1) { for (int y = y0; y < y1; y++) if (!Near(s.at(x, y), ref, tol)) return false; return true; };
    int top = 0, bottom = s.h, left = 0, right = s.w;
    while (top < bottom - 1 && rowUniform(top)) top++;
    while (bottom - 1 > top && rowUniform(bottom - 1)) bottom--;
    while (left < right - 1 && colUniform(left, top, bottom)) left++;
    while (right - 1 > left && colUniform(right - 1, top, bottom)) right--;
    return s.Crop({ left, top, right, bottom });
}

Image Scale(const Image& s, int percent) {
    if (percent == 100 || percent <= 0) return s;
    return ScaleImage(s, std::max(1, s.w * percent / 100), std::max(1, s.h * percent / 100));
}

void Grayscale(Image& img) {
    for (auto& p : img.px) {
        int l = (CR(p) * 299 + CG(p) * 587 + CB(p) * 114) / 1000;
        p = ARGB(CA(p), l, l, l);
    }
}

void Invert(Image& img) { for (auto& p : img.px) p = (p & 0xFF000000u) | (~p & 0x00FFFFFFu); }

void Sepia(Image& img) {
    for (auto& p : img.px) {
        int r = CR(p), g = CG(p), b = CB(p);
        int nr = std::min(255, (r * 393 + g * 769 + b * 189) / 1000);
        int ng = std::min(255, (r * 349 + g * 686 + b * 168) / 1000);
        int nb = std::min(255, (r * 272 + g * 534 + b * 131) / 1000);
        p = ARGB(CA(p), nr, ng, nb);
    }
}

void Monochrome(Image& img, int threshold) {
    for (auto& p : img.px) {
        int l = (CR(p) * 299 + CG(p) * 587 + CB(p) * 114) / 1000;
        int v = l >= threshold ? 255 : 0;
        p = ARGB(CA(p), v, v, v);
    }
}

void BrightnessContrast(Image& img, int b, int c) {
    double cf = (100.0 + c) / 100.0; cf *= cf;
    int lut[256];
    for (int i = 0; i < 256; i++) {
        double v = i + b * 2.55;
        v = ((v / 255.0 - 0.5) * cf + 0.5) * 255.0;
        lut[i] = std::clamp((int)std::lround(v), 0, 255);
    }
    for (auto& p : img.px) p = ARGB(CA(p), lut[CR(p)], lut[CG(p)], lut[CB(p)]);
}

static void RgbToHsl(int r, int g, int b, double& h, double& s, double& l) {
    double rf = r / 255.0, gf = g / 255.0, bf = b / 255.0;
    double mx = std::max({ rf, gf, bf }), mn = std::min({ rf, gf, bf });
    l = (mx + mn) / 2; h = s = 0;
    if (mx != mn) {
        double d = mx - mn;
        s = l > 0.5 ? d / (2 - mx - mn) : d / (mx + mn);
        if (mx == rf) h = (gf - bf) / d + (gf < bf ? 6 : 0);
        else if (mx == gf) h = (bf - rf) / d + 2;
        else h = (rf - gf) / d + 4;
        h /= 6;
    }
}
static double Hue2Rgb(double p, double q, double t) {
    if (t < 0) t += 1; if (t > 1) t -= 1;
    if (t < 1.0 / 6) return p + (q - p) * 6 * t;
    if (t < 0.5) return q;
    if (t < 2.0 / 3) return p + (q - p) * (2.0 / 3 - t) * 6;
    return p;
}

void HueSaturation(Image& img, int hueShift, int sat) {
    for (auto& p : img.px) {
        double h, s, l;
        RgbToHsl(CR(p), CG(p), CB(p), h, s, l);
        h = std::fmod(h + hueShift / 360.0 + 1.0, 1.0);
        s = std::clamp(s * (1.0 + sat / 100.0), 0.0, 1.0);
        double r, g, b;
        if (s == 0) r = g = b = l;
        else {
            double q = l < 0.5 ? l * (1 + s) : l + s - l * s, pp = 2 * l - q;
            r = Hue2Rgb(pp, q, h + 1.0 / 3); g = Hue2Rgb(pp, q, h); b = Hue2Rgb(pp, q, h - 1.0 / 3);
        }
        p = ARGB(CA(p), (int)(r * 255 + .5), (int)(g * 255 + .5), (int)(b * 255 + .5));
    }
}

void Gamma(Image& img, double gamma) {
    int lut[256];
    for (int i = 0; i < 256; i++) lut[i] = std::clamp((int)std::lround(std::pow(i / 255.0, 1.0 / gamma) * 255.0), 0, 255);
    for (auto& p : img.px) p = ARGB(CA(p), lut[CR(p)], lut[CG(p)], lut[CB(p)]);
}

void ReplaceColor(Image& img, uint32_t from, uint32_t to, int tol) {
    for (auto& p : img.px) if (Near(p | 0xFF000000u, from | 0xFF000000u, tol)) p = to;
}

static RECT ClipTo(const Image& img, RECT r) {
    r.left = std::max<LONG>(0, r.left); r.top = std::max<LONG>(0, r.top);
    r.right = std::min<LONG>(img.w, r.right); r.bottom = std::min<LONG>(img.h, r.bottom);
    return r;
}

void BoxBlur(Image& img, RECT r, int radius) {
    r = ClipTo(img, r);
    int w = r.right - r.left, h = r.bottom - r.top;
    if (w <= 1 || h <= 1 || radius < 1) return;
    std::vector<float> ch[4];
    for (int c = 0; c < 4; c++) ch[c].resize((size_t)w * h);
    for (int y = 0; y < h; y++)
        for (int x = 0; x < w; x++) {
            uint32_t p = img.at(r.left + x, r.top + y);
            float a = CA(p) / 255.0f;
            ch[0][(size_t)y * w + x] = CR(p) * a; ch[1][(size_t)y * w + x] = CG(p) * a;
            ch[2][(size_t)y * w + x] = CB(p) * a; ch[3][(size_t)y * w + x] = (float)CA(p);
        }
    std::vector<float> tmp(std::max(w, h));
    for (int pass = 0; pass < 3; pass++)
        for (int c = 0; c < 4; c++) {
            auto& a = ch[c];
            for (int y = 0; y < h; y++) {           // horizontal
                float* rowp = &a[(size_t)y * w];
                float sum = 0; int n = 0;
                for (int x = -radius; x <= radius; x++) { int xx = std::clamp(x, 0, w - 1); sum += rowp[xx]; n++; }
                for (int x = 0; x < w; x++) {
                    tmp[x] = sum / n;
                    sum += rowp[std::min(w - 1, x + radius + 1)] - rowp[std::max(0, x - radius)];
                }
                memcpy(rowp, tmp.data(), w * sizeof(float));
            }
            for (int x = 0; x < w; x++) {           // vertical
                float sum = 0; int n = 0;
                for (int y = -radius; y <= radius; y++) { int yy = std::clamp(y, 0, h - 1); sum += a[(size_t)yy * w + x]; n++; }
                for (int y = 0; y < h; y++) {
                    tmp[y] = sum / n;
                    sum += a[(size_t)std::min(h - 1, y + radius + 1) * w + x] - a[(size_t)std::max(0, y - radius) * w + x];
                }
                for (int y = 0; y < h; y++) a[(size_t)y * w + x] = tmp[y];
            }
        }
    for (int y = 0; y < h; y++)
        for (int x = 0; x < w; x++) {
            size_t i = (size_t)y * w + x;
            float al = ch[3][i];
            float inv = al > 0.5f ? 255.0f / al : 0;
            img.at(r.left + x, r.top + y) = ARGB((int)(al + .5f), std::clamp((int)(ch[0][i] * inv + .5f), 0, 255),
                                                 std::clamp((int)(ch[1][i] * inv + .5f), 0, 255),
                                                 std::clamp((int)(ch[2][i] * inv + .5f), 0, 255));
        }
}

void Pixelate(Image& img, RECT r, int block) {
    r = ClipTo(img, r);
    block = std::max(2, block);
    for (int by = r.top; by < r.bottom; by += block)
        for (int bx = r.left; bx < r.right; bx += block) {
            int ex = std::min<int>(bx + block, r.right), ey = std::min<int>(by + block, r.bottom);
            long long sr = 0, sg = 0, sb = 0, sa = 0, n = 0;
            for (int y = by; y < ey; y++)
                for (int x = bx; x < ex; x++) { uint32_t p = img.at(x, y); sr += CR(p); sg += CG(p); sb += CB(p); sa += CA(p); n++; }
            if (!n) continue;
            uint32_t v = ARGB((int)(sa / n), (int)(sr / n), (int)(sg / n), (int)(sb / n));
            for (int y = by; y < ey; y++) std::fill(img.row(y) + bx, img.row(y) + ex, v);
        }
}

Image Border(const Image& s, int width, uint32_t color) {
    Image d(s.w + 2 * width, s.h + 2 * width, color);
    d.Blit(s, width, width, false);
    return d;
}

Image Shadow(const Image& s, int offset, int blur, int opacity) {
    int pad = blur * 2;
    Image d(s.w + offset + pad * 2, s.h + offset + pad * 2, 0);
    Image sh(d.w, d.h, 0);
    for (int y = 0; y < s.h; y++)
        for (int x = 0; x < s.w; x++) {
            int a = CA(s.at(x, y)) * opacity / 100;
            sh.at(x + pad + offset, y + pad + offset) = ARGB(a, 0, 0, 0);
        }
    BoxBlur(sh, { 0, 0, sh.w, sh.h }, std::max(1, blur));
    d.Blit(sh, 0, 0, false);
    d.Blit(s, pad, pad, true);
    return d;
}

static double EdgeDepth(Edge st, int i, int size, std::mt19937& rng, double& walk) {
    switch (st) {
    case Edge::Torn: {
        std::uniform_real_distribution<double> u(-0.35, 0.35);
        walk = std::clamp(walk + u(rng) * size * 0.5, size * 0.15, (double)size);
        return walk;
    }
    case Edge::Wave: return size * 0.5 * (1 + std::sin(i * 2 * 3.14159265 / (size * 3.0)));
    case Edge::Sawtooth: { double per = size * 1.6; double t = std::fmod(i, per) / per; return size * (t < 0.5 ? t * 2 : (1 - t) * 2); }
    case Edge::SharkTooth: { double per = size * 1.8; double t = std::fmod(i, per) / per; return size * (1 - t); }
    default: return 0;
    }
}

Image EdgeEffect(const Image& s, Edge style, int size, int sides, bool shadow) {
    Image d = s;
    size = std::clamp(size, 2, std::max(2, std::min(s.w, s.h) / 3));
    std::mt19937 rng(1234);
    auto apply = [&](int len, auto setDepth) {
        double walk = size * 0.5;
        for (int i = 0; i < len; i++) setDepth(i, EdgeDepth(style, i, size, rng, walk));
    };
    if (style == Edge::Beveled) {
        // 3D bevel: light band at the top/left and dark band at the bottom/right, fading inward
        for (int y = 0; y < d.h; y++)
            for (int x = 0; x < d.w; x++) {
                double li = 0, dk = 0;
                if ((sides & Top) && y < size) li = std::max(li, 1.0 - y / (double)size);
                if ((sides & Left) && x < size) li = std::max(li, 1.0 - x / (double)size);
                if ((sides & Bottom) && d.h - 1 - y < size) dk = std::max(dk, 1.0 - (d.h - 1 - y) / (double)size);
                if ((sides & Right) && d.w - 1 - x < size) dk = std::max(dk, 1.0 - (d.w - 1 - x) / (double)size);
                if (li <= 0 && dk <= 0) continue;
                uint32_t& p = d.at(x, y);
                auto ch = [&](int c) { return std::clamp((int)std::lround(c + (255 - c) * 0.55 * li - c * 0.5 * dk), 0, 255); };
                p = ARGB(CA(p), ch(CR(p)), ch(CG(p)), ch(CB(p)));
            }
    } else if (style == Edge::Fade) {
        for (int y = 0; y < d.h; y++)
            for (int x = 0; x < d.w; x++) {
                double f = 1.0;
                if (sides & Left) f = std::min(f, x / (double)size);
                if (sides & Right) f = std::min(f, (d.w - 1 - x) / (double)size);
                if (sides & Top) f = std::min(f, y / (double)size);
                if (sides & Bottom) f = std::min(f, (d.h - 1 - y) / (double)size);
                f = std::clamp(f, 0.0, 1.0);
                uint32_t& p = d.at(x, y);
                p = (p & 0x00FFFFFFu) | ((uint32_t)(CA(p) * f) << 24);
            }
    } else {
        auto clearH = [&](int x, int y0, int y1) { for (int y = std::max(0, y0); y < std::min(d.h, y1); y++) d.at(x, y) = 0; };
        auto clearV = [&](int y, int x0, int x1) { for (int x = std::max(0, x0); x < std::min(d.w, x1); x++) d.at(x, y) = 0; };
        if (sides & Top) apply(d.w, [&](int i, double dep) { clearH(i, 0, (int)dep); });
        if (sides & Bottom) apply(d.w, [&](int i, double dep) { clearH(i, d.h - (int)dep, d.h); });
        if (sides & Left) apply(d.h, [&](int i, double dep) { clearV(i, 0, (int)dep); });
        if (sides & Right) apply(d.h, [&](int i, double dep) { clearV(i, d.w - (int)dep, d.w); });
    }
    return shadow ? Shadow(d, 5, 5, 45) : d;
}

// bilinear sample (outside the image = transparent)
static uint32_t Sample(const Image& s, double x, double y) {
    if (x < -0.5 || y < -0.5 || x > s.w - 0.5 || y > s.h - 0.5) return 0;
    int x0 = (int)std::floor(x), y0 = (int)std::floor(y);
    double fx = x - x0, fy = y - y0;
    auto at = [&](int xx, int yy) { return s.at(std::clamp(xx, 0, s.w - 1), std::clamp(yy, 0, s.h - 1)); };
    uint32_t p00 = at(x0, y0), p10 = at(x0 + 1, y0), p01 = at(x0, y0 + 1), p11 = at(x0 + 1, y0 + 1);
    auto mix = [&](int sh) {
        double a = ((p00 >> sh) & 255) * (1 - fx) + ((p10 >> sh) & 255) * fx;
        double b = ((p01 >> sh) & 255) * (1 - fx) + ((p11 >> sh) & 255) * fx;
        return (uint32_t)std::clamp((int)std::lround(a * (1 - fy) + b * fy), 0, 255) << sh;
    };
    return mix(24) | mix(16) | mix(8) | mix(0);
}

Image Perspective(const Image& s, int horizontal, int vertical, int shear) {
    // horizontal tilt: one side ends up lower than the other (trapezoid); vertical: same at the top/bottom;
    // shear: shifts the rows in proportion to the height
    Image d(s.w, s.h, 0);
    double hz = std::clamp(horizontal, -100, 100) / 100.0 * 0.6, vt = std::clamp(vertical, -100, 100) / 100.0 * 0.6;
    double sh = std::clamp(shear, -100, 100) / 100.0 * 0.5;
    double cx = s.w / 2.0, cy = s.h / 2.0;
    for (int y = 0; y < d.h; y++)
        for (int x = 0; x < d.w; x++) {
            double u = s.w > 1 ? x / (double)(s.w - 1) : 0, v = s.h > 1 ? y / (double)(s.h - 1) : 0;
            double sy = 1 - std::fabs(hz) * (hz > 0 ? u : 1 - u);       // vertical scale of each column
            double sx = 1 - std::fabs(vt) * (vt > 0 ? v : 1 - v);       // horizontal scale of each row
            double xs = (x - cx) / sx + cx - sh * (y - cy);
            double ys = (y - cy) / sy + cy;
            d.at(x, y) = Sample(s, xs, ys);
        }
    return d;
}

Image PageCurl(const Image& sIn, int size, int corner) {
    // always works on the bottom-right corner; the other corners are mirrored before and after
    bool fh = corner == 0 || corner == 3, fv = corner == 0 || corner == 1;
    Image s = sIn;
    if (fh) s = FlipH(s);
    if (fv) s = FlipV(s);
    size = std::clamp(size, 8, std::max(8, std::min(s.w, s.h) - 4));
    int W = s.w, H = s.h;
    // the corner beyond the fold line (x + y > W + H - size) disappears
    for (int y = std::max(0, H - size); y < H; y++)
        for (int x = std::max(0, W - size); x < W; x++)
            if (x + y > W + H - size) s.at(x, y) = 0;
    {
        auto bmp = s.WrapGdip();
        Graphics g(bmp.get());
        g.SetSmoothingMode(SmoothingModeAntiAlias);
        // the folded flap is the reflection of the removed corner over the fold line
        PointF a((REAL)(W - size), (REAL)H), b((REAL)W, (REAL)(H - size)), tip((REAL)(W - size), (REAL)(H - size));
        PointF shadowTri[3] = { a, b, PointF(tip.X - size * 0.08f, tip.Y - size * 0.08f) };
        SolidBrush shb(Color(70, 0, 0, 0));
        g.FillPolygon(&shb, shadowTri, 3);
        PointF flap[3] = { a, b, tip };
        PointF mid((a.X + b.X) / 2, (a.Y + b.Y) / 2);
        LinearGradientBrush lg(mid, tip, Color(255, 196, 198, 202), Color(255, 250, 250, 251));
        g.FillPolygon(&lg, flap, 3);
        Pen edge(Color(120, 120, 122, 128), 1.0f);
        g.DrawPolygon(&edge, flap, 3);
    }
    if (fv) s = FlipV(s);
    if (fh) s = FlipH(s);
    return s;
}

Image Spotlight(const Image& s, RECT f, int dimPercent, int blur, int zoomPercent, bool shadow) {
    f = ClipTo(s, f);
    if (f.right <= f.left || f.bottom <= f.top) return s;
    Image d = s;
    if (blur > 0) BoxBlur(d, { 0, 0, d.w, d.h }, blur);
    int dim = std::clamp(dimPercent, 0, 95);
    for (auto& p : d.px) p = ARGB(CA(p), CR(p) * (100 - dim) / 100, CG(p) * (100 - dim) / 100, CB(p) * (100 - dim) / 100);
    Image inner = s.Crop(f);
    int x = f.left, y = f.top;
    if (zoomPercent > 100) {
        int nw = inner.w * zoomPercent / 100, nh = inner.h * zoomPercent / 100;
        inner = ScaleImage(inner, nw, nh);
        x = (f.left + f.right) / 2 - nw / 2; y = (f.top + f.bottom) / 2 - nh / 2;
    }
    if (shadow) {
        Image sh(inner.w + 24, inner.h + 24, 0);
        for (int yy = 0; yy < inner.h; yy++) for (int xx = 0; xx < inner.w; xx++) sh.at(xx + 12, yy + 12) = ARGB(110, 0, 0, 0);
        BoxBlur(sh, { 0, 0, sh.w, sh.h }, 6);
        d.Blit(sh, x - 12 + 4, y - 12 + 5, true);
    }
    d.Blit(inner, x, y, true);
    return d;
}

void Emboss(Image& img, int depth) {
    Image s = img;
    double k = std::clamp(depth, 1, 10) / 2.0;
    auto lum = [&](int x, int y) { uint32_t p = s.at(std::clamp(x, 0, s.w - 1), std::clamp(y, 0, s.h - 1)); return 0.299 * CR(p) + 0.587 * CG(p) + 0.114 * CB(p); };
    for (int y = 0; y < img.h; y++)
        for (int x = 0; x < img.w; x++) {
            int v = std::clamp((int)std::lround(128 + (lum(x + 1, y + 1) - lum(x - 1, y - 1)) * k), 0, 255);
            img.at(x, y) = ARGB(CA(s.at(x, y)), v, v, v);
        }
}

void Sharpen(Image& img, int amount) {
    Image s = img;
    double k = std::clamp(amount, 0, 100) / 100.0;
    auto at = [&](int x, int y) { return s.at(std::clamp(x, 0, s.w - 1), std::clamp(y, 0, s.h - 1)); };
    for (int y = 0; y < img.h; y++)
        for (int x = 0; x < img.w; x++) {
            uint32_t c = at(x, y), l = at(x - 1, y), r = at(x + 1, y), u = at(x, y - 1), dn = at(x, y + 1);
            auto ch = [&](int sh) {
                int cc = (c >> sh) & 255;
                int nb = ((l >> sh) & 255) + ((r >> sh) & 255) + ((u >> sh) & 255) + ((dn >> sh) & 255);
                return std::clamp((int)std::lround(cc * (1 + 4 * k) - nb * k), 0, 255);
            };
            img.at(x, y) = ARGB(CA(c), ch(16), ch(8), ch(0));
        }
}

void Watermark(Image& img, const std::wstring& text, int opacity, bool tiled, bool emboss) {
    if (text.empty() || img.empty()) return;
    auto bmp = img.WrapGdip();
    Graphics g(bmp.get());
    g.SetTextRenderingHint(TextRenderingHintAntiAlias);
    g.SetSmoothingMode(SmoothingModeAntiAlias);
    REAL fs = (REAL)std::max(12, std::min(img.w, img.h) / (tiled ? 12 : 6));
    FontFamily ff(L"Segoe UI");
    Font font(&ff, fs, FontStyleBold, UnitPixel);
    int a = std::clamp(opacity, 0, 100) * 255 / 100;
    SolidBrush br(Color((BYTE)a, 255, 255, 255)), dark(Color((BYTE)a, 0, 0, 0));
    StringFormat sf; sf.SetAlignment(StringAlignmentCenter); sf.SetLineAlignment(StringAlignmentCenter);
    auto draw = [&](REAL x, REAL y) {
        RectF rc(x - 1000, y - fs, 2000, fs * 2);
        if (emboss) {
            RectF r2 = rc; r2.Offset(1.5f, 1.5f);
            g.DrawString(text.c_str(), -1, &font, r2, &sf, &dark);
            g.DrawString(text.c_str(), -1, &font, rc, &sf, &br);
        } else {
            RectF r2 = rc; r2.Offset(1, 1);
            g.DrawString(text.c_str(), -1, &font, r2, &sf, &dark);
            g.DrawString(text.c_str(), -1, &font, rc, &sf, &br);
        }
    };
    if (tiled) {
        g.TranslateTransform(img.w / 2.0f, img.h / 2.0f);
        g.RotateTransform(-30);
        REAL step = fs * 6, diag = (REAL)std::hypot(img.w, img.h);
        for (REAL y = -diag; y < diag; y += fs * 3)
            for (REAL x = -diag; x < diag; x += step) draw(x + (std::fmod(y / (fs * 3), 2.0f) != 0 ? step / 2 : 0), y);
    } else draw(img.w / 2.0f, img.h / 2.0f);
}

std::wstring ExpandCaptionTokens(const std::wstring& text) {
    SYSTEMTIME st; GetLocalTime(&st);
    std::wstring r = text;
    auto rep = [&](const std::wstring& k, const std::wstring& v) {
        for (size_t p; (p = r.find(k)) != std::wstring::npos;) r.replace(p, k.size(), v);
    };
    rep(L"{computer}", ComputerName());
    rep(L"{user}", UserName());
    rep(L"{date}", Format(L"%02d/%02d/%04d", st.wDay, st.wMonth, st.wYear));
    rep(L"{time}", Format(L"%02d:%02d:%02d", st.wHour, st.wMinute, st.wSecond));
    return r;
}

Image Caption(const Image& s, const std::wstring& textIn, bool top, int fontPx, uint32_t fg, uint32_t bg) {
    std::wstring text = ExpandCaptionTokens(textIn);
    int bar = fontPx * 2;
    Image d(s.w, s.h + bar, bg);
    d.Blit(s, 0, top ? bar : 0, false);
    auto bmp = d.WrapGdip();
    Graphics g(bmp.get());
    g.SetTextRenderingHint(TextRenderingHintAntiAlias);
    FontFamily ff(L"Segoe UI");
    Font font(&ff, (REAL)fontPx, FontStyleRegular, UnitPixel);
    SolidBrush br(Color(CA(fg), CR(fg), CG(fg), CB(fg)));
    StringFormat sf; sf.SetAlignment(StringAlignmentCenter); sf.SetLineAlignment(StringAlignmentCenter);
    sf.SetTrimming(StringTrimmingEllipsisCharacter);
    RectF rc(0, (REAL)(top ? 0 : s.h), (REAL)s.w, (REAL)bar);
    g.DrawString(text.c_str(), -1, &font, rc, &sf, &br);
    return d;
}

Image ApplyCaptureEffects(const Image& s, const CaptureEffects& e) {
    // geometry first, then color, text and finish (border, edge effects and shadow last)
    Image img = e.trim ? AutoTrim(s, e.trimTolerance) : s;
    if (e.rotate) {
        switch (e.rotateKind) {
        case 0: img = Rotate90(img, true); break;
        case 1: img = Rotate90(img, false); break;
        case 2: img = Rotate180(img); break;
        case 3: img = FlipH(img); break;
        default: img = FlipV(img); break;
        }
    }
    if (e.scale && e.scalePercent != 100) img = Scale(img, e.scalePercent);
    if (e.adjust) {
        if (e.brightness || e.contrast) BrightnessContrast(img, e.brightness, e.contrast);
        if (e.hue || e.saturation) HueSaturation(img, e.hue, e.saturation);
    }
    if (e.filter) {
        switch (e.filterKind) {
        case 0: Grayscale(img); break;
        case 1: Sepia(img); break;
        case 2: Invert(img); break;
        default: Monochrome(img); break;
        }
    }
    if (e.replaceColor) ReplaceColor(img, FromColorref(e.replaceFrom), FromColorref(e.replaceTo), e.replaceTolerance);
    if (e.watermark) Watermark(img, e.watermarkText, e.watermarkOpacity, e.watermarkTiled, false);
    if (e.caption) img = Caption(img, ExpandCaptionTokens(e.captionText), e.captionTop, 14, 0xFFFFFFFF, 0xFF202020);
    if (e.border) img = Border(img, e.borderWidth, FromColorref(e.borderColor));
    if (e.edge) img = EdgeEffect(img, (Edge)e.edgeStyle, e.edgeSize, e.edgeSides ? e.edgeSides : (Bottom | Right), false);
    if (e.shadow) img = Shadow(img, e.shadowOffset, e.shadowBlur, e.shadowOpacity);
    if (e.resolution) img.dpi = (float)e.dpi;
    return img;
}

} // namespace snap::fx
