//! Layout engine for computing widget positions and sizes.
//!
//! This module provides the `LayoutEngine` which reconciles the virtual element tree
//! with the realized node tree and computes layout rectangles.

pub(crate) mod axis;
pub(crate) mod dag;
pub(crate) mod drag_source_layout_hint;
pub(crate) mod hash;
pub(crate) mod measure;

pub(crate) mod reconcile;

pub(crate) mod stack;
pub(crate) mod tag;

use crate::core::component::FocusContext;
use crate::core::element::Element;
use crate::core::node::NodeTree;
use crate::style::Rect;

/// Stateless layout engine.
#[derive(Debug, Default)]
pub(crate) struct LayoutEngine;

impl LayoutEngine {
    /// Reconcile the existing tree with a new `Element` root and compute rects.
    /// This variant uses no overlays; primarily for testing.
    #[cfg(test)]
    pub(crate) fn reconcile_with_focus(
        tree: &mut NodeTree,
        root: &Element,
        bounds: Rect,
        focus: Option<&FocusContext>,
    ) {
        reconcile::reconcile_with_overlays_mode(tree, root, bounds, focus, &[], true)
    }

    #[cfg(test)]
    pub(crate) fn reconcile_with_overlays(
        tree: &mut NodeTree,
        root: &Element,
        bounds: Rect,
        focus: Option<&FocusContext>,
        overlays: &[crate::overlay::OverlayEntry],
    ) {
        reconcile::reconcile_with_overlays_mode(tree, root, bounds, focus, overlays, true)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reconcile_with_overlay_bounds(
        tree: &mut NodeTree,
        root: &Element,
        bounds: Rect,
        overlay_bounds: Rect,
        popover_expand_bounds: Option<Rect>,
        focus: Option<&FocusContext>,
        overlays: &[crate::overlay::OverlayEntry],
        allow_root_overlays: bool,
    ) {
        reconcile::reconcile_with_overlay_bounds(
            tree,
            root,
            bounds,
            overlay_bounds,
            popover_expand_bounds,
            focus,
            overlays,
            allow_root_overlays,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::element::{IntoElement, Key};
    use crate::core::node::{NodeId, NodeKind};
    use crate::style::{Color, Length, Style, Theme};
    use crate::widgets::{
        Button, Center, ContextProvider, EffectScope, Frame, HStack, Input, List, ListItem, Modal,
        ScrollView, Spacer, Text, ThemeProvider, VStack, ZStack,
    };

    fn find_by_key(tree: &NodeTree, k: &str) -> NodeId {
        let key = Key::from(k.to_string());
        tree.iter()
            .find(|n| n.key.as_ref() == Some(&key))
            .map(|n| n.id)
            .unwrap_or(NodeId::INVALID)
    }

    fn scroll_view_with_key(key: &'static str) -> Element {
        ScrollView::new()
            .children((0..20).map(|i| Text::new(format!("row {i}")).height(Length::Px(1)).into()))
            .key(key)
    }

    #[test]
    fn theme_provider_root_reuses_realized_child_node() {
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        };
        let theme_a = Theme::default().primary(Style::new().fg(Color::Red));
        let theme_b = Theme::default().primary(Style::new().fg(Color::Blue));

        let root1: Element = ThemeProvider::new(theme_a)
            .child(scroll_view_with_key("sv-root"))
            .into();
        LayoutEngine::reconcile_with_focus(&mut tree, &root1, bounds, None);

        let id1 = find_by_key(&tree, "sv-root");
        assert!(matches!(&tree.node(id1).kind, NodeKind::ScrollView(_)));

        let root2: Element = ThemeProvider::new(theme_b.clone())
            .child(scroll_view_with_key("sv-root"))
            .into();
        LayoutEngine::reconcile_with_focus(&mut tree, &root2, bounds, None);

        let id2 = find_by_key(&tree, "sv-root");
        assert_eq!(id1, id2);
        assert_eq!(tree.node(id2).active_theme().primary.fg, theme_b.primary.fg);
    }

    #[test]
    fn transparent_providers_inside_stack_reuse_realized_child_nodes() {
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };
        let theme_a = Theme::default().primary(Style::new().fg(Color::Red));
        let theme_b = Theme::default().primary(Style::new().fg(Color::Blue));

        let root1: Element = VStack::new()
            .child(ThemeProvider::new(theme_a).child(scroll_view_with_key("stack-sv")))
            .child(ContextProvider::new(1u32).child(Text::new("ctx").key("ctx-text")))
            .into();
        LayoutEngine::reconcile_with_focus(&mut tree, &root1, bounds, None);

        let scroll_id1 = find_by_key(&tree, "stack-sv");
        let text_id1 = find_by_key(&tree, "ctx-text");
        assert!(!scroll_id1.is_invalid());
        assert!(!text_id1.is_invalid());

        let root2: Element = VStack::new()
            .child(ThemeProvider::new(theme_b).child(scroll_view_with_key("stack-sv")))
            .child(ContextProvider::new(2u32).child(Text::new("ctx").key("ctx-text")))
            .into();
        LayoutEngine::reconcile_with_focus(&mut tree, &root2, bounds, None);

        assert_eq!(scroll_id1, find_by_key(&tree, "stack-sv"));
        assert_eq!(text_id1, find_by_key(&tree, "ctx-text"));
    }

    #[test]
    fn keyed_children_keep_ids_when_reordered() {
        let mut tree = NodeTree::new();

        let a = Button::new("A").key("a");
        let b = Button::new("B").key("b");

        let el1: Element = VStack::new().child(a.clone()).child(b.clone()).into();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &el1,
            Rect {
                x: 0,
                y: 0,
                w: 50,
                h: 10,
            },
            None,
        );

        let id_a1 = find_by_key(&tree, "a");
        let id_b1 = find_by_key(&tree, "b");
        assert!(!id_a1.is_invalid());
        assert!(!id_b1.is_invalid());

        let el2: Element = VStack::new().child(b).child(a).into();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &el2,
            Rect {
                x: 0,
                y: 0,
                w: 50,
                h: 10,
            },
            None,
        );

        let id_a2 = find_by_key(&tree, "a");
        let id_b2 = find_by_key(&tree, "b");
        assert_eq!(id_a1, id_a2);
        assert_eq!(id_b1, id_b2);

