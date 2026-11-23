use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TableDisclosureState, TestBackend};

fn row_text(frame: &CapturedFrame, y: u16) -> String {
    (0..frame.width)
        .map(|x| frame.cell(x, y).symbol.as_str())
        .collect()
}

fn find_col(frame: &CapturedFrame, y: u16, needle: &str) -> Option<usize> {
    let row = row_text(frame, y);
    let byte_idx = row.find(needle)?;
    Some(row[..byte_idx].chars().count())
}

#[derive(Clone, Copy)]
enum DefaultWidthCase {
    ProcessesWithSelection,
    RowsOnly,
    PartialWidths,
    AutoWidthInHStack,
    NarrowViewport,
    SelectionSymbolOnly,
    InspectorWithoutPreset,
    SectionOnlyAutoWidth,
    SectionWiderThanColumnsAutoWidth,
    SeparatorOnlyAutoWidth,
}

impl Component for DefaultWidthCase {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        match self {
            Self::ProcessesWithSelection => Table::new()
                .header(TableRow::new(["PID", "Name", "CPU"]).style(Style::new().bold()))
                .rows([
                    TableRow::new(["1204", "lipan", "2.1"]),
                    TableRow::new(["2048", "cargo", "18.4"]),
                    TableRow::new(["4096", "rustc", "41.0"]),
                ])
                .selected(Some(1))
                .selection_symbol(Some("> "))
                .unselected_symbol(Some("  "))
                .key("table"),
            Self::RowsOnly => Table::new()
                .rows([
                    TableRow::new(["alpha", "100"]),
                    TableRow::new(["beta", "200"]),
                ])
                .selected(None)
                .focusable(false)
                .into(),
            Self::PartialWidths => Table::new()
                .header(["PID", "Name", "CPU"])
                .rows([
                    TableRow::new(["1204", "lipan", "2.1"]),
                    TableRow::new(["2048", "cargo", "18.4"]),
                ])
                .widths([ColumnWidth::Fixed(8)])
                .selected(None)
                .focusable(false)
                .into(),
            Self::AutoWidthInHStack => HStack::new()
                .align(Align::Start)
                .gap(1)
                .child(
                    Table::new()
                        .header(["ID", "Name"])
                        .rows([TableRow::new(["1", "lipan"])])
                        .width(Length::Auto)
                        .height(Length::Auto)
                        .selected(None)
                        .focusable(false),
                )
                .child(Text::new("|END"))
                .into(),
            Self::NarrowViewport => Table::new()
                .header(["ColumnOne", "ColumnTwo", "CPU"])
                .rows([TableRow::new(["aaaaaaaaa", "bbbbbbbbb", "99.9"])])
                .selected(None)
                .focusable(false)
                .into(),
            Self::SelectionSymbolOnly => Table::new()
                .header(["ID", "Task"])
                .rows([
                    TableRow::new(["10", "build"]),
                    TableRow::new(["20", "test"]),
                ])
                .selected(Some(1))
                .selection_symbol(Some(">> "))
                .key("table"),
            Self::InspectorWithoutPreset => Table::new()
                .inspector(true)
                .header(["Key", "Value"])
                .rows([
                    TableRow::section("A Very Long Section Title That Spans"),
                    TableRow::key_value("Host", "localhost")
                        .depth(1)
                        .disclosure(TableDisclosureState::Expanded),
                    TableRow::separator(),
                ])
                .selected(None)
                .focusable(false)
                .into(),
            Self::SectionOnlyAutoWidth => HStack::new()
                .align(Align::Start)
                .gap(1)
                .child(
                    Table::new()
                        .rows([TableRow::section("Metadata")])
                        .width(Length::Auto)
                        .height(Length::Auto)
                        .selected(None)
                        .focusable(false),
                )
                .child(Text::new("|END"))
                .into(),
            Self::SectionWiderThanColumnsAutoWidth => HStack::new()
                .align(Align::Start)
                .gap(1)
                .child(
                    Table::new()
                        .rows([
                            TableRow::section("Personal Info"),
                            TableRow::key_value("Key", "Val"),
                        ])
                        .width(Length::Auto)
                        .height(Length::Auto)
                        .selected(None)
                        .focusable(false),
                )
                .child(Text::new("|END"))
                .into(),
            Self::SeparatorOnlyAutoWidth => Table::new()
                .rows([TableRow::separator()])
                .width(Length::Auto)
                .height(Length::Auto)
                .selected(None)
                .focusable(false)
                .into(),
        }
    }
}

