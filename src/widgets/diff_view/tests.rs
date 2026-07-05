use super::*;
use crate::core::element::{ElementKind, IntoElement};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::LayoutEngine;
use crate::layout::measure::min_size_constrained;
use crate::style::{DiffPalette, Length, Rect};
use crate::widgets::{Frame, Text, VStack};

const SCROLL_TEST_BEFORE: &str = r#"fn greet(name: &str) -> String {
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

const SCROLL_TEST_AFTER: &str = r#"fn greet(name: &str) -> String {
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

fn outer_content(element: Element) -> Element {
    let ElementKind::Frame(frame) = element.kind else {
        panic!("expected outer frame");
    };
    let Some(child) = frame.child else {
        panic!("expected outer frame child");
    };
    *child
}

fn pane_child_kind(element: Element) -> ElementKind {
    let ElementKind::Frame(frame) = element.kind else {
        panic!("expected frame-wrapped pane");
    };
    let Some(child) = frame.child else {
        panic!("expected pane frame child");
    };
    child.kind
}

fn split_leaf_visual_counts_from_tree(tree: &NodeTree) -> (usize, usize) {
    let content_id = tree.node(tree.root).children[0];
    let left_pane_id = tree.node(content_id).children[0];
    let right_pane_id = tree.node(content_id).children[1];
    let left_leaf_id = tree.node(left_pane_id).children[0];
    let right_leaf_id = tree.node(right_pane_id).children[0];

    let left_count = match &tree.node(left_leaf_id).kind {
        NodeKind::TextArea(area) => area.visual_lines_count,
        NodeKind::DocumentView(doc) => doc.total_visual_lines,
        _ => panic!("expected split pane leaf to be TextArea or DocumentView"),
    };
    let right_count = match &tree.node(right_leaf_id).kind {
        NodeKind::TextArea(area) => area.visual_lines_count,
        NodeKind::DocumentView(doc) => doc.total_visual_lines,
        _ => panic!("expected split pane leaf to be TextArea or DocumentView"),
    };

    (left_count, right_count)
}

fn split_leaf_rects_and_counts_from_tree(
    tree: &NodeTree,
    diff_root_id: NodeId,
) -> ((Rect, usize), (Rect, usize)) {
    let content_id = tree.node(diff_root_id).children[0];
    let left_pane_id = tree.node(content_id).children[0];
    let right_pane_id = tree.node(content_id).children[1];
    let left_leaf_id = tree.node(left_pane_id).children[0];
    let right_leaf_id = tree.node(right_pane_id).children[0];

    let left = match &tree.node(left_leaf_id).kind {
        NodeKind::TextArea(area) => (tree.node(left_leaf_id).rect, area.visual_lines_count),
        NodeKind::DocumentView(doc) => (tree.node(left_leaf_id).rect, doc.total_visual_lines),
        _ => panic!("expected split pane leaf to be TextArea or DocumentView"),
    };
    let right = match &tree.node(right_leaf_id).kind {
        NodeKind::TextArea(area) => (tree.node(right_leaf_id).rect, area.visual_lines_count),
        NodeKind::DocumentView(doc) => (tree.node(right_leaf_id).rect, doc.total_visual_lines),
        _ => panic!("expected split pane leaf to be TextArea or DocumentView"),
    };

    (left, right)
}

fn find_by_key(tree: &NodeTree, key: &str) -> Option<NodeId> {
    fn walk(tree: &NodeTree, node_id: NodeId, key: &str) -> Option<NodeId> {
        let node = tree.node(node_id);
        if node.key.as_ref().map(AsRef::as_ref) == Some(key) {
            return Some(node_id);
        }
        for child_id in &node.children {
            if let Some(found) = walk(tree, *child_id, key) {
                return Some(found);
            }
        }
        None
    }

    walk(tree, tree.root, key)
}

fn split_pane_rects_from_diff_root(tree: &NodeTree, diff_root_id: NodeId) -> (Rect, Rect, Rect) {
    let content_id = tree.node(diff_root_id).children[0];
    let left_pane_id = tree.node(content_id).children[0];
    let right_pane_id = tree.node(content_id).children[1];
    (
        tree.node(diff_root_id).rect,
        tree.node(left_pane_id).rect,
        tree.node(right_pane_id).rect,
    )
}

fn split_leaf_visual_counts(view: DiffView, bounds_w: u16, bounds_h: u16) -> (usize, usize) {
    let root: Element = view.into();
    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: bounds_w,
            h: bounds_h,
        },
        None,
    );

    split_leaf_visual_counts_from_tree(&tree)
}

#[test]
fn split_diff_aligns_lines() {
    let data = DiffData::with_config(
        "line1\nold\nline3",
        "line1\nnew\nline3\nline4",
        DiffDataConfig::default(),
    );

    assert_eq!(data.left.lines.len(), data.right.lines.len());
}

#[test]
fn split_wrapped_lines_are_padded_to_match_peer() {
    let before = "012345678901234567890123456789\n";
    let after = "short\n";

    for backend in [DiffViewBackend::TextArea, DiffViewBackend::DocumentView] {
        let view = DiffView::with_content(before, after)
            .mode(DiffViewMode::Split)
            .backend(backend)
            .wrap(true)
            .line_numbers(false)
            .border(false)
            .panels_border(false)
            .scrollbar(false)
            .h_scrollbar(false);

        let (left, right) = split_leaf_visual_counts(view, 24, 12);
        assert!(left > 1, "left side should wrap into multiple visual lines");
        assert_eq!(left, right, "split panes should stay wrap-aligned");
    }
}

#[test]
fn split_wrapped_context_lines_stay_aligned() {
    let before = "context-aaaaaaaaaaaaaaaa\nthis line is much much longer than peer\ncontext-bbbbbbbbbbbbbbbb\n";
    let after = "context-aaaaaaaaaaaaaaaa\nshort\ncontext-bbbbbbbbbbbbbbbb\n";

    for backend in [DiffViewBackend::TextArea, DiffViewBackend::DocumentView] {
        let view = DiffView::with_content(before, after)
            .mode(DiffViewMode::Split)
            .backend(backend)
            .wrap(true)
            .line_numbers(false)
            .border(false)
            .panels_border(false)
            .scrollbar(false)
            .h_scrollbar(false);

        let (left, right) = split_leaf_visual_counts(view, 28, 20);
        assert_eq!(left, right, "context + changed rows should remain aligned");
        assert!(
            left >= 5,
            "expected wrapped context rows to contribute visual height"
        );
    }
}

#[test]
fn split_wrapped_padding_recomputes_after_width_increase() {
    let before = "012345678901234567890123456789";
    let after = "short";

    for backend in [DiffViewBackend::TextArea, DiffViewBackend::DocumentView] {
        let view = DiffView::with_content(before, after)
            .mode(DiffViewMode::Split)
            .backend(backend)
            .wrap(true)
            .line_numbers(false)
            .border(false)
            .panels_border(false)
            .scrollbar(false)
            .h_scrollbar(false);

        let root: Element = view.into();
        let mut tree = NodeTree::new();

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 24,
                h: 12,
            },
            None,
        );

        let (narrow_left, narrow_right) = split_leaf_visual_counts_from_tree(&tree);
        assert!(narrow_left > 1, "left side should wrap at narrow width");
        assert_eq!(
            narrow_left, narrow_right,
            "narrow panes should stay aligned"
        );

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 12,
            },
            None,
        );

        let (wide_left, wide_right) = split_leaf_visual_counts_from_tree(&tree);
        assert_eq!(wide_left, 1, "left side should stop wrapping when widened");
        assert_eq!(wide_right, 1, "padding rows should be dropped when widened");
    }
}

