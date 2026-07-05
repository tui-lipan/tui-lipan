use std::cell::{Cell, RefCell};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Position;
use ratatui::style::Modifier;

use crate::app::ContrastPolicy;
use crate::capture::{CapturedCell, CapturedFrame, CellModifiers, CursorState};
use crate::core::node::{NodeId, NodeTree};
use crate::style::Rect;
use crate::style::Style;

use super::common::from_ratatui_color;
use super::render::{RenderContext, build_join_index, render};

fn convert_modifiers(modifier: Modifier) -> CellModifiers {
    CellModifiers {
        bold: modifier.contains(Modifier::BOLD),
        dim: modifier.contains(Modifier::DIM),
        italic: modifier.contains(Modifier::ITALIC),
        underline: modifier.contains(Modifier::UNDERLINED),
        reverse: modifier.contains(Modifier::REVERSED),
        strikethrough: modifier.contains(Modifier::CROSSED_OUT),
    }
}

/// Focus and pointer state for a headless capture.
#[derive(Clone, Copy, Default)]
pub(crate) struct CaptureInteraction {
    pub focused: Option<NodeId>,
    pub hovered: Option<NodeId>,
    pub mouse_pos: Option<(u16, u16)>,
}

pub(crate) fn render_to_captured_frame_with_interaction(
    tree: &NodeTree,
    viewport: Rect,
    interaction: CaptureInteraction,
    effect_phase: u64,
    screen_background: Option<ratatui::style::Style>,
) -> CapturedFrame {
    let CaptureInteraction {
        focused,
        hovered,
        mouse_pos,
    } = interaction;
    let join_index = build_join_index(tree);
    let width = viewport.w.max(1);
    let height = viewport.h.max(1);

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("capture terminal should init");

    let cursor_position = Cell::new(None::<Position>);
    let scrollbar_metrics_cache = RefCell::new(Default::default());
    let overlay_bg_snapshot = RefCell::new(Vec::new());
    let dnd_snapshot_cells = RefCell::new(None);

    let ctx = RenderContext {
        tree,
        focused,
        hovered,
        mouse_pos,
        suppress_pointer_item_hover_nodes: None,
        blink_visible: true,
        effect_phase,
        images_enabled: false,
        contrast_policy: ContrastPolicy::Off,
        read_only_selection: None,
        scrollbar_metrics_cache: &scrollbar_metrics_cache,
        overlay_bg_snapshot: &overlay_bg_snapshot,
        join_index: &join_index,
        cursor_position: &cursor_position,
        terminal_bg: None,
        drag_preview_label: None,
        drag_preview_at_mouse: false,
        drag_preview_snapshot_rect: None,
        dnd_snapshot_cells: &dnd_snapshot_cells,
        drag_preview_max_width: None,
        drag_preview_max_height: None,
        drop_slot_source_preview_rect: None,
        paint_glyph_caches: None,
        copy_feedback: None,
        copy_feedback_style: Style::default(),
    };

    let _screen_bg_scope =
        crate::backend::ratatui_backend::common::push_render_screen_background(screen_background);

    terminal
        .draw(|frame| render(frame, &ctx))
        .expect("capture render should succeed");

    let buffer = terminal.backend().buffer();
    let area = buffer.area();
    let mut cells = Vec::with_capacity(usize::from(area.width) * usize::from(area.height));

    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buffer[(x, y)];
            cells.push(CapturedCell {
                symbol: cell.symbol().to_owned(),
                fg: from_ratatui_color(cell.fg),
                bg: from_ratatui_color(cell.bg),
                underline_color: from_ratatui_color(cell.underline_color),
                modifiers: convert_modifiers(cell.modifier),
            });
        }
    }

    let cursor = cursor_position.get().map(|pos| CursorState {
        x: pos.x,
        y: pos.y,
        visible: true,
    });

    CapturedFrame {
        viewport,
        width: area.width,
        height: area.height,
        cells,
        cursor,
    }
}

#[cfg(test)]
mod hscroll_clip_tests {
    use super::{CaptureInteraction, render_to_captured_frame_with_interaction};
    use crate::core::element::IntoElement;
    use crate::core::node::{NodeKind, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::style::{Length, Rect};
    use crate::widgets::{HStack, ScrollAxis, ScrollView, Text};

    #[test]
    fn hscroll_clips_left_edge_not_right() {
        let root = ScrollView::new()
            .axis(ScrollAxis::Both)
            .child(
                HStack::new()
                    .width(Length::Auto)
                    .child(Text::new("AAAA"))
                    .child(Text::new("BBBB"))
                    .child(Text::new("CCCC")),
            )
            .into();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 6,
            h: 3,
        };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, viewport, None);

