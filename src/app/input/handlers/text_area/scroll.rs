use super::*;

// ── Scroll-wheel handler ────────────────────────────────────────────────────

/// Handle scroll-wheel events for a TextArea node.
///
/// `parent_integrated_v_edge` is pre-computed by the scroll dispatch loop so that
/// the integrated-scrollbar width calculation does not require re-borrowing
/// the tree.
pub(crate) fn handle_scroll(
    tree: &mut NodeTree,
    id: NodeId,
    action: crate::widgets::internal::ScrollAction,
    parent_integrated_v_edge: bool,
) -> bool {
    let node = tree.node_mut(id);
    let rect = node.rect;
    let NodeKind::TextArea(ta) = &mut node.kind else {
        return false;
    };

    if !ta.scroll_wheel || ta.disabled {
        return false;
    }

    let inner = rect.inner(ta.border, ta.padding);

    match action {
        crate::widgets::internal::ScrollAction::LineUp(_)
        | crate::widgets::internal::ScrollAction::LineDown(_)
        | crate::widgets::internal::ScrollAction::Home
        | crate::widgets::internal::ScrollAction::End => {
            let visible = ta.geometry.content_viewport_h(false) as usize;
            let metrics = scroll_metrics(ta.visual_lines_count, visible, ta.scroll_offset);
            let next = apply_scroll_action(ta.scroll_offset, metrics, action);
            let line_target_cancel_pending =
                ta.scroll_to_line.is_some() && ta.cancelled_scroll_to_line != ta.scroll_to_line;
            let offset_changed = next != ta.scroll_offset;

            if offset_changed || line_target_cancel_pending {
                ta.scroll_offset = next;
                ta.scroll_override = Some(next);
                ta.smooth_scroll.cancel_at(next);
                ta.cancelled_scroll_to_line = ta.scroll_to_line;

                if offset_changed && let Some(cb) = ta.on_scroll_to.as_ref() {
                    cb.emit(next);
                } else if offset_changed && let Some(cb) = ta.on_scroll.as_ref() {
                    cb.emit(ScrollEvent {
                        offset: next,
                        metrics,
                    });
                }
                if offset_changed {
                    emit_scroll_editor_state_change(ta);
                }
                true
            } else {
                false
            }
        }
        crate::widgets::internal::ScrollAction::LineLeft(_)
        | crate::widgets::internal::ScrollAction::LineRight(_) => {
            // Handle horizontal scroll
            let gutter_width = text_area_total_gutter_width(
                ta.logical_lines_count,
                ta.line_numbers,
                ta.min_line_number_width,
                ta.gutter_col_width,
                ta.gutter_gap,
            ) as usize;

            let v_scrollbar_over_border = ta.scrollbar
                && matches!(ta.scrollbar_variant, ScrollbarVariant::Integrated)
                && (ta.border || parent_integrated_v_edge);
            let scrollbar_cols = if ta.scrollbar && !v_scrollbar_over_border {
                1u16.saturating_add(ta.scrollbar_gap)
            } else {
                0
            };

            let content_width = inner
                .w
                .saturating_sub(gutter_width as u16)
                .saturating_sub(scrollbar_cols)
                .saturating_sub(text_area_cursor_reserve(ta.wrap, ta.read_only))
                as usize;

            if content_width > 0 {
                let metrics = scroll_metrics(ta.max_line_width, content_width, ta.h_scroll_offset);
                let next = apply_scroll_action(ta.h_scroll_offset, metrics, action);
                let line_target_cancel_pending =
                    ta.scroll_to_line.is_some() && ta.cancelled_scroll_to_line != ta.scroll_to_line;

                if next != ta.h_scroll_offset || line_target_cancel_pending {
                    if next != ta.h_scroll_offset {
                        ta.h_scroll_offset = next;
                        ta.h_scroll_override = Some(next);
                        emit_scroll_editor_state_change(ta);
                    }
                    ta.smooth_scroll.cancel_at(ta.scroll_offset);
                    ta.cancelled_scroll_to_line = ta.scroll_to_line;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
    }
}

// ── Private helpers ─────────────────────────────────────────────────────────

fn emit_scroll_editor_state_change(ta: &crate::widgets::internal::TextAreaNode) {
    let Some(cb) = ta.on_editor_state_change.as_ref() else {
        return;
    };
    cb.emit(crate::widgets::TextAreaStateChangeEvent {
        reason: crate::widgets::TextAreaStateChangeReason::Scroll,
        value: ta.value.clone(),
        cursor: ta.cursor,
        anchor: ta.anchor,
        edit: None,
        vim_mode: None,
    });
}

pub(super) fn cancel_text_area_smooth_scroll(tree: &mut NodeTree, id: NodeId) {
    if let NodeKind::TextArea(ta) = &mut tree.node_mut(id).kind {
        ta.smooth_scroll.cancel_at(ta.scroll_offset);
        ta.cancelled_scroll_to_line = ta.scroll_to_line;
    }
}
