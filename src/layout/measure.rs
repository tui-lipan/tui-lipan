//! Widget measurement functions.

use std::cell::RefCell;

use crate::core::element::{Element, ElementKind, MeasureCacheEntry};
#[cfg(feature = "big-text")]
use crate::widgets::internal::measure_big_text;
#[cfg(feature = "image")]
use crate::widgets::internal::measure_image;
#[cfg(feature = "terminal")]
use crate::widgets::internal::measure_terminal;
use crate::widgets::internal::{
    measure_animated, measure_ascii_canvas, measure_button, measure_canvas, measure_center,
    measure_center_pin, measure_chart, measure_checkbox, measure_class_diagram, measure_divider,
    measure_document_view, measure_document_view_constrained, measure_drag_source,
    measure_draggable_tab_bar, measure_drop_target, measure_effect_scope, measure_er_diagram,
    measure_flow, measure_flowchart, measure_frame, measure_gantt_diagram, measure_graph,
    measure_grid, measure_heatmap, measure_hex_area, measure_input, measure_mouse_region,
    measure_pan_view, measure_progress_bar, measure_scroll_view, measure_sequence_diagram,
    measure_slider, measure_spacer, measure_sparkline, measure_spinner, measure_splitter,
    measure_stack, measure_state_diagram, measure_status_bar_layout, measure_tabs,
    measure_text_area, measure_text_area_constrained,
};

use super::axis::Axis;
use super::hash::element_layout_hash;

type GlobalMeasureCacheKey = (u64, Option<u16>, Option<u16>);
type GlobalMeasureCache = crate::utils::gen_cache::GenerationalCache<
    GlobalMeasureCacheKey,
    (u16, u16),
    rustc_hash::FxBuildHasher,
>;

thread_local! {
    static GLOBAL_MEASURE_CACHE: RefCell<GlobalMeasureCache> =
        RefCell::new(GlobalMeasureCache::new(GLOBAL_MEASURE_CACHE_MAX_ENTRIES));
}

/// Per-generation cap. Large scroll lists × width probes would otherwise thrash a
/// tiny cache that cleared entirely on overflow; the generational cache keeps a
/// second generation so resize sweeps keep hitting (see [`crate::utils::gen_cache`]).
const GLOBAL_MEASURE_CACHE_MAX_ENTRIES: usize = 16_384;

#[cfg(feature = "diff-view")]
fn element_has_split_wrap_measure_state(el: &Element) -> bool {
    crate::widgets::element_subtree_has_split_wrap_sync(el)
}

#[cfg(not(feature = "diff-view"))]
fn element_has_split_wrap_measure_state(_el: &Element) -> bool {
    false
}

/// SourceSnapshot drag sources must skip the per-element measure cache when a
/// collapse hint is active, because the cached (non-collapsed) size would be
/// returned before `measure_drag_source` has a chance to check the hint.
fn drag_source_should_skip_measure_cache(el: &Element) -> bool {
    if let ElementKind::DragSource(source) = &el.kind {
        matches!(source.preview, crate::widgets::DragPreview::SourceSnapshot)
            && super::drag_source_layout_hint::drag_source_snapshot_collapse_key().is_some()
    } else {
        false
    }
}

/// Compute the minimum size of an element.
pub(crate) fn min_size(el: &Element) -> (u16, u16) {
    min_size_constrained(el, None, None)
}

/// Return this element's intrinsic `(min-content, max-content)` size along the
/// given stack main axis.
///
/// `max-content` is the size the child would use with unlimited main-axis space.
/// `min-content` is the narrowest main-axis size that preserves the child's own
/// content floor. Atomic widgets report the same value for both; wrapping
/// widgets can opt into narrower min-content and `LayoutConstraints::reflows`.
pub(crate) fn intrinsic_main(el: &Element, axis: Axis, cross: Option<u16>) -> (u16, u16) {
    match &el.kind {
        ElementKind::Group(group) => intrinsic_main(&group.child, axis, cross),
        ElementKind::EffectScope(scope) => scope
            .child
            .as_deref()
            .map(|child| intrinsic_main(child, axis, cross))
            .unwrap_or((0, 0)),
        ElementKind::ThemeProvider(tp) => intrinsic_main(&tp.child, axis, cross),
        ElementKind::ContextProvider(cp) => intrinsic_main(&cp.child, axis, cross),
        ElementKind::Flow(flow) if axis == Axis::Horizontal => {
            let max_content = measure_flow(flow, None, cross).0;
            (flow_intrinsic_min_width(flow), max_content)
        }
        _ => {
            let (w, h) = match axis {
                Axis::Vertical => min_size_constrained(el, cross, None),
                Axis::Horizontal => min_size_constrained(el, None, cross),
            };
            let main = match axis {
                Axis::Vertical => h,
                Axis::Horizontal => w,
            };
            (main, main)
        }
    }
}

