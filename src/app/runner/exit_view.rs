use std::cell::{Cell, RefCell};
use std::io::Write;

use crossterm::{execute, style::Print};
use ratatui::TerminalOptions;
use ratatui::backend::CrosstermBackend;

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
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    {
        let backend = CrosstermBackend::new(std::io::stdout());
        let mut terminal = ratatui::Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: ratatui::Viewport::Inline(height),
            },
        )?;
        terminal.draw(|f| render_tree(f, &ctx))?;
    }

    let mut stdout = std::io::stdout();
    execute!(stdout, Print("\n"))?;
    stdout.flush()?;
    Ok(())
}
