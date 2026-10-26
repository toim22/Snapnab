// Object painters (PainterDC/GDI+; comp_5 §3.5 "PaintTools::IPainter", ed_comp_4 §6.3)
// Each vector object is drawn on its own layer with full colors; the shadow (GetDropShadowImage: color,
// blur and offset in the chosen direction) comes from that layer's alpha, and both are composited with the
// object's opacity — so overlapping parts (line + end, body + callout tip) do not get darker.
#include "Document.h"
#include "Effects.h"
#include "Emoji.h"
#include <cmath>

namespace snap::ed {

using namespace Gdiplus;

Color Gc(uint32_t argb, int opacity) {
    int a = (int)((argb >> 24) & 255) * std::clamp(opacity, 0, 100) / 100;
    return Color((BYTE)a, (BYTE)((argb >> 16) & 255), (BYTE)((argb >> 8) & 255), (BYTE)(argb & 255));
}

static PointF Sub(PointF a, PointF b) { return PointF(a.X - b.X, a.Y - b.Y); }
static PointF Add(PointF a, PointF b) { return PointF(a.X + b.X, a.Y + b.Y); }
static PointF Mul(PointF a, float k) { return PointF(a.X * k, a.Y * k); }
static float Len(PointF a) { return std::sqrt(a.X * a.X + a.Y * a.Y); }
static PointF Norm(PointF a) { float l = Len(a); return l > 1e-4f ? PointF(a.X / l, a.Y / l) : PointF(1, 0); }
static PointF Center(RectF r) { return PointF(r.X + r.Width / 2, r.Y + r.Height / 2); }
static COLORREF Lighter(COLORREF c) { return RGB(GetRValue(c) + (255 - GetRValue(c)) * 2 / 5, GetGValue(c) + (255 - GetGValue(c)) * 2 / 5, GetBValue(c) + (255 - GetBValue(c)) * 2 / 5); }

static void AddRoundRect(GraphicsPath& gp, RectF r, float rad) {
    rad = std::min({ rad, r.Width / 2, r.Height / 2 });
    if (rad < 1) { gp.AddRectangle(r); return; }
    float d = rad * 2;
    gp.AddArc(r.X, r.Y, d, d, 180, 90);
    gp.AddArc(r.GetRight() - d, r.Y, d, d, 270, 90);
    gp.AddArc(r.GetRight() - d, r.GetBottom() - d, d, d, 0, 90);
    gp.AddArc(r.X, r.GetBottom() - d, d, d, 90, 90);
    gp.CloseFigure();
}

void ApplyDash(Pen& p, int dash) {
    switch (dash) {
    case DashDash: { REAL d[] = { 3.0f, 2.0f }; p.SetDashPattern(d, 2); break; }
    case DashDot: { REAL d[] = { 0.25f, 1.9f }; p.SetDashPattern(d, 2); p.SetDashCap(DashCapRound); break; }
    case DashDashDot: { REAL d[] = { 3.0f, 1.6f, 0.25f, 1.6f }; p.SetDashPattern(d, 4); p.SetDashCap(DashCapRound); break; }
    case DashDashDotDot: { REAL d[] = { 3.0f, 1.5f, 0.25f, 1.5f, 0.25f, 1.5f }; p.SetDashPattern(d, 6); p.SetDashCap(DashCapRound); break; }
    case DashLong: { REAL d[] = { 6.0f, 2.5f }; p.SetDashPattern(d, 2); break; }
    default: break;
    }
}

// ---------------------------------------------------------------- shapes
void ShapePath(GraphicsPath& gp, int kind, RectF r) {
    float l = r.X, t = r.Y, w = r.Width, h = r.Height, rr = l + w, b = t + h, cx = l + w / 2, cy = t + h / 2;
    switch (kind) {
    case ShapeRoundRect: AddRoundRect(gp, r, std::min(w, h) * 0.18f); break;
    case ShapeEllipse: gp.AddEllipse(r); break;
    case ShapeTriangle: { PointF p[] = { { cx, t }, { rr, b }, { l, b } }; gp.AddPolygon(p, 3); break; }
    case ShapeDiamond: { PointF p[] = { { cx, t }, { rr, cy }, { cx, b }, { l, cy } }; gp.AddPolygon(p, 4); break; }
    case ShapeParallelogram: { float k = w * 0.22f; PointF p[] = { { l + k, t }, { rr, t }, { rr - k, b }, { l, b } }; gp.AddPolygon(p, 4); break; }
    case ShapeHexagon: {
        float k = w * 0.25f;
        PointF p[] = { { l + k, t }, { rr - k, t }, { rr, cy }, { rr - k, b }, { l + k, b }, { l, cy } };
        gp.AddPolygon(p, 6);
        break;
    }
    case ShapeStar: {
        PointF p[10];
        for (int i = 0; i < 10; i++) {
            double a = -3.14159265 / 2 + i * 3.14159265 / 5;
            float k = i % 2 ? 0.40f : 1.0f;
            p[i] = PointF(cx + (float)std::cos(a) * w / 2 * k, cy + (float)std::sin(a) * h / 2 * k + h * 0.05f);
        }
        gp.AddPolygon(p, 10);
        break;
    }
    default: gp.AddRectangle(r); break;
    }
}

// ---------------------------------------------------------------- gradients
// color of the gradient at t (0..1), interpolated between the stops
static uint32_t GradColorAt(const Gradient& g, float t) {
    const auto& st = g.stops;
    if (st.empty()) return 0;
    if (t <= st.front().pos) return st.front().color;
    if (t >= st.back().pos) return st.back().color;
    for (size_t i = 1; i < st.size(); i++) {
        if (t > st[i].pos) continue;
        float span = std::max(1e-4f, st[i].pos - st[i - 1].pos), k = (t - st[i - 1].pos) / span;
        uint32_t a = st[i - 1].color, b = st[i].color;
        auto mix = [&](int sh) { return (uint32_t)std::lround(((a >> sh) & 255) + (((int)((b >> sh) & 255) - (int)((a >> sh) & 255)) * k)) << sh; };
        return mix(24) | mix(16) | mix(8) | mix(0);
    }
    return st.back().color;
}

// stops as GDI+ wants them: from 0 to 1, with opacity applied
static void GradArrays(const Gradient& g, int opacity, bool reverse, std::vector<Color>& cols, std::vector<REAL>& pos) {
    std::vector<GradStop> st = g.stops;
    if (st.front().pos > 0) st.insert(st.begin(), { 0, st.front().color });
    if (st.back().pos < 1) st.push_back({ 1, st.back().color });
    if (reverse) { std::reverse(st.begin(), st.end()); for (auto& x : st) x.pos = 1 - x.pos; }
    for (auto& x : st) { cols.push_back(Gc(x.color, opacity)); pos.push_back(std::clamp(x.pos, 0.0f, 1.0f)); }
    pos.front() = 0; pos.back() = 1;
}

// brush for a fill: solid color, or the linear (at an angle) / radial gradient spread over 'r'
std::unique_ptr<Brush> FillBrush(uint32_t color, const Gradient& g, RectF r, int opacity) {
    if (!g.Active() || r.Width < 1 || r.Height < 1) return std::make_unique<SolidBrush>(Gc(color, opacity));
    std::vector<Color> cols; std::vector<REAL> pos;
    if (g.type == 2) {                                      // radial: from the center (stop 0) to the corners (stop 1)
        GradArrays(g, opacity, true, cols, pos);
        RectF e = r; e.Inflate(r.Width * 0.2072f, r.Height * 0.2072f);    // ellipse through the corners (√2)
        GraphicsPath gp; gp.AddEllipse(e);
        auto b = std::make_unique<PathGradientBrush>(&gp);
        b->SetCenterPoint(Center(r));
        b->SetInterpolationColors(cols.data(), pos.data(), (INT)cols.size());
        return b;
    }
    GradArrays(g, opacity, false, cols, pos);
    RectF e = r; e.Inflate(1, 1);
    auto b = std::make_unique<LinearGradientBrush>(e, cols.front(), cols.back(), (REAL)g.angle, TRUE);
    b->SetInterpolationColors(cols.data(), pos.data(), (INT)cols.size());
    b->SetWrapMode(WrapModeTileFlipXY);
    return b;
}

// gradient painted (blended) on the pixels of the mask, spread over 'bounds' (Fill tool, canvas color)
void FillGradientMask(Image& img, const std::vector<uint8_t>& mask, RECT b, const Gradient& g, int opacity) {
    if (!g.Active() || b.right <= b.left || b.bottom <= b.top) return;
    float w = (float)(b.right - b.left), h = (float)(b.bottom - b.top), cx = b.left + w / 2, cy = b.top + h / 2;
    double a = g.angle * 3.14159265358979 / 180;
    float ux = (float)std::cos(a), uy = (float)std::sin(a);
    float half = (std::fabs(ux) * w + std::fabs(uy) * h) / 2, rad = std::sqrt(w * w + h * h) / 2;
    for (int y = std::max(0L, b.top); y < std::min((LONG)img.h, b.bottom); y++)
        for (int x = std::max(0L, b.left); x < std::min((LONG)img.w, b.right); x++) {
            int m = mask.empty() ? 255 : mask[(size_t)y * img.w + x];
            if (!m) continue;
            float dx = x + 0.5f - cx, dy = y + 0.5f - cy;
            float t = g.type == 2 ? std::sqrt(dx * dx + dy * dy) / std::max(1.0f, rad) : (dx * ux + dy * uy) / std::max(1.0f, 2 * half) + 0.5f;
            uint32_t c = GradColorAt(g, std::clamp(t, 0.0f, 1.0f));
            int al = (int)CA(c) * std::clamp(opacity, 0, 100) / 100 * m / 255;
            if (al) img.at(x, y) = BlendOver(img.at(x, y), (c & 0x00FFFFFF) | ((uint32_t)al << 24));
        }
}

// outline of a highlighter / spotlight / hide area: one of the shapes or the clicked polygon / freehand outline
void AreaPath(GraphicsPath& gp, const DrawObj& o) {
    if (o.IsPointShape()) {
        if (o.style.areaShape == AreaFreehand && o.pts.size() >= 3) gp.AddClosedCurve(o.pts.data(), (INT)o.pts.size(), 0.3f);
        else gp.AddPolygon(o.pts.data(), (INT)o.pts.size());
        return;
    }
    // a polygon / freehand area without its points yet is drawn as a rectangle
    ShapePath(gp, IsPointArea(o.style.areaShape) ? ShapeRect : std::clamp(o.style.areaShape, 0, ShapeCount - 1), o.Rect());
}

// ---------------------------------------------------------------- callouts (CalloutObject / RemixCallouts)
// 24 shapes, in gallery order: speech, clouds, box with arrow, arrows, rectangles, note, explosion, thought, whisper.
enum class Body { RoundSpeech, Rect, RoundRect, Ellipse, Cloud, Note, Burst, Tag, Block, Notch, Fat, Double, Chevron, ArrowBox };
enum class Tail { None, Wedge, Bubbles, Leader, Direction };
struct CalloutDef { Body body; Tail tail; bool dashed; };
static const CalloutDef kCallouts[kCalloutShapes] = {
    { Body::RoundSpeech, Tail::Wedge, false }, { Body::Rect, Tail::Wedge, false }, { Body::Cloud, Tail::Wedge, false },
    { Body::Cloud, Tail::Bubbles, false }, { Body::Rect, Tail::Leader, false }, { Body::Tag, Tail::Direction, false },
    { Body::Block, Tail::Direction, false }, { Body::Notch, Tail::Direction, false }, { Body::Fat, Tail::Direction, false },
    { Body::Double, Tail::Direction, false }, { Body::Chevron, Tail::Direction, false }, { Body::ArrowBox, Tail::Direction, false },
    { Body::Rect, Tail::Wedge, false }, { Body::RoundRect, Tail::Wedge, false }, { Body::Ellipse, Tail::Wedge, false },
    { Body::RoundRect, Tail::Leader, false }, { Body::Rect, Tail::None, false }, { Body::RoundRect, Tail::None, false },
    { Body::Ellipse, Tail::None, false }, { Body::Cloud, Tail::None, false }, { Body::Note, Tail::None, false },
    { Body::Burst, Tail::None, false }, { Body::Ellipse, Tail::Bubbles, false }, { Body::Ellipse, Tail::Wedge, true },
};
static const CalloutDef& Def(int shape) { return kCallouts[std::clamp(shape, 0, kCalloutShapes - 1)]; }

static bool IsArrowBody(Body b) { return b >= Body::Tag; }

// arrow direction from the draggable tip: 0 right, 1 down, 2 left, 3 up
static int ArrowDir(const DrawObj& o) {
    PointF c = Center(o.Rect()), d = Sub(o.tail, c);
    if (std::fabs(d.X) < 1 && std::fabs(d.Y) < 1) return 0;
    if (std::fabs(d.X) >= std::fabs(d.Y)) return d.X >= 0 ? 0 : 2;
    return d.Y >= 0 ? 1 : 3;
}

static void ArrowBodyPath(GraphicsPath& gp, Body body, RectF r, int dir) {
    bool vert = dir == 1 || dir == 3;
    float W = vert ? r.Height : r.Width, H = vert ? r.Width : r.Height;
    std::vector<PointF> p;
    float h2 = std::min(H * 0.5f, W * 0.35f);
    switch (body) {
    case Body::Tag: p = { { 0, 0 }, { W - h2, 0 }, { W, H / 2 }, { W - h2, H }, { 0, H } }; break;
    case Body::Notch: p = { { 0, 0 }, { W - h2, 0 }, { W, H / 2 }, { W - h2, H }, { 0, H }, { h2 * 0.6f, H / 2 } }; break;
    case Body::Chevron: p = { { 0, 0 }, { W - h2, 0 }, { W, H / 2 }, { W - h2, H }, { 0, H }, { h2, H / 2 } }; break;
    case Body::Block: { float hd = std::min(W * 0.4f, H * 0.9f); p = { { 0, H * 0.25f }, { W - hd, H * 0.25f }, { W - hd, 0 }, { W, H / 2 }, { W - hd, H }, { W - hd, H * 0.75f }, { 0, H * 0.75f } }; break; }
    case Body::Fat: { float hd = std::min(W * 0.5f, H); p = { { 0, H * 0.15f }, { W - hd, H * 0.15f }, { W - hd, 0 }, { W, H / 2 }, { W - hd, H }, { W - hd, H * 0.85f }, { 0, H * 0.85f } }; break; }
    case Body::Double: {
        float hd = std::min(W * 0.28f, H * 0.8f);
        p = { { 0, H / 2 }, { hd, 0 }, { hd, H * 0.25f }, { W - hd, H * 0.25f }, { W - hd, 0 }, { W, H / 2 }, { W - hd, H }, { W - hd, H * 0.75f }, { hd, H * 0.75f }, { hd, H } };
        break;
    }
    case Body::ArrowBox: { float k = std::min(W * 0.2f, H * 0.4f); p = { { 0, 0 }, { W - k, 0 }, { W - k, H * 0.3f }, { W, H / 2 }, { W - k, H * 0.7f }, { W - k, H }, { 0, H } }; break; }
    default: break;
    }
    GraphicsPath local;
    local.AddPolygon(p.data(), (INT)p.size());
    Matrix m;
    switch (dir) {
    case 1: m.Translate(r.X + r.Width, r.Y); m.Rotate(90); break;
    case 2: m.Translate(r.X + r.Width, r.Y + r.Height); m.Rotate(180); break;
    case 3: m.Translate(r.X, r.Y + r.Height); m.Rotate(-90); break;
    default: m.Translate(r.X, r.Y); break;
    }
    local.Transform(&m);
    gp.AddPath(&local, FALSE);
}

static void CloudPath(GraphicsPath& gp, RectF r) {
    PointF c = Center(r);
    int n = 9;
    for (int i = 0; i < n; i++) {
        double a = i * 2 * 3.14159265 / n + 0.3;
        float ex = c.X + (float)std::cos(a) * r.Width * 0.34f, ey = c.Y + (float)std::sin(a) * r.Height * 0.32f;
        gp.AddEllipse(ex - r.Width * 0.18f, ey - r.Height * 0.21f, r.Width * 0.36f, r.Height * 0.42f);
    }
    gp.AddEllipse(r.X + r.Width * 0.14f, r.Y + r.Height * 0.14f, r.Width * 0.72f, r.Height * 0.72f);
}

void CalloutBodyPath(GraphicsPath& gp, int shape, RectF r) {
    const CalloutDef& d = Def(shape);
    switch (d.body) {
    case Body::RoundSpeech: AddRoundRect(gp, r, std::min(r.Width, r.Height) * 0.45f); break;
    case Body::Rect: gp.AddRectangle(r); break;
    case Body::RoundRect: AddRoundRect(gp, r, std::min(r.Width, r.Height) * 0.2f); break;
    case Body::Ellipse: gp.AddEllipse(r); break;
    case Body::Cloud: CloudPath(gp, r); break;
    case Body::Note: {
        float f = std::min(r.Width, r.Height) * 0.22f;
        PointF p[] = { { r.X + f, r.Y }, { r.GetRight(), r.Y }, { r.GetRight(), r.GetBottom() }, { r.X, r.GetBottom() }, { r.X, r.Y + f } };
        gp.AddPolygon(p, 5);
        break;
    }
    case Body::Burst: {
        PointF c = Center(r);
        const int n = 14;
        PointF p[n * 2];
        for (int i = 0; i < n * 2; i++) {
            double a = -3.14159265 / 2 + i * 3.14159265 / n;
            float k = i % 2 ? (0.62f + 0.08f * ((i / 2) % 3)) : 1.0f;
            p[i] = PointF(c.X + (float)std::cos(a) * r.Width / 2 * k, c.Y + (float)std::sin(a) * r.Height / 2 * k);
        }
        gp.AddPolygon(p, n * 2);
        break;
    }
    default: ArrowBodyPath(gp, d.body, r, 0); break;
    }
}

// body + tip (wedge or bubbles) in a single "winding" fill path
static void CalloutFullPath(GraphicsPath& gp, const DrawObj& o) {
    const CalloutDef& d = Def(o.style.calloutShape);
    RectF r = o.Rect();
    if (IsArrowBody(d.body)) { ArrowBodyPath(gp, d.body, r, ArrowDir(o)); return; }
    CalloutBodyPath(gp, o.style.calloutShape, r);
    PointF c = Center(r);
    if (d.tail == Tail::Bubbles) {
        for (int i = 1; i <= 3; i++) {
            float t = i / 4.0f, s = std::max(3.0f, std::min(r.Width, r.Height) * 0.09f * (1.25f - t));
            PointF q(c.X + (o.tail.X - c.X) * (0.55f + t * 0.45f), c.Y + (o.tail.Y - c.Y) * (0.55f + t * 0.45f));
            gp.AddEllipse(q.X - s, q.Y - s, s * 2, s * 2);
        }
        return;
    }
    if (d.tail != Tail::Wedge || r.Contains(o.tail)) return;
    PointF dv = Sub(o.tail, c);
    float len = Len(dv);
    if (len < 1) return;
    PointF n(-dv.Y / len, dv.X / len);
    float base = std::max(6.0f, std::min(r.Width, r.Height) * (d.body == Body::Rect ? 0.12f : 0.17f));
    PointF tri[3] = { Add(c, Mul(n, base)), o.tail, Sub(c, Mul(n, base)) };
    float cross = (tri[1].X - tri[0].X) * (tri[2].Y - tri[0].Y) - (tri[1].Y - tri[0].Y) * (tri[2].X - tri[0].X);
    if (cross < 0) std::swap(tri[0], tri[2]);
    GraphicsPath t; t.AddPolygon(tri, 3);
    gp.AddPath(&t, FALSE);
}

RectF CalloutTextRect(const DrawObj& o) {
    const CalloutDef& d = Def(o.style.calloutShape);
    RectF r = o.Rect();
    float m = std::min(r.Width, r.Height);
    float in = o.style.padding + std::min(o.style.width, 8.0f) / 2;
    switch (d.body) {
    case Body::Ellipse: case Body::Cloud: in += m * 0.15f; r.Inflate(-r.Width * 0.06f, 0); break;
    case Body::Burst: in += m * 0.22f; r.Inflate(-r.Width * 0.08f, 0); break;
    case Body::RoundSpeech: r.Inflate(-m * 0.12f, 0); break;
    case Body::Note: r.Y += m * 0.12f; r.Height -= m * 0.12f; break;
    default: break;
    }
    if (IsArrowBody(d.body)) {
        int dir = ArrowDir(o);
        bool both = d.body == Body::Double;
        float hw = (dir == 0 || dir == 2 ? r.Width : r.Height) * 0.22f;
        if (dir == 0 || both) { if (dir == 0 || dir == 2) r.Width -= hw; else r.Height -= hw; }
        if (dir == 2 || both) { if (dir == 0 || dir == 2) { r.X += hw; r.Width -= hw; } else { r.Y += hw; r.Height -= hw; } }
        if (!both && dir == 1) r.Height -= hw;
        if (!both && dir == 3) { r.Y += hw; r.Height -= hw; }
    }
    r.Inflate(-in, -in);
    if (r.Width < 4) { r.X -= (4 - r.Width) / 2; r.Width = 4; }
    if (r.Height < 4) { r.Y -= (4 - r.Height) / 2; r.Height = 4; }
    return r;
}

// point on the rectangle border in the direction of 'to' (for the arrow of the box with a line)
static PointF RectEdge(RectF r, PointF to) {
    PointF c = Center(r), d = Sub(to, c);
    if (std::fabs(d.X) < 1e-3f && std::fabs(d.Y) < 1e-3f) return c;
    float sx = d.X != 0 ? (r.Width / 2) / std::fabs(d.X) : 1e9f, sy = d.Y != 0 ? (r.Height / 2) / std::fabs(d.Y) : 1e9f;
    float s = std::min(sx, sy);
    return Add(c, Mul(d, std::min(s, 1.0f)));
}

// ---------------------------------------------------------------- lines and ends
static float CapInset(int cap, float len, float w) {
    switch (cap) {
    case CapArrow: return len * 0.8f;
    case CapArrowWide: return len * 0.55f;
    case CapArrowOpen: return w * 0.4f;
    default: return 0;
    }
}

static void DrawCap(Graphics& g, int cap, PointF tip, PointF dir, float w, float len, Color col) {
    PointF n(-dir.Y, dir.X);
    SolidBrush br(col);
    switch (cap) {
    case CapArrow: {
        float half = len * 0.55f;
        PointF base = Sub(tip, Mul(dir, len));
        PointF p[3] = { tip, Add(base, Mul(n, half)), Sub(base, Mul(n, half)) };
        g.FillPolygon(&br, p, 3);
        break;
    }
    case CapArrowWide: {
        float half = len * 0.62f;
        PointF base = Sub(tip, Mul(dir, len)), notch = Sub(tip, Mul(dir, len * 0.6f));
        PointF p[4] = { tip, Add(base, Mul(n, half)), notch, Sub(base, Mul(n, half)) };
        g.FillPolygon(&br, p, 4);
        break;
    }
    case CapArrowOpen: {
        float half = len * 0.5f;
        PointF base = Sub(tip, Mul(dir, len * 0.85f));
        Pen p(col, std::max(1.0f, w * 0.9f));
        p.SetStartCap(LineCapRound); p.SetEndCap(LineCapRound); p.SetLineJoin(LineJoinRound);
        PointF pts[3] = { Add(base, Mul(n, half)), tip, Sub(base, Mul(n, half)) };
        g.DrawLines(&p, pts, 3);
        break;
    }
    case CapDot: {
        float r = std::max(w * 0.9f, len * 0.36f);
        g.FillEllipse(&br, tip.X - r, tip.Y - r, r * 2, r * 2);
        break;
    }
    case CapSquare: {
        float s = std::max(w * 1.6f, len * 0.6f) / 2;
        PointF p[4] = { Add(Add(tip, Mul(dir, s)), Mul(n, s)), Sub(Add(tip, Mul(dir, s)), Mul(n, s)),
                        Sub(Sub(tip, Mul(dir, s)), Mul(n, s)), Add(Sub(tip, Mul(dir, s)), Mul(n, s)) };
        g.FillPolygon(&br, p, 4);
        break;
    }
    case CapDiamond: {
        float l = std::max(w * 2.0f, len * 0.75f) / 2;
        PointF p[4] = { Add(tip, Mul(dir, l)), Add(tip, Mul(n, l * 0.7f)), Sub(tip, Mul(dir, l)), Sub(tip, Mul(n, l * 0.7f)) };
        g.FillPolygon(&br, p, 4);
        break;
    }
    default: break;
    }
}

// straight or curved line (quadratic through 'ctrl') with ends and dash pattern
static void StrokeLine(Graphics& g, PointF a, PointF b, bool curve, PointF ctrl, Color col, float w, int capA, int capB,
                       float sizeA, float sizeB, int dash) {
    w = std::max(0.5f, w);
    PointF da = Norm(curve ? Sub(a, ctrl) : Sub(a, b)), db = Norm(curve ? Sub(b, ctrl) : Sub(b, a));
    float total = curve ? Len(Sub(a, ctrl)) + Len(Sub(b, ctrl)) : Len(Sub(b, a));
    if (total < 0.5f) return;
    float la = CapLength(w, sizeA), lb = CapLength(w, sizeB);
    float ia = CapInset(capA, la, w), ib = CapInset(capB, lb, w);
    if (ia + ib > total * 0.9f) { float k = total * 0.9f / std::max(0.1f, ia + ib); ia *= k; ib *= k; la *= std::min(1.0f, k * 1.1f); lb *= std::min(1.0f, k * 1.1f); }
    PointF a2 = Sub(a, Mul(da, ia)), b2 = Sub(b, Mul(db, ib));
    Pen p(col, w);
    ApplyDash(p, dash);
    p.SetLineJoin(LineJoinRound);
    p.SetStartCap(capA == CapRound || capA == CapArrowOpen ? LineCapRound : LineCapFlat);
    p.SetEndCap(capB == CapRound || capB == CapArrowOpen ? LineCapRound : LineCapFlat);
    if (dash != DashSolid) p.SetDashCap(capA == CapRound ? DashCapRound : p.GetDashCap());
    if (curve) {
        PointF c1 = Add(a2, Mul(Sub(ctrl, a2), 2.0f / 3)), c2 = Add(b2, Mul(Sub(ctrl, b2), 2.0f / 3));
        g.DrawBezier(&p, a2, c1, c2, b2);
    } else g.DrawLine(&p, a2, b2);
    DrawCap(g, capA, a, da, w, la, col);
    DrawCap(g, capB, b, db, w, lb, col);
}

void DrawCapGlyph(Graphics& g, int cap, RectF r, Color c, bool atStart) {
    float y = r.Y + r.Height / 2, w = std::max(2.0f, r.Height * 0.16f);
    PointF a(r.X + r.Width * 0.12f, y), b(r.GetRight() - r.Width * 0.12f, y);
    if (atStart) StrokeLine(g, a, b, false, a, c, w, cap, CapFlat, 2.4f, 2.4f, DashSolid);
    else StrokeLine(g, a, b, false, a, c, w, CapFlat, cap, 2.4f, 2.4f, DashSolid);
}

// ---------------------------------------------------------------- text
// wraps the text to the width (as AddString does: by words, breaking words longer than the line)
static std::vector<std::wstring> WrapLines(const std::wstring& text, const Font& font, const StringFormat& sf, float width) {
    Bitmap one(1, 1, PixelFormat32bppARGB);
    Graphics mg(&one);
    auto fits = [&](const std::wstring& t) {
        RectF box;
        mg.MeasureString(t.c_str(), (INT)t.size(), &font, PointF(0, 0), &sf, &box);
        return box.Width <= width + 0.5f;
    };
    std::vector<std::wstring> lines;
    size_t i = 0;
    while (i <= text.size()) {
        size_t nl = text.find(L'\n', i);
        std::wstring para = text.substr(i, nl == std::wstring::npos ? std::wstring::npos : nl - i);
        std::wstring cur;
        size_t k = 0;
        while (k < para.size()) {
            size_t sp = para.find(L' ', k);
            std::wstring word = para.substr(k, sp == std::wstring::npos ? std::wstring::npos : sp - k + 1);
            k = sp == std::wstring::npos ? para.size() : sp + 1;
            std::wstring cand = cur + word;
            if (fits(cand) || cur.empty() && fits(word)) { cur = cand; continue; }
            if (!cur.empty()) { lines.push_back(cur); cur.clear(); }
            while (!word.empty() && !fits(word)) {          // a word wider than the line: broken by characters
                size_t n = 1;
                while (n < word.size() && fits(word.substr(0, n + 1))) n++;
                lines.push_back(word.substr(0, n));
                word = word.substr(n);
            }
            cur = word;
        }
        lines.push_back(cur);
        if (nl == std::wstring::npos) break;
        i = nl + 1;
    }
    return lines;
}

static void TextPath(GraphicsPath& gp, const DrawObj& o, RectF r) {
    const Style& s = o.style;
    FontFamily ff(s.font.c_str());
    FontFamily fallback(L"Segoe UI");
    FontFamily* fam = ff.IsAvailable() ? &ff : &fallback;
    INT st = (s.bold ? FontStyleBold : 0) | (s.italic ? FontStyleItalic : 0) | (s.underline ? FontStyleUnderline : 0);
    StringFormat sf;
    static const StringAlignment al[] = { StringAlignmentNear, StringAlignmentCenter, StringAlignmentFar };
    sf.SetAlignment(al[std::clamp(s.alignH, 0, 2)]);
    sf.SetLineAlignment(al[std::clamp(s.alignV, 0, 2)]);
    sf.SetTrimming(StringTrimmingWord);
    float em = std::max(4.0f, s.fontSize), spacing = std::clamp(s.lineSpacing, 0.5f, 4.0f);
    if (std::fabs(spacing - 1.0f) < 0.01f) { gp.AddString(o.text.c_str(), -1, fam, st, em, r, &sf); return; }
    // line spacing (LineSpacing): the lines are wrapped here and placed 'spacing' line heights apart
    StringFormat lf(&sf);
    lf.SetFormatFlags(lf.GetFormatFlags() | StringFormatFlagsNoWrap);
    lf.SetLineAlignment(StringAlignmentNear);
    Font font(fam, em, st, UnitPixel);
    auto lines = WrapLines(o.text, font, lf, r.Width);
    float lineH = em * fam->GetLineSpacing(st) / std::max<UINT16>(1, fam->GetEmHeight(st)), step = lineH * spacing;
    float total = lineH + step * (lines.size() - 1);
    float y = s.alignV == 1 ? r.Y + (r.Height - total) / 2 : s.alignV == 2 ? r.GetBottom() - total : r.Y;
    for (auto& line : lines) {
        if (!line.empty()) gp.AddString(line.c_str(), -1, fam, st, em, RectF(r.X, y, r.Width, lineH * 1.2f), &lf);
        y += step;
    }
}

float TextBlockHeight(const DrawObj& o, float width) {
    if (o.text.empty()) return o.style.fontSize * 1.35f;
    DrawObj t = o;
    t.style.alignV = 0;
    GraphicsPath gp;
    TextPath(gp, t, RectF(0, 0, std::max(4.0f, width), 100000.0f));
    RectF b; gp.GetBounds(&b);
    float extra = 0;                                  // empty last lines (Enter at the end) take up height too
    if (std::fabs(o.style.lineSpacing - 1.0f) >= 0.01f)
        for (size_t i = o.text.size(); i > 0 && o.text[i - 1] == L'\n'; i--) extra += o.style.fontSize * 1.2f * o.style.lineSpacing;
    return std::max(o.style.fontSize * 1.35f, b.GetBottom() + extra + o.style.fontSize * 0.3f);    // room for descenders
}

// arrow label (Skitch style): a text block beyond the tail, on the side opposite to the arrow head
RectF ArrowLabelRect(const DrawObj& o) {
    const Style& s = o.style;
    FontFamily ff(s.font.c_str());
    FontFamily fallback(L"Segoe UI");
    INT st = (s.bold ? FontStyleBold : 0) | (s.italic ? FontStyleItalic : 0) | (s.underline ? FontStyleUnderline : 0);
    Font font(ff.IsAvailable() ? &ff : &fallback, std::max(4.0f, s.fontSize), st, UnitPixel);
    Bitmap one(1, 1, PixelFormat32bppARGB);
    Graphics mg(&one);
    StringFormat sf; sf.SetAlignment(StringAlignmentCenter);
    std::wstring t = o.text.empty() ? std::wstring(L"Ag") : o.text;
    if (!t.empty() && t.back() == L'\n') t += L"Ag";
    SizeF sz;
    mg.MeasureString(t.c_str(), -1, &font, SizeF(std::max(160.0f, s.fontSize * 14), 100000.0f), &sf, &sz);
    float tw = sz.Width + 2, th = sz.Height + 2;
    PointF from = s.bezier ? o.ctrl : o.p2;
    PointF dir = Norm(Sub(o.p1, from));
    float gap = s.width * 1.5f + CapLength(s.width, s.startSize) * (s.arrowStart != CapFlat && s.arrowStart != CapRound ? 0.5f : 0.0f) + 4;
    PointF a = Add(o.p1, Mul(dir, gap));
    return RectF(a.X + dir.X * tw / 2 - tw / 2, a.Y + dir.Y * th / 2 - th / 2, tw, th);
}

static void DrawTextObj(Graphics& g, const DrawObj& o, RectF r, uint32_t fillColor, uint32_t outline, float outlineW) {
    if (o.text.empty()) return;
    GraphicsPath gp;
    TextPath(gp, o, r);
    if (outlineW > 0 && (outline >> 24)) {
        Pen p(Gc(outline), outlineW * 0.8f + 1);
        p.SetLineJoin(LineJoinRound);
        g.DrawPath(&p, &gp);
    }
    SolidBrush b(Gc(fillColor));
    g.FillPath(&b, &gp);
}

// ---------------------------------------------------------------- stamps
// font with the characters of the text (GDI+ paths do not fall back to other fonts)
static const wchar_t* FontForText(const std::wstring& t) {
    for (wchar_t c : t) {
        if ((c >= 0x2E80 && c <= 0x9FFF) || (c >= 0xAC00 && c <= 0xD7AF) || (c >= 0xF900 && c <= 0xFAFF) || (c >= 0xFF00 && c <= 0xFFEF)) return L"Microsoft YaHei UI";
        if (c >= 0x0900 && c <= 0x0DFF) return L"Nirmala UI";
    }
    return L"Segoe UI";
}

// label stamp ("label:RRGGBB:Text", Before/After, Wrong/Right…): rounded badge in the color with the text in white;
// the text is stored in the id, so the document looks the same in any language
static Image RenderLabelStamp(const std::wstring& id, int px) {
    size_t c2 = id.find(L':', 6);
    if (c2 == std::wstring::npos) return {};
    uint32_t col = 0xFF000000 | (uint32_t)wcstoul(id.substr(6, c2 - 6).c_str(), nullptr, 16);
    std::wstring text = id.substr(c2 + 1);
    float h = std::max(16.0f, px / 3.0f), em = h * 0.52f;
    FontFamily ff(FontForText(text));
    FontFamily fallback(L"Segoe UI");
    FontFamily* fam = ff.IsAvailable() ? &ff : &fallback;
    GraphicsPath tp;
    StringFormat sf(StringFormat::GenericTypographic());
    tp.AddString(text.c_str(), -1, fam, FontStyleBold, em, PointF(0, 0), &sf);
    RectF tb; tp.GetBounds(&tb);
    float padX = h * 0.42f;
    int w = (int)std::ceil(std::max(h * 1.6f, tb.Width + 2 * padX)) + 2, ih = (int)std::ceil(h) + 2;
    Image img(w, ih, 0);
    auto bmp = img.WrapGdip();
    Graphics g(bmp.get());
    g.SetSmoothingMode(SmoothingModeAntiAlias);
    g.SetPixelOffsetMode(PixelOffsetModeHalf);
    RectF box(1, 1, (float)w - 2, h);
    GraphicsPath bp; AddRoundRect(bp, box, h * 0.28f);
    LinearGradientBrush lb(PointF(0, box.Y), PointF(0, box.GetBottom()), Gc(FromColorref(Lighter(ToColorref(col)), 255)), Gc(col));
    g.FillPath(&lb, &bp);
    Pen edge(Color(70, 0, 0, 0), std::max(1.0f, h * 0.04f));
    g.DrawPath(&edge, &bp);
    Matrix m; m.Translate(box.X + (box.Width - tb.Width) / 2 - tb.X, box.Y + (box.Height - tb.Height) / 2 - tb.Y);
    tp.Transform(&m);
    SolidBrush white(Color(255, 255, 255, 255));
    g.FillPath(&white, &tp);
    g.Flush(FlushIntentionSync);
    return img;
}

static const Image& StampBitmap(const std::wstring& id, int px) {
    static std::map<std::wstring, Image> cache;
    int bucket = std::clamp((px + 31) / 32 * 32, 32, 1024);
    std::wstring key = id + L"|" + std::to_wstring(bucket);
    auto it = cache.find(key);
    if (it != cache.end()) return it->second;
    if (cache.size() > 160) cache.clear();
    Image img;
    if (id.rfind(L"file:", 0) == 0) LoadImageFile(id.substr(5), img);
    else if (id.rfind(L"label:", 0) == 0) img = RenderLabelStamp(id, bucket);
    else img = RenderEmoji(id, bucket);
    return cache[key] = std::move(img);
}

float StampAspect(const std::wstring& id) {
    const Image& img = StampBitmap(id, 96);
    return img.empty() || img.h == 0 ? 1.0f : (float)img.w / img.h;
}

Image StampImage(const std::wstring& id, int px) { return StampBitmap(id, px); }

// magnifier outline for MagnifyShape
static int MagShapeKind(int m) {
    static const int k[MagnifyKindCount] = { ShapeEllipse, ShapeRect, ShapeRoundRect, ShapeHexagon, ShapeDiamond, ShapeStar };
    return k[std::clamp(m, 0, MagnifyKindCount - 1)];
}

// loupe handle: from the lens border at 45° (bottom right) outward
static void LoupeHandle(const DrawObj& o, PointF& a, PointF& b, float& w) {
    RectF r = o.Rect();
    float rad = std::min(r.Width, r.Height) / 2;
    PointF c = Center(r), d(0.7071f, 0.7071f);
    float edge = MagShapeKind(o.style.magnifyShape) == ShapeEllipse ? std::min(r.Width, r.Height) / 2 * 0.98f : std::min(r.Width, r.Height) / 2 * 1.3f;
    a = Add(c, Mul(d, edge));
    b = Add(a, Mul(d, rad * 0.85f));
    w = std::max(6.0f, rad * 0.24f);
}

RectF MagnifierExtra(const DrawObj& o) {
    RectF r = o.Rect();
    if (!o.style.loupe || !o.style.loupeHandle) return r;
    PointF a, b; float w;
    LoupeHandle(o, a, b, w);
    RectF hb(std::min(a.X, b.X) - w, std::min(a.Y, b.Y) - w, std::fabs(b.X - a.X) + 2 * w, std::fabs(b.Y - a.Y) + 2 * w);
    RectF::Union(r, r, hb);
    return r;
}

// ---------------------------------------------------------------- body of each object (layer)
static void DrawBody(Graphics& g, const Image& canvas, const DrawObj& o, bool hideText) {
    const Style& s = o.style;
    switch (o.type) {
    case ObjType::Arrow: case ObjType::Line:
        StrokeLine(g, o.p1, o.p2, s.bezier, o.ctrl, Gc(s.stroke), s.width, s.arrowStart, s.arrowEnd, s.startSize, s.endSize, s.dash);
        if (o.HasLabel() && !o.text.empty() && !hideText) {         // label at the tail, in the arrow color with a white outline
            DrawObj t;
            t.text = o.text;
            t.style = s;
            t.style.alignH = 1; t.style.alignV = 1; t.style.lineSpacing = 1;
            DrawTextObj(g, t, ArrowLabelRect(o), s.stroke | 0xFF000000, 0xFFFFFFFF, std::max(2.0f, s.fontSize * 0.14f));
        }
        break;
    case ObjType::Rect: case ObjType::RoundRect: case ObjType::Ellipse: case ObjType::Shape: {
        GraphicsPath gp;
        ShapePath(gp, o.Kind(), o.Rect());
        if ((s.fill >> 24) || s.grad.Active()) { auto b = FillBrush(s.fill, s.grad, o.Rect()); g.FillPath(b.get(), &gp); }
        if (s.width > 0 && (s.stroke >> 24)) {
            Pen p(Gc(s.stroke), s.width); p.SetLineJoin(LineJoinMiter); ApplyDash(p, s.dash);
            g.DrawPath(&p, &gp);
        }
        break;
    }
    case ObjType::Pen: {
        if (o.pts.empty()) break;
        Pen p(Gc(s.stroke), std::max(0.5f, s.width));
        ApplyDash(p, s.dash);
        p.SetLineJoin(LineJoinRound);
        p.SetStartCap(LineCapRound); p.SetEndCap(LineCapRound);
        if (o.pts.size() == 1) { SolidBrush b(Gc(s.stroke)); g.FillEllipse(&b, o.pts[0].X - s.width / 2, o.pts[0].Y - s.width / 2, s.width, s.width); }
        else g.DrawLines(&p, o.pts.data(), (INT)o.pts.size());
        break;
    }
    case ObjType::Highlighter: {
        // highlighter (CPaintOpHighlight): rectangle in the chosen color; the opacity comes from the layer composition
        if (o.pts.empty() && s.areaShape == ShapeRect) { SolidBrush b(Gc(s.stroke)); g.FillRectangle(&b, o.Rect()); break; }
        if (o.pts.empty() || o.IsPointShape()) {                       // other shapes (F05): ellipse, rounded, polygon…
            GraphicsPath gp; AreaPath(gp, o);
            SolidBrush b(Gc(s.stroke)); g.FillPath(&b, &gp);
            break;
        }
        Pen p(Gc(s.stroke), std::max(1.0f, s.width));                  // old freehand highlighters
        p.SetLineJoin(LineJoinRound); p.SetStartCap(LineCapSquare); p.SetEndCap(LineCapSquare);
        g.DrawLines(&p, o.pts.data(), (INT)o.pts.size());
        break;
    }
    case ObjType::Text: {
        RectF r = o.Rect();
        if (s.fill >> 24) {                    // old texts with a background box
            GraphicsPath gp; AddRoundRect(gp, r, 6);
            SolidBrush b(Gc(s.fill)); g.FillPath(&b, &gp);
        }
        r.Inflate(-s.padding, -s.padding);
        if (!hideText) DrawTextObj(g, o, r, s.textColor, s.stroke, s.width);
        break;
    }
    case ObjType::Callout: {
        const CalloutDef& d = Def(s.calloutShape);
        GraphicsPath gp;
        gp.SetFillMode(FillModeWinding);
        CalloutFullPath(gp, o);
        if (d.tail == Tail::Leader && !o.Rect().Contains(o.tail))       // box with arrow: the line leaves from the border
            StrokeLine(g, RectEdge(o.Rect(), o.tail), o.tail, false, o.tail, Gc(s.tailColor), s.tailWidth, CapFlat, s.arrowEnd,
                       s.startSize, s.endSize, s.dash);
        if (s.fill >> 24) {
            RectF bb; gp.GetBounds(&bb);
            if (s.shaded && bb.Height > 1) {
                COLORREF c = ToColorref(s.fill);
                LinearGradientBrush lb(PointF(0, bb.Y), PointF(0, bb.GetBottom()), Gc(FromColorref(Lighter(c), CA(s.fill))), Gc(s.fill));
                g.FillPath(&lb, &gp);
            } else { SolidBrush b(Gc(s.fill)); g.FillPath(&b, &gp); }
        }
        if (s.width > 0 && (s.stroke >> 24)) {
            GraphicsPath outline; outline.SetFillMode(FillModeWinding); CalloutFullPath(outline, o);
            outline.Outline();
            Pen p(Gc(s.stroke), s.width); p.SetLineJoin(LineJoinRound);
            if (d.dashed) ApplyDash(p, DashDash);
            g.DrawPath(&p, &outline);
            if (d.body == Body::Note) {        // note fold
                RectF r = o.Rect(); float f = std::min(r.Width, r.Height) * 0.22f;
                PointF fold[3] = { { r.X + f, r.Y }, { r.X + f, r.Y + f }, { r.X, r.Y + f } };
                g.DrawLines(&p, fold, 3);
            }
        }
        if (!hideText) DrawTextObj(g, o, CalloutTextRect(o), s.textColor, 0, 0);
        break;
    }
    case ObjType::Step: {
        float d = s.fontSize * 1.5f;
        RectF r(o.p1.X - d / 2, o.p1.Y - d / 2, d, d);
        GraphicsPath gp;
        gp.SetFillMode(FillModeWinding);
        switch (s.stepShape) {
        case 1: {                               // teardrop pointing right
            gp.AddEllipse(r);
            PointF tri[3] = { { r.X + d * 0.5f, r.Y + d * 0.06f }, { r.GetRight() + d * 0.42f, r.Y + d / 2 }, { r.X + d * 0.5f, r.GetBottom() - d * 0.06f } };
            gp.AddPolygon(tri, 3);
            break;
        }
        case 2: gp.AddRectangle(r); break;
        case 3: AddRoundRect(gp, r, d * 0.22f); break;
        default: gp.AddEllipse(r); break;
        }
        SolidBrush b(Gc(s.stroke));
        g.FillPath(&b, &gp);
        if (s.stepOutlineWidth > 0 && (s.stepOutline >> 24)) {           // outline for contrast on any background
            GraphicsPath outline; outline.SetFillMode(FillModeWinding);
            outline.AddPath(&gp, FALSE);
            outline.Outline();
            Pen p(Gc(s.stepOutline), s.stepOutlineWidth); p.SetLineJoin(LineJoinRound);
            g.DrawPath(&p, &outline);
        }
        std::wstring label = StepLabel(o.stepValue, s.stepType);
        DrawObj t;
        t.text = label;
        t.style.font = L"Segoe UI";
        t.style.bold = true;
        t.style.alignH = t.style.alignV = 1;
        t.style.fontSize = d * (label.size() <= 1 ? 0.56f : label.size() == 2 ? 0.46f : 0.34f);
        DrawTextObj(g, t, RectF(r.X, r.Y + d * 0.02f, d, d), s.textColor, 0, 0);
        break;
    }
    case ObjType::Stamp: {
        RectF r = o.Rect();
        if (r.Width < 2 || r.Height < 2) break;
        const Image& img = StampBitmap(o.text, (int)std::max(r.Width, r.Height));
        if (img.empty()) break;
        float k = std::min(r.Width / img.w, r.Height / img.h);
        RectF dst(r.X + (r.Width - img.w * k) / 2, r.Y + (r.Height - img.h * k) / 2, img.w * k, img.h * k);
        Image copy = img;
        auto bmp = copy.WrapGdip();
        g.SetInterpolationMode(InterpolationModeHighQualityBicubic);
        g.DrawImage(bmp.get(), dst);
        break;
    }
    case ObjType::Magnify: {
        RectF r = o.Rect();
        if (r.Width < 4 || r.Height < 4) break;
        float k = std::clamp(s.magnifyScale / 100.0f, 1.1f, 10.0f);
        PointF src = s.magnifyLink ? o.tail : Center(r);
        float sw = r.Width / k, sh = r.Height / k;
        RectF sr(src.X - sw / 2, src.Y - sh / 2, sw, sh);
        int shape = MagShapeKind(s.magnifyShape);
        Pen pen(Gc(s.stroke), std::max(1.0f, s.width));
        if (s.magnifyLink) {                    // magnified area marked and connected to the lens
            Pen thin(Gc(s.stroke), std::max(1.0f, s.width * 0.6f));
            GraphicsPath sp; ShapePath(sp, shape, sr);
            PointF a = Center(sr), b = Center(r), dv = Norm(Sub(b, a));
            float ra = std::min(sr.Width, sr.Height) / 2, rb = std::min(r.Width, r.Height) / 2;
            if (Len(Sub(b, a)) > ra + rb) g.DrawLine(&thin, Add(a, Mul(dv, ra)), Sub(b, Mul(dv, rb)));
            g.DrawPath(&thin, &sp);
        }
        Image piece = canvas.Crop({ (LONG)std::floor(sr.X), (LONG)std::floor(sr.Y), (LONG)std::ceil(sr.GetRight()), (LONG)std::ceil(sr.GetBottom()) });
        GraphicsPath lens; ShapePath(lens, shape, r);
        SolidBrush white(Color(255, 255, 255, 255));
        g.FillPath(&white, &lens);
        if (!piece.empty()) {
            auto pb = piece.WrapGdip();
            GraphicsState gs = g.Save();
            g.SetClip(&lens);
            g.SetInterpolationMode(InterpolationModeHighQualityBicubic);
            float ox = std::max(0.0f, std::floor(sr.X)) - sr.X, oy = std::max(0.0f, std::floor(sr.Y)) - sr.Y;
            g.DrawImage(pb.get(), RectF(r.X + ox * k, r.Y + oy * k, piece.w * k, piece.h * k));
            g.Restore(gs);
        }
        if (s.loupe) {                          // real lens: handle, thick shaded rim, inner shadow and reflection
            float rad = std::min(r.Width, r.Height) / 2, rim = std::max(4.0f, rad * 0.11f + s.width * 0.5f);
            COLORREF base = ToColorref(s.stroke);
            Color light = Gc(FromColorref(Lighter(Lighter(base)), 255)), dark = Gc(FromColorref(RGB(GetRValue(base) * 3 / 5, GetGValue(base) * 3 / 5, GetBValue(base) * 3 / 5), 255));
            if (s.loupeHandle) {
                PointF a, b; float w;
                LoupeHandle(o, a, b, w);
                PointF n(-0.7071f, 0.7071f);
                LinearGradientBrush hb(Add(a, Mul(n, w / 2)), Sub(a, Mul(n, w / 2)), dark, light);
                Pen hp(&hb, w); hp.SetStartCap(LineCapFlat); hp.SetEndCap(LineCapRound);
                g.DrawLine(&hp, a, b);
                Pen collar(dark, w * 1.25f); collar.SetStartCap(LineCapFlat); collar.SetEndCap(LineCapFlat);
                g.DrawLine(&collar, a, Add(a, Mul(PointF(0.7071f, 0.7071f), w * 0.9f)));
            }
            GraphicsState gs = g.Save();                    // inner shadow at the edge of the glass
            g.SetClip(&lens);
            Pen inner(Color(60, 0, 0, 0), rim * 1.6f);
            g.DrawPath(&inner, &lens);
            GraphicsPath glare;                             // reflection on the upper left
            glare.AddEllipse(r.X + r.Width * 0.14f, r.Y + r.Height * 0.08f, r.Width * 0.52f, r.Height * 0.34f);
            PathGradientBrush gb(&glare);
            Color glareCols[2] = { Color(0, 255, 255, 255), Color(95, 255, 255, 255) };
            REAL glarePos[2] = { 0, 1 };
            gb.SetInterpolationColors(glareCols, glarePos, 2);
            gb.SetCenterPoint(PointF(r.X + r.Width * 0.36f, r.Y + r.Height * 0.2f));
            g.FillPath(&gb, &glare);
            g.Restore(gs);
            LinearGradientBrush rb(PointF(r.X, r.Y), PointF(r.GetRight(), r.GetBottom()), light, dark);
            Pen rp(&rb, rim); rp.SetLineJoin(LineJoinRound);
            g.DrawPath(&rp, &lens);
            Pen edge(Color(150, 0, 0, 0), 1.0f);
            GraphicsPath outer; ShapePath(outer, shape, RectF(r.X - rim / 2, r.Y - rim / 2, r.Width + rim, r.Height + rim));
            g.DrawPath(&edge, &outer);
            GraphicsPath innerEdge; ShapePath(innerEdge, shape, RectF(r.X + rim / 2, r.Y + rim / 2, r.Width - rim, r.Height - rim));
            g.DrawPath(&edge, &innerEdge);
        } else if (s.width > 0) g.DrawPath(&pen, &lens);
        break;
    }
    case ObjType::Picture: {
        if (!o.picture || o.picture->empty()) break;
        RectF r = o.Rect();
        auto pb = o.picture->WrapGdip();
        g.DrawImage(pb.get(), r, 0, 0, (REAL)o.picture->w, (REAL)o.picture->h, UnitPixel);
        if (s.width > 0 && (s.stroke >> 24)) { Pen p(Gc(s.stroke), s.width); g.DrawRectangle(&p, r); }
        break;
    }
    default: break;
    }
}

// blur of the alpha only (three box passes ≈ Gaussian), for the shadow
static void BlurAlpha(std::vector<int>& a, int w, int h, int radius) {
    if (radius < 1) return;
    std::vector<int> tmp(a.size());
    int r = std::max(1, radius / 2);
    for (int pass = 0; pass < 3; pass++) {
        for (int y = 0; y < h; y++) {
            int* src = a.data() + (size_t)y * w; int* dst = tmp.data() + (size_t)y * w;
            int sum = 0;
            for (int x = -r; x <= r; x++) sum += src[std::clamp(x, 0, w - 1)];
            for (int x = 0; x < w; x++) {
                dst[x] = sum / (2 * r + 1);
                sum += src[std::min(x + r + 1, w - 1)] - src[std::max(x - r, 0)];
            }
        }
        for (int x = 0; x < w; x++) {
            int sum = 0;
            for (int y = -r; y <= r; y++) sum += tmp[(size_t)std::clamp(y, 0, h - 1) * w + x];
            for (int y = 0; y < h; y++) {
                a[(size_t)y * w + x] = sum / (2 * r + 1);
                sum += tmp[(size_t)std::min(y + r + 1, h - 1) * w + x] - tmp[(size_t)std::max(y - r, 0) * w + x];
            }
        }
    }
}

static void Composite(Image& dst, const Image& src, int ox, int oy, int opacity) {
    opacity = std::clamp(opacity, 0, 100);
    for (int y = 0; y < src.h; y++) {
        int dy = oy + y;
        if (dy < 0 || dy >= dst.h) continue;
        for (int x = 0; x < src.w; x++) {
            int dx = ox + x;
            if (dx < 0 || dx >= dst.w) continue;
            uint32_t s = src.at(x, y);
            int a = CA(s) * opacity / 100;
            if (!a) continue;
            uint32_t& d = dst.at(dx, dy);
            d = BlendOver(d, (s & 0x00FFFFFF) | ((uint32_t)a << 24));
        }
    }
}

// old freehand highlighters (documents from earlier versions): the color is multiplied by the image, as before
static void LegacyHighlight(Graphics& g, Image& canvas, const DrawObj& o) {
    g.Flush(FlushIntentionSync);
    RectF bf = o.Bounds();
    RECT b{ std::max(0L, (LONG)std::floor(bf.X)), std::max(0L, (LONG)std::floor(bf.Y)),
            std::min((LONG)canvas.w, (LONG)std::ceil(bf.GetRight())), std::min((LONG)canvas.h, (LONG)std::ceil(bf.GetBottom())) };
    if (b.right <= b.left || b.bottom <= b.top) return;
    Image mask(b.right - b.left, b.bottom - b.top, 0);
    {
        auto mb = mask.WrapGdip();
        Graphics mg(mb.get());
        mg.SetSmoothingMode(SmoothingModeAntiAlias);
        mg.TranslateTransform(-(REAL)b.left, -(REAL)b.top);
        Pen p(Color(255, 255, 255, 255), std::max(1.0f, o.style.width));
        p.SetLineJoin(LineJoinRound); p.SetStartCap(LineCapSquare); p.SetEndCap(LineCapSquare);
        if (o.pts.size() == 1) { SolidBrush w(Color(255, 255, 255, 255)); mg.FillRectangle(&w, o.pts[0].X - o.style.width / 2, o.pts[0].Y - o.style.width / 2, o.style.width, o.style.width); }
        else mg.DrawLines(&p, o.pts.data(), (INT)o.pts.size());
    }
    int hr = CR(o.style.stroke), hg = CG(o.style.stroke), hb = CB(o.style.stroke), op = std::clamp(o.style.opacity, 0, 100);
    for (int y = 0; y < mask.h; y++)
        for (int x = 0; x < mask.w; x++) {
            int a = CA(mask.at(x, y)) * op / 100;
            if (!a) continue;
            uint32_t& p = canvas.at(b.left + x, b.top + y);
            int ca = CA(p);
            int r = (CR(p) * ca + 255 * (255 - ca)) / 255, gg = (CG(p) * ca + 255 * (255 - ca)) / 255, bb = (CB(p) * ca + 255 * (255 - ca)) / 255;
            p = ARGB(std::max(ca, a), r + (r * hr / 255 - r) * a / 255, gg + (gg * hg / 255 - gg) * a / 255, bb + (bb * hb / 255 - bb) * a / 255);
        }
}

// anti-aliased coverage of a path inside 'b' (one byte per pixel)
static std::vector<uint8_t> PathMask(GraphicsPath& gp, RECT b) {
    int w = b.right - b.left, h = b.bottom - b.top;
    Image m(w, h, 0);
    {
        auto mb = m.WrapGdip();
        Graphics mg(mb.get());
        mg.SetSmoothingMode(SmoothingModeAntiAlias);
        mg.SetPixelOffsetMode(PixelOffsetModeHalf);
        mg.TranslateTransform(-(REAL)b.left, -(REAL)b.top);
        SolidBrush white(Color(255, 255, 255, 255));
        mg.FillPath(&white, &gp);
        mg.Flush(FlushIntentionSync);
    }
    std::vector<uint8_t> a(m.px.size());
    for (size_t i = 0; i < a.size(); i++) a[i] = (uint8_t)CA(m.px[i]);
    return a;
}

static RECT ClipRect(RectF rf, const Image& canvas) {
    return { std::max(0L, (LONG)std::floor(rf.X)), std::max(0L, (LONG)std::floor(rf.Y)),
             std::min((LONG)canvas.w, (LONG)std::ceil(rf.GetRight())), std::min((LONG)canvas.h, (LONG)std::ceil(rf.GetBottom())) };
}

// applies an effect (blur/pixelate) to the canvas only inside the path, with soft edges
static void EffectInPath(Image& canvas, GraphicsPath& gp, const std::function<void(Image&, RECT)>& effect) {
    RectF bf; gp.GetBounds(&bf);
    RECT b = ClipRect(bf, canvas);
    if (b.right <= b.left || b.bottom <= b.top) return;
    auto mask = PathMask(gp, b);
    Image piece = canvas.Crop(b);
    effect(piece, { 0, 0, piece.w, piece.h });
    for (int y = 0; y < piece.h; y++)
        for (int x = 0; x < piece.w; x++) {
            int a = mask[(size_t)y * piece.w + x];
            if (!a) continue;
            uint32_t& d = canvas.at(b.left + x, b.top + y);
            uint32_t sp = piece.at(x, y);
            if (a == 255) { d = sp; continue; }
            auto mix = [&](int sh) { return (uint32_t)((((d >> sh) & 255) * (255 - a) + ((sp >> sh) & 255) * a) / 255) << sh; };
            d = mix(24) | mix(16) | mix(8) | mix(0);
        }
}

void RenderObject(Graphics& g, Image& canvas, const DrawObj& o, bool hideText) {
    const Style& s = o.style;
    if (o.type == ObjType::Highlighter && o.IsPath()) { LegacyHighlight(g, canvas, o); return; }   // polygon / freehand areas are not legacy
    if (o.type == ObjType::Group) { for (auto& c : o.children) RenderObject(g, canvas, c); return; }   // each child with its own shadow
    switch (o.type) {
    case ObjType::Blur: case ObjType::Pixelate: case ObjType::Spotlight: {
        g.Flush(FlushIntentionSync);
        if (s.areaShape != ShapeRect || o.IsPointShape()) {            // round, polygon and freehand areas (F05, F18)
            GraphicsPath gp; AreaPath(gp, o);
            if (o.type == ObjType::Blur) EffectInPath(canvas, gp, [&](Image& im, RECT r) { fx::BoxBlur(im, r, std::max(2, s.amount)); });
            else if (o.type == ObjType::Pixelate) EffectInPath(canvas, gp, [&](Image& im, RECT r) { fx::Pixelate(im, r, std::max(3, s.amount * 3 / 2)); });
            else {
                int dark = std::clamp(s.amount * 6, 20, 85);
                RectF bf; gp.GetBounds(&bf);
                RECT b = ClipRect(bf, canvas);
                std::vector<uint8_t> mask = b.right > b.left && b.bottom > b.top ? PathMask(gp, b) : std::vector<uint8_t>{};
                for (int y = 0; y < canvas.h; y++)
                    for (int x = 0; x < canvas.w; x++) {
                        int a = (x >= b.left && x < b.right && y >= b.top && y < b.bottom) ? mask[(size_t)(y - b.top) * (b.right - b.left) + x - b.left] : 0;
                        if (a == 255) continue;
                        int k = dark * (255 - a) / 255;
                        uint32_t& p = canvas.at(x, y);
                        p = ARGB(CA(p), CR(p) * (100 - k) / 100, CG(p) * (100 - k) / 100, CB(p) * (100 - k) / 100);
                    }
                Pen p(Color(200, 255, 255, 255), 2);
                p.SetLineJoin(LineJoinRound);
                g.DrawPath(&p, &gp);
            }
            return;
        }
        RectF rf = o.Rect();
        RECT r{ (LONG)std::floor(rf.X), (LONG)std::floor(rf.Y), (LONG)std::ceil(rf.GetRight()), (LONG)std::ceil(rf.GetBottom()) };
        if (o.type == ObjType::Blur) fx::BoxBlur(canvas, r, std::max(2, s.amount));
        else if (o.type == ObjType::Pixelate) fx::Pixelate(canvas, r, std::max(3, s.amount * 3 / 2));
        else {
            int dark = std::clamp(s.amount * 6, 20, 85);
            for (int y = 0; y < canvas.h; y++)
                for (int x = 0; x < canvas.w; x++) {
                    if (x >= r.left && x < r.right && y >= r.top && y < r.bottom) continue;
                    uint32_t& p = canvas.at(x, y);
                    p = ARGB(CA(p), CR(p) * (100 - dark) / 100, CG(p) * (100 - dark) / 100, CB(p) * (100 - dark) / 100);
                }
            Pen p(Color(200, 255, 255, 255), 2);
            g.DrawRectangle(&p, rf);
        }
        return;
    }
    default: break;
    }
    g.Flush(FlushIntentionSync);
    if (o.type == ObjType::Callout && s.bgBlur) {          // frosted glass: blurs the composed image behind the callout
        GraphicsPath gp; gp.SetFillMode(FillModeWinding);
        CalloutFullPath(gp, o);
        int radius = std::clamp(s.bgBlurAmount, 1, 40);
        EffectInPath(canvas, gp, [&](Image& im, RECT r) { fx::BoxBlur(im, r, radius); });
    }
    RectF bf = o.Bounds();
    RECT b{ std::max(0L, (LONG)std::floor(bf.X)), std::max(0L, (LONG)std::floor(bf.Y)),
            std::min((LONG)canvas.w, (LONG)std::ceil(bf.GetRight())), std::min((LONG)canvas.h, (LONG)std::ceil(bf.GetBottom())) };
    if (b.right <= b.left || b.bottom <= b.top) return;
    Image layer(b.right - b.left, b.bottom - b.top, 0);
    {
        auto lb = layer.WrapGdip();
        Graphics lg(lb.get());
        lg.SetSmoothingMode(SmoothingModeAntiAlias);
        lg.SetTextRenderingHint(TextRenderingHintAntiAlias);
        lg.SetPixelOffsetMode(PixelOffsetModeHalf);
        lg.SetInterpolationMode(InterpolationModeHighQualityBicubic);
        lg.TranslateTransform(-(REAL)b.left, -(REAL)b.top);
        DrawBody(lg, canvas, o, hideText);
        lg.Flush(FlushIntentionSync);
    }
    bool shadow = s.shadow && o.type != ObjType::Highlighter;
    if (shadow) {
        std::vector<int> a(layer.px.size());
        for (size_t i = 0; i < a.size(); i++) a[i] = CA(layer.px[i]);
        BlurAlpha(a, layer.w, layer.h, s.shadowBlur);
        Image sh(layer.w, layer.h, 0);
        uint32_t rgb = s.shadowColor & 0x00FFFFFF;
        for (size_t i = 0; i < a.size(); i++) if (a[i]) sh.px[i] = rgb | ((uint32_t)std::min(255, a[i]) << 24);
        int dir = std::clamp(s.shadowDir, 0, 8);
        int dx = (dir % 3 - 1) * s.shadowDist, dy = (dir / 3 - 1) * s.shadowDist;
        Composite(canvas, sh, b.left + dx, b.top + dy, s.shadowOpacity * s.opacity / 100);
    }
    Composite(canvas, layer, b.left, b.top, s.opacity);
}

} // namespace snap::ed
