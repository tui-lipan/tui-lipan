use std::time::Duration;

use super::*;
use crate::animation::{Easing, TransitionConfig};
use crate::callback::Callback;
use crate::core::element::{Element, ElementKind, IntoElement, Key};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::LayoutEngine;
use crate::layout::measure::min_size_constrained;
use crate::layout::stack::layout_scroll_content;
use crate::style::{Color, Length, Rect, ScrollbarConfig, ScrollbarVariant, Theme};
use crate::widgets::ScrollEvent;
use crate::widgets::VirtualChildEntry;
use crate::widgets::scroll_view::utils::calc_scroll_view_window;
#[cfg(feature = "diff-view")]
use crate::widgets::{DiffView, DiffViewBackend, DiffViewMode};
use crate::widgets::{
    DocumentView, Flow, Frame, HStack, ScrollBehavior, ScrollChildExitDirection,
    ScrollChildVisibility, ScrollRequest, ScrollTarget, ScrollView, ScrollViewportEvent, Spacer,
    Text, ThemeProvider, VStack,
};

fn linear_scroll_behavior(duration_ms: u64) -> ScrollBehavior {
    ScrollBehavior::smooth(TransitionConfig {
        duration: Duration::from_millis(duration_ms),
        easing: Easing::Linear,
    })
}

fn find_by_key(tree: &NodeTree, key: &str) -> Option<NodeId> {
    let key = Key::from(key.to_string());
    tree.iter()
        .find(|node| node.key.as_ref() == Some(&key))
        .map(|node| node.id)
}

fn viewport_events_callback() -> (
    std::rc::Rc<std::cell::RefCell<Vec<ScrollViewportEvent>>>,
    Callback<ScrollViewportEvent>,
) {
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let events_cb = events.clone();
    let cb = Callback::new(move |event: ScrollViewportEvent| {
        events_cb.borrow_mut().push(event);
    });
    (events, cb)
}

fn keyed_rows(count: usize, height: u16) -> impl Iterator<Item = Element> {
    (0..count).map(move |i| {
        Text::new(format!("row {i}"))
            .height(Length::Px(height))
            .key(format!("r{i}"))
    })
}

fn root_scroll_offset(tree: &NodeTree) -> usize {
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected root scroll view");
    };
    scroll.offset
}

fn first_visible_key_and_y(tree: &NodeTree, scroll_id: NodeId) -> (String, i16) {
    let child_id = *tree
        .node(scroll_id)
        .children
        .first()
        .expect("expected at least one visible scroll child");
    let child = tree.node(child_id);
    let key = child
        .key
        .as_ref()
        .expect("visible scroll child should have key")
        .as_ref()
        .to_string();
    (key, child.rect.y)
}

fn center_visible_key_and_y(tree: &NodeTree, scroll_id: NodeId) -> (String, i16) {
    let NodeKind::ScrollView(scroll) = &tree.node(scroll_id).kind else {
        panic!("expected scroll view");
    };
    let viewport_center = i32::from(scroll.viewport_height / 2);

    let child_id = tree
        .node(scroll_id)
        .children
        .iter()
        .copied()
        .min_by_key(|id| {
            let rect = tree.node(*id).rect;
            let rect_center = i32::from(rect.y) + i32::from(rect.h / 2);
            (rect_center - viewport_center).abs()
        })
        .expect("expected at least one visible scroll child");
    let child = tree.node(child_id);
    let key = child
        .key
        .as_ref()
        .expect("visible scroll child should have key")
        .as_ref()
        .to_string();
    (key, child.rect.y)
}

#[test]
fn viewport_change_initial_emit_visible_children_then_dedupe() {
    let (events, cb) = viewport_events_callback();
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    };
    let root: Element = ScrollView::new()
        .on_viewport_change(cb.clone())
        .children(keyed_rows(6, 1))
        .into();
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
    {
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.offset, 0);
        assert_eq!(event.first_visible_index, Some(0));
        assert_eq!(event.last_visible_index, Some(2));
        assert_eq!(
            event
                .visible
                .iter()
                .map(|child| child.index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(event.entered, event.visible);
        assert!(event.exited.is_empty());
        assert!(event.visible.iter().all(|child| {
            child.visible_height == 1
                && child.clipped_above == 0
                && child.clipped_below == 0
                && child.visibility == ScrollChildVisibility::FullyVisible
        }));
    }

    LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
    assert_eq!(events.borrow().len(), 1);
}

#[test]
fn viewport_change_controlled_offset_reports_entered_exited_above() {
    let (events, cb) = viewport_events_callback();
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    };
    let mut tree = NodeTree::new();
    let root_top: Element = ScrollView::new()
        .on_viewport_change(cb.clone())
        .children(keyed_rows(6, 1))
        .into();
    LayoutEngine::reconcile_with_focus(&mut tree, &root_top, bounds, None);

    let root_scrolled: Element = ScrollView::new()
        .offset(2)
        .on_viewport_change(cb.clone())
        .children(keyed_rows(6, 1))
        .into();
    LayoutEngine::reconcile_with_focus(&mut tree, &root_scrolled, bounds, None);

    let events = events.borrow();
    assert_eq!(events.len(), 2);
    let event = &events[1];
    assert_eq!(event.offset, 2);
    assert_eq!(
        event
            .visible
            .iter()
            .map(|child| child.index)
            .collect::<Vec<_>>(),
        vec![2, 3, 4]
    );
    assert_eq!(
        event
            .entered
            .iter()
            .map(|child| child.index)
            .collect::<Vec<_>>(),
        vec![3, 4]
    );
    assert_eq!(
        event
            .exited
            .iter()
            .map(|child| (child.child.index, child.direction))
            .collect::<Vec<_>>(),
        vec![
            (0, ScrollChildExitDirection::Above),
            (1, ScrollChildExitDirection::Above),
        ]
    );
}

#[test]
fn viewport_change_reports_partial_visibility_metadata() {
    let (events, cb) = viewport_events_callback();
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    };
    let root: Element = ScrollView::new()
        .offset(1)
        .on_viewport_change(cb)
        .children(keyed_rows(2, 2))
        .into();
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

    let events = events.borrow();
    assert_eq!(events.len(), 1);
    let first = &events[0].visible[0];
    assert_eq!(first.index, 0);
    assert_eq!(first.content_rect.y, 0);
    assert_eq!(first.viewport_rect.y, -1);
    assert_eq!(first.visible_rect.y, 0);
    assert_eq!(first.visible_height, 1);
    assert_eq!(first.clipped_above, 1);
    assert_eq!(first.clipped_below, 0);
    assert_eq!(first.visibility, ScrollChildVisibility::PartiallyVisible);
}

#[test]
fn viewport_change_emits_on_layout_only_height_change() {
    let (events, cb) = viewport_events_callback();
    let mut tree = NodeTree::new();
    let root: Element = ScrollView::new()
        .on_viewport_change(cb.clone())
        .children(keyed_rows(6, 1))
        .into();

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 2,
        },
        None,
    );
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

    let events = events.borrow();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].offset, 0);
    assert_eq!(events[1].offset, 0);
    assert_eq!(events[0].metrics.visible, 2);
    assert_eq!(events[1].metrics.visible, 3);
    assert_eq!(events[1].last_visible_index, Some(2));
}

#[test]
fn viewport_change_accounts_for_scroll_indicator_rows() {
    let (events, cb) = viewport_events_callback();
    let root: Element = ScrollView::new()
        .offset(2)
        .show_scroll_indicators(true)
        .on_viewport_change(cb)
        .children(keyed_rows(6, 1))
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

    let events = events.borrow();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert!(event.top_indicator);
    assert!(event.bottom_indicator);
    assert_eq!(event.bottom_count, 3);
    assert_eq!(event.metrics.visible, 1);
    assert_eq!(event.visible.len(), 1);
    assert_eq!(event.visible[0].index, 2);
    assert_eq!(event.visible[0].viewport_rect.y, 0);
}

#[test]
fn controlled_offset_desync_emits_on_scroll_until_prop_matches_effective() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let hits = Rc::new(RefCell::new(0usize));
    let hits_cb = hits.clone();
    let cb = Callback::new(move |_ev: ScrollEvent| {
        *hits_cb.borrow_mut() += 1;
    });

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 5,
    };
    let mut tree = NodeTree::new();
    let root_max: Element = ScrollView::new()
        .offset(usize::MAX)
        .on_scroll(cb.clone())
        .children((0..24).map(|i| {
            Text::new(format!("row {i}"))
                .height(Length::Px(1))
                .key(format!("r{i}"))
        }))
        .into();
    LayoutEngine::reconcile_with_focus(&mut tree, &root_max, bounds, None);
    assert_eq!(*hits.borrow(), 1);

    let NodeKind::ScrollView(sv) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    let at_bottom = sv.offset;

    let root_synced: Element = ScrollView::new()
        .offset(at_bottom)
        .on_scroll(cb.clone())
        .children((0..24).map(|i| {
            Text::new(format!("row {i}"))
                .height(Length::Px(1))
                .key(format!("r{i}"))
        }))
        .into();
    LayoutEngine::reconcile_with_focus(&mut tree, &root_synced, bounds, None);
    assert_eq!(
        *hits.borrow(),
        1,
        "no second emit once the element offset matches the reconciled offset"
    );
}

#[test]
fn zero_height_viewport_preserves_controlled_offset_without_scroll_emit() {
    use std::cell::RefCell;
    use std::rc::Rc;

    fn root(offset: usize, cb: Callback<ScrollEvent>) -> Element {
        ScrollView::new()
            .offset(offset)
            .on_scroll(cb)
            .children(keyed_rows(24, 1))
            .into()
    }

    let events = Rc::new(RefCell::new(Vec::<ScrollEvent>::new()));
    let events_cb = events.clone();
    let cb = Callback::new(move |ev: ScrollEvent| {
        events_cb.borrow_mut().push(ev);
    });
    let visible_bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 5,
    };
    let hidden_bounds = Rect {
        h: 0,
        ..visible_bounds
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &root(10, cb.clone()), visible_bounds, None);
    assert_eq!(root_scroll_offset(&tree), 10);

    LayoutEngine::reconcile_with_focus(&mut tree, &root(10, cb.clone()), hidden_bounds, None);
    assert!(
        events.borrow().is_empty(),
        "zero-height reconcile must not publish a synthetic scroll-to-top"
    );
    {
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("expected scroll view");
        };
        assert_eq!(scroll.offset, 10);
        assert_eq!(scroll.max_offset, 19);
        assert_eq!(scroll.viewport_height, 0);
    }

    LayoutEngine::reconcile_with_focus(&mut tree, &root(10, cb.clone()), visible_bounds, None);
    assert_eq!(root_scroll_offset(&tree), 10);
    assert!(events.borrow().is_empty());
}

#[test]
fn zero_height_viewport_does_not_consume_scroll_request() {
    fn root(request: Option<ScrollRequest>) -> Element {
        let mut scroll = ScrollView::new().children(keyed_rows(20, 1));
        if let Some(request) = request {
            scroll = scroll.scroll_request(request);
        }
        scroll.into()
    }

    let visible_bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 5,
    };
    let hidden_bounds = Rect {
        h: 0,
        ..visible_bounds
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &root(None), visible_bounds, None);
    assert_eq!(root_scroll_offset(&tree), 0);

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root(Some(ScrollRequest::half_page_down())),
        hidden_bounds,
        None,
    );
    assert_eq!(root_scroll_offset(&tree), 0);

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root(Some(ScrollRequest::half_page_down())),
        visible_bounds,
        None,
    );
    assert_eq!(
        root_scroll_offset(&tree),
        3,
        "request should apply once the viewport has rows again"
    );
}

#[test]
fn scroll_request_is_one_shot_until_cleared() {
    fn root(request: Option<ScrollRequest>) -> Element {
        let mut scroll = ScrollView::new().children((0..20).map(|i| {
            Text::new(format!("row {i}"))
                .height(Length::Px(1))
                .key(format!("r{i}"))
        }));
        if let Some(request) = request {
            scroll = scroll.scroll_request(request);
        }
        scroll.into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 5,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root(Some(ScrollRequest::half_page_down())),
        bounds,
        None,
    );
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(scroll.offset, 3);

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root(Some(ScrollRequest::half_page_down())),
        bounds,
        None,
    );
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(
        scroll.offset, 3,
        "same request should not reapply every render"
    );

    LayoutEngine::reconcile_with_focus(&mut tree, &root(None), bounds, None);
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root(Some(ScrollRequest::half_page_down())),
        bounds,
        None,
    );
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(
        scroll.offset, 6,
        "request reapplies after the prop is cleared"
    );
}

#[test]
fn scroll_request_emits_on_scroll_without_controlled_offset() {
    use std::cell::RefCell;
    use std::rc::Rc;

    fn root(request: Option<ScrollRequest>, cb: Callback<ScrollEvent>) -> Element {
        let mut scroll = ScrollView::new().on_scroll(cb).children((0..20).map(|i| {
            Text::new(format!("row {i}"))
                .height(Length::Px(1))
                .key(format!("r{i}"))
        }));
        if let Some(request) = request {
            scroll = scroll.scroll_request(request);
        }
        scroll.into()
    }

    let events = Rc::new(RefCell::new(Vec::<ScrollEvent>::new()));
    let events_cb = events.clone();
    let cb = Callback::new(move |ev: ScrollEvent| {
        events_cb.borrow_mut().push(ev);
    });
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 5,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &root(None, cb.clone()), bounds, None);
    assert!(events.borrow().is_empty());

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root(Some(ScrollRequest::bottom()), cb.clone()),
        bounds,
        None,
    );
    {
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].offset, events[0].metrics.max_offset);
    }

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root(Some(ScrollRequest::bottom()), cb.clone()),
        bounds,
        None,
    );
    assert_eq!(events.borrow().len(), 1, "same request remains one-shot");

    LayoutEngine::reconcile_with_focus(&mut tree, &root(None, cb.clone()), bounds, None);
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root(Some(ScrollRequest::top()), cb),
        bounds,
        None,
    );
    {
        let events = events.borrow();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].offset, 0);
    }
}

#[test]
fn scroll_request_takes_priority_over_controlled_offset() {
    let root: Element = ScrollView::new()
        .offset(0)
        .scroll_request(ScrollRequest::bottom())
        .children((0..20).map(|i| {
            Text::new(format!("row {i}"))
                .height(Length::Px(1))
                .key(format!("r{i}"))
        }))
        .into();

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 5,
    };
    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(scroll.offset, scroll.max_offset);
}

#[test]
fn bottom_scroll_request_measures_new_virtual_tail_rows() {
    fn root(include_tall_tail: bool, request: Option<ScrollRequest>) -> Element {
        let mut children: Vec<Element> = (0..12)
            .map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(1))
                    .key(format!("r{i}"))
            })
            .collect();
        if include_tall_tail {
            children.push(Text::new("tail").height(Length::Px(20)).key("tail"));
        }

        let mut scroll = ScrollView::new().gap(0).children(children);
        if let Some(request) = request {
            scroll = scroll.scroll_request(request);
        }
        scroll.into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 5,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &root(false, None), bounds, None);
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root(true, Some(ScrollRequest::bottom())),
        bounds,
        None,
    );

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(scroll.content_height, 32);
    assert_eq!(scroll.offset, scroll.max_offset);
    assert_eq!(scroll.max_offset, 27);
    assert!(find_by_key(&tree, "tail").is_some());
}

#[test]
fn top_scroll_request_refreshes_virtual_measurements() {
    fn root(include_tall_tail: bool, request: Option<ScrollRequest>) -> Element {
        let mut children: Vec<Element> = (0..12)
            .map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(1))
                    .key(format!("r{i}"))
            })
            .collect();
        if include_tall_tail {
            children.push(Text::new("tail").height(Length::Px(20)).key("tail"));
        }

        let mut scroll = ScrollView::new().gap(0).children(children);
        if let Some(request) = request {
            scroll = scroll.scroll_request(request);
        }
        scroll.into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 5,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root(false, Some(ScrollRequest::bottom())),
        bounds,
        None,
    );
    LayoutEngine::reconcile_with_focus(&mut tree, &root(false, None), bounds, None);
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root(true, Some(ScrollRequest::top())),
        bounds,
        None,
    );

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(scroll.content_height, 32);
    assert_eq!(scroll.offset, 0);
    assert_eq!(scroll.max_offset, 27);
    assert!(find_by_key(&tree, "r0").is_some());
}

#[test]
fn scroll_to_key_scrolls_direct_child_into_view() {
    let root: Element = ScrollView::new()
        .scroll_to_key("row-6")
        .children((0..10).map(|i| Text::new(format!("Row {i}")).key(format!("row-{i}"))))
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

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };

    assert_eq!(scroll.offset, 6);
    assert!(find_by_key(&tree, "row-6").is_some());
    assert!(find_by_key(&tree, "row-5").is_none());
}

