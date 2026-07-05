use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use ratatui::buffer::Cell as BufferCell;
use ratatui::layout::Position;
use ratatui::style::Color as RColor;
use ratatui::widgets::{Block, Clear};

use crate::app::ContrastPolicy;
use crate::app::copy_feedback::CopyFeedbackState;
use crate::backend::ratatui_backend::common::{
    apply_effect_style_clipped, apply_visual_effects_clipped, current_render_screen_background,
    from_ratatui_color, push_render_terminal_bg, render_placeholder_frame, to_ratatui_rect,
    to_ratatui_style,
};
use crate::backend::ratatui_backend::glyph_paint_cache::{ActiveMemoGuard, PaintGlyphCaches};
#[cfg(feature = "big-text")]
use crate::backend::ratatui_backend::renderers::big_text::render_big_text;
#[cfg(feature = "image")]
use crate::backend::ratatui_backend::renderers::image::{
    render_image, render_image_inline_fallback,
};
#[cfg(feature = "terminal")]
use crate::backend::ratatui_backend::renderers::terminal::render_terminal_node;
use crate::backend::ratatui_backend::renderers::{
    animated::render_animated,
    ascii_canvas::render_ascii_canvas,
    button::render_button_node,
    chart::render_chart,
    checkbox::render_checkbox_node,
    class_diagram::render_class_diagram,
    divider::render_divider_node,
    document_view::{DocumentViewRenderCtx, render_document_view},
    draggable_tab_bar::render_draggable_tab_bar_node,
    er_diagram::render_er_diagram,
    flowchart::render_flowchart,
    frame::{FrameRenderCtx, render_frame},
    gantt_diagram::render_gantt_diagram,
    graph::render_graph,
    heatmap::render_heatmap,
    hex_area::render_hex_area_node,
    input::render_input_node,
    list::render_list_node,
    mouse_region::render_mouse_region,
    progress::render_progress_bar_node,
    scroll_view::render_scroll_view_node,
    sequence_diagram::render_sequence_diagram,
    slider::render_slider_node,
    sparkline::render_sparkline,
    spinner::{SpinnerRenderCtx, render_spinner},
    splitter::render_splitter_node,
    stack::{
        GridRenderCtx, VStackRenderCtx, render_grid, render_hstack, render_vstack,
        render_zstack_center,
    },
    state_diagram::render_state_diagram,
    table::render_table_node,
    tabs::render_tabs_node,
    text::render_text,
    text_area::render_text_area_node,
};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::resolve::{resolve_base_style, resolve_force_accent_style};
use crate::style::{ColorTransform, Rect, Style, StyleSlot, ThemeRole, resolve_slot};
use crate::utils::scrollbar::ScrollbarMetricsCache;
use crate::widgets::internal::{FrameGeometry, StackProps, compute_frame_geometry};
use crate::widgets::{DragSlot, DropHighlight};

pub(crate) type EffectPhase = u64;

/// Bundles render state that needs to be passed through the render tree.
pub(crate) struct RenderContext<'a> {
    pub tree: &'a NodeTree,
    pub focused: Option<NodeId>,
    pub hovered: Option<NodeId>,
    pub mouse_pos: Option<(u16, u16)>,
    /// When set, List/Table skip pointer-derived `item_hover_style` for these node ids.
    pub suppress_pointer_item_hover_nodes: Option<&'a std::collections::HashSet<NodeId>>,
    pub blink_visible: bool,
    pub effect_phase: EffectPhase,
    pub copy_feedback: Option<&'a CopyFeedbackState>,
    pub copy_feedback_style: Style,
    #[cfg_attr(not(feature = "image"), allow(dead_code))]
    pub images_enabled: bool,
    pub contrast_policy: ContrastPolicy,
    pub read_only_selection: Option<&'a std::collections::HashMap<NodeId, (usize, Option<usize>)>>,
    /// Cache for scrollbar metrics to avoid recomputation.
    /// Wrapped in RefCell to allow mutation through immutable reference.
    /// Borrowed from a persistent field on `AppRunner` so entries survive
    /// across paint-only frames.
    pub scrollbar_metrics_cache: &'a RefCell<ScrollbarMetricsCache>,
    /// Reusable flat buffer for overlay cell snapshots.
    pub overlay_bg_snapshot: &'a RefCell<Vec<BufferCell>>,
    /// Pre-built index for O(log N) frame-join overlap lookups.
    pub join_index: &'a JoinIndex,
    /// Cursor position requested by renderers during the current pass.
    pub cursor_position: &'a Cell<Option<Position>>,
    /// Resolved terminal background for opacity blending through Reset.
    pub terminal_bg: Option<ratatui::style::Color>,
    /// Active drag preview label, if generic drag-and-drop is in progress.
    pub drag_preview_label: Option<&'a str>,
    /// Render the active drag preview with its top-left cell at the mouse position.
    pub drag_preview_at_mouse: bool,
    /// Source widget rect for a `DragPreview::SourceSnapshot` drag preview.
    /// When set, the cells at this rect are copied near the cursor after rendering.
    pub drag_preview_snapshot_rect: Option<ratatui::layout::Rect>,
    /// Filled on the first paint after the drag source is fully rendered; used when the
    /// source subtree is collapsed while the float preview still shows the card.
    pub dnd_snapshot_cells: &'a RefCell<Option<(u16, u16, Vec<ratatui::buffer::Cell>)>>,
    /// Max dimensions for the floating `SourceSnapshot` preview (`None` = framework defaults).
    pub drag_preview_max_width: Option<u16>,
    pub drag_preview_max_height: Option<u16>,
    /// When `Some`, a `DropTarget` with `DropSlot::SourcePreview` is hovered: render the snapshot
    /// at this rect instead of near the cursor, and suppress the cursor float.
    pub drop_slot_source_preview_rect: Option<ratatui::layout::Rect>,
    /// Slab reused each draw: glyphs + readable-fg memo for WCAG/APCA contrast (`None` disables).
    pub paint_glyph_caches: Option<Rc<RefCell<PaintGlyphCaches>>>,
}

