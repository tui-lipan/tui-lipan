//! Verifies `Tabs::caps`: the active tab's two padding cells become the `(left, right)` cap
//! glyphs, painted in the tab's own background over the strip background, without shifting tab
//! widths or hit regions. Inactive tabs and the un-capped default keep flat space padding.

use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

const PANEL: Color = Color::Rgb(40, 44, 60);
const BACKDROP: Color = Color::Rgb(12, 12, 18);
const ACCENT: Color = Color::Rgb(120, 200, 255);
// Round (pill) caps.
const LEFT_CAP: &str = "\u{e0b6}";
const RIGHT_CAP: &str = "\u{e0b4}";

struct TabsApp {
    caps: bool,
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
        // active middle tab sits on cols 4..=6.
        let caps = if self.caps {
            Some(('\u{e0b6}', '\u{e0b4}'))
        } else {
            None
        };
        Tabs::new()
            .tabs(vec![Tab::new("A"), Tab::new("B"), Tab::new("C")])
            .active(1)
            .focusable(false)
            .divider(' ')
            .caps(caps)
            .style(Style::new().fg(Color::White).bg(PANEL))
            .active_style(Style::new().fg(BACKDROP).bg(ACCENT).bold())
            .into()
    }
}

fn render(caps: bool) -> CapturedFrame {
    let mut backend = TestBackend::new(TabsApp { caps });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    });
    backend.render();
    backend.capture_frame()
}

#[test]
fn active_tab_padding_stays_flush_without_caps() {
    let frame = render(false);
    eprintln!("{}", frame.to_fixed_grid_lines()[0]);
    // Active tab " B " keeps its space padding; label unshifted at col 5.
    assert_eq!(frame.cell(4, 0).symbol, " ", "leading pad");
    assert_eq!(frame.cell(5, 0).symbol, "B", "label at col 5");
    assert_eq!(frame.cell(6, 0).symbol, " ", "trailing pad");
}

#[test]
fn active_tab_gets_caps_over_the_strip_without_shifting() {
    let frame = render(true);
    eprintln!("{}", frame.to_fixed_grid_lines()[0]);

    let left = frame.cell(4, 0);
    assert_eq!(left.symbol, LEFT_CAP, "left cap replaces leading pad");
    assert_eq!(left.fg, ACCENT, "left cap fills with the active tab color");
    assert_eq!(left.bg, PANEL, "left cap rounds off over the strip");

    // The label keeps its column and active colors; caps did not push it around.
    let body = frame.cell(5, 0);
    assert_eq!(body.symbol, "B", "label unshifted under the caps");
    assert_eq!(body.bg, ACCENT, "label keeps the active background");

    let right = frame.cell(6, 0);
    assert_eq!(right.symbol, RIGHT_CAP, "right cap replaces trailing pad");
    assert_eq!(
        right.fg, ACCENT,
        "right cap fills with the active tab color"
    );
    assert_eq!(right.bg, PANEL, "right cap rounds off over the strip");
}

#[test]
fn inactive_tabs_keep_flat_padding_when_capped() {
    let frame = render(true);
    // First tab " A " is inactive: no caps, plain space padding around the label.
    assert_eq!(frame.cell(0, 0).symbol, " ", "inactive leading pad");
    assert_eq!(frame.cell(1, 0).symbol, "A", "inactive label");
    assert_eq!(frame.cell(2, 0).symbol, " ", "inactive trailing pad");
}