        {
            let NodeKind::ScrollView(sv) = &mut tree.node_mut(tree.root).kind else {
                panic!("expected scroll view");
            };
            sv.h_offset = 2;
            sv.h_scroll_override = Some(2);
            sv.h_scroll_handler_dirty = true;
        }
        LayoutEngine::reconcile_with_focus(&mut tree, &root, viewport, None);

        let frame = render_to_captured_frame_with_interaction(
            &tree,
            viewport,
            CaptureInteraction::default(),
            0,
            None,
        );
        let row0 = frame.to_fixed_grid_lines()[0].clone();
        // Content cols: AAAA(0-3) BBBB(4-7) CCCC(8-11). With h_offset=2 the left
        // edge should show from col 2 -> "AABBBB".
        assert_eq!(row0, "AABBBB", "h-scroll should clip the LEFT edge");
    }

    #[test]
    fn hscroll_clips_left_edge_faithful() {
        let rows = (0..24).map(|r| {
            HStack::new()
                .gap(2)
                .width(Length::Auto)
                .height(Length::Px(1))
                .child(Text::new(format!("r{r:02}aaaa")))
                .child(Text::new("bbbbbb"))
                .child(Text::new("cccccc"))
                .key(format!("row-{r}"))
        });
        let root = ScrollView::new()
            .axis(ScrollAxis::Both)
            .scrollbar(true)
            .h_scrollbar(true)
            .children(rows)
            .into();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 14,
            h: 8,
        };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, viewport, None);
        {
            let NodeKind::ScrollView(sv) = &mut tree.node_mut(tree.root).kind else {
                panic!("expected scroll view");
            };
            sv.h_offset = 3;
            sv.h_scroll_override = Some(3);
            sv.h_scroll_handler_dirty = true;
        }
        LayoutEngine::reconcile_with_focus(&mut tree, &root, viewport, None);
        let frame = render_to_captured_frame_with_interaction(
            &tree,
            viewport,
            CaptureInteraction::default(),
            0,
            None,
        );
        let row0 = frame.to_lines()[0].clone();
        // Row content per line: "rNNaaaa"(0-6) gap(7-8) "bbbbbb"(9-14)...
        // With h_offset=3 the left edge must show from col 3 ("aaaa  bbbbbb"),
        // i.e. the left is clipped. Regression: it previously showed "r00a..."
        // (the start of the cell, clipped on the right) because the Ellipsis
        // overflow path ignored the horizontal clip offset.
        assert!(
            row0.starts_with("aaaa"),
            "h-scroll must clip the left edge, got: {row0:?}"
        );
        assert!(
            !row0.starts_with("r0"),
            "leftmost cell must not show its start when scrolled, got: {row0:?}"
        );
    }

    #[test]
    fn hscroll_clips_left_edge_cell_aware_ellipsis() {
        use crate::style::{Paint, Style};
        use crate::widgets::Overflow;

        // Alpha background forces the cell-aware render path; Ellipsis is the
        // single-line default that previously ignored the horizontal clip offset.
        let root = ScrollView::new()
            .axis(ScrollAxis::Both)
            .child(
                Text::new("0123456789ABCDEF")
                    .overflow(Overflow::Ellipsis)
                    .style(Style::default().bg(Paint::rgba(20, 20, 20, 128))),
            )
            .into();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 6,
            h: 3,
        };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, viewport, None);
        {
            let NodeKind::ScrollView(sv) = &mut tree.node_mut(tree.root).kind else {
                panic!("expected scroll view");
            };
            sv.h_offset = 3;
            sv.h_scroll_override = Some(3);
            sv.h_scroll_handler_dirty = true;
        }
        LayoutEngine::reconcile_with_focus(&mut tree, &root, viewport, None);
        let frame = render_to_captured_frame_with_interaction(
            &tree,
            viewport,
            CaptureInteraction::default(),
            0,
            None,
        );
        let row0 = frame.to_lines()[0].clone();
        // h_offset=3 -> left edge shows from col 3 ("345678"), not "012...".
        assert!(
            row0.starts_with("345"),
            "cell-aware ellipsis must clip the LEFT edge, got: {row0:?}"
        );
    }
}
