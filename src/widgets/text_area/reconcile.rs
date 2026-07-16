use crate::core::node::{
    NodeId, NodeKind, NodeTree, ScrollbarZonesParams, compute_scrollbar_zones,
};
use crate::style::{LayoutConstraints, Rect, ScrollbarVariant};
use crate::utils::text::SentinelInfo;
use crate::widgets::ScrollEvent;
use crate::widgets::scroll::SmoothScrollState;
use crate::widgets::text_area::TextArea;

use super::layout::{
    TextAreaGeometry, TextAreaVisualCache, calculate_text_area_visual_metrics, logical_line_count,
    measure_text_area, text_area_auto_height_for_width, text_area_cursor_reserve,
    text_area_pending_vim_search_row,
};

pub fn reconcile_text_area(
    tree: &mut NodeTree,
    id: NodeId,
    text_area: &TextArea,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    let normalized_cursor = crate::utils::text::clamp_cursor(&text_area.value, text_area.cursor);
    let normalized_anchor = text_area
        .anchor
        .map(|anchor| crate::utils::text::clamp_cursor(&text_area.value, anchor));
    let normalized_text_area;
    let text_area =
        if normalized_cursor != text_area.cursor || normalized_anchor != text_area.anchor {
            normalized_text_area = {
                let mut text_area = text_area.clone();
                text_area.cursor = normalized_cursor;
                text_area.anchor = normalized_anchor;
                text_area
            };
            &normalized_text_area
        } else {
            text_area
        };

    let value_hash = super::layout::hash_text(text_area.value.as_ref());
    let (w, _) = measure_text_area(text_area);
    let parent_id = tree.node(id).parent;

    let parent_h_edge = tree.parent_frame_integrated_h_edge(id).unwrap_or(false);
    let parent_border_x = tree.ancestor_frame_integrated_vscrollbar_x(parent_id);
    let parent_border_y = tree.ancestor_frame_integrated_hscrollbar_y(parent_id);
    let pending_vim_search_row =
        text_area_pending_vim_search_row(text_area, Some(&tree.node(id).kind));

    let mut rect = rect;
    let available_h = rect.h;
    let avail_w = rect.w;

    if matches!(text_area.width, crate::style::Length::Auto) {
        rect.w = w.min(rect.w);
    }
    rect.w = constraints.clamp_width(rect.w, avail_w);

    let height_is_auto = matches!(text_area.height, crate::style::Length::Auto);
    if height_is_auto {
        rect.h = text_area_auto_height_for_width(
            text_area,
            rect.w,
            pending_vim_search_row,
            parent_h_edge,
        );
        rect.h = rect.h.min(available_h);
    }
    rect.h = constraints.clamp_height(rect.h, available_h);

    let mut pending_scroll_event = None;
    {
        let node = tree.node_mut(id);

        let (
            had_existing_node,
            old_offset,
            old_cursor,
            old_value,
            old_override,
            old_on_scroll,
            old_on_scroll_to,
            old_h_override,
            old_h_scroll_offset,
            old_visual_cache,
            old_color_cache,
            old_vim_mode,
            old_vim_visual_line_caret,
            old_vim_search_feedback,
            old_vim_yank_feedback_range,
            old_smooth_scroll,
            old_cancelled_scroll_to_line,
        ) = if let NodeKind::TextArea(node) = &node.kind {
            (
                true,
                node.scroll_offset,
                node.cursor,
                node.value.clone(),
                node.scroll_override,
                node.on_scroll.clone(),
                node.on_scroll_to.clone(),
                node.h_scroll_override,
                node.h_scroll_offset,
                node.visual_cache.clone(),
                node.color_cache.clone(),
                node.vim_mode,
                node.vim_visual_line_caret,
                node.vim_search_feedback.clone(),
                node.vim_yank_feedback_range,
                node.smooth_scroll.clone(),
                node.cancelled_scroll_to_line,
            )
        } else {
            (
                false,
                0,
                0,
                std::sync::Arc::from(""),
                None,
                None,
                None,
                None,
                0,
                TextAreaVisualCache::default(),
                super::TextAreaColorCache::default(),
                crate::widgets::TextAreaVimMode::Normal,
                None,
                None,
                None,
                SmoothScrollState::default(),
                None,
            )
        };

        let mut current_override = old_override;
        let mut current_h_override = old_h_override;
        let mut smooth_scroll = old_smooth_scroll;

        if text_area.cursor != old_cursor || text_area.value != old_value {
            current_override = None;
            current_h_override = None;
            smooth_scroll.cancel_at(old_offset);
        }

        #[cfg(feature = "diff-view")]
        let scroll_anchor_source_line = if text_area.diff_context_separator_click.is_some()
            && text_area.value != old_value
            && text_area.scroll_offset.is_none()
            && text_area.scroll_to_line.is_none()
        {
            old_visual_cache.latest_lines().and_then(|lines| {
                lines
                    .get(old_offset)
                    .map(|line| line.line_num.saturating_sub(1))
            })
        } else {
            None
        };
        #[cfg(not(feature = "diff-view"))]
        let scroll_anchor_source_line: Option<usize> = None;

        let mut next_cancelled_scroll_to_line = if text_area.scroll_to_line.is_some()
            && text_area.scroll_to_line == old_cancelled_scroll_to_line
        {
            old_cancelled_scroll_to_line
        } else {
            None
        };

        let controlled_scroll_offset = text_area.scroll_offset;
        if let Some(forced) = controlled_scroll_offset {
            current_override = Some(forced);
            smooth_scroll.cancel_at(forced);
            next_cancelled_scroll_to_line = text_area.scroll_to_line;
        }

        let inner = rect.inner(text_area.border, text_area.padding);
        let mut visual_cache = old_visual_cache;
        let color_cache = old_color_cache;

        let h_scrollbar_over_border = text_area.h_scrollbar
            && matches!(text_area.h_scrollbar_variant, ScrollbarVariant::Integrated)
            && (text_area.border || parent_h_edge);
        let is_v_standalone = text_area.scrollbar
            && !matches!(
                text_area.scrollbar_config.variant,
                ScrollbarVariant::Integrated
            );

        let mut visible_lines = inner.h as usize;
        let mut h_visible = false;
        let mut actual_v_scrollbar = text_area.scrollbar;
        let mut geometry = TextAreaGeometry::default();

        let mut reserve_v_scrollbar_col = false;
        for _ in 0..3 {
            let mut pass_geometry = calculate_text_area_visual_metrics(
                text_area,
                inner.w,
                !reserve_v_scrollbar_col,
                value_hash,
                Some(&mut visual_cache),
            );

            let next_h_visible = !pending_vim_search_row
                && text_area.h_scrollbar
                && !text_area.wrap
                && pass_geometry.max_line_width > pass_geometry.content_width;
            let next_visible_lines = (inner.h as usize)
                .saturating_sub(usize::from(pending_vim_search_row))
                .saturating_sub(usize::from(next_h_visible && !h_scrollbar_over_border));
            let next_v_visible =
                text_area.scrollbar && pass_geometry.total_visual_lines > next_visible_lines;
            let next_reserve_v_scrollbar_col = is_v_standalone && next_v_visible;

            pass_geometry.inner_h = inner.h;
            pass_geometry.viewport_height = inner
                .h
                .saturating_sub(u16::from(pending_vim_search_row))
                .saturating_sub(u16::from(next_h_visible && !h_scrollbar_over_border));
            pass_geometry.h_scrollbar_visible = next_h_visible;
            pass_geometry.v_scrollbar_visible = next_v_visible;
            geometry = pass_geometry;

            h_visible = next_h_visible;
            visible_lines = next_visible_lines;
            actual_v_scrollbar = if is_v_standalone {
                next_v_visible
            } else {
                text_area.scrollbar
            };

            if next_reserve_v_scrollbar_col == reserve_v_scrollbar_col {
                break;
            }
            reserve_v_scrollbar_col = next_reserve_v_scrollbar_col;
        }

        let line_target_suppressed = text_area.scroll_to_line.is_some()
            && text_area.scroll_to_line == next_cancelled_scroll_to_line;
        let explicit_target = if controlled_scroll_offset.is_some() || line_target_suppressed {
            None
        } else {
            text_area.scroll_to_line.map(|line| {
                scroll_to_logical_line_offset(
                    line,
                    &visual_cache,
                    geometry.total_visual_lines,
                    visible_lines,
                )
            })
        };

        let new_offset = if visible_lines == 0 || geometry.total_visual_lines == 0 {
            smooth_scroll.cancel_at(0);
            0
        } else if let Some(forced) = controlled_scroll_offset {
            smooth_scroll.cancel_at(forced);
            forced
        } else if let Some(target) = explicit_target {
            smooth_scroll.resolve_target(
                old_offset,
                target,
                geometry.total_visual_lines.saturating_sub(visible_lines),
                text_area.scroll_behavior,
            )
        } else if let Some(forced) = current_override {
            smooth_scroll.cancel_at(forced);
            forced
        } else {
            smooth_scroll.cancel_at(old_offset);
            crate::widgets::scroll::smart_list_offset(
                old_offset,
                geometry.cursor_visual_line,
                geometry.total_visual_lines,
                visible_lines as u16,
            )
        };

        let max_offset = geometry.total_visual_lines.saturating_sub(visible_lines);
        let mut new_offset = new_offset.min(max_offset);
        if let Some(anchor_source_line) = scroll_anchor_source_line
            && let Some(anchored) = visual_cache.latest_lines().and_then(|lines| {
                lines
                    .iter()
                    .position(|line| line.line_num.saturating_sub(1) == anchor_source_line)
            })
        {
            new_offset = anchored.min(max_offset);
            smooth_scroll.cancel_at(new_offset);
        }

        let next_scroll_override = if explicit_target.is_some() || current_override.is_some() {
            Some(new_offset)
        } else {
            None
        };

        if had_existing_node
            && current_override.is_none()
            && explicit_target.is_none()
            && new_offset != old_offset
        {
            let metrics = crate::widgets::ScrollMetrics {
                len: geometry.total_visual_lines,
                visible: visible_lines.min(geometry.total_visual_lines),
                max_offset,
            };
            pending_scroll_event = Some((new_offset, metrics, old_on_scroll_to, old_on_scroll));
        }

        let mut h_scroll_offset = if !h_visible || geometry.content_width == 0 {
            0
        } else if let Some(forced) = current_h_override {
            forced
        } else {
            smart_text_area_h_scroll_offset(
                &text_area.value,
                text_area.cursor,
                geometry.content_width,
                old_h_scroll_offset,
                text_area.sentinel_info().as_ref(),
                text_area.tab_stop as usize,
                &text_area.virtual_texts,
            )
        };
        let max_h_offset = geometry
            .max_line_width
            .saturating_sub(geometry.content_width);
        h_scroll_offset = h_scroll_offset.min(max_h_offset);

        let next_h_override = if current_h_override.is_some() {
            Some(h_scroll_offset)
        } else {
            None
        };

        node.rect = rect;
        node.children.clear();
        node.kind = NodeKind::from(text_area.clone());

        if let NodeKind::TextArea(node) = &mut node.kind {
            let content_x = inner.x.saturating_add(geometry.gutter_width as i16);
            let content_width = inner
                .w
                .saturating_sub(geometry.gutter_width as u16)
                .saturating_sub(text_area_cursor_reserve(node.wrap, node.read_only));
            geometry.scrollbar_zones = compute_scrollbar_zones(ScrollbarZonesParams {
                id,
                rect,
                inner,
                border: node.border,
                scrollbar: actual_v_scrollbar,
                scrollbar_variant: node.scrollbar_variant,
                scrollbar_gap: node.scrollbar_gap,
                h_scrollbar: h_visible,
                h_scrollbar_variant: node.h_scrollbar_variant,
                content_x,
                content_width,
                max_content_width: geometry.max_line_width,
                wrap: node.wrap,
                parent_border_x,
                parent_border_y,
            });
            node.scroll_offset = new_offset;
            node.scroll_to_line = text_area.scroll_to_line;
            node.cancelled_scroll_to_line = next_cancelled_scroll_to_line;
            node.scroll_behavior = text_area.scroll_behavior;
            node.smooth_scroll = smooth_scroll;
            node.visual_lines_count = geometry.total_visual_lines;
            node.logical_lines_count = logical_line_count(node.value.as_ref());
            node.max_line_width = geometry.max_line_width;
            node.h_scroll_offset = h_scroll_offset;
            node.h_scroll_override = next_h_override;
            node.scroll_override = next_scroll_override;
            node.scrollbar = actual_v_scrollbar;
            node.h_scrollbar = h_visible;
            node.content_hash = value_hash;
            node.visual_cache = visual_cache;
            node.color_cache = color_cache;
            node.geometry = geometry;
            if text_area.vim_motions && !text_area.read_only && text_area.on_change.is_some() {
                node.vim_mode = old_vim_mode;
                node.vim_visual_line_caret = old_vim_visual_line_caret;
                node.vim_search_feedback = old_vim_search_feedback;
                node.vim_yank_feedback_range = old_vim_yank_feedback_range;
            } else {
                node.vim_mode = crate::widgets::TextAreaVimMode::Normal;
                node.vim_visual_line_caret = None;
                node.vim_search_feedback = None;
                node.vim_yank_feedback_range = None;
            }
            node.color_cache.update(
                node.color_strategy.as_ref(),
                node.value.as_ref(),
                value_hash,
                node.language.as_deref(),
                node.theme.as_deref(),
            );
        }
    }

    if let Some((offset, metrics, on_scroll_to, on_scroll)) = pending_scroll_event {
        if let Some(cb) = on_scroll_to.as_ref() {
            cb.emit(offset);
        } else if let Some(cb) = on_scroll.as_ref() {
            cb.emit(ScrollEvent { offset, metrics });
        }
    }

    tree.register_scrollbar_zone(id);

    id
}

