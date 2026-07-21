#[cfg(feature = "diff-view")]
use crate::app::input::mouse::types::DiffContextSeparatorClick;
use crate::app::input::mouse::types::{
    CheckboxToggle, DocumentClick, DragSourceGrab, DraggableTabBarAction, FlowchartItemClick,
    GraphNodeClick, HitActions, InputChange, ListSelect, ProgressChange, SliderChange,
    SplitterGrab, TableSelect, TabsChange, TextAreaChange,
};
use crate::core::event::KeyMods;
use crate::core::node::{Node, NodeId, NodeKind, NodeTree};
use crate::style::{Rect, ScrollbarVariant};
use crate::widgets::table::table_header_reserved_height;
use crate::widgets::{TextAreaVisualKeyArgs, hash_peer_source_lines, make_text_area_visual_key};
use unicode_width::UnicodeWidthStr;

#[cfg(feature = "diff-view")]
fn diff_context_separator_click_from_source_line(
    config: Option<&crate::widgets::DiffContextSeparatorClickConfig>,
    source_line: usize,
) -> Option<DiffContextSeparatorClick> {
    let config = config?;
    let event = config.events_by_source_line.get(source_line)?.clone()?;
    let cb = config.on_click.clone()?;
    Some(DiffContextSeparatorClick { cb, event })
}

#[cfg(feature = "diff-view")]
fn document_view_diff_context_separator_click(
    node: &Node,
    x: i16,
    y: i16,
) -> Option<DiffContextSeparatorClick> {
    let NodeKind::DocumentView(doc) = &node.kind else {
        return None;
    };
    let inner = node.rect.inner(doc.border, doc.padding);
    let cl = doc.content_layout(inner);
    let content_rect = Rect {
        x: cl.content_x,
        y: cl.content_y,
        w: cl.content_width,
        h: cl.content_height,
    };
    if !content_rect.contains(x, y) {
        return None;
    }

    let rel_y = y.saturating_sub(inner.y) as usize;
    let visual_idx = doc.scroll_offset.saturating_add(rel_y);
    let source_line = doc.visual_cache.source_line_map.get(visual_idx).copied()?;
    diff_context_separator_click_from_source_line(
        doc.diff_context_separator_click.as_ref(),
        source_line,
    )
}

#[cfg(feature = "diff-view")]
fn text_area_diff_context_separator_click(
    change: &TextAreaChange,
    x: i16,
    y: i16,
) -> Option<DiffContextSeparatorClick> {
    let inner = change.rect.inner(change.border, change.padding);
    let scrollbar_cols = if change.scrollbar && !change.scrollbar_over_border {
        1u16.saturating_add(change.scrollbar_gap)
    } else {
        0
    };
    let content_rect = Rect {
        x: inner.x,
        y: inner.y,
        w: inner.w.saturating_sub(scrollbar_cols),
        h: inner.h.saturating_sub(u16::from(
            change.h_scrollbar && !change.h_scrollbar_over_border,
        )),
    };
    if !content_rect.contains(x, y) {
        return None;
    }

    let rel_y = y.saturating_sub(inner.y) as usize;
    let visual_idx = change.scroll_offset.saturating_add(rel_y);
    let source_line = change
        .visual_lines
        .as_ref()?
        .get(visual_idx)?
        .line_num
        .saturating_sub(1);
    diff_context_separator_click_from_source_line(
        change.diff_context_separator_click.as_ref(),
        source_line,
    )
}

/// Resolve final left-click target from a deepest hit node.
///
/// If an ancestor `MouseRegion` has `capture_click = true`, is enabled, and has an
/// `on_click` handler, that region becomes the click target instead of an
/// interactive descendant.
pub(crate) fn resolve_left_click_target(tree: &NodeTree, hit: NodeId, mods: KeyMods) -> NodeId {
    let hit_node = tree.node(hit);
    let should_bubble_non_clickable_frame = matches!(hit_node.kind, NodeKind::Frame(_))
        && !hit_node.is_focusable()
        && !hit_node.has_on_click();

    let mut cur = Some(hit);
    while let Some(id) = cur {
        if !tree.is_valid(id) {
            break;
        }
        let node = tree.node(id);
        if let NodeKind::MouseRegion(region) = &node.kind
            && region.enabled
            && (region.capture_click
                || mouse_region_captures_mods(region, mods)
                || should_bubble_non_clickable_frame)
            && (region.on_click.is_some()
                || region.on_mouse_down.is_some()
                || region.on_mouse_up.is_some()
                || region.on_drag_start.is_some()
                || region.on_drag.is_some()
                || region.on_drag_end.is_some())
        {
            return id;
        }
        cur = node.parent;
    }
    hit
}

#[cfg(feature = "terminal")]
pub(crate) fn ancestor_mouse_region_captures_mods(
    tree: &NodeTree,
    start: NodeId,
    mods: KeyMods,
) -> bool {
    let mut cur = Some(start);
    while let Some(id) = cur {
        if !tree.is_valid(id) {
            break;
        }
        let node = tree.node(id);
        if let NodeKind::MouseRegion(region) = &node.kind
            && region.enabled
            && mouse_region_captures_mods(region, mods)
        {
            return true;
        }
        cur = node.parent;
    }
    false
}