fn flow_intrinsic_min_width(flow: &crate::widgets::Flow) -> u16 {
    let widest = flow
        .children
        .iter()
        .filter(|c| !matches!(c.kind, ElementKind::Portal(_)))
        .map(|c| min_size_constrained(c, None, None).0)
        .max()
        .unwrap_or(0);
    if widest == 0 {
        return 0;
    }
    widest.saturating_add(flow.padding.horizontal() + if flow.border { 2 } else { 0 })
}

pub(crate) fn min_size_constrained(
    el: &Element,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    let split_wrap_measure_state = element_has_split_wrap_measure_state(el);
    let skip_local_cache = split_wrap_measure_state || drag_source_should_skip_measure_cache(el);
    let skip_global_cache = drag_source_should_skip_measure_cache(el);

    if !skip_local_cache {
        for entry in el.measure_cache.get().into_iter().flatten() {
            if entry.max_w == max_w && entry.max_h == max_h {
                return entry.size;
            }
        }
    }

    let global_cache_key = if !skip_global_cache {
        element_layout_hash(el).map(|hash| (hash, max_w, max_h))
    } else {
        None
    };
    if let Some(key) = global_cache_key
        && let Some(size) = GLOBAL_MEASURE_CACHE.with(|cache| cache.borrow().get(&key).copied())
    {
        if !skip_local_cache {
            let new_entry = Some(MeasureCacheEntry { max_w, max_h, size });
            let mut slots = el.measure_cache.get();
            slots[1] = slots[0];
            slots[0] = new_entry;
            el.measure_cache.set(slots);
        }
        return size;
    }

    let constraints = el.layout_constraints();
    // Use the parent's offered size as the reference for resolving Percent constraints.
    // When no parent bound is given, u16::MAX means "unconstrained".
    let avail_w = max_w.unwrap_or(u16::MAX);
    let avail_h = max_h.unwrap_or(u16::MAX);

    // Resolve the element's own max constraints to pixels, then take the tighter of
    // the element's resolved max and the parent's offered width/height.
    let element_max_w = constraints.max_w.and_then(|l| l.resolve_as_max(avail_w));
    let element_max_h = constraints.max_h.and_then(|l| l.resolve_as_max(avail_h));
    let effective_max_w: Option<u16> = match (element_max_w, max_w) {
        (Some(em), Some(pm)) => Some(em.min(pm)),
        (em, pm) => em.or(pm),
    };
    let effective_max_h: Option<u16> = match (element_max_h, max_h) {
        (Some(em), Some(pm)) => Some(em.min(pm)),
        (em, pm) => em.or(pm),
    };

    let (w, h) = if let ElementKind::TextArea(t) = &el.kind {
        measure_text_area_constrained(t, effective_max_w)
    } else if let ElementKind::DocumentView(dv) = &el.kind {
        measure_document_view_constrained(dv, effective_max_w)
    } else {
        // Pass the effective (tighter) bounds so children are measured within
        // both the parent's available space and this element's own max constraints.
        min_size_unconstrained_constrained(el, effective_max_w, effective_max_h)
    };

    let size = (
        constraints.clamp_width(w, avail_w),
        constraints.clamp_height(h, avail_h),
    );

    if !skip_local_cache {
        let new_entry = Some(MeasureCacheEntry { max_w, max_h, size });
        let mut slots = el.measure_cache.get();
        if slots[0] == new_entry {
            return size;
        }
        slots[1] = slots[0];
        slots[0] = new_entry;
        el.measure_cache.set(slots);
    }

    if let Some(key) = global_cache_key {
        GLOBAL_MEASURE_CACHE.with(|cache| cache.borrow_mut().insert(key, size));
    }

    size
}

