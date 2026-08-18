use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

#[derive(Clone, Copy)]
struct RowGapTable;

impl Component for RowGapTable {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Table::new()
            .header(["H0", "H1"])
            .rows([TableRow::new(["A0", "B0"]), TableRow::new(["C0", "D0"])])
            .widths([ColumnWidth::Fixed(2), ColumnWidth::Fixed(2)])
            .column_spacing(1)
            .row_gap(1)
            .width(Length::Px(5))
            .height(Length::Px(5))
            .focusable(false)
            .into()
    }
}

fn render() -> CapturedFrame {
    let mut backend = TestBackend::new(RowGapTable);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 5,
        h: 5,
    });
    backend.render();
    backend.capture_frame()
}

fn assert_blank_row(frame: &CapturedFrame, y: u16) {
    for x in 0..5 {
        assert_eq!(
            frame.cell(x, y).symbol,
            " ",
            "cell ({x}, {y}) was not blank"
        );
    }
}

#[test]
fn row_gap_renders_blank_lines_between_header_and_data_rows() {
    let frame = render();

    assert_eq!(frame.cell(0, 0).symbol, "H");
    assert_blank_row(&frame, 1);
    assert_eq!(frame.cell(0, 2).symbol, "A");
    assert_blank_row(&frame, 3);
    assert_eq!(frame.cell(0, 4).symbol, "C");
}