fn mouse_region_captures_mods(
    region: &crate::widgets::internal::MouseRegionNode,
    mods: KeyMods,
) -> bool {
    region
        .capture_required_mods
        .is_some_and(|required| mods_contain(required, mods))
}

fn mods_contain(required: KeyMods, actual: KeyMods) -> bool {
    (!required.ctrl || actual.ctrl)
        && (!required.alt || actual.alt)
        && (!required.shift || actual.shift)
        && (!required.super_key || actual.super_key)
}

/// Walk up from `start` and return the first enabled `MouseRegion` ancestor
/// that has an `on_click` handler.
pub(crate) fn find_ancestor_on_click(
    tree: &NodeTree,
    start: NodeId,
) -> Option<crate::callback::Callback<crate::core::event::MouseEvent>> {
    let mut cur = tree.node(start).parent;
    while let Some(id) = cur {
        if !tree.is_valid(id) {
            break;
        }
        let node = tree.node(id);
        if let NodeKind::MouseRegion(region) = &node.kind
            && region.enabled
            && let Some(cb) = &region.on_click
        {
            return Some(cb.clone());
        }
        cur = node.parent;
    }
    None
}

/// Gather actions for a hit node.
pub(crate) fn gather_hit_actions(tree: &NodeTree, hit: NodeId, x: u16, y: u16) -> HitActions {
    let node = tree.node(hit);
    let x_i16 = x as i16;
    let y_i16 = y as i16;

    let on_click = match &node.kind {
        NodeKind::Button(btn) => btn.on_click.clone(),
        NodeKind::Input(node) => node.on_click.clone(),
        NodeKind::List(list_node) => list_node.on_click.clone(),
        NodeKind::Tabs(node) => node.on_click.clone(),
        NodeKind::DraggableTabBar(node) => node.on_click.clone(),
        NodeKind::Checkbox(checkbox) => checkbox.on_click.clone(),
        NodeKind::ProgressBar(progress) => progress.on_click.clone(),
        NodeKind::MouseRegion(region) => region.on_click.clone(),
        _ => None,
    };

    let on_mouse_down = match &node.kind {
        NodeKind::MouseRegion(region) => region.on_mouse_down.clone(),
        _ => None,
    };

    let on_mouse_up = match &node.kind {
        NodeKind::MouseRegion(region) => region.on_mouse_up.clone(),
        _ => None,
    };

    let (on_drag_start, on_drag, on_drag_end) = match &node.kind {
        NodeKind::MouseRegion(region) => (
            region.on_drag_start.clone(),
            region.on_drag.clone(),
            region.on_drag_end.clone(),
        ),
        _ => (None, None, None),
    };

    let document_click = match &node.kind {
        NodeKind::DocumentView(doc)
            if doc.on_click.is_some() && node.rect.contains(x_i16, y_i16) =>
        {
            let inner = node.rect.inner(doc.border, doc.padding);
            let cl = doc.content_layout(inner);

            let content_rect = Rect {
                x: cl.content_x,
                y: cl.content_y,
                w: cl.content_width,
                h: cl.content_height,
            };
            if !content_rect.contains(x_i16, y_i16) {
                None
            } else {
                let rel_y = y_i16.saturating_sub(inner.y) as usize;
                let visual_idx = doc.scroll_offset.saturating_add(rel_y);
                let source_line = doc
                    .visual_cache
                    .source_line_map
                    .get(visual_idx)
                    .copied()
                    .unwrap_or(0);

                let rel_x = x_i16.saturating_sub(cl.content_x).max(0) as usize;
                let visual_col = if !doc.wrap {
                    rel_x.saturating_add(doc.h_scroll_offset)
                } else {
                    rel_x
                };
                let link = doc.visual_cache.lines.get(visual_idx).and_then(|vline| {
                    let line_text = doc.visual_cache.line_texts.get(visual_idx)?;
                    let byte_in_line =
                        crate::widgets::document_view::node::document_view_byte_in_line_from_visual_col(
                            line_text.as_ref(),
                            vline,
                            visual_col,
                        );
                    let links = match &vline.kind {
                        crate::widgets::document_view::node::VisualLineKind::Text {
                            links, ..
                        } => links.as_slice(),
                        crate::widgets::document_view::node::VisualLineKind::BlockQuoteLine {
                            links,
                            ..
                        } => links.as_slice(),
                        _ => &[],
                    };
                    links
                        .iter()
                        .find(|l| byte_in_line >= l.start && byte_in_line < l.end)
                        .map(|l| l.url.clone())
                });

                doc.on_click.clone().map(|cb| DocumentClick {
                    cb,
                    source_line,
                    link,
                })
            }
        }
        _ => None,
    };

    let checkbox_toggle = match &node.kind {
        NodeKind::Checkbox(checkbox) => checkbox.on_toggle.clone().map(|cb| CheckboxToggle {
            cb,
            state: checkbox.state,
        }),
        _ => None,
    };

    let progress_change = match &node.kind {
        NodeKind::ProgressBar(progress) => {
            if progress.draggable || progress.on_change.is_some() {
                if node.rect.contains(x_i16, y_i16) {
                    crate::app::input::geometry::progress_value_at_x(progress, node.rect, x).map(
                        |p_value| ProgressChange {
                            on_change: progress.on_change.clone(),
                            progress: p_value,
                            node_id: node.id,
                            draggable: progress.draggable,
                        },
                    )
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    };

    let graph_node_click = match &node.kind {
        NodeKind::Graph(graph)
            if graph.focusable
                || graph.on_node_click.is_some()
                || graph.on_node_focus.is_some()
                || graph.on_node_activate.is_some() =>
        {
            graph
                .local_content_point(node.rect, x_i16, y_i16)
                .and_then(|(local_x, local_y)| graph.hit_test(local_x, local_y))
                .map(|(_, graph_node)| GraphNodeClick {
                    node_id: node.id,
                    cb: graph.on_node_click.clone(),
                    event: crate::widgets::GraphNodeEvent {
                        path: graph_node.path.clone(),
                        label: graph_node.label.clone(),
                    },
                })
        }
        _ => None,
    };

    let sequence_item_click = match &node.kind {
        NodeKind::SequenceDiagram(sequence) => sequence.on_item_click.clone().and_then(|cb| {
            sequence
                .local_content_point(node.rect, x_i16, y_i16)
                .and_then(|(local_x, local_y)| sequence.hit_test(local_x, local_y))
                .and_then(|path| sequence.item_event(path))
                .map(|event| crate::app::input::mouse::SequenceItemClick { cb, event })
        }),
        _ => None,
    };

    let flowchart_item_click = match &node.kind {
        NodeKind::Flowchart(flowchart) => flowchart
            .local_content_point(node.rect, x_i16, y_i16)
            .and_then(|(local_x, local_y)| flowchart.hit_test(local_x, local_y))
            .and_then(|(_, path)| flowchart.item_event(&path))
            .and_then(|event| match event {
                crate::widgets::internal::FlowchartItemEvent::Node(event) => flowchart
                    .on_node_click
                    .clone()
                    .map(|cb| FlowchartItemClick::Node { cb, event }),
                crate::widgets::internal::FlowchartItemEvent::Edge(event) => flowchart
                    .on_edge_click
                    .clone()
                    .map(|cb| FlowchartItemClick::Edge { cb, event }),
                crate::widgets::internal::FlowchartItemEvent::Subgraph(event) => flowchart
                    .on_subgraph_click
                    .clone()
                    .map(|cb| FlowchartItemClick::Subgraph { cb, event }),
            }),
        _ => None,
    };

    let slider_change = match &node.kind {
        NodeKind::Slider(slider) => {
            if slider.on_change.is_some() || slider.on_click.is_some() {
                if node.rect.contains(x_i16, y_i16) {
                    let track =
                        crate::app::input::geometry::slider_track_geometry(slider, node.rect);
                    match track {
                        Some(t)
                            if y_i16 == t.track_y
                                && x_i16 >= t.track_x
                                && x_i16 < t.track_x.saturating_add(t.track_w as i16) =>
                        {
                            Some(SliderChange { node_id: node.id })
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    };

    let splitter_grab = match &node.kind {
        NodeKind::Splitter(splitter) => {
            splitter.handle_at(x_i16, y_i16).map(|handle| SplitterGrab {
                node_id: node.id,
                handle,
            })
        }
        _ => None,
    };

    let mut drag_source_grab = None;
    let mut cur = Some(hit);
    while let Some(id) = cur {
        if !tree.is_valid(id) {
            break;
        }
        let n = tree.node(id);
        if let NodeKind::DragSource(source) = &n.kind
            && source.enabled
            && source.on_drag_start.is_some()
        {
            drag_source_grab = Some(DragSourceGrab { node_id: id });
            break;
        }
        cur = n.parent;
    }

    let input_change = match &node.kind {
        NodeKind::Input(input_node) => {
            if !input_node.disabled && (input_node.on_change.is_some() || input_node.read_only) {
                Some(InputChange {
                    on_change: input_node.on_change.clone(),
                    value: input_node.value.clone(),
                    cursor: input_node.cursor,
                    anchor: input_node.anchor,
                    focusable: input_node.focusable,
                    prefix: input_node.prefix.clone(),
                    border: input_node.border,
                    padding: input_node.padding,
                    rect: node.rect,
                    node_id: node.id,
                    read_only: input_node.read_only,
                    masked: input_node.mask.is_some(),
                })
            } else {
                None
            }
        }
        _ => None,
    };

    let list_select = match &node.kind {
        NodeKind::List(list_node) => list_node.on_select.clone().map(|cb| ListSelect {
            cb,
            on_item_click: list_node.on_item_click.clone(),
            on_activate: list_node.on_activate.clone(),
            activate_on_click: list_node.activate_on_click,
            len: list_node.items.len(),
            border: list_node.border,
            padding: list_node.padding,
            scrollbar: list_node.scrollbar,
            scrollbar_variant: list_node.scrollbar_variant,
            show_scroll_indicators: list_node.show_scroll_indicators,
            rect: node.rect,
        }),
        _ => None,
    };

    let tabs_change = match &node.kind {
        NodeKind::Tabs(node_tabs) => node_tabs.on_change.clone().and_then(|cb| {
            let len = node_tabs.tabs.len();
            if len == 0 {
                return None;
            }

            let active = node_tabs.active.min(len.saturating_sub(1));

            let inner = node.rect.inner(node_tabs.border, node_tabs.padding);

            if !inner.contains(x_i16, y_i16) {
                return None;
            }

            let col = (x_i16 as i32).saturating_sub(inner.x as i32) as usize;
            let idx = crate::widgets::Tabs::index_at_col(
                &node_tabs.tabs,
                node_tabs.divider,
                node_tabs.overflow,
                inner.w as usize,
                col,
            )?;
            Some(TabsChange {
                cb,
                next: idx,
                active,
            })
        }),
        _ => None,
    };

    let draggable_tab_bar_action = match &node.kind {
        NodeKind::DraggableTabBar(node_tabs) => {
            let len = node_tabs.tabs.len();
            if len == 0 {
                None
            } else {
                let has_handlers = node_tabs.on_change.is_some()
                    || node_tabs.on_action.is_some()
                    || node_tabs.on_close.is_some()
                    || (node_tabs.draggable
                        && (node_tabs.on_reorder.is_some() || node_tabs.on_transfer.is_some()))
                    || node_tabs.show_overflow_controls;
                if !has_handlers {
                    None
                } else {
                    let active = node_tabs.active.min(len.saturating_sub(1));
                    let inner = node.rect.inner(node_tabs.border, node_tabs.padding);
                    if !inner.contains(x_i16, y_i16) {
                        None
                    } else {
                        let col = (x_i16 as i32).saturating_sub(inner.x as i32) as usize;
                        crate::widgets::DraggableTabBar::hit_target_at_view_col(
                            &node_tabs.tabs,
                            &node_tabs.display_options(),
                            &node_tabs.viewport_options(inner.w as usize),
                            col,
                        )
                        .map(|target| match target {
                            crate::widgets::draggable_tab_bar::DraggableTabHitTarget::Overflow(
                                crate::widgets::draggable_tab_bar::OverflowControlSide::Left,
                            ) => DraggableTabBarAction {
                                node_id: node.id,
                                overflow_scroll_step: -1,
                                tab_index: active,
                                action_hit: false,
                                close_hit: false,
                                active,
                                bar_id: node_tabs.bar_id.clone(),
                                drag_group: node_tabs.drag_group.clone(),
                                draggable: false,
                                reorder_mode: node_tabs.reorder_mode,
                                drag_threshold: node_tabs.drag_threshold,
                                on_change: node_tabs.on_change.clone(),
                                on_action: node_tabs.on_action.clone(),
                                on_close: node_tabs.on_close.clone(),
                                on_transfer: node_tabs.on_transfer.clone(),
                            },
                            crate::widgets::draggable_tab_bar::DraggableTabHitTarget::Overflow(
                                crate::widgets::draggable_tab_bar::OverflowControlSide::Right,
                            ) => DraggableTabBarAction {
                                node_id: node.id,
                                overflow_scroll_step: 1,
                                tab_index: active,
                                action_hit: false,
                                close_hit: false,
                                active,
                                bar_id: node_tabs.bar_id.clone(),
                                drag_group: node_tabs.drag_group.clone(),
                                draggable: false,
                                reorder_mode: node_tabs.reorder_mode,
                                drag_threshold: node_tabs.drag_threshold,
                                on_change: node_tabs.on_change.clone(),
                                on_action: node_tabs.on_action.clone(),
                                on_close: node_tabs.on_close.clone(),
                                on_transfer: node_tabs.on_transfer.clone(),
                            },
                            crate::widgets::draggable_tab_bar::DraggableTabHitTarget::Tab(hit) => {
                                let action_hit = node_tabs.tabs.get(hit.index).is_some_and(|tab| {
                                    tab.kind == crate::widgets::DraggableTabKind::Action
                                });
                                DraggableTabBarAction {
                                    node_id: node.id,
                                    overflow_scroll_step: 0,
                                    tab_index: hit.index,
                                    action_hit,
                                    close_hit: matches!(
                                        hit.part,
                                        crate::widgets::DraggableTabHitPart::Close
                                    ),
                                    active,
                                    bar_id: node_tabs.bar_id.clone(),
                                    drag_group: node_tabs.drag_group.clone(),
                                    draggable: node_tabs.draggable,
                                    reorder_mode: node_tabs.reorder_mode,
                                    drag_threshold: node_tabs.drag_threshold,
                                    on_change: node_tabs.on_change.clone(),
                                    on_action: node_tabs.on_action.clone(),
                                    on_close: node_tabs.on_close.clone(),
                                    on_transfer: node_tabs.on_transfer.clone(),
                                }
                            }
                        })
                    }
                }
            }
        }
        _ => None,
    };

    let border_tabs_change = gather_border_tabs_change(tree, node, x_i16, y_i16);

    let table_select = match &node.kind {
        NodeKind::Table(table_node) => table_node.on_select.clone().map(|cb| TableSelect {
            cb,
            on_activate: table_node.on_activate.clone(),
            rows: table_node.rows.clone(),
            offset: table_node.offset,
            header_height: table_header_reserved_height(
                table_node.header.as_ref(),
                table_node.rows.len(),
                table_node.row_gap,
            ),
            row_gap: table_node.row_gap,
            rect: node.rect,
            border: table_node.border,
            padding: table_node.padding,
            show_scroll_indicators: table_node.show_scroll_indicators,
            top_indicator: table_node.top_indicator,
            bottom_indicator: table_node.bottom_indicator,
        }),
        _ => None,
    };

    let textarea_change = match &node.kind {
        NodeKind::TextArea(ta) => {
            let parent_v_edge = tree.parent_frame_integrated_v_edge(hit).unwrap_or(false);
            let parent_h_edge = tree.parent_frame_integrated_h_edge(hit).unwrap_or(false);

            let scrollbar_over_border = ta.scrollbar
                && matches!(ta.scrollbar_variant, ScrollbarVariant::Integrated)
                && (ta.border || parent_v_edge);
            let h_scrollbar_over_border = ta.h_scrollbar
                && matches!(ta.h_scrollbar_variant, ScrollbarVariant::Integrated)
                && (ta.border || parent_h_edge);

            if !ta.disabled
                && (ta.on_change.is_some() || ta.read_only || ta.on_sentinel_click.is_some())
            {
                let inner_w = node.rect.inner(ta.border, ta.padding).w;
                let sentinel_for_key = crate::widgets::sentinel_info_for(
                    ta.image_mode,
                    ta.images.len(),
                    &ta.image_placeholder,
                    &ta.sentinels,
                );
                let (sentinel_ph_width, sentinel_count) = sentinel_for_key
                    .as_ref()
                    .and_then(|si| si.image.map(|(_, _, pw)| (pw, ta.images.len())))
                    .unwrap_or((0, 0));
                let custom_sentinel_hash: u64 = {
                    use std::hash::{Hash, Hasher};
                    let mut h = rustc_hash::FxHasher::default();
                    if let Some(si) = sentinel_for_key.as_ref()
                        && let Some((_, _, ref widths, _)) = si.custom
                    {
                        widths.hash(&mut h);
                    }
                    h.finish()
                };
                let visual_key = make_text_area_visual_key(
                    ta.content_hash,
                    hash_peer_source_lines(ta.peer_source_lines.as_ref()),
                    TextAreaVisualKeyArgs {
                        inner_w,
                        wrap: ta.wrap,
                        line_numbers: ta.line_numbers,
                        min_line_number_width: ta.min_line_number_width,
                        scrollbar: ta.scrollbar,
                        scrollbar_over_border,
                        scrollbar_gap: ta.scrollbar_gap,
                        read_only: ta.read_only,
                        cursor: ta.cursor,
                        tab_stop: ta.tab_display_width,
                        sentinel_ph_width,
                        sentinel_count,
                        custom_sentinel_hash,
                        virtual_text_hash: crate::widgets::text_area_virtual_text_hash(
                            &ta.virtual_texts,
                        ),
                        gutter_col_width: ta.gutter_col_width,
                        gutter_gap: ta.gutter_gap,
                        #[cfg(feature = "diff-view")]
                        split_wrap_pane_widths: if let (Some(sync), Some(side)) =
                            (&ta.split_wrap_sync, ta.split_wrap_side)
                        {
                            crate::widgets::split_wrap_pane_widths(sync, side)
                        } else {
                            None
                        },
                        #[cfg(feature = "diff-view")]
                        split_wrap_scrollbar_cols: ta
                            .split_wrap_sync
                            .as_ref()
                            .map(crate::widgets::split_wrap_scrollbar_cols_pair),
                        #[cfg(feature = "diff-view")]
                        split_wrap_layout_pass: ta
                            .split_wrap_sync
                            .as_ref()
                            .map(crate::widgets::split_wrap_layout_pass)
                            .unwrap_or(0),
                    },
                );
                Some(TextAreaChange {
                    on_change: ta.on_change.clone(),
                    on_editor_state_change: ta.on_editor_state_change.clone(),
                    value: ta.value.clone(),
                    cursor: ta.cursor,
                    anchor: ta.anchor,
                    focusable: ta.focusable,
                    border: ta.border,
                    padding: ta.padding,
                    line_numbers: ta.line_numbers,
                    min_line_number_width: ta.min_line_number_width,
                    wrap: ta.wrap,
                    scroll_offset: ta.scroll_offset,
                    scrollbar: ta.scrollbar,
                    scrollbar_variant: ta.scrollbar_variant,
                    scrollbar_gap: ta.scrollbar_gap,
                    scrollbar_over_border,
                    h_scrollbar: ta.h_scrollbar,
                    h_scrollbar_variant: ta.h_scrollbar_variant,
                    h_scrollbar_over_border,
                    max_line_width: ta.max_line_width,
                    h_scroll_offset: ta.h_scroll_offset,
                    rect: node.rect,
                    node_id: node.id,
                    read_only: ta.read_only,
                    vim_motions: ta.vim_motions,
                    on_vim_mode_change: ta.on_vim_mode_change.clone(),
                    sentinel_info: crate::widgets::sentinel_info_for(
                        ta.image_mode,
                        ta.images.len(),
                        &ta.image_placeholder,
                        &ta.sentinels,
                    ),
                    tab_stop: ta.tab_display_width as usize,
                    gutter_col_width: ta.gutter_col_width,
                    gutter_gap: ta.gutter_gap,
                    logical_lines_count: ta.logical_lines_count,
                    visual_lines: ta.visual_cache.get_lines_cloned(&visual_key),
                    virtual_texts: ta.virtual_texts.clone(),
                    multi_click_select: ta.multi_click_select,
                    triple_click_mode: ta.triple_click_mode,
                    on_sentinel_click: ta.on_sentinel_click.clone(),
                    #[cfg(feature = "diff-view")]
                    diff_context_separator_click: ta.diff_context_separator_click.clone(),
                    images: if ta.on_sentinel_click.is_some() {
                        ta.images.clone()
                    } else {
                        Vec::new()
                    },
                    image_mode: ta.image_mode,
                    sentinels: if ta.on_sentinel_click.is_some() {
                        ta.sentinels.clone()
                    } else {
                        Vec::new()
                    },
                })
            } else {
                None
            }
        }
        _ => None,
    };

    #[cfg(feature = "diff-view")]
    let diff_context_separator_click =
        document_view_diff_context_separator_click(node, x_i16, y_i16).or_else(|| {
            textarea_change
                .as_ref()
                .and_then(|change| text_area_diff_context_separator_click(change, x_i16, y_i16))
        });

    HitActions {
        on_click,
        on_mouse_down,
        on_mouse_up,
        on_drag_start,
        on_drag,
        on_drag_end,
        document_click,
        input_change,
        list_select,
        table_select,
        tabs_change,
        draggable_tab_bar_action,
        border_tabs_change,
        checkbox_toggle,
        progress_change,
        graph_node_click,
        sequence_item_click,
        flowchart_item_click,
        #[cfg(feature = "diff-view")]
        diff_context_separator_click,
        slider_change,
        splitter_grab,
        drag_source_grab,
        textarea_change,
    }
}

pub(crate) fn gather_border_tabs_change(
    _tree: &NodeTree,
    node: &Node,
    x: i16,
    y: i16,
) -> Option<TabsChange> {
    match &node.kind {
        NodeKind::VStack(node_stack) => node_stack.on_tab_change.clone().and_then(|cb| {
            if !node_stack.props.border || node_stack.tab_titles.is_empty() {
                return None;
            }

            let title_w = node.rect.w.saturating_sub(2);
            if title_w == 0 {
                return None;
            }

            let title_rect = Rect {
                x: node.rect.x.saturating_add(1),
                y: node.rect.y,
                w: title_w,
                h: 1,
            };

            if !title_rect.contains(x, y) {
                return None;
            }

            let len = node_stack.tab_titles.len();
            let active = node_stack.active_tab.min(len.saturating_sub(1));

            let mut col = (x as i32).saturating_sub(title_rect.x as i32) as usize;
            if let Some(prefix) = &node_stack.title_prefix {
                let prefix_w = UnicodeWidthStr::width(prefix.as_ref());
                // Subtract prefix and the separator frame character (1 char).
                col = col.saturating_sub(prefix_w).saturating_sub(1);
            }

            let idx = crate::widgets::VStack::border_index_at_col(
                &node_stack.tab_titles,
                active,
                node_stack.tab_variant,
                col,
            )?;
            Some(TabsChange {
                cb,
                next: idx,
                active,
            })
        }),
        NodeKind::Frame(props) => props.on_tab_change.clone().and_then(|cb| {
            if !props.has_border() || props.tab_titles.is_empty() {
                return None;
            }

            let title_w = node.rect.w.saturating_sub(2);
            if title_w == 0 {
                return None;
            }

            let title_rect = Rect {
                x: node.rect.x.saturating_add(1),
                y: node.rect.y,
                w: title_w,
                h: 1,
            };

            if !title_rect.contains(x, y) {
                return None;
            }

            let len = props.tab_titles.len();
            let active = props.active_tab.min(len.saturating_sub(1));

            let mut col = (x as i32).saturating_sub(title_rect.x as i32) as usize;
            // Subtract header padding (left).
            col = col.saturating_sub(props.header_padding.left as usize);

            if let Some(prefix) = &props.title_prefix {
                let prefix_w = prefix.width();
                // Subtract prefix and the separator frame character (1 char).
                col = col.saturating_sub(prefix_w).saturating_sub(1);
            }

            let idx = crate::widgets::VStack::border_index_at_col(
                &props.tab_titles,
                active,
                props.tab_variant,
                col,
            )?;
            Some(TabsChange {
                cb,
                next: idx,
                active,
            })
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{find_ancestor_on_click, gather_hit_actions, resolve_left_click_target};
    use crate::callback::Callback;
    use crate::core::event::{KeyMods, MouseEvent};
    use crate::core::node::{NodeKind, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::style::Rect;
    use crate::widgets::internal::{MouseRegionNode, TextNode};
    use crate::widgets::{Graph, GraphNode};

    fn noop_mouse_cb() -> Callback<MouseEvent> {
        Callback::new(|_| {})
    }

    fn reconcile_graph(graph: Graph) -> NodeTree {
        let root: crate::Element = graph.into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            None,
        );
        tree
    }

    #[test]
    fn focusable_graph_gathers_node_click_without_callbacks() {
        let tree = reconcile_graph(Graph::new().root(GraphNode::new("root")).focusable(true));

        let actions = gather_hit_actions(&tree, tree.root, 0, 0);

        let click = actions
            .graph_node_click
            .expect("focusable graph should gather node target");
        assert_eq!(click.node_id, tree.root);
        assert!(click.cb.is_none());
        assert_eq!(click.event.label.as_ref(), "root");
        assert!(click.event.path.segments().is_empty());
    }

    #[test]
    fn activatable_graph_gathers_node_click_without_pointer_callback() {
        let tree = reconcile_graph(
            Graph::new()
                .root(GraphNode::new("root"))
                .on_node_activate(Callback::new(|_| {})),
        );

        let actions = gather_hit_actions(&tree, tree.root, 0, 0);

        assert!(actions.graph_node_click.is_some());
    }

    #[test]
    fn click_target_prefers_nearest_capturing_mouse_region() {
        let mut tree = NodeTree::new();

        let root = tree.alloc();
        let outer = tree.alloc();
        let inner = tree.alloc();
        let leaf = tree.alloc();

        tree.root = root;

        {
            let node = tree.node_mut(root);
            node.parent = None;
            node.children = vec![outer];
            node.kind = NodeKind::Text(TextNode::default());
        }
        {
            let node = tree.node_mut(outer);
            node.parent = Some(root);
            node.children = vec![inner];
            node.kind = NodeKind::MouseRegion(MouseRegionNode {
                on_click: Some(noop_mouse_cb()),
                on_mouse_down: None,
                bubble_mouse_down: false,
                on_mouse_up: None,
                on_mouse_move: None,
                on_drag_start: None,
                on_drag: None,
                on_drag_end: None,
                drag_required_mods: None,
                on_right_drag_start: None,
                on_right_drag: None,
                on_right_drag_end: None,
                right_drag_required_mods: None,
                on_hover_change: None,
                hit_test: None,
                capture_click: true,
                capture_required_mods: None,
                hover_style: Default::default(),
                hover_effects: Default::default(),
                enabled: true,
            });
        }
        {
            let node = tree.node_mut(inner);
            node.parent = Some(outer);
            node.children = vec![leaf];
            node.kind = NodeKind::MouseRegion(MouseRegionNode {
                on_click: Some(noop_mouse_cb()),
                on_mouse_down: None,
                bubble_mouse_down: false,
                on_mouse_up: None,
                on_mouse_move: None,
                on_drag_start: None,
                on_drag: None,
                on_drag_end: None,
                drag_required_mods: None,
                on_right_drag_start: None,
                on_right_drag: None,
                on_right_drag_end: None,
                right_drag_required_mods: None,
                on_hover_change: None,
                hit_test: None,
                capture_click: true,
                capture_required_mods: None,
                hover_style: Default::default(),
                hover_effects: Default::default(),
                enabled: true,
            });
        }
        {
            let node = tree.node_mut(leaf);
            node.parent = Some(inner);
            node.children = vec![];
            node.kind = NodeKind::Text(TextNode::default());
        }

        assert_eq!(resolve_left_click_target(&tree, leaf, KeyMods::NONE), inner);
    }

    #[test]
    fn click_target_keeps_leaf_when_capture_disabled() {
        let mut tree = NodeTree::new();

        let root = tree.alloc();
        let region = tree.alloc();
        let leaf = tree.alloc();

        tree.root = root;

        {
            let node = tree.node_mut(root);
            node.parent = None;
            node.children = vec![region];
            node.kind = NodeKind::Text(TextNode::default());
        }
        {
            let node = tree.node_mut(region);
            node.parent = Some(root);
            node.children = vec![leaf];
            node.kind = NodeKind::MouseRegion(MouseRegionNode {
                on_click: Some(noop_mouse_cb()),
                on_mouse_down: None,
                bubble_mouse_down: false,
                on_mouse_up: None,
                on_mouse_move: None,
                on_drag_start: None,
                on_drag: None,
                on_drag_end: None,
                drag_required_mods: None,
                on_right_drag_start: None,
                on_right_drag: None,
                on_right_drag_end: None,
                right_drag_required_mods: None,
                on_hover_change: None,
                hit_test: None,
                capture_click: false,
                capture_required_mods: None,
                hover_style: Default::default(),
                hover_effects: Default::default(),
                enabled: true,
            });
        }
        {
            let node = tree.node_mut(leaf);
            node.parent = Some(region);
            node.children = vec![];
            node.kind = NodeKind::Text(TextNode::default());
        }

        assert_eq!(resolve_left_click_target(&tree, leaf, KeyMods::NONE), leaf);
    }

    #[test]
    fn click_target_captures_only_when_required_mods_are_held() {
        let mut tree = NodeTree::new();

        let root = tree.alloc();
        let region = tree.alloc();
        let leaf = tree.alloc();

        tree.root = root;

        {
            let node = tree.node_mut(root);
            node.parent = None;
            node.children = vec![region];
            node.kind = NodeKind::Text(TextNode::default());
        }
        {
            let node = tree.node_mut(region);
            node.parent = Some(root);
            node.children = vec![leaf];
            node.kind = NodeKind::MouseRegion(MouseRegionNode {
                on_click: None,
                on_mouse_down: Some(noop_mouse_cb()),
                bubble_mouse_down: false,
                on_mouse_up: None,
                on_mouse_move: None,
                on_drag_start: None,
                on_drag: None,
                on_drag_end: None,
                drag_required_mods: None,
                on_right_drag_start: None,
                on_right_drag: None,
                on_right_drag_end: None,
                right_drag_required_mods: None,
                on_hover_change: None,
                hit_test: None,
                capture_click: false,
                capture_required_mods: Some(KeyMods::ALT),
                hover_style: Default::default(),
                hover_effects: Default::default(),
                enabled: true,
            });
        }
        {
            let node = tree.node_mut(leaf);
            node.parent = Some(region);
            node.children = vec![];
            node.kind = NodeKind::Text(TextNode::default());
        }

        assert_eq!(resolve_left_click_target(&tree, leaf, KeyMods::NONE), leaf);
        assert_eq!(resolve_left_click_target(&tree, leaf, KeyMods::ALT), region);
        assert_eq!(
            resolve_left_click_target(
                &tree,
                leaf,
                KeyMods {
                    shift: true,
                    ..KeyMods::ALT
                },
            ),
            region,
        );
    }

    /// `find_ancestor_on_click` finds the nearest enabled `MouseRegion`
    /// ancestor with an `on_click` handler.
    #[test]
    fn find_ancestor_on_click_finds_nearest_region() {
        let mut tree = NodeTree::new();

        let root = tree.alloc();
        let outer = tree.alloc();
        let inner = tree.alloc();
        let leaf = tree.alloc();

        tree.root = root;

        {
            let node = tree.node_mut(root);
            node.parent = None;
            node.children = vec![outer];
            node.kind = NodeKind::Text(TextNode::default());
        }
        {
            let node = tree.node_mut(outer);
            node.parent = Some(root);
            node.children = vec![inner];
            // outer has on_click but inner is closer - inner should win
            node.kind = NodeKind::MouseRegion(MouseRegionNode {
                on_click: Some(noop_mouse_cb()),
                on_mouse_down: None,
                bubble_mouse_down: false,
                on_mouse_up: None,
                on_mouse_move: None,
                on_drag_start: None,
                on_drag: None,
                on_drag_end: None,
                drag_required_mods: None,
                on_right_drag_start: None,
                on_right_drag: None,
                on_right_drag_end: None,
                right_drag_required_mods: None,
                on_hover_change: None,
                hit_test: None,
                capture_click: false,
                capture_required_mods: None,
                hover_style: Default::default(),
                hover_effects: Default::default(),
                enabled: true,
            });
        }
        {
            let node = tree.node_mut(inner);
            node.parent = Some(outer);
            node.children = vec![leaf];
            node.kind = NodeKind::MouseRegion(MouseRegionNode {
                on_click: Some(noop_mouse_cb()),
                on_mouse_down: None,
                bubble_mouse_down: false,
                on_mouse_up: None,
                on_mouse_move: None,
                on_drag_start: None,
                on_drag: None,
                on_drag_end: None,
                drag_required_mods: None,
                on_right_drag_start: None,
                on_right_drag: None,
                on_right_drag_end: None,
                right_drag_required_mods: None,
                on_hover_change: None,
                hit_test: None,
                capture_click: false,
                capture_required_mods: None,
                hover_style: Default::default(),
                hover_effects: Default::default(),
                enabled: true,
            });
        }
        {
            let node = tree.node_mut(leaf);
            node.parent = Some(inner);
            node.children = vec![];
            node.kind = NodeKind::Text(TextNode::default());
        }

        // Starting from leaf: nearest ancestor with on_click is inner
        assert!(find_ancestor_on_click(&tree, leaf).is_some());
    }

    /// `find_ancestor_on_click` skips disabled `MouseRegion` nodes.
    #[test]
    fn find_ancestor_on_click_skips_disabled() {
        let mut tree = NodeTree::new();

        let root = tree.alloc();
        let region = tree.alloc();
        let leaf = tree.alloc();

        tree.root = root;

        {
            let node = tree.node_mut(root);
            node.parent = None;
            node.children = vec![region];
            node.kind = NodeKind::Text(TextNode::default());
        }
        {
            let node = tree.node_mut(region);
            node.parent = Some(root);
            node.children = vec![leaf];
            node.kind = NodeKind::MouseRegion(MouseRegionNode {
                on_click: Some(noop_mouse_cb()),
                on_mouse_down: None,
                bubble_mouse_down: false,
                on_mouse_up: None,
                on_mouse_move: None,
                on_drag_start: None,
                on_drag: None,
                on_drag_end: None,
                drag_required_mods: None,
                on_right_drag_start: None,
                on_right_drag: None,
                on_right_drag_end: None,
                right_drag_required_mods: None,
                on_hover_change: None,
                hit_test: None,
                capture_click: false,
                capture_required_mods: None,
                hover_style: Default::default(),
                hover_effects: Default::default(),
                enabled: false, // disabled - should be skipped
            });
        }
        {
            let node = tree.node_mut(leaf);
            node.parent = Some(region);
            node.children = vec![];
            node.kind = NodeKind::Text(TextNode::default());
        }

        assert!(find_ancestor_on_click(&tree, leaf).is_none());
    }
}
