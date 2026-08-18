//! A `DraggableTabBar` that starts left of the visible area is *clipped*, not re-anchored.
//!
//! A bar can begin off-screen either because it sits part-way outside its parent - a Canvas child at
//! a negative offset, which is how a side panel slides into view - or because a clip starts inside
//! it. Either way the tabs must keep their places and let the edge cut through them; re-anchoring
//! them at the visible edge makes a panel appear to grow its tabs out of nothing as it arrives, and
//! silently re-lays-out a bar that was only meant to be partly hidden.

use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

const PANEL: Color = Color::Rgb(40, 44, 60);

/// A 12-column bar inside a 12-column window, shifted `offset` columns left of it.
#[derive(Clone, Copy)]
struct SlidingBar {
    offset: i16,
    empty: bool,
}

impl Component for SlidingBar {
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
            .border(false)
            .divider(' ')
            .width(Length::Px(12))
            .height(Length::Px(1))
            .style(Style::new().fg(Color::White).bg(PANEL));
        bar = if self.empty {
            bar.empty_text("ABCDEFGHIJKL")
        } else {
            bar.tabs(vec![DraggableTab::new("ABCDEFGHIJ")]).active(0)
        };
        Canvas::new()
            .child_at(
                Rect {
                    x: 0,
                    y: 0,
                    w: 12,
                    h: 1,
                },
                Canvas::new().child_at(
                    Rect {
                        x: self.offset,
                        y: 0,
                        w: 12,
                        h: 1,
                    },
                    bar,
                ),
            )
            .into()
    }
}

fn render(app: SlidingBar) -> CapturedFrame {
    let mut backend = TestBackend::new(app);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 12,
        h: 1,
    });
    backend.render();
    backend.capture_frame()
}

fn row(frame: &CapturedFrame) -> String {
    frame.to_fixed_grid_lines()[0].clone()
}

#[test]
fn a_bar_pushed_left_of_its_parent_is_cut_off_rather_than_re_anchored() {
    // Fully inside its window: the whole label, from the first column.
    let settled = row(&render(SlidingBar {
        offset: 0,
        empty: false,
    }));
    assert!(
        settled.starts_with(" ABCDEFGHIJ"),
        "the settled bar should show its whole label, got {settled:?}"
    );

    // Pushed four columns out: the label keeps its place, so the leading columns are cut away and
    // what shows is its tail - never the label restarted at the visible edge.
    let clipped = row(&render(SlidingBar {
        offset: -4,
        empty: false,
    }));
    assert!(
        clipped.starts_with("DEFGHIJ"),
        "a bar pushed left should be cut off, got {clipped:?}"
    );
    assert!(
        !clipped.starts_with(" ABC"),
        "the bar was re-anchored at the visible edge instead of clipped: {clipped:?}"
    );
}

#[test]
fn an_empty_bars_placeholder_is_cut_off_the_same_way() {
    let settled = row(&render(SlidingBar {
        offset: 0,
        empty: true,
    }));
    assert!(
        settled.starts_with("ABCDEFGHIJKL"),
        "the settled placeholder should be whole, got {settled:?}"
    );

    let clipped = row(&render(SlidingBar {
        offset: -5,
        empty: true,
    }));
    assert!(
        clipped.starts_with("FGHIJKL"),
        "the placeholder should be cut off, not restarted, got {clipped:?}"
    );
}
