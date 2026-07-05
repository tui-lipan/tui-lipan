use crate::layout::measure::min_size_constrained;
use crate::style::{Length, Rect};
use crate::widgets::Frame;

use super::box_metrics::{FrameJoinOverlap, compute_frame_geometry, frame_inner_max_size};

/// Calculate the minimal chrome height for a Frame (without content).
///
/// This is used for Flex-height frames to prevent them from claiming
/// full content height during min-size calculations.
pub(crate) fn measure_frame_chrome(frame: &Frame) -> (u16, u16) {
    let mut min_w = 0u16;
    let mut min_h = 0u16;

    // Add padding
    min_w = min_w.saturating_add(frame.props.padding.horizontal());
    min_h = min_h.saturating_add(frame.props.padding.vertical());

    // Add border
    if frame.props.has_border() {
        let border_padding = frame.props.border_padding();
        min_w = min_w.saturating_add(border_padding.horizontal());
        min_h = min_h.saturating_add(border_padding.vertical());

        // Account for title/tabs width
        if !frame.props.tab_titles.is_empty() {
            let active_tab = frame
                .props
                .active_tab
                .min(frame.props.tab_titles.len().saturating_sub(1));
            let mut w = 0usize;
            for (i, title) in frame.props.tab_titles.iter().enumerate() {
                let title_w = title.width();
                let (pad_l, pad_r) = if i == active_tab {
                    frame.props.tab_variant.active_padding_width()
                } else {
                    frame.props.tab_variant.inactive_padding_width()
                };
                w = w
                    .saturating_add(title_w)
                    .saturating_add(pad_l)
                    .saturating_add(pad_r);
                if i + 1 < frame.props.tab_titles.len() {
                    w = w.saturating_add(frame.props.tab_variant.separator_width());
                }
            }
            let w = w.saturating_add(2).min(u16::MAX as usize) as u16;
            min_w = min_w.max(w);
        } else if let Some(title) = &frame.props.title {
            let w = title.width().saturating_add(2).min(u16::MAX as usize) as u16;
            min_w = min_w.max(w);
        }
    } else {
        // Header takes space if no border
        if frame.header.is_some() {
            let (header_w, header_h) = frame
                .header
                .as_deref()
                .map(crate::layout::measure::min_size)
                .unwrap_or((0, 0));
            min_w = min_w.max(header_w);
            min_h = min_h.saturating_add(header_h.max(1));
        }

        // Status takes space if no border
        let has_status = frame.props.status.is_some()
            || frame.props.status_center.is_some()
            || frame.props.status_right.is_some();
        if has_status {
            let mut w = 0u16;
            if let Some(status) = &frame.props.status {
                w = w.max(status.width().min(u16::MAX as usize) as u16);
            }
            if let Some(status) = &frame.props.status_center {
                w = w.max(status.width().min(u16::MAX as usize) as u16);
            }
            if let Some(status) = &frame.props.status_right {
                w = w.max(status.width().min(u16::MAX as usize) as u16);
            }
            min_w = min_w.max(w);
            min_h = min_h.saturating_add(1);
        }
    }

    // Ensure at least 1 line of content space
    min_h = min_h.saturating_add(1);

    let decoration_pad = frame.props.decoration_outside_padding();
    min_w = min_w.saturating_add(decoration_pad.horizontal());
    min_h = min_h.saturating_add(decoration_pad.vertical());

    let border_deco = frame.props.decoration_border_content_inset();
    min_w = min_w.saturating_add(border_deco.horizontal());
    min_h = min_h.saturating_add(border_deco.vertical());

    (min_w, min_h)
}

