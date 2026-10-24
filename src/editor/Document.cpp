#include "Document.h"
#include <cmath>

namespace snap::ed {

using Gdiplus::PointF;
using Gdiplus::RectF;

RectF DrawObj::Rect() const {
    float x0 = std::min(p1.X, p2.X), y0 = std::min(p1.Y, p2.Y);
    return RectF(x0, y0, std::fabs(p2.X - p1.X), std::fabs(p2.Y - p1.Y));
}

RectF DrawObj::Bounds() const {
    RectF r;
    if (IsPath() && !pts.empty()) {
        float x0 = pts[0].X, y0 = pts[0].Y, x1 = x0, y1 = y0;
        for (auto& p : pts) { x0 = std::min(x0, p.X); y0 = std::min(y0, p.Y); x1 = std::max(x1, p.X); y1 = std::max(y1, p.Y); }
        r = RectF(x0, y0, x1 - x0, y1 - y0);
    } else r = Rect();
    if (type == ObjType::Step) {
        float d = style.fontSize * 1.5f;
        r = RectF(p1.X - d / 2, p1.Y - d / 2, d, d);
    }
    if (type == ObjType::Callout) {
        float x0 = std::min(r.X, tail.X), y0 = std::min(r.Y, tail.Y);
        float x1 = std::max(r.GetRight(), tail.X), y1 = std::max(r.GetBottom(), tail.Y);
        r = RectF(x0, y0, x1 - x0, y1 - y0);
    }
    float pad = style.width / 2 + (IsLine() ? style.width * 2 : 0) + (style.shadow ? 4 : 0);
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

bool DrawObj::HitTest(PointF p, float tol) const {
    float t = tol + style.width / 2;
    switch (type) {
    case ObjType::Arrow: case ObjType::Line: return DistSeg(p, p1, p2) <= t + 2;
    case ObjType::Pen: case ObjType::Highlighter:
        for (size_t i = 1; i < pts.size(); i++) if (DistSeg(p, pts[i - 1], pts[i]) <= t) return true;
        return pts.size() == 1 && DistSeg(p, pts[0], pts[0]) <= t;
    case ObjType::Rect: case ObjType::RoundRect: case ObjType::Ellipse: {
        RectF r = Rect();
        bool filled = (style.fill >> 24) != 0;
        if (type == ObjType::Ellipse) {
            float cx = r.X + r.Width / 2, cy = r.Y + r.Height / 2;
            auto inside = [&](float rx, float ry) {
                if (rx <= 0 || ry <= 0) return false;
                float dx = (p.X - cx) / rx, dy = (p.Y - cy) / ry; return dx * dx + dy * dy <= 1;
            };
            if (filled) return inside(r.Width / 2 + t, r.Height / 2 + t);
            return inside(r.Width / 2 + t, r.Height / 2 + t) && !inside(r.Width / 2 - t, r.Height / 2 - t);
        }
        RectF outer = r; outer.Inflate(t, t);
        if (!outer.Contains(p)) return false;
        if (filled) return true;
        RectF inner = r; inner.Inflate(-t, -t);
        return inner.Width <= 0 || inner.Height <= 0 || !inner.Contains(p);
    }
    case ObjType::Callout: {
        RectF r = Rect(); r.Inflate(tol, tol);
        return r.Contains(p) || DistSeg(p, tail, PointF(Rect().X + Rect().Width / 2, Rect().Y + Rect().Height / 2)) <= t + 6;
    }
    default: { RectF b = Bounds(); b.Inflate(tol, tol); return b.Contains(p); }
    }
}

void DrawObj::Move(float dx, float dy) {
    p1.X += dx; p1.Y += dy; p2.X += dx; p2.Y += dy;
    tail.X += dx; tail.Y += dy;
    for (auto& p : pts) { p.X += dx; p.Y += dy; }
}

void DrawObj::Scale(RectF from, RectF to) {
    auto map = [&](PointF& p) {
        float u = from.Width > 0.01f ? (p.X - from.X) / from.Width : 0;
        float v = from.Height > 0.01f ? (p.Y - from.Y) / from.Height : 0;
        p.X = to.X + u * to.Width; p.Y = to.Y + v * to.Height;
    };
    map(p1); map(p2); map(tail);
    for (auto& p : pts) map(p);
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
    f.doc.setW("Created", created).seti("NextStep", st.nextStep);
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
