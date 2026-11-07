//! The selection over a made-up frozen screen: what it looks like (tests/snapshots/select_region.png) and what the
//! mouse and the keys select, in screen coordinates (the virtual screen here starts left of the primary monitor).

use std::sync::Arc;

use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use snapcore::image::{Image, Rect, argb};
use snapnab_app::select::{Mode, Outcome, Overlay, Selected, Setup, WinInfo};

const VS: Rect = Rect::new(-100, 0, 540, 400);
const WINDOW: Rect = Rect::new(20, 60, 300, 260); // screen

fn frozen() -> Image {
    let mut img = Image::sized(VS.width(), VS.height(), 0);
    for y in 0..img.hi() {
        for x in 0..img.wi() {
            *img.at_mut(x, y) = argb(255, 40 + x * 120 / img.wi(), 90, 140 + y * 100 / img.hi());
        }
    }
    let w = WINDOW.offset(-VS.left, -VS.top);
    img.fill_rect(w, 0xFFF0F0F0);
    img.fill_rect(Rect::new(w.left, w.top, w.right, w.top + 24), 0xFF2B579A);
    img.fill_rect(Rect::new(w.left + 20, w.top + 50, w.left + 120, w.top + 70), 0xFFE04040);
    img
}

struct State {
    o: Overlay,
    out: Option<Outcome>,
}

fn harness(mode: Mode, ppp: f32, cursor: (i32, i32), history: Vec<Rect>) -> Harness<'static, State> {
    let setup = Setup {
        frozen: Arc::new(frozen()),
        vs: VS,
        mode,
        fixed: (200, 120),
        video: false,
        quick_crop: true,
        windows: vec![WinInfo { hwnd: 7, rect: WINDOW }],
        monitors: vec![Rect::new(-100, 0, 0, 400), Rect::new(0, 0, 540, 400)],
        history,
        cursor,
        child_at: None,
    };
    let state = State { o: Overlay::new(setup), out: None };
    Harness::builder()
        .with_size(vec2(VS.width() as f32 / ppp, VS.height() as f32 / ppp))
        .with_pixels_per_point(ppp)
        .build_ui_state(
            |ui, s: &mut State| {
                snapui::install_fonts(ui.ctx());
                if let Some(o) = s.o.ui(ui) {
                    s.out = Some(o);
                }
            },
            state,
        )
}

fn press(h: &Harness<State>, p: Pos2, down: bool, button: PointerButton, modifiers: Modifiers) {
    h.event(Event::PointerButton { pos: p, button, pressed: down, modifiers });
}

#[test]
fn looks_like_the_snapshot() {
    // the cursor over the window: the window is lit, the rest dimmed, with the magnifier and the hint
    let mut h = harness(Mode::Region, 1.0, (150, 200), vec![]);
    h.run();
    h.snapshot("select_region");
}

#[test]
fn a_click_takes_the_window_and_a_drag_the_region() {
    let mut h = harness(Mode::Region, 1.0, (150, 200), vec![]);
    h.run();
    let at = pos2(250.0, 200.0); // (150, 200) on the screen
    h.event(Event::PointerMoved(at));
    press(&h, at, true, PointerButton::Primary, Modifiers::NONE);
    press(&h, at, false, PointerButton::Primary, Modifiers::NONE);
    h.run();
    assert_eq!(h.state().out, Some(Outcome::Selected(Selected { rect: WINDOW, window: 7, ..Default::default() })));

    // at 150 %, the points of egui become pixels of the screen: (50, 60) → (75, 90), (250, 160) → (375, 240)
    let mut h = harness(Mode::Region, 1.5, (0, 0), vec![]);
    h.run();
    let (a, b) = (pos2(50.0, 60.0), pos2(250.0, 160.0));
    h.event(Event::PointerMoved(a));
    press(&h, a, true, PointerButton::Primary, Modifiers::NONE);
    h.event(Event::PointerMoved(b));
    press(&h, b, false, PointerButton::Primary, Modifiers::NONE);
    h.run();
    let Some(Outcome::Selected(s)) = &h.state().out else { panic!("{:?}", h.state().out) };
    assert_eq!(s.rect, Rect::new(75 - 100, 90, 376 - 100, 241));
}

