use super::overlay::reconcile_portal;
use super::state::{
    ReconcileCtx, SingleChildReconcile, reconcile_single_child_required, resolve_rect_with_auto,
    reuse_or_replace_kind,
};
use crate::core::element::{Element, ElementKind};
use crate::core::node::{NodeId, NodeKind};
use crate::layout::tag::can_reuse;
#[cfg(feature = "big-text")]
use crate::widgets::internal::reconcile_big_text;
#[cfg(feature = "image")]
use crate::widgets::internal::reconcile_image;
#[cfg(feature = "terminal")]
use crate::widgets::internal::reconcile_terminal;
use crate::widgets::internal::{
    CanvasReconcile, DividerReconcile, GridReconcile, HStackReconcile, PanViewReconcile,
    ScrollViewReconcile, SparklineNode, SplitterReconcile, StatusBarLayoutReconcile,
    VStackReconcile, measure_input, reconcile_animated, reconcile_ascii_canvas, reconcile_button,
    reconcile_canvas, reconcile_center, reconcile_center_pin, reconcile_chart, reconcile_checkbox,
    reconcile_class_diagram, reconcile_divider, reconcile_document_view, reconcile_drag_source,
    reconcile_draggable_tab_bar, reconcile_drop_target, reconcile_effect_scope,
    reconcile_er_diagram, reconcile_flow, reconcile_flowchart, reconcile_frame,
    reconcile_gantt_diagram, reconcile_graph, reconcile_grid, reconcile_hex_area, reconcile_hstack,
    reconcile_list, reconcile_mouse_region, reconcile_pan_view, reconcile_progress_bar,
    reconcile_scroll_view, reconcile_slider, reconcile_spacer, reconcile_spinner,
    reconcile_state_diagram, reconcile_status_bar_layout, reconcile_tabs, reconcile_text,
    reconcile_text_area, reconcile_vstack, reconcile_zstack,
};

pub(crate) struct ElementReconcile<'a> {
    pub reuse: Option<NodeId>,
    pub parent: Option<NodeId>,
    pub el: &'a Element,
    pub rect: crate::style::Rect,
}

