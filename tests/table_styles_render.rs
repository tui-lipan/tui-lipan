use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

const COLUMN_FG: Color = Color::Rgb(10, 20, 30);
const COLUMN_BG: Color = Color::Rgb(40, 50, 60);
const CELL_FG: Color = Color::Rgb(70, 80, 90);
const CELL_BG: Color = Color::Rgb(100, 110, 120);
const ROW_FG: Color = Color::Rgb(130, 140, 150);
const ROW_BG: Color = Color::Rgb(160, 170, 180);

#[derive(Clone, Copy)]
enum TableStyleCase {
    ColumnStyles,
    CellOverridesColumn,
    RowStyles,
    ColumnOverridesIndexedRow,
}

impl Component for TableStyleCase {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        match self {
            Self::ColumnStyles => base_table()
                .column_styles([Style::default(), Style::new().fg(COLUMN_FG).bg(COLUMN_BG)]),
            Self::CellOverridesColumn => Table::new()
                .header(["H0", "H1"])
                .rows([TableRow::new([
                    TableCell::new("A0").style(Style::new().fg(CELL_FG).bg(CELL_BG)),
                    TableCell::new("B0"),
                ])])
                .widths([ColumnWidth::Fixed(4), ColumnWidth::Fixed(4)])
                .column_spacing(1)
                .column_style(0, Style::new().fg(COLUMN_FG).bg(COLUMN_BG))
                .width(Length::Px(9))
                .height(Length::Px(2))
                .focusable(false),
            Self::RowStyles => base_table().row_styles([Style::new().fg(ROW_FG).bg(ROW_BG)]),
            Self::ColumnOverridesIndexedRow => base_table()
                .row_style_at(0, Style::new().fg(ROW_FG).bg(ROW_BG))
                .column_style(1, Style::new().fg(COLUMN_FG).bg(COLUMN_BG))
                .row_style_full_width(true),
        }
        .into()
    }
}

fn base_table() -> Table {
    Table::new()
        .header(["H0", "H1"])
        .rows([TableRow::new(["A0", "B0"]), TableRow::new(["C0", "D0"])])
        .widths([ColumnWidth::Fixed(4), ColumnWidth::Fixed(4)])
        .column_spacing(1)
        .width(Length::Px(12))
        .height(Length::Px(3))
        .focusable(false)
}

fn render(case: TableStyleCase) -> CapturedFrame {
    let mut backend = TestBackend::new(case);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 12,
        h: 3,
    });
    backend.render();
    backend.capture_frame()
}

#[test]
fn column_style_applies_to_header_and_data_cells() {
    let frame = render(TableStyleCase::ColumnStyles);

    let header_cell = frame.cell(5, 0);
    assert_eq!(header_cell.symbol, "H");
    assert_eq!(header_cell.fg, COLUMN_FG);
    assert_eq!(header_cell.bg, COLUMN_BG);

    let data_cell = frame.cell(5, 1);
    assert_eq!(data_cell.symbol, "B");
    assert_eq!(data_cell.fg, COLUMN_FG);
    assert_eq!(data_cell.bg, COLUMN_BG);
}

#[test]
fn cell_style_overrides_column_style() {
    let frame = render(TableStyleCase::CellOverridesColumn);
    let cell = frame.cell(0, 1);

    assert_eq!(cell.symbol, "A");
    assert_eq!(cell.fg, CELL_FG);
    assert_eq!(cell.bg, CELL_BG);
}

#[test]
fn indexed_row_style_applies_to_data_row_but_not_header() {
    let frame = render(TableStyleCase::RowStyles);

    let header_cell = frame.cell(0, 0);
    assert_eq!(header_cell.symbol, "H");
    assert_ne!(header_cell.fg, ROW_FG);
    assert_ne!(header_cell.bg, ROW_BG);

    let first_data_cell = frame.cell(0, 1);
    assert_eq!(first_data_cell.symbol, "A");
    assert_eq!(first_data_cell.fg, ROW_FG);
    assert_eq!(first_data_cell.bg, ROW_BG);

    let second_data_cell = frame.cell(0, 2);
    assert_eq!(second_data_cell.symbol, "C");
    assert_ne!(second_data_cell.fg, ROW_FG);
    assert_ne!(second_data_cell.bg, ROW_BG);
}

#[test]
fn column_style_overrides_indexed_row_style_and_row_background_fills_gap() {
    let frame = render(TableStyleCase::ColumnOverridesIndexedRow);

    let first_column_cell = frame.cell(0, 1);
    assert_eq!(first_column_cell.symbol, "A");
    assert_eq!(first_column_cell.fg, ROW_FG);
    assert_eq!(first_column_cell.bg, ROW_BG);

    let second_column_cell = frame.cell(5, 1);
    assert_eq!(second_column_cell.symbol, "B");
    assert_eq!(second_column_cell.fg, COLUMN_FG);
    assert_eq!(second_column_cell.bg, COLUMN_BG);

    let gap = frame.cell(4, 1);
    assert_eq!(gap.symbol, " ");
    assert_eq!(gap.bg, ROW_BG);
}
