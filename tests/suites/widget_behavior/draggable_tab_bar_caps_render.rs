//! Verifies `DraggableTabBar::caps`: a highlighted tab's two padding cells become the
//! `(left, right)` cap glyphs, painted in the tab's own background over the strip background,
//! without shifting tab widths or hit regions. Tabs that are inactive, clipped by scroll, or
//! background-matched to the strip, or capped with a wide glyph, keep flat space padding.

use tui_lipan::core::event::MouseKind;
use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, MouseEvent, TestBackend};

const PANEL: Color = Color::Rgb(40, 44, 60);
const BACKDROP: Color = Color::Rgb(12, 12, 18);
const ACCENT: Color = Color::Rgb(120, 200, 255);
const HOVER: Color = Color::Rgb(70, 80, 100);
const MARK: Color = Color::Rgb(150, 60, 65);

const LEFT_CAP: char = '\u{e0b6}';
const RIGHT_CAP: char = '\u{e0b4}';
const WIDE_LEFT_CAP: char = '【';
const WIDE_RIGHT_CAP: char = '】';

#[derive(Clone, Copy)]
struct BarApp {
    caps: Option<(char, char)>,
    active_bg: bool,
    mark_first: bool,
    variant: DraggableTabBarVariant,
    close_buttons: bool,
}

impl BarApp {
    fn capped(caps: Option<(char, char)>) -> Self {
        Self {
            caps,
            active_bg: true,
            mark_first: false,
            variant: DraggableTabBarVariant::Bordered,
            close_buttons: false,
        }
    }

    fn marked(caps: Option<(char, char)>) -> Self {
        Self {
            caps,
            active_bg: true,
            mark_first: true,
            variant: DraggableTabBarVariant::Bordered,
            close_buttons: false,
        }
    }
}

impl Component for BarApp {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let active_style = if self.active_bg {
            Style::new().fg(BACKDROP).bg(ACCENT).bold()
        } else {
            Style::new().fg(ACCENT).bold()
        };
        DraggableTabBar::new()
            .tabs(vec![
                DraggableTab::new("A")
                    .style(if self.mark_first {
                        Style::new().fg(BACKDROP).bg(MARK)
                    } else {
                        Style::default()
                    })
                    .capped(self.mark_first)
                    .closeable(self.close_buttons),
                DraggableTab::new("B").closeable(self.close_buttons),
                DraggableTab::new("C").closeable(self.close_buttons),
            ])
            .active(1)
            .focusable(false)
            .draggable(false)
            .divider(' ')
            .caps(self.caps)
            .variant(self.variant)
            .show_close_buttons(self.close_buttons)
            .close_symbol("x")
            .show_overflow_controls(false)
            .show_file_icons(false)
            .style(Style::new().fg(Color::White).bg(PANEL))
            .hover_style(Style::new())
            .tab_hover_style(Style::new().fg(BACKDROP).bg(HOVER))
            .active_style(active_style)
            .into()
    }
}

fn backend(app: BarApp, width: u16) -> TestBackend<BarApp> {
    let mut backend = TestBackend::new(app);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: width,
        h: 1,
    });
    backend.render();
    backend
}

fn render(app: BarApp) -> CapturedFrame {
    backend(app, 20).capture_frame()
}

fn row(frame: &CapturedFrame, width: u16) -> String {
    (0..width)
        .map(|x| frame.cell(x, 0).symbol.clone())
        .collect()
}

#[test]
fn active_tab_padding_stays_flush_without_caps() {
    let frame = render(BarApp::capped(None));
    assert_eq!(frame.cell(4, 0).symbol, " ", "leading pad");
    assert_eq!(frame.cell(5, 0).symbol, "B", "label at col 5");
    assert_eq!(frame.cell(6, 0).symbol, " ", "trailing pad");
}

