//! The parts of Snapnab.exe that do not need its windows running, so the tests reach them: the command line, the
//! capture window's drawing and the selection over the frozen screen.

pub mod cmdline;
pub mod hud;

pub mod select;
pub mod window;

/// The text in the interface language (the program's own strings are in Portuguese).
pub fn tr(s: &str) -> String {
    snapcore::i18n::tr_str(s)
}

/// Command IDs, numbered as in the C++ app (the e2e suite posts them as WM_COMMAND).
#[allow(dead_code)]
pub mod id {
    pub const EXIT: u32 = 1000;
    pub const OPEN_EDITOR: u32 = 1001;
    pub const CAPTURE_IMAGE: u32 = 1002;
    pub const CAPTURE_VIDEO: u32 = 1003;
    pub const CAPTURE_CLIPBOARD: u32 = 1004;
    pub const CAPTURE_FILE: u32 = 1005;
    pub const REPEAT: u32 = 1006;
    pub const OPTIONS: u32 = 1007;
    pub const PRESET_IMPORT: u32 = 1008;
    pub const PRESET_EXPORT: u32 = 1009;
    pub const ABOUT: u32 = 1010;
    pub const LIBRARY_FOLDER: u32 = 1011;
    pub const LOGS: u32 = 1012;
    pub const SHOW: u32 = 1013;
    pub const ONECLICK: u32 = 1014;
    pub const SHOW_PRESETS: u32 = 1015;
    pub const PRESET_FROM_CURRENT: u32 = 1016;
    pub const PRESET_NEW: u32 = 1017;
    pub const PRESET_HELP: u32 = 1018;
    pub const PRESET_RUN: u32 = 1019;
    pub const PRESET_RENAME: u32 = 1020;
    pub const PRESET_DUP: u32 = 1021;
    pub const PRESET_HOTKEY: u32 = 1022;
    pub const PRESET_DEL: u32 = 1023;
    pub const SETTINGS_EXPORT: u32 = 1024;
    pub const SETTINGS_IMPORT: u32 = 1025;
    pub const SETTINGS_BACKUPS: u32 = 1026;
    pub const MANUAL: u32 = 1027;
    pub const COLOR_PICKER: u32 = 1028;
    pub const PIN_CLIPBOARD: u32 = 1029;
    pub const PIN_FILE: u32 = 1030;
    pub const PINS_SHOW: u32 = 1031;
    pub const PINS_HIDE: u32 = 1032;
    pub const PINS_CLOSE: u32 = 1033;
    pub const PINS_REOPEN: u32 = 1034;
    pub const PINS_CLICKTHROUGH: u32 = 1035;
    pub const INTERVAL: u32 = 1036;
    pub const FOCUS: u32 = 1037;
    pub const QR_REGION: u32 = 1038;
    pub const UPLOAD_SETTINGS: u32 = 1039;
    pub const CAPTURE_PIN: u32 = 1040;
    pub const CLEANUP_NOW: u32 = 1041;
    pub const RECORD_AUDIO: u32 = 1042;
    pub const AUDIO_FOLDER: u32 = 1043;
    pub const AFTER_EDITOR: u32 = 1044;
    pub const PIN_HISTORY_BASE: u32 = 1500;
    pub const PRESET_BASE: u32 = 2000;
    // control IDs of the C++ window that are also commands
    pub const IDC_TAB_IMAGE: u32 = 101;
    pub const IDC_TAB_VIDEO: u32 = 102;
    pub const IDC_CAPTURE: u32 = 119;
    pub const IDC_PRESETS_LABEL: u32 = 120;
    pub const IDC_OPEN_EDITOR: u32 = 122;
}
