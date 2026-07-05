//! Graph keyboard handler.

use crate::core::event::{KeyCode, KeyEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::Rect;
use crate::widgets::PanEvent;
use crate::widgets::internal::{bound_pan_offset, pan_metrics};

const FOCUS_PAN_MARGIN: i32 = 1;

#[cfg(feature = "image")]
fn suspend_image_rendering_for_pan() {
    crate::backend::ratatui_backend::image_support::suspend_image_rendering_for(
        std::time::Duration::from_millis(120),
    );
}

/// Handle keyboard input for a focused Graph node.
///
/// Arrow/Home/End keys move the graph's internal focused node when a navigation
/// target exists. Enter/Space activates the currently focused graph node.
pub(crate) fn handle_key(tree: &mut NodeTree, id: NodeId, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End => handle_navigation_key(tree, id, key.code),
        KeyCode::Enter | KeyCode::Char(' ') => handle_activation_key(tree, id),
        _ => false,
    }
}

fn handle_navigation_key(tree: &mut NodeTree, id: NodeId, key: KeyCode) -> bool {
    let Some((target, event, on_node_focus)) = navigation_state(tree, id, key) else {
        return false;
    };

    let changed = {
        let NodeKind::Graph(graph) = &mut tree.node_mut(id).kind else {
            return false;
        };
        graph.set_focused_path(target.clone())
    };

    if !changed {
        return false;
    }

    if let (Some(cb), Some(event)) = (on_node_focus.as_ref(), event) {
        cb.emit(event);
    }

    ensure_focused_node_visible(tree, id, &target);

    true
}

fn navigation_state(
    tree: &NodeTree,
    id: NodeId,
    key: KeyCode,
) -> Option<(
    crate::widgets::GraphNodePath,
    Option<crate::widgets::GraphNodeEvent>,
    Option<crate::callback::Callback<crate::widgets::GraphNodeEvent>>,
)> {
    let NodeKind::Graph(graph) = &tree.node(id).kind else {
        return None;
    };

    let target = graph.navigation_target(key)?;
    let event = graph.event_for_path(&target);
    let on_node_focus = graph.on_node_focus.clone();
    Some((target, event, on_node_focus))
}

fn ensure_focused_node_visible(
    tree: &mut NodeTree,
    graph_id: NodeId,
    path: &crate::widgets::GraphNodePath,
) {
    let Some((pan_id, previous, next, event, on_pan, state_key)) =
        focused_pan_adjustment(tree, graph_id, path)
    else {
        return;
    };

    if let NodeKind::PanView(pan) = &mut tree.node_mut(pan_id).kind {
        pan.offset_x = next.0;
        pan.offset_y = next.1;
        pan.input_override = Some(next);
        pan.input_dirty = true;
    }
    // Keep the realized tree and controlled PanView parents in sync with the auto-pan.
    shift_pan_view_children(tree, pan_id, previous, next);
    if let Some(key) = state_key {
        tree.pan_input_offset_by_key.insert(key, next);
    }
    if let Some(cb) = on_pan.as_ref() {
        cb.emit(event);
    }
    #[cfg(feature = "image")]
    suspend_image_rendering_for_pan();
}

type FocusedPanAdjustment = (
    NodeId,
    (i32, i32),
    (i32, i32),
    PanEvent,
    Option<crate::callback::Callback<PanEvent>>,
    Option<crate::core::element::Key>,
);

fn focused_pan_adjustment(
    tree: &NodeTree,
    graph_id: NodeId,
    path: &crate::widgets::GraphNodePath,
) -> Option<FocusedPanAdjustment> {
    let graph_node = tree.node(graph_id);
    let NodeKind::Graph(graph) = &graph_node.kind else {
        return None;
    };
    let graph_content = graph.content_rect(graph_node.rect);
    let focused_node = graph.output.nodes.iter().find(|node| &node.path == path)?;
    let focused_rect = translate_rect(focused_node.rect, graph_content.x, graph_content.y);

    let pan_id = nearest_pan_view_ancestor(tree, graph_id)?;
    let pan_node = tree.node(pan_id);
    let NodeKind::PanView(pan) = &pan_node.kind else {
        return None;
    };

    let previous = (pan.offset_x, pan.offset_y);
    let metrics = pan_metrics(pan.content_w, pan.content_h, pan.viewport_w, pan.viewport_h);
    let target = (
        ensure_axis_visible(
            previous.0,
            focused_rect.x,
            focused_rect.w,
            pan_node.rect.x,
            pan_node.rect.w,
        ),
        ensure_axis_visible(
            previous.1,
            focused_rect.y,
            focused_rect.h,
            pan_node.rect.y,
            pan_node.rect.h,
        ),
    );
    let next = bound_pan_offset(target, metrics, pan.clamp, pan.free_pan_margin);
    if next == previous {
        return None;
    }

    Some((
        pan_id,
        previous,
        next,
        PanEvent {
            x: next.0,
            y: next.1,
            metrics,
        },
        pan.on_pan.clone(),
        pan.state_key.clone().or_else(|| pan_node.key.clone()),
    ))
}