pub(crate) fn apply_copy_feedback_to_selection_style(
    ctx: &RenderContext<'_>,
    node_id: NodeId,
    selection_style: Style,
) -> Style {
    if ctx
        .copy_feedback
        .is_some_and(|feedback| feedback.is_active(node_id))
    {
        selection_style.patch(ctx.copy_feedback_style)
    } else {
        selection_style
    }
}

/// Helper struct to pass common render parameters.
pub(crate) struct RenderState<'a, 'b, 'c> {
    pub(crate) f: &'a mut ratatui::Frame<'b>,
    pub(crate) ctx: &'c RenderContext<'c>,
    pub(crate) focus_chain: &'c [NodeId],
    pub(crate) content: ratatui::layout::Rect,
}

fn build_focus_chain(tree: &NodeTree, focused: Option<NodeId>) -> Vec<NodeId> {
    let mut focus_chain: Vec<NodeId> = Vec::with_capacity(16);
    if let Some(mut id) = focused {
        while tree.is_valid(id) {
            focus_chain.push(id);
            let Some(parent) = tree.node(id).parent else {
                break;
            };
            id = parent;
        }
    }
    focus_chain
}

pub(crate) fn render(f: &mut ratatui::Frame<'_>, ctx: &RenderContext<'_>) {
    let _terminal_bg_scope = push_render_terminal_bg(ctx.terminal_bg);
    if let Some(caches_cell) = ctx.paint_glyph_caches.as_ref() {
        caches_cell.borrow_mut().clear();
    }
    let _memo_guard = ActiveMemoGuard::install(ctx.paint_glyph_caches.clone());
    let tree = ctx.tree;
    let focused = ctx.focused;
    let size = f.area();

    // Paint the opt-in root viewport background first, so every cell the UI tree
    // does not explicitly style still reads as the configured surface instead of
    // the host terminal color.
    if let Some(bg) = current_render_screen_background() {
        f.render_widget(Block::default().style(bg), size);
    }

    let content = size;

    // Use Vec instead of HashSet - focus chains are typically < 16 nodes deep,
    // so linear .contains() is faster than hashing overhead.
    let focus_chain = build_focus_chain(tree, focused);

    let overlay_nodes = collect_overlay_nodes(tree);
    let content_rect = crate::style::Rect {
        x: content.x as i16,
        y: content.y as i16,
        w: content.width,
        h: content.height,
    };

    let mut state = RenderState {
        f,
        ctx,
        focus_chain: &focus_chain,
        content,
    };

    let extra_root = extra_root_wrapper_children(tree);
    let initial_root = extra_root.map_or(tree.root, |(base, _extra)| base);

    if tree.is_valid(initial_root) && !overlay_nodes.contains(&initial_root) {
        render_subtree(
            &mut state,
            initial_root,
            Some(content_rect),
            RenderOffset::ZERO,
        );
    }

    for overlay in tree.overlay_roots() {
        if !tree.is_valid(overlay.id) {
            continue;
        }
        let overlay_opacity = overlay.opacity.clamp(0.0, 1.0);
        let restore_mode = overlay_clear_restore_mode(tree.node(overlay.id));
        if let Some(style) = overlay.backdrop {
            render_overlay_backdrop(&mut state, content_rect, style, overlay_opacity);
        }
        let overlay_node = tree.node(overlay.id);
        let overlay_offset = if let NodeKind::Animated(animated) = &overlay_node.kind {
            RenderOffset::ZERO.add_cells(animated.visual_position_offset_cells())
        } else {
            RenderOffset::ZERO
        };
        let overlay_rect = overlay_offset.apply_to_rect(overlay_node.rect);
        let clear_rect = clip_overlay_clear_rect(content_rect, overlay_rect);

        // Snapshot all cells in the overlay area before clearing. After drawing
        // the overlay content, any untouched cells are restored so transparent
        // overlay regions keep the underlying glyphs and colors.
        if clear_rect.width > 0 && clear_rect.height > 0 {
            {
                let mut bg_snapshot = state.ctx.overlay_bg_snapshot.borrow_mut();
                let area_len = clear_rect.width as usize * clear_rect.height as usize;
                bg_snapshot.clear();
                bg_snapshot.resize(area_len, BufferCell::EMPTY);

                let buf = state.f.buffer_mut();
                for dy in 0..clear_rect.height {
                    for dx in 0..clear_rect.width {
                        let index = dy as usize * clear_rect.width as usize + dx as usize;
                        bg_snapshot[index] = buf
                            .cell((clear_rect.x + dx, clear_rect.y + dy))
                            .cloned()
                            .unwrap_or(BufferCell::EMPTY);
                    }
                }
            }

            state.f.render_widget(Clear, clear_rect);
        }
        render_subtree(
            &mut state,
            overlay.id,
            Some(content_rect),
            RenderOffset::ZERO,
        );
        apply_overlay_ancestor_effect_scopes(&mut state, overlay.id, overlay_rect, content_rect);

        // Restore any cell left untouched after the clear so transparent
        // overlays inherit the content already rendered beneath them.
        if clear_rect.width > 0 && clear_rect.height > 0 {
            {
                let bg_snapshot = state.ctx.overlay_bg_snapshot.borrow();
                let buf = state.f.buffer_mut();
                for dy in 0..clear_rect.height {
                    for dx in 0..clear_rect.width {
                        let saved_bg =
                            &bg_snapshot[dy as usize * clear_rect.width as usize + dx as usize];
                        let cx = clear_rect.x + dx;
                        let cy = clear_rect.y + dy;
                        if let Some(cell) = buf.cell_mut((cx, cy)) {
                            if is_clear_equivalent(cell) {
                                match restore_mode {
                                    OverlayClearRestoreMode::PreserveForeground => {
                                        *cell = saved_bg.clone();
                                    }
                                    OverlayClearRestoreMode::PreserveBackgroundOnly => {
                                        cell.bg = saved_bg.bg;
                                    }
                                }
                                continue;
                            }

                            if cell.bg == RColor::Reset {
                                cell.bg = saved_bg.bg;
                            }
                            if restore_mode == OverlayClearRestoreMode::PreserveForeground {
                                if cell.fg == RColor::Reset {
                                    cell.fg = saved_bg.fg;
                                }
                                if cell.underline_color == RColor::Reset {
                                    cell.underline_color = saved_bg.underline_color;
                                }
                            }
                        }
                    }
                }
            }
        }

        if overlay.copy_feedback_active {
            let feedback_rect = Rect {
                x: clear_rect.x as i16,
                y: clear_rect.y as i16,
                w: clear_rect.width,
                h: clear_rect.height,
            };
            apply_effect_style_clipped(
                state.f,
                feedback_rect,
                state.ctx.copy_feedback_style,
                Some(content_rect),
                state.ctx.terminal_bg,
            );
        }

        if overlay_opacity < 1.0 {
            let opacity_style = Style::new()
                .transform_fg(ColorTransform::Opacity(overlay_opacity))
                .transform_bg(ColorTransform::Opacity(overlay_opacity));
            let opacity_rect = Rect {
                x: clear_rect.x as i16,
                y: clear_rect.y as i16,
                w: clear_rect.width,
                h: clear_rect.height,
            };
            apply_effect_style_clipped(
                state.f,
                opacity_rect,
                opacity_style,
                Some(content_rect),
                state.ctx.terminal_bg,
            );
        }
    }

    if let (Some(label), Some((mx, my))) = (ctx.drag_preview_label, ctx.mouse_pos) {
        render_drag_preview(&mut state, ctx, label, mx, my);
    }

    if let Some(src_rect) = ctx.drag_preview_snapshot_rect {
        if let Some(target_rect) = ctx.drop_slot_source_preview_rect {
            render_drag_snapshot_at_target(&mut state, ctx, src_rect, target_rect);
        } else if let Some((mx, my)) = ctx.mouse_pos {
            render_drag_snapshot_preview(&mut state, ctx, src_rect, mx, my);
        }
    }

    // The synthetic extra root hosts framework chrome such as DevTools. Render it
    // after app overlays/backdrops/effect scopes so app-level visual treatments
    // cannot dim or cover the DevTools panel. Input hit-testing still uses the
    // reconciled tree order and the wrapper remains passthrough.
    if let Some((_base, extra)) = extra_root
        && tree.is_valid(extra)
        && !overlay_nodes.contains(&extra)
    {
        render_subtree(&mut state, extra, Some(content_rect), RenderOffset::ZERO);
    }
}

