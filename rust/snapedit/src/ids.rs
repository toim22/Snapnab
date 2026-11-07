//! The Editor's command IDs: the same numbers as the C++ editor (src/editor/main.cpp), which the menus, the toolbar and
//! the e2e suite (WM_COMMAND posted to the "SnapnabEditor" window) use.

// toolbar actions
pub const T_CAPTURE: u32 = 1;
pub const T_RECORD: u32 = 2;
pub const T_COPYALL: u32 = 3;
pub const T_SAVE: u32 = 4;
pub const T_SHARE: u32 = 5;
pub const T_TOOL_BASE: u32 = 100;

macro_rules! seq {
    ($base:expr; $($name:ident),* $(,)?) => { seq!(@ $base; $($name),*); };
    (@ $n:expr; $name:ident $(, $rest:ident)*) => { pub const $name: u32 = $n; seq!(@ $n + 1; $($rest),*); };
    (@ $n:expr;) => {};
}

seq!(1000;
    M_NEW_CAPTURE, M_NEW_VIDEO, M_NEW_CLIPBOARD, M_NEW_IMAGE, M_OPEN, M_SAVE, M_SAVEAS, M_DUPLICATE, M_DELETE, M_PRINT,
    M_EMAIL, M_OPEN_PROGRAM, M_LIBRARY, M_CLOSE, M_EXIT,
    M_UNDO, M_REDO, M_UNDOALL, M_CUT, M_COPY, M_COPYALL, M_PASTE, M_DELETE_SEL, M_SELECTALL, M_FLATTEN, M_FRONT, M_BACK, M_OCR,
    M_CROP_SEL, M_ROT_CW, M_ROT_CCW, M_ROT_180, M_ROT_FREE, M_FLIP_H, M_FLIP_V, M_RESIZE, M_CANVAS, M_CANVAS_COLOR, M_TRIM,
    M_FX_BORDER, M_FX_SHADOW, M_FX_EDGE, M_FX_GRAY, M_FX_INVERT, M_FX_SEPIA, M_FX_BC, M_FX_HS, M_FX_REPLACE, M_FX_WATERMARK, M_FX_CAPTION,
    M_FX_MONO,
    M_ZOOM_IN, M_ZOOM_OUT, M_ZOOM_100, M_ZOOM_FIT, M_BG_COLOR, M_SHOW_TRAY, M_ABOUT, M_SHORTCUTS, M_SHOW_EFFECTS,
    M_FX_PERSPECTIVE, M_FX_PAGECURL, M_FX_FILTER, M_FX_ADJUST, M_FX_SPOTLIGHT, M_SHOW_LIBRARY,
    M_BRING_FORWARD, M_SEND_BACKWARD, M_ALIGN_LEFT, M_ALIGN_CENTER, M_ALIGN_RIGHT, M_ALIGN_TOP, M_ALIGN_MIDDLE, M_ALIGN_BOTTOM,
    M_GROUP, M_UNGROUP, M_FLATTEN_SEL, M_APPLY_TEMPLATE, M_SNAPPING, M_TAG,
    M_DIST_H, M_DIST_V, M_PASTE_INPLACE, M_LOCK, M_UNLOCK, M_LIFT, M_DROP, M_RULERS, M_EDGESNAP, M_PLAINPASTE, M_RENAME_OBJ,
    M_SHOW_LAYERS, M_CLEAR_GUIDES, M_OPEN_LINK, M_MANUAL,
    M_READ_QR, M_BEAUTIFY, M_COMBINE, M_ADD_CAPTURE, M_REDACT, M_COMPARE, M_ASSISTANT, M_AI_SETTINGS, M_DATESTAMP, M_DATESTAMP_FMT,
    M_REMOVE_BG, M_BATCH, M_SPLIT, M_CONTRAST, M_PIN, M_UPLOAD, M_UPLOAD_SETTINGS, M_COPYAS, M_DRAWHOLD,
);

pub const M_TB_CUSTOMIZE: u32 = 1500;
pub const M_TB_RESET: u32 = 1501;
pub const M_MORE_TOOL_BASE: u32 = 1600;
/// Send to Word / PowerPoint / Excel / OneNote (C20)
pub const M_OFFICE_BASE: u32 = 1700;
/// Insert cursor › (C25)
pub const M_CURSOR_BASE: u32 = 1720;

/// The video view's buttons (src/editor/Video.cpp B_* + 2000); posted to the main window as WM_COMMAND (the C++ view
/// was a child window that got 1–11).
pub const B_PLAY: u32 = 2001;
pub const B_PREV: u32 = 2002;
pub const B_NEXT: u32 = 2003;
pub const B_MARKIN: u32 = 2004;
pub const B_MARKOUT: u32 = 2005;
pub const B_CUT: u32 = 2006;
pub const B_UNDO: u32 = 2007;
pub const B_SAVEMP4: u32 = 2008;
pub const B_SAVEFRAME: u32 = 2009;
pub const B_GIF: u32 = 2010;
pub const B_PLAYER: u32 = 2011;

/// The effect commands, in the Image › Effects menu order.
pub const EFFECTS: [u32; 11] = [
    M_FX_BORDER, M_FX_EDGE, M_FX_CAPTION, M_FX_PERSPECTIVE, M_FX_PAGECURL, M_FX_SHADOW, M_FX_FILTER, M_FX_ADJUST, M_FX_REPLACE,
    M_FX_SPOTLIGHT, M_FX_WATERMARK,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_the_cpp_enum() {
        assert_eq!(M_NEW_CAPTURE, 1000);
        assert_eq!(M_SAVE, 1005);
        assert_eq!(M_UNDO, 1015);
        assert_eq!(M_COPYALL, 1020);
        assert_eq!(M_FX_MONO, 1050);
        assert_eq!(M_SHOW_EFFECTS, 1059);
        assert_eq!(M_SHOW_LIBRARY, 1065);
        assert_eq!(M_TAG, 1079);
        assert_eq!(M_OPEN_LINK, 1093);
        assert_eq!(M_MANUAL, 1094);
        assert_eq!(M_DRAWHOLD, 1113);
    }
}