pub(crate) fn reconcile_element(ctx: &mut ReconcileCtx<'_>, args: ElementReconcile<'_>) -> NodeId {
    let ElementReconcile {
        reuse,
        parent,
        el,
        rect,
    } = args;
    let epoch = ctx.epoch;
    let focus = ctx.focus;
    // ThemeProvider scopes the active theme for every realized descendant.
    if let ElementKind::ThemeProvider(tp) = &el.kind {
        ctx.tree.push_active_theme(tp.theme.clone());
        let id = reconcile_element(
            ctx,
            ElementReconcile {
                reuse,
                parent,
                el: &tp.child,
                rect,
            },
        );
        ctx.tree.pop_active_theme();
        return id;
    }
    if let ElementKind::ContextProvider(cp) = &el.kind {
        return reconcile_element(
            ctx,
            ElementReconcile {
                reuse,
                parent,
                el: &cp.child,
                rect,
            },
        );
    }
    if let ElementKind::Memo(_) = &el.kind {
        if cfg!(debug_assertions) {
            panic!("memo elements must be expanded before layout");
        }

        let id = match reuse {
            Some(id) if ctx.tree.is_valid(id) => id,
            _ => ctx.tree.alloc(),
        };
        let active_theme = ctx.tree.current_active_theme();
        let node = ctx.tree.node_mut(id);
        node.epoch = epoch;
        node.key = el.key.clone();
        node.parent = parent;
        node.rect = rect;
        node.set_active_theme(active_theme);
        node.children.clear();
        node.kind = NodeKind::Text(crate::widgets::internal::TextNode::from(
            crate::widgets::Text::new("ERROR: Unexpanded memo"),
        ));
        ctx.tree.note_kind_set(id);
        return id;
    }

    let id = match reuse {
        Some(id) if ctx.tree.is_valid(id) && can_reuse(ctx.tree.node(id), el) => id,
        _ => ctx.tree.alloc(),
    };

    {
        let active_theme = ctx.tree.current_active_theme();
        let node = ctx.tree.node_mut(id);
        node.epoch = epoch;
        node.key = el.key.clone();
        node.parent = parent;
        node.set_active_theme(active_theme);
    }

    let result_id = match &el.kind {
        ElementKind::Text(text) => reconcile_text(ctx.tree, id, text, rect, &el.layout),
        #[cfg(feature = "big-text")]
        ElementKind::BigText(big_text) => {
            reconcile_big_text(ctx.tree, id, big_text, rect, &el.layout)
        }
        ElementKind::AsciiCanvas(canvas) => {
            reconcile_ascii_canvas(ctx.tree, id, canvas, rect, &el.layout)
        }
        ElementKind::Button(button) => reconcile_button(ctx.tree, id, button, rect, &el.layout),
        #[cfg(feature = "image")]
        ElementKind::Image(image) => reconcile_image(ctx.tree, id, image, rect, &el.layout),

        ElementKind::Input(input) => {
            let (w, h) = measure_input(input);
            let rect = resolve_rect_with_auto(rect, &el.layout, input.width, input.height, w, h);

            let node = ctx.tree.node_mut(id);
            node.rect = rect;
            node.children.clear();
            // Trivial payload: replace each reconcile.
            reuse_or_replace_kind(
                &mut node.kind,
                |_existing| false,
                || NodeKind::from((**input).clone()),
            );

            id
        }
        ElementKind::HexArea(hex_area) => {
            reconcile_hex_area(ctx.tree, id, hex_area, rect, &el.layout)
        }
        ElementKind::TextArea(text_area) => {
            reconcile_text_area(ctx.tree, id, text_area, rect, &el.layout)
        }
        #[cfg(feature = "terminal")]
        ElementKind::Terminal(terminal) => {
            reconcile_terminal(ctx.tree, id, terminal, rect, &el.layout)
        }
        ElementKind::Popover(popover) => crate::widgets::internal::reconcile_popover(
            ctx.tree,
            epoch,
            id,
            popover,
            rect,
            focus,
            ctx.overlay_state,
        ),
        ElementKind::Portal(portal) => reconcile_portal(
            ctx.tree,
            epoch,
            id,
            portal,
            &el.layout,
            focus,
            ctx.overlay_state,
        ),
        ElementKind::List(list) => reconcile_list(ctx.tree, id, list, rect),
        ElementKind::Table(table) => {
            crate::widgets::internal::reconcile_table(ctx.tree, id, table, rect)
        }
        ElementKind::Tabs(tabs) => reconcile_tabs(ctx.tree, id, tabs, rect, &el.layout),
        ElementKind::DraggableTabBar(bar) => reconcile_draggable_tab_bar(ctx.tree, id, bar, rect),
        ElementKind::Component(_component) => {
            if cfg!(debug_assertions) {
                panic!("component elements must be expanded before layout");
            }

            let node = ctx.tree.node_mut(id);
            node.rect = rect;
            node.children.clear();
            node.kind = NodeKind::Text(crate::widgets::internal::TextNode::from(
                crate::widgets::Text::new("ERROR: Unexpanded component"),
            ));

            id
        }
        ElementKind::Group(group) => {
            let old_children = {
                let node = ctx.tree.node_mut(id);
                node.rect = rect;
                node.kind = NodeKind::Group(crate::core::node::GroupNode { scope: group.scope });
                std::mem::take(&mut node.children)
            };
            debug_assert!(
                old_children.len() <= 1,
                "group must contain at most one reusable child"
            );

            let new_children = reconcile_single_child_required(
                ctx,
                SingleChildReconcile {
                    parent_id: id,
                    child: Some(group.child.as_ref()),
                    rect,
                    old_children,
                },
            );

            let node = ctx.tree.node_mut(id);
            node.children = new_children;
            debug_assert!(
                node.children.len() == 1,
                "group must contain exactly one reconciled child"
            );

            id
        }
        ElementKind::EffectScope(scope) => {
            reconcile_effect_scope(ctx.tree, epoch, id, scope, rect, focus, ctx.overlay_state)
        }
        ElementKind::Animated(animated) => reconcile_animated(
            ctx.tree,
            epoch,
            id,
            animated,
            rect,
            focus,
            ctx.overlay_state,
        ),
        ElementKind::DragSource(source) => {
            reconcile_drag_source(ctx.tree, epoch, id, source, rect, focus, ctx.overlay_state)
        }
        ElementKind::DropTarget(target) => {
            reconcile_drop_target(ctx.tree, epoch, id, target, rect, focus, ctx.overlay_state)
        }
        ElementKind::MouseRegion(region) => {
            reconcile_mouse_region(ctx.tree, epoch, id, region, rect, focus, ctx.overlay_state)
        }
        ElementKind::ScrollView(sv) => {
            let anchor_key = sv.scroll_state_key.clone().or_else(|| el.key.clone());
            reconcile_scroll_view(
                ctx,
                ScrollViewReconcile {
                    id,
                    sv,
                    scroll_key: anchor_key,
                    rect,
                },
            )
        }
        ElementKind::PanView(pan) => {
            let pan_key = pan.pan_state_key.clone().or_else(|| el.key.clone());
            reconcile_pan_view(
                ctx,
                PanViewReconcile {
                    id,
                    pan,
                    pan_key,
                    rect,
                    constraints: &el.layout,
                },
            )
        }
        ElementKind::Divider(div) => reconcile_divider(
            ctx,
            DividerReconcile {
                id,
                divider: div,
                rect,
                constraints: &el.layout,
            },
        ),
        ElementKind::Spacer(spacer) => reconcile_spacer(ctx.tree, id, spacer, rect, &el.layout),
        ElementKind::VStack(vs) => {
            let old_children = {
                let node = ctx.tree.node_mut(id);
                node.rect = rect;
                if let NodeKind::VStack(existing) = &mut node.kind {
                    // Update element-derived fields in place;
                    // layout_cache and last_focused_key are preserved automatically.
                    existing.props = vs.props.clone();
                    existing.tab_titles = vs.tab_titles.clone();
                    existing.active_tab = vs.active_tab;
                    existing.on_tab_change = vs.on_tab_change.clone();
                    existing.active_tab_style = vs.active_tab_style;
                    existing.inactive_tab_style = vs.inactive_tab_style;
                    existing.tab_variant = vs.tab_variant;
                    existing.title_prefix = vs.title_prefix.clone();
                } else {
                    node.kind = NodeKind::VStack(crate::widgets::internal::StackNode {
                        props: vs.props.clone(),
                        tab_titles: vs.tab_titles.clone(),
                        active_tab: vs.active_tab,
                        on_tab_change: vs.on_tab_change.clone(),
                        active_tab_style: vs.active_tab_style,
                        inactive_tab_style: vs.inactive_tab_style,
                        tab_variant: vs.tab_variant,
                        title_prefix: vs.title_prefix.clone(),
                        layout_cache: None,
                        last_focused_key: None,
                    });
                }
                std::mem::take(&mut node.children)
            };

            let inner = rect.inner(vs.props.border, vs.props.padding);
            let new_children = reconcile_vstack(
                ctx,
                VStackReconcile {
                    parent: id,
                    old_children,
                    vs,
                    bounds: inner,
                },
            );

            let node = ctx.tree.node_mut(id);
            node.children = new_children;

            id
        }
        ElementKind::HStack(hs) => {
            let old_children = {
                let node = ctx.tree.node_mut(id);
                node.rect = rect;
                if let NodeKind::HStack(existing) = &mut node.kind {
                    // Update only the element-derived field in place;
                    // layout_cache, last_focused_key, and tab fields are preserved.
                    existing.props = hs.props.clone();
                } else {
                    node.kind = NodeKind::HStack(crate::widgets::internal::StackNode {
                        props: hs.props.clone(),
                        tab_titles: Vec::new(),
                        active_tab: 0,
                        on_tab_change: None,
                        active_tab_style: crate::style::StyleSlot::Inherit,
                        inactive_tab_style: crate::style::Style::default(),
                        tab_variant: crate::widgets::TabVariant::default(),
                        title_prefix: None,
                        layout_cache: None,
                        last_focused_key: None,
                    });
                }
                std::mem::take(&mut node.children)
            };

            let inner = rect.inner(hs.props.border, hs.props.padding);
            let new_children = reconcile_hstack(
                ctx,
                HStackReconcile {
                    parent: id,
                    old_children,
                    hs,
                    bounds: inner,
                },
            );

            let node = ctx.tree.node_mut(id);
            node.children = new_children;

            id
        }
        ElementKind::Grid(grid) => {
            let old_children = {
                let node = ctx.tree.node_mut(id);
                node.rect = rect;
                if let NodeKind::Grid(existing) = &mut node.kind {
                    existing.props = grid.props.clone();
                } else {
                    node.kind = NodeKind::Grid(crate::widgets::internal::GridNode {
                        props: grid.props.clone(),
                        layout_cache: None,
                    });
                }
                std::mem::take(&mut node.children)
            };

            let inner = rect.inner(grid.props.border, grid.props.padding);
            let new_children = reconcile_grid(
                ctx,
                GridReconcile {
                    parent: id,
                    old_children,
                    grid,
                    bounds: inner,
                },
            );

            let node = ctx.tree.node_mut(id);
            node.children = new_children;

            id
        }
        ElementKind::Flow(flow) => reconcile_flow(ctx, id, flow, rect),
        ElementKind::Canvas(canvas) => reconcile_canvas(
            ctx,
            CanvasReconcile {
                id,
                canvas,
                rect,
                constraints: &el.layout,
            },
        ),
        ElementKind::Splitter(splitter) => {
            let old_children = {
                let node = ctx.tree.node_mut(id);
                node.rect = rect;
                std::mem::take(&mut node.children)
            };

            let new_children = crate::widgets::internal::reconcile_splitter(
                ctx,
                SplitterReconcile {
                    parent: id,
                    old_children,
                    splitter,
                    bounds: rect,
                    constraints: &el.layout,
                },
            );

            let node = ctx.tree.node_mut(id);
            node.children = new_children;

            id
        }
        ElementKind::ZStack(zs) => {
            reconcile_zstack(ctx.tree, id, zs, rect, focus, ctx.overlay_state, epoch)
        }
        ElementKind::Center(center) => {
            reconcile_center(ctx.tree, id, center, rect, focus, ctx.overlay_state, epoch)
        }
        ElementKind::CenterPin(cp) => {
            reconcile_center_pin(ctx.tree, id, cp, rect, focus, ctx.overlay_state, epoch)
        }
        ElementKind::Frame(frame) => {
            reconcile_frame(ctx.tree, epoch, id, frame, rect, focus, ctx.overlay_state)
        }
        ElementKind::StatusBarLayout(layout) => reconcile_status_bar_layout(
            ctx,
            StatusBarLayoutReconcile {
                id,
                layout,
                rect,
                constraints: &el.layout,
            },
        ),
        ElementKind::Checkbox(checkbox) => {
            reconcile_checkbox(ctx.tree, id, checkbox, rect, &el.layout)
        }
        ElementKind::ProgressBar(progress) => {
            reconcile_progress_bar(ctx.tree, id, progress, rect, &el.layout)
        }
        ElementKind::Spinner(spinner) => reconcile_spinner(ctx.tree, id, spinner, rect),
        ElementKind::Slider(slider) => reconcile_slider(ctx.tree, id, slider, rect, &el.layout),
        ElementKind::Sparkline(sparkline) => {
            let (w, h) = crate::widgets::internal::measure_sparkline(sparkline);
            let rect =
                resolve_rect_with_auto(rect, &el.layout, sparkline.width, sparkline.height, w, h);
            let alloc_w = Some(rect.w);

            let node = ctx.tree.node_mut(id);
            node.rect = rect;
            node.children.clear();

            // Cache-heavy payload: prefer in-place reconcile to preserve render caches.
            reuse_or_replace_kind(
                &mut node.kind,
                |existing| {
                    if let NodeKind::Sparkline(existing_node) = existing {
                        crate::widgets::internal::reconcile_sparkline(
                            sparkline,
                            existing_node,
                            alloc_w,
                        );
                        true
                    } else {
                        false
                    }
                },
                || {
                    let mut n = SparklineNode::default();
                    crate::widgets::internal::reconcile_sparkline(sparkline, &mut n, alloc_w);
                    NodeKind::Sparkline(n)
                },
            );

            id
        }
        ElementKind::Chart(chart) => {
            let (w, h) = crate::widgets::internal::measure_chart(chart);
            let rect = resolve_rect_with_auto(rect, &el.layout, chart.width, chart.height, w, h);

            let node = ctx.tree.node_mut(id);
            node.rect = rect;
            node.children.clear();

            reuse_or_replace_kind(
                &mut node.kind,
                |existing| {
                    if let NodeKind::Chart(existing_node) = existing {
                        reconcile_chart(chart, existing_node);
                        true
                    } else {
                        false
                    }
                },
                || NodeKind::from((**chart).clone()),
            );

            id
        }
        ElementKind::Graph(graph) => {
            let (w, h) = crate::widgets::internal::measure_graph(graph);
            let rect = resolve_rect_with_auto(rect, &el.layout, graph.width, graph.height, w, h);

            let node = ctx.tree.node_mut(id);
            node.rect = rect;
            node.children.clear();

            reuse_or_replace_kind(
                &mut node.kind,
                |existing| {
                    if let NodeKind::Graph(existing_node) = existing {
                        reconcile_graph(graph, existing_node);
                        true
                    } else {
                        false
                    }
                },
                || NodeKind::from((**graph).clone()),
            );

            id
        }
        ElementKind::SequenceDiagram(sequence) => {
            let (w, h) = crate::widgets::internal::measure_sequence_diagram(sequence);
            let rect =
                resolve_rect_with_auto(rect, &el.layout, sequence.width, sequence.height, w, h);

            let node = ctx.tree.node_mut(id);
            node.rect = rect;
            node.children.clear();

            reuse_or_replace_kind(
                &mut node.kind,
                |existing| {
                    if let NodeKind::SequenceDiagram(existing_node) = existing {
                        let content_width = rect.inner(sequence.border, sequence.padding).w;
                        crate::widgets::internal::reconcile_sequence_diagram_with_width(
                            sequence,
                            existing_node,
                            Some(content_width),
                        );
                        true
                    } else {
                        false
                    }
                },
                || {
                    let mut node = crate::widgets::internal::SequenceDiagramNode::default();
                    let content_width = rect.inner(sequence.border, sequence.padding).w;
                    crate::widgets::internal::reconcile_sequence_diagram_with_width(
                        sequence,
                        &mut node,
                        Some(content_width),
                    );
                    NodeKind::SequenceDiagram(node)
                },
            );

            id
        }
        ElementKind::Flowchart(flowchart) => {
            let (w, h) = crate::widgets::internal::measure_flowchart(flowchart);
            let rect =
                resolve_rect_with_auto(rect, &el.layout, flowchart.width, flowchart.height, w, h);

            let node = ctx.tree.node_mut(id);
            node.rect = rect;
            node.children.clear();

            reuse_or_replace_kind(
                &mut node.kind,
                |existing| {
                    if let NodeKind::Flowchart(existing_node) = existing {
                        reconcile_flowchart(flowchart, existing_node);
                        true
                    } else {
                        false
                    }
                },
                || NodeKind::from((**flowchart).clone()),
            );

            id
        }
        ElementKind::ClassDiagram(diagram) => {
            reconcile_class_diagram(ctx.tree, id, diagram, rect, &el.layout)
        }
        ElementKind::StateDiagram(diagram) => {
            reconcile_state_diagram(ctx.tree, id, diagram, rect, &el.layout)
        }
        ElementKind::ErDiagram(diagram) => {
            reconcile_er_diagram(ctx.tree, id, diagram, rect, &el.layout)
        }
        ElementKind::GanttDiagram(diagram) => {
            reconcile_gantt_diagram(ctx.tree, id, diagram, rect, &el.layout)
        }
        ElementKind::Heatmap(heatmap) => {
            let (w, h) = crate::widgets::internal::measure_heatmap(heatmap);
            let rect =
                resolve_rect_with_auto(rect, &el.layout, heatmap.width, heatmap.height, w, h);

            let node = ctx.tree.node_mut(id);
            node.rect = rect;
            node.children.clear();

            reuse_or_replace_kind(
                &mut node.kind,
                |existing| {
                    if let NodeKind::Heatmap(existing_node) = existing {
                        crate::widgets::internal::reconcile_heatmap_node(heatmap, existing_node);
                        true
                    } else {
                        false
                    }
                },
                || NodeKind::from(heatmap.clone()),
            );

            id
        }
        ElementKind::DocumentView(dv) => {
            reconcile_document_view(ctx.tree, id, parent, dv, rect, &el.layout)
        }
        ElementKind::ThemeProvider(_) => {
            // Handled by early return above; unreachable.
            unreachable!("ThemeProvider should be handled before node allocation")
        }
        ElementKind::ContextProvider(_) => {
            unreachable!("ContextProvider should be handled before node allocation")
        }
        ElementKind::Memo(_) => unreachable!("Memo should be handled before node allocation"),
    };

    // Incrementally track hover/mouse/animation capabilities instead of a
    // post-reconciliation full-tree DFS (refresh_hoverables).
    ctx.tree.note_kind_set(result_id);

    result_id
}