fn nearest_pan_view_ancestor(tree: &NodeTree, id: NodeId) -> Option<NodeId> {
    let mut current = tree.node(id).parent;
    while let Some(parent_id) = current {
        if !tree.is_valid(parent_id) {
            return None;
        }
        if matches!(&tree.node(parent_id).kind, NodeKind::PanView(_)) {
            return Some(parent_id);
        }
        current = tree.node(parent_id).parent;
    }
    None
}

fn ensure_axis_visible(
    offset: i32,
    item_start: i16,
    item_len: u16,
    viewport_start: i16,
    viewport_len: u16,
) -> i32 {
    if item_len == 0 || viewport_len == 0 {
        return offset;
    }

    let viewport_len = i32::from(viewport_len);
    let margin = FOCUS_PAN_MARGIN.min(viewport_len.saturating_sub(1) / 2);
    let visible_start = i32::from(viewport_start).saturating_add(margin);
    let visible_end = i32::from(viewport_start)
        .saturating_add(viewport_len)
        .saturating_sub(margin);
    let item_start = i32::from(item_start);
    let item_end = item_start.saturating_add(i32::from(item_len));

    if item_start < visible_start {
        offset.saturating_sub(visible_start.saturating_sub(item_start))
    } else if item_end > visible_end {
        offset.saturating_add(item_end.saturating_sub(visible_end))
    } else {
        offset
    }
}

fn translate_rect(rect: Rect, dx: i16, dy: i16) -> Rect {
    Rect {
        x: (i32::from(rect.x) + i32::from(dx)).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        y: (i32::from(rect.y) + i32::from(dy)).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        ..rect
    }
}

fn shift_pan_view_children(
    tree: &mut NodeTree,
    pan_id: NodeId,
    old_offset: (i32, i32),
    new_offset: (i32, i32),
) {
    let dx = old_offset.0.saturating_sub(new_offset.0);
    let dy = old_offset.1.saturating_sub(new_offset.1);
    if dx == 0 && dy == 0 {
        return;
    }

    let children = tree.node(pan_id).children.clone();
    for child in children {
        shift_subtree_rect(tree, child, dx, dy);
    }
}

fn shift_subtree_rect(tree: &mut NodeTree, id: NodeId, dx: i32, dy: i32) {
    if !tree.is_valid(id) {
        return;
    }
    let children = {
        let node = tree.node_mut(id);
        node.rect.x = (i32::from(node.rect.x) + dx).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        node.rect.y = (i32::from(node.rect.y) + dy).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        node.children.clone()
    };
    for child in children {
        shift_subtree_rect(tree, child, dx, dy);
    }
}

