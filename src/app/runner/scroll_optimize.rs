use std::collections::HashSet;
use std::ops::Range;

use ratatui::buffer::{Buffer, Cell};

use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::{Rect, ScrollbarVariant};

use super::ScrollFrameSnapshot;

#[derive(Clone, Debug)]
pub(super) struct IncrementalScrollPlan {
    pub(super) scroll_rows: Range<u16>,
    pub(super) delta_rows: i16,
    pub(super) repaint_regions: Vec<Rect>,
}
pub(super) fn collect_descendants(tree: &NodeTree, root: NodeId, out: &mut HashSet<NodeId>) {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !tree.is_valid(id) || !out.insert(id) {
            continue;
        }
        for &child in &tree.node(id).children {
            stack.push(child);
        }
    }
}

pub(super) fn subtree_has_hoverables(tree: &NodeTree, root: NodeId) -> bool {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !tree.is_valid(id) {
            continue;
        }

        let node = tree.node(id);
        if node.is_hoverable() {
            return true;
        }

        stack.extend(node.children.iter().copied());
    }

    false
}

pub(super) fn collect_scroll_repaint_regions(
    tree: &NodeTree,
    scroll_id: NodeId,
    scroll_band: Rect,
    frame_area: ratatui::layout::Rect,
) -> Vec<Rect> {
    let mut allowed = HashSet::new();
    collect_descendants(tree, scroll_id, &mut allowed);

    let mut parent = Some(scroll_id);
    while let Some(id) = parent {
        if !tree.is_valid(id) {
            break;
        }
        allowed.insert(id);
        parent = tree.node(id).parent;
    }

    let mut regions = Vec::new();

    for node in tree.iter() {
        if allowed.contains(&node.id) {
            continue;
        }

        if let Some(parent) = node.parent
            && !allowed.contains(&parent)
        {
            let parent_rect = tree.node(parent).rect.intersection(&scroll_band);
            if !parent_rect.is_empty() {
                continue;
            }
        }

        let clipped = node.rect.intersection(&scroll_band);
        if clipped.is_empty() {
            continue;
        }

        regions.push(Rect {
            x: clipped.x.saturating_add(frame_area.x as i16),
            y: clipped.y.saturating_add(frame_area.y as i16),
            w: clipped.w,
            h: clipped.h,
        });
    }

    regions
}

pub(super) fn capture_scroll_frames(tree: &NodeTree) -> Vec<ScrollFrameSnapshot> {
    let mut snapshots = Vec::new();

    for node in tree.iter() {
        let NodeKind::ScrollView(scroll_view) = &node.kind else {
            continue;
        };

        let mut inner = node
            .rect
            .inner(scroll_view.props.border, scroll_view.props.padding);
        if inner.w == 0 || inner.h == 0 {
            continue;
        }

        if scroll_view.show_scroll_indicators {
            if scroll_view.top_indicator {
                inner.y = inner.y.saturating_add(1);
                inner.h = inner.h.saturating_sub(1);
            }
            if scroll_view.bottom_indicator {
                inner.h = inner.h.saturating_sub(1);
            }
        }

        // Exclude the standalone horizontal scrollbar row from the scrollable
        // region (mirrors the renderer). Otherwise the incremental scroll-region
        // shift drags the bottom scrollbar with the content, leaving trails and
        // duplicates during vertical scrolling.
        let h_integrated = scroll_view.props.border
            && scroll_view.h_scrollbar
            && matches!(
                scroll_view.h_scrollbar_variant,
                ScrollbarVariant::Integrated
            );
        let h_standalone = scroll_view.h_scrollbar && scroll_view.h_max_offset > 0 && !h_integrated;
        if h_standalone && inner.h > 0 {
            inner.h = inner
                .h
                .saturating_sub(1u16.saturating_add(scroll_view.h_scrollbar_gap));
        }

        if inner.h == 0 {
            continue;
        }

        let parent_border_x = if !scroll_view.props.border
            && scroll_view.scrollbar
            && matches!(scroll_view.scrollbar_variant, ScrollbarVariant::Integrated)
        {
            tree.ancestor_frame_integrated_vscrollbar_x(node.parent)
        } else {
            None
        };

        let use_integrated = (scroll_view.props.border || parent_border_x.is_some())
            && matches!(scroll_view.scrollbar_variant, ScrollbarVariant::Integrated);
        let use_standalone = scroll_view.scrollbar && !use_integrated;

        let scrollbar_rect = if scroll_view.scrollbar && inner.w > 0 && inner.h > 0 {
            let x = if use_integrated {
                parent_border_x.unwrap_or_else(|| {
                    node.rect
                        .x
                        .saturating_add(node.rect.w.saturating_sub(1) as i16)
                })
            } else if use_standalone {
                inner.x.saturating_add(inner.w.saturating_sub(1) as i16)
            } else {
                0
            };

            if use_integrated || use_standalone {
                Some(Rect {
                    x,
                    y: inner.y,
                    w: 1,
                    h: inner.h,
                })
            } else {
                None
            }
        } else {
            None
        };

        snapshots.push(ScrollFrameSnapshot {
            node_id: node.id,
            scroll_offset: scroll_view.scroll_offset,
            content_height: scroll_view.content_height,
            content_hash: scroll_view.layout_cache.active_content_hash,
            viewport_height: scroll_view.viewport_height,
            scroll_rows: inner.y.max(0) as u16
                ..inner.y.saturating_add(inner.h as i16).max(0) as u16,
            scrollbar_rect,
            show_scroll_indicators: scroll_view.show_scroll_indicators,
        });
    }

    snapshots
}

