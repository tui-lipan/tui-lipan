use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::app::context::SurfaceMode;
use crate::core::component::{Component, Context, Update};
use crate::core::node::NodeKind;
use crate::runtime::RuntimeCore;
use crate::style::BorderStyle;
use crate::style::{Rect, Theme};
use crate::widgets::document_view::node::{DocumentVisualLine, VisualLineKind};
use crate::widgets::{DocumentView, Text, VStack};

use super::{
    TableTsvRangeParams, document_view_cursor_from_coords, document_view_selected_text_from_node,
    table_tsv_for_range,
};

struct DocumentViewDragSmoke;

struct WrappedDocumentViewCopySmoke;

struct NewlineDocumentViewCopySmoke;

#[cfg(feature = "markdown")]
struct MermaidDocumentViewCopySmoke;

#[cfg(feature = "markdown")]
struct MarkdownChromeCopySmoke;

impl Component for DocumentViewDragSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        VStack::new()
            .child(Text::new("pad"))
            .child(DocumentView::new("zero\none\ntwo\nthree").border(false))
            .into()
    }
}

impl Component for WrappedDocumentViewCopySmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        DocumentView::new(
            "Currently, there is no timeout, so it keeps waiting until the process is ready.",
        )
        .border(false)
        .into()
    }
}

impl Component for NewlineDocumentViewCopySmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        DocumentView::new("first line\nsecond line")
            .border(false)
            .into()
    }
}

#[cfg(feature = "markdown")]
impl Component for MermaidDocumentViewCopySmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        DocumentView::new("```mermaid\nflowchart TD\nA[Start] --> B[End]\n```")
            .markdown()
            .border(false)
            .into()
    }
}

#[cfg(feature = "markdown")]
impl Component for MarkdownChromeCopySmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        DocumentView::new("> Quote line\n\n```\nhello\n```")
            .markdown()
            .border(false)
            .into()
    }
}

#[test]
fn document_view_dragging_above_selects_to_document_start() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 4,
    };
    let mut runtime = RuntimeCore::new_test(
        DocumentViewDragSmoke,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let document_view_id = runtime
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
        .map(|node| node.id)
        .expect("document view exists");
    let rect = runtime.tree.node(document_view_id).rect;

    assert_eq!(
        document_view_cursor_from_coords(
            &runtime.tree,
            rect.x.saturating_add(3) as u16,
            rect.y.saturating_sub(1).max(0) as u16,
            document_view_id,
        ),
        Some(0)
    );
}

#[test]
fn document_view_copy_ignores_soft_wrap_newlines() {
    let text = "Currently, there is no timeout, so it keeps waiting until the process is ready.";
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 6,
    };
    let mut runtime = RuntimeCore::new_test(
        WrappedDocumentViewCopySmoke,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let copied = runtime
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::DocumentView(doc) => {
                assert!(doc.visual_cache.flat_text.contains('\n'));
                document_view_selected_text_from_node(
                    doc,
                    0,
                    doc.visual_cache.flat_text.len(),
                    false,
                )
            }
            _ => None,
        })
        .expect("document view exists");

    assert_eq!(copied, text);
}

#[test]
fn document_view_copy_preserves_real_newlines() {
    let text = "first line\nsecond line";
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 4,
    };
    let mut runtime = RuntimeCore::new_test(
        NewlineDocumentViewCopySmoke,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let copied = runtime
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::DocumentView(doc) => document_view_selected_text_from_node(
                doc,
                0,
                doc.visual_cache.flat_text.len(),
                false,
            ),
            _ => None,
        })
        .expect("document view exists");

    assert_eq!(copied, text);
}

#[cfg(feature = "markdown")]
#[test]
fn document_view_copy_emits_mermaid_source_once_for_visual_diagram_rows() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 20,
    };
    let mut runtime = RuntimeCore::new_test(
        MermaidDocumentViewCopySmoke,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let copied = runtime
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::DocumentView(doc) => {
                let diagram_rows = doc
                    .visual_cache
                    .lines
                    .iter()
                    .filter(|line| matches!(line.kind, VisualLineKind::DiagramRow { .. }))
                    .count();
                assert!(diagram_rows > 1);
                document_view_selected_text_from_node(
                    doc,
                    0,
                    doc.visual_cache.flat_text.len(),
                    false,
                )
            }
            _ => None,
        })
        .expect("document view exists");

    assert_eq!(copied, "flowchart TD\nA[Start] --> B[End]");
}

#[cfg(feature = "markdown")]
#[test]
fn document_view_copy_omits_render_only_markdown_chrome() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 12,
    };
    let mut runtime = RuntimeCore::new_test(
        MarkdownChromeCopySmoke,
        (),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runtime.init();
    runtime.render_element(viewport, None, None, None);

    let copied = runtime
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::DocumentView(doc) => document_view_selected_text_from_node(
                doc,
                0,
                doc.visual_cache.flat_text.len(),
                false,
            ),
            _ => None,
        })
        .expect("document view exists");

    assert!(!copied.contains('│'));
    assert_eq!(copied, "Quote line\n\nhello");
}

