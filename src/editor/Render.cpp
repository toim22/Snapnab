// Pintores dos objetos (PainterDC/GDI+; comp_5 §3.5 "PaintTools::IPainter")
#include "Document.h"
#include "Effects.h"
#include <cmath>

namespace snap::ed {

using namespace Gdiplus;

Color Gc(uint32_t argb, int opacity) {
    int a = (int)((argb >> 24) & 255) * std::clamp(opacity, 0, 100) / 100;
    return Color((BYTE)a, (BYTE)((argb >> 16) & 255), (BYTE)((argb >> 8) & 255), (BYTE)(argb & 255));
}

std::vector<std::wstring> StampGlyphs() {
    return { L"✔", L"✖", L"★", L"⚠", L"❤", L"☺", L"☹", L"?", L"!", L"➜", L"✎", L"☎", L"✉", L"⚑", L"♪", L"☀", L"⌛", L"⚙" };
}

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

static void CalloutPath(GraphicsPath& gp, const DrawObj& o) {
    RectF r = o.Rect();
    PointF c(r.X + r.Width / 2, r.Y + r.Height / 2);
    switch (o.style.calloutShape) {
    case 2: gp.AddEllipse(r); break;
    case 1: gp.AddRectangle(r); break;
    case 3: {   // pensamento: nuvem de elipses
        int n = 10;
        for (int i = 0; i < n; i++) {
            double a = i * 2 * 3.14159265 / n;
            float ex = c.X + (float)std::cos(a) * r.Width * 0.38f, ey = c.Y + (float)std::sin(a) * r.Height * 0.36f;
            gp.AddEllipse(ex - r.Width * 0.2f, ey - r.Height * 0.22f, r.Width * 0.4f, r.Height * 0.44f);
        }
        gp.AddEllipse(r.X + r.Width * 0.12f, r.Y + r.Height * 0.12f, r.Width * 0.76f, r.Height * 0.76f);
        break;
    }
    default: AddRoundRect(gp, r, std::min(r.Width, r.Height) * 0.2f);
    }
    if (o.style.calloutShape == 3) {
        // bolhas até a ponta
        for (int i = 1; i <= 3; i++) {
            float t = i / 4.0f, s = std::max(4.0f, std::min(r.Width, r.Height) * 0.09f * (1.2f - t));
            float x = c.X + (o.tail.X - c.X) * (0.55f + t * 0.45f), y = c.Y + (o.tail.Y - c.Y) * (0.55f + t * 0.45f);
            gp.AddEllipse(x - s, y - s, s * 2, s * 2);
        }
        return;
    }
    // cauda triangular da borda até a ponta
    float dx = o.tail.X - c.X, dy = o.tail.Y - c.Y;
    float len = std::sqrt(dx * dx + dy * dy);
    if (len < 1 || r.Contains(o.tail)) return;
    float nx = -dy / len, ny = dx / len;
    float base = std::max(8.0f, std::min(r.Width, r.Height) * 0.18f);
    PointF tri[3] = { PointF(c.X + nx * base, c.Y + ny * base), o.tail, PointF(c.X - nx * base, c.Y - ny * base) };
    // mesma orientação (horária) do corpo, para o preenchimento "winding" unir corpo e cauda
    float cross = (tri[1].X - tri[0].X) * (tri[2].Y - tri[0].Y) - (tri[1].Y - tri[0].Y) * (tri[2].X - tri[0].X);
    if (cross < 0) std::swap(tri[0], tri[2]);
    GraphicsPath t; t.AddPolygon(tri, 3);
    gp.AddPath(&t, FALSE);
}

static void DrawArrowLine(Graphics& g, const DrawObj& o, Color col, float w) {
    PointF a = o.p1, b = o.p2;
    float dx = b.X - a.X, dy = b.Y - a.Y, len = std::sqrt(dx * dx + dy * dy);
    if (len < 1) return;
    float ux = dx / len, uy = dy / len;
    float headLen = std::max(10.0f, w * 3.2f), half = std::max(5.0f, w * 1.7f);
    bool hs = o.type == ObjType::Arrow && o.style.arrowStart, he = o.type == ObjType::Arrow && o.style.arrowEnd;
    if (hs || he) headLen = std::min(headLen, len * (hs && he ? 0.45f : 0.8f));
    PointF sa = hs ? PointF(a.X + ux * headLen * 0.8f, a.Y + uy * headLen * 0.8f) : a;
    PointF sb = he ? PointF(b.X - ux * headLen * 0.8f, b.Y - uy * headLen * 0.8f) : b;
    Gdiplus::Pen p(col, w);
    p.SetStartCap(LineCapRound); p.SetEndCap(LineCapRound);
    g.DrawLine(&p, sa, sb);
    SolidBrush br(col);
    auto head = [&](PointF tip, float sx, float sy) {
        PointF bpt(tip.X - sx * headLen, tip.Y - sy * headLen);
        PointF pts[3] = { tip, PointF(bpt.X - sy * half, bpt.Y + sx * half), PointF(bpt.X + sy * half, bpt.Y - sx * half) };
        g.FillPolygon(&br, pts, 3);
    };
    if (he) head(b, ux, uy);
    if (hs) head(a, -ux, -uy);
}

static void DrawShape(Graphics& g, const DrawObj& o, bool shadow) {
    const Style& s = o.style;
    float off = shadow ? std::max(3.0f, s.width * 0.8f) : 0;
    Color stroke = shadow ? Color(70, 0, 0, 0) : Gc(s.stroke, s.opacity);
    Color fill = shadow ? Color((BYTE)((s.fill >> 24) ? 50 : 0), 0, 0, 0) : Gc(s.fill, s.opacity);
    GraphicsState gs = g.Save();
    if (shadow) g.TranslateTransform(off, off);
    switch (o.type) {
    case ObjType::Arrow: case ObjType::Line: DrawArrowLine(g, o, stroke, s.width); break;
    case ObjType::Rect: case ObjType::RoundRect: case ObjType::Ellipse: {
        RectF r = o.Rect();
        GraphicsPath gp;
        if (o.type == ObjType::Ellipse) gp.AddEllipse(r);
        else if (o.type == ObjType::RoundRect) AddRoundRect(gp, r, std::min(r.Width, r.Height) * 0.18f);
        else gp.AddRectangle(r);
        if ((s.fill >> 24) && fill.GetA()) { SolidBrush b(fill); g.FillPath(&b, &gp); }
        if (s.width > 0) { Gdiplus::Pen p(stroke, s.width); p.SetLineJoin(LineJoinRound); g.DrawPath(&p, &gp); }
        break;
    }
    case ObjType::Pen: case ObjType::Highlighter: {
        if (o.pts.empty()) break;
        bool hl = o.type == ObjType::Highlighter;
        Color c = hl ? Gc((s.stroke & 0x00FFFFFF) | 0x73000000u, s.opacity) : stroke;
        if (hl && shadow) break;
        Gdiplus::Pen p(c, s.width);
        p.SetLineJoin(LineJoinRound);
        p.SetStartCap(hl ? LineCapSquare : LineCapRound); p.SetEndCap(hl ? LineCapSquare : LineCapRound);
        if (o.pts.size() == 1) { SolidBrush b(c); g.FillEllipse(&b, o.pts[0].X - s.width / 2, o.pts[0].Y - s.width / 2, s.width, s.width); }
        else g.DrawLines(&p, o.pts.data(), (INT)o.pts.size());
        break;
    }
    default: break;
    }
    g.Restore(gs);
}

static void DrawTextBlock(Graphics& g, const DrawObj& o, RectF r, bool center, bool shadow) {
    if (o.text.empty()) return;
    const Style& s = o.style;
    FontFamily ff(s.font.c_str());
    FontFamily fallback(L"Segoe UI");
    Font font(ff.IsAvailable() ? &ff : &fallback, s.fontSize, s.bold ? FontStyleBold : FontStyleRegular, UnitPixel);
    StringFormat sf;
    sf.SetAlignment(center ? StringAlignmentCenter : StringAlignmentNear);
    sf.SetLineAlignment(center ? StringAlignmentCenter : StringAlignmentNear);
    sf.SetTrimming(StringTrimmingWord);
    if (shadow) {
        SolidBrush sb(Color((BYTE)(110 * s.opacity / 100), 0, 0, 0));
        RectF r2 = r; r2.Offset(2, 2);
        g.DrawString(o.text.c_str(), -1, &font, r2, &sf, &sb);
    }
    SolidBrush b(Gc(s.textColor, s.opacity));
    g.DrawString(o.text.c_str(), -1, &font, r, &sf, &b);
}

void RenderObject(Graphics& g, Image& canvas, const DrawObj& o, bool hideText) {
    const Style& s = o.style;
    switch (o.type) {
    case ObjType::Arrow: case ObjType::Line: case ObjType::Rect: case ObjType::RoundRect:
    case ObjType::Ellipse: case ObjType::Pen: case ObjType::Highlighter:
        if (s.shadow) DrawShape(g, o, true);
        DrawShape(g, o, false);
        break;
    case ObjType::Text: {
        RectF r = o.Rect();
        if ((s.fill >> 24)) {
            GraphicsPath gp; AddRoundRect(gp, r, 6);
            SolidBrush b(Gc(s.fill, s.opacity)); g.FillPath(&b, &gp);
            if (s.width > 0 && (s.stroke >> 24)) { Gdiplus::Pen p(Gc(s.stroke, s.opacity), std::min(s.width, 3.0f)); g.DrawPath(&p, &gp); }
        }
        RectF tr = r; tr.Inflate(-4, -2);
        if (!hideText) DrawTextBlock(g, o, tr, false, s.shadow && !(s.fill >> 24));
        break;
    }
    case ObjType::Callout: {
        GraphicsPath gp;
        gp.SetFillMode(FillModeWinding);
        CalloutPath(gp, o);
        if (s.shadow) {
            GraphicsState gs = g.Save(); g.TranslateTransform(4, 4);
            SolidBrush sb(Color((BYTE)(60 * s.opacity / 100), 0, 0, 0)); g.FillPath(&sb, &gp);
            g.Restore(gs);
        }
        if (s.fill >> 24) { SolidBrush b(Gc(s.fill, s.opacity)); g.FillPath(&b, &gp); }
        if (s.width > 0) {
            // contorno da união (sem as linhas internas da cauda)
            GraphicsPath outline; outline.SetFillMode(FillModeWinding); CalloutPath(outline, o);
            outline.Outline();
            Gdiplus::Pen p(Gc(s.stroke, s.opacity), std::min(s.width, 6.0f)); p.SetLineJoin(LineJoinRound);
            g.DrawPath(&p, &outline);
        }
        RectF r = o.Rect();
        float in = std::min(r.Width, r.Height) * (s.calloutShape >= 2 ? 0.18f : 0.08f) + 4;
        r.Inflate(-in, -in);
        if (!hideText) DrawTextBlock(g, o, r, true, false);
        break;
    }
    case ObjType::Step: {
        float d = s.fontSize * 1.5f;
        RectF r(o.p1.X - d / 2, o.p1.Y - d / 2, d, d);
        if (s.shadow) { SolidBrush sb(Color(70, 0, 0, 0)); g.FillEllipse(&sb, r.X + 3, r.Y + 3, d, d); }
        SolidBrush b(Gc(s.stroke, s.opacity));
        g.FillEllipse(&b, r);
        Gdiplus::Pen ring(Color((BYTE)(230 * s.opacity / 100), 255, 255, 255), std::max(2.0f, d * 0.06f));
        g.DrawEllipse(&ring, r.X + d * 0.06f, r.Y + d * 0.06f, d * 0.88f, d * 0.88f);
        FontFamily ff(L"Segoe UI");
        Font f(&ff, s.fontSize * 0.78f, FontStyleBold, UnitPixel);
        StringFormat sf; sf.SetAlignment(StringAlignmentCenter); sf.SetLineAlignment(StringAlignmentCenter);
        SolidBrush w(Gc(s.textColor == 0xFF000000 ? 0xFFFFFFFF : s.textColor, s.opacity));
        std::wstring t = std::to_wstring(o.stepValue);
        g.DrawString(t.c_str(), -1, &f, RectF(r.X, r.Y + d * 0.03f, d, d), &sf, &w);
        break;
    }
    case ObjType::Stamp: {
        RectF r = o.Rect();
        FontFamily ff(L"Segoe UI Symbol");
        Font f(&ff, std::max(8.0f, r.Height * 0.8f), FontStyleRegular, UnitPixel);
        StringFormat sf; sf.SetAlignment(StringAlignmentCenter); sf.SetLineAlignment(StringAlignmentCenter);
        if (s.shadow) { SolidBrush sb(Color(80, 0, 0, 0)); RectF r2 = r; r2.Offset(3, 3); g.DrawString(o.text.c_str(), -1, &f, r2, &sf, &sb); }
        SolidBrush b(Gc(s.stroke, s.opacity));
        g.DrawString(o.text.c_str(), -1, &f, r, &sf, &b);
        break;
    }
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
            Gdiplus::Pen p(Color(200, 255, 255, 255), 2);
            g.DrawRectangle(&p, rf);
        }
        break;
    }
    case ObjType::Magnify: {
        g.Flush(FlushIntentionSync);
        RectF r = o.Rect();
        if (r.Width < 4 || r.Height < 4) break;
        float zoom = std::clamp(s.amount / 5.0f, 1.5f, 6.0f);
        float sw = r.Width / zoom, sh = r.Height / zoom;
        float cx = r.X + r.Width / 2, cy = r.Y + r.Height / 2;
        Image src = canvas.Crop({ (LONG)(cx - sw / 2), (LONG)(cy - sh / 2), (LONG)(cx + sw / 2), (LONG)(cy + sh / 2) });
        if (src.empty()) break;
        auto sb = src.WrapGdip();
        GraphicsPath clip; clip.AddEllipse(r);
        if (s.shadow) { SolidBrush shb(Color(70, 0, 0, 0)); g.FillEllipse(&shb, r.X + 4, r.Y + 4, r.Width, r.Height); }
        GraphicsState gs = g.Save();
        g.SetClip(&clip);
        g.DrawImage(sb.get(), r);
        g.Restore(gs);
        Gdiplus::Pen p(Gc(s.stroke, s.opacity), std::max(2.0f, s.width));
        g.DrawEllipse(&p, r);
        break;
    }
    case ObjType::Picture: {
        if (!o.picture || o.picture->empty()) break;
        RectF r = o.Rect();
        auto pb = o.picture->WrapGdip();
        if (s.shadow) { SolidBrush shb(Color(60, 0, 0, 0)); g.FillRectangle(&shb, r.X + 4, r.Y + 4, r.Width, r.Height); }
        ImageAttributes ia;
        ColorMatrix cm = { { { 1, 0, 0, 0, 0 }, { 0, 1, 0, 0, 0 }, { 0, 0, 1, 0, 0 }, { 0, 0, 0, s.opacity / 100.0f, 0 }, { 0, 0, 0, 0, 1 } } };
        ia.SetColorMatrix(&cm);
        g.DrawImage(pb.get(), r, 0, 0, (REAL)o.picture->w, (REAL)o.picture->h, UnitPixel, &ia);
        if (s.width > 0 && (s.stroke >> 24)) {
            Gdiplus::Pen p(Gc(s.stroke, s.opacity), s.width); g.DrawRectangle(&p, r);
        }
        break;
    }
    }
}

} // namespace snap::ed