#[test]
fn active_tab_gets_caps_over_the_strip_without_shifting() {
    let frame = render(BarApp::capped(Some((LEFT_CAP, RIGHT_CAP))));

    let left = frame.cell(4, 0);
    assert_eq!(left.symbol, LEFT_CAP.to_string(), "left cap replaces pad");
    assert_eq!(left.fg, ACCENT, "left cap fills with the active tab color");
    assert_eq!(left.bg, PANEL, "left cap rounds off over the strip");

    let body = frame.cell(5, 0);
    assert_eq!(body.symbol, "B", "label unshifted under the caps");
    assert_eq!(body.bg, ACCENT, "label keeps the active background");

    let right = frame.cell(6, 0);
    assert_eq!(
        right.symbol,
        RIGHT_CAP.to_string(),
        "right cap replaces pad"
    );
    assert_eq!(
        right.fg, ACCENT,
        "right cap fills with the active tab color"
    );
    assert_eq!(right.bg, PANEL, "right cap rounds off over the strip");
}

#[test]
fn inactive_tabs_keep_flat_padding_when_capped() {
    let frame = render(BarApp::capped(Some((LEFT_CAP, RIGHT_CAP))));
    assert_eq!(frame.cell(0, 0).symbol, " ", "inactive leading pad");
    assert_eq!(frame.cell(1, 0).symbol, "A", "inactive label");
    assert_eq!(frame.cell(2, 0).symbol, " ", "inactive trailing pad");
}

#[test]
fn hovered_inactive_tab_gets_caps() {
    let mut backend = backend(BarApp::capped(Some((LEFT_CAP, RIGHT_CAP))), 20);
    backend
        .send_mouse(MouseEvent {
            x: 9,
            y: 0,
            kind: MouseKind::Moved,
            mods: Default::default(),
        })
        .expect("hover tab C");
    let frame = backend.capture_frame();

    let left = frame.cell(8, 0);
    assert_eq!(left.symbol, LEFT_CAP.to_string(), "hovered tab gets caps");
    assert_eq!(left.fg, HOVER, "cap fills with the hover background");
    assert_eq!(left.bg, PANEL, "cap rounds off over the strip");

    assert_eq!(frame.cell(9, 0).symbol, "C", "hovered label unshifted");
    assert_eq!(frame.cell(10, 0).symbol, RIGHT_CAP.to_string(), "right cap");
    assert_eq!(frame.cell(4, 0).symbol, LEFT_CAP.to_string(), "active cap");
}

#[test]
fn tab_matching_the_strip_background_falls_back_to_flat_padding() {
    let frame = render(BarApp {
        caps: Some((LEFT_CAP, RIGHT_CAP)),
        active_bg: false,
        mark_first: false,
        variant: DraggableTabBarVariant::Bordered,
        close_buttons: false,
    });
    assert_eq!(frame.cell(4, 0).symbol, " ", "leading pad stays a space");
    assert_eq!(frame.cell(5, 0).symbol, "B", "label unshifted");
    assert_eq!(frame.cell(6, 0).symbol, " ", "trailing pad stays a space");
}

#[test]
fn clipped_tab_falls_back_to_flat_padding() {
    let frame = backend(BarApp::capped(Some((LEFT_CAP, RIGHT_CAP))), 6).capture_frame();
    for x in 0..6 {
        let symbol = &frame.cell(x, 0).symbol;
        assert_ne!(symbol, &LEFT_CAP.to_string(), "no left cap at col {x}");
        assert_ne!(symbol, &RIGHT_CAP.to_string(), "no right cap at col {x}");
    }
}

#[test]
fn wide_caps_fall_back_to_flat_padding() {
    let uncapped = render(BarApp::capped(None));
    let wide = render(BarApp::capped(Some((WIDE_LEFT_CAP, WIDE_RIGHT_CAP))));
    assert_eq!(
        row(&wide, 20),
        row(&uncapped, 20),
        "wide caps must not shift tab columns"
    );
}

