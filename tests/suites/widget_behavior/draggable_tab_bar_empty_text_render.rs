//! Verifies `DraggableTabBar::empty_text`: when the bar has no tabs, the
//! placeholder is left-aligned inside the bar padding, truncated with an
//! ellipsis when it does not fit, and left blank when unset.

use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

const PANEL: Color = Color::Rgb(40, 44, 60);
const PLACEHOLDER: Color = Color::Rgb(160, 170, 190);

#[derive(Clone, Copy)]
struct EmptyBar {
    empty_text: Option<&'static str>,
}

impl Component for EmptyBar {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let mut bar = DraggableTabBar::new()
            .focusable(false)
            .show_close_buttons(false)
            .style(Style::new().fg(Color::White).bg(PANEL))
            .empty_text_style(Style::new().fg(PLACEHOLDER));
        if let Some(text) = self.empty_text {
            bar = bar.empty_text(text);
        }
        bar.into()
    }
}

fn render(app: EmptyBar, width: u16) -> CapturedFrame {
    let mut backend = TestBackend::new(app);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: width,
        h: 1,
    });
    backend.render();
    backend.capture_frame()
}

fn row(frame: &CapturedFrame, width: u16) -> String {
    (0..width)
        .map(|x| frame.cell(x, 0).symbol.clone())
        .collect()
}

#[test]
fn empty_bar_stays_blank_without_placeholder() {
    let frame = render(EmptyBar { empty_text: None }, 12);
    assert_eq!(row(&frame, 12), " ".repeat(12));
    assert_eq!(frame.cell(0, 0).bg, PANEL);
}

#[test]
fn empty_bar_renders_left_aligned_placeholder() {
    let frame = render(
        EmptyBar {
            empty_text: Some("No open tabs"),
        },
        20,
    );
    assert_eq!(row(&frame, 12), "No open tabs");
    assert_eq!(frame.cell(0, 0).fg, PLACEHOLDER);
    assert_eq!(frame.cell(0, 0).bg, PANEL);
    assert_eq!(frame.cell(12, 0).symbol, " ");
}

#[test]
fn empty_bar_truncates_placeholder_with_ellipsis() {
    let frame = render(
        EmptyBar {
            empty_text: Some("No open tabs"),
        },
        8,
    );
    assert_eq!(row(&frame, 8), "No open…");
}
