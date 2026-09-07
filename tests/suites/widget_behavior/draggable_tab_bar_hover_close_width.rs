//! With `close_on_hover_only`, the hidden close control lends its cells to the label.
//!
//! The close control is part of every closeable tab's measured width, so a bar that only paints
//! the symbol under the cursor was otherwise truncating labels to make room for something that is
//! not on screen. While the symbol is hidden the label spends those cells instead; the tab's total
//! width is unchanged, so neighbouring tabs and the close hit zone never move.

use tui_lipan::core::event::{KeyMods, MouseEvent, MouseKind};
use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

struct HoverCloseBar;

impl Component for HoverCloseBar {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        DraggableTabBar::new()
            .tabs(vec![DraggableTab::new("ABCDEFGHIJKL").closeable(true)])
            .active(0)
            .variant(DraggableTabBarVariant::FrameLine)
            .close_on_hover_only(true)
            .show_overflow_controls(false)
            .focusable(false)
            .overflow(DraggableTabBarOverflow::ShrinkThenScroll { min_tab_width: 6 })
            .height(Length::Px(1))
            .into()
    }
}

fn row(backend: &mut TestBackend<HoverCloseBar>) -> String {
    backend.render();
    let frame: CapturedFrame = backend.capture_frame();
    frame.to_fixed_grid_lines()[0].clone()
}

fn backend() -> TestBackend<HoverCloseBar> {
    let mut backend = TestBackend::new(HoverCloseBar);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 12,
        h: 1,
    });
    backend
}

#[test]
fn a_hidden_close_control_gives_its_cells_to_the_label() {
    let mut idle = backend();
    let resting = row(&mut idle);

    let mut hovered = backend();
    row(&mut hovered);
    assert!(
        hovered
            .send_mouse(MouseEvent {
                x: 2,
                y: 0,
                kind: MouseKind::Moved,
                mods: KeyMods::NONE,
            })
            .unwrap(),
        "a hover-only close has to be hover-tracked for the symbol to ever appear"
    );
    let under_cursor = row(&mut hovered);

    // Resting: the close cells are the label's, so two more characters of the title survive.
    assert_eq!(resting, "\u{258e} ABCDEFGH\u{2026} ");
    // Under the cursor: the symbol takes its cells back and the label yields exactly those two.
    assert_eq!(under_cursor, "\u{258e} ABCDEF\u{2026} \u{ea76} ");
    assert_eq!(
        resting.chars().count(),
        under_cursor.chars().count(),
        "the tab must keep its width whether or not the close symbol shows"
    );
}