fn min_size_unconstrained_constrained(
    el: &Element,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    match &el.kind {
        ElementKind::Text(t) => crate::widgets::internal::measure_text_constrained(t, max_w),
        #[cfg(feature = "big-text")]
        ElementKind::BigText(t) => measure_big_text(t),
        ElementKind::AsciiCanvas(canvas) => measure_ascii_canvas(canvas, max_w, max_h),
        ElementKind::Button(b) => measure_button(b),
        ElementKind::Input(i) => measure_input(i),
        ElementKind::HexArea(h) => measure_hex_area(h),
        #[cfg(feature = "image")]
        ElementKind::Image(i) => measure_image(i),
        ElementKind::List(l) => crate::widgets::list::layout::measure_list_constrained(l, max_w),
        ElementKind::TextArea(t) => measure_text_area(t),
        #[cfg(feature = "terminal")]
        ElementKind::Terminal(t) => measure_terminal(t),
        ElementKind::Popover(p) => min_size_constrained(p.trigger.as_ref(), max_w, max_h),
        ElementKind::Portal(_) => (0, 0),
        ElementKind::Table(t) => crate::widgets::internal::measure_table(t),
        ElementKind::Tabs(t) => measure_tabs(t),
        ElementKind::DraggableTabBar(t) => measure_draggable_tab_bar(t),
        ElementKind::Component(_) => (0, 0),
        ElementKind::Group(g) => min_size_constrained(g.child.as_ref(), max_w, max_h),
        ElementKind::EffectScope(scope) => measure_effect_scope(scope, max_w, max_h),
        ElementKind::Animated(animated) => measure_animated(animated, max_w, max_h),
        ElementKind::DragSource(source) => {
            measure_drag_source(source, el.key.as_ref(), max_w, max_h)
        }
        ElementKind::DropTarget(target) => measure_drop_target(target, max_w, max_h),
        ElementKind::MouseRegion(region) => measure_mouse_region(region, max_w, max_h),
        ElementKind::ScrollView(sv) => measure_scroll_view(&sv.props, &sv.children, max_w),
        ElementKind::PanView(pan) => measure_pan_view(pan, max_w, max_h),
        ElementKind::VStack(vs) => {
            measure_stack(&vs.props, &vs.children, Axis::Vertical, max_h, max_w)
        }
        ElementKind::HStack(hs) => {
            measure_stack(&hs.props, &hs.children, Axis::Horizontal, max_w, max_h)
        }
        ElementKind::Grid(g) => measure_grid(&g.props, &g.items, max_w, max_h),
        ElementKind::Flow(flow) => measure_flow(flow, max_w, max_h),
        ElementKind::Canvas(canvas) => measure_canvas(canvas, max_w, max_h),
        ElementKind::ZStack(zs) => crate::widgets::internal::measure_zstack(zs, max_w, max_h),
        ElementKind::Center(center) => measure_center(center, max_w, max_h),
        ElementKind::CenterPin(cp) => measure_center_pin(cp, max_w, max_h),
        ElementKind::Frame(frame) => measure_frame(frame, max_w, max_h).outer_size(),
        ElementKind::Divider(d) => measure_divider(d),
        ElementKind::Spacer(s) => measure_spacer(s),
        ElementKind::Sparkline(s) => measure_sparkline(s),
        ElementKind::Chart(chart) => measure_chart(chart),
        ElementKind::Graph(graph) => measure_graph(graph),
        ElementKind::SequenceDiagram(sequence) => measure_sequence_diagram(sequence),
        ElementKind::Flowchart(flowchart) => measure_flowchart(flowchart),
        ElementKind::ClassDiagram(diagram) => measure_class_diagram(diagram),
        ElementKind::StateDiagram(diagram) => measure_state_diagram(diagram),
        ElementKind::ErDiagram(diagram) => measure_er_diagram(diagram),
        ElementKind::GanttDiagram(diagram) => measure_gantt_diagram(diagram),
        ElementKind::StatusBarLayout(layout) => measure_status_bar_layout(layout, max_w, max_h),
        ElementKind::Heatmap(h) => measure_heatmap(h),
        ElementKind::Checkbox(cb) => measure_checkbox(cb),
        ElementKind::ProgressBar(pb) => measure_progress_bar(pb),
        ElementKind::Slider(s) => measure_slider(s),
        ElementKind::Spinner(sp) => measure_spinner(sp),
        ElementKind::Splitter(splitter) => measure_splitter(splitter, max_w, max_h),
        ElementKind::DocumentView(dv) => measure_document_view(dv),
        ElementKind::ThemeProvider(tp) => min_size_constrained(&tp.child, max_w, max_h),
        ElementKind::ContextProvider(cp) => min_size_constrained(&cp.child, max_w, max_h),
        ElementKind::Memo(_) => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use crate::core::element::Element;
    use crate::layout::axis::Axis;
    use crate::style::Length;
    use crate::widgets::{
        ContentFormatter, DocumentView, Flow, FormatInput, FormattedBlock, FormattedDocument,
        FormattedLine, Overflow, Text,
    };

    use super::{intrinsic_main, min_size_constrained};

    #[test]
    fn intrinsic_main_reports_atomic_text_as_single_content_size() {
        let el: Element = Text::new("hello").into();

        assert_eq!(intrinsic_main(&el, Axis::Horizontal, None), (5, 5));
        assert_eq!(intrinsic_main(&el, Axis::Vertical, None), (1, 1));
    }

    #[test]
    fn intrinsic_main_reports_flow_min_and_max_content_widths() {
        let el: Element = Flow::new()
            .width(Length::Auto)
            .gap(1)
            .child(Text::new("short"))
            .child(Text::new("much longer"))
            .child(Text::new("mid"))
            .into();

        assert_eq!(intrinsic_main(&el, Axis::Horizontal, None), (11, 21));
    }

    #[test]
    fn min_size_cache_reuses_two_recent_constraint_sets() {
        let el: Element = Text::new("hello").overflow(Overflow::Wrap).into();

        assert_eq!(el.measure_cache.get(), [None, None]);

        assert_eq!(min_size_constrained(&el, Some(10), None), (5, 1));
        assert_eq!(min_size_constrained(&el, Some(10), None), (5, 1));
        assert_eq!(min_size_constrained(&el, Some(3), None), (3, 2));

        let slots = el.measure_cache.get();
        assert_eq!(
            slots[0].map(|entry| (entry.max_w, entry.max_h, entry.size)),
            Some((Some(3), None, (3, 2)))
        );
        assert_eq!(
            slots[1].map(|entry| (entry.max_w, entry.max_h, entry.size)),
            Some((Some(10), None, (5, 1)))
        );
    }

    #[test]
    fn min_size_cache_is_cleared_when_layout_constraints_change() {
        let base = Text::new("hello").into();
        assert_eq!(min_size_constrained(&base, Some(10), None), (5, 1));
        assert_ne!(base.measure_cache.get(), [None, None]);

        let changed = base.clone().max_width(crate::style::Length::Px(4));
        assert_eq!(changed.measure_cache.get(), [None, None]);
        // Auto mode now wraps when constrained: "hello" (5) at max_w=4 → 2 lines.
        assert_eq!(min_size_constrained(&changed, Some(10), None), (4, 2));
    }

    #[derive(Clone)]
    struct CountingFormatter {
        calls: Rc<Cell<usize>>,
    }

    impl ContentFormatter for CountingFormatter {
        fn clone_box(&self) -> Box<dyn ContentFormatter> {
            Box::new(self.clone())
        }

        fn format(&self, input: FormatInput<'_>) -> FormattedDocument {
            self.calls.set(self.calls.get() + 1);
            FormattedDocument {
                blocks: vec![FormattedBlock::Lines(vec![FormattedLine {
                    spans: vec![crate::style::Span::new(input.value)],
                    source_line: 0,
                    indent: 0,
                    links: Vec::new(),
                }])],
            }
        }

        fn measure_cache_key(&self) -> u64 {
            1
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn global_measure_cache_reuses_across_fresh_equal_elements() {
        let calls = Rc::new(Cell::new(0));
        let a: Element = DocumentView::new("shared")
            .formatter(CountingFormatter {
                calls: calls.clone(),
            })
            .border(false)
            .scrollbar(false)
            .width(crate::style::Length::Px(10))
            .height(crate::style::Length::Auto)
            .into();
        let b: Element = DocumentView::new("shared")
            .formatter(CountingFormatter {
                calls: calls.clone(),
            })
            .border(false)
            .scrollbar(false)
            .width(crate::style::Length::Px(10))
            .height(crate::style::Length::Auto)
            .into();

        assert_eq!(min_size_constrained(&a, Some(10), None), (10, 1));
        assert_eq!(min_size_constrained(&b, Some(10), None), (10, 1));
        assert_eq!(calls.get(), 1);
        assert!(a.measure_cache.get()[0].is_some());
        assert!(b.measure_cache.get()[0].is_some());
    }
}