#[test]
fn split_wrapped_session_repro_leaf_rects_match_visual_counts() {
    let before = "fn render_assistant_part(part: &Part) -> Option<Element> {\n    match part {\n        Part::Text(text) => render_markdown(text.text.trim()),\n        Part::Tool(tool) => render_tool(tool),\n        _ => None,\n    }\n}\n";
    let after = "fn render_assistant_part(part: &Part) -> Option<Element> {\n    match part {\n        Part::Text(text) => render_markdown(text.text.trim()),\n        Part::Reasoning(reasoning) => render_reasoning(reasoning),\n        Part::Tool(tool) => render_tool(tool),\n        Part::Subtask(task) => render_subtask(task),\n        Part::Agent(agent) => render_agent(agent),\n        Part::Retry(retry) => render_retry(retry),\n        _ => None,\n    }\n}\n";

    for backend in [DiffViewBackend::DocumentView, DiffViewBackend::TextArea] {
        let root: Element = Frame::new()
            .border(false)
            .height(Length::Auto)
            .padding((1, 1, 1, 3))
            .child(
                VStack::new()
                    .height(Length::Auto)
                    .gap(1)
                    .child(Text::new("Update src/screens/session.rs"))
                    .child(
                        DiffView::with_content(before, after)
                            .backend(backend)
                            .mode(DiffViewMode::Split)
                            .wrap(true)
                            .line_numbers(true)
                            .gutter_inset(1)
                            .scrollbar(false)
                            .h_scrollbar(false)
                            .border(false)
                            .panels_border(false)
                            .key("diff"),
                    ),
            )
            .into();

        let mut tree = NodeTree::new();
        for width in [229u16, 231, 233, 235, 233, 231, 229] {
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
            let (left, right) = split_leaf_rects_and_counts_from_tree(&tree, diff_id);

            assert_eq!(
                left.0.h as usize, left.1,
                "left leaf rect should match visual lines at width {width} ({backend:?})"
            );
            assert_eq!(
                right.0.h as usize, right.1,
                "right leaf rect should match visual lines at width {width} ({backend:?})"
            );
            assert_eq!(
                left.1, right.1,
                "split leaves should stay aligned at width {width} ({backend:?})"
            );

            let (diff_rect, left_rect, right_rect) =
                split_pane_rects_from_diff_root(&tree, diff_id);
            assert_eq!(
                left_rect.w.saturating_add(right_rect.w),
                diff_rect.w,
                "pane widths should fill diff ({backend:?}, w={width})"
            );
            assert_eq!(
                right_rect.w,
                left_rect.w.saturating_add(1),
                "odd total width should give right pane +1 col ({backend:?}, w={width})"
            );
        }
    }
}

#[test]
fn word_diff_marks_changes() {
    let (left, right) = word_diff_ranges("hello world", "hello rust");
    assert!(!left.is_empty());
    assert!(!right.is_empty());
}

#[test]
fn default_backend_is_text_area() {
    assert_eq!(
        DiffView::with_content("a", "b").backend,
        DiffViewBackend::TextArea
    );
}

#[test]
fn backend_is_inferred_from_document_view_config_when_not_explicit() {
    let view = DiffView::with_content("a", "b").document_view(DocumentView::new(""));
    assert_eq!(view.backend, DiffViewBackend::DocumentView);
}

#[test]
fn gutter_inset_persists_when_text_area_is_set_afterward() {
    let view = DiffView::with_content("a", "b")
        .gutter_inset(3)
        .text_area(TextArea::new(""));

    assert_eq!(view.text_area.gutter_gap, 3);
}

#[test]
fn gutter_inset_persists_when_document_view_is_set_afterward() {
    let view = DiffView::with_content("a", "b")
        .gutter_inset(5)
        .document_view(DocumentView::new(""));

    assert_eq!(view.document_view.gutter_gap, 5);
}

#[test]
fn explicit_backend_is_not_overridden_by_config_helpers() {
    let view = DiffView::with_content("a", "b")
        .backend(DiffViewBackend::TextArea)
        .document_view(DocumentView::new(""));
    assert_eq!(view.backend, DiffViewBackend::TextArea);
}

#[test]
fn editable_enables_text_area_editing() {
    let element: Element = DiffView::with_content("left", "right")
        .mode(DiffViewMode::Unified)
        .editable(true)
        .into();

    let content = outer_content(element);
    let ElementKind::TextArea(area) = pane_child_kind(content) else {
        panic!("expected unified diff to render TextArea backend by default");
    };
    assert!(!area.read_only);
}

#[test]
fn unified_document_backend_renders_document_view() {
    let element: Element = DiffView::with_content("left", "right")
        .mode(DiffViewMode::Unified)
        .backend(DiffViewBackend::DocumentView)
        .into();

    assert!(matches!(
        pane_child_kind(outer_content(element)),
        ElementKind::DocumentView(_)
    ));
}

#[test]
fn split_document_backend_renders_two_document_views() {
    let element: Element = DiffView::with_content("left", "right")
        .mode(DiffViewMode::Split)
        .backend(DiffViewBackend::DocumentView)
        .into();

    let content = outer_content(element);
    let ElementKind::HStack(stack) = content.kind else {
        panic!("expected split diff to render HStack");
    };

    assert_eq!(stack.children.len(), 2);
    assert!(matches!(
        pane_child_kind(stack.children[0].clone()),
        ElementKind::DocumentView(_)
    ));
    assert!(matches!(
        pane_child_kind(stack.children[1].clone()),
        ElementKind::DocumentView(_)
    ));
}

#[test]
fn controlled_scroll_offset_applies_to_rendered_backend() {
    let text_element: Element = DiffView::with_content("left", "right")
        .mode(DiffViewMode::Unified)
        .scroll_offset(7)
        .into();
    let ElementKind::TextArea(area) = pane_child_kind(outer_content(text_element)) else {
        panic!("expected TextArea backend");
    };
    assert_eq!(area.scroll_offset, Some(7));

    let doc_element: Element = DiffView::with_content("left", "right")
        .mode(DiffViewMode::Unified)
        .backend(DiffViewBackend::DocumentView)
        .scroll_offset(7)
        .into();
    let ElementKind::DocumentView(doc) = pane_child_kind(outer_content(doc_element)) else {
        panic!("expected DocumentView backend");
    };
    assert_eq!(doc.scroll_offset, Some(7));
}

#[test]
fn patch_diff_data_exposes_hunk_anchors() {
    let patch = concat!(
        "diff --git a/x.rs b/x.rs\n",
        "--- a/x.rs\n",
        "+++ b/x.rs\n",
        "@@ -1,2 +1,2 @@\n",
        " a\n",
        "-old\n",
        "+new\n",
        "@@ -10,1 +10,1 @@\n",
        "-before\n",
        "+after\n",
    );
    let data = DiffData::from_patch(patch);

    let unified = data.hunk_anchors(DiffViewMode::Unified);
    assert_eq!(unified.len(), 2);
    assert_eq!(unified[0].pane, DiffPane::Unified);
    assert_eq!(unified[0].index, 0);
    assert_eq!(unified[0].old_start, Some(1));
    assert_eq!(unified[0].new_start, Some(1));
    assert_eq!(unified[0].logical_line, 1);
    assert_eq!(unified[1].index, 1);
    assert!(unified[1].logical_line > unified[0].logical_line);

    let split = data.hunk_anchors(DiffViewMode::Split);
    assert_eq!(split[0].pane, DiffPane::Left);
    assert_eq!(split[0].logical_line, unified[0].logical_line);

    let right = data.hunk_anchors_for_pane(DiffPane::Right);
    assert_eq!(right[0].pane, DiffPane::Right);
    assert_eq!(right[0].new_start, Some(1));
}

#[test]
fn before_after_diff_data_has_no_patch_hunk_anchors() {
    let data = DiffData::new("old\n", "new\n");
    assert!(data.hunk_anchors(DiffViewMode::Unified).is_empty());
    assert!(data.hunk_anchors(DiffViewMode::Split).is_empty());
}

