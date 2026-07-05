use ratatui::symbols::merge::MergeStrategy;

use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::{BorderStyle, Edge, Rect, ScrollbarVariant, Style};
use crate::widgets::BorderMergeMode;
use crate::widgets::DecorationPlacement;
use crate::widgets::internal::{FrameGeometry, FrameJoinOverlap, compute_frame_geometry};

use super::RenderState;

pub(crate) fn is_box_drawing_symbol(symbol: &str) -> bool {
    let mut chars = symbol.chars();
    let Some(ch) = chars.next() else {
        return false;
    };
    chars.next().is_none() && (0x2500..=0x257F).contains(&(ch as u32))
}

pub(crate) fn to_merge_strategy(strategy: BorderMergeMode) -> MergeStrategy {
    match strategy {
        BorderMergeMode::Replace => MergeStrategy::Replace,
        BorderMergeMode::Exact => MergeStrategy::Exact,
        BorderMergeMode::Fuzzy => MergeStrategy::Fuzzy,
    }
}

pub(crate) fn scroll_view_clip_rect(
    tree: &NodeTree,
    mut parent_id: Option<NodeId>,
    content: ratatui::layout::Rect,
) -> Option<Rect> {
    let mut clip: Option<Rect> = None;

    while let Some(id) = parent_id {
        let node = tree.node(id);
        if let NodeKind::ScrollView(scroll_view) = &node.kind {
            let mut rect = node.rect;
            rect.x = rect.x.saturating_add(content.x as i16);
            rect.y = rect.y.saturating_add(content.y as i16);

            let mut inner = rect.inner(scroll_view.props.border, scroll_view.props.padding);

            if scroll_view.show_scroll_indicators {
                if scroll_view.top_indicator {
                    inner.y = inner.y.saturating_add(1);
                    inner.h = inner.h.saturating_sub(1);
                }
                if scroll_view.bottom_indicator {
                    inner.h = inner.h.saturating_sub(1);
                }
            }

            let parent_integrated_v = node.parent.is_some_and(|pid| {
                matches!(
                    &tree.node(pid).kind,
                    NodeKind::Frame(props)
                        if (props.has_border()
                            && (props.border_edges.has_left() || props.border_edges.has_right()))
                            || props.decorations.iter().any(|d| {
                                d.placement == DecorationPlacement::Border
                                    && matches!(d.edge, Edge::Left | Edge::Right)
                            })
                )
            });
            let use_integrated = scroll_view.scrollbar
                && matches!(scroll_view.scrollbar_variant, ScrollbarVariant::Integrated)
                && (scroll_view.props.border || parent_integrated_v);
            let use_standalone = scroll_view.scrollbar && !use_integrated;

            if use_standalone && inner.w > 0 {
                inner.w = inner.w.saturating_sub(1);
            }

            clip = Some(match clip {
                Some(existing) => existing.intersection(&inner),
                None => inner,
            });
        }

        parent_id = node.parent;
    }

    clip
}

/// Vertical integrated scrollbar on a parent [`Frame`](crate::widgets::Frame) edge (border or
/// `DecorationPlacement::Border` strip).
#[derive(Clone, Copy)]
pub(crate) struct FrameIntegratedVTrack {
    pub track_x: i16,
    pub track_glyph: Option<char>,
    pub border_style_fallback: BorderStyle,
    pub track_style: Style,
}

/// Horizontal integrated scrollbar on a parent frame edge (border or border-like decoration).
#[derive(Clone, Copy)]
pub(crate) struct FrameIntegratedHTrack {
    pub track_y: i16,
    pub track_glyph: Option<char>,
    pub border_style_fallback: BorderStyle,
    pub track_style: Style,
}

#[derive(Clone, Copy)]
pub(crate) struct ParentFrameIntegratedTracks {
    pub v: Option<FrameIntegratedVTrack>,
    pub h: Option<FrameIntegratedHTrack>,
}

fn frame_parent_hover_active(
    state: &RenderState<'_, '_, '_>,
    parent_id: NodeId,
    props: &crate::widgets::internal::FrameNode,
) -> (bool, bool) {
    let active = state.focus_chain.contains(&parent_id);
    let is_hovered = if props.hover_style().is_some_and(|s| !s.is_empty()) {
        frame_contains_hovered_node(state, parent_id)
    } else {
        Some(parent_id) == state.ctx.hovered
    };
    (active, is_hovered)
}

pub(crate) fn frame_contains_hovered_node(
    state: &RenderState<'_, '_, '_>,
    frame_id: NodeId,
) -> bool {
    state.ctx.hovered.is_some_and(|hovered| {
        hovered == frame_id || state.ctx.tree.is_descendant(frame_id, hovered)
    })
}

fn parent_frame_screen_rect(state: &RenderState<'_, '_, '_>, parent_rect: Rect) -> Rect {
    let mut r = parent_rect;
    r.x = r.x.saturating_add(state.content.x as i16);
    r.y = r.y.saturating_add(state.content.y as i16);
    r
}

