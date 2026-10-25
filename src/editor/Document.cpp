#include "Document.h"
#include <cmath>

namespace snap::ed {

using Gdiplus::PointF;
using Gdiplus::RectF;

RectF DrawObj::Rect() const {
    float x0 = std::min(p1.X, p2.X), y0 = std::min(p1.Y, p2.Y);
    return RectF(x0, y0, std::fabs(p2.X - p1.X), std::fabs(p2.Y - p1.Y));
}

int DrawObj::Kind() const {
    switch (type) {
    case ObjType::Ellipse: return ShapeEllipse;
    case ObjType::RoundRect: return ShapeRoundRect;
    case ObjType::Rect: return ShapeRect;
    default: return std::clamp(style.shapeKind, 0, ShapeCount - 1);
    }
}

// formatos de balão com ponta: fala (cunha), pensamento (bolhas), caixa com seta (linha) e setas (a ponta dá a direção)
bool DrawObj::CalloutHasTail() const {
    switch (style.calloutShape) {
    case 16: case 17: case 18: case 19: case 20: case 21: return false;
    default: return true;
    }
}

std::wstring StepLabel(int v, int type) {
    v = std::max(1, v);
    auto letters = [](int n, wchar_t a) {
        std::wstring s;
        while (n > 0) { n--; s.insert(s.begin(), (wchar_t)(a + n % 26)); n /= 26; }
        return s;
    };
    auto roman = [](int n, bool lower) {
        static const int val[] = { 1000, 900, 500, 400, 100, 90, 50, 40, 10, 9, 5, 4, 1 };
        static const wchar_t* sym[] = { L"M", L"CM", L"D", L"CD", L"C", L"XC", L"L", L"XL", L"X", L"IX", L"V", L"IV", L"I" };
        std::wstring s;
        for (int i = 0; i < 13; i++) while (n >= val[i]) { s += sym[i]; n -= val[i]; }
        if (lower) for (auto& c : s) c = (wchar_t)towlower(c);
        return s;
    };
    switch (type) {
    case StepUpper: return letters(v, L'A');
    case StepLower: return letters(v, L'a');
    case StepRoman: return roman(std::min(v, 3999), false);
    case StepRomanLower: return roman(std::min(v, 3999), true);
    default: return std::to_wstring(v);
    }
}

float CapLength(float w, float size) { return std::max(8.0f, w * (1.0f + std::max(1.0f, size) * 0.6f)); }

static void Grow(RectF& r, PointF p) {
    float x0 = std::min(r.X, p.X), y0 = std::min(r.Y, p.Y), x1 = std::max(r.GetRight(), p.X), y1 = std::max(r.GetBottom(), p.Y);
    r = RectF(x0, y0, x1 - x0, y1 - y0);
}

RectF DrawObj::Bounds() const {
    if (type == ObjType::Group) {
        if (children.empty()) return Rect();
        RectF u = children[0].Bounds();
        for (auto& c : children) RectF::Union(u, u, c.Bounds());
        return u;
    }
    RectF r;
    if (IsPath() && !pts.empty()) {
        float x0 = pts[0].X, y0 = pts[0].Y, x1 = x0, y1 = y0;
        for (auto& p : pts) { x0 = std::min(x0, p.X); y0 = std::min(y0, p.Y); x1 = std::max(x1, p.X); y1 = std::max(y1, p.Y); }
        r = RectF(x0, y0, x1 - x0, y1 - y0);
    } else r = Rect();
    float pad = style.width / 2;
    switch (type) {
    case ObjType::Step: {
        float d = style.fontSize * 1.5f;
        r = RectF(p1.X - d / 2, p1.Y - d / 2, d, d);
        if (style.stepShape == 1) r.Width += d * 0.45f;           // gota: ponta para a direita
        pad = 0;
        break;
    }
    case ObjType::Callout:
        if (CalloutHasTail()) Grow(r, tail);
        pad = std::max(style.width, style.tailWidth) / 2 + CapLength(style.tailWidth, style.endSize);
        break;
    case ObjType::Arrow: case ObjType::Line:
        if (style.bezier) Grow(r, ctrl);
        pad = style.width / 2 + CapLength(style.width, std::max(style.startSize, style.endSize));
        break;
    case ObjType::Magnify:
        if (style.magnifyLink) {
            float k = std::max(1.1f, style.magnifyScale / 100.0f), sw = r.Width / k / 2, sh = r.Height / k / 2;
            Grow(r, PointF(tail.X - sw, tail.Y - sh)); Grow(r, PointF(tail.X + sw, tail.Y + sh));
        }
        break;
    case ObjType::Text: pad = style.width; break;
    case ObjType::Highlighter: if (pts.empty()) pad = 0; break;
    case ObjType::Stamp: case ObjType::Picture: pad = 0; break;
    default: break;
    }
    if (style.shadow && type != ObjType::Highlighter && type != ObjType::Blur && type != ObjType::Pixelate && type != ObjType::Spotlight)
        pad += style.shadowBlur * 2 + style.shadowDist + 2;
    r.Inflate(pad, pad);
    return r;
}

static float DistSeg(PointF p, PointF a, PointF b) {
    float dx = b.X - a.X, dy = b.Y - a.Y;
    float l2 = dx * dx + dy * dy;
    float t = l2 > 0 ? std::clamp(((p.X - a.X) * dx + (p.Y - a.Y) * dy) / l2, 0.0f, 1.0f) : 0;
    float qx = a.X + t * dx - p.X, qy = a.Y + t * dy - p.Y;
    return std::sqrt(qx * qx + qy * qy);
}

static float DistQuad(PointF p, PointF a, PointF c, PointF b) {     // curva quadrática a-c-b, amostrada
    float best = 1e9f;
    PointF prev = a;
    for (int i = 1; i <= 24; i++) {
        float t = i / 24.0f, u = 1 - t;
        PointF q(u * u * a.X + 2 * u * t * c.X + t * t * b.X, u * u * a.Y + 2 * u * t * c.Y + t * t * b.Y);
        best = std::min(best, DistSeg(p, prev, q));
        prev = q;
    }
    return best;
}

bool DrawObj::HitTest(PointF p, float tol) const {
    float t = tol + style.width / 2;
    switch (type) {
    case ObjType::Group:
        for (auto& c : children) if (c.HitTest(p, tol)) return true;
        return false;
    case ObjType::Arrow: case ObjType::Line:
        return (style.bezier ? DistQuad(p, p1, ctrl, p2) : DistSeg(p, p1, p2)) <= t + 2;
    case ObjType::Pen: case ObjType::Highlighter:
        if (type == ObjType::Highlighter && pts.empty()) { RectF r = Rect(); r.Inflate(tol, tol); return r.Contains(p); }
        for (size_t i = 1; i < pts.size(); i++) if (DistSeg(p, pts[i - 1], pts[i]) <= t) return true;
        return pts.size() == 1 && DistSeg(p, pts[0], pts[0]) <= t;
    case ObjType::Rect: case ObjType::RoundRect: case ObjType::Ellipse: case ObjType::Shape: {
        Gdiplus::GraphicsPath gp;
        ShapePath(gp, Kind(), Rect());
        if ((style.fill >> 24) && gp.IsVisible(p)) return true;
        Gdiplus::Pen pen(Gdiplus::Color(255, 0, 0, 0), std::max(2.0f, t * 2));
        return gp.IsOutlineVisible(p, &pen) != FALSE;
    }
    case ObjType::Callout: {
        RectF r = Rect(); r.Inflate(tol, tol);
        if (r.Contains(p)) return true;
        if (!CalloutHasTail()) return false;
        RectF c = Rect();
        return DistSeg(p, tail, PointF(c.X + c.Width / 2, c.Y + c.Height / 2)) <= std::max(t, style.tailWidth / 2 + tol) + 6;
    }
    case ObjType::Magnify: {
        RectF r = Rect(); r.Inflate(tol, tol);
        if (r.Contains(p)) return true;
        return style.magnifyLink && DistSeg(p, tail, tail) <= std::max(12.0f, r.Width / std::max(1.1f, style.magnifyScale / 100.0f) / 2);
    }
    default: { RectF b = Bounds(); b.Inflate(tol, tol); return b.Contains(p); }
    }
}

void DrawObj::Move(float dx, float dy) {
    p1.X += dx; p1.Y += dy; p2.X += dx; p2.Y += dy;
    tail.X += dx; tail.Y += dy;
    ctrl.X += dx; ctrl.Y += dy;
    for (auto& p : pts) { p.X += dx; p.Y += dy; }
    for (auto& c : children) c.Move(dx, dy);
}

void DrawObj::Scale(RectF from, RectF to) {
    auto map = [&](PointF& p) {
        float u = from.Width > 0.01f ? (p.X - from.X) / from.Width : 0;
        float v = from.Height > 0.01f ? (p.Y - from.Y) / from.Height : 0;
        p.X = to.X + u * to.Width; p.Y = to.Y + v * to.Height;
    };
    map(p1); map(p2); map(tail); map(ctrl);
    for (auto& p : pts) map(p);
    for (auto& c : children) c.Scale(from, to);
}

void DrawObj::MapPoints(const std::function<PointF(PointF)>& f) {
    p1 = f(p1); p2 = f(p2); tail = f(tail); ctrl = f(ctrl);
    for (auto& p : pts) p = f(p);
    for (auto& c : children) c.MapPoints(f);
}

void DrawObj::UpdateGroupRect() {
    if (type != ObjType::Group || children.empty()) return;
    RectF u = children[0].Bounds();
    for (auto& c : children) RectF::Union(u, u, c.Bounds());
    p1 = PointF(u.X, u.Y); p2 = PointF(u.GetRight(), u.GetBottom());
}

// ---------------------------------------------------------------- histórico
void Document::Checkpoint() {
    undo_.push_back(st);
    if (undo_.size() > 80) undo_.erase(undo_.begin());
    redo_.clear();
    modified = true;
}

void Document::Undo() {
    if (undo_.empty()) return;
    redo_.push_back(st);
    st = undo_.back(); undo_.pop_back();
    modified = true;
}

void Document::Redo() {
    if (redo_.empty()) return;
    undo_.push_back(st);
    st = redo_.back(); redo_.pop_back();
    modified = true;
}

void Document::UndoAll() {
    if (undo_.empty()) return;
    redo_.push_back(st);
    for (size_t i = undo_.size() - 1; i > 0; i--) redo_.push_back(undo_[i]);
    st = undo_.front();
    undo_.clear();
    modified = true;
}

DrawObj* Document::Mutable(size_t idx) {
    if (idx >= st.objs.size()) return nullptr;
    st.objs[idx] = std::make_shared<DrawObj>(*st.objs[idx]);
    return st.objs[idx].get();
}

Image& Document::MutableBase() {
    auto copy = std::make_shared<Image>(st.base ? *st.base : Image());
    st.base = copy;
    return *copy;
}

void Document::SetBase(Image img) { st.base = std::make_shared<Image>(std::move(img)); }

Image Document::Render(const DrawObj* hideText) const {
    if (!st.base) return {};
    Image out = *st.base;
    auto bmp = out.WrapGdip();
    Gdiplus::Graphics g(bmp.get());
    g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
    g.SetTextRenderingHint(Gdiplus::TextRenderingHintAntiAliasGridFit);
    g.SetInterpolationMode(Gdiplus::InterpolationModeHighQualityBicubic);
    g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
    for (auto& o : st.objs) RenderObject(g, out, *o, o.get() == hideText);
    g.Flush(Gdiplus::FlushIntentionSync);
    return out;
}

// ---------------------------------------------------------------- arquivo
bool Document::Save() {
    if (isVideo || path.empty() || !st.base) return false;
    SnapFrame f;
    f.base = *st.base;
    f.doc.setW("CaptureMode", captureMode).setW("WindowTitle", windowTitle).setW("Application", application);
    f.doc.setW("Created", created).seti("NextStep", st.nextStep).setW("Tags", tags);
    XmlNode& objs = f.doc.add("Objects");
    int counter = 1;
    for (auto& o : st.objs) objs.children.push_back(ObjToXml(*o, f, counter));
    if (!SaveSnapFile(path, { f })) return false;
    modified = false;
    return true;
}

bool Document::SaveAs(const std::wstring& file, int q) {
    std::wstring ext = FileExt(file);
    if (ext == L".snap") {
        std::wstring old = path;
        path = file;
        if (Save()) return true;
        path = old;
        return false;
    }
    Image img = Render();
    if (ext == L".pdf") { std::wstring err; return SavePdf(file, img, err); }
    return SaveImageFile(file, img, q);
}

std::unique_ptr<Document> Document::FromImage(Image img, const std::wstring& title) {
    auto d = std::make_unique<Document>();
    d->SetBase(std::move(img));
    d->title = title;
    d->created = TimeStamp(L"%04d-%02d-%02dT%02d:%02d:%02d");
    return d;
}

std::unique_ptr<Document> Document::Load(const std::wstring& file) {
    std::wstring ext = FileExt(file);
    if (ext == L".mp4") {
        auto d = std::make_unique<Document>();
        d->path = file; d->isVideo = true; d->title = FileNameOf(file);
        return d;
    }
    if (ext == L".snap") {
        std::vector<SnapFrame> frames;
        if (!LoadSnapFile(file, frames)) return nullptr;
        auto d = std::make_unique<Document>();
        SnapFrame& f = frames[0];
        d->path = file;
        d->captureMode = f.doc.getW("CaptureMode");
        d->windowTitle = f.doc.getW("WindowTitle");
        d->application = f.doc.getW("Application");
        d->created = f.doc.getW("Created");
        d->st.nextStep = f.doc.geti("NextStep", 1);
        d->tags = f.doc.getW("Tags");
        d->title = d->windowTitle.empty() ? (d->captureMode.empty() ? FileNameOf(file) : d->captureMode) : d->windowTitle;
        d->SetBase(std::move(f.base));
        if (const XmlNode* objs = f.doc.child("Objects"))
            for (auto& n : objs->children) if (auto o = ObjFromXml(n, f)) d->st.objs.push_back(o);
        return d;
    }
    Image img;
    if (!LoadImageFile(file, img)) return nullptr;
    return FromImage(std::move(img), FileNameOf(file));
}

} // namespace snap::ed
