use std::cell::{Cell, RefCell};
use std::io::Write;

use crossterm::style::{
    Attribute, Color as CrosstermColor, Print, PrintStyledContent, SetAttribute,
    SetBackgroundColor, SetForegroundColor, SetUnderlineColor,
};
use crossterm::{execute, queue};
use ratatui::backend::{IntoCrossterm, TestBackend};
use ratatui::buffer::{Buffer, Cell as RatatuiCell, CellDiffOption, CellWidth};
use ratatui::style::Color as RatatuiColor;

use crate::Result;
use crate::app::ContrastPolicy;
use crate::backend::ratatui_backend::common::to_ratatui_color;
use crate::backend::ratatui_backend::render::{
    RenderContext, build_join_index, render as render_tree,
};
use crate::core::element::Element;
use crate::core::node::NodeTree;
use crate::layout::measure::min_size_constrained;
use crate::layout::reconcile::reconcile_with_overlays_mode;
use crate::style::{Color, Rect, Style};

pub(crate) fn render(
    element: Element,
    contrast_policy: ContrastPolicy,
    terminal_bg: Option<Color>,
) -> Result<()> {
    let width = crossterm::terminal::size()?.0.max(1);
    let mut stdout = std::io::stdout();
    render_to_writer(element, contrast_policy, terminal_bg, width, &mut stdout)
}

fn render_to_writer(
    element: Element,
    contrast_policy: ContrastPolicy,
    terminal_bg: Option<Color>,
    width: u16,
    writer: &mut impl Write,
) -> Result<()> {
    let height = min_size_constrained(&element, Some(width), None).1;

    if height == 0 {
        return Ok(());
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: width,
        h: height,
    };

    let mut tree = NodeTree::new();
    reconcile_with_overlays_mode(&mut tree, &element, bounds, None, &[], true);
    let join_index = build_join_index(&tree);

    let scrollbar_metrics_cache = RefCell::new(Default::default());
    let overlay_bg_snapshot = RefCell::new(Vec::new());
    let cursor_position = Cell::new(None);
    let ctx = RenderContext {
        tree: &tree,
        focused: None,
        hovered: None,
        mouse_pos: None,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase: 0,
        images_enabled: false,
        contrast_policy,
        read_only_selection: None,
        scrollbar_metrics_cache: &scrollbar_metrics_cache,
        overlay_bg_snapshot: &overlay_bg_snapshot,
        join_index: &join_index,
        cursor_position: &cursor_position,
        terminal_bg: terminal_bg.map(to_ratatui_color),
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &RefCell::new(None),
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drag_preview_grab_offset: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    // This runs after the input reader and the main terminal have shut down. Render in memory so
    // the exit view never constructs an inline viewport and therefore never sends a CPR query.
    let backend = TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).unwrap_or_else(|never| match never {});
    let completed = terminal
        .draw(|frame| render_tree(frame, &ctx))
        .unwrap_or_else(|never| match never {});

    write_buffer(writer, completed.buffer)?;
    writer.flush()?;
    Ok(())
}

fn write_buffer(writer: &mut impl Write, buffer: &Buffer) -> std::io::Result<()> {
    let area = *buffer.area();
    execute!(writer, crossterm::cursor::MoveToColumn(0))?;

    for y in 0..area.height {
        write_row(writer, buffer, y)?;
        queue!(writer, Print("\r\n"))?;
    }

    reset_style(writer)?;

    Ok(())
}

fn write_row(writer: &mut impl Write, buffer: &Buffer, y: u16) -> std::io::Result<()> {
    let area = *buffer.area();
    let last = row_content_end(buffer, y);
    let mut x = 0;

    while x < last {
        let cell = &buffer[(x, y)];
        let width = cell.cell_width().max(1);
        if cell.diff_option != CellDiffOption::Skip {
            queue!(
                writer,
                crossterm::cursor::MoveToColumn(x),
                PrintStyledContent(cell.style().into_crossterm().apply(cell.symbol()))
            )?;
        }
        x = x.saturating_add(width).min(area.width);
    }

    Ok(())
}

