//! The Editor's document (CPaintDocument, comp_5 §3 / ed_comp_2; src/editor/Document.cpp): the base image, the
//! vector objects on it and the undo history. The objects, their painters, hit-testing and the .snap are snapcore's.

use std::path::Path;
use std::sync::Arc;

use snapcore::doc::{DrawObj, ObjType};
use snapcore::image::Image;
use snapcore::serial::DocFile;

/// One state of the document (what an undo step brings back); objects are shared until one is changed.
#[derive(Clone)]
pub struct DocState {
    pub base: Arc<Image>,
    pub objs: Vec<Arc<DrawObj>>,
    pub next_step: i32,
}

impl Default for DocState {
    /// the next Step object starts at 1, as in the C++ Document
    fn default() -> Self {
        DocState { base: Arc::default(), objs: Vec::new(), next_step: 1 }
    }
}

#[derive(Default)]
pub struct Document {
    /// the .snap (library or elsewhere) or the .mp4
    pub path: String,
    pub title: String,
    pub is_video: bool,
    pub modified: bool,
    pub capture_mode: String,
    pub window_title: String,
    pub application: String,
    pub created: String,
    /// comma-separated (Tag image)
    pub tags: String,
    pub guides_x: Vec<f32>,
    pub guides_y: Vec<f32>,
    pub st: DocState,
    undo: Vec<DocState>,
    redo: Vec<DocState>,
}

const MAX_UNDO: usize = 80;

pub fn ext_of(path: &str) -> String {
    Path::new(path).extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default()
}

pub fn file_name_of(path: &str) -> String {
    Path::new(path).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()
}

impl Document {
    pub fn w(&self) -> i32 {
        self.st.base.w as i32
    }

    pub fn h(&self) -> i32 {
        self.st.base.h as i32
    }

    /// Pushes the current state for undo (and marks the document changed).
    pub fn checkpoint(&mut self) {
        self.undo.push(self.st.clone());
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.modified = true;
    }