#[test]
fn reverse_table_rect_copy_trims_top_left_cell_from_cursor_side() {
    let mut doc: crate::widgets::document_view::node::DocumentViewNode =
        DocumentView::new("").into();
    doc.visual_cache.lines = vec![
        DocumentVisualLine {
            kind: VisualLineKind::TableRow {
                cells: Vec::new(),
                cell_line_texts: vec![Arc::from("alpha"), Arc::from("beta")],
                full_cell_texts: vec![Arc::from("alpha"), Arc::from("beta")],
                alignments: Vec::new(),
                widths: Vec::new(),
                table_id: 0,
                row_index: 1,
                row_line_index: 0,
                border_variant: BorderStyle::Plain,
                outer_frame: true,
                column_separators: true,
                cell_padding: 1,
            },
            source_line: 0,
        },
        DocumentVisualLine {
            kind: VisualLineKind::TableRow {
                cells: Vec::new(),
                cell_line_texts: vec![Arc::from("gamma"), Arc::from("delta")],
                full_cell_texts: vec![Arc::from("gamma"), Arc::from("delta")],
                alignments: Vec::new(),
                widths: Vec::new(),
                table_id: 0,
                row_index: 2,
                row_line_index: 0,
                border_variant: BorderStyle::Plain,
                outer_frame: true,
                column_separators: true,
                cell_padding: 1,
            },
            source_line: 0,
        },
    ];

    let copied = table_tsv_for_range(
        &doc,
        TableTsvRangeParams {
            table_id: 0,
            row_start: 1,
            row_end: 2,
            col_start: 0,
            col_end: 1,
            cursor_row_index: 1,
            cursor_col_index: 0,
            anchor_row_index: 2,
            anchor_col_index: 1,
            anchor_cell_line_anchor_byte: 2,
            cursor_cell_line_anchor_byte: 2,
        },
    );

    assert_eq!(copied.as_ref(), "pha\tbeta\ngamma\tdelta");
}

#[test]
fn table_rect_copy_suffix_mode_clamped_pointer_yields_full_cursor_cell() {
    let mut doc: crate::widgets::document_view::node::DocumentViewNode =
        DocumentView::new("").into();
    doc.visual_cache.lines = vec![
        DocumentVisualLine {
            kind: VisualLineKind::TableRow {
                cells: Vec::new(),
                cell_line_texts: vec![Arc::from("alpha"), Arc::from("beta")],
                full_cell_texts: vec![Arc::from("alpha"), Arc::from("beta")],
                alignments: Vec::new(),
                widths: Vec::new(),
                table_id: 0,
                row_index: 1,
                row_line_index: 0,
                border_variant: BorderStyle::Plain,
                outer_frame: true,
                column_separators: true,
                cell_padding: 1,
            },
            source_line: 0,
        },
        DocumentVisualLine {
            kind: VisualLineKind::TableRow {
                cells: Vec::new(),
                cell_line_texts: vec![Arc::from("gamma"), Arc::from("delta")],
                full_cell_texts: vec![Arc::from("gamma"), Arc::from("delta")],
                alignments: Vec::new(),
                widths: Vec::new(),
                table_id: 0,
                row_index: 2,
                row_line_index: 0,
                border_variant: BorderStyle::Plain,
                outer_frame: true,
                column_separators: true,
                cell_padding: 1,
            },
            source_line: 0,
        },
    ];

    let copied = table_tsv_for_range(
        &doc,
        TableTsvRangeParams {
            table_id: 0,
            row_start: 1,
            row_end: 2,
            col_start: 0,
            col_end: 1,
            cursor_row_index: 1,
            cursor_col_index: 0,
            anchor_row_index: 2,
            anchor_col_index: 1,
            anchor_cell_line_anchor_byte: 2,
            cursor_cell_line_anchor_byte: usize::MAX,
        },
    );

    assert_eq!(copied.as_ref(), "alpha\tbeta\ngamma\tdelta");
}

