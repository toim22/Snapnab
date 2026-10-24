#include "Icons.h"
#include <cmath>

namespace snap::icons {

using namespace Gdiplus;

void Draw(Graphics& g, Id id, RectF r, Color c) {
    GraphicsState st = g.Save();
    g.SetSmoothingMode(SmoothingModeAntiAlias);
    g.TranslateTransform(r.X, r.Y);
    g.ScaleTransform(r.Width / 24.0f, r.Height / 24.0f);
    Gdiplus::Pen p(c, 1.8f);
    p.SetLineJoin(LineJoinRound);
    p.SetStartCap(LineCapRound); p.SetEndCap(LineCapRound);
    Gdiplus::Pen thin(c, 1.2f);
    Gdiplus::Pen p3(c, 3.f), p26(c, 2.6f), p24(c, 2.4f);
    p3.SetEndCap(LineCapRound); p26.SetStartCap(LineCapRound); p26.SetEndCap(LineCapRound); p24.SetEndCap(LineCapRound);
    SolidBrush b(c);
    SolidBrush faint(Color(70, c.GetR(), c.GetG(), c.GetB()));
    auto arrowHead = [&](PointF tip, float ang, float sz) {
        PointF pts[3] = { tip, { tip.X - sz * std::cos(ang - 0.45f), tip.Y - sz * std::sin(ang - 0.45f) },
                          { tip.X - sz * std::cos(ang + 0.45f), tip.Y - sz * std::sin(ang + 0.45f) } };
        g.FillPolygon(&b, pts, 3);
    };
    switch (id) {
    case Capture:
        g.DrawLine(&p, 12.f, 2.f, 12.f, 8.f); g.DrawLine(&p, 12.f, 16.f, 12.f, 22.f);
        g.DrawLine(&p, 2.f, 12.f, 8.f, 12.f); g.DrawLine(&p, 16.f, 12.f, 22.f, 12.f);
        g.DrawEllipse(&p, 7.f, 7.f, 10.f, 10.f);
        break;
    case Image:
        g.DrawRectangle(&p, 3.f, 5.f, 18.f, 14.f);
        { PointF m[3] = { { 5, 17 }, { 10, 11 }, { 14, 15 } }; g.DrawLines(&p, m, 3); }
        { PointF m[3] = { { 12, 14 }, { 15, 11 }, { 19, 17 } }; g.DrawLines(&p, m, 3); }
        g.FillEllipse(&b, 15.f, 7.f, 3.f, 3.f);
        break;
    case Video:
        g.DrawRectangle(&p, 2.5f, 6.f, 13.f, 12.f);
        { PointF t[3] = { { 16, 10 }, { 21.5f, 7 }, { 21.5f, 17 } }; g.DrawPolygon(&p, t, 3); }
        break;
    case Record: g.FillEllipse(&b, 5.f, 5.f, 14.f, 14.f); break;
    case Pause: g.FillRectangle(&b, 6.f, 5.f, 4.f, 14.f); g.FillRectangle(&b, 14.f, 5.f, 4.f, 14.f); break;
    case Stop: g.FillRectangle(&b, 6.f, 6.f, 12.f, 12.f); break;
    case Restart:
        g.DrawArc(&p, 5.f, 5.f, 14.f, 14.f, -60.f, 300.f);
        arrowHead({ 18.5f, 6.5f }, -0.2f, 5.f);
        break;
    case Cancel: g.DrawLine(&p, 6.f, 6.f, 18.f, 18.f); g.DrawLine(&p, 18.f, 6.f, 6.f, 18.f); break;
    case Mic: case MicOff:
        g.DrawRectangle(&p, 9.f, 3.f, 6.f, 11.f);
        g.DrawArc(&p, 6.f, 6.f, 12.f, 11.f, 0.f, 180.f);
        g.DrawLine(&p, 12.f, 17.f, 12.f, 21.f); g.DrawLine(&p, 8.f, 21.f, 16.f, 21.f);
        if (id == MicOff) g.DrawLine(&p, 4.f, 4.f, 20.f, 20.f);
        break;
    case Speaker: case SpeakerOff: {
        PointF s[6] = { { 3, 9 }, { 7, 9 }, { 12, 4 }, { 12, 20 }, { 7, 15 }, { 3, 15 } };
        g.DrawPolygon(&p, s, 6);
        if (id == Speaker) { g.DrawArc(&p, 11.f, 8.f, 6.f, 8.f, -60.f, 120.f); g.DrawArc(&p, 11.f, 5.f, 10.f, 14.f, -60.f, 120.f); }
        else { g.DrawLine(&p, 15.f, 9.f, 21.f, 15.f); g.DrawLine(&p, 21.f, 9.f, 15.f, 15.f); }
        break;
    }
    case Camera:
        g.DrawRectangle(&p, 3.f, 7.f, 18.f, 12.f);
        g.DrawRectangle(&p, 8.f, 4.f, 8.f, 3.f);
        g.DrawEllipse(&p, 8.5f, 9.5f, 7.f, 7.f);
        break;
    case Select: {
        PointF s[7] = { { 6, 3 }, { 6, 19 }, { 10, 15 }, { 13, 21 }, { 15.5f, 20 }, { 12.5f, 14 }, { 18, 14 } };
        g.FillPolygon(&faint, s, 7); g.DrawPolygon(&thin, s, 7);
        break;
    }
    case Arrow: g.DrawLine(&p, 4.f, 20.f, 17.f, 7.f); arrowHead({ 20.f, 4.f }, -0.785f, 8.f); break;
    case Line: g.DrawLine(&p, 4.f, 20.f, 20.f, 4.f); break;
    case Rect: g.DrawRectangle(&p, 4.f, 6.f, 16.f, 12.f); break;
    case Ellipse: g.DrawEllipse(&p, 3.f, 6.f, 18.f, 12.f); break;
    case Pen: {
        PointF s[5] = { { 3, 17 }, { 7, 12 }, { 11, 16 }, { 16, 8 }, { 21, 12 } };
        g.DrawCurve(&p, s, 5);
        break;
    }
    case Highlighter: {
        SolidBrush hb(Color(150, 255, 214, 0));
        g.FillRectangle(&hb, 3.f, 15.f, 18.f, 5.f);
        PointF s[4] = { { 9, 14 }, { 15, 4 }, { 19, 6 }, { 13, 16 } };
        g.DrawPolygon(&p, s, 4);
        break;
    }
    case Text: {
        FontFamily ff(L"Georgia");
        Font f(&ff, 19, FontStyleBold, UnitPixel);
        StringFormat sf; sf.SetAlignment(StringAlignmentCenter); sf.SetLineAlignment(StringAlignmentCenter);
        g.DrawString(L"T", 1, &f, RectF(0, 0, 24, 25), &sf, &b);
        break;
    }
    case Callout: {
        GraphicsPath gp;
        gp.AddArc(3.f, 4.f, 6.f, 6.f, 180.f, 90.f); gp.AddArc(15.f, 4.f, 6.f, 6.f, 270.f, 90.f);
        gp.AddArc(15.f, 10.f, 6.f, 6.f, 0.f, 90.f);
        gp.AddLine(12.f, 16.f, 8.f, 21.f); gp.AddLine(8.f, 21.f, 8.f, 16.f);
        gp.AddArc(3.f, 10.f, 6.f, 6.f, 90.f, 90.f);
        gp.CloseFigure();
        g.DrawPath(&p, &gp);
        break;
    }
    case Step: {
        g.FillEllipse(&b, 3.f, 3.f, 18.f, 18.f);
        FontFamily ff(L"Segoe UI");
        Font f(&ff, 12, FontStyleBold, UnitPixel);
        SolidBrush w(Color(255, 255, 255, 255));
        StringFormat sf; sf.SetAlignment(StringAlignmentCenter); sf.SetLineAlignment(StringAlignmentCenter);
        g.DrawString(L"1", 1, &f, RectF(3, 3.5f, 18, 18), &sf, &w);
        break;
    }
    case Stamp: {
        PointF s[10];
        for (int i = 0; i < 10; i++) {
            float a = -1.5708f + i * 3.14159f / 5, rr = i % 2 ? 4.2f : 9.5f;
            s[i] = { 12 + rr * std::cos(a), 12.5f + rr * std::sin(a) };
        }
        g.DrawPolygon(&p, s, 10);
        break;
    }
    case Blur:
        for (int y = 0; y < 3; y++)
            for (int x = 0; x < 3; x++) {
                SolidBrush cb(Color((BYTE)((x + y) % 2 ? 90 : 220), c.GetR(), c.GetG(), c.GetB()));
                g.FillRectangle(&cb, 4.f + x * 5.5f, 4.f + y * 5.5f, 5.f, 5.f);
            }
        break;
    case Spotlight: {
        g.FillRectangle(&faint, 2.f, 3.f, 20.f, 18.f);
        SolidBrush w(Color(255, 255, 255, 255));
        g.FillEllipse(&w, 7.f, 7.f, 10.f, 10.f);
        g.DrawEllipse(&p, 7.f, 7.f, 10.f, 10.f);
        break;
    }
    case Magnify: case Ocr:
        g.DrawEllipse(&p, 3.f, 3.f, 13.f, 13.f);
        g.DrawLine(&p3, 14.5f, 14.5f, 20.5f, 20.5f);
        if (id == Ocr) {
            FontFamily ff(L"Segoe UI"); Font f(&ff, 7, FontStyleBold, UnitPixel);
            StringFormat sf; sf.SetAlignment(StringAlignmentCenter); sf.SetLineAlignment(StringAlignmentCenter);
            g.DrawString(L"Aa", 2, &f, RectF(3, 3, 13, 13), &sf, &b);
        }
        break;
    case Fill: {
        PointF s[4] = { { 4, 11 }, { 11, 4 }, { 18, 11 }, { 11, 18 } };
        g.DrawPolygon(&p, s, 4);
        g.FillEllipse(&b, 17.f, 15.f, 4.f, 5.f);
        break;
    }
    case Eraser: {
        PointF s[4] = { { 3, 15 }, { 12, 6 }, { 19, 13 }, { 10, 22 } };
        g.DrawPolygon(&p, s, 4);
        g.DrawLine(&p, 7.5f, 10.5f, 14.5f, 17.5f);
        g.DrawLine(&p, 10.f, 21.f, 21.f, 21.f);
        break;
    }
    case Crop:
        g.DrawLine(&p, 6.f, 2.f, 6.f, 18.f); g.DrawLine(&p, 6.f, 18.f, 22.f, 18.f);
        g.DrawLine(&p, 2.f, 6.f, 18.f, 6.f); g.DrawLine(&p, 18.f, 6.f, 18.f, 22.f);
        break;
    case Selection: {
        Gdiplus::Pen d(c, 1.6f); d.SetDashStyle(DashStyleDash);
        g.DrawRectangle(&d, 4.f, 5.f, 16.f, 14.f);
        break;
    }
    case Wand:
        g.DrawLine(&p26, 4.f, 20.f, 15.f, 9.f);
        for (int i = 0; i < 4; i++) {
            float a = i * 1.5708f; g.DrawLine(&thin, 18 + 2 * std::cos(a), 6 + 2 * std::sin(a), 18 + 4.5f * std::cos(a), 6 + 4.5f * std::sin(a));
        }
        break;
    case Cutout: {
        Gdiplus::Pen d(c, 1.6f); d.SetDashStyle(DashStyleDash);
        g.DrawLine(&d, 2.f, 10.f, 22.f, 10.f); g.DrawLine(&d, 2.f, 14.f, 22.f, 14.f);
        g.DrawRectangle(&p, 4.f, 3.f, 16.f, 18.f);
        break;
    }
    case Eyedropper:
        g.DrawLine(&p24, 5.f, 19.f, 14.f, 10.f);
        g.FillEllipse(&b, 13.f, 4.f, 7.f, 7.f);
        g.DrawLine(&p, 11.f, 8.f, 16.f, 13.f);
        break;
    case Undo: case Redo: {
        if (id == Redo) { g.TranslateTransform(24, 0); g.ScaleTransform(-1, 1); }
        g.DrawArc(&p, 6.f, 7.f, 14.f, 12.f, 270.f, 180.f);
        g.DrawLine(&p, 13.f, 19.f, 9.f, 19.f);
        g.DrawLine(&p, 13.f, 7.f, 5.f, 7.f);
        arrowHead({ 3.5f, 7.f }, 3.14159f, 5.f);
        break;
    }
    case Editor:
        g.DrawRectangle(&p, 3.f, 4.f, 14.f, 14.f);
        g.DrawLine(&p26, 10.f, 18.f, 20.f, 8.f);
        break;
    case Folder: {
        PointF s[6] = { { 3, 6 }, { 9, 6 }, { 11, 8 }, { 21, 8 }, { 21, 19 }, { 3, 19 } };
        g.DrawPolygon(&p, s, 6);
        break;
    }
    case Gear:
        g.DrawEllipse(&p, 8.f, 8.f, 8.f, 8.f);
        for (int i = 0; i < 8; i++) {
            float a = i * 0.785f;
            g.DrawLine(&p26, 12 + 6 * std::cos(a), 12 + 6 * std::sin(a), 12 + 9 * std::cos(a), 12 + 9 * std::sin(a));
        }
        break;
    case Copy:
        g.DrawRectangle(&p, 8.f, 8.f, 12.f, 13.f);
        { PointF s[4] = { { 5, 16 }, { 5, 4 }, { 15, 4 }, { 15, 6 } }; g.DrawLines(&p, s, 4); }
        break;
    case Save:
        g.DrawRectangle(&p, 4.f, 4.f, 16.f, 16.f);
        g.DrawRectangle(&p, 8.f, 4.f, 8.f, 5.f);
        g.DrawRectangle(&p, 7.f, 13.f, 10.f, 7.f);
        break;
    case Share:
        g.DrawEllipse(&p, 3.f, 9.f, 5.f, 5.f); g.DrawEllipse(&p, 16.f, 3.f, 5.f, 5.f); g.DrawEllipse(&p, 16.f, 16.f, 5.f, 5.f);
        g.DrawLine(&p, 7.5f, 10.5f, 16.5f, 6.5f); g.DrawLine(&p, 7.5f, 12.5f, 16.5f, 17.5f);
        break;
    case Library:
        g.DrawRectangle(&p, 3.f, 4.f, 7.f, 7.f); g.DrawRectangle(&p, 14.f, 4.f, 7.f, 7.f);
        g.DrawRectangle(&p, 3.f, 14.f, 7.f, 7.f); g.DrawRectangle(&p, 14.f, 14.f, 7.f, 7.f);
        break;
    }
    g.Restore(st);
}

void DrawOnDC(HDC dc, Id id, RECT r, COLORREF c) {
    Graphics g(dc);
    Draw(g, id, RectF((REAL)r.left, (REAL)r.top, (REAL)(r.right - r.left), (REAL)(r.bottom - r.top)),
         Color(255, GetRValue(c), GetGValue(c), GetBValue(c)));
}

HICON MakeAppIcon(int size, bool editor) {
    Bitmap bmp(size, size, PixelFormat32bppARGB);
    {
        Graphics g(&bmp);
        g.SetSmoothingMode(SmoothingModeAntiAlias);
        g.Clear(Color(0, 0, 0, 0));
        float s = (float)size;
        GraphicsPath gp;
        float rr = s * 0.22f;
        gp.AddArc(0.f, 0.f, rr, rr, 180.f, 90.f); gp.AddArc(s - rr - 1, 0.f, rr, rr, 270.f, 90.f);
        gp.AddArc(s - rr - 1, s - rr - 1, rr, rr, 0.f, 90.f); gp.AddArc(0.f, s - rr - 1, rr, rr, 90.f, 90.f);
        gp.CloseFigure();
        LinearGradientBrush lg(PointF(0, 0), PointF(s, s), editor ? Color(255, 255, 122, 60) : Color(255, 30, 140, 255),
                               editor ? Color(255, 214, 40, 90) : Color(255, 20, 70, 200));
        g.FillPath(&lg, &gp);
        Draw(g, editor ? Editor : Capture, RectF(s * 0.14f, s * 0.14f, s * 0.72f, s * 0.72f), Color(255, 255, 255, 255));
    }
    HICON ic = nullptr;
    bmp.GetHICON(&ic);
    return ic;
}

} // namespace snap::icons