#[test]
fn scroll_to_key_matches_nested_child_keys() {
    let root: Element = ScrollView::new()
        .scroll_to_key("message-4")
        .children((0..6).map(|i| {
            Frame::new()
                .border(false)
                .child(Text::new(format!("Message {i}")).key(format!("message-{i}")))
                .key(format!("frame-{i}"))
        }))
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 2,
        },
        None,
    );

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };

    assert_eq!(scroll.offset, 4);
    assert!(find_by_key(&tree, "message-4").is_some());
}

#[test]
fn scroll_to_key_offset_lands_inside_keyed_child() {
    let root: Element = ScrollView::new()
        .scroll_to_key_offset("frame-2", 3)
        .children((0..4).map(|i| {
            Frame::new()
                .border(false)
                .height(Length::Px(5))
                .child(Text::new(format!("Message {i}\nline\nline\nline\nline")))
                .key(format!("frame-{i}"))
        }))
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 4,
        },
        None,
    );

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };

    assert_eq!(scroll.offset, 13);
}

#[test]
fn scroll_view_gap_ignores_trailing_zero_height_key_target() {
    let root: Element = ScrollView::new()
        .gap(1)
        .scroll_to_key("bottom")
        .children(vec![
            Text::new("first row").height(Length::Px(1)).into(),
            Text::new("last row").height(Length::Px(1)).key("last"),
            Spacer::new().height(Length::Px(0)).key("bottom"),
        ])
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 1,
        },
        None,
    );

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };

    assert_eq!(scroll.content_height, 3);
    assert_eq!(scroll.max_offset, 2);
    assert_eq!(scroll.offset, scroll.max_offset);

    let last = find_by_key(&tree, "last").expect("last row should be visible at bottom");
    assert_eq!(tree.node(last).rect.y, 0);
    assert_eq!(tree.node(last).rect.h, 1);
}

#[test]
fn scroll_to_bottom_uses_actual_extent_without_sentinel() {
    let root: Element = ScrollView::new()
        .gap(1)
        .scroll_to_bottom()
        .children(vec![
            Text::new("first row").height(Length::Px(1)).into(),
            Text::new("last row").height(Length::Px(1)).key("last"),
        ])
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 1,
        },
        None,
    );

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };

    assert_eq!(scroll.content_height, 3);
    assert_eq!(scroll.max_offset, 2);
    assert_eq!(scroll.offset, scroll.max_offset);

    let last = find_by_key(&tree, "last").expect("last row should be visible at bottom");
    assert_eq!(tree.node(last).rect.y, 0);
    assert_eq!(tree.node(last).rect.h, 1);
}

#[test]
fn smooth_scroll_to_bottom_retargets_when_content_grows() {
    fn root(count: usize) -> Element {
        ScrollView::new()
            .scroll_to(ScrollTarget::Bottom)
            .scroll_behavior(linear_scroll_behavior(100))
            .children((0..count).map(|i| Text::new(format!("Row {i}")).key(format!("row-{i}"))))
            .into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    };
    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(&mut tree, &root(10), bounds, None);
    {
        let NodeKind::ScrollView(scroll) = &mut tree.node_mut(tree.root).kind else {
            panic!("root should be a scroll view");
        };
        assert_eq!(scroll.max_offset, 7);
        assert!(scroll.smooth_scroll.is_animating());
        let tick = scroll
            .smooth_scroll
            .tick(Duration::from_millis(50), scroll.max_offset);
        assert!(tick.still_animating);
    }

    LayoutEngine::reconcile_with_focus(&mut tree, &root(15), bounds, None);

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };
    assert_eq!(scroll.max_offset, 12);
    assert_eq!(scroll.offset, 4);
    assert!(scroll.smooth_scroll.is_animating());
}

#[test]
fn smooth_scroll_to_key_first_reconcile_uses_current_displayed_offset() {
    let root: Element = ScrollView::new()
        .scroll_to_key("row-6")
        .scroll_behavior(linear_scroll_behavior(100))
        .children((0..10).map(|i| Text::new(format!("Row {i}")).key(format!("row-{i}"))))
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

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };
    assert_eq!(scroll.offset, 0);
    assert!(scroll.smooth_scroll.is_animating());
}

#[test]
fn smooth_scroll_to_key_same_target_preserves_progress() {
    let root: Element = ScrollView::new()
        .scroll_to_key("row-6")
        .scroll_behavior(linear_scroll_behavior(100))
        .children((0..10).map(|i| Text::new(format!("Row {i}")).key(format!("row-{i}"))))
        .into();
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    };
    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
    {
        let NodeKind::ScrollView(scroll) = &mut tree.node_mut(tree.root).kind else {
            panic!("root should be a scroll view");
        };
        let tick = scroll
            .smooth_scroll
            .tick(Duration::from_millis(50), scroll.max_offset);
        assert!(tick.still_animating);
    }

    LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };
    assert_eq!(scroll.offset, 3);
    assert!(scroll.smooth_scroll.is_animating());
}

#[test]
fn smooth_scroll_to_key_retargets_from_displayed_offset() {
    fn root(target: &str) -> Element {
        ScrollView::new()
            .scroll_to_key(target.to_string())
            .scroll_behavior(linear_scroll_behavior(100))
            .children((0..20).map(|i| Text::new(format!("Row {i}")).key(format!("row-{i}"))))
            .into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    };
    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(&mut tree, &root("row-10"), bounds, None);
    {
        let NodeKind::ScrollView(scroll) = &mut tree.node_mut(tree.root).kind else {
            panic!("root should be a scroll view");
        };
        let tick = scroll
            .smooth_scroll
            .tick(Duration::from_millis(50), scroll.max_offset);
        assert!(tick.still_animating);
    }

    LayoutEngine::reconcile_with_focus(&mut tree, &root("row-15"), bounds, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };
    assert_eq!(scroll.offset, 5);
    assert!(scroll.smooth_scroll.is_animating());
}

#[test]
fn smooth_scroll_to_key_absent_cancels_at_displayed_offset() {
    fn root(with_target: bool) -> Element {
        let scroll = ScrollView::new()
            .scroll_behavior(linear_scroll_behavior(100))
            .children((0..10).map(|i| Text::new(format!("Row {i}")).key(format!("row-{i}"))));
        if with_target {
            scroll.scroll_to_key("row-6").into()
        } else {
            scroll.into()
        }
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    };
    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(&mut tree, &root(true), bounds, None);
    {
        let NodeKind::ScrollView(scroll) = &mut tree.node_mut(tree.root).kind else {
            panic!("root should be a scroll view");
        };
        let tick = scroll
            .smooth_scroll
            .tick(Duration::from_millis(50), scroll.max_offset);
        assert!(tick.still_animating);
    }

    LayoutEngine::reconcile_with_focus(&mut tree, &root(false), bounds, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };
    assert_eq!(scroll.offset, 3);
    assert!(!scroll.smooth_scroll.is_animating());
}

#[test]
fn controlled_offset_change_with_same_scroll_to_key_is_immediate_and_suppressed() {
    fn root(offset: Option<usize>) -> Element {
        let scroll = ScrollView::new()
            .scroll_to_key("row-10")
            .scroll_behavior(linear_scroll_behavior(100))
            .children((0..20).map(|i| Text::new(format!("Row {i}")).key(format!("row-{i}"))));
        if let Some(offset) = offset {
            scroll.offset(offset).into()
        } else {
            scroll.into()
        }
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    };
    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(&mut tree, &root(None), bounds, None);
    {
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("root should be a scroll view");
        };
        assert_eq!(scroll.offset, 0);
        assert!(scroll.smooth_scroll.is_animating());
    }

    LayoutEngine::reconcile_with_focus(&mut tree, &root(Some(5)), bounds, None);
    {
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("root should be a scroll view");
        };
        assert_eq!(scroll.offset, 5);
        assert!(!scroll.smooth_scroll.is_animating());
    }

    LayoutEngine::reconcile_with_focus(&mut tree, &root(Some(5)), bounds, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };
    assert_eq!(scroll.offset, 5);
    assert!(!scroll.smooth_scroll.is_animating());
}

#[test]
fn controlled_offset_assertion_with_same_scroll_to_key_cancels_smooth_target() {
    fn root(offset: Option<usize>) -> Element {
        let scroll = ScrollView::new()
            .scroll_to_key("row-10")
            .scroll_behavior(linear_scroll_behavior(100))
            .children((0..20).map(|i| Text::new(format!("Row {i}")).key(format!("row-{i}"))));
        if let Some(offset) = offset {
            scroll.offset(offset).into()
        } else {
            scroll.into()
        }
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    };
    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(&mut tree, &root(None), bounds, None);
    {
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("root should be a scroll view");
        };
        assert_eq!(scroll.offset, 0);
        assert!(scroll.smooth_scroll.is_animating());
    }

    LayoutEngine::reconcile_with_focus(&mut tree, &root(Some(0)), bounds, None);

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };
    assert_eq!(scroll.offset, 0);
    assert!(matches!(
        scroll.cancelled_scroll_target.as_ref(),
        Some(ScrollTarget::Key(key)) if key.as_ref() == "row-10"
    ));
    assert!(!scroll.smooth_scroll.is_animating());
}

#[test]
fn keyed_scroll_rows_restore_offscreen_document_visual_cache() {
    fn root(offset: usize) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .gap(0)
            .offset(offset)
            .children((0..6).map(|i| {
                DocumentView::new(format!("row {i}"))
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(false)
                    .height(Length::Auto)
                    .key(format!("row-{i}"))
            }))
            .into()
    }

    let row_key = Key::from("row-0".to_string());
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 2,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &root(0), bounds, None);
    let initial_flat_text = {
        let row_id = find_by_key(&tree, "row-0").expect("row visible");
        let NodeKind::DocumentView(doc) = &tree.node(row_id).kind else {
            panic!("expected document view row");
        };
        doc.visual_cache.flat_text.clone()
    };

    LayoutEngine::reconcile_with_focus(&mut tree, &root(2), bounds, None);
    let saved_flat_text = {
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("expected root scroll view");
        };
        let saved = scroll
            .offscreen_doc_selections
            .get(&row_key)
            .expect("row-0 should be saved offscreen");
        saved.docs[0].visual_cache.flat_text.clone()
    };
    assert!(std::sync::Arc::ptr_eq(&initial_flat_text, &saved_flat_text));

    LayoutEngine::reconcile_with_focus(&mut tree, &root(0), bounds, None);
    let restored_flat_text = {
        let row_id = find_by_key(&tree, "row-0").expect("row visible again");
        let NodeKind::DocumentView(doc) = &tree.node(row_id).kind else {
            panic!("expected document view row");
        };
        doc.visual_cache.flat_text.clone()
    };
    assert!(
        std::sync::Arc::ptr_eq(&saved_flat_text, &restored_flat_text),
        "restored row should reuse the saved visual cache instead of rebuilding it"
    );
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected root scroll view");
    };
    assert!(!scroll.offscreen_doc_selections.contains_key(&row_key));
}

#[test]
fn scroll_to_key_falls_back_to_explicit_offset_when_missing() {
    let root: Element = ScrollView::new()
        .offset(3)
        .scroll_to_key("missing")
        .children((0..8).map(|i| Text::new(format!("Row {i}")).key(format!("row-{i}"))))
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

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };

    assert_eq!(scroll.offset, 3);
}

#[test]
fn handler_dirty_preserves_dragged_offset_until_element_catches_up() {
    let stale_root: Element = ScrollView::new()
        .offset(0)
        .children((0..12).map(|i| Text::new(format!("Row {i}")).key(format!("row-{i}"))))
        .into();

    let mut tree = NodeTree::new();
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    };

    LayoutEngine::reconcile_with_focus(&mut tree, &stale_root, bounds, None);

    let NodeKind::ScrollView(scroll) = &mut tree.node_mut(tree.root).kind else {
        panic!("root should be a scroll view");
    };
    scroll.offset = 4;
    scroll.scroll_offset = 4;
    scroll.scroll_override = Some(4);
    scroll.scroll_handler_dirty = true;

    LayoutEngine::reconcile_with_focus(&mut tree, &stale_root, bounds, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };
    assert_eq!(scroll.offset, 4);
    assert_eq!(scroll.scroll_override, Some(4));
    assert!(scroll.scroll_handler_dirty);

    LayoutEngine::reconcile_with_focus(&mut tree, &stale_root, bounds, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };
    assert_eq!(scroll.offset, 4);

    let fresh_root: Element = ScrollView::new()
        .offset(4)
        .children((0..12).map(|i| Text::new(format!("Row {i}")).key(format!("row-{i}"))))
        .into();
    LayoutEngine::reconcile_with_focus(&mut tree, &fresh_root, bounds, None);

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("root should be a scroll view");
    };
    assert_eq!(scroll.offset, 4);
    assert_eq!(scroll.scroll_override, Some(4));
    assert!(!scroll.scroll_handler_dirty);
    assert_eq!(scroll.element_offset, Some(4));
}

#[test]
fn unchanged_element_offset_does_not_keep_scroll_override_stuck_after_resize() {
    fn make_root(offset: Option<usize>, body: &str) -> Element {
        let mut sv = ScrollView::new();
        if let Some(offset) = offset {
            sv = sv.offset(offset);
        }
        sv.children((0..40).map(|i| {
            Frame::new()
                .border(false)
                .height(Length::Auto)
                .child(
                    DocumentView::new(body)
                        .border(false)
                        .scrollbar(false)
                        .h_scrollbar(false)
                        .wrap(true)
                        .height(Length::Auto),
                )
                .key(format!("row-{i}"))
        }))
        .into()
    }

    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
    let mut tree = NodeTree::new();
    let wide_bounds = Rect {
        x: 0,
        y: 0,
        w: 72,
        h: 10,
    };
    let narrow_bounds = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 10,
    };

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(Some(120), body), wide_bounds, None);
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(Some(120), body), narrow_bounds, None);

    // Simulate wheel-dragged offset after resize while app still passes stale offset(120).
    let NodeKind::ScrollView(scroll) = &mut tree.node_mut(tree.root).kind else {
        panic!("expected scroll view");
    };
    scroll.offset = scroll.offset.saturating_add(5).min(scroll.max_offset);
    scroll.scroll_offset = scroll.offset as u16;
    scroll.scroll_override = Some(scroll.offset);
    scroll.scroll_handler_dirty = true;

    // During active input ownership, element does not provide offset.
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(None, body), narrow_bounds, None);

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert!(
        !scroll.scroll_handler_dirty,
        "when element stops providing offset, handler-dirty should clear"
    );

    // App catches up with the current offset and then keeps it unchanged.
    let settled = scroll.offset;
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(Some(settled), body),
        narrow_bounds,
        None,
    );

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert!(
        !scroll.scroll_handler_dirty,
        "handler-dirty should clear when app catches up"
    );
    assert_eq!(
        scroll.scroll_override,
        Some(scroll.offset),
        "override should settle to the effective offset"
    );
    assert_eq!(scroll.element_offset, Some(scroll.offset));
}

#[test]
fn wheel_updates_keep_working_after_resize_with_stale_element_offset() {
    fn make_root(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .offset(offset)
            .children((0..40).map(|i| {
                Frame::new()
                    .border(false)
                    .height(Length::Auto)
                    .child(
                        DocumentView::new(body)
                            .border(false)
                            .scrollbar(false)
                            .h_scrollbar(false)
                            .wrap(true)
                            .height(Length::Auto),
                    )
                    .key(format!("row-{i}"))
            }))
            .into()
    }

    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
    let mut tree = NodeTree::new();
    let wide_bounds = Rect {
        x: 0,
        y: 0,
        w: 72,
        h: 10,
    };
    let narrow_bounds = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 10,
    };

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(120, body), wide_bounds, None);
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(120, body), narrow_bounds, None);

    // Simulate one wheel tick while app keeps passing stale offset(120).
    let expected_next = {
        let NodeKind::ScrollView(scroll) = &mut tree.node_mut(tree.root).kind else {
            panic!("expected scroll view");
        };
        let next = if scroll.offset < scroll.max_offset {
            scroll.offset.saturating_add(1).min(scroll.max_offset)
        } else {
            scroll.offset.saturating_sub(1)
        };
        scroll.offset = next;
        scroll.scroll_offset = next as u16;
        scroll.scroll_override = Some(next);
        scroll.scroll_handler_dirty = true;
        next
    };

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(120, body), narrow_bounds, None);

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(
        scroll.offset, expected_next,
        "stale external offset must not clobber wheel-updated offset"
    );
    assert!(
        scroll.scroll_handler_dirty,
        "handler should own offset until app catches up"
    );

    // Once app catches up with the live offset, handler ownership clears.
    let settled = scroll.offset;
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(settled, body), narrow_bounds, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert!(!scroll.scroll_handler_dirty);
}