fn handle_activation_key(tree: &NodeTree, id: NodeId) -> bool {
    let NodeKind::Graph(graph) = &tree.node(id).kind else {
        return false;
    };

    let Some(cb) = graph.on_node_activate.clone() else {
        return false;
    };
    let Some(event) = graph.focused_event() else {
        return false;
    };

    cb.emit(event);
    true
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::callback::Callback;
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};
    use crate::core::node::{NodeKind, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::style::{Length, Rect};
    use crate::widgets::{Graph, GraphDirection, GraphNode, GraphNodeEvent, PanEvent, PanView};

    use super::handle_key;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods::default(),
        }
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
                w: 80,
                h: 24,
            },
            None,
        );
        tree
    }

    fn sample_tree() -> GraphNode {
        GraphNode::new("root").children([GraphNode::new("left"), GraphNode::new("right")])
    }

    fn recorder() -> (Rc<RefCell<Vec<GraphNodeEvent>>>, Callback<GraphNodeEvent>) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let callback_events = Rc::clone(&events);
        (
            events,
            Callback::new(move |event| callback_events.borrow_mut().push(event)),
        )
    }

    fn pan_recorder() -> (Rc<RefCell<Vec<PanEvent>>>, Callback<PanEvent>) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let callback_events = Rc::clone(&events);
        (
            events,
            Callback::new(move |event| callback_events.borrow_mut().push(event)),
        )
    }

    #[test]
    fn on_node_click_does_not_make_graph_focusable() {
        let (_events, cb) = recorder();
        let tree = reconcile_graph(Graph::new().root(sample_tree()).on_node_click(cb));

        assert!(!tree.node(tree.root).is_focusable());
    }

    #[test]
    fn on_node_activate_makes_graph_focusable_and_enter_activates_root() {
        let (events, cb) = recorder();
        let tree = reconcile_graph(Graph::new().root(sample_tree()).on_node_activate(cb));

        assert!(tree.node(tree.root).is_focusable());

        let mut tree = tree;
        let root = tree.root;
        assert!(handle_key(&mut tree, root, key(KeyCode::Enter)));
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].label.as_ref(), "root");
        assert!(events[0].path.segments().is_empty());
    }

    #[test]
    fn top_down_arrows_move_between_parent_child_and_siblings() {
        let (events, cb) = recorder();
        let mut tree = reconcile_graph(Graph::new().root(sample_tree()).on_node_focus(cb));
        let root = tree.root;

        assert!(handle_key(&mut tree, root, key(KeyCode::Down)));
        assert!(handle_key(&mut tree, root, key(KeyCode::Right)));
        assert!(handle_key(&mut tree, root, key(KeyCode::Up)));

        let events = events.borrow();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].path.segments(), &[0]);
        assert_eq!(events[0].label.as_ref(), "left");
        assert_eq!(events[1].path.segments(), &[1]);
        assert_eq!(events[1].label.as_ref(), "right");
        assert!(events[2].path.segments().is_empty());
        assert_eq!(events[2].label.as_ref(), "root");
    }

    #[test]
    fn left_right_arrows_move_between_parent_child_and_siblings() {
        let (events, cb) = recorder();
        let mut tree = reconcile_graph(
            Graph::new()
                .root(sample_tree())
                .direction(GraphDirection::LeftRight)
                .on_node_focus(cb),
        );
        let root = tree.root;

        assert!(handle_key(&mut tree, root, key(KeyCode::Right)));
        assert!(handle_key(&mut tree, root, key(KeyCode::Down)));
        assert!(handle_key(&mut tree, root, key(KeyCode::Left)));

        let events = events.borrow();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].path.segments(), &[0]);
        assert_eq!(events[1].path.segments(), &[1]);
        assert!(events[2].path.segments().is_empty());
    }

    #[test]
    fn graph_navigation_pans_nearest_pan_view_to_focused_node() {
        let (events, cb) = pan_recorder();
        let graph = Graph::new()
            .root(GraphNode::new("root").child(GraphNode::new("far child")))
            .direction(GraphDirection::LeftRight)
            .gap_x(12)
            .node_border(false)
            .focusable(true);
        let root: crate::Element = PanView::new()
            .child(graph)
            .width(Length::Px(10))
            .height(Length::Px(5))
            .clamp(false)
            .center_content(false)
            .on_pan(cb)
            .into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 5,
            },
            None,
        );
        let pan_id = tree.root;
        let graph_id = tree.node(pan_id).children[0];

        assert!(handle_key(&mut tree, graph_id, key(KeyCode::Right)));

        let NodeKind::PanView(pan) = &tree.node(pan_id).kind else {
            panic!("expected pan view");
        };
        let offset_x = pan.offset_x;
        assert!(offset_x > 0);
        assert_eq!(events.borrow().len(), 1);
        assert_eq!(events.borrow()[0].x, offset_x);
        assert_eq!(i32::from(tree.node(graph_id).rect.x), -offset_x);
    }

    #[test]
    fn activation_uses_keyboard_focused_node() {
        let (events, cb) = recorder();
        let mut tree = reconcile_graph(Graph::new().root(sample_tree()).on_node_activate(cb));
        let root = tree.root;

        assert!(handle_key(&mut tree, root, key(KeyCode::Down)));
        assert!(handle_key(&mut tree, root, key(KeyCode::Char(' '))));

        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].path.segments(), &[0]);
        assert_eq!(events[0].label.as_ref(), "left");
    }

    #[test]
    fn boundary_navigation_is_unhandled() {
        let mut tree = reconcile_graph(Graph::new().root(sample_tree()).focusable(true));
        let root = tree.root;

        assert!(!handle_key(&mut tree, root, key(KeyCode::Up)));
        let NodeKind::Graph(graph) = &tree.node(root).kind else {
            panic!("expected graph");
        };
        assert!(graph.focused_event().is_some());
    }
}
