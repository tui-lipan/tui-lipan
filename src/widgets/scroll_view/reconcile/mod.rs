use std::collections::HashMap;
use std::sync::atomic::Ordering;

use crate::core::element::Key;
use crate::core::node::{NodeId, NodeKind};
use crate::layout::reconcile::ReconcileCtx;
use crate::layout::stack::VIRTUAL_THRESHOLD;
use crate::style::{Rect, ScrollbarVariant};
use crate::widgets::internal::ScrollViewNode;
use crate::widgets::scroll::{
    KineticScrollState, SmoothScrollState, apply_scroll_request, scroll_metrics,
};
use crate::widgets::scroll_view::node::OffscreenDocSelection;
use crate::widgets::scroll_view::utils::calc_scroll_view_window;
use crate::widgets::{
    ScrollEvent, ScrollMetrics, ScrollRequest, ScrollTarget, ScrollView, ScrollViewLayoutCache,
    ScrollWheelBehavior, VirtualHeightCache,
};

mod anchor;
mod cache;
mod snapshot;

pub(crate) use anchor::*;
pub(crate) use cache::*;
pub(crate) use snapshot::*;

pub(crate) struct ScrollViewReconcile<'a> {
    pub id: NodeId,
    pub sv: &'a ScrollView,
    pub scroll_key: Option<Key>,
    pub rect: Rect,
}