#[test]
fn a_capped_inactive_tab_gets_caps() {
    let frame = render(BarApp::marked(Some((LEFT_CAP, RIGHT_CAP))));

    let left = frame.cell(0, 0);
    assert_eq!(
        left.symbol,
        LEFT_CAP.to_string(),
        "an opted-in inactive tab gets caps"
    );
    assert_eq!(left.fg, MARK, "left cap fills with the tab's own color");
    assert_eq!(left.bg, PANEL, "left cap rounds off over the strip");
    assert_eq!(
        frame.cell(1, 0).symbol,
        "A",
        "label unshifted under the caps"
    );

    let right = frame.cell(2, 0);
    assert_eq!(right.symbol, RIGHT_CAP.to_string());
    assert_eq!(right.fg, MARK);
    assert_eq!(right.bg, PANEL);
    assert_eq!(frame.cell(4, 0).symbol, LEFT_CAP.to_string());
}

#[test]
fn a_capped_tab_still_degrades_when_the_caps_do_not_fit() {
    let frame = render(BarApp::marked(Some((WIDE_LEFT_CAP, WIDE_RIGHT_CAP))));
    assert_eq!(frame.cell(0, 0).symbol, " ", "leading pad stays a space");
    assert_eq!(frame.cell(1, 0).symbol, "A", "label unshifted");
    assert_eq!(frame.cell(2, 0).symbol, " ", "trailing pad stays a space");
}

#[test]
fn close_button_tabs_keep_caps_on_the_outer_padding() {
    let frame = render(BarApp {
        caps: Some((LEFT_CAP, RIGHT_CAP)),
        active_bg: true,
        mark_first: false,
        variant: DraggableTabBarVariant::Bordered,
        close_buttons: true,
    });
    // Active " B x " starts after " A x " (cols 0..=4) and a space divider (5).
    assert_eq!(frame.cell(6, 0).symbol, LEFT_CAP.to_string());
    assert_eq!(frame.cell(7, 0).symbol, "B");
    assert_eq!(frame.cell(9, 0).symbol, "x");
    assert_eq!(frame.cell(10, 0).symbol, RIGHT_CAP.to_string());
}

#[test]
fn label_ellipsis_still_gets_caps() {
    struct EllipsisBar;

    impl Component for EllipsisBar {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            DraggableTabBar::new()
                .tabs(["AAAAAA", "BBBBBB", "CCCCCC"].map(DraggableTab::new))
                .active(1)
                .focusable(false)
                .draggable(false)
                .divider(' ')
                .caps(Some((LEFT_CAP, RIGHT_CAP)))
                .show_close_buttons(false)
                .show_overflow_controls(false)
                .tab_max_width(Some(2))
                .style(Style::new().fg(Color::White).bg(PANEL))
                .hover_style(Style::new())
                .active_style(Style::new().fg(BACKDROP).bg(ACCENT).bold())
                .into()
        }
    }

    let mut backend = TestBackend::new(EllipsisBar);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    });
    backend.render();
    let frame = backend.capture_frame();
    // Each tab is pad + 2-cell label + pad (width 4), plus a space divider.
    assert_eq!(frame.cell(5, 0).symbol, LEFT_CAP.to_string());
    assert_eq!(frame.cell(8, 0).symbol, RIGHT_CAP.to_string());
}

#[test]
fn frame_line_caps_replace_the_accent_slot() {
    let frame = render(BarApp {
        caps: Some((LEFT_CAP, RIGHT_CAP)),
        active_bg: true,
        mark_first: false,
        variant: DraggableTabBarVariant::FrameLine,
        close_buttons: false,
    });
    // FrameLine tab "B" after tab "A" (width 4): left cap in the accent slot, inner pad, label,
    // right cap. The accent marker is not stacked in front of the pill.
    assert_eq!(frame.cell(4, 0).symbol, LEFT_CAP.to_string());
    assert_eq!(frame.cell(4, 0).fg, ACCENT);
    assert_eq!(frame.cell(4, 0).bg, PANEL);
    assert_eq!(frame.cell(5, 0).symbol, " ");
    assert_eq!(frame.cell(5, 0).bg, ACCENT);
    assert_eq!(frame.cell(6, 0).symbol, "B");
    assert_eq!(frame.cell(7, 0).symbol, RIGHT_CAP.to_string());
    assert_eq!(frame.cell(7, 0).fg, ACCENT);
    assert_eq!(frame.cell(7, 0).bg, PANEL);
}