#[test]
fn changed_element_offset_after_resize_takes_control() {
    fn make_root(offset: Option<usize>, body: &str) -> Element {
        let mut sv = ScrollView::new();
        if let Some(offset) = offset {
            sv = sv.offset(offset);
        }
        sv.children((0..32).map(|i| {
            Frame::new()
                .border(false)
                .height(Length::Auto)
                .child(
                    DocumentView::new(body)
                        .border(false)
                        .scrollbar(false)
                        .h_scrollbar(false)
                        .wrap(true)
                        .height(Length::Auto),
                )
                .key(format!("row-{i}"))
        }))
        .into()
    }

    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
    let mut tree = NodeTree::new();
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 10,
    };

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(Some(120), body), bounds, None);

    // Simulate local scroll movement.
    let NodeKind::ScrollView(scroll) = &mut tree.node_mut(tree.root).kind else {
        panic!("expected scroll view");
    };
    scroll.offset = scroll.offset.saturating_add(5).min(scroll.max_offset);
    scroll.scroll_offset = scroll.offset as u16;
    scroll.scroll_override = Some(scroll.offset);
    scroll.scroll_handler_dirty = true;

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(None, body), bounds, None);

    // App catches up with a NEW offset request (different from last element_offset)
    // which should clear handler-dirty ownership and apply the element offset.
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(Some(128), body), bounds, None);

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(scroll.offset, 128.min(scroll.max_offset));
    assert!(!scroll.scroll_handler_dirty);
}

#[test]
fn resize_keeps_anchor_when_controlled_offset_catches_up_after_local_scroll() {
    fn center_visible_key(tree: &NodeTree) -> String {
        let root = tree.root;
        let NodeKind::ScrollView(scroll) = &tree.node(root).kind else {
            panic!("expected scroll view");
        };
        let viewport_center = i32::from(scroll.viewport_height / 2);

        tree.node(root)
            .children
            .iter()
            .copied()
            .min_by_key(|id| {
                let rect = tree.node(*id).rect;
                let rect_center = i32::from(rect.y) + i32::from(rect.h / 2);
                (rect_center - viewport_center).abs()
            })
            .and_then(|id| {
                tree.node(id)
                    .key
                    .as_ref()
                    .map(|key| key.as_ref().to_string())
            })
            .expect("center visible child key")
    }

    fn make_row(i: usize, body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(body)
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .height(Length::Auto),
            )
            .key(format!("row-{i}"))
    }

    fn make_root(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..32).map(|i| make_row(i, body)))
            .into()
    }

    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
    let wide_bounds = Rect {
        x: 0,
        y: 0,
        w: 72,
        h: 10,
    };
    let narrow_bounds = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 10,
    };

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(12, body), wide_bounds, None);

    let settled = {
        let NodeKind::ScrollView(scroll) = &mut tree.node_mut(tree.root).kind else {
            panic!("expected scroll view");
        };
        let next = scroll.offset.saturating_add(5).min(scroll.max_offset);
        scroll.offset = next;
        scroll.scroll_offset = next as u16;
        scroll.scroll_override = Some(next);
        scroll.scroll_handler_dirty = true;
        next
    };

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(12, body), wide_bounds, None);
    let wide_center_key = center_visible_key(&tree);

    let wide_offset = {
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("expected scroll view");
        };
        scroll.offset
    };

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(settled, body), narrow_bounds, None);

    let narrow_offset = {
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("expected scroll view");
        };
        scroll.offset
    };
    let narrow_center_key = center_visible_key(&tree);
    assert_eq!(
        narrow_center_key, wide_center_key,
        "anchor drifted across resize after controlled catch-up: wide_offset={wide_offset} narrow_offset={narrow_offset}"
    );
}

#[test]
fn resize_keeps_bottom_pinned_when_controlled_offset_catches_up_after_local_scroll() {
    fn make_row(i: usize, body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(format!("{body} [row {i}]"))
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .height(Length::Auto),
            )
            .key(format!("row-{i}"))
    }

    fn make_root(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..24).map(|i| make_row(i, body)))
            .into()
    }

    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz ";
    let wide_bounds = Rect {
        x: 0,
        y: 0,
        w: 84,
        h: 10,
    };
    let narrow_bounds = Rect {
        x: 0,
        y: 0,
        w: 32,
        h: 10,
    };

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(9999, body), wide_bounds, None);

    let settled = {
        let NodeKind::ScrollView(scroll) = &mut tree.node_mut(tree.root).kind else {
            panic!("expected scroll view");
        };
        assert_eq!(scroll.offset, scroll.max_offset, "precondition: at bottom");
        scroll.scroll_override = Some(scroll.offset);
        scroll.scroll_handler_dirty = true;
        scroll.offset
    };

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(9999, body), wide_bounds, None);
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(settled, body), narrow_bounds, None);

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(scroll.offset, scroll.max_offset);
}

#[test]
fn repeated_resize_keeps_anchor_when_controlled_prop_catches_up_between_steps() {
    fn center_visible_key(tree: &NodeTree) -> String {
        let root = tree.root;
        let NodeKind::ScrollView(scroll) = &tree.node(root).kind else {
            panic!("expected scroll view");
        };
        let viewport_center = i32::from(scroll.viewport_height / 2);

        tree.node(root)
            .children
            .iter()
            .copied()
            .min_by_key(|id| {
                let rect = tree.node(*id).rect;
                let rect_center = i32::from(rect.y) + i32::from(rect.h / 2);
                (rect_center - viewport_center).abs()
            })
            .and_then(|id| {
                tree.node(id)
                    .key
                    .as_ref()
                    .map(|key| key.as_ref().to_string())
            })
            .expect("center visible child key")
    }

    fn make_row(i: usize, body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(format!("{body} [{i}]"))
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .height(Length::Auto),
            )
            .key(format!("row-{i}"))
    }

    fn make_root(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..48).map(|i| make_row(i, body)))
            .into()
    }

    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz abcdefghijklmnopqrstuvwxyz";
    let wide = Rect {
        x: 0,
        y: 0,
        w: 72,
        h: 10,
    };
    let medium = Rect {
        x: 0,
        y: 0,
        w: 28,
        h: 10,
    };
    let narrow = Rect {
        x: 0,
        y: 0,
        w: 18,
        h: 10,
    };

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(14, body), wide, None);
    let expected_key = center_visible_key(&tree);

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(14, body), medium, None);
    let medium_offset = {
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("expected scroll view");
        };
        scroll.offset
    };

    // Simulate the app catching up to the offset emitted by on_scroll from the
    // previous resize before the next resize step happens.
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(medium_offset, body), narrow, None);

    let actual_key = center_visible_key(&tree);
    assert_eq!(
        actual_key, expected_key,
        "continuous resize should keep the same anchored child visible"
    );
}

#[test]
fn controlled_offset_preserves_anchor_when_content_above_grows() {
    fn center_visible_key(tree: &NodeTree) -> String {
        let root = tree.root;
        let NodeKind::ScrollView(scroll) = &tree.node(root).kind else {
            panic!("expected scroll view");
        };
        let viewport_center = i32::from(scroll.viewport_height / 2);

        tree.node(root)
            .children
            .iter()
            .copied()
            .min_by_key(|id| {
                let rect = tree.node(*id).rect;
                let rect_center = i32::from(rect.y) + i32::from(rect.h / 2);
                (rect_center - viewport_center).abs()
            })
            .and_then(|id| {
                tree.node(id)
                    .key
                    .as_ref()
                    .map(|key| key.as_ref().to_string())
            })
            .expect("center visible child key")
    }

    fn make_root(first_row_height: u16, offset: usize) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..8).map(move |i| {
                let height = if i == 0 { first_row_height } else { 10 };
                Text::new(format!("row {i}"))
                    .height(Length::Px(height))
                    .key(format!("row-{i}"))
            }))
            .into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 32,
        h: 5,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(10, 12), bounds, None);
    assert_eq!(center_visible_key(&tree), "row-1");

    // The app is still passing the old settled offset, but content above the
    // viewport grew. Keep the same visible row anchored instead of leaving the
    // viewport at the old absolute offset.
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(20, 12), bounds, None);

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(scroll.offset, 22);
    assert_eq!(center_visible_key(&tree), "row-1");
}

#[test]
fn controlled_tail_offset_pins_bottom_when_content_first_becomes_scrollable() {
    fn make_root(row_count: usize, offset: usize) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..row_count).map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(1))
                    .key(format!("r{i}"))
            }))
            .into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 32,
        h: 5,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(5, usize::MAX), bounds, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(scroll.max_offset, 0);
    assert_eq!(scroll.offset, 0);

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(8, usize::MAX), bounds, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert!(
        scroll.max_offset > 0,
        "content should become scrollable after growth"
    );
    assert_eq!(
        scroll.offset, scroll.max_offset,
        "tail sentinel should pin to the new bottom when content first becomes scrollable"
    );
}

#[test]
fn bottom_scroll_request_pins_bottom_when_content_first_becomes_scrollable() {
    fn make_root(row_count: usize, request: Option<ScrollRequest>) -> Element {
        let mut scroll = ScrollView::new()
            .scrollbar(false)
            .children((0..row_count).map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(1))
                    .key(format!("r{i}"))
            }));
        if let Some(request) = request {
            scroll = scroll.scroll_request(request);
        }
        scroll.into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 32,
        h: 5,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(5, Some(ScrollRequest::bottom())),
        bounds,
        None,
    );
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(scroll.max_offset, 0);
    assert_eq!(scroll.offset, 0);

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(8, Some(ScrollRequest::bottom())),
        bounds,
        None,
    );
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert!(
        scroll.max_offset > 0,
        "content should become scrollable after growth"
    );
    assert_eq!(
        scroll.offset, scroll.max_offset,
        "bottom request should pin to the new bottom when content first becomes scrollable"
    );
}

#[test]
fn wrapped_document_view_in_scroll_view_remeasures_on_resize() {
    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
    let root: Element = ScrollView::new()
        .child(
            Frame::new()
                .border(false)
                .height(Length::Auto)
                .child(
                    DocumentView::new(body)
                        .border(false)
                        .scrollbar(false)
                        .h_scrollbar(false)
                        .wrap(true)
                        .height(Length::Auto),
                )
                .key("frame"),
        )
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 60,
            h: 20,
        },
        None,
    );

    let wide_h = find_by_key(&tree, "frame")
        .map(|id| tree.node(id).rect.h)
        .expect("frame should exist");

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

    let narrow_h = find_by_key(&tree, "frame")
        .map(|id| tree.node(id).rect.h)
        .expect("frame should exist after resize");

    assert!(
        narrow_h > wide_h,
        "wrapped document view inside scroll view should grow after narrowing: wide={wide_h}, narrow={narrow_h}"
    );
}

#[test]
fn implicit_auto_document_view_is_width_sensitive_in_scroll_view() {
    let el: Element = DocumentView::new("abcdefghijklmnopqrstuvwxyz")
        .border(false)
        .scrollbar(false)
        .h_scrollbar(false)
        .wrap(true)
        .focusable(false)
        .into();

    assert!(scroll_child_height_depends_on_width(&el));
}

#[test]
fn focusable_default_flex_document_view_is_not_width_sensitive_in_scroll_view() {
    let el: Element = DocumentView::new("abcdefghijklmnopqrstuvwxyz")
        .border(false)
        .scrollbar(false)
        .h_scrollbar(false)
        .wrap(true)
        .focusable(true)
        .into();

    assert!(!scroll_child_height_depends_on_width(&el));
}

#[test]
fn scroll_content_hashes_ignore_viewport_h_for_fixed_height_rows() {
    use crate::widgets::internal::StackProps;

    let props = StackProps::default();
    let children: Vec<Element> = (0..12u8)
        .map(|i| {
            Text::new(format!("row {i}"))
                .height(Length::Px(1))
                .key(format!("r{i}"))
        })
        .collect();

    let viewport_h_dep = children
        .iter()
        .any(scroll_child_height_depends_on_scroll_viewport_h);
    assert!(!viewport_h_dep);

    let lo = scroll_content_hashes(&props, &children, 50, 12, viewport_h_dep, false).unwrap();
    let hi = scroll_content_hashes(&props, &children, 50, 120, viewport_h_dep, false).unwrap();
    assert_eq!(lo.layout_hash, hi.layout_hash);
    assert_eq!(lo.content_hash_no_width, hi.content_hash_no_width);
}

#[test]
fn scroll_content_hashes_include_viewport_h_when_percent_min_height() {
    use crate::widgets::internal::StackProps;

    let props = StackProps::default();
    let children = vec![
        Text::new("x")
            .height(Length::Auto)
            .min_height(Length::Percent(10)),
    ];

    let viewport_h_dep = children
        .iter()
        .any(scroll_child_height_depends_on_scroll_viewport_h);
    assert!(viewport_h_dep);

    let short = scroll_content_hashes(&props, &children, 50, 20, viewport_h_dep, false).unwrap();
    let tall = scroll_content_hashes(&props, &children, 50, 80, viewport_h_dep, false).unwrap();
    assert_ne!(short.layout_hash, tall.layout_hash);
    assert_ne!(short.content_hash_no_width, tall.content_hash_no_width);
}

#[test]
fn scroll_child_percent_min_height_depends_on_scroll_viewport_h() {
    let el: Element = Text::new("x")
        .height(Length::Auto)
        .min_height(Length::Percent(10));
    assert!(scroll_child_height_depends_on_scroll_viewport_h(&el));
}

#[test]
fn theme_wrapped_document_view_resize_remeasures_wrapped_height() {
    use crate::widgets::internal::StackProps;

    fn row(i: usize) -> Element {
        ThemeProvider::new(Theme::default())
                .child(
                    DocumentView::new(format!(
                        "row {i}: this is deliberately long enough to wrap several times at narrow widths but fewer times at wider widths"
                    ))
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .height(Length::Auto)
                    .key(format!("row-{i}")),
                )
                .into()
    }

    let props = StackProps::default();
    let children: Vec<Element> = (0..4).map(row).collect();
    let mut layout_cache = ScrollViewLayoutCache::default();
    let mut virtual_cache = VirtualHeightCache::default();

    let narrow = layout_scroll_content_cached(
        &props,
        &children,
        &mut layout_cache,
        &mut virtual_cache,
        ScrollLayoutCachedParams {
            viewport_w: 22,
            viewport_h: 12,
            scroll_offset: 0,
            estimated_child_height: 1,
            horizontal_overflow: false,
        },
    );
    let wide = layout_scroll_content_cached(
        &props,
        &children,
        &mut layout_cache,
        &mut virtual_cache,
        ScrollLayoutCachedParams {
            viewport_w: 80,
            viewport_h: 12,
            scroll_offset: 0,
            estimated_child_height: 1,
            horizontal_overflow: false,
        },
    );
    let exact_wide = layout_scroll_content(&props, &children, 80, 12, false);

    assert!(
        narrow.content_height > exact_wide.content_height,
        "test fixture must shrink when the viewport widens"
    );
    assert_eq!(wide.content_height, exact_wide.content_height);
    assert!(scroll_child_height_depends_on_width(&children[0]));
}

#[test]
fn partial_virtual_layout_does_not_seed_exact_cache_with_stale_heights() {
    use crate::widgets::internal::StackProps;

    let props = StackProps::default();
    let mut layout_cache = ScrollViewLayoutCache::default();
    let mut virtual_cache = VirtualHeightCache::default();

    let tall: Vec<_> = (0..12)
        .map(|i| Text::new("x").height(Length::Px(5)).key(format!("row-{i}")))
        .collect();
    let _ = layout_scroll_content_cached(
        &props,
        &tall,
        &mut layout_cache,
        &mut virtual_cache,
        ScrollLayoutCachedParams {
            viewport_w: 20,
            viewport_h: 2,
            scroll_offset: 0,
            estimated_child_height: 5,
            horizontal_overflow: false,
        },
    );

    let short: Vec<_> = (0..12)
        .map(|i| Text::new("x").height(Length::Px(1)).key(format!("row-{i}")))
        .collect();
    let partial = layout_scroll_content_cached(
        &props,
        &short,
        &mut layout_cache,
        &mut virtual_cache,
        ScrollLayoutCachedParams {
            viewport_w: 20,
            viewport_h: 2,
            scroll_offset: 0,
            estimated_child_height: 5,
            horizontal_overflow: false,
        },
    );
    assert!(
        partial.content_height > 12,
        "the first virtual pass may still estimate offscreen stale rows"
    );
    assert!(
        virtual_cache
            .entries
            .iter()
            .flatten()
            .any(|entry| entry.stale),
        "offscreen stale rows remain after a tight-buffer virtual pass"
    );

    // If the partial virtual result was promoted into the exact/height cache,
    // a later cold pass for the same content would incorrectly reuse it.
    virtual_cache.reset();
    let exact = layout_scroll_content_cached(
        &props,
        &short,
        &mut layout_cache,
        &mut virtual_cache,
        ScrollLayoutCachedParams {
            viewport_w: 20,
            viewport_h: 2,
            scroll_offset: 0,
            estimated_child_height: 5,
            horizontal_overflow: false,
        },
    );
    assert_eq!(exact.content_height, 12);
}