fn render_case(case: DefaultWidthCase, w: u16, h: u16, focus: bool) -> CapturedFrame {
    let mut backend = TestBackend::new(case);
    backend.set_viewport(Rect { x: 0, y: 0, w, h });
    backend.render();
    if focus {
        backend.focus_next();
        backend.render();
    }
    backend.capture_frame()
}

#[test]
fn table_without_widths_renders_header_all_rows_and_aligned_selection_symbol() {
    let frame = render_case(DefaultWidthCase::ProcessesWithSelection, 40, 6, true);

    let header = row_text(&frame, 0);
    let r0 = row_text(&frame, 1);
    let r1 = row_text(&frame, 2);
    let r2 = row_text(&frame, 3);

    assert!(header.contains("PID"), "header missing PID: {header:?}");
    assert!(header.contains("Name"), "header missing Name: {header:?}");
    assert!(header.contains("CPU"), "header missing CPU: {header:?}");

    assert!(r0.starts_with("  1204"), "row 0 prefix mismatch: {r0:?}");
    assert!(r0.contains("lipan") && r0.contains("2.1"), "row 0: {r0:?}");

    assert!(
        r1.starts_with("> 2048"),
        "row 1 selected symbol mismatch: {r1:?}"
    );
    assert!(r1.contains("cargo") && r1.contains("18.4"), "row 1: {r1:?}");

    assert!(r2.starts_with("  4096"), "row 2 prefix mismatch: {r2:?}");
    assert!(r2.contains("rustc") && r2.contains("41.0"), "row 2: {r2:?}");

    // Every column aligns vertically across header and data rows.
    let col0_x = header.find("PID").expect("PID column");
    assert_eq!(r0.find("1204"), Some(col0_x));
    assert_eq!(r1.find("2048"), Some(col0_x));
    assert_eq!(r2.find("4096"), Some(col0_x));

    let col1_x = header.find("Name").expect("Name column");
    assert_eq!(r0.find("lipan"), Some(col1_x));
    assert_eq!(r1.find("cargo"), Some(col1_x));
    assert_eq!(r2.find("rustc"), Some(col1_x));

    let col2_x = header.find("CPU").expect("CPU column");
    assert_eq!(r0.find("2.1"), Some(col2_x));
    assert_eq!(r1.find("18.4"), Some(col2_x));
    assert_eq!(r2.find("41.0"), Some(col2_x));
}

#[test]
fn table_without_widths_and_without_header_renders_all_rows() {
    let frame = render_case(DefaultWidthCase::RowsOnly, 30, 4, false);
    let r0 = row_text(&frame, 0);
    let r1 = row_text(&frame, 1);

    assert!(r0.contains("alpha") && r0.contains("100"), "row 0: {r0:?}");
    assert!(r1.contains("beta") && r1.contains("200"), "row 1: {r1:?}");
    assert_eq!(r0.find("100"), r1.find("200"));
}

#[test]
fn table_with_partial_widths_renders_remaining_columns() {
    let frame = render_case(DefaultWidthCase::PartialWidths, 36, 4, false);
    let header = row_text(&frame, 0);
    let r0 = row_text(&frame, 1);
    let r1 = row_text(&frame, 2);

    // Explicit Fixed(8) + 1 column_spacing = column 1 starts at x = 9.
    assert_eq!(header.find("Name"), Some(9), "header: {header:?}");
    assert_eq!(r0.find("lipan"), Some(9), "row 0: {r0:?}");
    assert_eq!(r1.find("cargo"), Some(9), "row 1: {r1:?}");

    // Column 2 must also render and align.
    let col2_x = header.find("CPU").expect("CPU column");
    assert!(col2_x > 9, "CPU should be after Name: {header:?}");
    assert_eq!(r0.find("2.1"), Some(col2_x));
    assert_eq!(r1.find("18.4"), Some(col2_x));
}

#[test]
fn table_without_widths_auto_width_measures_content() {
    let frame = render_case(DefaultWidthCase::AutoWidthInHStack, 30, 4, false);
    let header = row_text(&frame, 0);
    let r0 = row_text(&frame, 1);

    assert!(
        header.contains("ID") && header.contains("Name"),
        "header: {header:?}"
    );
    assert!(r0.contains('1') && r0.contains("lipan"), "row 0: {r0:?}");
    // Neighboring HStack child starts right after the auto-sized table + gap.
    let end_x = header.find("|END").expect("|END marker");
    // Col 0 width 2 ("ID") + 1 spacing + Col 1 width 5 ("lipan") = 8, plus HStack gap 1 = 9.
    assert_eq!(end_x, 9, "header: {header:?}");
}