pub(crate) fn reconcile_scroll_view(
    ctx: &mut ReconcileCtx<'_>,
    args: ScrollViewReconcile<'_>,
) -> NodeId {
    let ScrollViewReconcile {
        id,
        sv,
        scroll_key,
        rect,
    } = args;
    let ReconcileCtx {
        tree,
        epoch,
        focus,
        overlay_state,
    } = ctx;
    let epoch = *epoch;
    let focus = *focus;
    let (
        mut layout_cache,
        mut virtual_cache,
        old_viewport_w,
        old_viewport_h,
        old_max_offset,
        old_content_height,
        mut offscreen_doc_selections,
        mut smooth_scroll,
        mut wheel_scroll,
        old_viewport_snapshot,
        old_had_viewport_callback,
    ) = if matches!(&tree.node(id).kind, NodeKind::ScrollView(_)) {
        let node_ref = tree.node_mut(id);
        let old_rect = node_ref.rect;
        let NodeKind::ScrollView(node) = &mut node_ref.kind else {
            unreachable!();
        };
        let previous_viewport_w = if node.content_viewport_w > 0 {
            node.content_viewport_w
        } else {
            let mut w = node.virtual_cache.viewport_w;
            if w == 0 {
                w = old_rect.inner(node.props.border, node.props.padding).w;
                let old_use_standalone = node.scrollbar
                    && matches!(node.scrollbar_variant, ScrollbarVariant::Standalone);
                if old_use_standalone {
                    w = w.saturating_sub(1u16.saturating_add(node.scrollbar_gap));
                }
            }
            w
        };
        (
            node.layout_cache.clone(),
            node.virtual_cache.clone(),
            previous_viewport_w,
            node.viewport_height,
            node.max_offset,
            node.content_height,
            std::mem::take(&mut node.offscreen_doc_selections),
            node.smooth_scroll.clone(),
            node.wheel_scroll.clone(),
            if node.on_viewport_change.is_some() {
                node.viewport_snapshot.clone()
            } else {
                None
            },
            node.on_viewport_change.is_some(),
        )
    } else {
        (
            ScrollViewLayoutCache::default(),
            VirtualHeightCache::default(),
            0u16,
            0u16,
            0usize,
            0u16,
            std::collections::HashMap::new(),
            SmoothScrollState::default(),
            KineticScrollState::default(),
            None,
            false,
        )
    };

    let had_scroll_view_state = matches!(&tree.node(id).kind, NodeKind::ScrollView(_));
    let remembered_anchor = if had_scroll_view_state {
        None
    } else {
        scroll_key
            .as_ref()
            .and_then(|key| tree.remembered_scroll_anchor_by_key.get(key).cloned())
    };

    let use_standalone = sv.scrollbar
        && (!sv.props.border
            || matches!(sv.scrollbar_config.variant, ScrollbarVariant::Standalone));
    let inner = rect.inner(sv.props.border, sv.props.padding);
    let horizontal_overflow = sv.axis.horizontal_enabled();

    let scroll_offset_hint = if let NodeKind::ScrollView(node) = &tree.node(id).kind {
        if node.smooth_scroll.is_animating() {
            node.smooth_scroll.current_offset(node.max_offset)
        } else {
            node.offset
        }
    } else {
        sv.offset.unwrap_or(0)
    };
    let estimated_child_height = sv.estimated_child_height;

    // Compute scroll anchor from OLD virtual cache state before layout
    // modifies it. This lets us correct the offset after resize so the
    // user keeps seeing the same child.
    let anchor = if had_scroll_view_state {
        if scroll_offset_hint == 0 || (old_max_offset > 0 && scroll_offset_hint >= old_max_offset) {
            compute_scroll_anchor(
                &virtual_cache,
                scroll_offset_hint,
                old_viewport_h,
                old_max_offset,
                estimated_child_height,
                sv.props.gap,
            )
        } else {
            compute_visible_scroll_anchor(tree, id, &sv.children).or_else(|| {
                compute_scroll_anchor(
                    &virtual_cache,
                    scroll_offset_hint,
                    old_viewport_h,
                    old_max_offset,
                    estimated_child_height,
                    sv.props.gap,
                )
            })
        }
    } else {
        remembered_anchor
    };

    let standalone_scrollbar_cols = 1u16.saturating_add(sv.scrollbar_config.gap);

    // Detect whether the standalone scrollbar was present on the previous frame
    // by comparing the virtual cache's stored viewport_w with the full inner.w.
    // This lets us skip an expensive probe pass at the wrong width.
    let prev_had_standalone = use_standalone && old_viewport_w > 0 && old_viewport_w < inner.w;

    let mut actual_standalone = false;
    let mut viewport_w = inner.w;
    let mut content_layout;

    if use_standalone && inner.w > 0 {
        if prev_had_standalone {
            // Scrollbar was present last frame - probe at the narrow width
            // first to avoid an unnecessary width mismatch on the virtual cache.
            let narrow = inner.w.saturating_sub(standalone_scrollbar_cols);
            let mut probe_virtual = virtual_cache.clone();
            let probe = layout_scroll_content_cached(
                &sv.props,
                &sv.children,
                &mut layout_cache,
                &mut probe_virtual,
                ScrollLayoutCachedParams {
                    viewport_w: narrow,
                    viewport_h: inner.h,
                    scroll_offset: scroll_offset_hint,
                    estimated_child_height,
                    horizontal_overflow,
                },
            );
            if probe.content_height > inner.h {
                // Scrollbar still needed - use narrow width with the REAL cache.
                actual_standalone = true;
                viewport_w = narrow;
                content_layout = layout_scroll_content_cached(
                    &sv.props,
                    &sv.children,
                    &mut layout_cache,
                    &mut virtual_cache,
                    ScrollLayoutCachedParams {
                        viewport_w: narrow,
                        viewport_h: inner.h,
                        scroll_offset: scroll_offset_hint,
                        estimated_child_height,
                        horizontal_overflow,
                    },
                );
            } else {
                // Scrollbar no longer needed - re-probe at full width.
                let mut full_probe = virtual_cache.clone();
                let full = layout_scroll_content_cached(
                    &sv.props,
                    &sv.children,
                    &mut layout_cache,
                    &mut full_probe,
                    ScrollLayoutCachedParams {
                        viewport_w: inner.w,
                        viewport_h: inner.h,
                        scroll_offset: scroll_offset_hint,
                        estimated_child_height,
                        horizontal_overflow,
                    },
                );
                if full.content_height > inner.h {
                    // Scrollbar appears at full width - need narrow pass.
                    actual_standalone = true;
                    viewport_w = narrow;
                    content_layout = layout_scroll_content_cached(
                        &sv.props,
                        &sv.children,
                        &mut layout_cache,
                        &mut virtual_cache,
                        ScrollLayoutCachedParams {
                            viewport_w: narrow,
                            viewport_h: inner.h,
                            scroll_offset: scroll_offset_hint,
                            estimated_child_height,
                            horizontal_overflow,
                        },
                    );
                } else {
                    viewport_w = inner.w;
                    virtual_cache = full_probe;
                    content_layout = full;
                }
            }
        } else {
            // No scrollbar last frame - probe at full width.
            let mut probe_virtual = virtual_cache.clone();
            let probe = layout_scroll_content_cached(
                &sv.props,
                &sv.children,
                &mut layout_cache,
                &mut probe_virtual,
                ScrollLayoutCachedParams {
                    viewport_w: inner.w,
                    viewport_h: inner.h,
                    scroll_offset: scroll_offset_hint,
                    estimated_child_height,
                    horizontal_overflow,
                },
            );
            if probe.content_height > inner.h {
                // Need scrollbar - redo at narrow width with REAL cache.
                actual_standalone = true;
                viewport_w = inner.w.saturating_sub(standalone_scrollbar_cols);
                content_layout = layout_scroll_content_cached(
                    &sv.props,
                    &sv.children,
                    &mut layout_cache,
                    &mut virtual_cache,
                    ScrollLayoutCachedParams {
                        viewport_w,
                        viewport_h: inner.h,
                        scroll_offset: scroll_offset_hint,
                        estimated_child_height,
                        horizontal_overflow,
                    },
                );
            } else {
                // No scrollbar needed. Promote the probe cache.
                virtual_cache = probe_virtual;
                content_layout = probe;
            }
        }
    } else {
        // No standalone scrollbar configured or inner.w == 0.
        content_layout = layout_scroll_content_cached(
            &sv.props,
            &sv.children,
            &mut layout_cache,
            &mut virtual_cache,
            ScrollLayoutCachedParams {
                viewport_w: inner.w,
                viewport_h: inner.h,
                scroll_offset: scroll_offset_hint,
                estimated_child_height,
                horizontal_overflow,
            },
        );
    }

    let mut content_height = content_layout.content_height;
    let viewport_height = inner.h;

    let mut layout_max_offset = calc_scroll_view_window(
        0,
        content_height as usize,
        viewport_height as usize,
        sv.show_scroll_indicators,
    )
    .max_offset;

    let (old_offset, old_override, handler_dirty, old_element_offset, old_element_scroll_request) =
        if let NodeKind::ScrollView(node) = &tree.node(id).kind {
            let displayed_offset = if node.smooth_scroll.is_animating() {
                node.smooth_scroll.current_offset(node.max_offset)
            } else {
                node.offset
            };
            (
                displayed_offset,
                node.scroll_override,
                node.scroll_handler_dirty,
                node.element_offset,
                node.element_scroll_request,
            )
        } else {
            (0, None, false, None, None)
        };
    let old_cancelled_scroll_target = if let NodeKind::ScrollView(node) = &tree.node(id).kind {
        node.cancelled_scroll_target.clone()
    } else {
        None
    };

    // When a parent mirrors `on_scroll` into controlled `.offset(...)` state, the
    // prop can "catch up" to the live node offset on a later full render (resize,
    // theme/sidebar change, etc.). That catch-up is state synchronization, not a
    // fresh external navigation request. If we treat it as external, resize-anchor
    // correction gets disabled exactly when wrapped children are reflowing.
    let live_scroll_offset = old_override.unwrap_or(old_offset);
    let element_caught_up_to_live_offset =
        sv.offset.is_some_and(|offset| offset == live_scroll_offset);

    let scroll_target_suppressed = sv.scroll_target.is_some()
        && sv.scroll_target.as_ref() == old_cancelled_scroll_target.as_ref();
    let raw_target_offset = if scroll_target_suppressed {
        None
    } else {
        sv.scroll_target.as_ref().and_then(|target| {
            scroll_offset_for_target(
                &sv.children,
                &content_layout.rects,
                target,
                layout_max_offset,
            )
        })
    };
    let input_offset = scroll_key
        .as_ref()
        .and_then(|key| tree.scroll_input_offset_by_key.get(key).copied())
        .filter(|offset| sv.offset == Some(*offset));
    let request_offset = if sv.scroll_request != old_element_scroll_request {
        sv.scroll_request.map(|request| {
            let base_offset = if had_scroll_view_state {
                live_scroll_offset
            } else {
                sv.offset.unwrap_or(0)
            };
            let metrics = scroll_metrics(
                content_height as usize,
                viewport_height as usize,
                base_offset,
            );
            apply_scroll_request(base_offset.min(metrics.max_offset), metrics, request)
        })
    } else {
        None
    };
    let edge_scroll_request_applied = if request_offset.is_some() {
        match sv.scroll_request {
            Some(ScrollRequest::Top | ScrollRequest::Bottom) => sv.scroll_request,
            _ => None,
        }
    } else {
        None
    };

    // Detect whether the element is requesting a genuinely new offset.
    // When both old and new values are at-or-beyond the previous max_offset
    // they're functionally equivalent ("stay at bottom"), even though the
    // raw numeric values differ (e.g. 9999 vs the clamped 411).  Without
    // this, the component "catching up" to the clamped value after a
    // resize is misidentified as a new external request, which bypasses
    // anchor-based scroll correction and breaks bottom-pinning.
    //
    // First `ScrollView::offset(Some(_))` bind (remount / reparent, e.g. sidebar
    // appears and the scroll node is re-allocated): if the prop is already
    // tail-aligned for *this* layout's max_offset, do not treat it as a hard
    // navigation request - same rule as `both_at_bottom`.
    let element_offset_changed = match (sv.offset, old_element_offset) {
        (Some(new_val), Some(old_val)) => {
            let both_at_bottom =
                old_max_offset > 0 && new_val >= old_max_offset && old_val >= old_max_offset;
            !both_at_bottom && !element_caught_up_to_live_offset && new_val != old_val
        }
        // `(Some, None)` only happens on the first reconcile pass for this node id
        // (`element_offset` is filled at the end of the previous frame).
        //
        // - **New** scroll node (reparent / first mount): the prop is initial
        //   state, not a navigation request - otherwise a controlled offset that
        //   still equals the *old* column's max is misclassified as external and
        //   blocks tail correction when the column narrows (e.g. sidebar docks).
        // - **Existing** node: app toggled `offset` from `None` → `Some` (e.g.
        //   handler-dirty catch-up) and that must remain a real external request.
        (Some(_), None) => had_scroll_view_state && !element_caught_up_to_live_offset,
        (a, b) => a != b,
    };
    let element_offset_interrupts_smooth_target = had_scroll_view_state
        && smooth_scroll.is_animating()
        && sv.offset.is_some()
        && raw_target_offset.is_some();
    let non_target_external_request = if let Some(requested) = request_offset {
        Some(requested)
    } else if let Some(input_offset) = input_offset {
        Some(input_offset)
    } else if element_offset_changed || element_offset_interrupts_smooth_target {
        sv.offset
    } else {
        None
    };
    let mut target_offset = if non_target_external_request.is_none() {
        raw_target_offset
    } else {
        None
    };
    let edge_scroll_target_applied = if target_offset.is_some() {
        match sv.scroll_target.as_ref() {
            Some(ScrollTarget::Top) => Some(ScrollRequest::Top),
            Some(ScrollTarget::Bottom) => Some(ScrollRequest::Bottom),
            _ => None,
        }
    } else {
        None
    };
    let external_request = non_target_external_request.or(target_offset);
    let handler_owns_offset =
        handler_dirty && !element_caught_up_to_live_offset && external_request.is_none();

    let requested_offset = if let Some(forced) = external_request {
        forced
    } else if handler_owns_offset {
        live_scroll_offset
    } else if had_scroll_view_state {
        old_offset
    } else {
        // New node (blank placeholder / first bind): `old_offset` is zero but the
        // element may already pass a meaningful `ScrollView::offset` prop.
        scroll_offset_hint
    };

    // Reparent / remount: node id is fresh (not ScrollView yet) but the user is
    // tail-pinned (large sentinel or `offset >= max` for this layout). Without
    // this, `old_viewport_w == 0` skips resize detection and anchor never runs.
    let remount_seeks_tail = !had_scroll_view_state
        && layout_max_offset > 0
        && sv.offset.is_some_and(|o| o >= layout_max_offset);
    let remount_has_anchor = !had_scroll_view_state && anchor.is_some();

    // Apply anchor-based scroll correction when viewport width or height
    // changed and no active explicit override (key request, fresh element
    // offset, or handler-owned override) is active.
    //
    // Height-only resizes must run the same path: when the viewport shrinks,
    // `max_offset` grows while a bottom-pinned `old_offset` would otherwise
    // sit below the new bottom until the user scrolls again.
    let viewport_resized = (old_viewport_w > 0 && viewport_w != old_viewport_w)
        || (old_viewport_h > 0 && viewport_height != old_viewport_h)
        || remount_seeks_tail
        || remount_has_anchor;
    let mut corrected_offset =
        if external_request.is_none() && !handler_owns_offset && viewport_resized {
            if let Some(ref anchor) = anchor {
                apply_scroll_anchor(
                    &sv.children,
                    &content_layout.rects,
                    anchor,
                    content_height,
                    viewport_height,
                    sv.show_scroll_indicators,
                )
            } else if remount_seeks_tail {
                layout_max_offset
            } else {
                requested_offset
            }
        } else {
            requested_offset
        };

    // Reparent with a new node id: if this `ScrollView` uses a stable `key` and
    // the same key was scrolled to the bottom on the previous frame, keep the
    // tail pinned without width probes (which were unstable across shrink/grow).
    if external_request.is_none()
        && !handler_owns_offset
        && !had_scroll_view_state
        && layout_max_offset > 0
        && scroll_key
            .as_ref()
            .is_some_and(|k| *tree.scroll_was_at_bottom_by_key.get(k).unwrap_or(&false))
    {
        corrected_offset = layout_max_offset;
    }

    // Geometry-stable reflow: wrapped / auto-height children (e.g. DocumentView)
    // can change `content_height` without the scroll viewport's width or height
    // changing. If the user was pinned to the bottom, keep them there.
    //
    // Also treat the transition from non-scrollable -> scrollable as a tail-pin
    // case when the element is still expressing bottom intent via a controlled
    // offset (sentinel / positive stale max) or an equivalent bottom request.
    let clamped_tail_catch_up = sv.offset == Some(0)
        && old_element_offset.is_some_and(|offset| offset == usize::MAX || offset > 0);
    let tail_requested = sv
        .offset
        .is_some_and(|offset| offset == usize::MAX || offset > 0)
        || clamped_tail_catch_up
        || matches!(sv.scroll_request, Some(ScrollRequest::Bottom))
        || matches!(sv.scroll_target, Some(ScrollTarget::Bottom));
    let geometry_stable_viewport = external_request.is_none()
        && !handler_owns_offset
        && old_viewport_w > 0
        && viewport_w == old_viewport_w
        && old_viewport_h > 0
        && viewport_height == old_viewport_h;
    let geometry_stable_content_changed =
        geometry_stable_viewport && content_height != old_content_height;
    if geometry_stable_content_changed {
        if let Some(tail_offset) = content_change_tail_offset(
            old_offset,
            old_max_offset,
            content_height,
            viewport_height,
            sv.show_scroll_indicators,
            tail_requested,
        ) {
            corrected_offset = tail_offset;
        } else if let Some(ref anchor) = anchor {
            corrected_offset = apply_scroll_anchor_top_edge(
                &sv.children,
                &content_layout.rects,
                anchor,
                content_height,
                viewport_height,
                sv.show_scroll_indicators,
            );
        }
    }

    // New-content tail-follow on the non-scrollable → scrollable transition.
    // Only fires when `old_max_offset == 0` and `layout_max_offset > 0` - i.e.
    // new items just arrived that pushed content past the viewport for the
    // first time - and the element is still expressing bottom intent via the
    // sentinel or an explicit `ScrollRequest::Bottom`. Outside this transition
    // we leave the offset alone so that user scroll position and anchor-based
    // resize corrections are respected.
    let sentinel_tail_intent = sv.offset == Some(usize::MAX)
        || matches!(sv.scroll_request, Some(ScrollRequest::Bottom))
        || matches!(sv.scroll_target, Some(ScrollTarget::Bottom));
    if external_request.is_none()
        && !handler_owns_offset
        && sentinel_tail_intent
        && old_max_offset == 0
        && layout_max_offset > 0
    {
        corrected_offset = layout_max_offset;
    }

    let hidden_viewport_offset = if viewport_height == 0 {
        Some(match sv.offset {
            Some(offset) if offset != usize::MAX => offset,
            _ if had_scroll_view_state => old_offset,
            _ => 0,
        })
    } else {
        None
    };
    if let Some(preserved_offset) = hidden_viewport_offset {
        target_offset = None;
        corrected_offset = preserved_offset;
    }

    if hidden_viewport_offset.is_none()
        && let Some(edge_request) = edge_scroll_request_applied.or(edge_scroll_target_applied)
    {
        // Edge requests/targets may jump away from the old viewport that was
        // used for the first virtual layout pass. Force an exact pass so newly
        // visible streaming rows are measured before we settle on the final offset.
        virtual_cache.reset();
        content_layout = layout_scroll_content_cached(
            &sv.props,
            &sv.children,
            &mut layout_cache,
            &mut virtual_cache,
            ScrollLayoutCachedParams {
                viewport_w,
                viewport_h: inner.h,
                scroll_offset: corrected_offset,
                estimated_child_height,
                horizontal_overflow,
            },
        );
        content_height = content_layout.content_height;
        layout_max_offset = calc_scroll_view_window(
            0,
            content_height as usize,
            viewport_height as usize,
            sv.show_scroll_indicators,
        )
        .max_offset;
        corrected_offset = match edge_request {
            ScrollRequest::Top => 0,
            ScrollRequest::Bottom => layout_max_offset,
            _ => corrected_offset,
        };
        if edge_scroll_target_applied.is_some() {
            target_offset = Some(corrected_offset);
        }
    }

    if let Some(target_offset) = target_offset {
        wheel_scroll.cancel_at(old_offset);
        corrected_offset = smooth_scroll.resolve_target(
            old_offset,
            target_offset,
            layout_max_offset,
            sv.scroll_behavior,
        );
    } else {
        if external_request.is_some() {
            wheel_scroll.cancel_at(corrected_offset);
        }
        smooth_scroll.cancel_at(corrected_offset);
    }
    if !sv.scroll_wheel || matches!(sv.scroll_wheel_behavior, ScrollWheelBehavior::Immediate) {
        wheel_scroll.cancel_at(corrected_offset);
    }

    if hidden_viewport_offset.is_none()
        && target_offset.is_none()
        && sv.children.len() > VIRTUAL_THRESHOLD
    {
        let measured_offset = corrected_offset.min(layout_max_offset);
        if virtual_cache.has_unresolved_in_zone(
            measured_offset,
            viewport_height,
            sv.props.gap,
            estimated_child_height,
            sv.children.len(),
        ) {
            let was_bottom_intent = corrected_offset >= layout_max_offset
                || sv.offset == Some(usize::MAX)
                || matches!(sv.scroll_request, Some(ScrollRequest::Bottom))
                || matches!(sv.scroll_target, Some(ScrollTarget::Bottom));
            content_layout = layout_scroll_content_cached(
                &sv.props,
                &sv.children,
                &mut layout_cache,
                &mut virtual_cache,
                ScrollLayoutCachedParams {
                    viewport_w,
                    viewport_h: inner.h,
                    scroll_offset: measured_offset,
                    estimated_child_height,
                    horizontal_overflow,
                },
            );
            content_height = content_layout.content_height;
            layout_max_offset = calc_scroll_view_window(
                0,
                content_height as usize,
                viewport_height as usize,
                sv.show_scroll_indicators,
            )
            .max_offset;
            corrected_offset = if was_bottom_intent {
                layout_max_offset
            } else {
                corrected_offset.min(layout_max_offset)
            };
            smooth_scroll.cancel_at(corrected_offset);
        }
    }

    let window = calc_scroll_view_window(
        corrected_offset,
        content_height as usize,
        viewport_height as usize,
        sv.show_scroll_indicators,
    );
    let mut effective_offset = hidden_viewport_offset.unwrap_or(window.offset);
    let mut max_offset = if hidden_viewport_offset.is_some() && had_scroll_view_state {
        old_max_offset
    } else {
        window.max_offset
    };
    let mut visible_rows = if hidden_viewport_offset.is_some() {
        0
    } else {
        window.visible_rows
    };

    let mut next_scroll_handler_dirty =
        handler_owns_offset && sv.offset.is_some_and(|offset| offset != effective_offset);
    let mut next_scroll_override = if external_request.is_some()
        || next_scroll_handler_dirty
        || element_caught_up_to_live_offset
    {
        Some(effective_offset)
    } else {
        None
    };

    let mut top_indicator = if hidden_viewport_offset.is_some() {
        false
    } else {
        window.top_indicator
    };
    let mut bottom_indicator = if hidden_viewport_offset.is_some() {
        false
    } else {
        window.bottom_indicator
    };
    let mut bottom_count = if hidden_viewport_offset.is_some() {
        0
    } else {
        window.bottom_count
    };

    let (old_h_offset, old_h_override, h_handler_dirty) =
        if let NodeKind::ScrollView(node) = &tree.node(id).kind {
            (
                node.h_offset,
                node.h_scroll_override,
                node.h_scroll_handler_dirty,
            )
        } else {
            (0, None, false)
        };
    let h_max_offset = if horizontal_overflow {
        content_layout.content_width.saturating_sub(viewport_w) as usize
    } else {
        0
    };
    let effective_h_offset = if h_handler_dirty {
        old_h_override.unwrap_or(old_h_offset).min(h_max_offset)
    } else {
        old_h_offset.min(h_max_offset)
    };
    let h_scrollbar_active = horizontal_overflow && sv.h_scrollbar && h_max_offset > 0;

    let (mut visible_indices, mut visible_rects) = collect_visible_scroll_children(
        &sv.children,
        &content_layout,
        ScrollVisibleCollectCtx {
            inner,
            viewport_w,
            top_indicator,
            bottom_indicator,
            effective_offset,
            h_offset: effective_h_offset,
        },
    );

    let mut visible_doc_restores = HashMap::new();
    let visible_child_keys = take_visible_doc_restores(
        &sv.children,
        &visible_indices,
        &mut offscreen_doc_selections,
        &mut visible_doc_restores,
    );

    // Save state from old children that are about to go off-screen. Their nodes
    // will be swept (epoch stale), so we stash selections and caches here and
    // restore them before reconcile when the child scrolls back into view.
    // DocumentViews may be nested arbitrarily deep (e.g. Frame → VStack →
    // DocumentView), so we walk the subtree of each scroll-view child and key by
    // the child's Key.
    {
        let old_child_ids = tree.node(id).children.clone();
        for old_cid in &old_child_ids {
            if !tree.is_valid(*old_cid) {
                continue;
            }
            let Some(key) = tree.node(*old_cid).key.clone() else {
                continue;
            };
            if visible_child_keys.contains(&key) {
                continue;
            }
            let mut docs = Vec::new();
            collect_doc_selections_in_subtree(tree, *old_cid, &mut docs);
            // Always snapshot DocumentView rows when they leave the viewport so
            // cross-`DocumentView` selection/copy can still address their text.
            if !docs.is_empty() {
                offscreen_doc_selections.insert(key, OffscreenDocSelection { docs });
            }
        }
    }

    let old_children = {
        let node = tree.node_mut(id);
        node.rect = rect;
        node.kind = NodeKind::ScrollView(ScrollViewNode {
            props: sv.props.clone(),
            element_offset: if hidden_viewport_offset.is_some() {
                old_element_offset
            } else {
                sv.offset
            },
            element_scroll_request: if hidden_viewport_offset.is_some() {
                old_element_scroll_request
            } else {
                sv.scroll_request
            },
            scroll_target: sv.scroll_target.clone(),
            cancelled_scroll_target: if hidden_viewport_offset.is_some() || scroll_target_suppressed
            {
                old_cancelled_scroll_target.clone()
            } else if non_target_external_request.is_some() {
                sv.scroll_target.clone()
            } else {
                None
            },
            offset: effective_offset,
            smooth_scroll,
            wheel_scroll,
            max_offset,
            scroll_offset: effective_offset as u16,
            content_height,
            content_width: content_layout.content_width,
            viewport_height,
            viewport_width: viewport_w,
            axis: sv.axis,
            h_offset: effective_h_offset,
            h_max_offset,
            h_scroll_offset: effective_h_offset as u16,
            h_scroll_override: if h_handler_dirty {
                Some(effective_h_offset)
            } else {
                None
            },
            h_scroll_handler_dirty: h_handler_dirty,
            content_viewport_w: viewport_w,
            scroll_keys: sv.scroll_keys,
            scroll_wheel: sv.scroll_wheel,
            scroll_wheel_multiplier: sv.scroll_wheel_multiplier,
            h_scroll_wheel_multiplier: sv.h_scroll_wheel_multiplier,
            scroll_wheel_behavior: sv.scroll_wheel_behavior,
            ambient_page_scroll: sv.ambient_page_scroll,
            focusable: sv.focusable,
            scrollbar: actual_standalone || (sv.scrollbar && !use_standalone),
            scrollbar_variant: sv.scrollbar_config.variant,
            scrollbar_gap: sv.scrollbar_config.gap,
            scrollbar_thumb: sv.scrollbar_config.thumb,
            scrollbar_thumb_style: sv.scrollbar_config.thumb_style,
            scrollbar_thumb_focus_style: sv.scrollbar_config.thumb_focus_style,
            scrollbar_track_style: sv.scrollbar_config.track_style,
            h_scrollbar: h_scrollbar_active,
            h_scrollbar_variant: sv.h_scrollbar_config.variant,
            h_scrollbar_gap: sv.h_scrollbar_config.gap,
            h_scrollbar_thumb: sv.h_scrollbar_config.thumb,
            h_scrollbar_thumb_style: sv.h_scrollbar_config.thumb_style,
            h_scrollbar_thumb_focus_style: sv.h_scrollbar_config.thumb_focus_style,
            h_scrollbar_track_style: sv.h_scrollbar_config.track_style,
            show_scroll_indicators: sv.show_scroll_indicators,
            scroll_indicator_style: sv.scroll_indicator_style,
            top_indicator,
            bottom_indicator,
            bottom_count,
            scroll_override: next_scroll_override,
            scroll_handler_dirty: next_scroll_handler_dirty,
            on_scroll: sv.on_scroll.clone(),
            on_scroll_to: sv.on_scroll_to.clone(),
            on_viewport_change: sv.on_viewport_change.clone(),
            viewport_snapshot: None,
            layout_cache,
            virtual_cache,
            offscreen_doc_selections: HashMap::new(),
        });
        std::mem::take(&mut node.children)
    };

    let mut new_children = reconcile_visible_scroll_children(
        tree,
        &sv.children,
        &visible_indices,
        &visible_rects,
        ScrollVisibleReconcileCtx {
            epoch,
            parent_id: id,
            old_children,
            visible_doc_restores: &mut visible_doc_restores,
            focus,
            overlay_state,
        },
    );

    // Pre-reconcile `content_layout` can underestimate row height when nested
    // widgets (notably `DocumentView` with auto height) gain lines only after
    // full format + visual planning. Keep scrollbar / max_offset aligned with
    // reconciled root rects and refresh cached stack layout when totals drift.
    let synced_layout = recompute_scroll_content_height_with_reconciled_roots(
        &sv.children,
        &content_layout,
        &visible_indices,
        &visible_rects,
        &new_children,
        tree,
        sv.props.gap,
    );
    if synced_layout.content_height != content_height {
        SCROLL_LAYOUT_CACHE_SYNC_DRIFTS.fetch_add(1, Ordering::Relaxed);
        content_layout = synced_layout;
        content_height = content_layout.content_height;
        let offset_before_sync_anchor = effective_offset;
        if external_request.is_none() && !handler_owns_offset {
            if let Some(tail_offset) = content_change_tail_offset(
                old_offset,
                old_max_offset,
                content_height,
                viewport_height,
                sv.show_scroll_indicators,
                tail_requested,
            ) {
                effective_offset = tail_offset;
            } else if let Some(ref anchor) = anchor {
                effective_offset = if geometry_stable_viewport {
                    apply_scroll_anchor_top_edge(
                        &sv.children,
                        &content_layout.rects,
                        anchor,
                        content_height,
                        viewport_height,
                        sv.show_scroll_indicators,
                    )
                } else {
                    apply_scroll_anchor(
                        &sv.children,
                        &content_layout.rects,
                        anchor,
                        content_height,
                        viewport_height,
                        sv.show_scroll_indicators,
                    )
                };
            }
        }
        let synced_window = calc_scroll_view_window(
            effective_offset,
            content_height as usize,
            viewport_height as usize,
            sv.show_scroll_indicators,
        );
        effective_offset = synced_window.offset;
        max_offset = synced_window.max_offset;
        visible_rows = synced_window.visible_rows;
        top_indicator = synced_window.top_indicator;
        bottom_indicator = synced_window.bottom_indicator;
        bottom_count = synced_window.bottom_count;
        let synced_offset_changed = effective_offset != offset_before_sync_anchor;
        let (synced_visible_indices, synced_visible_rects) = collect_visible_scroll_children(
            &sv.children,
            &content_layout,
            ScrollVisibleCollectCtx {
                inner,
                viewport_w,
                top_indicator,
                bottom_indicator,
                effective_offset,
                h_offset: effective_h_offset,
            },
        );
        let synced_visible_geometry_changed =
            synced_visible_indices != visible_indices || synced_visible_rects != visible_rects;
        next_scroll_handler_dirty =
            handler_owns_offset && sv.offset.is_some_and(|offset| offset != effective_offset);
        next_scroll_override = if external_request.is_some()
            || next_scroll_handler_dirty
            || element_caught_up_to_live_offset
        {
            Some(effective_offset)
        } else {
            None
        };

        let visible_updates: Vec<(usize, u16)> = visible_indices
            .iter()
            .enumerate()
            .filter_map(|(slot, &idx)| {
                let cid = *new_children.get(slot)?;
                if !tree.is_valid(cid) {
                    return None;
                }
                Some((idx, tree.node(cid).rect.h))
            })
            .collect();

        let mut needs_visible_reflow = synced_offset_changed || synced_visible_geometry_changed;
        if let NodeKind::ScrollView(ref mut sv_node) = tree.node_mut(id).kind {
            sv_node.content_height = content_height;
            sv_node.offset = effective_offset;
            sv_node.scroll_offset = effective_offset as u16;
            if target_offset.is_none() {
                sv_node.smooth_scroll.cancel_at(effective_offset);
            }
            sv_node.max_offset = max_offset;
            sv_node.top_indicator = top_indicator;
            sv_node.bottom_indicator = bottom_indicator;
            sv_node.bottom_count = bottom_count;
            sv_node.scroll_override = next_scroll_override;
            sv_node.scroll_handler_dirty = next_scroll_handler_dirty;

            let mut pending_heights: Vec<(usize, u16, u16)> = Vec::new();
            for (child_idx, actual) in visible_updates {
                let Some(Some(entry)) = sv_node.virtual_cache.entries.get(child_idx) else {
                    continue;
                };
                let old_h = entry.h;
                if old_h != actual {
                    pending_heights.push((child_idx, old_h, actual));
                }
            }
            // Keep the offset / visible-geometry triggers from above: a tail
            // follow or anchor correction can move `effective_offset` after the
            // initial reconcile without any *further* child-height drift here
            // (the height was already reconciled into the cache). The visible
            // children must still be re-collected and repositioned for the new
            // offset, otherwise they keep their pre-correction screen rects and
            // the freshly exposed rows render at the wrong place (or blank).
            needs_visible_reflow = needs_visible_reflow || !pending_heights.is_empty();
            for (child_idx, old_h, actual) in pending_heights {
                sv_node.virtual_cache.unrecord_measurement(old_h);
                if let Some(Some(ent)) = sv_node.virtual_cache.entries.get_mut(child_idx) {
                    ent.h = actual;
                }
                sv_node.virtual_cache.record_measurement(actual);
            }

            SCROLL_LAYOUT_CACHE_INVALIDATIONS.fetch_add(1, Ordering::Relaxed);
            sv_node.layout_cache.invalidate();
            maybe_log_scroll_layout_cache_stats();
        }

        if needs_visible_reflow {
            // Reconcile the visible children against the recompute-corrected
            // `content_layout` / `effective_offset` already settled above. The
            // previous implementation re-ran `layout_scroll_content_cached`
            // here, but for auto-height children whose reconciled height exceeds
            // the measure-pass estimate (e.g. themed markdown `DocumentView`s)
            // that re-measure silently reverts `content_height` — and the tail
            // follow that depends on it — back to the underestimate, leaving the
            // freshly exposed bottom rows positioned for the stale offset (they
            // render blank or show the wrong slice). `synced_visible_*` already
            // reflects the corrected heights, offset, and indicators, so just
            // reconcile those.
            visible_indices = synced_visible_indices;
            visible_rects = synced_visible_rects;
            take_visible_doc_restores(
                &sv.children,
                &visible_indices,
                &mut offscreen_doc_selections,
                &mut visible_doc_restores,
            );
            new_children = reconcile_visible_scroll_children(
                tree,
                &sv.children,
                &visible_indices,
                &visible_rects,
                ScrollVisibleReconcileCtx {
                    epoch,
                    parent_id: id,
                    old_children: new_children,
                    visible_doc_restores: &mut visible_doc_restores,
                    focus,
                    overlay_state,
                },
            );
        }
    }

    let viewport_event = if sv.on_viewport_change.is_some() {
        let viewport_snapshot = build_scroll_viewport_snapshot(
            &sv.children,
            &content_layout,
            &visible_indices,
            ScrollSnapshotBuildCtx {
                effective_offset,
                visible_rows,
                viewport_w,
                content_height,
                max_offset,
                top_indicator,
                bottom_indicator,
                bottom_count,
            },
        );
        let viewport_callback_newly_added = !old_had_viewport_callback;
        let viewport_changed = old_viewport_snapshot.as_ref() != Some(&viewport_snapshot);
        let viewport_event = if viewport_changed || viewport_callback_newly_added {
            Some(build_scroll_viewport_event(
                &viewport_snapshot,
                if viewport_callback_newly_added {
                    None
                } else {
                    old_viewport_snapshot.as_ref()
                },
                &sv.children,
                &content_layout,
            ))
        } else {
            None
        };
        if let NodeKind::ScrollView(ref mut sv_node) = tree.node_mut(id).kind {
            sv_node.viewport_snapshot = Some(viewport_snapshot);
        }
        viewport_event
    } else {
        if let NodeKind::ScrollView(ref mut sv_node) = tree.node_mut(id).kind {
            sv_node.viewport_snapshot = None;
        }
        None
    };
    if let (Some(cb), Some(event)) = (sv.on_viewport_change.as_ref(), viewport_event) {
        cb.emit(event);
    }

    let scroll_request_changed = sv.scroll_request != old_element_scroll_request;
    let controlled_offset_desynced = sv.offset.is_some_and(|prop| prop != effective_offset);
    let scroll_request_moved_uncontrolled = sv.offset.is_none()
        && scroll_request_changed
        && request_offset.is_some()
        && effective_offset != old_offset;
    if let Some(cb) = sv.on_scroll.as_ref()
        && hidden_viewport_offset.is_none()
        && (controlled_offset_desynced || scroll_request_moved_uncontrolled)
    {
        cb.emit(ScrollEvent {
            offset: effective_offset,
            metrics: ScrollMetrics {
                len: content_height as usize,
                visible: visible_rows,
                max_offset,
            },
        });
    }

    if let Some(ref k) = scroll_key {
        tree.scroll_was_at_bottom_by_key
            .insert(k.clone(), max_offset > 0 && effective_offset >= max_offset);
        if tree
            .scroll_input_offset_by_key
            .get(k)
            .is_some_and(|offset| *offset == effective_offset)
        {
            tree.scroll_input_offset_by_key.remove(k);
        }
    }

    {
        let node = tree.node_mut(id);
        if let NodeKind::ScrollView(sv_node) = &mut node.kind {
            sv_node.offscreen_doc_selections = offscreen_doc_selections;
        }
        node.children = new_children;
    }

    if let Some(ref k) = scroll_key {
        if let Some(anchor) = compute_visible_scroll_anchor(tree, id, &sv.children).or_else(|| {
            compute_layout_scroll_anchor(
                &sv.children,
                &content_layout.rects,
                effective_offset,
                viewport_height,
                max_offset,
            )
        }) {
            tree.remembered_scroll_anchor_by_key
                .insert(k.clone(), anchor);
        } else {
            tree.remembered_scroll_anchor_by_key.remove(k);
        }
    }

    tree.register_scrollbar_zone(id);

    id
}

#[cfg(test)]
mod tests;