#[test]
fn flow_scroll_rows_use_exact_layout_cache() {
    use crate::widgets::internal::StackProps;

    let props = StackProps::default();
    let children: Vec<Element> = vec![
        Flow::new()
            .gap(1)
            .height(Length::Auto)
            .child(Text::new("attachment").height(Length::Px(1)))
            .child(Text::new("badge").height(Length::Px(1)))
            .into(),
    ];
    let viewport_w = 20;
    let viewport_h = 5;
    let viewport_h_dep = children
        .iter()
        .any(scroll_child_height_depends_on_scroll_viewport_h);
    let hashes = scroll_content_hashes(
        &props,
        &children,
        viewport_w,
        viewport_h,
        viewport_h_dep,
        false,
    )
    .expect("flow rows should be layout-hashable");

    let mut layout_cache = ScrollViewLayoutCache::default();
    let mut virtual_cache = VirtualHeightCache::default();
    let initial = layout_scroll_content_cached(
        &props,
        &children,
        &mut layout_cache,
        &mut virtual_cache,
        ScrollLayoutCachedParams {
            viewport_w,
            viewport_h,
            scroll_offset: 0,
            estimated_child_height: 1,
            horizontal_overflow: false,
        },
    );

    let sentinel_rects = vec![Rect {
        x: 2,
        y: 3,
        w: 5,
        h: 7,
    }];
    let sentinel_height = initial.content_height.saturating_add(11);
    layout_cache.insert(
        viewport_w,
        hashes.layout_hash,
        sentinel_rects.clone(),
        sentinel_height,
    );

    let cached = layout_scroll_content_cached(
        &props,
        &children,
        &mut layout_cache,
        &mut virtual_cache,
        ScrollLayoutCachedParams {
            viewport_w,
            viewport_h,
            scroll_offset: 3,
            estimated_child_height: 1,
            horizontal_overflow: false,
        },
    );

    assert_eq!(cached.rects, sentinel_rects);
    assert_eq!(cached.content_height, sentinel_height);
}

#[cfg(feature = "diff-view")]
#[test]
fn split_wrap_scroll_layout_uses_exact_cache_for_same_width_and_content() {
    use crate::widgets::internal::StackProps;

    let props = StackProps::default();
    let children: Vec<Element> = vec![
        DiffView::with_content(
            "fn main() { println!(\"old value\"); }",
            "fn main() { println!(\"new value with a longer wrapped body\"); }",
        )
        .mode(DiffViewMode::Split)
        .backend(DiffViewBackend::DocumentView)
        .height(Length::Auto)
        .wrap(true)
        .border(false)
        .panels_border(false)
        .scrollbar(false)
        .h_scrollbar(false)
        .key("diff"),
    ];

    let has_split_wrap_sync = children
        .iter()
        .any(crate::widgets::element_subtree_has_split_wrap_sync);
    assert!(has_split_wrap_sync);

    let viewport_w = 80;
    let viewport_h = 20;
    let viewport_h_dep = children
        .iter()
        .any(scroll_child_height_depends_on_scroll_viewport_h);
    let mut layout_cache = ScrollViewLayoutCache::default();
    let mut virtual_cache = VirtualHeightCache::default();
    let initial = layout_scroll_content_cached(
        &props,
        &children,
        &mut layout_cache,
        &mut virtual_cache,
        ScrollLayoutCachedParams {
            viewport_w,
            viewport_h,
            scroll_offset: 0,
            estimated_child_height: 1,
            horizontal_overflow: false,
        },
    );

    let hashes = scroll_content_hashes(
        &props,
        &children,
        viewport_w,
        viewport_h,
        viewport_h_dep,
        has_split_wrap_sync,
    )
    .expect("split diff should be layout-hashable after width hints are populated");

    let sentinel_rects = vec![Rect {
        x: 3,
        y: 5,
        w: 7,
        h: 11,
    }];
    let sentinel_height = initial.content_height.saturating_add(37);
    layout_cache.insert(
        viewport_w,
        hashes.layout_hash,
        sentinel_rects.clone(),
        sentinel_height,
    );

    let cached = layout_scroll_content_cached(
        &props,
        &children,
        &mut layout_cache,
        &mut virtual_cache,
        ScrollLayoutCachedParams {
            viewport_w,
            viewport_h,
            scroll_offset: 12,
            estimated_child_height: 1,
            horizontal_overflow: false,
        },
    );

    assert_eq!(cached.rects, sentinel_rects);
    assert_eq!(cached.content_height, sentinel_height);
}

#[test]
fn post_reconcile_height_sync_ignores_partially_visible_child_rects() {
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 5,
    };
    let child: Element = Text::new("tall child").height(Length::Px(20)).key("row");
    let root: Element = ScrollView::new()
        .border(false)
        .scrollbar(false)
        .offset(8)
        .child(child.clone())
        .into();
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

    let NodeKind::ScrollView(_) = &tree.node(tree.root).kind else {
        panic!("expected root scroll view");
    };
    let child_id = *tree
        .node(tree.root)
        .children
        .first()
        .expect("expected visible child");
    tree.node_mut(child_id).rect.h = bounds.h;
    let visible_rect = tree.node(child_id).rect;
    let full_content_height = 20;
    assert!(visible_rect.h < full_content_height);

    let content_layout = crate::layout::stack::make_scroll_content_layout(
        vec![Rect {
            x: 0,
            y: 0,
            w: bounds.w,
            h: full_content_height,
        }],
        full_content_height,
    );
    let synced = recompute_scroll_content_height_with_reconciled_roots(
        &[child],
        &content_layout,
        &[0],
        &[visible_rect],
        &[child_id],
        &tree,
        0,
    );

    assert_eq!(
        synced.content_height, full_content_height,
        "clipped visible child height must not replace full scroll content height",
    );
}

#[derive(Clone, Default)]
struct ThemeHeightFormatter {
    extra_lines: usize,
}

impl crate::widgets::ContentFormatter for ThemeHeightFormatter {
    fn clone_box(&self) -> Box<dyn crate::widgets::ContentFormatter> {
        Box::new(self.clone())
    }

    fn set_app_theme_if_absent(&mut self, theme: &Theme) {
        self.extra_lines = if theme.primary.bg == Some(Color::Black.into()) {
            0
        } else {
            3
        };
    }

    fn format(&self, input: crate::widgets::FormatInput<'_>) -> crate::widgets::FormattedDocument {
        let mut lines = Vec::new();
        for (idx, line) in input.value.lines().enumerate() {
            lines.push(crate::widgets::FormattedLine {
                spans: vec![crate::style::Span::new(line.to_string())],
                source_line: idx,
                indent: 0,
                links: Vec::new(),
            });
            for extra in 0..self.extra_lines {
                lines.push(crate::widgets::FormattedLine {
                    spans: vec![crate::style::Span::new(format!("theme extra {extra}"))],
                    source_line: idx,
                    indent: 0,
                    links: Vec::new(),
                });
            }
        }
        crate::widgets::FormattedDocument {
            blocks: vec![crate::widgets::FormattedBlock::Lines(lines)],
        }
    }

    fn measure_format(
        &self,
        input: crate::widgets::FormatInput<'_>,
    ) -> crate::widgets::FormattedDocument {
        let lines = input
            .value
            .lines()
            .enumerate()
            .map(|(idx, line)| crate::widgets::FormattedLine {
                spans: vec![crate::style::Span::new(line.to_string())],
                source_line: idx,
                indent: 0,
                links: Vec::new(),
            })
            .collect();
        crate::widgets::FormattedDocument {
            blocks: vec![crate::widgets::FormattedBlock::Lines(lines)],
        }
    }

    fn cache_key(&self) -> u64 {
        self.extra_lines as u64
    }

    fn measure_cache_key(&self) -> u64 {
        0
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn theme_height_theme(bg: Color) -> Theme {
    Theme::custom(Color::White, bg, Color::Cyan)
}

fn theme_height_row(i: usize) -> Element {
    DocumentView::new(format!("row {i}"))
        .formatter(ThemeHeightFormatter::default())
        .border(false)
        .scrollbar(false)
        .h_scrollbar(false)
        .wrap(true)
        .height(Length::Auto)
        .key(format!("row-{i}"))
}

fn theme_height_scroll_root(offset: usize, theme: Theme, row_count: usize) -> Element {
    let root: Element = ScrollView::new()
        .scrollbar(false)
        .offset(offset)
        .children((0..row_count).map(theme_height_row))
        .into();
    crate::style::apply_document_theme_carve_out(&theme, root)
}

fn scroll_view_children(root: &Element) -> &[Element] {
    let ElementKind::ScrollView(scroll) = &root.kind else {
        panic!("expected scroll view root");
    };
    &scroll.children
}

#[test]
fn scroll_anchor_survives_post_reconcile_auto_height_drift() {
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 8,
    };
    let mut tree = NodeTree::new();
    let before = theme_height_scroll_root(18, theme_height_theme(Color::Black), 40);
    LayoutEngine::reconcile_with_focus(&mut tree, &before, bounds, None);
    let before_anchor =
        compute_visible_scroll_anchor(&tree, tree.root, scroll_view_children(&before))
            .expect("expected visible anchor before theme change");

    let after = theme_height_scroll_root(18, theme_height_theme(Color::rgb(8, 8, 24)), 40);
    LayoutEngine::reconcile_with_focus(&mut tree, &after, bounds, None);
    let after_anchor =
        compute_visible_scroll_anchor(&tree, tree.root, scroll_view_children(&after))
            .expect("expected visible anchor after theme change");

    assert_eq!(after_anchor.top_child_key, before_anchor.top_child_key);

    let settled_offset = root_scroll_offset(&tree);
    let before_append_top = first_visible_key_and_y(&tree, tree.root);
    let appended =
        theme_height_scroll_root(settled_offset, theme_height_theme(Color::rgb(8, 8, 24)), 45);
    LayoutEngine::reconcile_with_focus(&mut tree, &appended, bounds, None);

    assert_eq!(root_scroll_offset(&tree), settled_offset);
    assert_eq!(
        first_visible_key_and_y(&tree, tree.root),
        before_append_top,
        "appending below post-reconcile auto-height rows should not move the scrolled-up viewport"
    );
}

#[test]
fn append_during_post_reconcile_auto_height_drift_keeps_top_visible_row() {
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 8,
    };
    let offset = 18usize;
    let mut tree = NodeTree::new();
    let before = theme_height_scroll_root(offset, theme_height_theme(Color::Black), 40);

    LayoutEngine::reconcile_with_focus(&mut tree, &before, bounds, None);
    assert_eq!(root_scroll_offset(&tree), offset);
    let before_top = first_visible_key_and_y(&tree, tree.root);
    let before_top_node = find_by_key(&tree, &before_top.0).expect("top row should be visible");
    assert_eq!(tree.node(before_top_node).rect.h, 1);

    let appended_with_drift =
        theme_height_scroll_root(offset, theme_height_theme(Color::rgb(8, 8, 24)), 45);
    LayoutEngine::reconcile_with_focus(&mut tree, &appended_with_drift, bounds, None);

    assert_eq!(root_scroll_offset(&tree), offset);
    assert_eq!(
        first_visible_key_and_y(&tree, tree.root),
        before_top,
        "appending while post-reconcile height drift fires should not move the top visible row"
    );
    let after_top_node = find_by_key(&tree, &before_top.0).expect("top row should remain visible");
    assert!(
        tree.node(after_top_node).rect.h > 1,
        "fixture should exercise post-reconcile auto-height growth on the append frame"
    );
}

#[derive(Default)]
struct GrowRootState {
    full: bool,
}

enum GrowRootMsg {
    Grow,
}

struct GrowRoot;

impl crate::core::component::Component for GrowRoot {
    type Message = GrowRootMsg;
    type Properties = ();
    type State = GrowRootState;

    fn create_state(&self, _: &Self::Properties) -> Self::State {
        GrowRootState::default()
    }

    fn update(
        &mut self,
        msg: Self::Message,
        ctx: &mut crate::core::component::Context<Self>,
    ) -> crate::core::component::Update {
        match msg {
            GrowRootMsg::Grow => {
                ctx.state.full = true;
                crate::core::component::Update::full()
            }
        }
    }

    fn view(&self, ctx: &crate::core::component::Context<Self>) -> Element {
        // Non-black background drives `ThemeHeightFormatter` to inflate each
        // source line into 4 visual lines at *render* time while `measure`
        // reports just the source line — i.e. the height a row reports during
        // the scroll layout's measure pass is smaller than its reconciled
        // height, exactly like themed markdown in a `DocumentView`.
        let theme = theme_height_theme(Color::rgb(8, 8, 24));
        // The last row's content grows in place when `full` flips, mirroring a
        // streamed assistant message that finishes while its tab was away and
        // gets replaced by the fuller server snapshot on the next render.
        let last_body = if ctx.state.full {
            "GROWN-A\nGROWN-B\nGROWN-C\nGROWN-D"
        } else {
            "small"
        };
        let mut children: Vec<Element> = (0..6)
            .map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(2))
                    .key(format!("row-{i}"))
            })
            .collect();
        children.push(
            DocumentView::new(last_body)
                .formatter(ThemeHeightFormatter::default())
                .border(false)
                .scrollbar(false)
                .h_scrollbar(false)
                .wrap(true)
                .height(Length::Auto)
                .key("grow-row"),
        );
        // `offset(usize::MAX)` is the tail sentinel: keep pinned to the bottom
        // across the in-place growth.
        let root: Element = ScrollView::new()
            .scrollbar(false)
            .offset(usize::MAX)
            .children(children)
            .key("grow-scroll");
        crate::style::apply_document_theme_carve_out(&theme, root)
    }
}

/// Regression: when a tail-pinned `ScrollView` child grows in place and its
/// reconciled height exceeds the measure-pass estimate, the rows freshly
/// exposed at the bottom must actually be painted — not left blank with their
/// height merely reserved.
#[test]
fn tail_pinned_in_place_growth_paints_exposed_rows() {
    let mut backend = crate::TestBackend::new(GrowRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 8,
    });
    backend.render();

    backend.dispatch(GrowRootMsg::Grow).expect("dispatch grow");
    backend.render();

    let frame = backend.capture_frame();
    let mut rows = Vec::new();
    for y in 0..frame.height {
        let mut line = String::new();
        for x in 0..frame.width {
            line.push_str(&frame.cell(x, y).symbol);
        }
        rows.push(line);
    }
    let rendered = rows.join("\n");

    // The bottom of the grown row should be visible while pinned to the tail.
    assert!(
        rendered.contains("GROWN-D"),
        "tail-pinned grown row bottom not painted after in-place growth:\n{rendered}"
    );
    // ...and the pre-growth top should have scrolled out of view, not lingered.
    assert!(
        !rendered.contains("GROWN-A"),
        "stale top of grown row still painted after tail follow:\n{rendered}"
    );
}

#[test]
fn append_with_indicators_keeps_exact_offset() {
    fn root(row_count: usize, offset: usize) -> Element {
        ScrollView::new()
            .show_scroll_indicators(true)
            .scrollbar(false)
            .offset(offset)
            .children((0..row_count).map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(3))
                    .key(format!("row-{i}"))
            }))
            .into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 36,
        h: 10,
    };
    let target_offset = 30usize;
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &root(30, target_offset), bounds, None);
    assert_eq!(root_scroll_offset(&tree), target_offset);

    for row_count in 31..=36 {
        // First frame: parent still provides the pre-append controlled offset.
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root(row_count, target_offset),
            bounds,
            None,
        );
        assert_eq!(
            root_scroll_offset(&tree),
            target_offset,
            "stale controlled offset drifted after append at row_count={row_count}"
        );

        // Second frame: parent has caught up to the offset emitted by the node.
        let caught_up = root_scroll_offset(&tree);
        LayoutEngine::reconcile_with_focus(&mut tree, &root(row_count, caught_up), bounds, None);
        assert_eq!(
            root_scroll_offset(&tree),
            target_offset,
            "caught-up controlled offset drifted after append at row_count={row_count}"
        );
    }
}