#[test]
fn table_without_widths_narrow_viewport_keeps_last_column_visible() {
    // Natural widths are 9 + 1 + 9 + 1 + 4 = 24, viewport is 14: all 3 columns must remain visible.
    let frame = render_case(DefaultWidthCase::NarrowViewport, 14, 3, false);
    let header = row_text(&frame, 0);
    let r0 = row_text(&frame, 1);

    assert!(
        header.contains("CPU") || header.contains("CP"),
        "header lost last column: {header:?}"
    );
    assert!(
        r0.contains("99") || r0.contains('9'),
        "row 0 lost last column: {r0:?}"
    );
}

#[test]
fn table_without_widths_reserves_selection_symbol_space_in_first_column() {
    let frame = render_case(DefaultWidthCase::SelectionSymbolOnly, 30, 4, true);
    let r1 = row_text(&frame, 2);
    assert!(r1.starts_with(">> 20"), "selected row truncated: {r1:?}");
    assert!(r1.contains("test"), "selected row missing col 1: {r1:?}");
}

#[test]
fn table_without_widths_inspector_rows_render_without_section_bloating_key_col() {
    // Viewport width 28: section heading is 36 chars (would push col 1 off-screen if counted
    // toward col 0), while col 0 ("  ▾ Host" = 8) and col 1 ("localhost" = 9) fit easily.
    let frame = render_case(DefaultWidthCase::InspectorWithoutPreset, 28, 5, false);
    let header = row_text(&frame, 0);
    let section = row_text(&frame, 1);
    let kv = row_text(&frame, 2);
    let sep = row_text(&frame, 3);

    assert!(
        header.contains("Key") && header.contains("Value"),
        "header: {header:?}"
    );
    assert!(
        section.contains("A Very Long Section Title"),
        "section: {section:?}"
    );
    assert!(
        kv.contains("Host") && kv.contains("localhost"),
        "kv: {kv:?}"
    );
    assert!(sep.contains('─'), "separator: {sep:?}");
    let val_x = find_col(&frame, 0, "Value").expect("Value column");
    assert!(val_x < 18, "section heading bloated col 0: {header:?}");
    assert_eq!(find_col(&frame, 2, "localhost"), Some(val_x), "kv: {kv:?}");
}

#[test]
fn table_section_only_auto_width_renders_section_title() {
    let frame = render_case(DefaultWidthCase::SectionOnlyAutoWidth, 30, 4, false);
    let r0 = row_text(&frame, 0);

    assert!(r0.starts_with("Metadata"), "section row missing: {r0:?}");
    // "Metadata" (8) + HStack gap (1) = "|END" at x = 9.
    assert_eq!(find_col(&frame, 0, "|END"), Some(9), "row 0: {r0:?}");
}

#[test]
fn table_section_wider_than_columns_auto_width_keeps_full_section_without_bloating_col0() {
    let frame = render_case(
        DefaultWidthCase::SectionWiderThanColumnsAutoWidth,
        30,
        4,
        false,
    );
    let section = row_text(&frame, 0);
    let kv = row_text(&frame, 1);

    assert!(
        section.starts_with("Personal Info"),
        "section row truncated: {section:?}"
    );
    // "Personal Info" (13) + HStack gap (1) = "|END" at x = 14.
    assert_eq!(
        find_col(&frame, 0, "|END"),
        Some(14),
        "section: {section:?}"
    );
    // Key and Val both render and Val sits well left of 13 (col 0 was not bloated to 13).
    assert!(kv.contains("Key") && kv.contains("Val"), "kv: {kv:?}");
    let val_x = find_col(&frame, 1, "Val").expect("Val column");
    assert!(val_x < 10, "section heading bloated col 0: {kv:?}");
}

#[test]
fn table_separator_only_auto_width_renders_separator() {
    let frame = render_case(DefaultWidthCase::SeparatorOnlyAutoWidth, 10, 2, false);
    let r0 = row_text(&frame, 0);

    assert!(r0.starts_with('─'), "separator missing: {r0:?}");
}