#[test]
fn diff_view_scroll_to_hunk_targets_visible_collapsed_row() {
    let patch = concat!(
        "@@ -1,5 +1,5 @@\n",
        " keep1\n",
        " keep2\n",
        "-old\n",
        "+new\n",
        " keep3\n",
    );

    let element: Element = DiffView::from_patch(patch)
        .mode(DiffViewMode::Unified)
        .context_lines(0)
        .scroll_to_hunk(0)
        .into();
    let ElementKind::TextArea(area) = pane_child_kind(outer_content(element)) else {
        panic!("expected textarea backend");
    };

    assert_eq!(area.scroll_to_line, Some(1));
    assert!(
        area.value
            .lines()
            .next()
            .is_some_and(|line| line.contains("hidden"))
    );
}

#[test]
fn document_diff_view_scroll_to_hunk_sets_source_line_target() {
    let patch = concat!(
        "@@ -1,1 +1,1 @@\n",
        "-old\n",
        "+new\n",
        "@@ -8,1 +8,1 @@\n",
        "-before\n",
        "+after\n",
    );

    let element: Element = DiffView::from_patch(patch)
        .mode(DiffViewMode::Unified)
        .backend(DiffViewBackend::DocumentView)
        .scroll_to_hunk(1)
        .into();
    let ElementKind::DocumentView(doc) = pane_child_kind(outer_content(element)) else {
        panic!("expected document backend");
    };

    assert_eq!(doc.scroll_to_source_line, Some(2));
}

#[test]
fn patch_headers_stay_in_scrollable_pane_content() {
    let patch = concat!(
        "diff --git a/src/lib.rs b/src/lib.rs\n",
        "--- a/src/lib.rs\n",
        "+++ b/src/lib.rs\n",
    );

    let text_element: Element = DiffView::from_patch(patch)
        .mode(DiffViewMode::Unified)
        .into();
    let ElementKind::TextArea(area) = pane_child_kind(outer_content(text_element)) else {
        panic!("expected TextArea backend");
    };
    assert!(
        area.value
            .starts_with("diff --git a/src/lib.rs b/src/lib.rs")
    );

    let doc_element: Element = DiffView::from_patch(patch)
        .mode(DiffViewMode::Unified)
        .backend(DiffViewBackend::DocumentView)
        .into();
    let ElementKind::DocumentView(doc) = pane_child_kind(outer_content(doc_element)) else {
        panic!("expected DocumentView backend");
    };
    assert!(
        doc.value
            .starts_with("diff --git a/src/lib.rs b/src/lib.rs")
    );
}

#[test]
fn wrap_builder_applies_to_both_backends() {
    let view = DiffView::with_content("a", "b").wrap(true);
    assert!(view.text_area.wrap);
    assert!(view.document_view.wrap);
}

#[test]
fn common_indent_trim_is_enabled_by_default() {
    let element: Element = DiffView::with_content("    one", "    two")
        .mode(DiffViewMode::Unified)
        .into();

    let ElementKind::TextArea(area) = pane_child_kind(outer_content(element)) else {
        panic!("expected TextArea backend");
    };
    assert_eq!(area.value.as_ref(), "one\ntwo");
}

#[test]
fn common_indent_trim_can_be_disabled() {
    let element: Element = DiffView::with_content("    left", "    right")
        .mode(DiffViewMode::Unified)
        .backend(DiffViewBackend::DocumentView)
        .trim_common_indent(false)
        .into();

    let ElementKind::DocumentView(doc) = pane_child_kind(outer_content(element)) else {
        panic!("expected DocumentView backend");
    };
    assert_eq!(doc.value.as_ref(), "    left\n    right");
}

#[test]
fn min_line_number_width_applies_to_both_backends() {
    let view = DiffView::with_content("a", "b").min_line_number_width(4);
    assert_eq!(view.min_line_number_width_override, Some(4));
}

#[test]
fn explicit_auto_height_propagates_to_outer_frame() {
    let element: Element = DiffView::with_content("a\nold\nline", "a\nnew\nline")
        .mode(DiffViewMode::Unified)
        .height(Length::Auto)
        .into();

    let ElementKind::Frame(frame) = &element.kind else {
        panic!("expected outer frame");
    };
    assert_eq!(frame.props.height, Length::Auto);

    let stack: Element = VStack::new().height(Length::Auto).child(element).into();
    let (_, h) = min_size_constrained(&stack, Some(80), None);
    assert!(h > 3, "expected diff content to contribute height, got {h}");
}

#[test]
fn backend_auto_height_is_inherited_by_outer_frame() {
    let element: Element = DiffView::with_content("a\nold\nline", "a\nnew\nline")
        .mode(DiffViewMode::Unified)
        .backend(DiffViewBackend::DocumentView)
        .document_view(DocumentView::new("").height(Length::Auto))
        .into();

    let ElementKind::Frame(frame) = &element.kind else {
        panic!("expected outer frame");
    };
    assert_eq!(frame.props.height, Length::Auto);
}

#[test]
fn wrapped_non_scrollable_document_diff_implicitly_uses_auto_height() {
    let element: Element = DiffView::with_content("123456789012", "123456789012")
        .mode(DiffViewMode::Unified)
        .backend(DiffViewBackend::DocumentView)
        .wrap(true)
        .scrollbar(false)
        .line_numbers(false)
        .border(false)
        .panels_border(false)
        .into();

    let ElementKind::Frame(frame) = &element.kind else {
        panic!("expected outer frame");
    };
    assert_eq!(frame.props.height, Length::Auto);

    let Some(inner) = frame.child.as_deref() else {
        panic!("expected diff child");
    };
    let ElementKind::Frame(pane) = &inner.kind else {
        panic!("expected pane frame");
    };
    assert_eq!(pane.props.height, Length::Auto);
    let Some(doc_el) = pane.child.as_deref() else {
        panic!("expected document view child");
    };
    let ElementKind::DocumentView(doc) = &doc_el.kind else {
        panic!("expected document view");
    };
    assert_eq!(doc.height, Length::Auto);
}

#[test]
fn wrapped_non_scrollable_text_diff_implicitly_uses_auto_height() {
    let element: Element = DiffView::with_content("123456789012", "123456789012")
        .mode(DiffViewMode::Unified)
        .backend(DiffViewBackend::TextArea)
        .wrap(true)
        .scrollbar(false)
        .line_numbers(false)
        .border(false)
        .panels_border(false)
        .into();

    let ElementKind::Frame(frame) = &element.kind else {
        panic!("expected outer frame");
    };
    assert_eq!(frame.props.height, Length::Auto);

    let Some(inner) = frame.child.as_deref() else {
        panic!("expected diff child");
    };
    let ElementKind::Frame(pane) = &inner.kind else {
        panic!("expected pane frame");
    };
    assert_eq!(pane.props.height, Length::Auto);
    let Some(area_el) = pane.child.as_deref() else {
        panic!("expected text area child");
    };
    let ElementKind::TextArea(area) = &area_el.kind else {
        panic!("expected text area");
    };
    assert_eq!(area.height, Length::Auto);
}

#[test]
fn wrapped_non_scrollable_split_document_diff_uses_auto_height_for_pane_frames() {
    let element: Element = DiffView::with_content("123456789012", "123456789012")
        .mode(DiffViewMode::Split)
        .backend(DiffViewBackend::DocumentView)
        .wrap(true)
        .scrollbar(false)
        .line_numbers(false)
        .border(false)
        .panels_border(false)
        .into();

    let ElementKind::Frame(frame) = &element.kind else {
        panic!("expected outer frame");
    };
    let Some(content) = frame.child.as_deref() else {
        panic!("expected split content");
    };
    let ElementKind::HStack(stack) = &content.kind else {
        panic!("expected split hstack");
    };

    assert_eq!(stack.children.len(), 2);
    for pane_el in &stack.children {
        let ElementKind::Frame(pane) = &pane_el.kind else {
            panic!("expected pane frame");
        };
        assert_eq!(pane.props.height, Length::Auto);
    }
}