#[test]
fn append_keeps_offset_when_center_anchor_child_is_short() {
    fn alternating_root(row_count: usize, offset: usize) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..row_count).map(|i| {
                let height = if i % 2 == 0 { 7 } else { 1 };
                Text::new(format!("row {i}"))
                    .height(Length::Px(height))
                    .key(format!("row-{i}"))
            }))
            .into()
    }

    fn gapped_root(row_count: usize, offset: usize) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .gap(2)
            .offset(offset)
            .children((0..row_count).map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(3))
                    .key(format!("row-{i}"))
            }))
            .into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 36,
        h: 10,
    };

    let mut tree = NodeTree::new();
    let offset = 10usize;
    LayoutEngine::reconcile_with_focus(&mut tree, &alternating_root(20, offset), bounds, None);
    assert_eq!(root_scroll_offset(&tree), offset);
    let before_center = center_visible_key_and_y(&tree, tree.root);
    assert_eq!(
        before_center.0, "row-3",
        "fixture should place the viewport center on a short row"
    );

    LayoutEngine::reconcile_with_focus(&mut tree, &alternating_root(21, offset), bounds, None);
    assert_eq!(root_scroll_offset(&tree), offset);
    assert_eq!(
        center_visible_key_and_y(&tree, tree.root),
        before_center,
        "appending below should not move a short center anchor row"
    );

    // Also cover the top edge being in an inter-row gap. The first visible child
    // starts below screen row 0, so the top-edge delta is negative.
    let mut gap_tree = NodeTree::new();
    let gap_offset = 4usize;
    LayoutEngine::reconcile_with_focus(&mut gap_tree, &gapped_root(12, gap_offset), bounds, None);
    assert_eq!(root_scroll_offset(&gap_tree), gap_offset);
    let before_top = first_visible_key_and_y(&gap_tree, gap_tree.root);
    assert_eq!(before_top, ("row-1".to_string(), 1));

    LayoutEngine::reconcile_with_focus(&mut gap_tree, &gapped_root(13, gap_offset), bounds, None);
    assert_eq!(root_scroll_offset(&gap_tree), gap_offset);
    assert_eq!(
        first_visible_key_and_y(&gap_tree, gap_tree.root),
        before_top,
        "appending below should keep a gap-offset top anchor in the same screen row"
    );
}

#[test]
fn virtual_estimate_repeated_appends_keep_visible_anchor_stable() {
    fn row_height(i: usize) -> u16 {
        match i % 5 {
            0 => 1,
            1 => 4,
            2 => 2,
            3 => 5,
            _ => 3,
        }
    }

    fn root(row_count: usize, offset: usize) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..row_count).map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(row_height(i)))
                    .key(format!("row-{i}"))
            }))
            .into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 36,
        h: 10,
    };
    let initial_count = 24usize;
    let offset = 18usize;
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &root(initial_count, offset), bounds, None);
    assert_eq!(root_scroll_offset(&tree), offset);
    let expected_top = first_visible_key_and_y(&tree, tree.root);

    // More than `VIRTUAL_THRESHOLD` rows: after the initial full layout, repeated
    // appends exercise the virtual estimate path while keeping the viewport stable.
    for row_count in (initial_count + 1)..=(initial_count + 10) {
        LayoutEngine::reconcile_with_focus(&mut tree, &root(row_count, offset), bounds, None);
        assert_eq!(
            root_scroll_offset(&tree),
            offset,
            "virtual append drifted offset at row_count={row_count}"
        );
        assert_eq!(
            first_visible_key_and_y(&tree, tree.root),
            expected_top,
            "virtual append moved top visible row at row_count={row_count}"
        );
    }
}

#[test]
fn anchor_child_removed_keeps_clamped_offset() {
    fn root(row_count: usize, offset: usize, removed_key: Option<&str>) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..row_count).filter_map(move |i| {
                let key = format!("row-{i}");
                if removed_key == Some(key.as_str()) {
                    None
                } else {
                    Some(Text::new(format!("row {i}")).height(Length::Px(3)).key(key))
                }
            }))
            .into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 36,
        h: 10,
    };
    let offset = 30usize;
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &root(20, offset, None), bounds, None);
    let before_offset = root_scroll_offset(&tree);
    assert_eq!(before_offset, offset);
    let before_top = first_visible_key_and_y(&tree, tree.root);
    let (removed_center_key, _) = center_visible_key_and_y(&tree, tree.root);
    assert_ne!(
        removed_center_key, before_top.0,
        "fixture should remove the center anchor while leaving the top anchor available"
    );

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root(20, before_offset, Some(&removed_center_key)),
        bounds,
        None,
    );

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected root scroll view");
    };
    let expected_clamped = before_offset.min(scroll.max_offset);
    assert_ne!(
        scroll.offset, 0,
        "removing the anchor child must not reset a scrolled viewport to the top"
    );
    assert_eq!(
        scroll.offset, expected_clamped,
        "removed anchor child should keep the previous offset clamped to the new range"
    );
}

#[test]
fn missing_keyed_center_anchor_falls_back_to_old_offset() {
    use crate::widgets::internal::StackProps;

    fn row(i: usize, height: u16) -> Element {
        Text::new(format!("row {i}"))
            .height(Length::Px(height))
            .key(format!("row-{i}"))
    }

    let props = StackProps::default();
    let viewport_w = 36u16;
    let viewport_h = 10u16;
    let old_offset = 10usize;
    let old_children = vec![
        row(0, 5),
        row(1, 5),
        row(2, 20),
        row(3, 5),
        row(4, 5),
        row(5, 5),
    ];
    let old_layout = layout_scroll_content(&props, &old_children, viewport_w, viewport_h, false);
    let old_max_offset = calc_scroll_view_window(
        0,
        old_layout.content_height as usize,
        viewport_h as usize,
        false,
    )
    .max_offset;
    let anchor = compute_layout_scroll_anchor(
        &old_children,
        &old_layout.rects,
        old_offset,
        viewport_h,
        old_max_offset,
    )
    .expect("expected old layout anchor");
    assert_eq!(anchor.center_child_key, Some(Key::from("row-2")));

    let new_children = vec![row(0, 5), row(1, 5), row(3, 5), row(4, 5), row(5, 5)];
    let new_layout = layout_scroll_content(&props, &new_children, viewport_w, viewport_h, false);
    let expected = old_offset.min(
        calc_scroll_view_window(
            0,
            new_layout.content_height as usize,
            viewport_h as usize,
            false,
        )
        .max_offset,
    );

    assert_eq!(
        apply_scroll_anchor(
            &new_children,
            &new_layout.rects,
            &anchor,
            new_layout.content_height,
            viewport_h,
            false,
        ),
        expected,
        "missing keyed center anchor should clamp the old offset instead of anchoring to the shifted positional index"
    );
}

#[test]
fn layout_scroll_anchor_top_edge_round_trips_with_indicators() {
    use crate::widgets::internal::StackProps;

    let props = StackProps::default();
    let children: Vec<Element> = (0..20)
        .map(|i| {
            Text::new(format!("row {i}"))
                .height(Length::Px(3))
                .key(format!("row-{i}"))
        })
        .collect();
    let viewport_w = 36u16;
    let viewport_h = 10u16;
    let layout = layout_scroll_content(&props, &children, viewport_w, viewport_h, false);
    let max_offset =
        calc_scroll_view_window(0, layout.content_height as usize, viewport_h as usize, true)
            .max_offset;

    for offset in (6usize..=36usize).step_by(3) {
        assert!(
            offset < max_offset,
            "fixture offset should stay away from bottom pinning"
        );
        let anchor =
            compute_layout_scroll_anchor(&children, &layout.rects, offset, viewport_h, max_offset)
                .expect("expected layout anchor");
        let actual = apply_scroll_anchor_top_edge(
            &children,
            &layout.rects,
            &anchor,
            layout.content_height,
            viewport_h,
            true,
        );

        assert_eq!(
            actual, offset,
            "top-edge anchor should round-trip safe offset {offset} with scroll indicators"
        );
    }
}

#[test]
fn wrapped_document_view_virtual_scroll_keeps_visible_height_correct_after_resize() {
    fn make_row(i: usize, body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(body)
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .height(Length::Auto),
            )
            .key(format!("frame-{i}"))
    }

    fn make_root(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..24).map(|i| make_row(i, body)))
            .into()
    }

    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
    let mut tree = NodeTree::new();

    // First pass (wide) seeds caches.
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(120, body),
        Rect {
            x: 0,
            y: 0,
            w: 72,
            h: 10,
        },
        None,
    );

    // Resize narrower at a non-zero offset to exercise virtual-scroll
    // width-sensitive handling.
    let narrow_w = 24u16;
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(120, body),
        Rect {
            x: 0,
            y: 0,
            w: narrow_w,
            h: 10,
        },
        None,
    );

    let scroll_children = tree.node(tree.root).children.clone();
    assert!(
        !scroll_children.is_empty(),
        "scroll view should render children"
    );

    // Validate the top visible row uses the true wrap-aware measured height,
    // not an estimated/offscreen placeholder height.
    let top_id = scroll_children[0];
    let actual_h = tree.node(top_id).rect.h;
    let top_key = tree
        .node(top_id)
        .key
        .as_ref()
        .expect("visible child should keep key")
        .as_ref();
    let idx: usize = top_key
        .strip_prefix("frame-")
        .expect("key prefix")
        .parse()
        .expect("numeric key suffix");

    let expected_h = min_size_constrained(&make_row(idx, body), Some(narrow_w), None).1;
    assert_eq!(
        actual_h, expected_h,
        "visible wrapped row should use wrap-aware measured height"
    );
}

#[test]
fn wrapped_document_view_virtual_scroll_keeps_height_correct_with_standalone_scrollbar() {
    fn make_row(i: usize, body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(body)
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .height(Length::Auto),
            )
            .key(format!("frame-{i}"))
    }

    fn make_root(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .scrollbar(true)
            .scrollbar_config(ScrollbarConfig::new().variant(ScrollbarVariant::Standalone))
            .offset(offset)
            .children((0..24).map(|i| make_row(i, body)))
            .into()
    }

    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(120, body),
        Rect {
            x: 0,
            y: 0,
            w: 72,
            h: 10,
        },
        None,
    );

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(120, body),
        Rect {
            x: 0,
            y: 0,
            w: 24,
            h: 10,
        },
        None,
    );

    let scroll_children = tree.node(tree.root).children.clone();
    let top_id = scroll_children[0];
    let row_width = tree.node(top_id).rect.w;
    let actual_h = tree.node(top_id).rect.h;
    let top_key = tree
        .node(top_id)
        .key
        .as_ref()
        .expect("visible child should keep key")
        .as_ref();
    let idx: usize = top_key
        .strip_prefix("frame-")
        .expect("key prefix")
        .parse()
        .expect("numeric key suffix");

    let expected_h = min_size_constrained(&make_row(idx, body), Some(row_width), None).1;
    assert_eq!(
        actual_h, expected_h,
        "visible wrapped row should keep wrap-aware height with standalone scrollbar"
    );
}

#[test]
fn non_zero_offset_preserves_visible_wrap_height_with_long_rows() {
    fn make_row(i: usize, body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(body)
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .height(Length::Auto),
            )
            .key(format!("frame-{i}"))
    }

    fn make_root(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .scrollbar(true)
            .scrollbar_config(ScrollbarConfig::new().variant(ScrollbarVariant::Standalone))
            .offset(offset)
            .children((0..60).map(|i| make_row(i, body)))
            .into()
    }

    let body = "I dug deeper and the remaining memory growth was not the keyed screen anymore - it was PTY transport/state being kept app-global and surviving session switches.";

    let mut tree = NodeTree::new();

    // Wide render to seed caches.
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(140, body),
        Rect {
            x: 0,
            y: 0,
            w: 84,
            h: 10,
        },
        None,
    );

    // Narrow render at non-zero offset (problematic case).
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(140, body),
        Rect {
            x: 0,
            y: 0,
            w: 28,
            h: 10,
        },
        None,
    );

    let visible = tree.node(tree.root).children.clone();
    assert!(
        !visible.is_empty(),
        "scroll view should render visible rows"
    );

    // Validate first 3 visible rows all use true measured heights.
    for &node_id in visible.iter().take(3) {
        let actual_h = tree.node(node_id).rect.h;
        let row_w = tree.node(node_id).rect.w;
        let key = tree.node(node_id).key.as_ref().expect("row key").as_ref();
        let idx: usize = key
            .strip_prefix("frame-")
            .expect("frame key")
            .parse()
            .expect("numeric suffix");

        let expected_h = min_size_constrained(&make_row(idx, body), Some(row_w), None).1;
        assert_eq!(
            actual_h, expected_h,
            "visible row should keep exact wrap-aware height at non-zero offset"
        );
    }
}

#[test]
fn resize_preserves_scroll_position_via_anchor() {
    // 10 children each 5px tall = 50px content, viewport 10px.
    // Scroll to row-4 (offset 20) then simulate resize.
    let make_root = |offset: usize| -> Element {
        ScrollView::new()
            .offset(offset)
            .children((0..10).map(|i| {
                Text::new(format!("Row {i}"))
                    .height(Length::Px(5))
                    .key(format!("row-{i}"))
            }))
            .into()
    };

    let mut tree = NodeTree::new();
    let wide_bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    };
    let narrow_bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 10,
    };

    // Initial render at offset 20 (row-4 at viewport top).
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(20), wide_bounds, None);

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(scroll.offset, 20);

    // Resize without changing offset - anchor should keep row-4 at top.
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(20), narrow_bounds, None);

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    // Child heights don't depend on width (Px), so offset stays at 20.
    assert_eq!(scroll.offset, 20);
}

#[test]
fn resize_keeps_same_visible_keyed_row_when_wrapped_rows_reflow() {
    fn center_visible_key(tree: &NodeTree) -> String {
        let root = tree.root;
        let NodeKind::ScrollView(scroll) = &tree.node(root).kind else {
            panic!("expected scroll view");
        };
        let viewport_center = i32::from(scroll.viewport_height / 2);

        tree.node(root)
            .children
            .iter()
            .copied()
            .min_by_key(|id| {
                let rect = tree.node(*id).rect;
                let rect_center = i32::from(rect.y) + i32::from(rect.h / 2);
                (rect_center - viewport_center).abs()
            })
            .and_then(|id| {
                tree.node(id)
                    .key
                    .as_ref()
                    .map(|key| key.as_ref().to_string())
            })
            .expect("center visible child key")
    }

    fn make_row(i: usize, body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(body)
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .height(Length::Auto),
            )
            .key(format!("frame-{i}"))
    }

    fn make_root(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..20).map(|i| make_row(i, body)))
            .into()
    }

    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
    let wide_w = 72u16;
    let narrow_w = 24u16;
    let viewport_h = 10u16;
    let wide_row_h = min_size_constrained(&make_row(0, body), Some(wide_w), None).1 as usize;
    let target_row = 6usize;
    let offset = wide_row_h * target_row;

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(offset, body),
        Rect {
            x: 0,
            y: 0,
            w: wide_w,
            h: viewport_h,
        },
        None,
    );

    let wide_center_key = center_visible_key(&tree);

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(offset, body),
        Rect {
            x: 0,
            y: 0,
            w: narrow_w,
            h: viewport_h,
        },
        None,
    );

    let narrow_center_key = center_visible_key(&tree);
    assert_eq!(narrow_center_key, wide_center_key);
}

#[test]
fn resize_keeps_bottom_pinned_for_wrapped_rows() {
    fn make_row(i: usize, body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(body)
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .height(Length::Auto),
            )
            .key(format!("frame-{i}"))
    }

    fn make_root(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..16).map(|i| make_row(i, body)))
            .into()
    }

    let body = "I dug deeper and the remaining memory growth was not the keyed screen anymore - it was PTY transport/state being kept app-global and surviving session switches.";
    let wide_w = 84u16;
    let narrow_w = 28u16;
    let viewport_h = 10u16;
    let row_count = 16usize;
    let wide_row_h = min_size_constrained(&make_row(0, body), Some(wide_w), None).1 as usize;
    let wide_max_offset = wide_row_h
        .saturating_mul(row_count)
        .saturating_sub(viewport_h as usize);

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(wide_max_offset, body),
        Rect {
            x: 0,
            y: 0,
            w: wide_w,
            h: viewport_h,
        },
        None,
    );

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(wide_max_offset, body),
        Rect {
            x: 0,
            y: 0,
            w: narrow_w,
            h: viewport_h,
        },
        None,
    );

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(
        scroll.offset, scroll.max_offset,
        "bottom-pinned resize should stay at the new max offset"
    );
}

#[test]
fn resize_viewport_height_only_keeps_bottom_pinned() {
    let row_h: u16 = 2;
    let row_count = 30usize;
    let make_root = |offset: usize| -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..row_count).map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(row_h))
                    .key(format!("r{i}"))
            }))
            .into()
    };

    let w = 50u16;
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(9999),
        Rect {
            x: 0,
            y: 0,
            w,
            h: 12,
        },
        None,
    );
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    let stale = scroll.offset;
    assert_eq!(
        scroll.offset, scroll.max_offset,
        "precondition: start at bottom"
    );

    // Shorter viewport (drag terminal bottom up): must stay tail-pinned.
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(stale),
        Rect {
            x: 0,
            y: 0,
            w,
            h: 6,
        },
        None,
    );
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(
        scroll.offset, scroll.max_offset,
        "bottom pin lost after height shrink: offset={} max={}",
        scroll.offset, scroll.max_offset
    );

    let stale = scroll.offset;

    // Taller viewport: still tail-pinned.
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(stale),
        Rect {
            x: 0,
            y: 0,
            w,
            h: 18,
        },
        None,
    );
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(
        scroll.offset, scroll.max_offset,
        "bottom pin lost after height grow: offset={} max={}",
        scroll.offset, scroll.max_offset
    );
}