pub(crate) fn measure_frame(
    frame: &Frame,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> super::box_metrics::FrameGeometry {
    let decoration_outside = frame.props.decoration_outside_padding();
    let border_deco_inset = frame.props.decoration_border_content_inset();

    let (inner_max_w, inner_max_h) = frame_inner_max_size(frame, max_w, max_h);

    let (content_w, content_h) = frame
        .child
        .as_deref()
        .map(|c| min_size_constrained(c, inner_max_w, inner_max_h))
        .unwrap_or((0, 0));

    // Measure header if exists (for dynamic height calculation)
    let (header_w, _header_h) = frame
        .header
        .as_deref()
        .map(crate::layout::measure::min_size)
        .unwrap_or((0, 0));

    let mut inner_w = content_w
        .max(header_w)
        .saturating_add(frame.props.padding.horizontal())
        .saturating_add(border_deco_inset.horizontal());
    let mut inner_h = content_h
        .saturating_add(frame.props.padding.vertical())
        .saturating_add(border_deco_inset.vertical());

    // If header exists and NO border, it consumes vertical space inside the layout logic
    // (With border, header is in the border area)
    if frame.header.is_some() && !frame.props.has_border() {
        let (_, header_h) = frame
            .header
            .as_deref()
            .map(crate::layout::measure::min_size)
            .unwrap_or((0, 0));
        inner_h = inner_h.saturating_add(header_h.max(1));
    }

    let has_status = frame.props.status.is_some()
        || frame.props.status_center.is_some()
        || frame.props.status_right.is_some();
    if has_status {
        let mut w = 0u16;
        if let Some(status) = &frame.props.status {
            w = w.max(status.width().min(u16::MAX as usize) as u16);
        }
        if let Some(status) = &frame.props.status_center {
            w = w.max(status.width().min(u16::MAX as usize) as u16);
        }
        if let Some(status) = &frame.props.status_right {
            w = w.max(status.width().min(u16::MAX as usize) as u16);
        }
        inner_w = inner_w.max(w);
        if !frame.props.has_border() {
            inner_h = inner_h.saturating_add(1);
        }
    }

    let mut outer_w = inner_w;
    let mut outer_h = inner_h;

    if frame.props.has_border() {
        if !frame.props.tab_titles.is_empty() {
            let active_tab = frame
                .props
                .active_tab
                .min(frame.props.tab_titles.len().saturating_sub(1));
            let mut w = 0usize;
            for (i, title) in frame.props.tab_titles.iter().enumerate() {
                let title_w = title.width();
                let (pad_l, pad_r) = if i == active_tab {
                    frame.props.tab_variant.active_padding_width()
                } else {
                    frame.props.tab_variant.inactive_padding_width()
                };
                w = w
                    .saturating_add(title_w)
                    .saturating_add(pad_l)
                    .saturating_add(pad_r);
                if i + 1 < frame.props.tab_titles.len() {
                    w = w.saturating_add(frame.props.tab_variant.separator_width());
                }
            }
            let w = w.saturating_add(2).min(u16::MAX as usize) as u16;
            outer_w = outer_w.max(w);
        } else if let Some(title) = &frame.props.title {
            let w = title.width().saturating_add(2).min(u16::MAX as usize) as u16;
            outer_w = outer_w.max(w);
        }

        let border_padding = frame.props.border_padding();
        outer_w = outer_w.saturating_add(border_padding.horizontal());
        outer_h = outer_h.saturating_add(border_padding.vertical());
    }

    outer_w = outer_w.saturating_add(decoration_outside.horizontal());
    outer_h = outer_h.saturating_add(decoration_outside.vertical());

    // Fixed sizes are outer frame sizes. Border/padding/decorations consume
    // space inside the same allocated rect instead of inflating it.
    if let Length::Px(px) = frame.props.width {
        outer_w = px;
    }
    if let Length::Percent(pct) = frame.props.width
        && let Some(parent_w) = max_w
    {
        outer_w = ((parent_w as u32 * pct.min(100) as u32) / 100).min(u16::MAX as u32) as u16;
    }
    if let Length::Px(px) = frame.props.height {
        outer_h = px;
    }
    if let Length::Percent(pct) = frame.props.height
        && let Some(parent_h) = max_h
    {
        outer_h = ((parent_h as u32 * pct.min(100) as u32) / 100).min(u16::MAX as u32) as u16;
    }

    let rect = Rect {
        x: 0,
        y: 0,
        w: outer_w,
        h: outer_h,
    };

    compute_frame_geometry(&frame.props, rect, FrameJoinOverlap::default(), true)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::measure_frame;
    use crate::app::context::SurfaceMode;
    use crate::core::component::{Component, Context, Update};
    use crate::core::node::NodeKind;
    use crate::runtime::RuntimeCore;
    use crate::style::{BorderEdges, Length};
    use crate::style::{Rect, Theme};
    use crate::widgets::text::Overflow;
    use crate::widgets::{DocumentView, Frame, Text, TextArea, VStack};

    #[cfg(feature = "diff-view")]
    use crate::layout::measure::min_size_constrained;
    #[cfg(feature = "diff-view")]
    use crate::widgets::{DiffView, DiffViewBackend, DiffViewMode};

    #[test]
    fn frame_px_size_is_outer_size_with_border_and_padding() {
        let frame = Frame::new()
            .border(true)
            .padding((1, 2))
            .width(Length::Px(20))
            .height(Length::Px(6))
            .child(Text::new("content"));

        let geometry = measure_frame(&frame, None, None);
        assert_eq!(geometry.outer_size(), (20, 6));
    }

    #[test]
    fn horizontal_caps_border_does_not_consume_content_columns() {
        let frame = Frame::new()
            .border(true)
            .border_edges(BorderEdges::HorizontalCaps)
            .width(Length::Px(20))
            .height(Length::Px(5))
            .child(Text::new("content"));

        let geometry = measure_frame(&frame, None, None);
        assert_eq!(geometry.content_rect.x, 0);
        assert_eq!(geometry.content_rect.w, 20);
        assert_eq!(geometry.content_rect.y, 1);
        assert_eq!(geometry.content_rect.h, 3);
    }

    #[test]
    fn horizontal_caps_border_measures_wrap_height_at_full_width() {
        let frame = Frame::new()
            .border(true)
            .border_edges(BorderEdges::HorizontalCaps)
            .width(Length::Px(11))
            .child(Text::new("hello world").overflow(Overflow::Wrap));

        let geometry = measure_frame(&frame, Some(80), None);
        assert_eq!(geometry.outer_size().0, 11);
        assert_eq!(geometry.outer_size().1, 3);
    }

    #[test]
    fn frame_px_size_matches_for_border_and_no_border() {
        let with_border = Frame::new()
            .border(true)
            .padding((1, 1))
            .width(Length::Px(18))
            .height(Length::Px(5))
            .child(Text::new("x"));
        let without_border = Frame::new()
            .border(false)
            .padding((1, 1))
            .width(Length::Px(18))
            .height(Length::Px(5))
            .child(Text::new("x"));

        assert_eq!(
            measure_frame(&with_border, None, None).outer_size(),
            (18, 5)
        );
        assert_eq!(
            measure_frame(&without_border, None, None).outer_size(),
            (18, 5)
        );
    }

    #[test]
    fn frame_borderless_border_decoration_adds_horizontal_measurement() {
        use crate::style::Edge;
        use crate::widgets::frame::{DecorationGlyph, EdgeDecoration};

        let frame = Frame::new()
            .border(false)
            .decorations(vec![
                EdgeDecoration::new(Edge::Left).glyph(DecorationGlyph::AutoBlock),
            ])
            .child(Text::new("ab"));

        let geometry = measure_frame(&frame, None, None);
        assert_eq!(geometry.outer_size(), (3, 1));
    }

    /// A Px-width frame narrower than the parent must use its own inner width
    /// when computing the child's height, so that wrapping text gets the
    /// correct (larger) height instead of the parent's wider measurement.
    #[test]
    fn px_width_frame_measures_wrap_height_at_own_inner_width() {
        // "hello world" = 11 chars.  Frame inner width = 10-2(border) = 8.
        // ceil(11/8) = 2 wrapped lines → frame height = 2 + 2(border) = 4.
        let frame = Frame::new()
            .border(true)
            .width(Length::Px(10))
            .child(Text::new("hello world").overflow(Overflow::Wrap));

        // Parent offers 80 columns - must NOT use that for the height calculation.
        let geometry = measure_frame(&frame, Some(80), None);
        assert_eq!(geometry.outer_size().0, 10);
        assert_eq!(
            geometry.outer_size().1,
            4,
            "expected 2 wrapped lines + 2 border rows"
        );
    }

    /// A Percent-width frame also constrains child measurement to its own
    /// inner width so that wrapping text is sized correctly.
    #[test]
    fn percent_width_frame_measures_wrap_height_at_own_inner_width() {
        // 50% of 20 = 10.  Inner width = 10-2(border) = 8.
        // "hello world" (11 chars) at 8 → 2 lines → frame height = 4.
        let frame = Frame::new()
            .border(true)
            .width(Length::Percent(50))
            .child(Text::new("hello world").overflow(Overflow::Wrap));

        let geometry = measure_frame(&frame, Some(20), None);
        assert_eq!(geometry.outer_size().0, 10);
        assert_eq!(
            geometry.outer_size().1,
            4,
            "expected 2 wrapped lines + 2 border rows"
        );
    }

    /// Auto-width frame (the default) continues to use the parent's available
    /// width for measurement - no regression.
    #[test]
    fn auto_width_frame_uses_parent_width_for_wrap_measurement() {
        // Available width 10 → inner 8 → "hello world" wraps to 2 lines.
        let frame = Frame::new()
            .border(true)
            .child(Text::new("hello world").overflow(Overflow::Wrap));

        let geometry = measure_frame(&frame, Some(10), None);
        assert_eq!(
            geometry.outer_size().1,
            4,
            "expected 2 wrapped lines + 2 border rows"
        );
    }

    #[test]
    fn auto_height_frame_matches_wrapped_document_view_height() {
        let doc = DocumentView::new(
            "A long paragraph that wraps across multiple visual lines when the frame gets narrow enough to constrain the document width.",
        )
        .height(Length::Auto)
        .border(false)
        .wrap(true);

        let child =
            crate::layout::measure::min_size_constrained(&doc.clone().into(), Some(26), None);
        let frame = Frame::new().height(Length::Auto).border(true).child(doc);

        let geometry = measure_frame(&frame, Some(28), None);

        assert_eq!(geometry.outer_size().1, child.1.saturating_add(2));
    }

    #[test]
    fn auto_height_frame_matches_wrapped_text_area_height() {
        let area = TextArea::new(
            "A long paragraph that wraps across multiple visual lines when the frame gets narrow enough to constrain the text area width.",
        )
        .height(Length::Auto)
        .border(false)
        .wrap(true)
        .line_numbers(false);

        let child =
            crate::layout::measure::min_size_constrained(&area.clone().into(), Some(26), None);
        let frame = Frame::new().height(Length::Auto).border(true).child(area);

        let geometry = measure_frame(&frame, Some(28), None);

        assert_eq!(geometry.outer_size().1, child.1.saturating_add(2));
    }

    struct WrappedDocumentFrameImplicitAutoRepro;

    struct CollapsibleBorderFrameHeightRepro;

    impl Component for WrappedDocumentFrameImplicitAutoRepro {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
            Frame::new()
                .border(false)
                .height(Length::Auto)
                .padding((1, 1, 1, 3))
                .child(
                    VStack::new()
                        .height(Length::Auto)
                        .gap(1)
                        .child(Text::new("Title"))
                        .child(
                            DocumentView::new(SAMPLE_WRAP_TEXT)
                                .border(false)
                                .wrap(true)
                                .scrollbar(false)
                                .h_scrollbar(false)
                                .focusable(false),
                        ),
                )
                .into()
        }
    }

    impl Component for CollapsibleBorderFrameHeightRepro {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
            VStack::new()
                .child(
                    Frame::new()
                        .border(true)
                        .padding(1)
                        .height(Length::Flex(1))
                        .child(Text::new("body")),
                )
                .into()
        }
    }

    const SAMPLE_WRAP_TEXT: &str = "warning: unused variable: `now_ms`\n  --> src/dialogs/session_list.rs:33:9\n   |\n33 |     let now_ms = now_secs * 1000.0;\n   |         ^^^^^^ help: if this is intentional, prefix it with an underscore: `_now_ms`\n   |\n   = note: `#[warn(unused_variables)]` on by default\n";

    fn render_document_viewport_height(width: u16) -> (usize, usize) {
        let viewport = Rect {
            x: 0,
            y: 0,
            w: width,
            h: 200,
        };
        let mut runtime = RuntimeCore::new_test(
            WrappedDocumentFrameImplicitAutoRepro,
            (),
            viewport,
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(Cell::new(false)),
        );
        runtime.init();
        runtime.render_element(viewport, None, None, None);

        let doc_node = runtime
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
            .expect("document view exists");
        let doc = match &doc_node.kind {
            NodeKind::DocumentView(doc) => doc,
            _ => unreachable!(),
        };

        (
            doc_node.rect.inner(doc.border, doc.padding).h as usize,
            doc.total_visual_lines,
        )
    }

    #[test]
    fn implicit_auto_wrapped_document_inside_auto_frame_keeps_full_visible_height() {
        let (viewport_h, total_visual_lines) = render_document_viewport_height(22);
        assert_eq!(viewport_h, total_visual_lines);
    }

    #[test]
    fn bordered_collapsible_frame_keeps_three_rows_when_stack_overflows() {
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 4,
        };
        let mut runtime = RuntimeCore::new_test(
            CollapsibleBorderFrameHeightRepro,
            (),
            viewport,
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(Cell::new(false)),
        );
        runtime.init();
        runtime.render_element(viewport, None, None, None);

        let frame_node = runtime
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Frame(_)))
            .expect("frame exists");

        assert_eq!(frame_node.rect.h, 3);
    }

    #[cfg(feature = "diff-view")]
    #[test]
    fn auto_height_frame_matches_wrapped_diffview_height() {
        let diff: crate::core::element::Element = DiffView::with_content(
            "this is a very long line that should wrap\nshort",
            "this is a very long line that should wrap differently\nshort",
        )
        .mode(DiffViewMode::Unified)
        .backend(DiffViewBackend::DocumentView)
        .wrap(true)
        .height(Length::Auto)
        .border(false)
        .panels_border(true)
        .document_view(DocumentView::new("").height(Length::Auto).border(false))
        .into();

        let (_, diff_h) = min_size_constrained(&diff, Some(22), None);
        let frame = Frame::new()
            .width(Length::Auto)
            .height(Length::Auto)
            .child(diff);

        let geometry = measure_frame(&frame, Some(24), None);

        assert_eq!(geometry.outer_size().1, diff_h.saturating_add(2));
    }

    #[cfg(feature = "diff-view")]
    #[test]
    fn auto_height_frame_matches_wrapped_split_diffview_height() {
        let diff: crate::core::element::Element = DiffView::with_content(
            "this is a very long line that should wrap on the left pane\nshort",
            "this is a very long line that should wrap on the right pane differently\nshort",
        )
        .mode(DiffViewMode::Split)
        .backend(DiffViewBackend::DocumentView)
        .wrap(true)
        .height(Length::Auto)
        .border(false)
        .panels_border(true)
        .document_view(DocumentView::new("").height(Length::Auto).border(false))
        .into();

        let (_, diff_h) = min_size_constrained(&diff, Some(32), None);
        let frame = Frame::new()
            .width(Length::Auto)
            .height(Length::Auto)
            .child(diff);

        let geometry = measure_frame(&frame, Some(34), None);

        assert_eq!(geometry.outer_size().1, diff_h.saturating_add(2));
    }
}