#[test]
fn split_auto_height_tracks_pane_width_for_horizontal_scrollbars() {
    let root: Element = VStack::new()
        .width(Length::Auto)
        .height(Length::Auto)
        .child(
            DiffView::with_content("123456789012\nabc", "123456789012\nxyz")
                .mode(DiffViewMode::Split)
                .backend(DiffViewBackend::DocumentView)
                .height(Length::Auto)
                .border(false)
                .document_view(
                    DocumentView::new("")
                        .height(Length::Auto)
                        .wrap(false)
                        .scrollbar(false)
                        .h_scrollbar(true)
                        .border(false),
                ),
        )
        .into();

    let mut tree = NodeTree::new();
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

    let diff_id = tree.node(tree.root).children[0];
    assert_eq!(tree.node(diff_id).rect.h, 5);
}

#[test]
fn split_auto_height_with_horizontal_scrollbar_allocates_scrollbar_row() {
    let root: Element = VStack::new()
        .width(Length::Auto)
        .height(Length::Auto)
        .child(
            DiffView::with_content("123456789012\nabc", "123456789012\nxyz")
                .mode(DiffViewMode::Split)
                .backend(DiffViewBackend::DocumentView)
                .height(Length::Auto)
                .wrap(false)
                .border(false)
                .document_view(
                    DocumentView::new("")
                        .height(Length::Auto)
                        .wrap(false)
                        .scrollbar(false)
                        .h_scrollbar(true)
                        .border(false),
                ),
        )
        .into();

    let mut tree = NodeTree::new();
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

    let diff_id = tree.node(tree.root).children[0];
    assert_eq!(tree.node(diff_id).rect.h, 5);
}

#[test]
fn split_auto_height_tracks_pane_width_for_wrapping_inside_auto_frame() {
    let root: Element = VStack::new()
        .width(Length::Auto)
        .height(Length::Auto)
        .child(
            Frame::new().width(Length::Auto).height(Length::Auto).child(
                DiffView::with_content("123456789012", "123456789012")
                    .mode(DiffViewMode::Split)
                    .backend(DiffViewBackend::DocumentView)
                    .height(Length::Auto)
                    .wrap(true)
                    .line_numbers(false)
                    .border(false)
                    .panels_border(false)
                    .document_view(
                        DocumentView::new("")
                            .height(Length::Auto)
                            .line_numbers(false)
                            .wrap(true)
                            .scrollbar(false)
                            .h_scrollbar(false)
                            .border(false),
                    ),
            ),
        )
        .into();

    let mut tree = NodeTree::new();
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

    let outer_frame_id = tree.node(tree.root).children[0];
    let diff_id = tree.node(outer_frame_id).children[0];
    assert_eq!(tree.node(diff_id).rect.h, 2);
    assert_eq!(tree.node(outer_frame_id).rect.h, 4);
}

#[test]
fn wrapped_non_scrollable_split_diff_grows_inside_auto_frame_without_explicit_height() {
    let root: Element = VStack::new()
        .width(Length::Auto)
        .height(Length::Auto)
        .child(
            Frame::new().width(Length::Auto).height(Length::Auto).child(
                DiffView::with_content("123456789012", "123456789012")
                    .mode(DiffViewMode::Split)
                    .backend(DiffViewBackend::DocumentView)
                    .wrap(true)
                    .scrollbar(false)
                    .line_numbers(false)
                    .border(false)
                    .panels_border(false),
            ),
        )
        .into();

    let mut tree = NodeTree::new();
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

    let outer_frame_id = tree.node(tree.root).children[0];
    let diff_id = tree.node(outer_frame_id).children[0];
    assert_eq!(tree.node(diff_id).rect.h, 2);
    assert_eq!(tree.node(outer_frame_id).rect.h, 4);
}

#[test]
fn split_text_area_auto_height_tracks_pane_width_for_wrapping_inside_auto_frame() {
    let root: Element = VStack::new()
        .width(Length::Auto)
        .height(Length::Auto)
        .child(
            Frame::new().width(Length::Auto).height(Length::Auto).child(
                DiffView::with_content("123456789012", "123456789012")
                    .mode(DiffViewMode::Split)
                    .backend(DiffViewBackend::TextArea)
                    .height(Length::Auto)
                    .wrap(true)
                    .line_numbers(false)
                    .border(false)
                    .panels_border(false)
                    .text_area(
                        TextArea::new("")
                            .height(Length::Auto)
                            .line_numbers(false)
                            .wrap(true)
                            .scrollbar(false)
                            .h_scrollbar(false)
                            .border(false)
                            .read_only(true),
                    ),
            ),
        )
        .into();

    let mut tree = NodeTree::new();
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

    let outer_frame_id = tree.node(tree.root).children[0];
    let diff_id = tree.node(outer_frame_id).children[0];
    assert_eq!(tree.node(diff_id).rect.h, 2);
    assert_eq!(tree.node(outer_frame_id).rect.h, 4);
}

#[test]
fn split_wrapped_diff_height_matches_final_width_inside_auto_hstack_frame() {
    let diff: Element =
        DiffView::with_content("12345678901234567890\nshort", "12345678901234567890\nshort")
            .mode(DiffViewMode::Split)
            .backend(DiffViewBackend::DocumentView)
            .height(Length::Auto)
            .wrap(true)
            .line_numbers(false)
            .border(false)
            .panels_border(true)
            .document_view(
                DocumentView::new("")
                    .height(Length::Auto)
                    .line_numbers(false)
                    .wrap(true)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .border(false),
            )
            .into();

    let root: Element = HStack::new()
        .width(Length::Auto)
        .height(Length::Auto)
        .child(
            Frame::new()
                .width(Length::Auto)
                .height(Length::Auto)
                .child(diff.clone()),
        )
        .child(Text::new("tail"))
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 30,
            h: 20,
        },
        None,
    );

    let outer_frame_id = tree.node(tree.root).children[0];
    let diff_id = tree.node(outer_frame_id).children[0];
    let actual = tree.node(diff_id).rect;
    let expected = min_size_constrained(&diff, Some(actual.w), None);

    assert_eq!(actual.h, expected.1);
}

#[test]
fn split_diff_document_view_horizontal_metrics_respect_gutter_and_scroll_range() {
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

    let root: Element = DiffView::with_content(before, after)
        .mode(DiffViewMode::Split)
        .backend(DiffViewBackend::DocumentView)
        .border(false)
        .panels_border(true)
        .single_scrollbar(true)
        .highlight_full_width(true)
        .document_view(
            DocumentView::new("")
                .line_numbers(true)
                .wrap(false)
                .scrollbar(false)
                .h_scrollbar(true),
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
            h: 12,
        },
        None,
    );

    let diff_id = tree.root;
    let Some(content_id) = tree.node(diff_id).children.first().copied() else {
        panic!("missing diff content");
    };
    let NodeKind::HStack(_) = &tree.node(content_id).kind else {
        panic!("expected split hstack");
    };
    let right_pane_id = tree.node(content_id).children[1];
    let right_doc_id = tree.node(right_pane_id).children[0];
    let NodeKind::DocumentView(doc) = &tree.node(right_doc_id).kind else {
        panic!("expected document view");
    };
    let metrics = crate::app::input::scrollbar::compute_metrics(
        tree.node(right_doc_id),
        crate::core::node::ScrollbarAxis::Horizontal,
    )
    .expect("expected horizontal metrics");

    let expected_track_w = tree.node(right_doc_id).rect.w - doc.gutter_col_width - 1;
    assert_eq!(metrics.inner.w, expected_track_w);
    assert_eq!(
        metrics.core.max_offset,
        (doc.max_line_width as usize).saturating_sub(expected_track_w as usize)
    );
}