    /// The operation ended without changes.
    pub fn discard_checkpoint(&mut self) {
        self.undo.pop();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.st, s));
            self.modified = true;
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.st, s));
            self.modified = true;
        }
    }

    pub fn undo_all(&mut self) {
        if self.undo.is_empty() {
            return;
        }
        let first = self.undo[0].clone();
        self.redo.push(std::mem::replace(&mut self.st, first));
        while self.undo.len() > 1 {
            let s = self.undo.pop().unwrap();
            self.redo.push(s);
        }
        self.undo.clear();
        self.modified = true;
    }

    /// The object, cloned if it is shared with an undo step (copy on write).
    pub fn obj_mut(&mut self, i: usize) -> Option<&mut DrawObj> {
        self.st.objs.get_mut(i).map(Arc::make_mut)
    }

    pub fn base_mut(&mut self) -> &mut Image {
        Arc::make_mut(&mut self.st.base)
    }

    /// A new base image; crop, resize and the effects keep the photo's metadata.
    pub fn set_base(&mut self, mut img: Image) {
        if img.meta.is_none() {
            img.meta = self.st.base.meta.clone();
        }
        self.st.base = Arc::new(img);
    }

    pub fn objects(&self) -> Vec<DrawObj> {
        self.st.objs.iter().map(|o| (**o).clone()).collect()
    }

    /// The composed image (objects drawn on the base); `hide_text` leaves out the text of the object being edited.
    pub fn render(&self, hide_text: Option<usize>) -> Image {
        if self.st.base.is_empty() {
            return Image::default();
        }
        let objs: Vec<DrawObj> = self.st.objs.iter().map(|o| (**o).clone()).collect();
        snapcore::render::render(&self.st.base, &objs, hide_text)
    }

    fn doc_file(&self) -> DocFile {
        DocFile {
            base: (*self.st.base).clone(),
            objs: self.objects(),
            capture_mode: self.capture_mode.clone(),
            window_title: self.window_title.clone(),
            application: self.application.clone(),
            created: self.created.clone(),
            tags: self.tags.clone(),
            next_step: self.st.next_step,
            guides_x: self.guides_x.clone(),
            guides_y: self.guides_y.clone(),
        }
    }

    /// Writes the .snap to `path`.
    pub fn save(&mut self) -> bool {
        if self.is_video || self.path.is_empty() || self.st.base.is_empty() {
            return false;
        }
        if !self.doc_file().save(Path::new(&self.path), crate::VERSION) {
            return false;
        }
        self.modified = false;
        true
    }

    /// Save as: a .snap becomes the document's file; any other extension is an export of the composed image (a PDF
    /// keeps the captured links clickable).
    pub fn save_as(&mut self, file: &str, jpeg_quality: i32) -> Result<(), String> {
        let ext = ext_of(file);
        if ext == ".snap" {
            let old = std::mem::replace(&mut self.path, file.to_owned());
            if self.save() {
                return Ok(());
            }
            self.path = old;
            return Err(String::new());
        }
        let mut img = self.render(None);
        img.meta = self.st.base.meta.clone();
        if ext == ".pdf" {
            let links: Vec<(snapcore::doc::RectF, String)> = self
                .st
                .objs
                .iter()
                .filter(|o| o.kind == ObjType::Link && !o.hidden)
                .map(|o| (o.rect(), String::from_utf16_lossy(&o.text)))
                .collect();
            return crate::files::save_pdf(file, &img, &links);
        }
        crate::files::save_image(file, &img, jpeg_quality)
    }

    pub fn from_image(img: Image, title: &str) -> Document {
        let mut d = Document { title: title.to_owned(), ..Default::default() };
        d.st.next_step = 1;
        d.st.base = Arc::new(img);
        d.created = String::from_utf16_lossy(&snapcore::library::time_stamp(true));
        d
    }

    /// Opens a .snap, an .mp4 (a video document) or an image file.
    pub fn load(file: &str) -> Option<Document> {
        let ext = ext_of(file);
        if ext == ".mp4" {
            return Some(Document { path: file.to_owned(), is_video: true, title: file_name_of(file), ..Default::default() });
        }
        if ext == ".snap" {
            let f = DocFile::load(Path::new(file))?;
            let mut d = Document {
                path: file.to_owned(),
                capture_mode: f.capture_mode,
                window_title: f.window_title,
                application: f.application,
                created: f.created,
                tags: f.tags,
                guides_x: f.guides_x,
                guides_y: f.guides_y,
                ..Default::default()
            };
            d.title = if !d.window_title.is_empty() {
                d.window_title.clone()
            } else if !d.capture_mode.is_empty() {
                d.capture_mode.clone()
            } else {
                file_name_of(file)
            };
            let name = crate::files::capture_name(file);
            if !name.is_empty() {
                d.title = name;
            }
            d.st = DocState { base: Arc::new(f.base), objs: f.objs.into_iter().map(Arc::new).collect(), next_step: f.next_step };
            return Some(d);
        }
        let img = crate::files::load_image(file)?;
        Some(Document::from_image(img, &file_name_of(file)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_and_redo_bring_back_the_states() {
        let mut d = Document::from_image(Image::new(4, 4, 0xFFFFFFFF), "t");
        d.checkpoint();
        d.st.objs.push(Arc::new(DrawObj::new(ObjType::Arrow)));
        d.checkpoint();
        d.st.objs.push(Arc::new(DrawObj::new(ObjType::Line)));
        assert_eq!(d.st.objs.len(), 2);
        d.undo();
        assert_eq!(d.st.objs.len(), 1);
        d.redo();
        assert_eq!(d.st.objs.len(), 2);
        d.undo_all();
        assert!(d.st.objs.is_empty() && !d.can_undo() && d.can_redo());
        d.redo();
        d.redo();
        assert_eq!(d.st.objs.len(), 2);
    }

    #[test]
    fn a_change_does_not_touch_the_undo_copy() {
        let mut d = Document::from_image(Image::new(4, 4, 0xFFFFFFFF), "t");
        d.st.objs.push(Arc::new(DrawObj::new(ObjType::Arrow)));
        d.checkpoint();
        d.obj_mut(0).unwrap().p2.X = 50.0;
        d.undo();
        assert_eq!(d.st.objs[0].p2.X, 0.0);
    }
}
