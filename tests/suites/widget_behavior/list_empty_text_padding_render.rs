//! Empty-state copy is prose, not a row. `empty_text_padding` insets it even when
//! `item_horizontal_padding` left is 0 (the usual setup when a gutter carries the
//! label gap instead).

use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

const W: u16 = 20;
const H: u16 = 3;

#[derive(Clone, Copy)]
struct EmptyList {
    left_pad: u16,
}

impl Component for EmptyList {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        List::new()
            .empty_text("No sessions")
            .item_horizontal_padding((0, 1, 0, 0))
            .empty_text_padding((0, 0, 0, self.left_pad))
            .symbol_column(false)
            .width(Length::Px(W))
            .height(Length::Px(H))
            .focusable(false)
            .into()
    }
}

fn render(left_pad: u16) -> CapturedFrame {
    let mut backend = TestBackend::new(EmptyList { left_pad });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: W,
        h: H,
    });
    backend.render();
    backend.capture_frame()
}

fn row(frame: &CapturedFrame) -> String {
    (0..W).map(|x| frame.cell(x, 0).symbol.clone()).collect()
}

#[test]
fn empty_text_stays_flush_without_padding() {
    let frame = render(0);
    assert_eq!(&row(&frame)[..11], "No sessions");
}

#[test]
fn empty_text_padding_insets_independently_of_row_padding() {
    let frame = render(1);
    assert_eq!(&row(&frame)[..12], " No sessions");
}