fn smart_text_area_h_scroll_offset(
    value: &str,
    cursor: usize,
    content_width: usize,
    old_offset: usize,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
    virtual_texts: &[crate::widgets::TextAreaVirtualText],
) -> usize {
    if content_width == 0 {
        return 0;
    }

    let cursor = crate::utils::text::clamp_cursor(value, cursor);

    let mut line_start = 0usize;
    for line in value.split('\n') {
        let line_len = line.len();
        let line_end = line_start.saturating_add(line_len);
        if cursor >= line_start && cursor <= line_end {
            let cursor_in_line = cursor.saturating_sub(line_start);
            let insertions = crate::widgets::inline_virtual_insertions_for_line(
                value,
                virtual_texts,
                line_start,
                line_end,
            );
            let cursor_col = crate::utils::text::visual_col_with_virtual(
                &line[..cursor_in_line],
                0,
                tab_stop,
                sentinel,
                &insertions,
            );

            let visible_start = old_offset;
            let visible_end = old_offset + content_width.saturating_sub(1);

            if cursor_col >= visible_start && cursor_col <= visible_end {
                return old_offset;
            }

            if cursor_col < visible_start {
                return cursor_col;
            } else {
                return cursor_col.saturating_sub(content_width.saturating_sub(1));
            }
        }
        line_start = line_start.saturating_add(line_len).saturating_add(1);
    }

    0
}

