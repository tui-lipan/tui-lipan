use super::layout::{terminal_content_layout, terminal_lines};
use super::mod_private::Terminal;
use super::node::TerminalNode;
use super::screen::TerminalViewport;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::{LayoutConstraints, Rect, ScrollbarVariant};

pub(crate) fn reconcile_terminal(
    tree: &mut NodeTree,
    id: NodeId,
    terminal: &Terminal,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    let lines = terminal
        .color_lines
        .clone()
        .unwrap_or_else(|| terminal_lines(terminal.content.as_ref()));

    let avail_w = rect.w;
    let avail_h = rect.h;
    let mut rect = rect;
    let parent_v_edge = tree.parent_frame_integrated_v_edge(id).unwrap_or(false);
    let scrollbar_visible = terminal.scrollbar && terminal.total_scrollback_rows > 0;
    let scrollbar_integrated = scrollbar_visible
        && matches!(terminal.scrollbar_variant, ScrollbarVariant::Integrated)
        && (terminal.border || parent_v_edge);
    let _scrollbar_cols = if scrollbar_visible && !scrollbar_integrated {
        1
    } else {
        0
    };

    // Note: Terminal builder should already have handled Length::Auto via measure_terminal,
    // but we ensure constraints are applied here.
    rect.w = constraints.clamp_width(rect.w, avail_w);
    rect.h = constraints.clamp_height(rect.h, avail_h);

    // Compute viewport rows (inner area height).
    let inner = rect.inner(terminal.border, terminal.padding);
    let layout = terminal_content_layout(
        inner,
        terminal.border,
        terminal.scrollbar,
        terminal.scrollbar_variant,
        terminal.scrollbar_gap,
        terminal.total_scrollback_rows,
        parent_v_edge,
    );
    let viewport_rows = layout.content_rect.h as usize;
    let viewport_cols = layout.content_rect.w as usize;

    // Read previous scroll state from existing node to persist across frames.
    let (old_scroll_override, old_selection, old_viewport_rows, old_viewport_cols) =
        if let NodeKind::Terminal(old) = &tree.node(id).kind {
            (
                old.scroll_override,
                old.selection.clone(),
                old.viewport_rows,
                old.viewport_cols,
            )
        } else {
            (None, None, 0, 0)
        };

    // Determine effective scrollback offset:
    // - If the mouse scroll handler set a scroll_override, use it.
    // - Otherwise use whatever the snapshot provides (which the user set
    //   via TerminalScreen::set_scrollback).
    let scrollback_offset = old_scroll_override.unwrap_or(terminal.scrollback_offset);
    let selection = if terminal.selection_controlled {
        terminal.selection.clone()
    } else {
        old_selection.or(terminal.selection.clone())
    };

    if let Some(cb) = terminal.on_resize.as_ref()
        && (viewport_cols != old_viewport_cols || viewport_rows != old_viewport_rows)
    {
        let cols = viewport_cols.max(1).min(u16::MAX as usize) as u16;
        let rows = viewport_rows.max(1).min(u16::MAX as usize) as u16;
        cb.emit(TerminalViewport { cols, rows });
    }

    let node = tree.node_mut(id);
    node.rect = rect;
    node.children.clear();
    node.kind = NodeKind::Terminal(TerminalNode {
        lines,
        cursor_row: terminal.cursor_row,
        cursor_col: terminal.cursor_col,
        cursor_visible: terminal.show_cursor,
        cursor_shape: terminal.cursor_shape,
        cursor_blinking: terminal.cursor_blinking,
        selection,
        selection_style: terminal.selection_style,
        mouse_mode: terminal.mouse_mode,
        on_selection: terminal.on_selection.clone(),
        on_mouse_forward: terminal.on_mouse_forward.clone(),
        style: terminal.style,
        hover_style: terminal.hover_style,
        focus_style: terminal.focus_style,
        focus_content_style: terminal.focus_content_style,
        border: terminal.border,
        border_style: terminal.border_style,
        padding: terminal.padding,
        scrollback_offset,
        total_scrollback_rows: terminal.total_scrollback_rows,
        viewport_rows,
        viewport_cols,
        scroll_wheel: terminal.scroll_wheel,
        scroll_override: old_scroll_override,
        scrollbar: terminal.scrollbar,
        scrollbar_variant: terminal.scrollbar_variant,
        scrollbar_gap: terminal.scrollbar_gap,
        scrollbar_thumb: terminal.scrollbar_thumb,
        scrollbar_thumb_style: terminal.scrollbar_thumb_style,
        scrollbar_thumb_focus_style: terminal.scrollbar_thumb_focus_style,
        scrollbar_track_style: terminal.scrollbar_track_style,
        on_scroll: terminal.on_scroll.clone(),
        on_scroll_to: terminal.on_scroll_to.clone(),
        focusable: terminal.focusable,
        on_key: terminal.on_key.clone(),
        on_input: terminal.on_input.clone(),
    });

    tree.register_scrollbar_zone(id);

    id
}
