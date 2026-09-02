use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

const W: u16 = 30;
const H: u16 = 5;
const SELECTION_BG: Color = Color::Blue;

#[derive(Clone, Copy)]
struct MultiLineList {
    selected: usize,
}

impl Component for MultiLineList {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        List::new()
            .items([
                // Only the second line carries a right-aligned description.
                ListItem::new("Publish")
                    .line(ListItemLine::new("  crates.io").description("uploading")),
                // No line carries one.
                ListItem::new("Plain").line(ListItemLine::new("  no description")),
                ListItem::new("Other"),
            ])
            .selected(self.selected)
            .selection_style(Style::new().bg(SELECTION_BG))
            .symbol_column(false)
            .width(Length::Px(W))
            .height(Length::Px(H))
            .focusable(false)
            .into()
    }
}

/// Columns painted with the selection background on row `y`.
fn highlighted_columns(frame: &CapturedFrame, y: u16) -> usize {
    (0..W)
        .filter(|x| frame.cell(*x, y).bg == SELECTION_BG)
        .count()
}

fn frame(selected: usize) -> CapturedFrame {
    let mut backend = TestBackend::new(MultiLineList { selected });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: W,
        h: H,
    });
    backend.render();
    backend.capture_frame()
}

#[test]
fn every_line_of_a_selected_row_shares_one_highlight_width() {
    let frame = frame(0);

    // The description line has to pad out to the row edge to right-align its text. The
    // label-only line above it must not stop short, or one row draws two different bars.
    assert_eq!(highlighted_columns(&frame, 0), W as usize);
    assert_eq!(highlighted_columns(&frame, 1), W as usize);
}

#[test]
fn a_row_without_right_aligned_content_still_hugs_its_label() {
    // The default (`selection_full_width(false)`) pill must survive: only rows that have
    // to reach the edge do.
    let frame = frame(1);

    assert_eq!(highlighted_columns(&frame, 2), "Plain".len());
    assert_eq!(highlighted_columns(&frame, 3), "  no description".len());
    assert_eq!(highlighted_columns(&frame, 0), 0, "row 0 is not selected");
}
