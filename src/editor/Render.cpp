// Pintores dos objetos (PainterDC/GDI+; comp_5 §3.5 "PaintTools::IPainter", ed_comp_4 §6.3)
// Cada objeto vetorial é desenhado numa camada própria, com as cores cheias; a sombra (GetDropShadowImage: cor,
// desfoque e deslocamento na direção escolhida) sai do alfa dessa camada e as duas são compostas com a opacidade
// do objeto — assim partes que se sobrepõem (linha + ponta, corpo + ponta do balão) não ficam mais escuras.
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

// ---------------------------------------------------------------- formas
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

// ---------------------------------------------------------------- balões (CalloutObject / RemixCallouts)
// 24 formatos, na ordem da galeria: fala, nuvens, caixa com seta, setas, retângulos, nota, explosão, pensamento, sussurro.
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

// direção das setas pela ponta arrastável: 0 direita, 1 baixo, 2 esquerda, 3 cima
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

// corpo + ponta (cunha ou bolhas) num único caminho de preenchimento "winding"
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

// ponto da borda do retângulo na direção de 'to' (para a seta da caixa com linha)
static PointF RectEdge(RectF r, PointF to) {
    PointF c = Center(r), d = Sub(to, c);
    if (std::fabs(d.X) < 1e-3f && std::fabs(d.Y) < 1e-3f) return c;
    float sx = d.X != 0 ? (r.Width / 2) / std::fabs(d.X) : 1e9f, sy = d.Y != 0 ? (r.Height / 2) / std::fabs(d.Y) : 1e9f;
    float s = std::min(sx, sy);
    return Add(c, Mul(d, std::min(s, 1.0f)));
}

// ---------------------------------------------------------------- linhas e pontas
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

// linha reta ou curva (quadrática por 'ctrl') com pontas e tracejado
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

// ---------------------------------------------------------------- texto
static void TextPath(GraphicsPath& gp, const DrawObj& o, RectF r) {
    const Style& s = o.style;
    FontFamily ff(s.font.c_str());
    FontFamily fallback(L"Segoe UI");
    INT st = (s.bold ? FontStyleBold : 0) | (s.italic ? FontStyleItalic : 0) | (s.underline ? FontStyleUnderline : 0);
    StringFormat sf;
    static const StringAlignment al[] = { StringAlignmentNear, StringAlignmentCenter, StringAlignmentFar };
    sf.SetAlignment(al[std::clamp(s.alignH, 0, 2)]);
    sf.SetLineAlignment(al[std::clamp(s.alignV, 0, 2)]);
    sf.SetTrimming(StringTrimmingWord);
    gp.AddString(o.text.c_str(), -1, ff.IsAvailable() ? &ff : &fallback, st, std::max(4.0f, s.fontSize), r, &sf);
}

float TextBlockHeight(const DrawObj& o, float width) {
    if (o.text.empty()) return o.style.fontSize * 1.35f;
    DrawObj t = o;
    t.style.alignV = 0;
    GraphicsPath gp;
    TextPath(gp, t, RectF(0, 0, std::max(4.0f, width), 100000.0f));
    RectF b; gp.GetBounds(&b);
    return std::max(o.style.fontSize * 1.35f, b.GetBottom() + o.style.fontSize * 0.3f);    // folga para os descendentes
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

// ---------------------------------------------------------------- carimbos
static const Image& StampBitmap(const std::wstring& id, int px) {
    static std::map<std::wstring, Image> cache;
    int bucket = std::clamp((px + 31) / 32 * 32, 32, 1024);
    std::wstring key = id + L"|" + std::to_wstring(bucket);
    auto it = cache.find(key);
    if (it != cache.end()) return it->second;
    if (cache.size() > 160) cache.clear();
    Image img;
    if (id.rfind(L"file:", 0) == 0) LoadImageFile(id.substr(5), img);
    else img = RenderEmoji(id, bucket);
    return cache[key] = std::move(img);
}

// ---------------------------------------------------------------- corpo de cada objeto (camada)
static void DrawBody(Graphics& g, const Image& canvas, const DrawObj& o, bool hideText) {
    const Style& s = o.style;
    switch (o.type) {
    case ObjType::Arrow: case ObjType::Line:
        StrokeLine(g, o.p1, o.p2, s.bezier, o.ctrl, Gc(s.stroke), s.width, s.arrowStart, s.arrowEnd, s.startSize, s.endSize, s.dash);
        break;
    case ObjType::Rect: case ObjType::RoundRect: case ObjType::Ellipse: case ObjType::Shape: {
        GraphicsPath gp;
        ShapePath(gp, o.Kind(), o.Rect());
        if (s.fill >> 24) { SolidBrush b(Gc(s.fill)); g.FillPath(&b, &gp); }
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
        // marcador (CPaintOpHighlight): retângulo na cor escolhida; a opacidade vem na composição da camada
        if (o.pts.empty()) { SolidBrush b(Gc(s.stroke)); g.FillRectangle(&b, o.Rect()); break; }
        Pen p(Gc(s.stroke), std::max(1.0f, s.width));                  // marcadores antigos à mão livre
        p.SetLineJoin(LineJoinRound); p.SetStartCap(LineCapSquare); p.SetEndCap(LineCapSquare);
        g.DrawLines(&p, o.pts.data(), (INT)o.pts.size());
        break;
    }
    case ObjType::Text: {
        RectF r = o.Rect();
        if (s.fill >> 24) {                    // textos antigos com caixa de fundo
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
        if (d.tail == Tail::Leader && !o.Rect().Contains(o.tail))       // caixa com seta: a linha sai da borda
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
            if (d.body == Body::Note) {        // dobra da nota
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
        case 1: {                               // gota apontando para a direita
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
        int shape = s.magnifyShape == 1 ? ShapeRect : s.magnifyShape == 2 ? ShapeRoundRect : ShapeEllipse;
        Pen pen(Gc(s.stroke), std::max(1.0f, s.width));
        if (s.magnifyLink) {                    // área ampliada marcada e ligada à lente
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
        if (s.width > 0) g.DrawPath(&pen, &lens);
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

// desfoque só do alfa (três passadas de caixa ≈ gaussiano), para a sombra
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

// marcadores antigos à mão livre (documentos de versões anteriores): a cor é multiplicada pela imagem, como antes
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

void RenderObject(Graphics& g, Image& canvas, const DrawObj& o, bool hideText) {
    const Style& s = o.style;
    if (o.type == ObjType::Highlighter && !o.pts.empty()) { LegacyHighlight(g, canvas, o); return; }
    if (o.type == ObjType::Group) { for (auto& c : o.children) RenderObject(g, canvas, c); return; }   // cada filho com a sua sombra
    switch (o.type) {
    case ObjType::Blur: case ObjType::Pixelate: case ObjType::Spotlight: {
        g.Flush(FlushIntentionSync);
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