#[test]
fn ctrl_keeps_the_region_to_adjust_and_enter_takes_it() {
    let mut h = harness(Mode::Region, 1.0, (0, 0), vec![]);
    h.run();
    let (a, b) = (pos2(400.0, 100.0), pos2(500.0, 180.0));
    h.event(Event::PointerMoved(a));
    press(&h, a, true, PointerButton::Primary, Modifiers::NONE);
    h.event(Event::PointerMoved(b));
    press(&h, b, false, PointerButton::Primary, Modifiers::CTRL);
    h.run();
    assert_eq!(h.state().out, None, "the region stays to be adjusted");
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowRight); // 10 px to the right
    h.key_press_modifiers(Modifiers::CTRL, Key::ArrowDown); // the bottom edge grows by 1
    h.key_press(Key::Enter);
    h.run();
    let Some(Outcome::Selected(s)) = &h.state().out else { panic!("{:?}", h.state().out) };
    assert_eq!(s.rect, Rect::new(300 + 10, 100, 401 + 10, 182));
}

#[test]
fn keys_cancel_take_the_monitor_and_recall_regions() {
    let mut h = harness(Mode::Region, 1.0, (-50, 10), vec![]);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().out, Some(Outcome::Canceled));

    // Enter: the monitor under the cursor (the left one), no window
    let mut h = harness(Mode::Region, 1.0, (-50, 10), vec![]);
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().out, Some(Outcome::Selected(Selected { rect: Rect::new(-100, 0, 0, 400), ..Default::default() })));

    // Tab: the last regions (only those still on the screen), then Enter takes the one shown
    let old = vec![Rect::new(5000, 0, 5100, 100), Rect::new(10, 20, 110, 90), Rect::new(-90, 5, -10, 50)];
    let mut h = harness(Mode::Region, 1.0, (0, 0), old);
    h.run();
    h.key_press(Key::Tab);
    h.key_press(Key::Tab);
    h.key_press(Key::Enter);
    h.run();
    let Some(Outcome::Selected(s)) = &h.state().out else { panic!("{:?}", h.state().out) };
    assert_eq!(s.rect, Rect::new(-90, 5, -10, 50));
}

#[test]
fn other_modes() {
    // fixed size around the cursor, kept inside the screen
    let mut h = harness(Mode::Fixed, 1.0, (500, 390), vec![]);
    h.run();
    let at = pos2(600.0, 390.0);
    press(&h, at, true, PointerButton::Primary, Modifiers::NONE);
    press(&h, at, false, PointerButton::Primary, Modifiers::NONE);
    h.run();
    let Some(Outcome::Selected(s)) = &h.state().out else { panic!("{:?}", h.state().out) };
    assert_eq!(s.rect, Rect::new(340, 280, 540, 400));

    // several areas: two drags, Backspace drops the last, the right button ends
    let mut h = harness(Mode::MultiArea, 1.0, (0, 0), vec![]);
    h.run();
    for (a, b) in [((10.0, 10.0), (60.0, 40.0)), ((100.0, 100.0), (150.0, 130.0)), ((200.0, 10.0), (260.0, 30.0))] {
        let (a, b) = (pos2(a.0, a.1), pos2(b.0, b.1));
        h.event(Event::PointerMoved(a));
        press(&h, a, true, PointerButton::Primary, Modifiers::NONE);
        h.event(Event::PointerMoved(b));
        press(&h, b, false, PointerButton::Primary, Modifiers::NONE);
        h.run();
    }
    h.key_press(Key::Backspace);
    h.run();
    press(&h, pos2(5.0, 5.0), false, PointerButton::Secondary, Modifiers::NONE);
    h.run();
    let Some(Outcome::Selected(s)) = &h.state().out else { panic!("{:?}", h.state().out) };
    assert_eq!(s.rects, [Rect::new(-90, 10, -39, 41), Rect::new(0, 100, 51, 131)]);
    assert_eq!(s.rect, Rect::new(-90, 10, 51, 131));

    // H asks the app for the recent captures menu, where the cursor is
    let mut h = harness(Mode::Region, 1.0, (30, 40), vec![]);
    h.run();
    h.key_press(Key::H);
    h.run();
    assert_eq!(h.state().out, Some(Outcome::RecentCaptures((30, 40))));
}