/// Rebuild `buf` into the frame the app intends to show after an incremental
/// scroll: rows shifted in place, exposed rows filled with the configured root
/// viewport background so the surface stays continuous through scroll regions.
pub(super) fn apply_rendered_scroll(
    buf: &mut Buffer,
    scroll_rows: &Range<u16>,
    delta_rows: i16,
    exposed_rows: &Range<u16>,
) {
    shift_buffer_rows(buf, scroll_rows, delta_rows);
    fill_buffer_rows(buf, exposed_rows, &screen_background_cell());
}

/// Rebuild `buf` into what the *terminal* displays after its scroll-region
/// command: rows shifted in place, exposed rows at the terminal default.
///
/// `CSI S`/`CSI T` carry no SGR of their own and the backend resets colors after
/// every draw, so background-color erase fills the exposed rows with the default
/// background no matter which screen background is configured. The diff baseline
/// has to model that. Filling it with the screen background instead — as
/// [`apply_rendered_scroll`] does — marks every blank cell already-correct, so
/// the diff skips them and leaves default-background holes on screen wherever
/// the scroll exposed empty space.
pub(super) fn apply_terminal_scroll(
    buf: &mut Buffer,
    scroll_rows: &Range<u16>,
    delta_rows: i16,
    exposed_rows: &Range<u16>,
) {
    shift_buffer_rows(buf, scroll_rows, delta_rows);
    fill_buffer_rows(buf, exposed_rows, &Cell::EMPTY);
}

fn shift_buffer_rows(buf: &mut Buffer, rows: &Range<u16>, delta_rows: i16) {
    if delta_rows == 0 || rows.start >= rows.end {
        return;
    }

    let area = buf.area;
    let start = rows.start.saturating_sub(area.y) as usize;
    let end = rows.end.saturating_sub(area.y) as usize;
    if start >= end || end > area.height as usize {
        return;
    }

    let height = end - start;
    let shift = delta_rows.unsigned_abs() as usize;
    if shift == 0 || shift >= height {
        return;
    }

    let width = area.width as usize;

    if delta_rows > 0 {
        for row in 0..(height - shift) {
            let dst = (start + row) * width;
            let src = (start + row + shift) * width;
            let (head, tail) = buf.content.split_at_mut(src);
            head[dst..dst + width].clone_from_slice(&tail[..width]);
        }
    } else {
        for row in (shift..height).rev() {
            let dst = (start + row) * width;
            let src = (start + row - shift) * width;
            let (head, tail) = buf.content.split_at_mut(dst);
            tail[..width].clone_from_slice(&head[src..src + width]);
        }
    }
}

fn screen_background_cell() -> Cell {
    match crate::backend::ratatui_backend::common::current_render_screen_background() {
        Some(style) => {
            let mut cell = Cell::EMPTY;
            cell.set_style(style);
            cell
        }
        None => Cell::EMPTY,
    }
}

fn fill_buffer_rows(buf: &mut Buffer, rows: &Range<u16>, fill: &Cell) {
    if rows.start >= rows.end {
        return;
    }

    let area = buf.area;
    let start = rows.start.saturating_sub(area.y) as usize;
    let end = rows.end.saturating_sub(area.y) as usize;
    if start >= end || end > area.height as usize {
        return;
    }

    let width = area.width as usize;
    for row in start..end {
        let row_start = row * width;
        let row_end = row_start + width;
        buf.content[row_start..row_end].fill(fill.clone());
    }
}

pub(super) fn replace_buffer_snapshot(slot: &mut Option<Buffer>, source: &Buffer) {
    match slot {
        Some(snapshot) if snapshot.area == source.area => {
            snapshot.content.clone_from(&source.content);
        }
        Some(snapshot) => {
            snapshot.resize(source.area);
            snapshot.content.clone_from(&source.content);
        }
        None => *slot = Some(source.clone()),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::{Buffer, Cell};
    use ratatui::style::{Color, Style};

    use crate::backend::ratatui_backend::common::push_render_screen_background;

    use super::{apply_rendered_scroll, apply_terminal_scroll};

    /// Model one incremental scroll step over an empty surface and return the
    /// cells the diff would push to the backend.
    fn exposed_row_updates(screen_bg: Option<Style>) -> Vec<(u16, u16, Cell)> {
        let area = ratatui::layout::Rect::new(0, 0, 4, 4);
        let scroll_rows = 0..4;
        let exposed = 3..4;
        let _scope = push_render_screen_background(screen_bg);

        let mut rendered = Buffer::empty(area);
        apply_rendered_scroll(&mut rendered, &scroll_rows, 1, &exposed);

        let mut shown_by_terminal = Buffer::empty(area);
        apply_terminal_scroll(&mut shown_by_terminal, &scroll_rows, 1, &exposed);

        shown_by_terminal
            .diff(&rendered)
            .into_iter()
            .map(|(x, y, cell)| (x, y, cell.clone()))
            .collect()
    }

    #[test]
    fn exposed_rows_repaint_the_screen_background_the_terminal_did_not_fill() {
        let bg = Color::Rgb(20, 20, 20);
        let updates = exposed_row_updates(Some(Style::new().bg(bg)));

        // Background-color erase fills the exposed rows with the terminal
        // default, so every cell of the band has to be written back — otherwise
        // blank cells keep the host background and read as holes in the surface.
        assert_eq!(updates.len(), 4, "every exposed cell must be repainted");
        assert!(updates.iter().all(|(_, y, cell)| *y == 3 && cell.bg == bg));
    }

    #[test]
    fn exposed_rows_stay_cheap_without_a_screen_background() {
        assert!(
            exposed_row_updates(None).is_empty(),
            "a transparent surface already matches what the terminal scrolled in"
        );
    }
}