fn extra_root_wrapper_children(tree: &NodeTree) -> Option<(NodeId, NodeId)> {
    if !tree.is_valid(tree.root) {
        return None;
    }
    let root = tree.node(tree.root);
    if root
        .key
        .as_ref()
        .is_none_or(|key| key.as_ref() != crate::runtime::EXTRA_ROOT_WRAPPER_KEY)
    {
        return None;
    }
    if !matches!(root.kind, NodeKind::ZStack(_)) || root.children.len() != 2 {
        return None;
    }
    Some((root.children[0], root.children[1]))
}

pub(crate) fn render_regions(
    f: &mut ratatui::Frame<'_>,
    ctx: &RenderContext<'_>,
    regions: &[Rect],
) {
    let tree = ctx.tree;
    let content = f.area();
    let focus_chain = build_focus_chain(tree, ctx.focused);

    let mut state = RenderState {
        f,
        ctx,
        focus_chain: &focus_chain,
        content,
    };

    if !tree.is_valid(tree.root) || !tree.overlay_roots().is_empty() {
        render(state.f, ctx);
        return;
    }

    for &region in regions {
        if region.is_empty() {
            continue;
        }
        render_subtree(&mut state, tree.root, Some(region), RenderOffset::ZERO);
    }
}

fn collect_overlay_nodes(tree: &NodeTree) -> HashSet<NodeId> {
    let mut overlay_nodes = HashSet::new();
    for overlay in tree.overlay_roots() {
        collect_overlay_subtree(tree, overlay.id, &mut overlay_nodes);
    }
    overlay_nodes
}

fn collect_overlay_subtree(tree: &NodeTree, root: NodeId, out: &mut HashSet<NodeId>) {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !tree.is_valid(id) {
            continue;
        }
        if !out.insert(id) {
            continue;
        }
        let node = tree.node(id);
        for &child in node.children.iter().rev() {
            if tree.is_valid(child) {
                stack.push(child);
            }
        }
    }
}

fn apply_overlay_ancestor_effect_scopes(
    state: &mut RenderState<'_, '_, '_>,
    overlay_root: NodeId,
    overlay_rect: Rect,
    content_rect: Rect,
) {
    let tree = state.ctx.tree;
    let mut parent = tree.node(overlay_root).parent;
    let mut rect = overlay_rect;
    rect.x = rect.x.saturating_add(state.content.x as i16);
    rect.y = rect.y.saturating_add(state.content.y as i16);

    while let Some(id) = parent {
        if !tree.is_valid(id) {
            break;
        }

        let node = tree.node(id);
        if let NodeKind::EffectScope(scope) = &node.kind
            && !scope.effects.is_empty()
        {
            apply_visual_effects_clipped(
                state.f,
                rect,
                &scope.effects,
                state.ctx.effect_phase,
                Some(content_rect),
                state.ctx.terminal_bg,
            );
        }

        parent = node.parent;
    }
}