#[test]
fn tail_pin_survives_when_scrollview_is_reparented_next_to_sidebar() {
    fn timeline(offset: usize) -> Element {
        ScrollView::new()
            .scrollbar(true)
            .scrollbar_config(
                ScrollbarConfig::new()
                    .variant(ScrollbarVariant::Standalone)
                    .gap(1),
            )
            .offset(offset)
            .children((0..36).map(|i| {
                Text::new(format!("r{i}"))
                    .height(Length::Px(1))
                    .key(format!("row-{i}"))
            }))
            .key("timeline")
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 64,
        h: 12,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &timeline(9999), bounds, None);
    let NodeKind::ScrollView(s0) = &tree.node(tree.root).kind else {
        panic!("expected root scroll view");
    };
    assert_eq!(s0.offset, s0.max_offset);

    let sidebar: Element = Frame::new()
        .width(Length::Px(14))
        .border(false)
        .child(Text::new("sb"))
        .key("sidebar");

    let with_sidebar: Element = HStack::new()
        .width(Length::Flex(1))
        .height(Length::Flex(1))
        .child(timeline(9999))
        .child(sidebar)
        .into();

    LayoutEngine::reconcile_with_focus(&mut tree, &with_sidebar, bounds, None);

    let tid = find_by_key(&tree, "timeline").expect("timeline scroll");
    let NodeKind::ScrollView(s1) = &tree.node(tid).kind else {
        panic!("timeline should be a ScrollView");
    };
    assert_eq!(
        s1.offset, s1.max_offset,
        "reparented scroll should stay tail-pinned: offset={} max={}",
        s1.offset, s1.max_offset
    );
}

#[test]
fn middle_anchor_survives_when_scrollview_is_reparented_next_to_sidebar() {
    fn center_visible_key(tree: &NodeTree, scroll_id: NodeId) -> String {
        let NodeKind::ScrollView(scroll) = &tree.node(scroll_id).kind else {
            panic!("expected scroll view");
        };
        let viewport_center = i32::from(scroll.viewport_height / 2);

        tree.node(scroll_id)
            .children
            .iter()
            .copied()
            .min_by_key(|id| {
                let rect = tree.node(*id).rect;
                let rect_center = i32::from(rect.y) + i32::from(rect.h / 2);
                (rect_center - viewport_center).abs()
            })
            .and_then(|id| {
                tree.node(id)
                    .key
                    .as_ref()
                    .map(|key| key.as_ref().to_string())
            })
            .expect("center visible child key")
    }

    fn make_row(i: usize, body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(format!("{body} [{i}]"))
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .focusable(false)
                    .height(Length::Auto),
            )
            .key(format!("row-{i}"))
    }

    fn timeline(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..30).map(|i| make_row(i, body)))
            .key("timeline")
    }

    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz ";
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 86,
        h: 10,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &timeline(12, body), bounds, None);
    let expected_key = center_visible_key(&tree, tree.root);

    let sidebar: Element = Frame::new()
        .width(Length::Px(20))
        .border(false)
        .child(Text::new("sb"))
        .key("sidebar");

    let with_sidebar: Element = HStack::new()
        .width(Length::Flex(1))
        .height(Length::Flex(1))
        .child(timeline(12, body))
        .child(sidebar)
        .into();

    LayoutEngine::reconcile_with_focus(&mut tree, &with_sidebar, bounds, None);

    let tid = find_by_key(&tree, "timeline").expect("timeline scroll");
    let actual_key = center_visible_key(&tree, tid);
    assert_eq!(
        actual_key, expected_key,
        "reparented keyed scroll should preserve the anchored visible row"
    );
}

#[test]
fn middle_anchor_survives_when_framed_scrollview_moves_into_docked_layout() {
    fn center_visible_key(tree: &NodeTree, scroll_id: NodeId) -> String {
        let NodeKind::ScrollView(scroll) = &tree.node(scroll_id).kind else {
            panic!("expected scroll view");
        };
        let viewport_center = i32::from(scroll.viewport_height / 2);

        tree.node(scroll_id)
            .children
            .iter()
            .copied()
            .min_by_key(|id| {
                let rect = tree.node(*id).rect;
                let rect_center = i32::from(rect.y) + i32::from(rect.h / 2);
                (rect_center - viewport_center).abs()
            })
            .and_then(|id| {
                tree.node(id)
                    .key
                    .as_ref()
                    .map(|key| key.as_ref().to_string())
            })
            .expect("center visible child key")
    }

    fn visible_keys(tree: &NodeTree, scroll_id: NodeId) -> Vec<String> {
        tree.node(scroll_id)
            .children
            .iter()
            .copied()
            .filter_map(|id| {
                tree.node(id)
                    .key
                    .as_ref()
                    .map(|key| key.as_ref().to_string())
            })
            .collect()
    }

    fn make_row(i: usize, body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(format!("{body} [{i}]"))
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .focusable(false)
                    .height(Length::Auto),
            )
            .key(format!("row-{i}"))
    }

    fn timeline(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..30).map(|i| make_row(i, body)))
            .key("timeline")
    }

    fn main_content(offset: usize, body: &str) -> Element {
        VStack::new()
            .child(Text::new("header").height(Length::Px(1)))
            .child(
                Frame::new()
                    .border(true)
                    .child(timeline(offset, body))
                    .key("timeline-frame"),
            )
            .child(Text::new("input").height(Length::Px(1)))
            .into()
    }

    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz abcdefghijklmnopqrstuvwxyz";
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 96,
        h: 18,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &main_content(14, body), bounds, None);
    let tid = find_by_key(&tree, "timeline").expect("timeline scroll");
    let expected_key = center_visible_key(&tree, tid);

    let sidebar: Element = Frame::new()
        .width(Length::Px(24))
        .border(true)
        .child(Text::new("sidebar"))
        .key("sidebar");

    let with_sidebar: Element = HStack::new()
        .width(Length::Flex(1))
        .height(Length::Flex(1))
        .child(main_content(14, body))
        .child(sidebar)
        .into();

    LayoutEngine::reconcile_with_focus(&mut tree, &with_sidebar, bounds, None);

    let tid = find_by_key(&tree, "timeline").expect("timeline scroll");
    let actual_visible = visible_keys(&tree, tid);
    assert!(
        actual_visible.iter().any(|key| key == &expected_key),
        "framed scrollview should keep the anchored row visible across layout moves; visible={actual_visible:?} expected={expected_key}"
    );
}

#[test]
fn tail_pin_survives_sidebar_dock_with_width_sensitive_clamped_offset() {
    fn make_row(i: usize, body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(format!("{body} [row {i}]"))
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .focusable(false)
                    .height(Length::Auto),
            )
            .key(format!("frame-{i}"))
    }

    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz ";
    fn timeline(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..14).map(|i| make_row(i, body)))
            .key("timeline")
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 86,
        h: 10,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &timeline(9999, body), bounds, None);
    let NodeKind::ScrollView(s0) = &tree.node(tree.root).kind else {
        panic!("expected root scroll view");
    };
    let stale = s0.offset;
    assert_eq!(
        s0.offset, s0.max_offset,
        "precondition: at bottom when wide"
    );
    assert!(
        stale > 0 && stale < 10_000,
        "use numeric stale offset (not sentinel) to match real apps"
    );

    let sidebar: Element = Frame::new()
        .width(Length::Px(20))
        .border(false)
        .child(Text::new("sb"))
        .key("sidebar");

    let with_sidebar: Element = HStack::new()
        .width(Length::Flex(1))
        .height(Length::Flex(1))
        .child(timeline(stale, body))
        .child(sidebar)
        .into();

    LayoutEngine::reconcile_with_focus(&mut tree, &with_sidebar, bounds, None);

    let tid = find_by_key(&tree, "timeline").expect("timeline scroll");
    let NodeKind::ScrollView(s1) = &tree.node(tid).kind else {
        panic!("timeline should be a ScrollView");
    };
    assert_eq!(
        s1.offset, s1.max_offset,
        "expected tail pin after HStack dock with stale clamped offset: offset={} max={}",
        s1.offset, s1.max_offset
    );
}

#[test]
fn bottom_pin_survives_standalone_scrollbar_toggle_with_stale_element_offset() {
    fn make_root(offset: usize, show_scrollbar: bool) -> Element {
        ScrollView::new()
            .scrollbar(show_scrollbar)
            .scrollbar_config(
                ScrollbarConfig::new()
                    .variant(ScrollbarVariant::Standalone)
                    .gap(1),
            )
            .offset(offset)
            .children((0..24).map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(1))
                    .key(format!("r{i}"))
            }))
            .into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 28,
        h: 10,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(9999, true), bounds, None);
    let NodeKind::ScrollView(s0) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert!(s0.scrollbar, "precondition: standalone scrollbar visible");
    let frozen_stale = s0.offset;
    assert_eq!(s0.offset, s0.max_offset);

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(frozen_stale, false), bounds, None);
    let NodeKind::ScrollView(s1) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(
        s1.offset, s1.max_offset,
        "bottom pin lost after scrollbar off: offset={} max={}",
        s1.offset, s1.max_offset
    );

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(frozen_stale, true), bounds, None);
    let NodeKind::ScrollView(s2) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(
        s2.offset, s2.max_offset,
        "bottom pin lost after scrollbar on (stale offset): offset={} max={}",
        s2.offset, s2.max_offset
    );

    // App never catches up with `on_scroll` - same numeric offset across rapid toggles.
    for i in 0..6 {
        let show = i % 2 == 0;
        LayoutEngine::reconcile_with_focus(&mut tree, &make_root(frozen_stale, show), bounds, None);
        let NodeKind::ScrollView(s) = &tree.node(tree.root).kind else {
            panic!("expected scroll view");
        };
        assert_eq!(
            s.offset, s.max_offset,
            "bottom pin lost at toggle iter {i} (scrollbar={show}): offset={} max={}",
            s.offset, s.max_offset
        );
    }
}

#[test]
fn resize_preserves_relative_position_inside_single_long_visible_item() {
    fn make_root(offset: usize, long_body: &str) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children([
                Frame::new()
                    .border(false)
                    .height(Length::Auto)
                    .child(Text::new("short top"))
                    .key("top"),
                Frame::new()
                    .border(false)
                    .height(Length::Auto)
                    .child(
                        DocumentView::new(long_body)
                            .border(false)
                            .scrollbar(false)
                            .h_scrollbar(false)
                            .wrap(true)
                            .height(Length::Auto),
                    )
                    .key("long"),
                Frame::new()
                    .border(false)
                    .height(Length::Auto)
                    .child(Text::new("short bottom"))
                    .key("bottom"),
            ])
            .into()
    }

    fn long_row(long_body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(long_body)
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .height(Length::Auto),
            )
            .key("long")
    }

    let body = "This is a very long message block that should dominate the viewport and change height a lot when width shrinks. ".repeat(24);
    let wide_w = 84u16;
    let narrow_w = 28u16;
    let viewport_h = 12u16;

    let short_h =
        min_size_constrained(&Text::new("short top").into(), Some(wide_w), None).1 as usize;
    let wide_long_h = min_size_constrained(&long_row(&body), Some(wide_w), None).1 as usize;
    let narrow_long_h = min_size_constrained(&long_row(&body), Some(narrow_w), None).1 as usize;
    let offset = short_h + wide_long_h / 2usize - viewport_h as usize / 2usize;
    let expected = short_h + narrow_long_h / 2usize - viewport_h as usize / 2usize;

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(offset, &body),
        Rect {
            x: 0,
            y: 0,
            w: wide_w,
            h: viewport_h,
        },
        None,
    );
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(offset, &body),
        Rect {
            x: 0,
            y: 0,
            w: narrow_w,
            h: viewport_h,
        },
        None,
    );

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert!(
        scroll.offset.abs_diff(expected) <= 2,
        "resize should preserve relative position inside the long item: actual={}, expected={expected}",
        scroll.offset,
    );
}

#[test]
fn resize_pins_to_top_when_scrolled_to_zero() {
    let make_root = || -> Element {
        ScrollView::new()
            .offset(0)
            .children((0..10).map(|i| {
                Text::new(format!("Row {i}"))
                    .height(Length::Px(5))
                    .key(format!("row-{i}"))
            }))
            .into()
    };

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(),
        Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        },
        None,
    );
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &make_root(),
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 10,
        },
        None,
    );

    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(
        scroll.offset, 0,
        "pinned-top should remain at 0 after resize"
    );
}

#[test]
fn bottom_pinning_survives_resize_when_virtual_cache_is_unseeded() {
    // `VIRTUAL_THRESHOLD` is 8: with `children.len() <= 8` the virtual height cache
    // is never populated, so `compute_scroll_anchor` must not require cache entries
    // for top/bottom pinning.
    fn make_row(i: usize, body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(format!("{body} [row {i}]"))
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .focusable(false)
                    .height(Length::Auto),
            )
            .key(format!("row-{i}"))
    }

    fn make_root(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .children((0..8).map(|i| make_row(i, body)))
            .into()
    }

    let body = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
    let mut tree = NodeTree::new();
    // Keep the viewport short so the list is scrollable even when wide; otherwise
    // `max_offset == 0` and the anchor is (correctly) pinned to the top.
    let wide = Rect {
        x: 0,
        y: 0,
        w: 72,
        h: 4,
    };
    let narrow = Rect {
        x: 0,
        y: 0,
        w: 28,
        h: 4,
    };

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(9999, body), wide, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    let stale = scroll.offset;
    assert_eq!(scroll.offset, scroll.max_offset);

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(stale, body), narrow, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(
        scroll.offset, scroll.max_offset,
        "bottom pin lost with unseeded virtual cache: offset={} max={}",
        scroll.offset, scroll.max_offset
    );
}

#[test]
fn same_width_taller_content_keeps_bottom_pin_without_viewport_resize() {
    fn make_root(body: &str, offset: usize) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .offset(offset)
            .child(
                Frame::new()
                    .border(false)
                    .height(Length::Auto)
                    .child(
                        DocumentView::new(body)
                            .border(false)
                            .scrollbar(false)
                            .h_scrollbar(false)
                            .wrap(true)
                            .focusable(false)
                            .height(Length::Auto),
                    )
                    .key("block"),
            )
            .into()
    }

    let shorter = "Line\nLine\nLine\nLine\nLine\nLine\nLine\nLine\n";
    let longer =
        "Line\nLine\nLine\nLine\nLine\nLine\nLine\nLine\nExtra\nExtra\nExtra\nExtra\nExtra\n";

    let mut tree = NodeTree::new();
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 36,
        h: 6,
    };

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(shorter, 9999), bounds, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert!(
        scroll.max_offset > 0,
        "precondition: content should scroll at this viewport"
    );
    let pinned = scroll.offset;
    assert_eq!(pinned, scroll.max_offset);

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(longer, pinned), bounds, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(
        scroll.offset, scroll.max_offset,
        "still pinned after in-place reflow: offset={} max={}",
        scroll.offset, scroll.max_offset
    );
}

/// Regression: pre-reconcile stack layout can underestimate a row whose
/// `DocumentView` only reaches final visual height after themed markdown
/// formatting; scrollbar math must use reconciled root height.
#[test]
#[cfg(feature = "markdown")]
fn scroll_content_height_matches_reconciled_markdown_document_row() {
    let body = concat!(
            "## Section\n\n",
            "This is a long paragraph that wraps across multiple visual lines when the viewport is narrow. ",
            "Repeat the tail so reflow is non-trivial. ",
            "Repeat the tail so reflow is non-trivial.\n\n",
        )
        .repeat(15);
    let root: Element = ScrollView::new()
        .scrollbar(true)
        .child(
            DocumentView::new(body)
                .markdown()
                .border(false)
                .scrollbar(false)
                .h_scrollbar(false)
                .wrap(true)
                .focusable(false)
                .height(Length::Auto)
                .key("doc-row"),
        )
        .into();

    let mut tree = NodeTree::new();
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 56,
        h: 18,
    };
    LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

    let NodeKind::ScrollView(sv) = &tree.node(tree.root).kind else {
        panic!("expected ScrollView");
    };
    let Some(&child_id) = tree.node(tree.root).children.first() else {
        panic!("expected scroll child");
    };
    let reconciled_h = tree.node(child_id).rect.h;
    assert_eq!(
        sv.content_height, reconciled_h,
        "scroll extent must match reconciled message row height for correct scrollbar mapping"
    );
}

