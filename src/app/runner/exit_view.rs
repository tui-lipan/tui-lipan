use std::cell::{Cell, RefCell};
use std::io::Write;

use crossterm::style::{Print, PrintStyledContent};
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

    Ok(())
}

fn write_row(writer: &mut impl Write, buffer: &Buffer, y: u16) -> std::io::Result<()> {
    let area = *buffer.area();
    let mut last = 0;
    let mut x = 0;

    while x < area.width {
        let cell = &buffer[(x, y)];
        let width = cell.cell_width().max(1);
        if cell.diff_option != CellDiffOption::Skip && !is_empty_cell(cell) {
            last = x.saturating_add(width).min(area.width);
        }
        x = x.saturating_add(width);
    }

    x = 0;
    while x < last {
        while x < last && buffer[(x, y)].diff_option == CellDiffOption::Skip {
            x += 1;
        }
        if x == last {
            break;
        }

        let style = buffer[(x, y)].style();
        let mut text = String::new();

        while x < last && buffer[(x, y)].style() == style {
            let cell = &buffer[(x, y)];
            if cell.diff_option != CellDiffOption::Skip {
                text.push_str(cell.symbol());
            }
            x = x.saturating_add(cell.cell_width().max(1));
        }

        if !text.is_empty() {
            queue!(
                writer,
                PrintStyledContent(style.into_crossterm().apply(text))
            )?;
        }
    }

    Ok(())
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
            .into();
        let mut output = Vec::new();

        render_to_writer(element, ContrastPolicy::Off, None, 80, &mut output).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Detached from dev"));
        assert!(output.contains("Reattach: rozi sessions attach dev"));
        assert!(output.contains("界x"), "{output:?}");
        assert!(!output.contains("\x1b[6n"));
    }
}