fn render_subtree(
    state: &mut RenderState<'_, '_, '_>,
    root: NodeId,
    clip_rect: Option<crate::style::Rect>,
    offset: RenderOffset,
) {
    let tree = state.ctx.tree;
    enum RenderStackItem {
        Node(NodeId, Option<crate::style::Rect>, RenderOffset),
        EffectScopePost(
            NodeId,
            Option<crate::style::Rect>,
            RenderOffset,
            EffectPhase,
        ),
        AnimatedPost(
            NodeId,
            Option<crate::style::Rect>,
            RenderOffset,
            Option<AnimatedRestoreSnapshot>,
        ),
        SplitterPost(NodeId, Option<crate::style::Rect>, RenderOffset),
        DropTargetPost(NodeId, Option<crate::style::Rect>, RenderOffset),
        MouseRegionPost(NodeId, Option<crate::style::Rect>, RenderOffset),
    }

    let mut stack: Vec<RenderStackItem> = Vec::new();
    if tree.is_valid(root) {
        stack.push(RenderStackItem::Node(root, clip_rect, offset));
    }

    while let Some(item) = stack.pop() {
        match item {
            RenderStackItem::Node(id, current_clip, inherited_offset) => {
                if !tree.is_valid(id) {
                    continue;
                }
                let node = tree.node(id);
                let (node_offset, node_clip) = if let NodeKind::Animated(animated) = &node.kind {
                    let delta = animated.visual_position_offset_cells();
                    (
                        inherited_offset.add_cells(delta),
                        translate_clip(current_clip, delta),
                    )
                } else {
                    (inherited_offset, current_clip)
                };
                let (
                    child_clip,
                    defer_effect_scope_render,
                    defer_animated_render,
                    defer_splitter_render,
                    defer_drop_overlay,
                    defer_mouse_region_post,
                ) = render_node(state, node, node_clip, node_offset);
                if defer_effect_scope_render {
                    stack.push(RenderStackItem::EffectScopePost(
                        id,
                        node_clip,
                        node_offset,
                        state.ctx.effect_phase,
                    ));
                }
                let animated_restore_snapshot = if let NodeKind::Animated(animated) = &node.kind {
                    if animated.opacity <= f32::EPSILON
                        || (animated.opacity < 1.0 && animated.opacity_target.is_none())
                    {
                        let mut rect = node_offset.apply_to_rect(node.rect);
                        rect.h = animated
                            .prev_height
                            .or(animated.target_height)
                            .unwrap_or(rect.h)
                            .min(rect.h);
                        if rect.h == 0 {
                            None
                        } else {
                            rect.x = rect.x.saturating_add(state.content.x as i16);
                            rect.y = rect.y.saturating_add(state.content.y as i16);
                            snapshot_animated_restore_rect(state.f, rect, node_clip)
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                if defer_animated_render {
                    stack.push(RenderStackItem::AnimatedPost(
                        id,
                        node_clip,
                        node_offset,
                        animated_restore_snapshot,
                    ));
                }
                if defer_splitter_render {
                    stack.push(RenderStackItem::SplitterPost(id, node_clip, node_offset));
                }
                if defer_drop_overlay {
                    stack.push(RenderStackItem::DropTargetPost(id, node_clip, node_offset));
                }
                if defer_mouse_region_post {
                    stack.push(RenderStackItem::MouseRegionPost(id, node_clip, node_offset));
                }
                for &child in node.children.iter().rev() {
                    if tree.is_valid(child) {
                        stack.push(RenderStackItem::Node(child, child_clip, node_offset));
                    }
                }
            }
            RenderStackItem::EffectScopePost(id, current_clip, node_offset, effect_phase) => {
                if !tree.is_valid(id) {
                    continue;
                }
                let node = tree.node(id);
                let NodeKind::EffectScope(scope) = &node.kind else {
                    continue;
                };
                let mut rect = node_offset.apply_to_rect(node.rect);
                rect.x = rect.x.saturating_add(state.content.x as i16);
                rect.y = rect.y.saturating_add(state.content.y as i16);
                apply_visual_effects_clipped(
                    state.f,
                    rect,
                    &scope.effects,
                    effect_phase,
                    current_clip,
                    state.ctx.terminal_bg,
                );
            }
            RenderStackItem::AnimatedPost(id, current_clip, node_offset, restore_snapshot) => {
                if !tree.is_valid(id) {
                    continue;
                }
                let node = tree.node(id);
                let NodeKind::Animated(animated) = &node.kind else {
                    continue;
                };
                let mut rect = node_offset.apply_to_rect(node.rect);
                rect.h = animated
                    .prev_height
                    .or(animated.target_height)
                    .unwrap_or(rect.h)
                    .min(rect.h);
                if rect.h == 0 {
                    continue;
                }
                rect.x = rect.x.saturating_add(state.content.x as i16);
                rect.y = rect.y.saturating_add(state.content.y as i16);
                render_animated(
                    state.f,
                    animated,
                    rect,
                    current_clip,
                    restore_snapshot.as_ref(),
                    state.ctx.terminal_bg,
                );
                if animated.opacity <= f32::EPSILON
                    && let Some(snapshot) = restore_snapshot
                {
                    restore_fully_transparent_animated(state.f, snapshot, animated.opacity_fg_only);
                }
            }
            RenderStackItem::SplitterPost(id, current_clip, node_offset) => {
                if !tree.is_valid(id) {
                    continue;
                }
                let node = tree.node(id);
                let NodeKind::Splitter(splitter) = &node.kind else {
                    continue;
                };
                let mut rect = node_offset.apply_to_rect(node.rect);
                rect.x = rect.x.saturating_add(state.content.x as i16);
                rect.y = rect.y.saturating_add(state.content.y as i16);
                render_splitter_node(state, id, splitter, rect, current_clip);
            }
            RenderStackItem::DropTargetPost(id, current_clip, node_offset) => {
                if !tree.is_valid(id) {
                    continue;
                }
                let node = tree.node(id);
                let NodeKind::DropTarget(target) = &node.kind else {
                    continue;
                };
                if !target.dnd_highlighted || target.highlight != DropHighlight::Overlay {
                    continue;
                }
                let mut rect = node_offset.apply_to_rect(node.rect);
                rect.x = rect.x.saturating_add(state.content.x as i16);
                rect.y = rect.y.saturating_add(state.content.y as i16);
                let (_clipped, rrect) = if let Some(clip) = current_clip {
                    let clipped = rect.intersection(&clip);
                    if clipped.is_empty() {
                        continue;
                    }
                    (clipped, to_ratatui_rect(clipped))
                } else {
                    (rect, to_ratatui_rect(rect))
                };
                let highlight_style = resolve_slot(
                    node.active_theme(),
                    ThemeRole::DropTargetActive,
                    &target.highlight_style,
                );
                if !highlight_style.is_empty() {
                    state.f.render_widget(
                        Block::default().style(to_ratatui_style(highlight_style)),
                        rrect,
                    );
                }
            }
            RenderStackItem::MouseRegionPost(id, current_clip, node_offset) => {
                if !tree.is_valid(id) {
                    continue;
                }
                let node = tree.node(id);
                let NodeKind::MouseRegion(region) = &node.kind else {
                    continue;
                };
                let mut rect = node_offset.apply_to_rect(node.rect);
                rect.x = rect.x.saturating_add(state.content.x as i16);
                rect.y = rect.y.saturating_add(state.content.y as i16);
                apply_visual_effects_clipped(
                    state.f,
                    rect,
                    &region.hover_effects,
                    state.ctx.effect_phase,
                    current_clip,
                    state.ctx.terminal_bg,
                );
            }
        }
    }
}

fn render_node(
    state: &mut RenderState<'_, '_, '_>,
    node: &crate::core::node::Node,
    clip_rect: Option<crate::style::Rect>,
    offset: RenderOffset,
) -> (Option<crate::style::Rect>, bool, bool, bool, bool, bool) {
    let mut rect = offset.apply_to_rect(node.rect);
    rect.x = rect.x.saturating_add(state.content.x as i16);
    rect.y = rect.y.saturating_add(state.content.y as i16);

    let (_clipped_rect, rrect) = if let Some(clip) = clip_rect {
        let clipped = rect.intersection(&clip);
        if clipped.is_empty() {
            return (Some(clipped), false, false, false, false, false);
        }
        (clipped, to_ratatui_rect(clipped))
    } else {
        (rect, to_ratatui_rect(rect))
    };

    let clip_bounds = clip_rect;
    let mut child_clip = clip_rect;
    let mut defer_effect_scope_render = false;
    let mut defer_animated_render = false;
    let mut defer_splitter_render = false;
    let mut defer_drop_target_overlay = false;
    let mut defer_mouse_region_post = false;
    let intersect_clip =
        |target: crate::style::Rect| clip_rect.map(|c| c.intersection(&target)).unwrap_or(target);

    let node_id = node.id;
    let active_theme = node.active_theme();
    match &node.kind {
        NodeKind::Frame(props) => {
            let join_overlap =
                frame_join_overlap_indexed(state.ctx.join_index, node.id, props, node.rect);
            let geometry = compute_frame_geometry(props, rect, join_overlap, true);
            render_frame_node(state, node, props, &geometry, clip_bounds);
            let mut inner_clip = geometry.content_rect;
            if let Some(header_rect) = geometry.header_rect {
                let x1 = inner_clip.x.min(header_rect.x);
                let y1 = inner_clip.y.min(header_rect.y);
                let x2 = (inner_clip.x.saturating_add(inner_clip.w as i16))
                    .max(header_rect.x.saturating_add(header_rect.w as i16));
                let y2 = (inner_clip.y.saturating_add(inner_clip.h as i16))
                    .max(header_rect.y.saturating_add(header_rect.h as i16));
                inner_clip = Rect {
                    x: x1,
                    y: y1,
                    w: x2.saturating_sub(x1).max(0) as u16,
                    h: y2.saturating_sub(y1).max(0) as u16,
                };
            }
            child_clip = Some(intersect_clip(inner_clip));
        }
        NodeKind::VStack(node) => {
            render_vstack_node(
                state.f,
                node,
                active_theme,
                rect,
                rrect,
                clip_bounds,
                state.ctx.terminal_bg.map(from_ratatui_color),
            );
            let inner = rect.inner(node.props.border, node.props.padding);
            child_clip = Some(intersect_clip(inner));
        }
        NodeKind::HStack(node) => {
            render_hstack_node(
                state.f,
                node,
                active_theme,
                rect,
                rrect,
                clip_bounds,
                state.ctx.terminal_bg.map(from_ratatui_color),
            );
            let inner = rect.inner(node.props.border, node.props.padding);
            child_clip = Some(intersect_clip(inner));
        }
        NodeKind::Grid(node) => {
            let mut props = node.props.clone();
            props.style = resolve_base_style(active_theme, props.style);
            render_grid(
                state.f,
                &props,
                rect,
                rrect,
                GridRenderCtx {
                    clip_rect: clip_bounds,
                    terminal_bg: state.ctx.terminal_bg.map(from_ratatui_color),
                },
            );
            let inner = rect.inner(node.props.border, node.props.padding);
            child_clip = Some(intersect_clip(inner));
        }
        NodeKind::Flow(node) => {
            render_hstack(
                state.f,
                &StackProps {
                    style: resolve_base_style(active_theme, node.style),
                    padding: node.padding,
                    border: node.border,
                    border_style: node.border_style,
                    ..StackProps::default()
                },
                rect,
                rrect,
                clip_bounds,
                state.ctx.terminal_bg.map(from_ratatui_color),
            );
            let inner = rect.inner(node.border, node.padding);
            child_clip = Some(intersect_clip(inner));
        }
        NodeKind::Canvas(node) => {
            render_zstack_center(
                state.f,
                &resolve_base_style(active_theme, node.style),
                rect,
                rrect,
                clip_bounds,
                state.ctx.terminal_bg,
            );
            child_clip = Some(intersect_clip(rect));
        }
        NodeKind::Center(node) => {
            render_zstack_center(
                state.f,
                &resolve_base_style(active_theme, node.style),
                rect,
                rrect,
                clip_bounds,
                state.ctx.terminal_bg,
            );
            child_clip = Some(intersect_clip(rect));
        }
        NodeKind::EffectScope(scope) => {
            child_clip = Some(intersect_clip(rect));
            defer_effect_scope_render = !scope.effects.is_empty();
        }
        NodeKind::Animated(animated) => {
            child_clip = Some(intersect_clip(rect));
            let color_animated = animated.current_fg != animated.target_fg
                || animated.fg_anim.is_some()
                || animated.current_fg.is_some()
                || animated.current_bg != animated.target_bg
                || animated.bg_anim.is_some()
                || animated.current_bg.is_some();
            defer_animated_render = animated.opacity < 1.0 || color_animated;
        }
        NodeKind::CenterPin(node) => {
            render_zstack_center(
                state.f,
                &resolve_base_style(active_theme, node.style),
                rect,
                rrect,
                clip_bounds,
                state.ctx.terminal_bg,
            );
            child_clip = Some(intersect_clip(rect));
        }
        NodeKind::StatusBarLayout(node) => {
            render_zstack_center(
                state.f,
                &resolve_base_style(active_theme, node.style),
                rect,
                rrect,
                clip_bounds,
                state.ctx.terminal_bg,
            );
            let inner = rect.inner(false, node.padding);
            child_clip = Some(intersect_clip(inner));
        }
        NodeKind::ScrollView(scroll_view) => {
            child_clip = Some(render_scroll_view_node(
                state,
                node,
                scroll_view,
                rect,
                rrect,
                clip_bounds,
            ));
        }
        NodeKind::PanView(_) => {
            child_clip = Some(intersect_clip(rect));
        }
        NodeKind::Text(node) => {
            render_text(
                state.f,
                &node.spans,
                resolve_base_style(active_theme, node.style),
                node.overflow,
                crate::backend::ratatui_backend::renderers::text::TextRenderCtx {
                    rect,
                    rrect,
                    clip_rect: clip_bounds,
                    terminal_bg: state.ctx.terminal_bg.map(from_ratatui_color),
                },
            );
        }
        #[cfg(feature = "big-text")]
        NodeKind::BigText(node) => {
            render_big_text(
                state.f,
                &node.output.lines,
                node.output.width,
                rect,
                crate::backend::ratatui_backend::renderers::big_text::BigTextRenderCtx {
                    rrect,
                    clip_rect: clip_bounds,
                    base_style: resolve_base_style(active_theme, node.style),
                    gradient: node.gradient,
                    shadow: node.shadow,
                },
            );
        }
        NodeKind::AsciiCanvas(node) => {
            render_ascii_canvas(
                state.f,
                node,
                state.ctx.tree.node(node_id).active_theme(),
                rect,
                clip_bounds,
                state.ctx.terminal_bg.map(from_ratatui_color),
            );
        }
        NodeKind::Input(node) => {
            render_input_node(state, node_id, node, rect, rrect, clip_bounds);
        }
        NodeKind::HexArea(hex_area) => {
            render_hex_area_node(state, node_id, hex_area, rect, rrect, clip_bounds);
        }
        #[cfg(feature = "image")]
        NodeKind::Image(node) => {
            if state.ctx.images_enabled {
                render_image(
                    state.f,
                    node,
                    state.ctx.tree.node(node_id).active_theme(),
                    rect,
                    clip_bounds,
                );
            } else {
                render_image_inline_fallback(
                    state.f,
                    node,
                    state.ctx.tree.node(node_id).active_theme(),
                    rect,
                    clip_bounds,
                );
            }
        }
        NodeKind::TextArea(ta) => {
            render_text_area_node(state, node, ta, rect, rrect, clip_bounds);
        }
        #[cfg(feature = "terminal")]
        NodeKind::Terminal(terminal) => {
            render_terminal_node(state, node_id, node, terminal, rect, rrect, clip_bounds);
        }
        NodeKind::List(list_node) => {
            render_list_node(state, node_id, node, list_node, rect, rrect, clip_bounds);
        }
        NodeKind::Table(table_node) => {
            render_table_node(state, node_id, node, table_node, rect, rrect, clip_bounds);
        }
        NodeKind::Tabs(node) => {
            render_tabs_node(state, node_id, node, rect, rrect, clip_bounds);
        }
        NodeKind::DraggableTabBar(node) => {
            render_draggable_tab_bar_node(state, node_id, node, rect, rrect, clip_bounds);
        }
        NodeKind::Sparkline(node) => {
            render_sparkline(
                state.f,
                node,
                state.ctx.tree.node(node_id).active_theme(),
                rect,
                rrect,
                clip_bounds,
            );
        }
        NodeKind::Chart(node) => {
            render_chart(
                state.f,
                node,
                state.ctx.tree.node(node_id).active_theme(),
                rect,
                clip_bounds.unwrap_or(rect),
            );
        }
        NodeKind::Graph(node) => {
            render_graph(
                state.f,
                node,
                state.ctx.tree.node(node_id).active_theme(),
                rect,
                clip_bounds.unwrap_or(rect),
                state.focus_chain.contains(&node_id),
                (state.ctx.hovered == Some(node_id))
                    .then_some(())
                    .and(state.ctx.mouse_pos),
            );
        }
        NodeKind::SequenceDiagram(node) => {
            render_sequence_diagram(
                state.f,
                node,
                state.ctx.tree.node(node_id).active_theme(),
                rect,
                clip_bounds.unwrap_or(rect),
                (state.ctx.hovered == Some(node_id))
                    .then_some(())
                    .and(state.ctx.mouse_pos),
            );
        }
        NodeKind::Flowchart(node) => {
            render_flowchart(
                state.f,
                node,
                state.ctx.tree.node(node_id).active_theme(),
                rect,
                clip_bounds.unwrap_or(rect),
                (state.ctx.hovered == Some(node_id))
                    .then_some(())
                    .and(state.ctx.mouse_pos),
            );
        }
        NodeKind::ClassDiagram(node) => {
            render_class_diagram(
                state.f,
                node,
                state.ctx.tree.node(node_id).active_theme(),
                rect,
                clip_bounds.unwrap_or(rect),
            );
        }
        NodeKind::StateDiagram(node) => {
            render_state_diagram(
                state.f,
                node,
                state.ctx.tree.node(node_id).active_theme(),
                rect,
                clip_bounds.unwrap_or(rect),
            );
        }
        NodeKind::ErDiagram(node) => {
            render_er_diagram(
                state.f,
                node,
                state.ctx.tree.node(node_id).active_theme(),
                rect,
                clip_bounds.unwrap_or(rect),
            );
        }
        NodeKind::GanttDiagram(node) => {
            render_gantt_diagram(
                state.f,
                node,
                state.ctx.tree.node(node_id).active_theme(),
                rect,
                clip_bounds.unwrap_or(rect),
            );
        }
        NodeKind::Heatmap(node) => {
            render_heatmap(
                state.f,
                node,
                state.ctx.tree.node(node_id).active_theme(),
                rect,
                clip_bounds.unwrap_or(rect),
            );
        }
        NodeKind::Divider(divider_node) => {
            render_divider_node(state, node, divider_node, rect, clip_bounds);
        }
        NodeKind::Button(node) => {
            render_button_node(state, node_id, node, rect, rrect, clip_bounds);
        }
        NodeKind::ZStack(node) => {
            render_zstack_center(
                state.f,
                &resolve_base_style(active_theme, node.style),
                rect,
                rrect,
                clip_bounds,
                state.ctx.terminal_bg,
            );
        }
        NodeKind::Checkbox(node) => {
            render_checkbox_node(state, node_id, node, rect, rrect, clip_bounds);
        }
        NodeKind::ProgressBar(node) => {
            render_progress_bar_node(state, node_id, node, rect, rrect, clip_bounds);
        }
        NodeKind::Spinner(node) => {
            render_spinner(
                state.f,
                node.spinner_style,
                rect,
                rrect,
                SpinnerRenderCtx {
                    frame: node.frame,
                    label: node.label.as_deref(),
                    gap: node.gap,
                    style: resolve_force_accent_style(active_theme, node.style),
                    label_style: crate::style::resolve::resolve_muted_style(
                        active_theme,
                        node.label_style,
                    ),
                    clip_rect: clip_bounds,
                    paint_glyph_caches: state.ctx.paint_glyph_caches.clone(),
                },
            );
        }
        NodeKind::Slider(node) => {
            render_slider_node(state, node_id, node, rect, rrect, clip_bounds);
        }
        NodeKind::Splitter(splitter) => {
            defer_splitter_render = true;
            let mut splitter_clip = rect;
            if splitter.join_frame {
                splitter_clip.x = splitter_clip.x.saturating_sub(1);
                splitter_clip.w = splitter_clip.w.saturating_add(1);
                splitter_clip.y = splitter_clip.y.saturating_sub(1);
                splitter_clip.h = splitter_clip.h.saturating_add(1);
            }
            child_clip = Some(intersect_clip(splitter_clip));
        }
        NodeKind::MouseRegion(region) => {
            let is_hovered = Some(node_id) == state.ctx.hovered;
            render_mouse_region(
                state.f,
                region,
                rect,
                clip_bounds,
                is_hovered,
                active_theme,
                state.ctx.contrast_policy,
            );
            defer_mouse_region_post = is_hovered && !region.hover_effects.is_empty();
        }
        NodeKind::DragSource(source) => {
            let dragging_style = resolve_slot(
                state.ctx.tree.node(node_id).active_theme(),
                ThemeRole::DragSource,
                &source.dragging_style,
            );
            if source.is_dragging
                && matches!(source.preview, crate::widgets::DragPreview::SourceSnapshot)
            {
                let reserve_strip = !matches!(source.drag_slot, DragSlot::Collapse);
                if state.ctx.dnd_snapshot_cells.borrow().is_some() {
                    if reserve_strip
                        && !dragging_style.is_empty()
                        && rrect.width > 0
                        && rrect.height > 0
                    {
                        state.f.render_widget(
                            Block::default().style(to_ratatui_style(dragging_style)),
                            rrect,
                        );
                    }
                    child_clip = Some(crate::style::Rect::default());
                } else if !dragging_style.is_empty() {
                    state.f.render_widget(
                        Block::default().style(to_ratatui_style(dragging_style)),
                        rrect,
                    );
                    child_clip = Some(intersect_clip(rect));
                } else {
                    child_clip = Some(intersect_clip(rect));
                }
            } else if source.is_dragging && !dragging_style.is_empty() {
                state.f.render_widget(
                    Block::default().style(to_ratatui_style(dragging_style)),
                    rrect,
                );
                child_clip = Some(intersect_clip(rect));
            } else {
                child_clip = Some(intersect_clip(rect));
            }
        }
        NodeKind::DropTarget(target) => {
            let highlight_style = resolve_slot(
                state.ctx.tree.node(node_id).active_theme(),
                ThemeRole::DropTargetActive,
                &target.highlight_style,
            );
            if target.dnd_highlighted {
                match target.highlight {
                    DropHighlight::None => {}
                    DropHighlight::Fill if !highlight_style.is_empty() => {
                        state.f.render_widget(
                            Block::default().style(to_ratatui_style(highlight_style)),
                            rrect,
                        );
                    }
                    DropHighlight::Placeholder => {
                        render_placeholder_frame(
                            state.f,
                            rect,
                            to_ratatui_style(highlight_style),
                            None,
                        );
                    }
                    DropHighlight::Overlay if !highlight_style.is_empty() => {
                        defer_drop_target_overlay = true;
                    }
                    _ => {}
                }
            }
            child_clip = Some(intersect_clip(rect));
        }
        NodeKind::Popover(_) | NodeKind::Portal(_) | NodeKind::Group(_) | NodeKind::Spacer(_) => {}
        NodeKind::DocumentView(dv) => {
            let is_focused = state.focus_chain.contains(&node_id);
            let is_hovered = Some(node_id) == state.ctx.hovered;
            // When pin_scrollbar_focus is set (single-scrollbar DiffView), check
            // if the parent container has a focused descendant so the scrollbar
            // appears active regardless of which split pane holds focus.
            #[cfg(feature = "diff-view")]
            let scrollbar_focus_override = !is_focused
                && dv.pin_scrollbar_focus
                && node
                    .parent
                    .and_then(|frame_id| state.ctx.tree.node(frame_id).parent)
                    .is_some_and(|hstack_id| state.focus_chain.contains(&hstack_id));
            #[cfg(not(feature = "diff-view"))]
            let scrollbar_focus_override = false;
            let copy_feedback_style = state
                .ctx
                .copy_feedback
                .filter(|feedback| feedback.is_active(node_id))
                .map(|_| state.ctx.copy_feedback_style);
            render_document_view(
                state.f,
                dv,
                DocumentViewRenderCtx {
                    rect,
                    clip_bounds,
                    is_focused,
                    is_hovered,
                    scrollbar_focus_override,
                    theme: active_theme,
                    copy_feedback_style,
                    #[cfg(feature = "diff-view")]
                    hover_mouse_pos: is_hovered.then_some(()).and(state.ctx.mouse_pos),
                },
            );
        }
    }

    (
        child_clip,
        defer_effect_scope_render,
        defer_animated_render,
        defer_splitter_render,
        defer_drop_target_overlay,
        defer_mouse_region_post,
    )
}

fn render_frame_node(
    state: &mut RenderState<'_, '_, '_>,
    node: &crate::core::node::Node,
    props: &crate::widgets::internal::FrameNode,
    geometry: &FrameGeometry,
    clip_bounds: Option<Rect>,
) {
    let mut themed_props = props.clone();
    let theme = node.active_theme();
    themed_props.style = crate::style::resolve::resolve_border_style(theme, themed_props.style);
    themed_props.title_style = resolve_base_style(theme, themed_props.title_style);
    themed_props.status_style =
        crate::style::resolve::resolve_muted_style(theme, themed_props.status_style);
    themed_props.active_tab_style =
        crate::style::resolve::resolve_accent_style(theme, themed_props.active_tab_style);
    themed_props.inactive_tab_style =
        crate::style::resolve::resolve_muted_style(theme, themed_props.inactive_tab_style);
    if themed_props
        .style_overrides
        .as_ref()
        .is_none_or(|overrides| overrides.focus_style.is_none())
        && !theme.focus.is_empty()
    {
        themed_props.overrides_mut().focus_style = Some(StyleSlot::Inherit);
    }
    if let Some(overrides) = themed_props.style_overrides.as_mut() {
        overrides.inner_style = overrides
            .inner_style
            .map(|style| resolve_base_style(theme, style));
        overrides.focus_style = overrides
            .focus_style
            .map(|slot| StyleSlot::Replace(resolve_slot(theme, ThemeRole::Focus, &slot)));
        overrides.hover_style = overrides
            .hover_style
            .map(|slot| StyleSlot::Replace(resolve_slot(theme, ThemeRole::Hover, &slot)));
        overrides.focus_title_style = overrides
            .focus_title_style
            .map(|style| resolve_base_style(theme, style));
        overrides.focus_status_style = overrides
            .focus_status_style
            .map(|style| resolve_base_style(theme, style));
        overrides.focus_active_tab_style = overrides
            .focus_active_tab_style
            .map(|style| resolve_base_style(theme, style));
        overrides.focus_inactive_tab_style = overrides
            .focus_inactive_tab_style
            .map(|style| resolve_base_style(theme, style));
    }
    for decoration in &mut themed_props.decorations {
        decoration.style = resolve_base_style(theme, decoration.style);
        decoration.focus_style = decoration
            .focus_style
            .map(|style| resolve_base_style(theme, style));
        decoration.hover_style = decoration
            .hover_style
            .map(|style| resolve_base_style(theme, style));
    }
    let active = state.focus_chain.contains(&node.id);
    let is_hovered = if themed_props.hover_style().is_some_and(|s| !s.is_empty()) {
        frame_contains_hovered_node(state, node.id)
    } else {
        Some(node.id) == state.ctx.hovered
    };
    let terminal_bg = state.ctx.terminal_bg.map(from_ratatui_color);
    render_frame(
        state.f,
        &themed_props,
        geometry,
        FrameRenderCtx {
            active,
            is_hovered,
            clip_rect: clip_bounds,
            terminal_bg,
        },
    );
}

fn render_vstack_node(
    f: &mut ratatui::Frame<'_>,
    node: &crate::widgets::internal::StackNode,
    theme: &crate::style::Theme,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    clip_bounds: Option<Rect>,
    terminal_bg: Option<crate::style::Color>,
) {
    let mut props = node.props.clone();
    props.style = resolve_base_style(theme, props.style);
    render_vstack(
        f,
        &props,
        &node.tab_titles,
        node.active_tab,
        rect,
        rrect,
        VStackRenderCtx {
            active_tab_style: &resolve_slot(theme, ThemeRole::Selection, &node.active_tab_style),
            inactive_tab_style: &crate::style::resolve::resolve_muted_style(
                theme,
                node.inactive_tab_style,
            ),
            tab_variant: &node.tab_variant,
            title_prefix: &node.title_prefix,
            clip_rect: clip_bounds,
            terminal_bg,
        },
    );
}

fn render_hstack_node(
    f: &mut ratatui::Frame<'_>,
    node: &crate::widgets::internal::StackNode,
    theme: &crate::style::Theme,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    clip_bounds: Option<Rect>,
    terminal_bg: Option<crate::style::Color>,
) {
    let mut props = node.props.clone();
    props.style = resolve_base_style(theme, props.style);
    render_hstack(f, &props, rect, rrect, clip_bounds, terminal_bg);
}

mod drag_preview;
mod frame_integration;
mod offset;
mod overlay;

pub(crate) use drag_preview::*;
pub(crate) use frame_integration::*;
pub(crate) use offset::*;
pub(crate) use overlay::*;

#[cfg(test)]
mod render_tests;