#[test]
fn table_rect_copy_same_column_vertical_uses_prefix_trim() {
    let mut doc: crate::widgets::document_view::node::DocumentViewNode =
        DocumentView::new("").into();
    doc.visual_cache.lines = vec![
        DocumentVisualLine {
            kind: VisualLineKind::TableRow {
                cells: Vec::new(),
                cell_line_texts: vec![Arc::from("delta")],
                full_cell_texts: vec![Arc::from("delta")],
                alignments: Vec::new(),
                widths: Vec::new(),
                table_id: 0,
                row_index: 0,
                row_line_index: 0,
                border_variant: BorderStyle::Plain,
                outer_frame: true,
                column_separators: true,
                cell_padding: 1,
            },
            source_line: 0,
        },
        DocumentVisualLine {
            kind: VisualLineKind::TableRow {
                cells: Vec::new(),
                cell_line_texts: vec![Arc::from("omega")],
                full_cell_texts: vec![Arc::from("omega")],
                alignments: Vec::new(),
                widths: Vec::new(),
                table_id: 0,
                row_index: 1,
                row_line_index: 0,
                border_variant: BorderStyle::Plain,
                outer_frame: true,
                column_separators: true,
                cell_padding: 1,
            },
            source_line: 0,
        },
    ];

    let copied = table_tsv_for_range(
        &doc,
        TableTsvRangeParams {
            table_id: 0,
            row_start: 0,
            row_end: 1,
            col_start: 0,
            col_end: 0,
            cursor_row_index: 0,
            cursor_col_index: 0,
            anchor_row_index: 1,
            anchor_col_index: 0,
            anchor_cell_line_anchor_byte: 5,
            cursor_cell_line_anchor_byte: 2,
        },
    );

    assert_eq!(copied.as_ref(), "de\nomega");
}

#[test]
fn table_rect_copy_cursor_right_of_anchor_column_uses_prefix_trim() {
    let mut doc: crate::widgets::document_view::node::DocumentViewNode =
        DocumentView::new("").into();
    doc.visual_cache.lines = vec![
        DocumentVisualLine {
            kind: VisualLineKind::TableRow {
                cells: Vec::new(),
                cell_line_texts: vec![Arc::from("a0"), Arc::from("b0"), Arc::from("c0")],
                full_cell_texts: vec![Arc::from("a0"), Arc::from("b0"), Arc::from("c0")],
                alignments: Vec::new(),
                widths: Vec::new(),
                table_id: 0,
                row_index: 0,
                row_line_index: 0,
                border_variant: BorderStyle::Plain,
                outer_frame: true,
                column_separators: true,
                cell_padding: 1,
            },
            source_line: 0,
        },
        DocumentVisualLine {
            kind: VisualLineKind::TableRow {
                cells: Vec::new(),
                cell_line_texts: vec![Arc::from("a1"), Arc::from("b1"), Arc::from("c1")],
                full_cell_texts: vec![Arc::from("a1"), Arc::from("b1"), Arc::from("c1")],
                alignments: Vec::new(),
                widths: Vec::new(),
                table_id: 0,
                row_index: 1,
                row_line_index: 0,
                border_variant: BorderStyle::Plain,
                outer_frame: true,
                column_separators: true,
                cell_padding: 1,
            },
            source_line: 0,
        },
    ];

    // Anchor center (1,1) "b1", drag end top-right (0,2) "c0" with cursor byte 1 → prefix.
    let copied = table_tsv_for_range(
        &doc,
        TableTsvRangeParams {
            table_id: 0,
            row_start: 0,
            row_end: 1,
            col_start: 1,
            col_end: 2,
            cursor_row_index: 0,
            cursor_col_index: 2,
            anchor_row_index: 1,
            anchor_col_index: 1,
            anchor_cell_line_anchor_byte: 2,
            cursor_cell_line_anchor_byte: 1,
        },
    );

    assert_eq!(copied.as_ref(), "b0\tc\nb1\tc1");
}

#[test]
fn find_junction_splitter_bounds_math_does_not_overflow_i16() {
    use crate::core::node::{NodeKind, NodeTree};
    use crate::widgets::internal::SplitterNode;
    use crate::widgets::{Orientation, Splitter, SplitterHandleMode};

    // A handle rect taller than i16::MAX (32767): the old `rect.h as i16` cast would wrap
    // this to a negative number before the bounds check ran, breaking hit-testing for any
    // splitter long enough to overflow. `Rect::h` is `u16`, so 40_000 is a valid width/height
    // even though no real terminal is ever this large.
    let huge_handle = Rect {
        x: 5,
        y: 0,
        w: 1,
        h: 40_000,
    };

    let mut tree = NodeTree::new();
    let perpendicular_id = tree.alloc();
    tree.root = perpendicular_id;
    let mut node: SplitterNode = Splitter::horizontal().into();
    node.orientation = Orientation::Horizontal;
    node.handle_mode = SplitterHandleMode::Gutter;
    node.handle_rects = vec![huge_handle];
    node.pane_sizes = vec![1, 1];
    tree.node_mut(perpendicular_id).kind = NodeKind::Splitter(node);

    let primary = crate::core::node::NodeId::INVALID;

    // A point one cell to the left of the handle (inside the documented one-cell expansion)
    // must still be found; with the old wraparound bug this range collapsed and the point
    // was missed instead.
    let target = super::find_junction_splitter(&tree, primary, Orientation::Vertical, 4, 100)
        .expect("point within the expanded handle bounds must be found");
    assert_eq!(target.id, perpendicular_id);

    // A point clearly outside the (correctly, non-overflowing) expanded bounds must not match.
    assert!(
        super::find_junction_splitter(&tree, primary, Orientation::Vertical, 100, 100).is_none()
    );
}