#[test]
fn split_diff_text_area_horizontal_metrics_respect_gutter_and_read_only_width() {
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

    let root: Element = DiffView::with_content(before, after)
        .mode(DiffViewMode::Split)
        .backend(DiffViewBackend::TextArea)
        .border(false)
        .panels_border(false)
        .single_scrollbar(true)
        .highlight_full_width(true)
        .text_area(
            TextArea::new("")
                .line_numbers(true)
                .min_line_number_width(3)
                .wrap(false)
                .scrollbar(true)
                .h_scrollbar(true),
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
            h: 12,
        },
        None,
    );

    let diff_id = tree.root;
    let Some(content_id) = tree.node(diff_id).children.first().copied() else {
        panic!("missing diff content");
    };
    let right_pane_id = tree.node(content_id).children[1];
    let right_area_id = tree.node(right_pane_id).children[0];
    let NodeKind::TextArea(area) = &tree.node(right_area_id).kind else {
        panic!("expected text area");
    };
    let metrics = crate::app::input::scrollbar::compute_metrics(
        tree.node(right_area_id),
        crate::core::node::ScrollbarAxis::Horizontal,
    )
    .expect("expected horizontal metrics");

    let expected_track_w = tree.node(right_area_id).rect.w - area.gutter_col_width - 1;
    assert_eq!(metrics.inner.w, expected_track_w);
    assert_eq!(
        metrics.core.max_offset,
        area.max_line_width
            .saturating_sub(expected_track_w as usize)
    );
}

#[test]
fn split_diff_document_view_horizontal_drag_reaches_max_offset() {
    let root: Element = DiffView::with_content(SCROLL_TEST_BEFORE, SCROLL_TEST_AFTER)
        .mode(DiffViewMode::Split)
        .backend(DiffViewBackend::DocumentView)
        .border(false)
        .panels_border(true)
        .single_scrollbar(true)
        .document_view(
            DocumentView::new("")
                .line_numbers(true)
                .wrap(false)
                .scrollbar(false)
                .scrollbar_config(
                    crate::style::ScrollbarConfig::new()
                        .variant(crate::style::ScrollbarVariant::Standalone),
                )
                .h_scrollbar(true),
        )
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 12,
        },
        None,
    );

    let content_id = tree.node(tree.root).children[0];
    let right_pane_id = tree.node(content_id).children[1];
    let right_doc_id = tree.node(right_pane_id).children[0];
    let metrics = crate::app::input::scrollbar::compute_metrics(
        tree.node(right_doc_id),
        crate::core::node::ScrollbarAxis::Horizontal,
    )
    .expect("expected horizontal metrics");

    let target = tree
        .scrollbar_target_at(metrics.inner.x, metrics.inner.y)
        .expect("missing scrollbar target");
    assert_eq!(target.id, right_doc_id);
    assert_eq!(target.axis, crate::core::node::ScrollbarAxis::Horizontal);

    let drag = crate::app::input::scrollbar::start_drag(
        tree.node(right_doc_id),
        crate::core::node::ScrollbarAxis::Horizontal,
        metrics.inner.x as u16,
        metrics.inner.y as u16,
    )
    .expect("failed to start drag");

    let end_x = metrics
        .inner
        .x
        .saturating_add(metrics.inner.w.saturating_sub(1) as i16) as u16;
    let handled = crate::app::input::scrollbar::handle_drag(
        tree.node_mut(right_doc_id),
        crate::core::node::ScrollbarAxis::Horizontal,
        end_x,
        metrics.inner.y as u16,
        drag.grab_offset,
        drag.grab_subcell,
    );
    assert!(handled);

    let NodeKind::DocumentView(doc) = &tree.node(right_doc_id).kind else {
        panic!("expected document view");
    };
    assert_eq!(doc.h_scroll_offset, metrics.core.max_offset);
}

#[test]
fn split_diff_document_view_example_geometry_preserves_full_h_scroll_after_reconcile() {
    let root: Element = Frame::new()
        .padding(1)
        .height(Length::Px(14))
        .child(
            DiffView::with_content(SCROLL_TEST_BEFORE, SCROLL_TEST_AFTER)
                .mode(DiffViewMode::Split)
                .backend(DiffViewBackend::DocumentView)
                .border(false)
                .panels_border(true)
                .single_scrollbar(true)
                .document_view(
                    DocumentView::new("")
                        .line_numbers(true)
                        .wrap(false)
                        .scrollbar(false)
                        .scrollbar_config(
                            crate::style::ScrollbarConfig::new()
                                .variant(crate::style::ScrollbarVariant::Standalone),
                        )
                        .h_scrollbar(true),
                ),
        )
        .into();

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 99,
        h: 14,
    };

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

    let frame_child = tree.node(tree.root).children[0];
    let content_id = tree.node(frame_child).children[0];
    let right_pane_id = tree.node(content_id).children[1];
    let right_doc_id = tree.node(right_pane_id).children[0];

    let metrics = crate::app::input::scrollbar::compute_metrics(
        tree.node(right_doc_id),
        crate::core::node::ScrollbarAxis::Horizontal,
    )
    .expect("expected horizontal metrics");
    assert!(metrics.core.max_offset > 1);

    let drag = crate::app::input::scrollbar::start_drag(
        tree.node(right_doc_id),
        crate::core::node::ScrollbarAxis::Horizontal,
        metrics.inner.x as u16,
        metrics.inner.y as u16,
    )
    .expect("failed to start drag");

    let end_x = metrics
        .inner
        .x
        .saturating_add(metrics.inner.w.saturating_sub(1) as i16) as u16;
    let handled = crate::app::input::scrollbar::handle_drag(
        tree.node_mut(right_doc_id),
        crate::core::node::ScrollbarAxis::Horizontal,
        end_x,
        metrics.inner.y as u16,
        drag.grab_offset,
        drag.grab_subcell,
    );
    assert!(handled);

    LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

    let frame_child = tree.node(tree.root).children[0];
    let content_id = tree.node(frame_child).children[0];
    let right_pane_id = tree.node(content_id).children[1];
    let right_doc_id = tree.node(right_pane_id).children[0];
    let NodeKind::DocumentView(doc) = &tree.node(right_doc_id).kind else {
        panic!("expected document view");
    };
    assert_eq!(doc.h_scroll_offset, metrics.core.max_offset);
}

#[test]
fn split_diff_document_view_example_geometry_shows_h_scrollbar_at_108_cols() {
    // With even_flex remainder distribution, the right pane gets the extra
    // pixel at odd total widths. At 108 both panes get equal width, keeping
    // the right pane narrow enough for content to overflow horizontally.
    let root: Element = Frame::new()
        .padding(1)
        .height(Length::Px(14))
        .child(
            DiffView::with_content(SCROLL_TEST_BEFORE, SCROLL_TEST_AFTER)
                .mode(DiffViewMode::Split)
                .backend(DiffViewBackend::DocumentView)
                .border(false)
                .panels_border(true)
                .single_scrollbar(true)
                .document_view(
                    DocumentView::new("")
                        .line_numbers(true)
                        .wrap(false)
                        .scrollbar(false)
                        .scrollbar_config(
                            crate::style::ScrollbarConfig::new()
                                .variant(crate::style::ScrollbarVariant::Standalone),
                        )
                        .h_scrollbar(true),
                ),
        )
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 108,
            h: 14,
        },
        None,
    );

    let frame_child = tree.node(tree.root).children[0];
    let content_id = tree.node(frame_child).children[0];
    let right_pane_id = tree.node(content_id).children[1];
    let right_doc_id = tree.node(right_pane_id).children[0];
    let metrics = crate::app::input::scrollbar::compute_metrics(
        tree.node(right_doc_id),
        crate::core::node::ScrollbarAxis::Horizontal,
    );
    assert!(metrics.is_some(), "h scrollbar metrics should be present");
}