fn scroll_to_logical_line_offset(
    target_line: usize,
    visual_cache: &TextAreaVisualCache,
    total_visual_lines: usize,
    visible_lines: usize,
) -> usize {
    let max_offset = total_visual_lines.saturating_sub(visible_lines);
    let target_one_based = target_line.saturating_add(1);

    // `TextAreaVisualLine::line_num` is currently one-based. Choose the first
    // visual row whose source line is at or after the requested zero-based
    // logical line. If the requested line no longer exists (or cache rows are
    // unavailable), fall back to the last available visual row and let the
    // viewport clamp to `max_offset` so out-of-range targets remain safe.
    let Some(lines) = visual_cache.latest_lines() else {
        return max_offset;
    };
    if lines.is_empty() {
        return 0;
    }

    lines
        .iter()
        .position(|line| line.line_num >= target_one_based)
        .unwrap_or_else(|| lines.len().saturating_sub(1))
        .min(max_offset)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use crate::animation::{Easing, TransitionConfig};
    use crate::callback::Callback;
    use crate::core::element::Element;
    use crate::core::node::{NodeKind, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::style::{Length, Rect, Span};
    use crate::widgets::internal::ScrollAction;
    use crate::widgets::{
        ScrollBehavior, ScrollEvent, TextArea, TextAreaGutter, TextAreaGutterSign, VStack,
    };

    #[test]
    fn reconcile_clamps_text_area_cursor_inside_unicode_for_h_scroll() {
        let value = "części ewaluacyjnej|późniejszej analizie|opisan[ayąe]* w @chapter|w @chapter";
        let cursor_inside_s = 5;
        assert!(!value.is_char_boundary(cursor_inside_s));

        let root: Element = TextArea::new(value)
            .cursor(cursor_inside_s)
            .anchor(Some(cursor_inside_s))
            .width(Length::Px(12))
            .height(Length::Px(2))
            .wrap(false)
            .scrollbar(false)
            .h_scrollbar(true)
            .border(false)
            .into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 12,
                h: 3,
            },
            None,
        );

        let NodeKind::TextArea(node) = &tree.node(tree.root).kind else {
            panic!("expected text area root");
        };
        assert_eq!(node.cursor, 4);
        assert_eq!(node.anchor, Some(4));
        assert!(value.is_char_boundary(node.cursor));
    }

    #[test]
    fn gutter_builder_clamps_cursor_inside_unicode_before_slicing() {
        let value = "części ewaluacyjnej";
        let cursor_inside_s = 5;
        assert!(!value.is_char_boundary(cursor_inside_s));

        let text_area = TextArea::new(value).cursor(cursor_inside_s).gutter(
            TextAreaGutter::new().signs([TextAreaGutterSign::new(0, vec![Span::new(">")])]),
        );

        assert_eq!(text_area.cursor, cursor_inside_s);
        assert!(text_area.gutter_lines.is_some());
    }

    #[test]
    fn auto_height_includes_standalone_horizontal_scrollbar_row() {
        let root: Element = VStack::new()
            .width(Length::Auto)
            .height(Length::Auto)
            .child(
                TextArea::new("123456789\nabc")
                    .width(Length::Px(5))
                    .height(Length::Auto)
                    .wrap(false)
                    .scrollbar(false)
                    .h_scrollbar(true)
                    .border(false),
            )
            .into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 20,
            },
            None,
        );

        let child_id = tree.node(tree.root).children[0];
        let NodeKind::TextArea(node) = &tree.node(child_id).kind else {
            panic!("expected text area root");
        };

        assert_eq!(tree.node(child_id).rect.h, 3);
        assert!(node.h_scrollbar);
    }

    #[test]
    fn h_scrollbar_can_appear_only_after_vertical_scrollbar_reserves_width() {
        let value = [
            "    let items = vec![\"Rust\", \"TUI\", \"Lipan\"];".to_string(),
            "line 2".to_string(),
            "line 3".to_string(),
            "line 4".to_string(),
            "line 5".to_string(),
            "line 6".to_string(),
            "line 7".to_string(),
            "line 8".to_string(),
            "line 9".to_string(),
            "line 10".to_string(),
            "line 11".to_string(),
            "line 12".to_string(),
            "line 13".to_string(),
            "line 14".to_string(),
            "line 15".to_string(),
            "line 16".to_string(),
            "line 17".to_string(),
            "line 18".to_string(),
            "line 19".to_string(),
            "line 20".to_string(),
        ]
        .join("\n");

        let root: Element = VStack::new()
            .width(Length::Auto)
            .height(Length::Auto)
            .child(
                TextArea::new(value)
                    .width(Length::Px(51))
                    .height(Length::Px(10))
                    .wrap(false)
                    .read_only(true)
                    .scrollbar(true)
                    .h_scrollbar(true)
                    .border(false)
                    .line_numbers(false)
                    .gutter_lines(Arc::new(vec![vec![]; 20]), 6),
            )
            .into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 20,
            },
            None,
        );

        let child_id = tree.node(tree.root).children[0];
        let NodeKind::TextArea(node) = &tree.node(child_id).kind else {
            panic!("expected text area root");
        };

        assert!(node.scrollbar);
        assert!(node.h_scrollbar);
        assert_eq!(node.max_line_width, 45);
        assert_eq!(node.h_scroll_offset, 0);
    }

    #[test]
    fn cursor_auto_scroll_emits_scroll_event() {
        let emitted = Rc::new(RefCell::new(Vec::<ScrollEvent>::new()));
        let on_scroll = {
            let emitted = Rc::clone(&emitted);
            Callback::new(move |event| emitted.borrow_mut().push(event))
        };

        let root: Element = TextArea::new("one\ntwo\nthree")
            .cursor("one\ntwo\nthree".len())
            .height(Length::Px(3))
            .border(false)
            .on_scroll(on_scroll.clone())
            .into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 3,
            },
            None,
        );
        assert!(emitted.borrow().is_empty());

        let next = "one\ntwo\nthree\nfour";
        let root: Element = TextArea::new(next)
            .cursor(next.len())
            .height(Length::Px(3))
            .border(false)
            .on_scroll(on_scroll)
            .into();

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 3,
            },
            None,
        );

        let emitted = emitted.borrow();
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].offset, 1);
        assert_eq!(emitted[0].metrics.max_offset, 1);
    }

    #[test]
    fn scroll_to_line_maps_logical_line_to_first_wrapped_visual_row() {
        let root: Element = TextArea::new("abcdefghij\nkl")
            .width(Length::Px(5))
            .height(Length::Px(1))
            .border(false)
            .read_only(true)
            .scrollbar(false)
            .scroll_to_line(1)
            .into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 5,
                h: 1,
            },
            None,
        );

        let NodeKind::TextArea(node) = &tree.node(tree.root).kind else {
            panic!("expected text area root");
        };

        assert_eq!(node.scroll_offset, 2);
    }

    #[test]
    fn scroll_to_line_defaults_to_instant_snap() {
        let root: Element = TextArea::new("0\n1\n2\n3\n4")
            .height(Length::Px(2))
            .border(false)
            .read_only(true)
            .scroll_to_line(3)
            .into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 2,
            },
            None,
        );

        let NodeKind::TextArea(node) = &tree.node(tree.root).kind else {
            panic!("expected text area root");
        };

        assert_eq!(node.scroll_offset, 3);
        assert!(!node.smooth_scroll.is_animating());
    }

    #[test]
    fn scroll_to_line_does_not_emit_scroll_callbacks() {
        let scroll_events = Rc::new(RefCell::new(Vec::<ScrollEvent>::new()));
        let on_scroll = {
            let scroll_events = Rc::clone(&scroll_events);
            Callback::new(move |event| scroll_events.borrow_mut().push(event))
        };

        let mut tree = NodeTree::new();
        let rect = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 2,
        };

        let root: Element = TextArea::new("0\n1\n2\n3\n4")
            .height(Length::Px(2))
            .border(false)
            .read_only(true)
            .on_scroll(on_scroll.clone())
            .into();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, rect, None);

        let target: Element = TextArea::new("0\n1\n2\n3\n4")
            .height(Length::Px(2))
            .border(false)
            .read_only(true)
            .scroll_to_line(3)
            .on_scroll(on_scroll)
            .into();
        LayoutEngine::reconcile_with_focus(&mut tree, &target, rect, None);
        assert!(scroll_events.borrow().is_empty());

        let scroll_to_events = Rc::new(RefCell::new(Vec::<usize>::new()));
        let on_scroll_to = {
            let scroll_to_events = Rc::clone(&scroll_to_events);
            Callback::new(move |offset| scroll_to_events.borrow_mut().push(offset))
        };

        let mut tree = NodeTree::new();
        let root: Element = TextArea::new("0\n1\n2\n3\n4")
            .height(Length::Px(2))
            .border(false)
            .read_only(true)
            .on_scroll_to(on_scroll_to.clone())
            .into();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, rect, None);

        let target: Element = TextArea::new("0\n1\n2\n3\n4")
            .height(Length::Px(2))
            .border(false)
            .read_only(true)
            .scroll_to_line(3)
            .scroll_behavior(ScrollBehavior::smooth(TransitionConfig {
                duration: Duration::from_millis(100),
                easing: Easing::Linear,
            }))
            .on_scroll_to(on_scroll_to)
            .into();
        LayoutEngine::reconcile_with_focus(&mut tree, &target, rect, None);
        assert!(scroll_to_events.borrow().is_empty());
    }

    #[test]
    fn smooth_scroll_to_line_starts_at_current_offset_and_advances() {
        let root: Element = TextArea::new("0\n1\n2\n3\n4\n5\n6\n7")
            .height(Length::Px(2))
            .border(false)
            .read_only(true)
            .scroll_to_line(5)
            .scroll_behavior(ScrollBehavior::smooth(TransitionConfig {
                duration: Duration::from_millis(100),
                easing: Easing::Linear,
            }))
            .into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 2,
            },
            None,
        );

        let root_id = tree.root;
        let NodeKind::TextArea(node) = &mut tree.node_mut(root_id).kind else {
            panic!("expected text area root");
        };

        assert_eq!(node.scroll_offset, 0);
        assert!(node.smooth_scroll.is_animating());
        let tick = node.smooth_scroll.tick(Duration::from_millis(50), 6);
        assert!(tick.changed);
        assert_eq!(node.smooth_scroll.current_offset(6), 3);
    }

    #[test]
    fn controlled_scroll_offset_is_instant_and_cancels_smooth_target() {
        let smooth_root: Element = TextArea::new("0\n1\n2\n3\n4\n5\n6\n7")
            .height(Length::Px(2))
            .border(false)
            .read_only(true)
            .scroll_to_line(6)
            .scroll_transition(TransitionConfig {
                duration: Duration::from_millis(100),
                easing: Easing::Linear,
            })
            .into();

        let mut tree = NodeTree::new();
        let rect = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 2,
        };
        LayoutEngine::reconcile_with_focus(&mut tree, &smooth_root, rect, None);

        let controlled: Element = TextArea::new("0\n1\n2\n3\n4\n5\n6\n7")
            .height(Length::Px(2))
            .border(false)
            .read_only(true)
            .scroll_to_line(6)
            .scroll_offset(4)
            .into();
        LayoutEngine::reconcile_with_focus(&mut tree, &controlled, rect, None);

        let NodeKind::TextArea(node) = &tree.node(tree.root).kind else {
            panic!("expected text area root");
        };

        assert_eq!(node.scroll_offset, 4);
        assert_eq!(node.scroll_override, Some(4));
        assert_eq!(node.cancelled_scroll_to_line, Some(6));
        assert!(!node.smooth_scroll.is_animating());
    }

    #[test]
    fn horizontal_wheel_cancels_same_smooth_line_target() {
        let value = ["01234567890123456789", "1", "2", "3", "4", "5", "6", "7"].join("\n");
        let root: Element = TextArea::new(value)
            .width(Length::Px(5))
            .height(Length::Px(2))
            .wrap(false)
            .h_scrollbar(true)
            .border(false)
            .read_only(true)
            .scroll_to_line(6)
            .scroll_transition(TransitionConfig {
                duration: Duration::from_millis(100),
                easing: Easing::Linear,
            })
            .into();
        let rect = Rect {
            x: 0,
            y: 0,
            w: 5,
            h: 2,
        };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, rect, None);
        let root_id = tree.root;
        let NodeKind::TextArea(node) = &tree.node(root_id).kind else {
            panic!("expected text area root");
        };
        assert!(node.smooth_scroll.is_animating());
        assert_eq!(node.h_scroll_offset, 0);

        assert!(crate::app::input::handlers::text_area::handle_scroll(
            &mut tree,
            root_id,
            ScrollAction::LineRight(1),
            false,
        ));

        LayoutEngine::reconcile_with_focus(&mut tree, &root, rect, None);

        let NodeKind::TextArea(node) = &tree.node(root_id).kind else {
            panic!("expected text area root");
        };
        assert_eq!(node.h_scroll_offset, 1);
        assert_eq!(node.cancelled_scroll_to_line, Some(6));
        assert!(!node.smooth_scroll.is_animating());
    }

    #[test]
    fn user_scroll_suppresses_same_smooth_line_target() {
        let root: Element = TextArea::new("0\n1\n2\n3\n4\n5\n6\n7")
            .height(Length::Px(2))
            .border(false)
            .read_only(true)
            .scroll_to_line(6)
            .scroll_transition(TransitionConfig {
                duration: Duration::from_millis(100),
                easing: Easing::Linear,
            })
            .into();
        let rect = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 2,
        };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, rect, None);
        let root_id = tree.root;
        let NodeKind::TextArea(node) = &tree.node(root_id).kind else {
            panic!("expected text area root");
        };
        assert!(node.smooth_scroll.is_animating());

        assert!(crate::app::input::handlers::text_area::handle_scroll(
            &mut tree,
            root_id,
            ScrollAction::LineDown(1),
            false,
        ));

        LayoutEngine::reconcile_with_focus(&mut tree, &root, rect, None);

        let NodeKind::TextArea(node) = &tree.node(root_id).kind else {
            panic!("expected text area root");
        };
        assert_eq!(node.scroll_offset, 1);
        assert!(!node.smooth_scroll.is_animating());
    }
}
