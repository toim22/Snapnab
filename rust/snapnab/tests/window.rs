//! The capture window drawn on the GPU against tests/snapshots/capture_window*.png, and its clicks through the
//! accessibility tree: each control is found by its UI Automation id (the C++ control's name), as the end-to-end tests
//! find it in the running app.
//! After an intended change: UPDATE_SNAPSHOTS=1 cargo test -p snapnab, then look at the new image before committing.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use snapcore::presets::{CaptureKind, PresetStore, Selection, default_presets};
use snapnab_app::id;
use snapnab_app::window::{Action, Share, View, draw, share_of};

struct State {
    store: PresetStore,
    view: View,
    actions: Vec<Action>,
}

fn harness_with(view: View) -> Harness<'static, State> {
    let state = State { store: PresetStore { presets: default_presets(), selected: 0 }, view, actions: Vec::new() };
    let h = snapnab_app::window::height(&state.store, &state.view);
    Harness::builder().with_size(egui::vec2(snapnab_app::window::WIDTH, h)).build_ui_state(
        |ui, s: &mut State| {
            snapui::install_fonts(ui.ctx());
            let a = draw(&mut s.store, &mut s.view, &|m, v| format!("{m}+{v}"), ui);
            s.actions.extend(a);
        },
        state,
    )
}

fn harness() -> Harness<'static, State> {
    harness_with(View { image_hotkey: "Print Screen".into(), video_hotkey: "Shift+Print Screen".into(), ..Default::default() })
}

/// Clicks the control with this UI Automation id.
fn click(h: &mut Harness<'static, State>, aid: &str) {
    h.get_by(|n| n.author_id() == Some(aid)).click();
    h.run();
}

fn count(h: &Harness<'static, State>, prefix: &str) -> usize {
    h.query_all_by(|n| n.author_id().is_some_and(|a| a.starts_with(prefix) && a[prefix.len()..].parse::<u32>().is_ok())).count()
}

#[test]
fn looks_like_the_snapshot() {
    let mut h = harness();
    h.run();
    h.snapshot("capture_window");
}

#[test]
fn the_preset_list_looks_like_the_snapshot() {
    let mut h = harness_with(View { presets_open: true, ..Default::default() });
    h.run();
    h.snapshot("capture_window_presets");
}

#[test]
fn the_controls_carry_the_cpp_ids() {
    let mut h = harness();
    h.run();
    for aid in ["IDC_TAB_IMAGE", "IDC_TAB_VIDEO", "IDC_SELECTION", "IDC_EFFECTS", "IDC_SHARE", "IDC_PREVIEW", "IDC_CLIPBOARD", "IDC_CURSOR",
                "IDC_DELAY", "IDC_CAPTURE", "IDC_PRESETS_LABEL", "IDC_PRESET_ADD", "IDC_OPEN_EDITOR"] {
        assert!(h.query_by(|n| n.author_id() == Some(aid)).is_some(), "{aid}");
    }
    click(&mut h, "IDC_SELECTION");
    assert_eq!(count(&h, "IDC_SELECTION."), 13, "image selections (SelectionsFor)");
}

#[test]
fn the_video_tab_shows_the_video_controls() {
    let mut h = harness();
    h.run();
    let start = h.state().store.presets[0].kind;
    assert_eq!(start, CaptureKind::Image);
    click(&mut h, "IDC_TAB_VIDEO");
    let p = &h.state().store.presets[0];
    assert_eq!(p.kind, CaptureKind::Video);
    assert!(Selection::for_kind(CaptureKind::Video).contains(&p.selection));
    for aid in ["IDC_QUALITY", "IDC_WEBCAM", "IDC_MIC", "IDC_SYS"] {
        assert!(h.query_by(|n| n.author_id() == Some(aid)).is_some(), "{aid}");
    }
    assert!(h.query_by(|n| n.author_id() == Some("IDC_SHARE")).is_none());
    click(&mut h, "IDC_SELECTION");
    assert_eq!(count(&h, "IDC_SELECTION."), 4, "video selections: region, window, full screen, monitor");
    assert!(h.state().actions.contains(&Action::Changed));
}

#[test]
fn the_selection_and_share_boxes_change_the_preset() {
    let mut h = harness();
    h.run();
    click(&mut h, "IDC_SELECTION");
    click(&mut h, "IDC_SELECTION.2");
    assert_eq!(h.state().store.presets[0].selection, Selection::FullScreen);
    click(&mut h, "IDC_SHARE");
    click(&mut h, "IDC_SHARE.1");
    let p = &h.state().store.presets[0];
    assert!(p.save_to_file && share_of(p) == Share::File);
    assert!(h.state().actions.contains(&Action::ShareChosen(Share::File)));
    // the file output has settings behind its gear
    click(&mut h, "IDC_SHARE.gear");
    assert_eq!(h.state().actions.last(), Some(&Action::ShareGear));
}

#[test]
fn the_switches_flip_and_the_capture_button_captures() {
    let mut h = harness();
    h.run();
    let before = h.state().store.presets[0].copy_to_clipboard;
    click(&mut h, "IDC_CLIPBOARD");
    assert_eq!(h.state().store.presets[0].copy_to_clipboard, !before);
    click(&mut h, "IDC_CAPTURE");
    let a = &h.state().actions;
    assert!(a.contains(&Action::Changed) && a.last() == Some(&Action::Capture), "{a:?}");
}

#[test]
fn the_delay_menu_sets_the_delay() {
    let mut h = harness();
    h.run();
    click(&mut h, "IDC_DELAY");
    click(&mut h, "IDC_DELAY.5");
    assert_eq!(h.state().store.presets[0].delay_seconds, 5);
}

#[test]
fn the_effects_list_opens_an_effect() {
    let mut h = harness();
    h.run();
    click(&mut h, "IDC_EFFECTS");
    click(&mut h, "IDC_EFFECTS.0");
    assert_eq!(h.state().actions.last(), Some(&Action::EditFx(0)));
}

#[test]
fn the_preset_list_selects_and_asks_for_a_hotkey() {
    let mut h = harness_with(View { presets_open: true, ..Default::default() });
    h.run();
    let video = h.state().store.presets.iter().position(|p| p.kind == CaptureKind::Video).unwrap();
    click(&mut h, &format!("IDC_PRESETS.{video}"));
    assert_eq!(h.state().store.selected, video as i32);
    click(&mut h, "IDC_PRESETS.0.hotkey");
    assert_eq!(h.state().store.selected, 0);
    assert_eq!(h.state().actions.last(), Some(&Action::PresetHotkey(0)));
    click(&mut h, "IDC_PRESET_ADD");
    h.get_by_label(&snapcore::i18n::tr_str("Novo preset…")).click();
    h.run();
    assert_eq!(h.state().actions.last(), Some(&Action::Command(id::PRESET_NEW)));
}
