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
    case Step: {              // steps: "1" and "2" behind, "3" in front
        FontFamily ff(L"Segoe UI");
        Font f(&ff, 8.5f, FontStyleBold, UnitPixel), f1(&ff, 7.0f, FontStyleBold, UnitPixel);
        StringFormat sf; sf.SetAlignment(StringAlignmentCenter); sf.SetLineAlignment(StringAlignmentCenter);
        SolidBrush white(Color(255, 255, 255, 255));
        Gdiplus::Pen ring1(Color(150, c.GetR(), c.GetG(), c.GetB()), 1.0f);
        g.FillEllipse(&white, 1.5f, 1.f, 9.f, 9.f);                    // 1: white dot with the number in the color of 2
        g.DrawEllipse(&ring1, 1.5f, 1.f, 9.f, 9.f);
        g.DrawString(L"1", 1, &f1, RectF(1.5f, 1.4f, 9.f, 9.f), &sf, &b);
        g.FillEllipse(&faint, 10.5f, 2.f, 11.5f, 11.5f);
        g.DrawString(L"2", 1, &f, RectF(10.5f, 2.5f, 11.5f, 11.5f), &sf, &b);
        g.FillEllipse(&white, 1.f, 8.5f, 14.f, 14.f);
        g.FillEllipse(&b, 2.f, 9.5f, 12.f, 12.f);
        g.DrawString(L"3", 1, &f, RectF(2.f, 10.f, 12.f, 12.f), &sf, &white);
        break;
    }
    case Stamp: {             // rubber stamp: handle, stem, body and base
        g.FillEllipse(&b, 8.6f, 1.6f, 6.8f, 6.8f);
        g.FillRectangle(&b, 10.6f, 7.f, 2.8f, 4.5f);
        PointF body[] = { { 7.f, 11.f }, { 17.f, 11.f }, { 18.5f, 14.5f }, { 5.5f, 14.5f } };
        g.FillPolygon(&b, body, 4);
        g.FillRectangle(&b, 3.5f, 14.f, 17.f, 4.2f);
        Gdiplus::Pen base(c, 1.6f);
        g.DrawLine(&base, 4.f, 21.f, 20.f, 21.f);
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
    case Fill: {              // tilted bucket with its mouth up to the left, upright handle and a falling paint drop
        Color light(150, c.GetR(), c.GetG(), c.GetB());
        SolidBrush lb(light);
        Gdiplus::Pen handle(light, 2.f);
        handle.SetStartCap(LineCapRound); handle.SetEndCap(LineCapFlat);
        g.DrawLine(&handle, 10.f, 1.2f, 10.f, 8.6f);
        PointF bucket[] = { { 12.f, 5.f }, { 13.5f, 3.5f }, { 22.5f, 12.5f }, { 13.5f, 21.5f }, { 4.5f, 12.5f }, { 7.f, 10.f }, { 12.f, 10.f } };
        Gdiplus::Pen edge(c, 1.4f);
        edge.SetLineJoin(LineJoinRound);
        g.FillPolygon(&b, bucket, 7);
        g.DrawPolygon(&edge, bucket, 7);              // rounded corners
        GraphicsPath drop;
        drop.AddBezier(4.5f, 15.f, 5.5f, 16.7f, 7.f, 18.3f, 7.f, 20.4f);
        drop.AddArc(2.f, 17.9f, 5.f, 5.f, 0.f, 180.f);
        drop.AddBezier(2.f, 20.4f, 2.f, 18.3f, 3.5f, 16.7f, 4.5f, 15.f);
        drop.CloseFigure();
        g.FillPath(&lb, &drop);
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
    case Cutout: {            // cut out: image with a zigzag tear
        GraphicsPath gp;
        float r = 2.5f, d = r * 2;
        gp.AddArc(2.f, 4.f, d, d, 180, 90); gp.AddArc(22.f - d, 4.f, d, d, 270, 90);
        gp.AddArc(22.f - d, 20.f - d, d, d, 0, 90); gp.AddArc(2.f, 20.f - d, d, d, 90, 90);
        gp.CloseFigure();
        g.FillPath(&b, &gp);
        Gdiplus::Pen zig(Color(255, 255, 255, 255), 2.2f);
        zig.SetLineJoin(LineJoinMiter);
        PointF z[] = { { 1, 10.5f }, { 5, 7.5f }, { 8.5f, 11 }, { 12, 7.5f }, { 15.5f, 11 }, { 19, 7.5f }, { 23, 10.5f } };
        g.DrawLines(&zig, z, 7);
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
    case CameraFill: {        // filled camera (tabs and lists of the capture window, material style)
        GraphicsPath gp(FillModeAlternate);
        gp.AddLine(4.5f, 6.f, 8.f, 6.f); gp.AddLine(8.f, 6.f, 9.6f, 3.6f); gp.AddLine(9.6f, 3.6f, 14.4f, 3.6f);
        gp.AddLine(14.4f, 3.6f, 16.f, 6.f); gp.AddLine(16.f, 6.f, 19.5f, 6.f);
        gp.AddArc(17.f, 6.f, 5.f, 5.f, 270.f, 90.f); gp.AddArc(17.f, 15.f, 5.f, 5.f, 0.f, 90.f);
        gp.AddArc(2.f, 15.f, 5.f, 5.f, 90.f, 90.f); gp.AddArc(2.f, 6.f, 5.f, 5.f, 180.f, 90.f);
        gp.CloseFigure();
        gp.AddEllipse(7.4f, 8.4f, 9.2f, 9.2f);
        g.FillPath(&b, &gp);
        g.FillEllipse(&b, 9.3f, 10.3f, 5.4f, 5.4f);
        break;
    }
    case VideoFill: {
        GraphicsPath gp;
        gp.AddArc(2.f, 6.f, 5.f, 5.f, 180.f, 90.f); gp.AddArc(11.5f, 6.f, 5.f, 5.f, 270.f, 90.f);
        gp.AddArc(11.5f, 13.f, 5.f, 5.f, 0.f, 90.f); gp.AddArc(2.f, 13.f, 5.f, 5.f, 90.f, 90.f);
        gp.CloseFigure();
        g.FillPath(&b, &gp);
        PointF t[4] = { { 17.5f, 10.4f }, { 22.f, 7.4f }, { 22.f, 16.6f }, { 17.5f, 13.6f } };
        g.FillPolygon(&b, t, 4);
        break;
    }
    case Mail:
        g.DrawRectangle(&p, 3.f, 6.f, 18.f, 13.f);
        { PointF s[3] = { { 3.5f, 6.5f }, { 12, 13 }, { 20.5f, 6.5f } }; g.DrawLines(&p, s, 3); }
        break;
    case Print:
        g.DrawRectangle(&p, 3.f, 9.f, 18.f, 8.f);
        { PointF s[4] = { { 7, 9 }, { 7, 3.5f }, { 17, 3.5f }, { 17, 9 } }; g.DrawLines(&p, s, 4); }
        g.DrawRectangle(&p, 7.f, 14.f, 10.f, 6.5f);
        break;
    case NewDoc: {
        PointF s[6] = { { 5, 3 }, { 14, 3 }, { 19, 8 }, { 19, 21 }, { 5, 21 }, { 5, 3 } };
        g.DrawLines(&p, s, 6);
        { PointF f[3] = { { 14, 3 }, { 14, 8 }, { 19, 8 } }; g.DrawLines(&p, f, 3); }
        g.DrawLine(&p, 12.f, 11.f, 12.f, 18.f); g.DrawLine(&p, 8.5f, 14.5f, 15.5f, 14.5f);
        break;
    }
    case Trash:
        g.DrawLine(&p, 4.f, 6.5f, 20.f, 6.5f);
        { PointF s[4] = { { 9, 6 }, { 9.5f, 3.5f }, { 14.5f, 3.5f }, { 15, 6 } }; g.DrawLines(&p, s, 4); }
        { PointF s[4] = { { 6, 7 }, { 7, 21 }, { 17, 21 }, { 18, 7 } }; g.DrawLines(&p, s, 4); }
        g.DrawLine(&thin, 10.f, 10.f, 10.f, 18.f); g.DrawLine(&thin, 14.f, 10.f, 14.f, 18.f);
        break;
    case Layers: {
        PointF top[4] = { { 12, 3 }, { 21, 8 }, { 12, 13 }, { 3, 8 } };
        g.FillPolygon(&faint, top, 4); g.DrawPolygon(&p, top, 4);
        { PointF s[3] = { { 3, 12 }, { 12, 17 }, { 21, 12 } }; g.DrawLines(&p, s, 3); }
        { PointF s[3] = { { 3, 16 }, { 12, 21 }, { 21, 16 } }; g.DrawLines(&p, s, 3); }
        break;
    }
    case Rotate:
        g.DrawArc(&p, 4.f, 5.f, 15.f, 15.f, 200.f, 250.f);
        arrowHead({ 19.f, 9.f }, 1.35f, 5.5f);
        g.DrawRectangle(&thin, 9.f, 10.f, 6.f, 6.f);
        break;
    case Effects: {           // wand with sparkles: image effects
        g.DrawLine(&p26, 4.f, 20.f, 14.f, 10.f);
        auto spark = [&](float x, float y, float r) {
            PointF s[8] = { { x, y - r }, { x + r * .28f, y - r * .28f }, { x + r, y }, { x + r * .28f, y + r * .28f },
                            { x, y + r }, { x - r * .28f, y + r * .28f }, { x - r, y }, { x - r * .28f, y - r * .28f } };
            g.FillPolygon(&b, s, 8);
        };
        spark(17.f, 7.f, 4.5f); spark(9.f, 5.f, 2.6f); spark(20.f, 15.f, 2.6f);
        break;
    }
    case Star: {
        PointF s[10];
        for (int i = 0; i < 10; i++) {
            float a = -1.5708f + i * 3.14159f / 5, rr = i % 2 ? 4.4f : 10.0f;
            s[i] = { 12 + rr * std::cos(a), 12.8f + rr * std::sin(a) };
        }
        g.FillPolygon(&b, s, 10);
        break;
    }
    case Clipboard:
        g.DrawRectangle(&p, 5.f, 5.f, 14.f, 16.f);
        g.FillRectangle(&b, 9.f, 3.f, 6.f, 4.f);
        g.DrawLine(&thin, 8.f, 11.f, 16.f, 11.f); g.DrawLine(&thin, 8.f, 15.f, 14.f, 15.f);
        break;
    case Program:
        g.DrawRectangle(&p, 3.f, 4.f, 18.f, 15.f);
        g.FillRectangle(&b, 3.f, 4.f, 18.f, 4.f);
        g.DrawLine(&thin, 7.f, 12.f, 17.f, 12.f); g.DrawLine(&thin, 7.f, 15.f, 13.f, 15.f);
        break;
    case Duplicate:
        g.DrawRectangle(&p, 4.f, 4.f, 11.f, 11.f);
        g.FillRectangle(&faint, 9.f, 9.f, 11.f, 11.f); g.DrawRectangle(&p, 9.f, 9.f, 11.f, 11.f);
        break;
    case SaveAs:
        g.DrawRectangle(&p, 3.f, 3.f, 14.f, 14.f);
        g.DrawRectangle(&p, 6.f, 3.f, 7.f, 4.f);
        g.DrawLine(&p26, 13.f, 21.f, 21.f, 13.f);
        break;
    case NewClipboard:
        g.DrawRectangle(&p, 4.f, 5.f, 12.f, 15.f);
        g.FillRectangle(&b, 7.f, 3.f, 6.f, 4.f);
        g.DrawLine(&p, 18.f, 14.f, 18.f, 22.f); g.DrawLine(&p, 14.f, 18.f, 22.f, 18.f);
        break;
    case AutoTrim: {
        Gdiplus::Pen d(c, 1.4f); d.SetDashStyle(DashStyleDash);
        g.DrawRectangle(&d, 3.f, 3.f, 18.f, 18.f);
        g.FillRectangle(&b, 8.f, 8.f, 8.f, 8.f);
        break;
    }
    case DeleteObject:
        g.DrawEllipse(&p, 3.f, 6.f, 13.f, 12.f);
        g.FillEllipse(&b, 13.f, 2.f, 9.f, 9.f);
        { Gdiplus::Pen w(Color(255, 255, 255, 255), 1.4f); g.DrawLine(&w, 15.5f, 4.5f, 19.5f, 8.5f); g.DrawLine(&w, 19.5f, 4.5f, 15.5f, 8.5f); }
        break;
    case FxBorder: g.DrawRectangle(&p3, 4.5f, 4.5f, 15.f, 15.f); break;
    case FxEdges: {
        PointF s[] = { { 4, 4 }, { 20, 4 }, { 20, 16 }, { 17, 19 }, { 14, 16 }, { 11, 20 }, { 8, 17 }, { 4, 20 } };
        g.FillPolygon(&b, s, 8);
        break;
    }
    case FxCaption: g.DrawRectangle(&p, 3.f, 4.f, 18.f, 16.f); g.FillRectangle(&b, 3.f, 15.f, 18.f, 5.f); break;
    case FxPerspective: { PointF s[] = { { 4, 6 }, { 20, 3 }, { 20, 21 }, { 4, 18 } }; g.FillPolygon(&b, s, 4); break; }
    case FxPageCurl: {
        PointF s[] = { { 4, 3 }, { 20, 3 }, { 20, 14 }, { 13, 21 }, { 4, 21 } };
        g.FillPolygon(&b, s, 5);
        PointF f[] = { { 20, 14 }, { 13, 14 }, { 13, 21 } };
        SolidBrush w(Color(255, 255, 255, 255)); g.FillPolygon(&w, f, 3);
        Gdiplus::Pen fp(c, 1.2f); g.DrawPolygon(&fp, f, 3);
        break;
    }
    case FxShadow:
        g.FillRectangle(&faint, 8.f, 8.f, 13.f, 13.f);
        g.FillRectangle(&b, 4.f, 4.f, 13.f, 13.f);
        break;
    case FxFilters: {
        SolidBrush a(Color(200, c.GetR(), c.GetG(), c.GetB()));
        g.FillEllipse(&a, 3.f, 3.f, 11.f, 11.f); g.FillEllipse(&faint, 10.f, 3.f, 11.f, 11.f); g.FillEllipse(&faint, 6.5f, 10.f, 11.f, 11.f);
        break;
    }
    case FxColorAdjust:
        for (int i = 0; i < 4; i++) { SolidBrush cb(Color((BYTE)(70 + i * 60), c.GetR(), c.GetG(), c.GetB())); g.FillRectangle(&cb, 4.f, 4.f + i * 4.f, 16.f, 4.f); }
        break;
    case FxColorReplace:
        g.DrawLine(&p24, 5.f, 19.f, 14.f, 10.f);
        g.FillEllipse(&b, 13.f, 4.f, 7.f, 7.f);
        g.DrawLine(&p, 11.f, 8.f, 16.f, 13.f);
        g.FillEllipse(&faint, 3.f, 15.f, 6.f, 6.f);
        break;
    case FxSpotlight:
        g.DrawEllipse(&p, 3.f, 3.f, 13.f, 13.f);
        g.DrawLine(&p3, 14.5f, 14.5f, 20.5f, 20.5f);
        g.FillEllipse(&faint, 6.f, 6.f, 7.f, 7.f);
        break;
    case FxWatermark: {
        PointF m[] = { { 3, 19 }, { 9, 9 }, { 13, 15 }, { 16, 11 }, { 21, 19 } };
        g.FillPolygon(&faint, m, 5);
        g.DrawPolygon(&p, m, 5);
        g.FillEllipse(&b, 15.f, 4.f, 4.f, 4.f);
        break;
    }
    case Webcam: case WebcamOff:
        g.DrawEllipse(&p, 5.5f, 3.f, 13.f, 13.f);
        g.FillEllipse(&b, 9.6f, 7.1f, 4.8f, 4.8f);
        g.DrawLine(&p, 12.f, 16.f, 12.f, 20.5f);
        g.DrawLine(&p, 7.5f, 20.8f, 16.5f, 20.8f);
        if (id == WebcamOff) g.DrawLine(&p, 3.5f, 3.5f, 20.5f, 20.5f);
        break;
    case Lock: case Unlock: {             // padlock: shackle closed or open
        g.FillRectangle(&b, 5.f, 11.f, 14.f, 10.f);
        if (id == Lock) {
            g.DrawArc(&p, 8.f, 4.f, 8.f, 12.f, 180.f, 180.f);
            g.DrawLine(&p, 8.f, 10.f, 8.f, 11.5f); g.DrawLine(&p, 16.f, 10.f, 16.f, 11.5f);
        } else {
            g.DrawArc(&p, 12.f, 3.f, 8.f, 11.f, 180.f, 180.f);
            g.DrawLine(&p, 20.f, 8.5f, 20.f, 10.f);
        }
        break;
    }
    case Eye: case EyeOff: {
        GraphicsPath e;
        e.AddBezier(2.5f, 12.f, 7.f, 4.5f, 17.f, 4.5f, 21.5f, 12.f);
        e.AddBezier(21.5f, 12.f, 17.f, 19.5f, 7.f, 19.5f, 2.5f, 12.f);
        g.DrawPath(&p, &e);
        g.FillEllipse(&b, 9.f, 9.f, 6.f, 6.f);
        if (id == EyeOff) g.DrawLine(&p, 4.f, 20.f, 20.f, 4.f);
        break;
    }
    case Pin: {                           // push pin
        PointF head[] = { { 14, 2.5f }, { 21.5f, 10 }, { 18.5f, 11 }, { 13.5f, 16 }, { 13, 19 }, { 5, 11 }, { 8, 10.5f }, { 13, 5.5f } };
        g.FillPolygon(&b, head, 8);
        g.DrawLine(&p, 8.5f, 15.5f, 3.f, 21.f);
        break;
    }
    case AlignLeft: case AlignCenter: case AlignRight: {
        float x = id == AlignLeft ? 4.f : id == AlignCenter ? 12.f : 20.f;
        g.DrawLine(&p, x, 3.f, x, 21.f);
        float w1 = 13.f, w2 = 8.f;
        float a1 = id == AlignLeft ? x + 2 : id == AlignCenter ? x - w1 / 2 : x - 2 - w1, a2 = id == AlignLeft ? x + 2 : id == AlignCenter ? x - w2 / 2 : x - 2 - w2;
        g.FillRectangle(&b, a1, 6.f, w1, 4.5f);
        g.FillRectangle(&b, a2, 13.5f, w2, 4.5f);
        break;
    }
    case AlignTop: case AlignMiddle: case AlignBottom: {
        float y = id == AlignTop ? 4.f : id == AlignMiddle ? 12.f : 20.f;
        g.DrawLine(&p, 3.f, y, 21.f, y);
        float h1 = 13.f, h2 = 8.f;
        float a1 = id == AlignTop ? y + 2 : id == AlignMiddle ? y - h1 / 2 : y - 2 - h1, a2 = id == AlignTop ? y + 2 : id == AlignMiddle ? y - h2 / 2 : y - 2 - h2;
        g.FillRectangle(&b, 6.f, a1, 4.5f, h1);
        g.FillRectangle(&b, 13.5f, a2, 4.5f, h2);
        break;
    }
    case DistributeH:                     // three blocks with equal gaps between two vertical lines
        g.DrawLine(&thin, 2.5f, 3.f, 2.5f, 21.f); g.DrawLine(&thin, 21.5f, 3.f, 21.5f, 21.f);
        g.FillRectangle(&b, 5.f, 8.f, 3.5f, 8.f); g.FillRectangle(&b, 10.25f, 5.f, 3.5f, 14.f); g.FillRectangle(&b, 15.5f, 9.f, 3.5f, 6.f);
        break;
    case DistributeV:
        g.DrawLine(&thin, 3.f, 2.5f, 21.f, 2.5f); g.DrawLine(&thin, 3.f, 21.5f, 21.f, 21.5f);
        g.FillRectangle(&b, 8.f, 5.f, 8.f, 3.5f); g.FillRectangle(&b, 5.f, 10.25f, 14.f, 3.5f); g.FillRectangle(&b, 9.f, 15.5f, 6.f, 3.5f);
        break;
    case Ruler: {
        GraphicsPath rp; rp.AddRectangle(RectF(2.5f, 8.f, 19.f, 8.f));
        g.DrawPath(&p, &rp);
        for (int i = 0; i < 5; i++) { float x = 5.5f + i * 3.3f; g.DrawLine(&thin, x, 8.f, x, i % 2 ? 11.f : 12.5f); }
        break;
    }
    case Link:                            // two chain links
        g.DrawArc(&p, 3.f, 9.f, 10.f, 6.f, 90.f, 180.f); g.DrawLine(&p, 8.f, 9.f, 11.f, 9.f); g.DrawLine(&p, 8.f, 15.f, 11.f, 15.f);
        g.DrawArc(&p, 11.f, 9.f, 10.f, 6.f, -90.f, 180.f); g.DrawLine(&p, 13.f, 9.f, 16.f, 9.f); g.DrawLine(&p, 13.f, 15.f, 16.f, 15.f);
        g.DrawLine(&p, 8.5f, 12.f, 15.5f, 12.f);
        break;
    case Sliders:                         // volume: three sliders with knobs
        for (int i = 0; i < 3; i++) {
            float y = 6.f + i * 6.f, k = i == 0 ? 15.f : i == 1 ? 8.f : 13.f;
            g.DrawLine(&p, 3.f, y, 21.f, y);
            g.FillEllipse(&b, k - 2.6f, y - 2.6f, 5.2f, 5.2f);
        }
        break;
    case Group:                           // two shapes inside a dashed frame
        { Gdiplus::Pen d(c, 1.2f); d.SetDashStyle(DashStyleDash); g.DrawRectangle(&d, 2.5f, 2.5f, 19.f, 19.f); }
        g.FillRectangle(&b, 6.f, 6.f, 7.f, 7.f);
        g.DrawEllipse(&p, 11.f, 11.f, 7.f, 7.f);
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