#[test]
fn split_diff_text_view_example_geometry_shows_h_scrollbar_at_103_and_105_cols() {
    for width in [103u16, 105u16] {
        let root: Element = VStack::new()
            .child(
                Frame::new()
                    .title("DiffView - Split (TextArea backend, scroll synced)")
                    .padding(1)
                    .height(Length::Flex(1))
                    .child(
                        DiffView::with_content(SCROLL_TEST_BEFORE, SCROLL_TEST_AFTER)
                            .mode(DiffViewMode::Split)
                            .word_diff(true)
                            .border(false)
                            .panels_border(false)
                            .highlight_full_width(true)
                            .single_scrollbar(true)
                            .text_area(
                                TextArea::new("")
                                    .line_numbers(true)
                                    .min_line_number_width(3)
                                    .wrap(false)
                                    .scrollbar(true)
                                    .scrollbar_config(
                                        crate::style::ScrollbarConfig::new()
                                            .variant(crate::style::ScrollbarVariant::Standalone),
                                    )
                                    .h_scrollbar(true),
                            ),
                    ),
            )
            .child(
                Frame::new()
                    .title("DiffView - Split (DocumentView backend, scroll synced)")
                    .padding(1)
                    .height(Length::Flex(1))
                    .child(crate::widgets::Text::new("placeholder")),
            )
            .child(
                Frame::new()
                    .title("DiffView - Unified (editable TextArea backend)")
                    .padding(1)
                    .height(Length::Flex(1))
                    .child(crate::widgets::Text::new("placeholder")),
            )
            .into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: width,
                h: 41,
            },
            None,
        );

        let top_frame_id = tree.node(tree.root).children[0];
        let diff_outer_id = tree.node(top_frame_id).children[0];
        let content_id = tree.node(diff_outer_id).children[0];
        let right_pane_id = tree.node(content_id).children[1];
        let right_area_id = tree.node(right_pane_id).children[0];
        let NodeKind::TextArea(area) = &tree.node(right_area_id).kind else {
            panic!("expected text area");
        };
        let metrics = crate::app::input::scrollbar::compute_metrics(
            tree.node(right_area_id),
            crate::core::node::ScrollbarAxis::Horizontal,
        )
        .expect("expected horizontal metrics");

        assert!(area.scrollbar);
        assert!(area.h_scrollbar);
        assert!(
            metrics.core.max_offset > 0,
            "width {width} should still overflow"
        );
    }
}

#[test]
fn split_diff_text_area_horizontal_drag_reaches_max_offset() {
    let root: Element = DiffView::with_content(SCROLL_TEST_BEFORE, SCROLL_TEST_AFTER)
        .mode(DiffViewMode::Split)
        .backend(DiffViewBackend::TextArea)
        .border(false)
        .panels_border(false)
        .single_scrollbar(true)
        .text_area(
            TextArea::new("")
                .line_numbers(true)
                .min_line_number_width(3)
                .wrap(false)
                .scrollbar(true)
                .scrollbar_config(
                    crate::style::ScrollbarConfig::new()
                        .variant(crate::style::ScrollbarVariant::Standalone),
                )
                .h_scrollbar(true),
        )
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 12,
        },
        None,
    );

    let content_id = tree.node(tree.root).children[0];
    let right_pane_id = tree.node(content_id).children[1];
    let right_area_id = tree.node(right_pane_id).children[0];
    let metrics = crate::app::input::scrollbar::compute_metrics(
        tree.node(right_area_id),
        crate::core::node::ScrollbarAxis::Horizontal,
    )
    .expect("expected horizontal metrics");

    let target = tree
        .scrollbar_target_at(metrics.inner.x, metrics.inner.y)
        .expect("missing scrollbar target");
    assert_eq!(target.id, right_area_id);
    assert_eq!(target.axis, crate::core::node::ScrollbarAxis::Horizontal);

    let drag = crate::app::input::scrollbar::start_drag(
        tree.node(right_area_id),
        crate::core::node::ScrollbarAxis::Horizontal,
        metrics.inner.x as u16,
        metrics.inner.y as u16,
    )
    .expect("failed to start drag");

    let end_x = metrics
        .inner
        .x
        .saturating_add(metrics.inner.w.saturating_sub(1) as i16) as u16;
    let handled = crate::app::input::scrollbar::handle_drag(
        tree.node_mut(right_area_id),
        crate::core::node::ScrollbarAxis::Horizontal,
        end_x,
        metrics.inner.y as u16,
        drag.grab_offset,
        drag.grab_subcell,
    );
    assert!(handled);

    let NodeKind::TextArea(area) = &tree.node(right_area_id).kind else {
        panic!("expected text area");
    };
    assert_eq!(area.h_scroll_offset, metrics.core.max_offset);
}

#[test]
fn split_source_numbering_follows_original_line_indices() {
    let data = DiffData::new("a\nb\nc\n", "a\nx\nc\ny\n");

    let left = add_source_line_numbers(&data.left, DiffViewMode::Split, DiffPane::Left, true, 1);
    let right = add_source_line_numbers(&data.right, DiffViewMode::Split, DiffPane::Right, true, 1);

    assert!(left.lines[0].prefix.starts_with("1 "));
    assert!(left.lines[1].prefix.starts_with("2 "));
    assert!(left.lines[2].prefix.starts_with("3 "));
    assert!(left.lines[3].prefix.starts_with("  "));

    assert!(right.lines[0].prefix.starts_with("1 "));
    assert!(right.lines[1].prefix.starts_with("2 "));
    assert!(right.lines[2].prefix.starts_with("3 "));
    assert!(right.lines[3].prefix.starts_with("4 "));
}

#[test]
fn diffview_disables_inner_viewer_line_gutters() {
    let element: Element = DiffView::with_content("a\nb\n", "a\nc\n")
        .mode(DiffViewMode::Unified)
        .line_numbers(true)
        .into();

    let ElementKind::TextArea(area) = pane_child_kind(outer_content(element)) else {
        panic!("expected textarea backend");
    };
    assert!(!area.line_numbers);
}

#[test]
fn border_override_wins_even_when_text_area_set_afterward() {
    let element: Element = DiffView::with_content("a", "b")
        .mode(DiffViewMode::Unified)
        .border(false)
        .panels_border(false)
        .text_area(TextArea::new("").border(true))
        .into();

    let ElementKind::Frame(frame) = element.kind else {
        panic!("expected frame wrapper");
    };
    assert!(!frame.props.border);

    let Some(content) = frame.child else {
        panic!("expected outer frame child");
    };
    let ElementKind::Frame(pane) = content.kind else {
        panic!("expected pane frame");
    };
    assert!(!pane.props.border);
    let Some(inner) = pane.child else {
        panic!("expected pane child");
    };
    let ElementKind::TextArea(area) = inner.kind else {
        panic!("expected textarea child");
    };
    assert!(!area.border);
}

#[test]
fn single_scrollbar_keeps_only_right_split_scrollbar() {
    let element: Element = DiffView::with_content("a", "b")
        .mode(DiffViewMode::Split)
        .single_scrollbar(true)
        .into();

    let content = outer_content(element);
    let ElementKind::HStack(stack) = content.kind else {
        panic!("expected split HStack");
    };
    assert_eq!(stack.children.len(), 2);

    let ElementKind::TextArea(left) = pane_child_kind(stack.children[0].clone()) else {
        panic!("expected left textarea");
    };
    let ElementKind::TextArea(right) = pane_child_kind(stack.children[1].clone()) else {
        panic!("expected right textarea");
    };

    assert!(!left.scrollbar);
    assert!(right.scrollbar);
}

#[test]
fn single_scrollbar_marks_right_textarea_for_theme_thumb_seeding() {
    let focus_thumb = Style::new().fg(Color::Yellow);
    let element: Element =
        DiffView::with_content("a", "b")
            .mode(DiffViewMode::Split)
            .single_scrollbar(true)
            .text_area(TextArea::new("").scrollbar_config(
                crate::style::ScrollbarConfig::new().thumb_focus_style(focus_thumb),
            ))
            .into();

    let content = outer_content(element);
    let ElementKind::HStack(stack) = content.kind else {
        panic!("expected split HStack");
    };

    let ElementKind::TextArea(left) = pane_child_kind(stack.children[0].clone()) else {
        panic!("expected left textarea");
    };
    let ElementKind::TextArea(right) = pane_child_kind(stack.children[1].clone()) else {
        panic!("expected right textarea");
    };

    assert!(!left.pin_scrollbar_focus_style);
    assert!(right.pin_scrollbar_focus_style);
    assert_eq!(right.scrollbar_config.thumb_style, None);
    assert_eq!(right.scrollbar_config.thumb_focus_style, Some(focus_thumb));
}

