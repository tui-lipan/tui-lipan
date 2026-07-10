//! Verifies `Tabs::caps`: a highlighted tab's two padding cells become the `(left, right)` cap
//! glyphs, painted in the tab's own background over the strip background, without shifting tab
//! widths or hit regions. Tabs that are inactive, truncated, background-matched to the strip, or
//! capped with a wide glyph all keep flat space padding.

use tui_lipan::core::event::MouseKind;
use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, MouseEvent, TestBackend};

const PANEL: Color = Color::Rgb(40, 44, 60);
const BACKDROP: Color = Color::Rgb(12, 12, 18);
const ACCENT: Color = Color::Rgb(120, 200, 255);
const HOVER: Color = Color::Rgb(70, 80, 100);

// Round (pill) caps, both single-width (Private Use Area).
const LEFT_CAP: char = '\u{e0b6}';
const RIGHT_CAP: char = '\u{e0b4}';
// Fullwidth brackets: two cells each, so they cannot stand in for a padding cell.
const WIDE_LEFT_CAP: char = '【';
const WIDE_RIGHT_CAP: char = '】';

#[derive(Clone, Copy)]
struct TabsApp {
    caps: Option<(char, char)>,
    /// When false, the active tab is styled fg-only and inherits the strip background.
    active_bg: bool,
}

impl TabsApp {
    fn capped(caps: Option<(char, char)>) -> Self {
        Self {
            caps,
            active_bg: true,
        }
    }
}

impl Component for TabsApp {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        // Labels "A"/"B"/"C" render as " A "/" B "/" C " (width 3) with a space divider, so the
        // tabs sit on cols 0..=2, 4..=6 and 8..=10. Tab "B" is active.
        let active_style = if self.active_bg {
            Style::new().fg(BACKDROP).bg(ACCENT).bold()
        } else {
            Style::new().fg(ACCENT).bold()
        };
        Tabs::new()
            .tabs(vec![Tab::new("A"), Tab::new("B"), Tab::new("C")])
            .active(1)
            .focusable(false)
            .divider(' ')
            .caps(self.caps)
            .style(Style::new().fg(Color::White).bg(PANEL))
            // Pin the strip's own hover styling to a no-op so hovering a tab changes only that
            // tab's background, never the strip background the caps blend against.
            .hover_style(Style::new())
            .tab_hover_style(Style::new().fg(BACKDROP).bg(HOVER))
            .active_style(active_style)
            .into()
    }
}

fn backend(app: TabsApp, width: u16) -> TestBackend<TabsApp> {
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

fn render(app: TabsApp) -> CapturedFrame {
    backend(app, 20).capture_frame()
}

/// The top row as symbols, for whole-strip layout comparisons.
fn row(frame: &CapturedFrame, width: u16) -> String {
    (0..width)
        .map(|x| frame.cell(x, 0).symbol.clone())
        .collect()
}

#[test]
fn active_tab_padding_stays_flush_without_caps() {
    let frame = render(TabsApp::capped(None));
    // Active tab " B " keeps its space padding; label unshifted at col 5.
    assert_eq!(frame.cell(4, 0).symbol, " ", "leading pad");
    assert_eq!(frame.cell(5, 0).symbol, "B", "label at col 5");
    assert_eq!(frame.cell(6, 0).symbol, " ", "trailing pad");
}

#[test]
fn active_tab_gets_caps_over_the_strip_without_shifting() {
    let frame = render(TabsApp::capped(Some((LEFT_CAP, RIGHT_CAP))));

    let left = frame.cell(4, 0);
    assert_eq!(left.symbol, LEFT_CAP.to_string(), "left cap replaces pad");
    assert_eq!(left.fg, ACCENT, "left cap fills with the active tab color");
    assert_eq!(left.bg, PANEL, "left cap rounds off over the strip");

    // The label keeps its column and active colors; caps did not push it around.
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
    let frame = render(TabsApp::capped(Some((LEFT_CAP, RIGHT_CAP))));
    // First tab " A " is inactive and unhovered: no caps, plain space padding.
    assert_eq!(frame.cell(0, 0).symbol, " ", "inactive leading pad");
    assert_eq!(frame.cell(1, 0).symbol, "A", "inactive label");
    assert_eq!(frame.cell(2, 0).symbol, " ", "inactive trailing pad");
}

#[test]
fn hovered_inactive_tab_gets_caps() {
    let mut backend = backend(TabsApp::capped(Some((LEFT_CAP, RIGHT_CAP))), 20);
    // Hover tab "C" (cols 8..=10), which is not the active tab.
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

    // The active tab keeps its own caps while another tab is hovered.
    assert_eq!(frame.cell(4, 0).symbol, LEFT_CAP.to_string(), "active cap");
}

#[test]
fn tab_matching_the_strip_background_falls_back_to_flat_padding() {
    // The active tab is styled fg-only, so it inherits the strip background and there is no
    // distinct color to fill a cap glyph with. Painting one would put an unrenderable private-use
    // codepoint in a cell that looks blank.
    let frame = render(TabsApp {
        caps: Some((LEFT_CAP, RIGHT_CAP)),
        active_bg: false,
    });
    assert_eq!(frame.cell(4, 0).symbol, " ", "leading pad stays a space");
    assert_eq!(frame.cell(5, 0).symbol, "B", "label unshifted");
    assert_eq!(frame.cell(6, 0).symbol, " ", "trailing pad stays a space");
}

#[test]
fn truncated_tab_falls_back_to_flat_padding() {
    // Width 6 cuts the active tab " B " short, so its trailing padding cell is gone and a right
    // cap would have nothing to sit in.
    let frame = backend(TabsApp::capped(Some((LEFT_CAP, RIGHT_CAP))), 6).capture_frame();
    for x in 0..6 {
        let symbol = &frame.cell(x, 0).symbol;
        assert_ne!(symbol, &LEFT_CAP.to_string(), "no left cap at col {x}");
        assert_ne!(symbol, &RIGHT_CAP.to_string(), "no right cap at col {x}");
    }
}

#[test]
fn wide_caps_fall_back_to_flat_padding() {
    // A two-cell cap cannot replace a one-cell pad without pushing every later tab off the columns
    // `Tabs::index_at_col` hit-tests against, so the strip must render exactly as if uncapped.
    let uncapped = render(TabsApp::capped(None));
    let wide = render(TabsApp::capped(Some((WIDE_LEFT_CAP, WIDE_RIGHT_CAP))));
    assert_eq!(
        row(&wide, 20),
        row(&uncapped, 20),
        "wide caps must not shift tab columns"
    );
}
