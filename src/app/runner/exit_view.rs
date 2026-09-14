use std::cell::{Cell, RefCell};
use std::io::Write;
use std::time::Duration;

use crossterm::{execute, style::Print};
use ratatui::TerminalOptions;
use ratatui::backend::{Backend, CrosstermBackend};

use crate::Result;
use crate::app::ContrastPolicy;
use crate::backend::ratatui_backend::OwnedTerminal;
use crate::backend::ratatui_backend::common::to_ratatui_color;
use crate::backend::ratatui_backend::render::{
    RenderContext, build_join_index, render as render_tree,
};
use crate::backend::ratatui_backend::tty_liveness::{cursor_row, host_tty_hung_up};
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
    // Nobody is left to read it, and placing an inline viewport asks the terminal where the cursor
    // is, which would spin forever on a hung-up tty.
    if host_tty_hung_up() {
        return Ok(());
    }
    let width = crossterm::terminal::size()?.0.max(1);
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

    {
        let mut backend = CrosstermBackend::new(std::io::stdout());
        let Some(area) = inline_area(&mut backend, height)? else {
            return Ok(());
        };
        let mut terminal = OwnedTerminal::new(ratatui::Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: ratatui::Viewport::Fixed(area),
            },
        )?);
        terminal.draw(|f| render_tree(f, &ctx))?;
    }

    let mut stdout = std::io::stdout();
    execute!(stdout, Print("\n"))?;
    stdout.flush()?;
    Ok(())
}

/// Where `height` rows of output go below the cursor, scrolling to make room: the placement
/// ratatui's `Viewport::Inline` makes, with the cursor row read by [`cursor_row`] instead of
/// crossterm, which cannot give up on a terminal that hangs up while it waits. `None` when the
/// terminal does not say where its cursor is.
fn inline_area(
    backend: &mut CrosstermBackend<std::io::Stdout>,
    height: u16,
) -> std::io::Result<Option<ratatui::layout::Rect>> {
    const CURSOR_REPLY_TIMEOUT: Duration = Duration::from_secs(2);

    let Some(mut row) = cursor_row(CURSOR_REPLY_TIMEOUT) else {
        return Ok(None);
    };
    let size = backend.size()?;
    let lines_after_cursor = height.saturating_sub(1);
    backend.append_lines(lines_after_cursor)?;
    let available_lines = size.height.saturating_sub(row).saturating_sub(1);
    row = row.saturating_sub(lines_after_cursor.saturating_sub(available_lines));
    Ok(Some(ratatui::layout::Rect {
        x: 0,
        y: row,
        width: size.width,
        height: size.height.min(height),
    }))
}
