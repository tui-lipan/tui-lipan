use crate::core::node::{NodeId, NodeKind};
use crate::layout::reconcile::{
    ElementReconcile, ReconcileCtx, reconcile_element, resolve_rect_with_auto,
};
use crate::style::{LayoutConstraints, Rect};
use crate::widgets::containers::reconcile::stack_reuse_plan;

use super::layout::measure_canvas;

fn translate_rect(origin: Rect, local: Rect) -> Rect {
    Rect {
        x: origin.x.saturating_add(local.x),
        y: origin.y.saturating_add(local.y),
        w: local.w,
        h: local.h,
    }
}

pub(crate) struct CanvasReconcile<'a> {
    pub id: NodeId,
    pub canvas: &'a super::Canvas,
    pub rect: Rect,
    pub constraints: &'a LayoutConstraints,
}

pub(crate) fn reconcile_canvas(ctx: &mut ReconcileCtx<'_>, args: CanvasReconcile<'_>) -> NodeId {
    let CanvasReconcile {
        id,
        canvas,
        rect,
        constraints,
    } = args;
    let measured = measure_canvas(canvas, Some(rect.w), Some(rect.h));
    let rect = resolve_rect_with_auto(
        rect,
        constraints,
        canvas.width,
        canvas.height,
        measured.0,
        measured.1,
    );

    let old_children = {
        let node = ctx.tree.node_mut(id);
        node.rect = rect;
        node.kind = NodeKind::Canvas(super::CanvasNode {
            style: canvas.style,
            passthrough: canvas.passthrough,
        });
        std::mem::take(&mut node.children)
    };

    let child_refs: Vec<&crate::core::element::Element> =
        canvas.items.iter().map(|item| &item.element).collect();
    let plan = stack_reuse_plan(ctx.tree, &old_children, &child_refs);

    let mut new_children = old_children;
    new_children.clear();

    for (item, reuse_id) in canvas.items.iter().zip(plan) {
        let child_id = reconcile_element(
            ctx,
            ElementReconcile {
                reuse: reuse_id,
                parent: Some(id),
                el: &item.element,
                rect: translate_rect(rect, item.rect),
            },
        );
        new_children.push(child_id);
    }

    let node = ctx.tree.node_mut(id);
    node.children = new_children;

    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestBackend;
    use crate::core::component::{Component, Context, Update};
    use crate::core::element::{Element, IntoElement, Key};
    use crate::core::node::{NodeId, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::style::{Color, Length, Style};
    use crate::widgets::{Button, Canvas, Frame, ScrollView, Text, VStack};

    fn find_by_key(tree: &NodeTree, key: &str) -> Option<NodeId> {
        let key = Key::from(key.to_string());
        tree.iter()
            .find(|node| node.key.as_ref() == Some(&key))
            .map(|node| node.id)
    }

    #[test]
    fn child_rects_are_translated_from_canvas_local_coordinates() {
        let canvas: Element = Canvas::new()
            .child_at(
                Rect {
                    x: 2,
                    y: 3,
                    w: 7,
                    h: 4,
                },
                Frame::new().border(false).key("child"),
            )
            .into();
        let mut tree = NodeTree::new();

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &canvas,
            Rect {
                x: 10,
                y: 5,
                w: 20,
                h: 10,
            },
            None,
        );

        let child = tree.node(find_by_key(&tree, "child").expect("child should reconcile"));
        assert_eq!(
            child.rect,
            Rect {
                x: 12,
                y: 8,
                w: 7,
                h: 4,
            }
        );
    }

    #[test]
    fn passthrough_false_blocks_lower_canvas_layers() {
        let canvas: Element = Canvas::new()
            .child_at(
                Rect {
                    x: 0,
                    y: 0,
                    w: 10,
                    h: 3,
                },
                Button::new("under")
                    .width(Length::Px(10))
                    .height(Length::Px(3)),
            )
            .child_at(
                Rect {
                    x: 0,
                    y: 0,
                    w: 10,
                    h: 3,
                },
                Text::new("cover")
                    .width(Length::Px(10))
                    .height(Length::Px(3)),
            )
            .into();
        let mut tree = NodeTree::new();

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &canvas,
            Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 3,
            },
            None,
        );

        assert_eq!(tree.hit_test(1, 1), None);
    }

    #[test]
    fn passthrough_true_allows_lower_canvas_layers() {
        let canvas: Element = Canvas::new()
            .passthrough(true)
            .child_at(
                Rect {
                    x: 0,
                    y: 0,
                    w: 10,
                    h: 3,
                },
                Button::new("under")
                    .width(Length::Px(10))
                    .height(Length::Px(3))
                    .key("button"),
            )
            .child_at(
                Rect {
                    x: 0,
                    y: 0,
                    w: 10,
                    h: 3,
                },
                Text::new("cover")
                    .width(Length::Px(10))
                    .height(Length::Px(3)),
            )
            .into();
        let mut tree = NodeTree::new();

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &canvas,
            Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 3,
            },
            None,
        );

        assert_eq!(tree.hit_test(1, 1), find_by_key(&tree, "button"));
    }

    fn overflow_scroll_view() -> ScrollView {
        (0..8).fold(
            ScrollView::new()
                .width(Length::Px(6))
                .height(Length::Px(4))
                .scrollbar(true),
            |scroll, row| scroll.child(Text::new(format!("row {row}"))),
        )
    }

    #[test]
    fn scrollbar_zones_outside_canvas_clip_are_ignored() {
        let canvas: Element = Canvas::new()
            .width(Length::Px(4))
            .height(Length::Px(4))
            .child_at(
                Rect {
                    x: 2,
                    y: 0,
                    w: 6,
                    h: 4,
                },
                overflow_scroll_view(),
            )
            .into();
        let mut tree = NodeTree::new();

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &canvas,
            Rect {
                x: 0,
                y: 0,
                w: 4,
                h: 4,
            },
            None,
        );

        assert!(tree.scrollbar_target_at(7, 1).is_none());
    }

    #[test]
    fn visible_canvas_scrollbar_zones_are_targetable() {
        let canvas: Element = Canvas::new()
            .child_at(
                Rect {
                    x: 0,
                    y: 0,
                    w: 6,
                    h: 4,
                },
                overflow_scroll_view(),
            )
            .into();
        let mut tree = NodeTree::new();

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &canvas,
            Rect {
                x: 0,
                y: 0,
                w: 6,
                h: 4,
            },
            None,
        );

        assert!(tree.scrollbar_target_at(5, 1).is_some());
    }

    #[test]
    fn covered_canvas_scrollbar_zones_are_blocked() {
        let canvas: Element = Canvas::new()
            .child_at(
                Rect {
                    x: 0,
                    y: 0,
                    w: 6,
                    h: 4,
                },
                overflow_scroll_view(),
            )
            .child_at(
                Rect {
                    x: 5,
                    y: 0,
                    w: 1,
                    h: 4,
                },
                Text::new("cover")
                    .width(Length::Px(1))
                    .height(Length::Px(4)),
            )
            .into();
        let mut tree = NodeTree::new();

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &canvas,
            Rect {
                x: 0,
                y: 0,
                w: 6,
                h: 4,
            },
            None,
        );

        assert!(tree.scrollbar_target_at(5, 1).is_none());
    }

    #[test]
    fn passthrough_canvas_allows_lower_scrollbar_zones() {
        let canvas: Element = Canvas::new()
            .passthrough(true)
            .child_at(
                Rect {
                    x: 0,
                    y: 0,
                    w: 6,
                    h: 4,
                },
                overflow_scroll_view(),
            )
            .child_at(
                Rect {
                    x: 5,
                    y: 0,
                    w: 1,
                    h: 4,
                },
                Text::new("cover")
                    .width(Length::Px(1))
                    .height(Length::Px(4)),
            )
            .into();
        let mut tree = NodeTree::new();

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &canvas,
            Rect {
                x: 0,
                y: 0,
                w: 6,
                h: 4,
            },
            None,
        );

        assert!(tree.scrollbar_target_at(5, 1).is_some());
    }

    #[test]
    fn keyed_reorder_preserves_canvas_child_identity() {
        fn keyed_canvas(order: &[&str]) -> Element {
            order
                .iter()
                .enumerate()
                .fold(Canvas::new(), |canvas, (idx, key)| {
                    canvas.child_at(
                        Rect {
                            x: idx as i16,
                            y: 0,
                            w: 1,
                            h: 1,
                        },
                        Text::new((*key).to_string()).key((*key).to_string()),
                    )
                })
                .into()
        }

        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 4,
        };

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &keyed_canvas(&["a", "b", "c"]),
            bounds,
            None,
        );
        let a_before = find_by_key(&tree, "a").expect("missing key a");
        let b_before = find_by_key(&tree, "b").expect("missing key b");
        let c_before = find_by_key(&tree, "c").expect("missing key c");

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &keyed_canvas(&["c", "a", "b"]),
            bounds,
            None,
        );
        assert_eq!(find_by_key(&tree, "a"), Some(a_before));
        assert_eq!(find_by_key(&tree, "b"), Some(b_before));
        assert_eq!(find_by_key(&tree, "c"), Some(c_before));
    }

    #[derive(Clone)]
    struct CanvasLeaf;

    impl Component for CanvasLeaf {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("nested").key("nested-text")
        }
    }

    #[derive(Clone)]
    struct CanvasNestedRoot;

    impl Component for CanvasNestedRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Canvas::new()
                .child_at(
                    Rect {
                        x: 3,
                        y: 2,
                        w: 8,
                        h: 1,
                    },
                    crate::child(|| CanvasLeaf, ()),
                )
                .into()
        }
    }

    #[test]
    fn nested_component_children_expand_inside_canvas_items() {
        let mut backend = TestBackend::new(CanvasNestedRoot);
        backend.set_viewport(Rect {
            x: 10,
            y: 5,
            w: 20,
            h: 6,
        });
        backend.render();

        let tree = &backend.core.tree;
        let text_id = find_by_key(tree, "nested-text").expect("nested component text should exist");
        assert_eq!(tree.node(text_id).rect.x, 13);
        assert_eq!(tree.node(text_id).rect.y, 7);
    }

    #[derive(Clone)]
    struct CanvasClipRoot;

    impl Component for CanvasClipRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            VStack::new()
                .child(
                    Canvas::new()
                        .width(Length::Px(4))
                        .height(Length::Px(2))
                        .style(Style::new().bg(Color::rgb(10, 10, 10)))
                        .child_at(
                            Rect {
                                x: 2,
                                y: 0,
                                w: 4,
                                h: 1,
                            },
                            Text::new("ABCD").width(Length::Px(4)).height(Length::Px(1)),
                        ),
                )
                .into()
        }
    }

    #[test]
    fn render_clips_children_to_canvas_rect() {
        let mut backend = TestBackend::new(CanvasClipRoot);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 6,
            h: 2,
        });
        backend.render();

        let frame = backend.capture_frame();
        assert_eq!(frame.cell(2, 0).symbol, "A");
        assert_eq!(frame.cell(3, 0).symbol, "B");
        assert_eq!(frame.cell(4, 0).symbol, " ");
        assert_eq!(frame.cell(5, 0).symbol, " ");
    }

    #[test]
    fn measure_canvas_uses_requested_size_not_children() {
        let canvas = Canvas::new()
            .width(Length::Px(3))
            .height(Length::Percent(50))
            .child_at(
                Rect {
                    x: 0,
                    y: 0,
                    w: 99,
                    h: 99,
                },
                Text::new("large child"),
            );

        assert_eq!(measure_canvas(&canvas, Some(20), Some(10)), (3, 5));
    }

    #[test]
    fn default_canvas_measure_fills_offered_bounds() {
        let canvas = Canvas::new();

        assert_eq!(measure_canvas(&canvas, Some(20), Some(10)), (20, 10));
        assert_eq!(measure_canvas(&canvas, None, None), (0, 0));
    }
}