        let root = tree.node(tree.root);
        assert_eq!(root.children, vec![id_b2, id_a2]);
    }

    #[test]
    fn unkeyed_children_keep_ids_when_shape_stable() {
        let mut tree = NodeTree::new();

        let el1: Element = VStack::new()
            .child(Button::new("A"))
            .child(Button::new("B"))
            .into();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &el1,
            Rect {
                x: 0,
                y: 0,
                w: 50,
                h: 10,
            },
            None,
        );

        let ids1: Vec<NodeId> = tree
            .iter()
            .filter(|n| matches!(&n.kind, NodeKind::Button { .. }))
            .map(|n| n.id)
            .collect();

        let el2: Element = VStack::new()
            .child(Button::new("A"))
            .child(Button::new("B"))
            .into();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &el2,
            Rect {
                x: 0,
                y: 0,
                w: 50,
                h: 10,
            },
            None,
        );

        let ids2: Vec<NodeId> = tree
            .iter()
            .filter(|n| matches!(&n.kind, NodeKind::Button { .. }))
            .map(|n| n.id)
            .collect();

        assert_eq!(ids1, ids2);
    }

    #[test]
    fn zstack_hit_test_prefers_last_child() {
        let mut tree = NodeTree::new();

        let a: Element = Button::new("A")
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .key("a");
        let b: Element = Button::new("B")
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .key("b");

        let el: Element = ZStack::new().child(a).child(b).into();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 50,
            h: 10,
        };
        LayoutEngine::reconcile_with_focus(&mut tree, &el, bounds, None);

        let id_a = find_by_key(&tree, "a");
        let id_b = find_by_key(&tree, "b");

        assert_eq!(tree.node(id_a).rect, bounds);
        assert_eq!(tree.node(id_b).rect, bounds);

        let hit = tree.hit_test(1, 1);
        assert_eq!(hit, Some(id_b));
    }

    #[test]
    fn zstack_hit_test_blocks_lower_layers_when_top_has_no_handler() {
        let mut tree = NodeTree::new();

        let base: Element = Button::new("Base")
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .key("base");

        let modal: Element = Frame::new().key("modal");

        // Center covers the entire ZStack layer, while its child is smaller.
        let overlay: Element = Center::new()
            .width(crate::style::Size::Fixed(10))
            .height(crate::style::Size::Fixed(4))
            .child(modal)
            .into();

        let el: Element = ZStack::new().child(base).child(overlay).into();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 50,
            h: 20,
        };
        LayoutEngine::reconcile_with_focus(&mut tree, &el, bounds, None);

        let id_base = find_by_key(&tree, "base");
        assert!(!id_base.is_invalid());

        // Click outside the centered modal: the top layer should block hit-testing.
        let hit = tree.hit_test(1, 1);
        assert_eq!(hit, None);
    }

    #[test]
    fn effect_scope_wraps_portaled_modal_overlay_content() {
        let mut tree = NodeTree::new();

        let root: Element = EffectScope::new()
            .dim_by(0.25)
            .child(
                Modal::new()
                    .width(Length::Px(20))
                    .height(Length::Px(4))
                    .child(Text::new("inside")),
            )
            .into();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 20,
        };
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let root_node = tree.node(tree.root);
        let NodeKind::EffectScope(root_scope) = &root_node.kind else {
            panic!("root must remain an effect scope wrapper");
        };
        assert!(root_scope.effects.is_empty());

        let overlays = tree.overlay_roots();
        assert_eq!(overlays.len(), 1);
        let overlay_node = tree.node(overlays[0].id);
        assert_eq!(
            overlay_node.rect,
            Rect {
                x: 30,
                y: 8,
                w: 20,
                h: 4,
            }
        );
        let NodeKind::EffectScope(overlay_scope) = &overlay_node.kind else {
            panic!("overlay root must carry the scoped effect");
        };
        assert_eq!(overlay_scope.effects.len(), 1);
        assert!(matches!(
            &tree.node(overlay_node.children[0]).kind,
            NodeKind::Frame(_)
        ));
    }

    #[test]
    fn hstack_percent_width_resolves_against_parent() {
        let mut tree = NodeTree::new();

        let el: Element = crate::widgets::HStack::new()
            .child(
                crate::widgets::Spacer::new()
                    .width(Length::Percent(25))
                    .key("p"),
            )
            .child(
                crate::widgets::Spacer::new()
                    .width(Length::Flex(1))
                    .key("f"),
            )
            .into();

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &el,
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 8,
            },
            None,
        );

        let p = tree.node(find_by_key(&tree, "p")).rect;
        let f = tree.node(find_by_key(&tree, "f")).rect;
        assert_eq!(p.w, 20);
        assert_eq!(p.x, 0);
        assert_eq!(f.w, 60);
        assert_eq!(f.x, 20);
    }

    #[test]
    fn vstack_percent_height_resolves_against_parent() {
        let mut tree = NodeTree::new();

        let el: Element = crate::widgets::VStack::new()
            .child(
                crate::widgets::Spacer::new()
                    .height(Length::Percent(50))
                    .key("top"),
            )
            .child(
                crate::widgets::Spacer::new()
                    .height(Length::Flex(1))
                    .key("bottom"),
            )
            .into();

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &el,
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 20,
            },
            None,
        );

        let top = tree.node(find_by_key(&tree, "top")).rect;
        let bottom = tree.node(find_by_key(&tree, "bottom")).rect;
        assert_eq!(top.h, 10);
        assert_eq!(top.y, 0);
        assert_eq!(bottom.h, 10);
        assert_eq!(bottom.y, 10);
    }

    #[test]
    fn nested_percent_frame_does_not_double_apply_height() {
        let mut tree = NodeTree::new();

        let top: Element = Frame::new()
            .title("Top")
            .border(true)
            .height(Length::Percent(50))
            .child(
                crate::widgets::HStack::new()
                    .gap(1)
                    .child(
                        Frame::new()
                            .title("A")
                            .border(true)
                            .width(Length::Percent(25))
                            .key("a"),
                    )
                    .child(
                        Frame::new()
                            .title("B")
                            .border(true)
                            .width(Length::Percent(35))
                            .key("b"),
                    )
                    .child(
                        Frame::new()
                            .title("C")
                            .border(true)
                            .width(Length::Flex(1))
                            .key("c"),
                    ),
            )
            .key("top");

        let root: Element = crate::widgets::VStack::new()
            .child(top)
            .child(
                crate::widgets::Spacer::new()
                    .height(Length::Flex(1))
                    .key("rest"),
            )
            .into();

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 40,
            },
            None,
        );

        let top_rect = tree.node(find_by_key(&tree, "top")).rect;
        let card_a = tree.node(find_by_key(&tree, "a")).rect;

        assert_eq!(top_rect.h, 20);
        // Top frame has border=true and no padding, so child content height is h - 2.
        assert_eq!(card_a.h, 18);
    }

    #[test]
    fn test_flex_stack_not_pushed_by_content() {
        use crate::widgets::{Frame, HStack, Input, VStack};
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 20,
        };

        let long_input = Input::new("I am a very long input that should not push its parent");

        let root: Element = HStack::new()
            .gap(0)
            .child(
                VStack::new()
                    .width(Length::Flex(1))
                    .child(long_input)
                    .key("left"),
            )
            .child(
                VStack::new()
                    .width(Length::Flex(1))
                    .child(Frame::new())
                    .key("right"),
            )
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let left_id = find_by_key(&tree, "left");
        let right_id = find_by_key(&tree, "right");

        let left_rect = tree.node(left_id).rect;
        let right_rect = tree.node(right_id).rect;

        assert_eq!(left_rect.w, 50);
        assert_eq!(right_rect.w, 50);
    }

    #[test]
    fn hstack_equal_flex_frames_ignore_border_chrome_width() {
        use crate::widgets::HStack;

        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 81,
            h: 10,
        };

        let root: Element = HStack::new()
            .gap(1)
            .child(
                Frame::new()
                    .title("Bordered")
                    .border(true)
                    .width(Length::Flex(1))
                    .key("bordered"),
            )
            .child(
                Frame::new()
                    .title("Plain")
                    .border(false)
                    .width(Length::Flex(1))
                    .key("plain"),
            )
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let bordered_id = find_by_key(&tree, "bordered");
        let plain_id = find_by_key(&tree, "plain");
        let bordered = tree.node(bordered_id).rect;
        let plain = tree.node(plain_id).rect;

        assert_eq!(bordered.w, plain.w);
    }

    #[test]
    fn center_positions_child() {
        let mut tree = NodeTree::new();

        let modal: Element = Frame::new().key("modal");

        let el: Element = Center::new()
            .width(crate::style::Size::Fixed(10))
            .height(crate::style::Size::Fixed(4))
            .child(modal)
            .into();

        let bounds = Rect {
            x: 0,
            y: 0,
            w: 50,
            h: 20,
        };
        LayoutEngine::reconcile_with_focus(&mut tree, &el, bounds, None);

        let id_modal = find_by_key(&tree, "modal");
        assert_eq!(
            tree.node(id_modal).rect,
            Rect {
                x: 20,
                y: 8,
                w: 10,
                h: 4,
            }
        );
    }

    #[test]
    fn test_hstack_caching_on_textarea_update() {
        use crate::widgets::{HStack, TextArea};
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 100,
        };

        // Initial render
        let text_area = TextArea::new("Initial").height(Length::Flex(1));
        let root: Element = HStack::new()
            .child(text_area)
            .child(crate::widgets::Spacer::new().width(Length::Px(10)))
            .into();

        LayoutEngine::reconcile_with_overlays(&mut tree, &root, bounds, None, &[]);

        // Second render - Change text content ONLY
        println!("--- Second Render (Update Text) ---");
        let text_area_2 = TextArea::new("Updated").height(Length::Flex(1));
        let root_2: Element = HStack::new()
            .child(text_area_2)
            .child(crate::widgets::Spacer::new().width(Length::Px(10)))
            .into();

        LayoutEngine::reconcile_with_overlays(&mut tree, &root_2, bounds, None, &[]);

        // If output shows "HStack Relayout!", then caching failed.
    }

    #[test]
    fn test_textarea_auto_height_with_wrapping_consistency() {
        // This test verifies that TextArea with height: Auto and wrapping
        // calculates consistent heights between measurement and reconciliation.
        // Previously, there was a bug where measure_text_area_constrained used
        // natural width for wrapping calculations, but reconcile_text_area used
        // the full allocated width, causing height mismatches.
        use crate::core::node::NodeKind;
        use crate::widgets::{Text, TextArea, VStack};

        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 50,
            h: 20,
        };

        // TextArea with Auto height (wraps by default)
        // The text "ab" is short and should fit on one line
        let text_area = TextArea::new("ab").height(Length::Auto).border(false);

        // Add a text widget below to check for gaps
        let status_text = Text::new("status");

        let root: Element = VStack::new().child(text_area).child(status_text).into();

        LayoutEngine::reconcile_with_overlays(&mut tree, &root, bounds, None, &[]);

        // Find the TextArea and Text nodes
        let mut textarea_rect = None;
        let mut text_rect = None;

        for node in tree.iter() {
            match &node.kind {
                NodeKind::TextArea(_) => {
                    textarea_rect = Some(node.rect);
                }
                NodeKind::Text(t)
                    if !t.spans.is_empty() && t.spans[0].content.as_ref() == "status" =>
                {
                    text_rect = Some(node.rect);
                }
                _ => {}
            }
        }

        let textarea_rect = textarea_rect.expect("TextArea node not found");
        let text_rect = text_rect.expect("Text node not found");

        // The TextArea should have height of 1 line (plus any padding)
        // With border=false and default padding (0), height should be 1
        assert_eq!(
            textarea_rect.h, 1,
            "TextArea with 'ab' should have height 1"
        );

        // The Text widget should be positioned directly below the TextArea
        // without any extra gap (gap defaults to 0 in VStack)
        let expected_text_y = textarea_rect.y + textarea_rect.h as i16;
        assert_eq!(
            text_rect.y, expected_text_y,
            "Text widget should be directly below TextArea without extra space. \
             TextArea rect: {:?}, Text rect: {:?}",
            textarea_rect, text_rect
        );
    }

    #[test]
    fn frame_overflowing_textarea_keeps_siblings_visible() {
        use crate::core::node::NodeKind;
        use crate::widgets::{Frame, HStack, Text, TextArea, VStack};

        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 60,
            h: 12,
        };

        let mut value = String::new();
        for i in 0..30 {
            value.push_str(&format!("line{index}\n", index = i));
        }

        let frame_content: Element = VStack::new()
            .gap(0)
            .child(TextArea::new(value).height(Length::Auto))
            .child(
                HStack::new()
                    .height(Length::Px(1))
                    .child(Text::new("status")),
            )
            .into();

        let root: Element = VStack::new()
            .gap(1)
            .child(HStack::new().child(Text::new("logo")))
            .child(Frame::new().border(true).child(frame_content))
            .child(HStack::new().child(Text::new("shortcuts")))
            .into();

        LayoutEngine::reconcile_with_overlays(&mut tree, &root, bounds, None, &[]);

        let mut shortcuts_parent = None;
        for node in tree.iter() {
            if let NodeKind::Text(t) = &node.kind
                && !t.spans.is_empty()
                && t.spans[0].content.as_ref() == "shortcuts"
            {
                shortcuts_parent = node.parent;
                break;
            }
        }

        let shortcuts_parent = shortcuts_parent.expect("shortcuts text not found");
        let shortcuts_rect = tree.node(shortcuts_parent).rect;
        let shortcuts_bottom = shortcuts_rect.y.saturating_add(shortcuts_rect.h as i16);
        let bounds_bottom = bounds.y.saturating_add(bounds.h as i16);

        assert!(
            shortcuts_bottom <= bounds_bottom,
            "shortcuts row should remain within viewport. Rect: {:?}, bounds: {:?}",
            shortcuts_rect,
            bounds
        );
    }

    #[test]
    fn hstack_justify_center_positions_children() {
        use crate::core::node::NodeKind;
        use crate::style::Justify;
        use crate::widgets::{HStack, Text};

        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 3,
        };

        let root: Element = HStack::new()
            .gap(1)
            .justify(Justify::Center)
            .child(Text::new("aaa"))
            .child(Text::new("bb"))
            .into();

        LayoutEngine::reconcile_with_overlays(&mut tree, &root, bounds, None, &[]);

        let mut min_x: Option<i16> = None;
        for node in tree.iter() {
            if let NodeKind::Text(t) = &node.kind
                && !t.spans.is_empty()
                && (t.spans[0].content.as_ref() == "aaa" || t.spans[0].content.as_ref() == "bb")
            {
                min_x = Some(min_x.map_or(node.rect.x, |x| x.min(node.rect.x)));
            }
        }

        let min_x = min_x.expect("expected text nodes to be laid out");
        let expected_x = 7i16;
        assert_eq!(
            min_x, expected_x,
            "expected HStack contents to be centered at x=7, got {min_x}"
        );
    }

    #[test]
    fn hstack_end_justify_uses_max_constrained_focused_width() {
        use crate::style::Justify;

        fn row(value: &str) -> Element {
            let input = Input::new(value)
                .placeholder("Search… (/)")
                .width(Length::Auto)
                .height(Length::Px(1))
                .border(false)
                .padding(0)
                .key("input");
            HStack::new()
                .justify(Justify::End)
                .height(Length::Px(1))
                .child(input.min_width(Length::Px(11)).max_width(Length::Px(22)))
                .into()
        }

        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 60,
            h: 1,
        };
        LayoutEngine::reconcile_with_focus(&mut tree, &row(""), bounds, None);
        let focus = crate::core::component::FocusContext::default();
        let focused = find_by_key(&tree, "input");
        focus.update_from_tree(&tree, Some(focused), None);
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &row("here i am quite long and it is moving left"),
            bounds,
            Some(&focus),
        );

        let input = tree.node(find_by_key(&tree, "input")).rect;
        assert_eq!(input.w, 22);
        assert_eq!(input.x, 38, "max-constrained input should stay end-aligned");
    }

    // ---------------------------------------------------------------
    // compute_stack_layout tests
    // ---------------------------------------------------------------

    #[test]
    fn mixed_px_auto_flex_distribution() {
        // VStack with Px(10) + Auto + Flex(1) + Flex(2) in 40 available height.
        // Px gets 10, Auto gets content size (1), remaining 29 split 1:2.
        use crate::widgets::{Spacer, Text};

        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 40,
        };

        let root: Element = VStack::new()
            .gap(0)
            .child(Text::new("px").height(Length::Px(10)).key("px"))
            .child(Text::new("a").key("auto"))
            .child(Spacer::new().height(Length::Flex(1)).key("f1"))
            .child(Spacer::new().height(Length::Flex(2)).key("f2"))
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let px = tree.node(find_by_key(&tree, "px")).rect;
        let auto = tree.node(find_by_key(&tree, "auto")).rect;
        let f1 = tree.node(find_by_key(&tree, "f1")).rect;
        let f2 = tree.node(find_by_key(&tree, "f2")).rect;

        assert_eq!(px.h, 10, "Px child should get exact 10");
        assert_eq!(auto.h, 1, "Auto child (single-line text) should get 1");

        // Remaining = 40 - 10 - 1 = 29, split 1:2 across flex_sum=3
        // Flex(1): 29*1/3 = 9, Flex(2): 29*2/3 = 19, used = 28, remainder = 1
        // Remainder goes round-robin: Flex(1) gets +1 → 10
        assert_eq!(f1.h, 10, "Flex(1) should get ceil(29/3) = 10");
        assert_eq!(f2.h, 19, "Flex(2) should get floor(29*2/3) = 19");

        // Verify total adds up
        assert_eq!(px.h + auto.h + f1.h + f2.h, 40);
    }

    #[test]
    fn flex_remainder_distribution() {
        // 3 Flex(1) children in 10 cells: remainder distributed round-robin.
        use crate::widgets::Spacer;

        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 10,
        };

        let root: Element = VStack::new()
            .gap(0)
            .child(Spacer::new().height(Length::Flex(1)).key("a"))
            .child(Spacer::new().height(Length::Flex(1)).key("b"))
            .child(Spacer::new().height(Length::Flex(1)).key("c"))
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let a = tree.node(find_by_key(&tree, "a")).rect.h;
        let b = tree.node(find_by_key(&tree, "b")).rect.h;
        let c = tree.node(find_by_key(&tree, "c")).rect.h;

        // 10/3 = 3 each, remainder 1 → first child gets +1
        assert_eq!(a, 4, "first Flex(1) gets remainder: 4");
        assert_eq!(b, 3, "second Flex(1) gets 3");
        assert_eq!(c, 3, "third Flex(1) gets 3");
        assert_eq!(a + b + c, 10, "total should equal available");
    }

    #[test]
    fn overflow_drops_auto_siblings_progressively_to_honor_px_size() {
        // VStack with 4 Auto Texts followed by a Px(20) List. Available height
        // 22 leaves a 2-row deficit beyond the List's full size. The Px(20)
        // is treated as rigid: rather than shrinking it, two trailing Auto
        // siblings are dropped (size 0, gaps suppressed) so the earlier
        // Texts remain at full size and the List stays at 20.
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 22,
        };

        let root: Element = VStack::new()
            .gap(0)
            .child(Text::new("t1").key("t1"))
            .child(Text::new("t2").key("t2"))
            .child(Text::new("t3").key("t3"))
            .child(Text::new("t4").key("t4"))
            .child(
                List::new()
                    .items(vec![
                        ListItem::new("a"),
                        ListItem::new("b"),
                        ListItem::new("c"),
                    ])
                    .height(Length::Px(20))
                    .border(true)
                    .key("list"),
            )
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let t1 = tree.node(find_by_key(&tree, "t1")).rect;
        let t2 = tree.node(find_by_key(&tree, "t2")).rect;
        let t3 = tree.node(find_by_key(&tree, "t3")).rect;
        let t4 = tree.node(find_by_key(&tree, "t4")).rect;
        let list = tree.node(find_by_key(&tree, "list")).rect;

        assert_eq!(list.h, 20, "List should keep its full Px(20) size");
        assert_eq!(t1.h, 0, "leading Texts drop first to free overflow");
        assert_eq!(t2.h, 0, "leading Texts drop first to free overflow");
        assert_eq!(t3.h, 1, "Texts closest to the Px anchor remain visible");
        assert_eq!(t4.h, 1, "Texts closest to the Px anchor remain visible");
    }

    #[test]
    fn overflow_shrinks_auto_first() {
        // Total Px+Auto children exceed available. Auto children shrink before Px.
        // VStack h=10: Px(8) + Auto(content=5 lines "a\nb\nc\nd\ne")
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 10,
        };

        let root: Element = VStack::new()
            .gap(0)
            .child(Text::new("fixed").height(Length::Px(8)).key("px"))
            .child(Text::new("a\nb\nc\nd\ne").key("auto"))
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let px = tree.node(find_by_key(&tree, "px")).rect;
        let auto = tree.node(find_by_key(&tree, "auto")).rect;

        // Px(8) is not shrinkable, keeps 8. Auto(5) is shrinkable, gets 10-8=2.
        assert_eq!(px.h, 8, "Px child should keep its fixed size");
        assert_eq!(auto.h, 2, "Auto child should shrink to fill remaining");
    }

    #[test]
    fn small_overflow_does_not_collapse_all_collapsible_frames() {
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 54,
            h: 21,
        };

        let root: Element = VStack::new()
            .gap(1)
            .padding((1, 1))
            .child(
                Frame::new()
                    .height(Length::Auto)
                    .child(Text::new("a\nb\nc\nd\ne\nf\ng").height(Length::Px(7)))
                    .key("header"),
            )
            .child(
                Frame::new()
                    .height(Length::Flex(1))
                    .child(Text::new("timeline"))
                    .key("timeline"),
            )
            .child(
                Frame::new()
                    .height(Length::Auto)
                    .child(Text::new("a\nb\nc\nd").height(Length::Px(4)))
                    .key("input"),
            )
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let header = tree.node(find_by_key(&tree, "header")).rect;
        let timeline = tree.node(find_by_key(&tree, "timeline")).rect;
        let input = tree.node(find_by_key(&tree, "input")).rect;

        assert!(
            header.h > 3,
            "one-row overflow should shrink instead of collapsing header: {header:?}"
        );
        assert_eq!(timeline.h, 3, "flex frame keeps its chrome floor");
        assert_eq!(input.h, 6, "later auto frame remains at natural height");
        assert_eq!(
            header.h + timeline.h + input.h + 2,
            19,
            "children and two gaps should fill the padded inner height"
        );
    }

    #[test]
    fn max_height_reserves_space_for_later_auto_siblings() {
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 20,
        };

        let capped: Element = Element::from(
            VStack::new()
                .height(Length::Auto)
                .gap(1)
                .child(Text::new("header").key("header"))
                .child(
                    Frame::new()
                        .border(false)
                        .height(Length::Auto)
                        .child(Text::new("a\nb\nc\nd\ne\nf").key("body"))
                        .key("palette"),
                )
                .child(Text::new("footer").key("footer")),
        )
        .max_height(Length::Px(6));

        let root: Element = VStack::new()
            .height(Length::Flex(1))
            .child(capped.key("capped"))
            .child(Spacer::new().height(Length::Flex(1)))
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let header = tree.node(find_by_key(&tree, "header")).rect;
        let palette = tree.node(find_by_key(&tree, "palette")).rect;
        let footer = tree.node(find_by_key(&tree, "footer")).rect;

        assert_eq!(header.h, 1);
        assert_eq!(
            palette.h, 2,
            "middle auto child should use only remaining bounded space"
        );
        assert_eq!(footer.h, 1);
        assert_eq!(
            footer.y, 5,
            "footer should remain visible within the capped stack"
        );
    }

    #[test]
    fn later_auto_sibling_keeps_visible_space_under_cap() {
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 20,
        };

        let capped: Element = Element::from(
            VStack::new()
                .height(Length::Auto)
                .child(Text::new("a\nb\nc\nd").key("first"))
                .child(Text::new("e\nf\ng\nh").key("second")),
        )
        .max_height(Length::Px(5));

        let root: Element = VStack::new()
            .height(Length::Flex(1))
            .child(capped)
            .child(Spacer::new().height(Length::Flex(1)))
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let first = tree.node(find_by_key(&tree, "first")).rect;
        let second = tree.node(find_by_key(&tree, "second")).rect;

        assert_eq!(first.h, 1);
        assert_eq!(second.h, 4);
    }

    #[test]
    fn max_width_reserves_space_for_later_auto_siblings() {
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        };

        let capped: Element = Element::from(
            HStack::new()
                .width(Length::Auto)
                .child(Text::new("L").key("left"))
                .child(
                    Frame::new()
                        .border(false)
                        .child(Text::new("abcdef"))
                        .key("middle"),
                )
                .child(Text::new("R").key("right")),
        )
        .max_width(Length::Px(6));

        let root: Element = VStack::new()
            .height(Length::Flex(1))
            .child(capped)
            .child(Spacer::new().height(Length::Flex(1)))
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let middle = tree.node(find_by_key(&tree, "middle")).rect;
        let right = tree.node(find_by_key(&tree, "right")).rect;

        assert_eq!(middle.w, 4);
        assert_eq!(right.x, 5);
    }

    #[test]
    fn max_width_caps_filling_children_on_the_cross_axis() {
        use crate::style::Align;
        use crate::widgets::ScrollView;
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };
        let column = |key: &'static str| -> Element {
            VStack::new()
                .height(Length::Px(1))
                .child(Text::new("x"))
                .key(key)
                .max_width(Length::Px(20))
        };

        for align in [Align::Center, Align::Stretch] {
            let mut tree = NodeTree::new();
            let root: Element = VStack::new().align(align).child(column("stack")).into();
            LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
            let rect = tree.node(find_by_key(&tree, "stack")).rect;
            assert_eq!(rect.w, 20, "{align:?}");
            let expected_x = if align == Align::Center { 10 } else { 0 };
            assert_eq!(rect.x, expected_x, "{align:?}");
        }

        let mut tree = NodeTree::new();
        let root: Element = ScrollView::new()
            .align(Align::Center)
            .child(column("scrolled"))
            .into();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
        let rect = tree.node(find_by_key(&tree, "scrolled")).rect;
        assert_eq!((rect.x, rect.w), (10, 20));
    }

    #[test]
    fn max_height_caps_filling_children_in_horizontal_stack() {
        use crate::style::Align;
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };

        for (cap, expected_h) in [(Length::Px(8), 8), (Length::Percent(50), 10)] {
            for align in [Align::Center, Align::Stretch] {
                let mut tree = NodeTree::new();
                let child: Element = VStack::new()
                    .width(Length::Px(10))
                    .child(Text::new("x"))
                    .key("panel")
                    .max_height(cap);
                let root: Element = HStack::new().align(align).child(child).into();
                LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
                let rect = tree.node(find_by_key(&tree, "panel")).rect;
                assert_eq!(rect.h, expected_h, "cap={cap:?} align={align:?}");
                let expected_y = if align == Align::Center {
                    ((bounds.h - expected_h) / 2) as i16
                } else {
                    0
                };
                assert_eq!(rect.y, expected_y, "cap={cap:?} align={align:?}");
            }
        }
    }

    #[test]
    fn min_constraint_wins_over_max_on_stack_and_scroll_cross_axis() {
        use crate::style::Align;
        use crate::widgets::ScrollView;

        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 40,
        };

        for align in [Align::Center, Align::Stretch] {
            // Vertical stack: min_width(30) wins over max_width(20) (both explicit
            // and when a percentage max_width resolves below min_width).
            let mut tree = NodeTree::new();
            let vchild: Element = VStack::new()
                .height(Length::Px(1))
                .child(Text::new("x"))
                .key("vchild")
                .max_width(Length::Px(20))
                .min_width(Length::Px(30));
            let vroot: Element = VStack::new().align(align).child(vchild).into();
            LayoutEngine::reconcile_with_focus(&mut tree, &vroot, bounds, None);
            let rect = tree.node(find_by_key(&tree, "vchild")).rect;
            assert_eq!(rect.w, 30, "vstack {align:?}");
            assert_eq!(rect.x, if align == Align::Center { 5 } else { 0 });

            // Horizontal stack: min_height(30) wins over max_height(20).
            let mut tree = NodeTree::new();
            let hchild: Element = VStack::new()
                .width(Length::Px(5))
                .child(Text::new("x"))
                .key("hchild")
                .max_height(Length::Px(20))
                .min_height(Length::Px(30));
            let hroot: Element = HStack::new().align(align).child(hchild).into();
            LayoutEngine::reconcile_with_focus(&mut tree, &hroot, bounds, None);
            let rect = tree.node(find_by_key(&tree, "hchild")).rect;
            assert_eq!(rect.h, 30, "hstack {align:?}");
            assert_eq!(rect.y, if align == Align::Center { 5 } else { 0 });

            // ScrollView: min_width(30) wins over max_width(50%) (= 20 at width 40)
            // and measures wrapping content at 30 columns (so 25 chars fit on 1 line, not 2).
            let mut tree = NodeTree::new();
            let schild: Element = VStack::new()
                .height(Length::Auto)
                .child(Text::new("a".repeat(25)).overflow(crate::widgets::Overflow::Wrap))
                .key("schild")
                .min_width(Length::Px(30))
                .max_width(Length::Percent(50));
            let sroot: Element = ScrollView::new()
                .scrollbar(false)
                .align(align)
                .child(schild)
                .into();
            LayoutEngine::reconcile_with_focus(&mut tree, &sroot, bounds, None);
            let rect = tree.node(find_by_key(&tree, "schild")).rect;
            assert_eq!(rect.w, 30, "scroll {align:?}");
            assert_eq!(rect.h, 1, "scroll wrap height {align:?}");
            assert_eq!(rect.x, if align == Align::Center { 5 } else { 0 });
        }

        // Still clamped to parent bounds when parent is narrower than min_width.
        let mut tree = NodeTree::new();
        let narrow = Rect {
            x: 0,
            y: 0,
            w: 25,
            h: 10,
        };
        let child: Element = VStack::new()
            .height(Length::Px(1))
            .child(Text::new("x"))
            .key("narrow")
            .max_width(Length::Px(20))
            .min_width(Length::Px(30));
        let root: Element = VStack::new().align(Align::Center).child(child).into();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, narrow, None);
        assert_eq!(tree.node(find_by_key(&tree, "narrow")).rect.w, 25);
    }

    #[test]
    fn percent_max_constraints_are_not_reapplied_during_leaf_reconcile() {
        use crate::style::Align;
        use crate::widgets::{EffectScope, Overflow, ScrollView};

        let bounds = Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 20,
        };

        // Filling Text directly inside a 100-col VStack with max_width(50%)
        // must keep its 50-col final node rect (not be halved again to 25).
        for align in [Align::Center, Align::Stretch] {
            let mut tree = NodeTree::new();
            let text: Element = Text::new("a".repeat(60))
                .width(Length::Flex(1))
                .overflow(Overflow::Wrap)
                .key("leaf")
                .max_width(Length::Percent(50));
            let root: Element = VStack::new().align(align).child(text).into();
            LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
            let rect = tree.node(find_by_key(&tree, "leaf")).rect;
            assert_eq!(rect.w, 50, "vstack leaf text {align:?}");
            assert_eq!(rect.h, 2, "vstack leaf text height {align:?}");
            assert_eq!(rect.x, if align == Align::Center { 25 } else { 0 });
        }

        // Filling Input directly inside a 100-col VStack.
        let mut tree = NodeTree::new();
        let input: Element = Input::new("hello")
            .key("inp")
            .max_width(Length::Percent(50));
        let root: Element = VStack::new().align(Align::Center).child(input).into();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
        let rect = tree.node(find_by_key(&tree, "inp")).rect;
        assert_eq!((rect.x, rect.w), (25, 50));

        // Filling Text directly inside a 100-col ScrollView.
        let mut tree = NodeTree::new();
        let text: Element = Text::new("a".repeat(60))
            .width(Length::Flex(1))
            .overflow(Overflow::Wrap)
            .key("scroll-leaf")
            .max_width(Length::Percent(50));
        let root: Element = ScrollView::new()
            .scrollbar(false)
            .align(Align::Center)
            .child(text)
            .into();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
        let rect = tree.node(find_by_key(&tree, "scroll-leaf")).rect;
        assert_eq!((rect.x, rect.w, rect.h), (25, 50, 2));

        // Filling Text directly inside a 20-row HStack with max_height(50%).
        let mut tree = NodeTree::new();
        let text: Element = Text::new("x")
            .height(Length::Flex(1))
            .key("h-leaf")
            .max_height(Length::Percent(50));
        let root: Element = HStack::new().align(Align::Center).child(text).into();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
        let rect = tree.node(find_by_key(&tree, "h-leaf")).rect;
        assert_eq!((rect.y, rect.h), (5, 10));

        // Transparent wrapper (EffectScope) preserves the stack's available bounds.
        let mut tree = NodeTree::new();
        let wrapped: Element = EffectScope::new()
            .child(
                Text::new("a".repeat(60))
                    .width(Length::Flex(1))
                    .overflow(Overflow::Wrap)
                    .key("wrapped-leaf")
                    .max_width(Length::Percent(50)),
            )
            .into();
        let root: Element = VStack::new().align(Align::Center).child(wrapped).into();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
        let rect = tree.node(find_by_key(&tree, "wrapped-leaf")).rect;
        assert_eq!((rect.x, rect.w, rect.h), (25, 50, 2));

        // Nested VStack: inner 50% stack (50 cols) -> child 50% text resolves against 50 -> 25.
        let mut tree = NodeTree::new();
        let inner_leaf: Element = Text::new("x")
            .width(Length::Flex(1))
            .key("nested-leaf")
            .max_width(Length::Percent(50));
        let inner_stack: Element = VStack::new()
            .align(Align::Center)
            .child(inner_leaf)
            .key("nested-stack")
            .max_width(Length::Percent(50));
        let root: Element = VStack::new().align(Align::Center).child(inner_stack).into();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
        assert_eq!(tree.node(find_by_key(&tree, "nested-stack")).rect.w, 50);
        assert_eq!(tree.node(find_by_key(&tree, "nested-leaf")).rect.w, 25);
    }

    #[test]
    fn min_constraint_wins_over_explicit_width_during_measurement() {
        use crate::style::Align;
        use crate::widgets::{Overflow, ScrollView};

        let make_text = |key: &'static str| -> Element {
            Text::new("a".repeat(25))
                .width(Length::Px(10))
                .overflow(Overflow::Wrap)
                .key(key)
                .max_width(Length::Px(20))
                .min_width(Length::Px(30))
        };

        // 40-col parent: min_width(30) beats both width(10) and max_width(20),
        // so 25 chars fit on 1 row instead of wrapping at 10 cols into 3 rows.
        let wide = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };
        let mut tree = NodeTree::new();
        let vroot: Element = VStack::new()
            .align(Align::Center)
            .child(make_text("v"))
            .into();
        LayoutEngine::reconcile_with_focus(&mut tree, &vroot, wide, None);
        let vrect = tree.node(find_by_key(&tree, "v")).rect;
        assert_eq!((vrect.x, vrect.w, vrect.h), (5, 30, 1));

        let mut tree = NodeTree::new();
        let sroot: Element = ScrollView::new()
            .scrollbar(false)
            .align(Align::Center)
            .child(make_text("s"))
            .into();
        LayoutEngine::reconcile_with_focus(&mut tree, &sroot, wide, None);
        let srect = tree.node(find_by_key(&tree, "s")).rect;
        assert_eq!((srect.x, srect.w, srect.h), (5, 30, 1));

        // 20-col parent: measurement still respects parent's available width (20),
        // wrapping 25 chars into 2 rows.
        let narrow = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 10,
        };
        let mut tree = NodeTree::new();
        let nroot: Element = VStack::new().child(make_text("n")).into();
        LayoutEngine::reconcile_with_focus(&mut tree, &nroot, narrow, None);
        let nrect = tree.node(find_by_key(&tree, "n")).rect;
        assert_eq!((nrect.w, nrect.h), (20, 2));
    }

    #[test]
    fn bounded_flex_scrollable_child_keeps_footer_visible() {
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 10,
        };

        let list: Element = List::new()
            .height(Length::Flex(1))
            .items([
                ListItem::new("one"),
                ListItem::new("two"),
                ListItem::new("three"),
            ])
            .key("list");

        let capped: Element = Element::from(
            VStack::new()
                .height(Length::Auto)
                .child(Text::new("header").key("header"))
                .child(list)
                .child(Text::new("footer").key("footer")),
        )
        .max_height(Length::Px(3));

        let root: Element = VStack::new()
            .height(Length::Flex(1))
            .child(capped)
            .child(Spacer::new().height(Length::Flex(1)))
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let list_rect = tree.node(find_by_key(&tree, "list")).rect;
        let footer = tree.node(find_by_key(&tree, "footer")).rect;

        assert_eq!(list_rect.h, 1);
        assert_eq!(footer.y, 2);
    }

    #[test]
    fn modal_max_height_keeps_footer_visible() {
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };

        let modal_content: Element = VStack::new()
            .height(Length::Auto)
            .gap(1)
            .child(Text::new("header").key("modal_header"))
            .child(
                Frame::new()
                    .border(false)
                    .height(Length::Auto)
                    .child(Text::new("a\nb\nc\nd\ne\nf").key("modal_body"))
                    .key("modal_palette"),
            )
            .child(Text::new("footer").key("modal_footer"))
            .into();

        let root: Element =
            Element::from(Modal::new().border(false).padding(0).child(modal_content))
                .max_height(Length::Percent(50));

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let palette = tree.node(find_by_key(&tree, "modal_palette")).rect;
        let footer = tree.node(find_by_key(&tree, "modal_footer")).rect;

        assert_eq!(palette.h, 1);
        assert!(footer.y.saturating_add(footer.h as i16) <= 7);
    }

    #[test]
    fn auto_height_frame_keeps_default_hstack_action_row_visible() {
        let actions: Element = HStack::new()
            .gap(2)
            .child(Button::new("Parent"))
            .child(Button::new("Prev"))
            .child(Button::new("Next"))
            .into();

        let frame: Element = Frame::new()
            .border(false)
            .height(Length::Auto)
            .padding((1, 1, 1, 3))
            .child(
                VStack::new()
                    .height(Length::Auto)
                    .gap(1)
                    .child(
                        VStack::new()
                            .height(Length::Auto)
                            .gap(0)
                            .child(Text::new("Subagent session").key("title")),
                    )
                    .child(actions.key("actions")),
            )
            .key("frame");

        let root: Element = VStack::new()
            .child(frame)
            .child(Spacer::new().height(Length::Flex(1)).key("rest"))
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

        let frame_rect = tree.node(find_by_key(&tree, "frame")).rect;
        let title_rect = tree.node(find_by_key(&tree, "title")).rect;
        let actions_rect = tree.node(find_by_key(&tree, "actions")).rect;

        assert_eq!(frame_rect.h, 5);
        assert_eq!(title_rect.h, 1);
        assert!(actions_rect.h >= 1);
        assert!(title_rect.y < actions_rect.y);
    }

    #[test]
    fn min_constraint_never_violated() {
        // Child with min_height=5 in overflow scenario keeps at least 5 cells.
        // VStack h=10: Auto(8 lines) with min_h=5 + Auto(8 lines) with min_h=0.
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 10,
        };

        let root: Element = VStack::new()
            .gap(0)
            .child(
                Text::new("a\nb\nc\nd\ne\nf\ng\nh")
                    .min_height(Length::Px(5))
                    .key("constrained"),
            )
            .child(Text::new("a\nb\nc\nd\ne\nf\ng\nh").key("unconstrained"))
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let constrained = tree.node(find_by_key(&tree, "constrained")).rect;
        let unconstrained = tree.node(find_by_key(&tree, "unconstrained")).rect;

        // Both start at 8, total=16, overflow=6.
        // shrink_indices sorted by size desc: both at 8.
        // Constrained: cap = 8-5 = 3, take = min(6,3) = 3 → size 5
        // Unconstrained: cap = 8-0 = 8, take = min(3,8) = 3 → size 5
        assert!(
            constrained.h >= 5,
            "constrained child must keep min_height=5, got {}",
            constrained.h
        );
        assert_eq!(
            constrained.h + unconstrained.h,
            10,
            "total should equal available"
        );
    }

    #[test]
    fn single_flex_gets_all_space() {
        // One Flex(1) child gets entire available space.
        use crate::widgets::Spacer;

        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 30,
            h: 25,
        };

        let root: Element = VStack::new()
            .gap(0)
            .child(Spacer::new().height(Length::Flex(1)).key("sole"))
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let sole = tree.node(find_by_key(&tree, "sole")).rect;
        assert_eq!(sole.h, 25, "single Flex(1) should fill all 25 rows");
    }

    #[test]
    fn join_overlap_increases_effective_available() {
        // Two joined frames: effective available increases by 1 per overlap.
        // VStack h=20: two Frame.border(true).join_frame(true).height(Flex(1))
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 30,
            h: 20,
        };

        let root: Element = VStack::new()
            .gap(0)
            .child(
                Frame::new()
                    .border(true)
                    .join_frame(true)
                    .height(Length::Flex(1))
                    .key("top"),
            )
            .child(
                Frame::new()
                    .border(true)
                    .join_frame(true)
                    .height(Length::Flex(1))
                    .key("bot"),
            )
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let top = tree.node(find_by_key(&tree, "top")).rect;
        let bot = tree.node(find_by_key(&tree, "bot")).rect;

        // effective_available = 20 + 1 (one join overlap) = 21
        // Two Flex(1): 21/2 = 10 each, remainder 1 → first gets 11
        assert_eq!(
            top.h + bot.h,
            21,
            "joined frames should share 21 cells (20 + 1 overlap)"
        );
        // The rects overlap by 1 row, so bot.y = top.y + top.h - 1
        assert_eq!(
            bot.y,
            top.y + top.h as i16 - 1,
            "bottom frame should overlap top frame by 1 row"
        );
    }

    #[test]
    fn zero_available_no_panic() {
        // All children in 0 available space → no panic, sizes are 0 or min.
        use crate::widgets::{Spacer, Text};

        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 0,
        };

        let root: Element = VStack::new()
            .gap(0)
            .child(Text::new("hello").height(Length::Px(5)).key("px"))
            .child(Text::new("world").key("auto"))
            .child(Spacer::new().height(Length::Flex(1)).key("flex"))
            .into();

        // Should not panic
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let px = tree.node(find_by_key(&tree, "px")).rect;
        let auto = tree.node(find_by_key(&tree, "auto")).rect;
        let flex = tree.node(find_by_key(&tree, "flex")).rect;

        // With 0 available, everything should be clamped to 0
        assert_eq!(px.h, 0, "Px child clamped to 0 in zero-height bounds");
        assert_eq!(auto.h, 0, "Auto child clamped to 0 in zero-height bounds");
        assert_eq!(flex.h, 0, "Flex child clamped to 0 in zero-height bounds");
    }

    #[test]
    fn all_px_exact_sizes() {
        // 3 Px children: each gets exact requested size.
        use crate::widgets::Text;

        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 100,
        };

        let root: Element = VStack::new()
            .gap(0)
            .child(Text::new("a").height(Length::Px(5)).key("a"))
            .child(Text::new("b").height(Length::Px(10)).key("b"))
            .child(Text::new("c").height(Length::Px(15)).key("c"))
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let a = tree.node(find_by_key(&tree, "a")).rect;
        let b = tree.node(find_by_key(&tree, "b")).rect;
        let c = tree.node(find_by_key(&tree, "c")).rect;

        assert_eq!(a.h, 5, "Px(5) should get exactly 5");
        assert_eq!(b.h, 10, "Px(10) should get exactly 10");
        assert_eq!(c.h, 15, "Px(15) should get exactly 15");

        // Verify correct positioning (y offsets)
        assert_eq!(a.y, 0);
        assert_eq!(b.y, 5);
        assert_eq!(c.y, 15);
    }

    #[test]
    fn empty_vstack_sibling_does_not_steal_space_via_gap() {
        use crate::widgets::{Text, VStack};

        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };

        let root: Element = VStack::new()
            .gap(1)
            .child(Text::new("content").height(Length::Auto).key("content"))
            .child(VStack::new().height(Length::Auto).key("empty"))
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let content = tree.node(find_by_key(&tree, "content")).rect;
        let empty = tree.node(find_by_key(&tree, "empty")).rect;

        assert_eq!(content.h, 1, "Auto text should get its intrinsic 1 line");
        assert_eq!(empty.h, 0, "Empty VStack should get 0 height");
    }

    #[test]
    fn empty_vstack_sibling_no_shrink_of_auto_sibling() {
        use crate::widgets::{Text, VStack};

        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 5,
        };

        let root: Element = VStack::new()
            .gap(1)
            .child(Text::new("line1\nline2\nline3\nline4\nline5").key("big"))
            .child(VStack::new().height(Length::Auto).key("empty"))
            .into();

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let big = tree.node(find_by_key(&tree, "big")).rect;
        let empty = tree.node(find_by_key(&tree, "empty")).rect;

        assert_eq!(
            big.h, 5,
            "Auto text (5 lines) must not be shrunk by phantom gap from empty sibling"
        );
        assert_eq!(empty.h, 0);
    }

    #[test]
    fn transparent_wrapper_merges_mixed_px_and_percent_max_constraints() {
        use crate::style::{Align, Theme};
        use crate::widgets::{DropTarget, HStack, Overflow, Text, ThemeProvider, VStack};

        // 1. Child max_width(Px(20)) + wrapper max_width(Percent(50)):
        //    - at 100 cols => min(20, 50) = 20
        //    - at 30 cols  => min(20, 15) = 15
        let make_outer_pct = || -> Element {
            let child = Element::from(
                VStack::new()
                    .height(Length::Px(1))
                    .child(Text::new("content")),
            )
            .key("mixed-max")
            .max_width(Length::Px(20));
            let wrapped = Element::from(ThemeProvider::new(Theme::default()).child(child))
                .max_width(Length::Percent(50));
            VStack::new().align(Align::Center).child(wrapped).into()
        };

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &make_outer_pct(),
            Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 10,
            },
            None,
        );
        assert_eq!(tree.node(find_by_key(&tree, "mixed-max")).rect.w, 20);

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &make_outer_pct(),
            Rect {
                x: 0,
                y: 0,
                w: 30,
                h: 10,
            },
            None,
        );
        assert_eq!(tree.node(find_by_key(&tree, "mixed-max")).rect.w, 15);

        // 2. Child max_width(Percent(50)) + wrapper max_width(Px(60)):
        //    - at 100 cols => min(50, 60) = 50 (and wrapping text resolves 50% against 100, not 60!)
        //    - at 150 cols => min(75, 60) = 60
        let make_outer_px = || -> Element {
            let child = Element::from(
                Text::new("a".repeat(80))
                    .overflow(Overflow::Wrap)
                    .height(Length::Auto),
            )
            .max_width(Length::Percent(50));
            let wrapped = Element::from(DropTarget::new().child(child))
                .max_width(Length::Px(60))
                .key("mixed-wrap");
            VStack::new().align(Align::Center).child(wrapped).into()
        };

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &make_outer_px(),
            Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 10,
            },
            None,
        );
        let rect_100 = tree.node(find_by_key(&tree, "mixed-wrap")).rect;
        assert_eq!(
            (rect_100.w, rect_100.h),
            (50, 2),
            "80 chars wrapped at 50 cols (50% of 100) must take 2 rows, not 3 (50% of 60)"
        );

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &make_outer_px(),
            Rect {
                x: 0,
                y: 0,
                w: 150,
                h: 10,
            },
            None,
        );
        let rect_150 = tree.node(find_by_key(&tree, "mixed-wrap")).rect;
        assert_eq!((rect_150.w, rect_150.h), (60, 2));

        // 3. Mixed max_height on cross-axis of HStack:
        //    child max_height(Px(10)) + wrapper max_height(Percent(50)):
        //    - at 40 rows => min(10, 20) = 10
        //    - at 16 rows => min(10, 8) = 8
        let make_hstack = || -> Element {
            let child = Element::from(VStack::new().width(Length::Px(10)).child(Text::new("tall")))
                .key("mixed-h")
                .max_height(Length::Px(10));
            let wrapped = Element::from(ThemeProvider::new(Theme::default()).child(child))
                .max_height(Length::Percent(50));
            HStack::new().align(Align::Center).child(wrapped).into()
        };

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &make_hstack(),
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 40,
            },
            None,
        );
        assert_eq!(tree.node(find_by_key(&tree, "mixed-h")).rect.h, 10);

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &make_hstack(),
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 16,
            },
            None,
        );
        assert_eq!(tree.node(find_by_key(&tree, "mixed-h")).rect.h, 8);
    }

    #[test]
    fn unconstrained_transparent_wrappers_preserve_child_min_over_max_precedence() {
        use crate::style::{Align, Theme};
        use crate::widgets::{
            Animated, DragSource, DropTarget, EffectScope, Text, ThemeProvider, VStack,
        };

        let make_child = || -> Element {
            Element::from(VStack::new().height(Length::Px(1)).child(Text::new("x")))
                .key("wrapped")
                .max_width(Length::Px(20))
                .min_width(Length::Px(30))
        };

        let wrappers: Vec<(&str, Element)> = vec![
            (
                "ThemeProvider",
                ThemeProvider::new(Theme::default())
                    .child(make_child())
                    .into(),
            ),
            ("DropTarget", DropTarget::new().child(make_child()).into()),
            ("DragSource", DragSource::new().child(make_child()).into()),
            ("EffectScope", EffectScope::new().child(make_child()).into()),
            ("Animated", Animated::new(make_child()).into()),
        ];

        let bounds = Rect {
            x: 0,
            y: 0,
            w: 60,
            h: 10,
        };
        for (name, wrapped) in wrappers {
            let mut tree = NodeTree::new();
            let root: Element = VStack::new().align(Align::Center).child(wrapped).into();
            LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
            let rect = tree.node(find_by_key(&tree, "wrapped")).rect;
            assert_eq!(
                rect.w, 30,
                "unconstrained {name} must not lower child's min_width(30) to max_width(20)"
            );
        }

        // Unconstrained DropTarget around a shrinkable Text must not inject measured min_width.
        let mut tree = NodeTree::new();
        let root: Element = VStack::new()
            .child(
                Element::from(DropTarget::new().child(Text::new("0123456789abcdef")))
                    .key("shrinkable"),
            )
            .into();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 8,
                h: 4,
            },
            None,
        );
        assert_eq!(tree.node(find_by_key(&tree, "shrinkable")).rect.w, 8);
    }

    #[test]
    fn transparent_wrapper_preserves_outer_minimums_and_nested_constraints() {
        use crate::style::{Align, Theme};
        use crate::widgets::{DropTarget, Text, ThemeProvider, VStack};

        // 1. Outer min_width / min_height on wrappers are honored.
        let mut tree = NodeTree::new();
        let outer_min = Element::from(
            DropTarget::new().child(Text::new("hi").width(Length::Auto).height(Length::Auto)),
        )
        .min_width(Length::Px(30))
        .min_height(Length::Px(5))
        .key("outer-min");
        let root: Element = VStack::new().align(Align::Center).child(outer_min).into();
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
        let rect = tree.node(find_by_key(&tree, "outer-min")).rect;
        assert_eq!((rect.w, rect.h), (30, 5));

        // 2. Mixed outer min_width(Percent(40)) + inner min_width(Px(30)):
        //    - at 100 cols => max(40, 30) = 40
        //    - at 50 cols  => max(20, 30) = 30
        let make_mixed_min = || -> Element {
            let inner = Element::from(Text::new("hi").width(Length::Auto).height(Length::Px(1)))
                .key("mixed-min")
                .min_width(Length::Px(30));
            let wrapped = Element::from(ThemeProvider::new(Theme::default()).child(inner))
                .min_width(Length::Percent(40));
            VStack::new().align(Align::Center).child(wrapped).into()
        };

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &make_mixed_min(),
            Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 10,
            },
            None,
        );
        assert_eq!(tree.node(find_by_key(&tree, "mixed-min")).rect.w, 40);

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &make_mixed_min(),
            Rect {
                x: 0,
                y: 0,
                w: 50,
                h: 10,
            },
            None,
        );
        assert_eq!(tree.node(find_by_key(&tree, "mixed-min")).rect.w, 30);

        // 3. Multi-level nested wrappers:
        //    ThemeProvider(max 50%) -> DropTarget(min 15) -> VStack(max 25):
        //    - at 100 cols => max(15, min(50, 25)) = 25
        //    - at 40 cols  => max(15, min(20, 25)) = 20
        //    - at 20 cols  => max(15, min(10, 25)) = 15 (min beats max)
        let make_nested = || -> Element {
            let inner = Element::from(
                VStack::new()
                    .height(Length::Px(1))
                    .child(Text::new("nested")),
            )
            .key("nested")
            .max_width(Length::Px(25));
            let mid = Element::from(DropTarget::new().child(inner)).min_width(Length::Px(15));
            let outer = Element::from(ThemeProvider::new(Theme::default()).child(mid))
                .max_width(Length::Percent(50));
            VStack::new().align(Align::Center).child(outer).into()
        };

        for (avail_w, expected_w) in [(100, 25), (40, 20), (20, 15)] {
            let mut tree = NodeTree::new();
            LayoutEngine::reconcile_with_focus(
                &mut tree,
                &make_nested(),
                Rect {
                    x: 0,
                    y: 0,
                    w: avail_w,
                    h: 10,
                },
                None,
            );
            assert_eq!(
                tree.node(find_by_key(&tree, "nested")).rect.w,
                expected_w,
                "nested constraints at width {avail_w}"
            );
        }
    }

    #[test]
    fn drag_source_preserves_percentage_basis_and_outer_min_propagates_into_measurement() {
        use crate::style::{Align, Theme};
        use crate::widgets::{DragSource, Overflow, Text, ThemeProvider, VStack};

        // 1. DragSource with inner max_width(Percent(50)) and outer max_width(Px(60)) in a
        //    100-col parent must resolve 50% against 100 (= 50 cols, 2 rows), not against 60
        //    (= 30 cols, 3 rows), and at 150 cols must cap at 60 cols, 2 rows.
        let make_drag_source = || -> Element {
            let child = Element::from(
                Text::new("a".repeat(80))
                    .overflow(Overflow::Wrap)
                    .height(Length::Auto),
            )
            .max_width(Length::Percent(50));
            let wrapped = Element::from(DragSource::new().child(child))
                .max_width(Length::Px(60))
                .key("drag-mixed");
            VStack::new().align(Align::Center).child(wrapped).into()
        };

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &make_drag_source(),
            Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 10,
            },
            None,
        );
        let rect_100 = tree.node(find_by_key(&tree, "drag-mixed")).rect;
        assert_eq!(
            (rect_100.w, rect_100.h),
            (50, 2),
            "DragSource must resolve inner 50% against 100-col parent (50x2), not 60-col outer cap (30x3)"
        );

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &make_drag_source(),
            Rect {
                x: 0,
                y: 0,
                w: 150,
                h: 10,
            },
            None,
        );
        let rect_150 = tree.node(find_by_key(&tree, "drag-mixed")).rect;
        assert_eq!((rect_150.w, rect_150.h), (60, 2));

        // 2. Outer min_width(Px(50)) overriding inner max_width(Px(20)) must also govern
        //    measurement: 80-char wrapping text wraps at 50 cols (2 rows), not 20 cols (4 rows),
        //    and the sibling below starts at y = 2.
        let mut tree = NodeTree::new();
        let inner = Element::from(
            Text::new("a".repeat(80))
                .overflow(Overflow::Wrap)
                .height(Length::Auto),
        )
        .key("min-beats-max-text")
        .max_width(Length::Px(20));
        let wrapped = Element::from(ThemeProvider::new(Theme::default()).child(inner))
            .min_width(Length::Px(50));
        let root: Element = VStack::new()
            .child(wrapped)
            .child(Text::new("after").height(Length::Px(1)).key("after"))
            .into();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 10,
            },
            None,
        );
        let text_rect = tree.node(find_by_key(&tree, "min-beats-max-text")).rect;
        let after_rect = tree.node(find_by_key(&tree, "after")).rect;
        assert_eq!(
            (text_rect.w, text_rect.h),
            (50, 2),
            "outer min_width(50) must propagate into child measurement so 80 chars wrap into 2 rows, not 4"
        );
        assert_eq!(
            after_rect.y, 2,
            "sibling after wrapped text must sit directly below row 2"
        );
    }
}