#[test]
fn single_scrollbar_marks_right_document_view_for_theme_thumb_seeding() {
    let focus_thumb = Style::new().fg(Color::Yellow);
    let element: Element =
        DiffView::with_content("a", "b")
            .mode(DiffViewMode::Split)
            .backend(DiffViewBackend::DocumentView)
            .single_scrollbar(true)
            .document_view(DocumentView::new("").scrollbar_config(
                crate::style::ScrollbarConfig::new().thumb_focus_style(focus_thumb),
            ))
            .into();

    let content = outer_content(element);
    let ElementKind::HStack(stack) = content.kind else {
        panic!("expected split HStack");
    };

    let ElementKind::DocumentView(left) = pane_child_kind(stack.children[0].clone()) else {
        panic!("expected left document view");
    };
    let ElementKind::DocumentView(right) = pane_child_kind(stack.children[1].clone()) else {
        panic!("expected right document view");
    };

    assert!(!left.pin_scrollbar_focus_style);
    assert!(right.pin_scrollbar_focus_style);
    assert_eq!(right.scrollbar_config.thumb_style, None);
    assert_eq!(right.scrollbar_config.thumb_focus_style, Some(focus_thumb));
}

#[test]
fn split_wrap_padding_gutter_uses_context_line_number_style() {
    let empty = Style::new().bg(Color::rgb(12, 18, 24)).dim();
    let context_line_number = Style::new().fg(Color::DarkGray);
    let expected = empty.patch(context_line_number);

    let text_element: Element = DiffView::with_content("short", "much much longer")
        .mode(DiffViewMode::Split)
        .backend(DiffViewBackend::TextArea)
        .wrap(true)
        .diff_style(DiffPalette {
            empty,
            context_line_number,
            ..DiffPalette::default()
        })
        .into();
    let doc_element: Element = DiffView::with_content("short", "much much longer")
        .mode(DiffViewMode::Split)
        .backend(DiffViewBackend::DocumentView)
        .wrap(true)
        .diff_style(DiffPalette {
            empty,
            context_line_number,
            ..DiffPalette::default()
        })
        .into();

    let text_content = outer_content(text_element);
    let ElementKind::HStack(text_stack) = text_content.kind else {
        panic!("expected split HStack");
    };
    let ElementKind::TextArea(left_text) = pane_child_kind(text_stack.children[0].clone()) else {
        panic!("expected left textarea");
    };

    let doc_content = outer_content(doc_element);
    let ElementKind::HStack(doc_stack) = doc_content.kind else {
        panic!("expected split HStack");
    };
    let ElementKind::DocumentView(left_doc) = pane_child_kind(doc_stack.children[0].clone()) else {
        panic!("expected left document view");
    };

    assert_eq!(left_text.split_wrap_padding_gutter_style, Some(expected));
    assert_eq!(left_doc.split_wrap_padding_gutter_style, Some(expected));
}

#[test]
fn join_frame_is_applied_to_pane_wrappers() {
    let element: Element = DiffView::with_content("a", "b")
        .mode(DiffViewMode::Split)
        .join_frame(true)
        .into();

    let content = outer_content(element);
    let ElementKind::HStack(stack) = content.kind else {
        panic!("expected split HStack");
    };

    for pane in stack.children {
        let ElementKind::Frame(frame) = pane.kind else {
            panic!("expected frame-wrapped pane");
        };
        assert!(frame.props.join_frame);
    }
}

#[test]
fn vertical_separator_is_inserted_between_split_panes() {
    let element: Element = DiffView::with_content("a", "b")
        .mode(DiffViewMode::Split)
        .vertical_separator(true)
        .into();

    let content = outer_content(element);
    let ElementKind::HStack(stack) = content.kind else {
        panic!("expected split HStack");
    };
    assert_eq!(stack.children.len(), 3);
    assert!(matches!(stack.children[1].kind, ElementKind::Divider(_)));
}

#[test]
fn document_backend_receives_full_width_highlight_flag() {
    let element: Element = DiffView::with_content("a", "b")
        .mode(DiffViewMode::Unified)
        .backend(DiffViewBackend::DocumentView)
        .highlight_full_width(true)
        .into();

    let content = outer_content(element);
    let ElementKind::Frame(pane) = content.kind else {
        panic!("expected pane frame");
    };
    let Some(inner) = pane.child else {
        panic!("expected pane child");
    };
    let ElementKind::DocumentView(doc) = inner.kind else {
        panic!("expected document view child");
    };
    assert!(doc.highlight_full_width);
}

#[test]
fn neutral_bg_sets_context_only_not_empty() {
    let view = DiffView::with_content("a", "b").neutral_bg(Color::rgb(12, 34, 56));
    assert_eq!(
        view.diff_style.context.bg,
        Some(crate::style::Paint::Solid(Color::rgb(12, 34, 56)))
    );
    assert_eq!(view.diff_style.empty.bg, None);
}

#[test]
fn default_word_styles_are_not_bold() {
    let palette = DiffPalette::default();
    assert_ne!(palette.added_word.bold, Some(true));
    assert_ne!(palette.removed_word.bold, Some(true));
}

#[test]
fn diff_data_cache_distinguishes_same_prefix_suffix_inputs() {
    let head = "h".repeat(64);
    let tail = "t".repeat(64);
    let before = format!("{head}before-middle-0000{tail}");
    let after_one = format!("{head}after-middle-11111{tail}");
    let after_two = format!("{head}after-middle-22222{tail}");

    let _: Element = DiffView::with_content(before.clone(), after_one)
        .mode(DiffViewMode::Unified)
        .into();
    let second: Element = DiffView::with_content(before, after_two)
        .mode(DiffViewMode::Unified)
        .into();
    let ElementKind::TextArea(area) = pane_child_kind(outer_content(second)) else {
        panic!("expected textarea backend");
    };

    assert!(area.value.contains("after-middle-22222"));
    assert!(!area.value.contains("after-middle-11111"));
}

#[test]
fn diff_data_cache_distinguishes_custom_config() {
    let before = "line 0\nline 1\nold\nline 3\nline 4\n";
    let after = "line 0\nline 1\nnew\nline 3\nline 4\n";

    let _: Element = DiffView::with_content(before, after)
        .mode(DiffViewMode::Unified)
        .context_lines(0)
        .into();
    let second: Element = DiffView::with_content(before, after)
        .mode(DiffViewMode::Unified)
        .context_lines(0)
        .prefixes(DiffPrefixes::new("== ", "ADD ", "REM "))
        .context_separator_text("CUSTOM {count}")
        .into();
    let ElementKind::TextArea(area) = pane_child_kind(outer_content(second)) else {
        panic!("expected textarea backend");
    };

    let gutter = area.gutter_lines.expect("expected diff gutter");
    let gutter_text = gutter
        .iter()
        .flat_map(|line| line.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(gutter_text.contains("ADD"));
    assert!(gutter_text.contains("REM"));
    assert!(area.value.contains("CUSTOM"));
}

#[test]
fn text_area_backend_attaches_context_separator_click_config() {
    let before: String = (1..=21).map(|i| format!("line{i}\n")).collect();
    let mut after_lines: Vec<String> = (1..=21).map(|i| format!("line{i}\n")).collect();
    after_lines[10] = "CHANGED\n".to_string();
    let after: String = after_lines.concat();

    let element: Element = DiffView::with_content(before, after)
        .mode(DiffViewMode::Unified)
        .context_lines(2)
        .context_expand_lines(5)
        .context_separator_hover_style(Style::new().underline())
        .on_context_separator_click(crate::callback::Callback::new(|_| {}))
        .into();
    let ElementKind::TextArea(area) = pane_child_kind(outer_content(element)) else {
        panic!("expected textarea backend");
    };

    let config = area
        .diff_context_separator_click
        .expect("expected separator click config");
    let events: Vec<_> = config
        .events_by_source_line
        .iter()
        .filter_map(Clone::clone)
        .collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].pane, DiffPane::Unified);
    assert_eq!(events[0].hidden_lines, 8);
    assert_eq!(events[0].expand_lines, 5);
    assert_eq!(config.hover_style, Some(Style::new().underline()));
    assert!(config.on_click.is_some());
}