fn row_content_end(buffer: &Buffer, y: u16) -> u16 {
    let area = *buffer.area();
    let mut end = 0;
    let mut x = 0;

    while x < area.width {
        let cell = &buffer[(x, y)];
        let width = cell.cell_width().max(1);
        if cell.diff_option != CellDiffOption::Skip && !is_empty_cell(cell) {
            end = x.saturating_add(width).min(area.width);
        }
        x = x.saturating_add(width).min(area.width);
    }

    end
}

fn reset_style(writer: &mut impl Write) -> std::io::Result<()> {
    queue!(
        writer,
        SetForegroundColor(CrosstermColor::Reset),
        SetBackgroundColor(CrosstermColor::Reset),
        SetUnderlineColor(CrosstermColor::Reset),
        SetAttribute(Attribute::Reset),
    )
}

fn is_empty_cell(cell: &RatatuiCell) -> bool {
    cell.symbol() == " "
        && cell.fg == RatatuiColor::Reset
        && cell.bg == RatatuiColor::Reset
        && cell.underline_color == RatatuiColor::Reset
        && cell.modifier.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::{Text, VStack};

    #[test]
    fn exit_view_renders_without_cursor_position_query() {
        let element = VStack::new()
            .child(Text::new("Detached from dev"))
            .child(Text::new("Reattach: rozi sessions attach dev"))
            .child(Text::new("界x"))
            .child(Text::new("❤️x"))
            .child(Text::new("1️⃣x"))
            .into();
        let mut output = Vec::new();

        render_to_writer(element, ContrastPolicy::Off, None, 80, &mut output).unwrap();

        let output = String::from_utf8(output).unwrap();
        let visible = visible_text(&output);
        assert!(visible.contains("Detached from dev"));
        assert!(visible.contains("Reattach: rozi sessions attach dev"));
        assert!(visible.contains("界x"), "{visible:?}");
        assert_following_cell_positioned(&output, "❤️", 2, "x");
        assert_following_cell_positioned(&output, "1️⃣", 2, "x");
        assert!(!output.contains("\x1b[6n"));
    }

    #[test]
    fn exit_view_resets_underline_color_before_returning_to_the_shell() {
        let element = Text::new("underlined")
            .style(Style::new().underline_color(Color::Red))
            .into();
        let mut output = Vec::new();

        render_to_writer(element, ContrastPolicy::Off, None, 80, &mut output).unwrap();

        let mut underline_red = Vec::new();
        queue!(underline_red, SetUnderlineColor(CrosstermColor::DarkRed)).unwrap();
        let mut reset = Vec::new();
        reset_style(&mut reset).unwrap();
        assert!(
            output
                .windows(underline_red.len())
                .any(|window| window == underline_red)
        );
        assert!(output.ends_with(&reset));
    }

    fn visible_text(output: &str) -> String {
        crate::style::parse_ansi(output)
            .into_iter()
            .fold(String::new(), |mut text, span| {
                text.push_str(&span.content);
                text
            })
    }

    fn assert_following_cell_positioned(
        output: &str,
        symbol: &str,
        expected_column: u16,
        following: &str,
    ) {
        let row = output
            .split("\r\n")
            .find(|row| row.contains(symbol))
            .expect("emoji row should be present");
        let symbol_end = row.find(symbol).unwrap() + symbol.len();
        let after_symbol = &row[symbol_end..];
        let mut move_to_column = Vec::new();
        queue!(
            move_to_column,
            crossterm::cursor::MoveToColumn(expected_column)
        )
        .unwrap();
        let move_to_column = String::from_utf8(move_to_column).unwrap();
        let repositioned = after_symbol
            .find(&move_to_column)
            .expect("following cell should have an explicit column");
        let following = after_symbol
            .find(following)
            .expect("following cell should be rendered");
        assert!(repositioned < following, "{row:?}");
    }
}