/// Shared context for resolving integrated tracks on a single `Frame` node.
struct FrameTrackCtx {
    active: bool,
    is_hovered: bool,
    geometry: FrameGeometry,
    has_border: bool,
    border_style: BorderStyle,
    border_render_style: Style,
}

fn frame_track_ctx<'a>(
    state: &'a RenderState<'_, '_, '_>,
    frame_id: NodeId,
) -> Option<(FrameTrackCtx, &'a crate::widgets::internal::FrameNode)> {
    let parent = state.ctx.tree.node(frame_id);
    let NodeKind::Frame(props) = &parent.kind else {
        return None;
    };
    let (active, is_hovered) = frame_parent_hover_active(state, frame_id, props);
    let screen_rect = parent_frame_screen_rect(state, parent.rect);
    let geometry = compute_frame_geometry(props, screen_rect, FrameJoinOverlap::default(), true);

    let (border_render_style, border_style) = if props.has_border() {
        crate::backend::ratatui_backend::renderers::frame::render::resolve_block_style(
            props, active, is_hovered,
        )
    } else {
        (Style::new(), BorderStyle::Plain)
    };

    Some((
        FrameTrackCtx {
            active,
            is_hovered,
            geometry,
            has_border: props.has_border(),
            border_style,
            border_render_style,
        },
        props,
    ))
}

fn frame_integrated_vtrack_from(
    ctx: &FrameTrackCtx,
    props: &crate::widgets::internal::FrameNode,
) -> Option<FrameIntegratedVTrack> {
    if ctx.has_border {
        return Some(FrameIntegratedVTrack {
            track_x: ctx.geometry.vscrollbar_track_x?,
            track_glyph: None,
            border_style_fallback: ctx.border_style,
            track_style: ctx.border_render_style,
        });
    }

    let deco = props
        .decorations
        .iter()
        .find(|d| d.placement == DecorationPlacement::Border && d.edge == Edge::Right)
        .or_else(|| {
            props
                .decorations
                .iter()
                .find(|d| d.placement == DecorationPlacement::Border && d.edge == Edge::Left)
        })?;

    let track_x = match deco.edge {
        Edge::Right => ctx.geometry.vscrollbar_track_x?,
        Edge::Left => ctx.geometry.vscrollbar_track_x?,
        _ => return None,
    };

    let track_style =
        crate::backend::ratatui_backend::renderers::frame::render::resolve_edge_decoration_style(
            deco,
            ctx.active,
            ctx.is_hovered,
        );
    let glyph = deco.glyph.resolve(deco.edge);

    Some(FrameIntegratedVTrack {
        track_x,
        track_glyph: Some(glyph),
        border_style_fallback: BorderStyle::Plain,
        track_style,
    })
}

fn frame_integrated_htrack_from(
    ctx: &FrameTrackCtx,
    props: &crate::widgets::internal::FrameNode,
) -> Option<FrameIntegratedHTrack> {
    if ctx.has_border {
        return Some(FrameIntegratedHTrack {
            track_y: ctx.geometry.hscrollbar_track_y?,
            track_glyph: None,
            border_style_fallback: ctx.border_style,
            track_style: ctx.border_render_style,
        });
    }

    let deco = props
        .decorations
        .iter()
        .find(|d| d.placement == DecorationPlacement::Border && d.edge == Edge::Bottom)
        .or_else(|| {
            props
                .decorations
                .iter()
                .find(|d| d.placement == DecorationPlacement::Border && d.edge == Edge::Top)
        })?;

    let track_y = match deco.edge {
        Edge::Bottom => ctx.geometry.hscrollbar_track_y?,
        Edge::Top => ctx.geometry.hscrollbar_track_y?,
        _ => return None,
    };

    let track_style =
        crate::backend::ratatui_backend::renderers::frame::render::resolve_edge_decoration_style(
            deco,
            ctx.active,
            ctx.is_hovered,
        );
    let glyph = deco.glyph.resolve(deco.edge);

    Some(FrameIntegratedHTrack {
        track_y,
        track_glyph: Some(glyph),
        border_style_fallback: BorderStyle::Plain,
        track_style,
    })
}

fn frame_integrated_vtrack_at(
    state: &RenderState<'_, '_, '_>,
    frame_id: NodeId,
) -> Option<FrameIntegratedVTrack> {
    let (ctx, props) = frame_track_ctx(state, frame_id)?;
    frame_integrated_vtrack_from(&ctx, props)
}

fn frame_integrated_tracks_at(
    state: &RenderState<'_, '_, '_>,
    frame_id: NodeId,
) -> Option<ParentFrameIntegratedTracks> {
    let (ctx, props) = frame_track_ctx(state, frame_id)?;
    let v = frame_integrated_vtrack_from(&ctx, props);
    let h = frame_integrated_htrack_from(&ctx, props);
    if v.is_none() && h.is_none() {
        return None;
    }
    Some(ParentFrameIntegratedTracks { v, h })
}