#[test]
#[cfg(feature = "markdown")]
fn synced_height_updates_bottom_tracking_with_final_offset_range() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let body = concat!(
            "## Section\n\n",
            "This is a long paragraph that wraps across multiple visual lines when the viewport is narrow. ",
            "Repeat the tail so reflow is non-trivial. ",
            "Repeat the tail so reflow is non-trivial.\n\n",
        )
        .repeat(15);
    let seen = Rc::new(RefCell::new(None::<ScrollEvent>));
    let seen_cb = seen.clone();
    let cb = Callback::new(move |event: ScrollEvent| {
        *seen_cb.borrow_mut() = Some(event);
    });
    let root: Element = ScrollView::new()
        .scrollbar(true)
        .offset(usize::MAX)
        .on_scroll(cb)
        .child(
            DocumentView::new(body)
                .markdown()
                .border(false)
                .scrollbar(false)
                .h_scrollbar(false)
                .wrap(true)
                .focusable(false)
                .height(Length::Auto)
                .key("doc-row"),
        )
        .into();
    let root = root.key("scroll-root");

    let mut tree = NodeTree::new();
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 56,
        h: 18,
    };
    LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

    let NodeKind::ScrollView(sv) = &tree.node(tree.root).kind else {
        panic!("expected ScrollView");
    };
    let event = (*seen.borrow()).expect("expected on_scroll event");
    assert_eq!(event.metrics.len, sv.content_height as usize);
    assert_eq!(event.metrics.max_offset, sv.max_offset);
    assert_eq!(event.offset, sv.offset);
    assert_eq!(
        tree.scroll_was_at_bottom_by_key
            .get(&Key::from("scroll-root".to_string())),
        Some(&(sv.max_offset > 0 && sv.offset >= sv.max_offset)),
        "bottom tracking must use the final offset range"
    );
}

#[test]
#[cfg(feature = "markdown")]
fn expanding_visible_row_reflows_siblings_in_same_frame() {
    fn expanding_row(body: String) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(
                DocumentView::new(body)
                    .markdown()
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .focusable(false)
                    .height(Length::Auto),
            )
            .key("expanding-row")
    }

    fn root(body: String) -> Element {
        ScrollView::new()
            .scrollbar(false)
            .gap(1)
            .children([
                expanding_row(body),
                Text::new("tail row").height(Length::Px(1)).key("tail-row"),
            ])
            .into()
    }

    let mut tree = NodeTree::new();
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 44,
        h: 80,
    };
    LayoutEngine::reconcile_with_focus(&mut tree, &root("short".to_string()), bounds, None);

    let expanded = concat!(
        "## Section\n\n",
        "This row expands to multiple visual lines after full markdown formatting and wrapping. ",
        "Repeat the tail so layout drift is obvious. ",
        "Repeat the tail so layout drift is obvious.\n\n",
    )
    .repeat(10);
    LayoutEngine::reconcile_with_focus(&mut tree, &root(expanded), bounds, None);

    let expanding_id = find_by_key(&tree, "expanding-row").expect("expanding row");
    let tail_id = find_by_key(&tree, "tail-row").expect("tail row");
    let expanding_rect = tree.node(expanding_id).rect;
    let tail_rect = tree.node(tail_id).rect;
    assert_eq!(
        tail_rect.y,
        expanding_rect
            .y
            .saturating_add(expanding_rect.h as i16)
            .saturating_add(1),
        "same-frame reflow must move following visible rows below the expanded row",
    );

    let NodeKind::ScrollView(sv) = &tree.node(tree.root).kind else {
        panic!("expected ScrollView");
    };
    assert_eq!(
        sv.content_height,
        expanding_rect
            .h
            .saturating_add(1)
            .saturating_add(tail_rect.h),
        "content height must match the final visible row stack after expansion",
    );
}

#[test]
#[cfg(feature = "markdown")]
fn streaming_markdown_fenced_code_in_virtual_scroll_keeps_full_visual_content() {
    fn streaming_row(body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .padding((0, 0, 0, 3))
            .child(
                DocumentView::new(body)
                    .markdown()
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .focusable(false)
                    .height(Length::Auto),
            )
            .key("streaming-row")
    }

    fn root(body: &str, offset: usize) -> Element {
        let mut children: Vec<Element> = (0..12)
            .map(|i| {
                Text::new(format!("previous message {i}"))
                    .height(Length::Px(1))
                    .key(format!("row-{i}"))
            })
            .collect();
        children.push(streaming_row(body));

        ScrollView::new()
            .scrollbar(false)
            .gap(1)
            .offset(offset)
            .children(children)
            .into()
    }

    let initial = concat!(
        "Run this first if you have not already:\n\n",
        "```bash\n",
        "flatpak override --user --filesystem=/media/user/DataDrive com.usebottles.bottles\n",
        "```\n"
    );
    let streamed = concat!(
        "Run this first if you have not already:\n\n",
        "```bash\n",
        "flatpak override --user --filesystem=/media/user/DataDrive com.usebottles.bottles\n",
        "```\n\n",
        "For maximum compatibility, avoid symlinking Windows game folders unless needed. ",
        "Some installers/launchers dislike symlinks. Better is to move the game folder ",
        "and update the shortcut path in Bottles.\n\n",
        "**My Recommendation**\n",
        "For already installed games: no urgent need to move everything. Move only big games ",
        "or games with loading/stutter problems.\n\n",
        "For future FitGirl/repack installs: definitely install to the separate drive. ",
        "That gives the biggest benefit because installation is the workload that currently ",
        "freezes your system."
    );

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 72,
        h: 12,
    };
    let mut tree = NodeTree::new();

    LayoutEngine::reconcile_with_focus(&mut tree, &root(initial, usize::MAX), bounds, None);
    let stale_bottom_offset = {
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("expected ScrollView");
        };
        assert_eq!(
            scroll.offset, scroll.max_offset,
            "precondition: bottom pinned"
        );
        scroll.offset
    };

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root(streamed, stale_bottom_offset),
        bounds,
        None,
    );

    let row_id = find_by_key(&tree, "streaming-row").expect("streaming row should be visible");
    let doc_id = tree
        .node(row_id)
        .children
        .iter()
        .copied()
        .find(|id| matches!(tree.node(*id).kind, NodeKind::DocumentView(_)))
        .expect("streaming row should contain a DocumentView");
    let NodeKind::DocumentView(doc) = &tree.node(doc_id).kind else {
        panic!("expected DocumentView");
    };

    assert!(
        doc.visual_cache.flat_text.contains("flatpak override"),
        "fenced command should remain present during stream; rendered flat text was: {:?}",
        doc.visual_cache.flat_text,
    );
    assert!(
        doc.visual_cache
            .flat_text
            .contains("For maximum compatibility"),
        "post-code paragraph should be present during stream; rendered flat text was: {:?}",
        doc.visual_cache.flat_text,
    );
    assert_eq!(
        tree.node(doc_id).rect.h as usize,
        doc.total_visual_lines,
        "auto-height DocumentView rect should fit all visual markdown lines",
    );
}

#[test]
fn bottom_pinning_uses_scroll_indicator_max_offset_on_resize() {
    let row_h: u16 = 2;
    let row_count = 40usize;
    let make_root = |offset: usize| -> Element {
        ScrollView::new()
            .show_scroll_indicators(true)
            .scrollbar(false)
            .offset(offset)
            .children((0..row_count).map(|i| {
                Text::new(format!("line {i}"))
                    .height(Length::Px(row_h))
                    .key(format!("row-{i}"))
            }))
            .into()
    };

    let mut tree = NodeTree::new();
    let wide = Rect {
        x: 0,
        y: 0,
        w: 60,
        h: 12,
    };
    let narrow = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    };

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(9999), wide, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    let stale = scroll.offset;
    assert_eq!(scroll.offset, scroll.max_offset);

    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(stale), narrow, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    assert_eq!(
        scroll.offset, scroll.max_offset,
        "bottom pin with scroll indicators should match calc_scroll_view_window max_offset"
    );
}

#[test]
fn bottom_pinning_holds_across_alternating_width_changes_with_stale_offset() {
    fn make_row(i: usize, body: &str) -> Element {
        Frame::new()
            .border(false)
            .height(Length::Auto)
            .padding((1, 0, 1, 3))
            .child(
                DocumentView::new(format!("{body} [msg {i}]"))
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .focusable(false)
                    .height(Length::Auto),
            )
            .key(format!("msg-{i}"))
    }

    fn make_root(offset: usize, body: &str) -> Element {
        ScrollView::new()
            .scrollbar(true)
            .scrollbar_config(
                ScrollbarConfig::new()
                    .variant(ScrollbarVariant::Standalone)
                    .gap(1),
            )
            .padding(1)
            .gap(1)
            .offset(offset)
            .children((0..90).map(|i| make_row(i, body)))
            .into()
    }

    let body = "This is a longer message that should wrap when viewport is narrow enough to cause reflow of the wrapped text content inside the document view.";
    let wide = Rect {
        x: 0,
        y: 0,
        w: 120,
        h: 40,
    };
    let narrow = Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 40,
    };

    let mut tree = NodeTree::new();

    // Scroll to bottom.
    LayoutEngine::reconcile_with_focus(&mut tree, &make_root(9999, body), wide, None);
    let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
        panic!("expected scroll view");
    };
    let initial_max = scroll.max_offset;
    assert_eq!(scroll.offset, initial_max, "should start at bottom");

    // Alternating resizes with a stale offset (component never learned
    // the corrected offset because on_scroll doesn't fire on reconcile).
    let stale_offset = initial_max;
    for i in 0..8 {
        let bounds = if i % 2 == 0 { narrow } else { wide };
        LayoutEngine::reconcile_with_focus(&mut tree, &make_root(stale_offset, body), bounds, None);
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("expected scroll view");
        };
        assert_eq!(
            scroll.offset, scroll.max_offset,
            "bottom pinning lost on iteration {i}: offset={}, max={}",
            scroll.offset, scroll.max_offset
        );
    }
}

#[cfg(feature = "diff-view")]
#[test]
fn auto_height_diff_view_in_scroll_view_remeasures_on_resize() {
    let diff: Element = DiffView::with_content("abcdefghijklmnopqrst", "abcdefghijklmnopqrst")
        .mode(DiffViewMode::Split)
        .backend(DiffViewBackend::DocumentView)
        .wrap(true)
        .height(Length::Auto)
        .border(false)
        .panels_border(false)
        .scrollbar(false)
        .h_scrollbar(false)
        .document_view(DocumentView::new("").height(Length::Auto).border(false))
        .key("diff");
    let root: Element = ScrollView::new().child(diff.clone()).into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 60,
            h: 20,
        },
        None,
    );

    let wide_h = find_by_key(&tree, "diff")
        .map(|id| tree.node(id).rect.h)
        .expect("diff should exist");

    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 24,
            h: 20,
        },
        None,
    );

    let narrow_h = find_by_key(&tree, "diff")
        .map(|id| tree.node(id).rect.h)
        .expect("diff should exist after resize");

    let narrow_id = find_by_key(&tree, "diff").expect("diff should exist after resize");
    let narrow_rect = tree.node(narrow_id).rect;
    let expected = min_size_constrained(&diff, Some(narrow_rect.w), None);

    assert!(
        narrow_h > wide_h,
        "auto-height diff view inside scroll view should grow after narrowing: wide={wide_h}, narrow={narrow_h}"
    );
    assert_eq!(narrow_rect.h, expected.1);
}

#[cfg(feature = "diff-view")]
#[test]
fn split_document_diff_in_scroll_view_allocates_full_leaf_height() {
    fn collect_document_views(tree: &NodeTree, node_id: NodeId, out: &mut Vec<NodeId>) {
        let node = tree.node(node_id);
        if matches!(node.kind, NodeKind::DocumentView(_)) {
            out.push(node_id);
            return;
        }
        for child_id in &node.children {
            collect_document_views(tree, *child_id, out);
        }
    }

    let before = r#"fn greet(name: &str) -> String {
    format!("Hello, {}", name)
}

fn goodbye(name: &str) -> String {
    format!("Goodbye, {}", name)
}

fn main() {
    let msg = greet("World");
    println!("{}", msg);

    let bye = goodbye("World");
    println!("{}", bye);

    println!("End of program.");
}
"#;
    let after = r#"fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

fn main() {
    let user = "Lipan";
    let msg = greet(user);
    println!("{msg}");

    let items = vec!["Rust", "TUI", "Lipan"];
    for item in items {
        println!("Loading {item}...");
    }

    println!("Application ready.");
}
"#;

    let root: Element = ScrollView::new()
        .child(
            DiffView::with_content(before, after)
                .mode(DiffViewMode::Split)
                .backend(DiffViewBackend::DocumentView)
                .height(Length::Auto)
                .wrap(true)
                .scrollbar(false)
                .h_scrollbar(false)
                .key("diff"),
        )
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 84,
            h: 40,
        },
        None,
    );

    let diff_id = find_by_key(&tree, "diff").expect("diff should exist");
    let mut docs = Vec::new();
    collect_document_views(&tree, diff_id, &mut docs);

    assert_eq!(
        docs.len(),
        2,
        "expected split diff to render two document views"
    );
    for doc_id in docs {
        let node = tree.node(doc_id);
        let NodeKind::DocumentView(doc) = &node.kind else {
            panic!("expected document view node");
        };
        assert_eq!(node.rect.h, doc.total_visual_lines as u16);
    }
}

#[cfg(all(feature = "diff-view", feature = "markdown"))]
#[test]
fn patch_stack_diff_tool_panel_height_matches_title_padding_and_diff_body() {
    fn collect_document_views(tree: &NodeTree, node_id: NodeId, out: &mut Vec<NodeId>) {
        let node = tree.node(node_id);
        if matches!(node.kind, NodeKind::DocumentView(_)) {
            out.push(node_id);
            return;
        }
        for child_id in &node.children {
            collect_document_views(tree, *child_id, out);
        }
    }

    let before = "DiffView::with_content(before, after)\n    .mode(DiffViewMode::Unified)\n    .wrap(true)\n    .scrollbar(false)\n    .focusable(false)\n";
    let after = "DiffView::with_content(before, after)\n    .backend(DiffViewBackend::DocumentView)\n    .mode(DiffViewMode::Split)\n    .wrap(false)\n    .h_scrollbar(true)\n    .scrollbar(false)\n    .focusable(false)\n";

    for width in 70..130u16 {
        let diff: Element = DiffView::with_content(before, after)
            .backend(DiffViewBackend::DocumentView)
            .mode(DiffViewMode::Split)
            .height(Length::Auto)
            .border(false)
            .panels_border(false)
            .highlight_full_width(true)
            .line_numbers(true)
            .gutter_inset(1)
            .wrap(false)
            .h_scrollbar(true)
            .scrollbar(false)
            .focusable(false)
            .key("diff");

        let panel: Element = Frame::new()
            .border(false)
            .height(Length::Auto)
            .padding((1, 1, 1, 3))
            .child(
                VStack::new()
                    .height(Length::Auto)
                    .gap(1)
                    .child(Text::new("Update src/widgets/message_view.rs"))
                    .child(diff)
                    .key("panel-body"),
            )
            .key("panel");

        let root: Element = ScrollView::new().child(panel).into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: width,
                h: 40,
            },
            None,
        );

        let diff_id = find_by_key(&tree, "diff").expect("diff should exist");
        let panel_id = find_by_key(&tree, "panel").expect("panel should exist");
        let mut docs = Vec::new();
        collect_document_views(&tree, diff_id, &mut docs);
        let max_doc_h = docs
            .iter()
            .map(|id| tree.node(*id).rect.h)
            .max()
            .expect("expected document views");

        assert_eq!(
            tree.node(diff_id).rect.h,
            max_doc_h,
            "diff height should match tallest pane at width {width}",
        );
        assert_eq!(
            tree.node(panel_id).rect.h,
            tree.node(diff_id).rect.h + 4,
            "panel height should be title+gap+top/bottom padding plus diff height at width {width}",
        );
    }
}