#[test]
fn context_separator_hover_style_works_without_click_callback() {
    let before: String = (1..=21).map(|i| format!("line{i}\n")).collect();
    let mut after_lines: Vec<String> = (1..=21).map(|i| format!("line{i}\n")).collect();
    after_lines[10] = "CHANGED\n".to_string();
    let after: String = after_lines.concat();

    let element: Element = DiffView::with_content(before, after)
        .mode(DiffViewMode::Unified)
        .context_lines(2)
        .context_separator_hover_style(Style::new().underline())
        .into();
    let ElementKind::TextArea(area) = pane_child_kind(outer_content(element)) else {
        panic!("expected textarea backend");
    };

    let config = area
        .diff_context_separator_click
        .expect("expected separator hover config");
    assert_eq!(config.hover_style, Some(Style::new().underline()));
    assert!(config.on_click.is_none());
}

#[test]
fn expanded_context_hides_matching_separator_in_diff_view() {
    let before: String = (1..=21).map(|i| format!("line{i}\n")).collect();
    let mut after_lines: Vec<String> = (1..=21).map(|i| format!("line{i}\n")).collect();
    after_lines[10] = "CHANGED\n".to_string();
    let after: String = after_lines.concat();

    let collapsed: Element = DiffView::with_content(before.as_str(), after.as_str())
        .mode(DiffViewMode::Unified)
        .context_lines(2)
        .into();
    let ElementKind::TextArea(area) = pane_child_kind(outer_content(collapsed)) else {
        panic!("expected textarea backend");
    };
    assert!(area.value.lines().any(|line| line.contains("hidden")));
    let range = DiffContextRange {
        old_start: Some(1),
        old_end: Some(8),
        new_start: Some(1),
        new_end: Some(8),
    };

    let expanded: Element = DiffView::with_content(before, after)
        .mode(DiffViewMode::Unified)
        .context_lines(2)
        .expanded_context(range)
        .into();
    let ElementKind::TextArea(area) = pane_child_kind(outer_content(expanded)) else {
        panic!("expected textarea backend");
    };

    assert!(area.value.contains("line1"));
    assert!(area.value.contains("line8"));
    assert_eq!(
        area.value
            .lines()
            .filter(|line| line.contains("hidden"))
            .count(),
        1
    );
}

#[test]
fn patch_diff_data_cache_reuses_parsed_patch_data() {
    let patch = concat!(
        "--- a/src/lib.rs\n",
        "+++ b/src/lib.rs\n",
        "@@ -1,2 +1,2 @@\n",
        " fn main() {\n",
        "-    old();\n",
        "+    new();\n",
        " }\n",
    );

    let patch = Arc::<str>::from(patch);
    let first = cached_patch_diff_data(Arc::clone(&patch), DiffDataConfig::default());
    let second = cached_patch_diff_data(Arc::clone(&patch), DiffDataConfig::default());

    assert!(Arc::ptr_eq(&first, &second));

    let context_config = DiffDataConfig {
        context_lines: Some(0),
        show_context_separator: false,
        ..DiffDataConfig::default()
    };
    let first = cached_patch_diff_data(Arc::clone(&patch), context_config.clone());
    let second = cached_patch_diff_data(patch, context_config);

    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn diff_data_caches_stay_bounded_over_a_long_session() {
    // Distinct diff content on every call (as a long session scrolling through
    // many files would produce) must not grow the caches without bound.
    let n = super::DIFF_DATA_CACHE_LIMIT * 2 + 10;
    for i in 0..n {
        let before = format!("fn f{i}() {{ old() }}\n");
        let after = format!("fn f{i}() {{ new() }}\n");
        let _ = cached_diff_data(&before, &after, DiffDataConfig::default());

        let patch = Arc::<str>::from(format!("@@ -1 +1 @@\n-old{i}\n+new{i}\n"));
        let _ = cached_patch_diff_data(patch, DiffDataConfig::default());

        let content_len = DIFF_DATA_CACHE.with(|c| c.borrow().len());
        let ptr_len = PATCH_DIFF_DATA_PTR_CACHE.with(|c| c.borrow().len());
        assert!(
            content_len <= super::DIFF_DATA_CACHE_LIMIT,
            "content cache unbounded: {content_len}"
        );
        assert!(
            ptr_len <= super::DIFF_DATA_CACHE_LIMIT,
            "ptr cache unbounded: {ptr_len}"
        );
    }
}

#[test]
fn marker_style_does_not_force_dim_foreground() {
    let palette = DiffPalette {
        added_marker: Style::new().fg(Color::Green),
        ..DiffPalette::default()
    };
    let base = Style::new().bg(Color::rgb(16, 42, 30)).dim_by(0.25);
    let marker = palette
        .marker_style(DiffLineKind::Added, base)
        .expect("expected marker style for added line");
    assert_ne!(marker.dim, Some(true));
    assert_eq!(marker.fg, Some(crate::style::Paint::Solid(Color::Green)));
}

#[test]
fn context_line_number_style_is_applied_to_number_segment() {
    let lines = vec![DiffRenderLine {
        prefix: Arc::from(" 12   "),
        text: Arc::from("context"),
        kind: DiffLineKind::Context,
        old_line: Some(12),
        new_line: Some(12),
        word_ranges: Vec::new(),
        context_separator: None,
        hunk: None,
    }];
    let palette = DiffPalette {
        context_line_number: Style::new().fg(Color::DarkGray),
        ..DiffPalette::default()
    };

    let (gutter, _) = build_diff_gutter_from_lines(&lines, palette);
    let spans = &gutter[0];

    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].content.as_ref(), " 12 ");
    assert_eq!(spans[0].style.fg, Some(Color::DarkGray.into()));
    assert_eq!(spans[1].content.as_ref(), "  ");
}

#[test]
fn border_controls_outer_only() {
    let element: Element = DiffView::with_content("a", "b")
        .mode(DiffViewMode::Unified)
        .border(false)
        .into();

    let ElementKind::Frame(outer) = element.kind else {
        panic!("expected outer frame");
    };
    assert!(!outer.props.border);
    let Some(content) = outer.child else {
        panic!("expected outer child");
    };
    let ElementKind::Frame(pane) = content.kind else {
        panic!("expected pane frame");
    };
    assert!(pane.props.border);
}

#[test]
fn panels_border_controls_pane_border() {
    let element: Element = DiffView::with_content("a", "b")
        .mode(DiffViewMode::Unified)
        .border(true)
        .panels_border(false)
        .into();

    let ElementKind::Frame(outer) = element.kind else {
        panic!("expected outer frame");
    };
    assert!(outer.props.border);
    let Some(content) = outer.child else {
        panic!("expected outer child");
    };
    let ElementKind::Frame(pane) = content.kind else {
        panic!("expected pane frame");
    };
    assert!(!pane.props.border);
}

#[test]
fn unified_numbering_uses_before_for_removed_and_after_otherwise() {
    let data = DiffData::new("a\nb\n", "a\nc\nd\n");
    let unified = add_source_line_numbers(
        &data.unified,
        DiffViewMode::Unified,
        DiffPane::Unified,
        true,
        1,
    );
    assert!(unified.lines[0].prefix.starts_with("1 "));

    let removed = unified
        .lines
        .iter()
        .find(|l| l.kind == DiffLineKind::Removed)
        .expect("expected removed line in unified diff");
    assert!(removed.prefix.starts_with("2 "));

    let added = unified
        .lines
        .iter()
        .find(|l| l.kind == DiffLineKind::Added)
        .expect("expected added line in unified diff");
    assert!(added.prefix.starts_with("2 "));
}