/// Vertical integrated track: walk from `start` (typically a widget's `parent` id) up to the root
/// and use the first ancestor [`Frame`](crate::widgets::Frame) that exposes a right/left edge.
pub(crate) fn ancestor_frame_integrated_vtrack(
    state: &RenderState<'_, '_, '_>,
    mut cur: Option<NodeId>,
) -> Option<FrameIntegratedVTrack> {
    while let Some(id) = cur {
        if let Some(t) = frame_integrated_vtrack_at(state, id) {
            return Some(t);
        }
        cur = state.ctx.tree.node(id).parent;
    }
    None
}

/// Integrated tracks for [`TextArea`](crate::widgets::TextArea): first ancestor `Frame` that has any
/// integrated edge (border or border-like decoration).
pub(crate) fn ancestor_frame_integrated_tracks(
    state: &RenderState<'_, '_, '_>,
    mut cur: Option<NodeId>,
) -> Option<ParentFrameIntegratedTracks> {
    while let Some(id) = cur {
        if let Some(t) = frame_integrated_tracks_at(state, id) {
            return Some(t);
        }
        cur = state.ctx.tree.node(id).parent;
    }
    None
}

fn rect_right(rect: Rect) -> i16 {
    rect.x.saturating_add(rect.w as i16).saturating_sub(1)
}

fn rect_bottom(rect: Rect) -> i16 {
    rect.y.saturating_add(rect.h as i16).saturating_sub(1)
}

fn spans_overlap(a_start: i16, a_end: i16, b_start: i16, b_end: i16) -> bool {
    a_start <= b_end && b_start <= a_end
}

fn frame_border_rect(node_rect: Rect, props: &crate::widgets::internal::FrameNode) -> Rect {
    compute_frame_geometry(props, node_rect, FrameJoinOverlap::default(), true).frame_rect
}

/// Pre-collected join-eligible rectangles for O(1) adjacency lookups.
///
/// Built once per render pass to avoid the O(F*N) full-tree scan that the
/// previous `frame_join_overlap` performed for every join-enabled Frame.
#[derive(Default)]
pub(crate) struct JoinIndex {
    /// Join-eligible rects sorted by right-edge x-coordinate.
    by_right_edge: Vec<(NodeId, Rect)>,
    /// Join-eligible rects sorted by bottom-edge y-coordinate.
    by_bottom_edge: Vec<(NodeId, Rect)>,
}

pub(crate) fn build_join_index(tree: &NodeTree) -> JoinIndex {
    let mut eligible: Vec<(NodeId, Rect)> = Vec::new();

    for node in tree.iter() {
        let rect = match &node.kind {
            NodeKind::Frame(props) => {
                if !props.join_frame || !props.has_border() {
                    continue;
                }
                let r = frame_border_rect(node.rect, props);
                if r.is_empty() {
                    continue;
                }
                r
            }
            NodeKind::HStack(_) | NodeKind::VStack(_) | NodeKind::Grid(_) | NodeKind::Flow(_) => {
                if node.rect.is_empty() {
                    continue;
                }
                node.rect
            }
            _ => continue,
        };
        eligible.push((node.id, rect));
    }

    let mut by_right_edge = eligible.clone();
    by_right_edge.sort_by_key(|(_, r)| rect_right(*r));

    let mut by_bottom_edge = eligible;
    by_bottom_edge.sort_by_key(|(_, r)| rect_bottom(*r));

    JoinIndex {
        by_right_edge,
        by_bottom_edge,
    }
}

pub(crate) fn frame_join_overlap_indexed(
    join_index: &JoinIndex,
    node_id: NodeId,
    props: &crate::widgets::internal::FrameNode,
    node_rect: Rect,
) -> FrameJoinOverlap {
    if !props.join_frame || !props.has_border() {
        return FrameJoinOverlap::default();
    }

    let self_rect = frame_border_rect(node_rect, props);
    if self_rect.is_empty() {
        return FrameJoinOverlap::default();
    }

    let self_left = self_rect.x;
    let self_top = self_rect.y;
    let self_right = rect_right(self_rect);
    let self_bottom = rect_bottom(self_rect);

    let mut overlap = FrameJoinOverlap::default();

    // Check left adjacency: find nodes whose right_edge == self_left - 1
    let target_right = self_left.saturating_sub(1);
    let start = join_index
        .by_right_edge
        .partition_point(|(_, r)| rect_right(*r) < target_right);
    for &(id, r) in &join_index.by_right_edge[start..] {
        if rect_right(r) != target_right {
            break;
        }
        if id == node_id {
            continue;
        }
        if spans_overlap(r.y, rect_bottom(r), self_top, self_bottom) {
            overlap.left = true;
            break;
        }
    }

    // Check top adjacency: find nodes whose bottom_edge == self_top - 1
    let target_bottom = self_top.saturating_sub(1);
    let start = join_index
        .by_bottom_edge
        .partition_point(|(_, r)| rect_bottom(*r) < target_bottom);
    for &(id, r) in &join_index.by_bottom_edge[start..] {
        if rect_bottom(r) != target_bottom {
            break;
        }
        if id == node_id {
            continue;
        }
        if spans_overlap(r.x, rect_right(r), self_left, self_right) {
            overlap.top = true;
            break;
        }
    }

    overlap
}