#[cfg(all(feature = "diff-view", feature = "markdown"))]
#[test]
fn patch_stack_diff_tool_panel_remeasures_cleanly_after_width_changes() {
    fn collect_document_views(tree: &NodeTree, node_id: NodeId, out: &mut Vec<NodeId>) {
        let node = tree.node(node_id);
        if matches!(node.kind, NodeKind::DocumentView(_)) {
            out.push(node_id);
            return;
        }
        for child_id in &node.children {
            collect_document_views(tree, *child_id, out);
        }
    }

    let before = "DiffView::with_content(before, after)\n    .mode(DiffViewMode::Unified)\n    .wrap(true)\n    .scrollbar(false)\n    .focusable(false)\n";
    let after = "DiffView::with_content(before, after)\n    .backend(DiffViewBackend::DocumentView)\n    .mode(DiffViewMode::Split)\n    .wrap(false)\n    .h_scrollbar(true)\n    .scrollbar(false)\n    .focusable(false)\n";

    let build_root = || {
        let diff: Element = DiffView::with_content(before, after)
            .backend(DiffViewBackend::DocumentView)
            .mode(DiffViewMode::Split)
            .height(Length::Auto)
            .border(false)
            .panels_border(false)
            .highlight_full_width(true)
            .line_numbers(true)
            .gutter_inset(1)
            .wrap(false)
            .h_scrollbar(true)
            .scrollbar(false)
            .focusable(false)
            .key("diff");

        let panel: Element = Frame::new()
            .border(false)
            .height(Length::Auto)
            .padding((1, 1, 1, 3))
            .child(
                VStack::new()
                    .height(Length::Auto)
                    .gap(1)
                    .child(Text::new("Update src/widgets/message_view.rs"))
                    .child(diff)
                    .key("panel-body"),
            )
            .key("panel");

        Element::from(ScrollView::new().child(panel))
    };

    let mut tree = NodeTree::new();
    for width in [70u16, 82, 94, 106, 118, 106, 94, 82, 70, 118] {
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &build_root(),
            Rect {
                x: 0,
                y: 0,
                w: width,
                h: 40,
            },
            None,
        );

        let diff_id = find_by_key(&tree, "diff").expect("diff should exist");
        let panel_id = find_by_key(&tree, "panel").expect("panel should exist");
        let mut docs = Vec::new();
        collect_document_views(&tree, diff_id, &mut docs);
        let max_doc_h = docs
            .iter()
            .map(|id| tree.node(*id).rect.h)
            .max()
            .expect("expected document views");

        assert_eq!(
            tree.node(diff_id).rect.h,
            max_doc_h,
            "diff height should match tallest pane after resize at width {width}",
        );
        assert_eq!(
            tree.node(panel_id).rect.h,
            tree.node(diff_id).rect.h + 4,
            "panel height should remeasure cleanly after resize at width {width}",
        );
    }
}

#[cfg(all(feature = "diff-view", feature = "markdown"))]
#[test]
fn virtualized_patch_stack_diff_panels_remeasure_cleanly_after_width_changes() {
    fn collect_document_views(tree: &NodeTree, node_id: NodeId, out: &mut Vec<NodeId>) {
        let node = tree.node(node_id);
        if matches!(node.kind, NodeKind::DocumentView(_)) {
            out.push(node_id);
            return;
        }
        for child_id in &node.children {
            collect_document_views(tree, *child_id, out);
        }
    }

    let before = "DiffView::with_content(before, after)\n    .mode(DiffViewMode::Unified)\n    .wrap(true)\n    .scrollbar(false)\n    .focusable(false)\n";
    let after = "DiffView::with_content(before, after)\n    .backend(DiffViewBackend::DocumentView)\n    .mode(DiffViewMode::Split)\n    .wrap(false)\n    .h_scrollbar(true)\n    .scrollbar(false)\n    .focusable(false)\n";

    let build_root = || {
        let mut scroll = ScrollView::new();
        for idx in 0..14 {
            let diff: Element = DiffView::with_content(before, after)
                .backend(DiffViewBackend::DocumentView)
                .mode(DiffViewMode::Split)
                .height(Length::Auto)
                .border(false)
                .panels_border(false)
                .highlight_full_width(true)
                .line_numbers(true)
                .gutter_inset(1)
                .wrap(false)
                .h_scrollbar(true)
                .scrollbar(false)
                .focusable(false)
                .key(format!("diff-{idx}"));

            let panel: Element = Frame::new()
                .border(false)
                .height(Length::Auto)
                .padding((1, 1, 1, 3))
                .child(
                    VStack::new()
                        .height(Length::Auto)
                        .gap(1)
                        .child(Text::new("Update src/widgets/message_view.rs"))
                        .child(diff),
                )
                .key(format!("panel-{idx}"));

            scroll = scroll.child(panel);
        }
        Element::from(scroll)
    };

    let mut tree = NodeTree::new();
    for width in [72u16, 88, 104, 120, 104, 88, 72, 120] {
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &build_root(),
            Rect {
                x: 0,
                y: 0,
                w: width,
                h: 40,
            },
            None,
        );

        for idx in 0..3 {
            let diff_id = find_by_key(&tree, &format!("diff-{idx}")).expect("diff should exist");
            let panel_id = find_by_key(&tree, &format!("panel-{idx}")).expect("panel should exist");
            let mut docs = Vec::new();
            collect_document_views(&tree, diff_id, &mut docs);
            let max_doc_h = docs
                .iter()
                .map(|id| tree.node(*id).rect.h)
                .max()
                .expect("expected document views");

            assert_eq!(
                tree.node(diff_id).rect.h,
                max_doc_h,
                "virtualized diff height should match tallest pane after resize at width {width}, idx {idx}",
            );
            assert_eq!(
                tree.node(panel_id).rect.h,
                tree.node(diff_id).rect.h + 4,
                "virtualized panel height should remeasure cleanly after resize at width {width}, idx {idx}",
            );
        }
    }
}

#[cfg(all(feature = "diff-view", feature = "markdown"))]
#[test]
fn repro_timeline_patch_stack_diff_panel_matches_diff_body_after_resize() {
    fn collect_document_views(tree: &NodeTree, node_id: NodeId, out: &mut Vec<NodeId>) {
        let node = tree.node(node_id);
        if matches!(node.kind, NodeKind::DocumentView(_)) {
            out.push(node_id);
            return;
        }
        for child_id in &node.children {
            collect_document_views(tree, *child_id, out);
        }
    }

    let before = "DiffView::with_content(before, after)\n    .mode(DiffViewMode::Unified)\n    .wrap(true)\n    .scrollbar(false)\n    .focusable(false)\n";
    let after = "DiffView::with_content(before, after)\n    .backend(DiffViewBackend::DocumentView)\n    .mode(DiffViewMode::Split)\n    .wrap(false)\n    .h_scrollbar(true)\n    .scrollbar(false)\n    .focusable(false)\n";

    let build_panel = |idx: usize| {
        let diff: Element = DiffView::with_content(before, after)
            .backend(DiffViewBackend::DocumentView)
            .mode(DiffViewMode::Split)
            .height(Length::Auto)
            .border(false)
            .panels_border(false)
            .highlight_full_width(true)
            .line_numbers(true)
            .gutter_inset(1)
            .wrap(true)
            .h_scrollbar(false)
            .scrollbar(false)
            .focusable(false)
            .key(format!("diff-{idx}"));

        Frame::new()
            .border(false)
            .height(Length::Auto)
            .padding((1, 1, 1, 3))
            .child(
                VStack::new()
                    .height(Length::Auto)
                    .gap(1)
                    .child(Text::new("Update src/widgets/message_view.rs"))
                    .child(diff),
            )
            .key(format!("panel-{idx}"))
    };

    let build_root = || {
        let mut scroll = ScrollView::new()
            .border(false)
            .scrollbar(true)
            .scrollbar_config(ScrollbarConfig::new().gap(1))
            .padding(1)
            .gap(1);
        for idx in 0..14 {
            scroll = scroll.child(
                VStack::new()
                    .height(Length::Auto)
                    .gap(1)
                    .child(build_panel(idx))
                    .child(Text::new("metadata row"))
                    .key(format!("message-{idx}")),
            );
        }

        Element::from(Frame::new().title("Timeline").border(true).child(scroll))
    };

    let mut tree = NodeTree::new();
    for width in [84u16, 96, 108, 120, 108, 96, 84] {
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &build_root(),
            Rect {
                x: 0,
                y: 0,
                w: width,
                h: 40,
            },
            None,
        );

        for idx in 0..2 {
            let diff_id = find_by_key(&tree, &format!("diff-{idx}")).expect("diff should exist");
            let panel_id = find_by_key(&tree, &format!("panel-{idx}")).expect("panel should exist");
            let mut docs = Vec::new();
            collect_document_views(&tree, diff_id, &mut docs);
            let max_doc_h = docs
                .iter()
                .map(|id| tree.node(*id).rect.h)
                .max()
                .expect("expected document views");

            assert_eq!(
                tree.node(diff_id).rect.h,
                max_doc_h,
                "timeline diff height should match tallest pane after resize at width {width}, idx {idx}",
            );
            assert_eq!(
                tree.node(panel_id).rect.h,
                tree.node(diff_id).rect.h + 4,
                "timeline panel height should be title+gap+padding plus diff height at width {width}, idx {idx}",
            );
        }
    }
}

#[cfg(all(feature = "diff-view", feature = "markdown"))]
#[test]
fn repro_timeline_split_wrapped_diff_keeps_both_panes_same_height_on_resize() {
    fn collect_document_views(tree: &NodeTree, node_id: NodeId, out: &mut Vec<NodeId>) {
        let node = tree.node(node_id);
        if matches!(node.kind, NodeKind::DocumentView(_)) {
            out.push(node_id);
            return;
        }
        for child_id in &node.children {
            collect_document_views(tree, *child_id, out);
        }
    }

    let before = "fn render_assistant_part(part: &Part) -> Option<Element> {\n    match part {\n        Part::Text(text) => render_markdown(text.text.trim()),\n        Part::Tool(tool) => render_tool(tool),\n        _ => None,\n    }\n}\n";
    let after = "fn render_assistant_part(part: &Part) -> Option<Element> {\n    match part {\n        Part::Text(text) => render_markdown(text.text.trim()),\n        Part::Reasoning(reasoning) => render_reasoning(reasoning),\n        Part::Tool(tool) => render_tool(tool),\n        Part::Subtask(task) => render_subtask(task),\n        Part::Agent(agent) => render_agent(agent),\n        Part::Retry(retry) => render_retry(retry),\n        _ => None,\n    }\n}\n";

    let build_root = || {
        let diff: Element = DiffView::with_content(before, after)
            .backend(DiffViewBackend::DocumentView)
            .mode(DiffViewMode::Split)
            .height(Length::Auto)
            .border(false)
            .panels_border(false)
            .highlight_full_width(true)
            .line_numbers(true)
            .gutter_inset(1)
            .wrap(true)
            .h_scrollbar(false)
            .scrollbar(false)
            .focusable(false)
            .key("diff");

        let panel: Element = Frame::new()
            .border(false)
            .height(Length::Auto)
            .padding((1, 1, 1, 3))
            .child(
                VStack::new()
                    .height(Length::Auto)
                    .gap(1)
                    .child(Text::new("Update src/screens/session.rs"))
                    .child(diff)
                    .key("panel-body"),
            )
            .key("panel");

        Element::from(
            Frame::new().title("Timeline").border(true).child(
                ScrollView::new()
                    .border(false)
                    .scrollbar(true)
                    .scrollbar_config(ScrollbarConfig::new().gap(1))
                    .padding(1)
                    .gap(1)
                    .child(panel),
            ),
        )
    };

    let root = build_root();
    let mut tree = NodeTree::new();
    for width in (200u16..=240).rev() {
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: width,
                h: 40,
            },
            None,
        );

        let diff_id = find_by_key(&tree, "diff").expect("diff should exist");
        let mut docs = Vec::new();
        collect_document_views(&tree, diff_id, &mut docs);
        assert_eq!(docs.len(), 2);

        let left = tree.node(docs[0]);
        let right = tree.node(docs[1]);
        let NodeKind::DocumentView(left_doc) = &left.kind else {
            panic!("expected left document view");
        };
        let NodeKind::DocumentView(right_doc) = &right.kind else {
            panic!("expected right document view");
        };

        assert_eq!(
            left.rect.h as usize, left_doc.total_visual_lines,
            "left pane height should match visual lines at width {width}",
        );
        assert_eq!(
            right.rect.h as usize, right_doc.total_visual_lines,
            "right pane height should match visual lines at width {width}",
        );
        assert_eq!(
            left.rect.h, right.rect.h,
            "split panes should have identical visible heights at width {width}",
        );
    }
}

#[test]
fn offset_max_jump_after_append_measures_estimated_tail_on_followup_reconcile() {
    fn children(appended: bool) -> Vec<Element> {
        let mut rows: Vec<Element> = keyed_rows(12, 5).collect();
        if appended {
            rows.push(
                Text::new("long user message")
                    .height(Length::Px(30))
                    .key("long"),
            );
            rows.push(
                Text::new("assistant header")
                    .height(Length::Px(1))
                    .key("tail"),
            );
        }
        rows
    }
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 10,
    };
    let mut tree = NodeTree::new();

    // Frame 1: seed the virtual cache with everything measured, pinned to bottom.
    let seed: Element = ScrollView::new()
        .offset(usize::MAX)
        .children(children(false))
        .into();
    LayoutEngine::reconcile_with_focus(&mut tree, &seed, bounds, None);
    {
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("expected root scroll view");
        };
        assert_eq!(scroll.content_height, 60);
        assert_eq!(scroll.offset, scroll.max_offset);
    }

    // Frame 2: a tall message + short tail are appended while a pending
    // scroll-to-bottom keeps the controlled offset at usize::MAX. The additive
    // near-visible remeasure should resolve the tail in this same reconcile.
    let jump: Element = ScrollView::new()
        .offset(usize::MAX)
        .children(children(true))
        .into();
    LayoutEngine::reconcile_with_focus(&mut tree, &jump, bounds, None);
    let frame2_offset = {
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("expected root scroll view");
        };
        let tail = find_by_key(&tree, "tail").expect("tail should exist");
        assert_eq!(tree.node(tail).rect.h, 1);
        assert_eq!(scroll.content_height, 91);
        assert_eq!(scroll.offset, scroll.max_offset);
        scroll.offset
    };

    // Frame 3: the app mirrors the emitted offset back (controlled offset
    // catch-up) with unchanged content. The exact tail measurement should stay
    // stable.
    let caught_up: Element = ScrollView::new()
        .offset(frame2_offset)
        .children(children(true))
        .into();
    LayoutEngine::reconcile_with_focus(&mut tree, &caught_up, bounds, None);
    {
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("expected root scroll view");
        };
        let tail = find_by_key(&tree, "tail").expect("tail should exist");
        assert_eq!(
            tree.node(tail).rect.h,
            1,
            "tail should remain measured after the offset catch-up reconcile"
        );
        assert_eq!(scroll.content_height, 91);
        assert_eq!(
            scroll.offset, scroll.max_offset,
            "bottom pin should survive the corrected content height"
        );
    }
}

#[test]
fn standalone_scrollbar_exact_hit_seeds_real_virtual_cache_on_append() {
    fn root(count: usize) -> Element {
        let children: Vec<Element> = (0..count)
            .map(|i| {
                let h = if i + 1 == count { 1 } else { 7 };
                Text::new(format!(
                    "row {i} wraps tall enough to require a standalone scrollbar"
                ))
                .height(Length::Px(h))
                .key(format!("row-{i}"))
            })
            .collect();

        ScrollView::new()
            .border(false)
            .scrollbar(true)
            .offset(usize::MAX)
            .children(children)
            .into()
    }

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 8,
    };
    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(&mut tree, &root(10), bounds, None);

    for count in 11..15 {
        LayoutEngine::reconcile_with_focus(&mut tree, &root(count), bounds, None);
        let key = format!("row-{}", count - 1);
        let tail = find_by_key(&tree, &key).expect("appended tail should be visible at bottom");
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("expected root scroll view");
        };
        assert_eq!(scroll.virtual_cache.entries.len(), count);
        assert!(
            scroll
                .virtual_cache
                .entries
                .iter()
                .all(|entry| entry.as_ref().is_some_and(|entry| !entry.stale)),
            "virtual cache should be fully seeded after append {count}"
        );
        assert_eq!(
            tree.node(tail).rect.h,
            1,
            "{key} should be exact immediately"
        );
        assert_eq!(scroll.offset, scroll.max_offset);
    }
}

fn virtual_entry(h: u16, gap_after: bool, stale: bool) -> VirtualChildEntry {
    VirtualChildEntry {
        layout_hash: 1,
        key: None,
        h,
        w: 1,
        x: 0,
        flex_w: false,
        percent_w: None,
        gap_after,
        is_portal: false,
        stale,
    }
}

#[test]
fn virtual_cache_has_unresolved_in_zone_detects_none_and_stale() {
    let cache = VirtualHeightCache {
        entries: vec![
            Some(virtual_entry(10, false, false)),
            None,
            Some(virtual_entry(10, false, false)),
        ],
        ..Default::default()
    };
    assert!(cache.has_unresolved_in_zone(10, 5, 0, 10, 3));

    let cache = VirtualHeightCache {
        entries: vec![
            None,
            Some(virtual_entry(10, false, false)),
            Some(virtual_entry(10, false, false)),
        ],
        ..Default::default()
    };
    assert!(!cache.has_unresolved_in_zone(25, 3, 0, 10, 3));

    let cache = VirtualHeightCache {
        entries: vec![
            Some(virtual_entry(10, false, false)),
            Some(virtual_entry(10, false, true)),
            Some(virtual_entry(10, false, false)),
        ],
        ..Default::default()
    };
    assert!(cache.has_unresolved_in_zone(10, 5, 0, 10, 3));
    assert!(cache.has_unresolved_in_zone(10, 5, 0, 10, 4));
}
