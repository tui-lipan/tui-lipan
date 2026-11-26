//! Widget measurement functions.

use std::cell::{Cell, RefCell};

use crate::core::element::{Element, ElementKind, MeasureCacheEntry, MeasureContext};
#[cfg(feature = "big-text")]
use crate::widgets::internal::measure_big_text;
#[cfg(feature = "image")]
use crate::widgets::internal::measure_image_constrained;
#[cfg(feature = "terminal")]
use crate::widgets::internal::measure_terminal;
use crate::widgets::internal::{
    drag_source_is_snapshot_collapsed, measure_animated_within, measure_ascii_canvas,
    measure_button, measure_canvas, measure_center, measure_center_pin, measure_chart,
    measure_checkbox, measure_class_diagram, measure_divider, measure_document_view,
    measure_document_view_constrained, measure_drag_source_within, measure_draggable_tab_bar,
    measure_drop_target, measure_effect_scope, measure_er_diagram, measure_flow, measure_flowchart,
    measure_frame, measure_frame_within, measure_gantt_diagram, measure_graph, measure_grid,
    measure_heatmap, measure_hex_area, measure_input, measure_mouse_region, measure_pan_view,
    measure_progress_bar, measure_scroll_view, measure_sequence_diagram, measure_slider,
    measure_spacer, measure_sparkline, measure_spinner, measure_splitter, measure_stack,
    measure_state_diagram, measure_status_bar_layout, measure_tabs, measure_text_area,
    measure_text_area_constrained,
};

use crate::style::{Edge, Length};
use crate::widgets::document_view::layout::{
    ParentIntegrated, measure_document_view_constrained_with, measure_document_view_with,
};
use crate::widgets::{DecorationPlacement, Frame};

use super::axis::{Axis, requested_main_axis};
use super::hash::element_layout_hash;

/// Resolve a widget's requested [`Length`] against measured content during measurement.
///
/// - `Auto` / `Flex`: keep content size (flex share is assigned at reconcile).
/// - `Px`: use the fixed size so Auto parents can shrink-wrap to it.
/// - `Percent`: resolve against parent-available space when known; otherwise keep content.
fn resolve_requested_measure(len: Length, content: u16, available: u16) -> u16 {
    match len {
        Length::Auto | Length::Flex(_) => content,
        Length::Px(px) => px,
        Length::Percent(_) if available == u16::MAX => content,
        Length::Percent(percent) => Length::Percent(percent).resolve(available, content),
    }
}

/// Bound used to tighten content measurement for wrapping widgets (`Text`, `TextArea`, …).
fn requested_measure_bound(len: Length, available: u16) -> Option<u16> {
    match len {
        Length::Px(px) => Some(px),
        Length::Percent(_) if available == u16::MAX => None,
        Length::Percent(percent) => Some(Length::Percent(percent).resolve(available, available)),
        Length::Auto | Length::Flex(_) => None,
    }
}

type GlobalMeasureCacheKey = (u64, Option<u16>, Option<u16>, u16, u16, MeasureContext, u64);
type GlobalMeasureCache = crate::utils::gen_cache::GenerationalCache<
    GlobalMeasureCacheKey,
    (u16, u16),
    rustc_hash::FxBuildHasher,
>;

thread_local! {
    static GLOBAL_MEASURE_CACHE: RefCell<GlobalMeasureCache> =
        RefCell::new(GlobalMeasureCache::new(GLOBAL_MEASURE_CACHE_MAX_ENTRIES));
    static CURRENT_MEASURE_CONTEXT: Cell<MeasureContext> = const {
        Cell::new(MeasureContext::NONE)
    };
}

/// Per-generation cap. Large scroll lists × width probes would otherwise thrash a
/// tiny cache that cleared entirely on overflow; the generational cache keeps a
/// second generation so resize sweeps keep hitting (see [`crate::utils::gen_cache`]).
const GLOBAL_MEASURE_CACHE_MAX_ENTRIES: usize = 16_384;

struct MeasureContextGuard(MeasureContext);

impl Drop for MeasureContextGuard {
    fn drop(&mut self) {
        CURRENT_MEASURE_CONTEXT.with(|context| context.set(self.0));
    }
}

fn with_measure_context<T>(context: MeasureContext, f: impl FnOnce() -> T) -> T {
    let previous = CURRENT_MEASURE_CONTEXT.with(|current| current.replace(context));
    let _guard = MeasureContextGuard(previous);
    f()
}

fn current_measure_context() -> MeasureContext {
    CURRENT_MEASURE_CONTEXT.with(Cell::get)
}

#[cfg(feature = "image")]
fn current_image_measure_epoch() -> u64 {
    crate::widgets::internal::image_measure_invalidation_epoch()
}

#[cfg(not(feature = "image"))]
fn current_image_measure_epoch() -> u64 {
    0
}

pub(crate) fn with_frame_measure_context<T>(frame: &Frame, f: impl FnOnce() -> T) -> T {
    let parent = current_measure_context();
    let border_v = frame.props.has_border() && frame.props.border_edges.has_right();
    let border_h = frame.props.has_border() && frame.props.border_edges.has_bottom();
    let decoration_v = frame.props.decorations.iter().any(|decoration| {
        decoration.placement == DecorationPlacement::Border
            && matches!(decoration.edge, Edge::Left | Edge::Right)
    });
    let decoration_h = frame.props.decorations.iter().any(|decoration| {
        decoration.placement == DecorationPlacement::Border
            && matches!(decoration.edge, Edge::Top | Edge::Bottom)
    });
    let context = MeasureContext {
        ancestor_integrated_v: parent.ancestor_integrated_v || border_v || decoration_v,
        ancestor_integrated_h: parent.ancestor_integrated_h || border_h || decoration_h,
    };

    with_measure_context(context, f)
}

#[cfg(feature = "diff-view")]
fn element_has_split_wrap_measure_state(el: &Element) -> bool {
    crate::widgets::element_subtree_has_split_wrap_sync(el)
}

#[cfg(not(feature = "diff-view"))]
fn element_has_split_wrap_measure_state(_el: &Element) -> bool {
    false
}

/// Hidden source-slot drags must skip the per-element measure cache when a
/// collapse hint is active, because the cached size would be returned before
/// `measure_drag_source` has a chance to check the hint. A source that stays
/// visible does not change size, so its cache remains valid.
fn drag_source_should_skip_measure_cache(el: &Element) -> bool {
    if let ElementKind::DragSource(source) = &el.kind {
        source.preview.uses_source_slot()
            && source.hide_source_while_dragging
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
    let mut target = el;
    while let Some(inner) = transparent_measure_child(target) {
        target = inner;
    }
    if let ElementKind::Flow(flow) = &target.kind
        && axis == Axis::Horizontal
    {
        let constraints = el.layout_constraints();
        let max_content = min_size_constrained(el, None, cross).0;
        let min_content = constraints.clamp_width(flow_intrinsic_min_width(flow), u16::MAX);
        return (min_content.min(max_content), max_content);
    }
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
    // Use the parent's offered size as the reference for resolving Percent constraints.
    // When no parent bound is given, u16::MAX means "unconstrained".
    min_size_within(
        el,
        max_w,
        max_h,
        max_w.unwrap_or(u16::MAX),
        max_h.unwrap_or(u16::MAX),
    )
}

/// Measure `el` within `max_w`/`max_h`, resolving its relative constraints against
/// `avail_w`/`avail_h` instead of against the bound.
///
/// Overlay and `ScrollView` sizing need the two apart: a child capped at `Percent(50)` resolves
/// that cap against the parent/viewport, then is measured at its final, already-capped width.
/// Passing that width as the parent's offered size would apply the percentage a second time.
pub(crate) fn min_size_within(
    el: &Element,
    max_w: Option<u16>,
    max_h: Option<u16>,
    avail_w: u16,
    avail_h: u16,
) -> (u16, u16) {
    let measure_context = current_measure_context();
    let image_measure_epoch = current_image_measure_epoch();
    let split_wrap_measure_state = element_has_split_wrap_measure_state(el);
    let skip_local_cache = split_wrap_measure_state || drag_source_should_skip_measure_cache(el);
    let skip_global_cache = drag_source_should_skip_measure_cache(el);

    if !skip_local_cache {
        for entry in el.measure_cache.get().into_iter().flatten() {
            if entry.max_w == max_w
                && entry.max_h == max_h
                && entry.avail_w == avail_w
                && entry.avail_h == avail_h
                && entry.context == measure_context
                && entry.image_measure_epoch == image_measure_epoch
            {
                return entry.size;
            }
        }
    }

    let global_cache_key = if !skip_global_cache {
        element_layout_hash(el).map(|hash| {
            (
                hash,
                max_w,
                max_h,
                avail_w,
                avail_h,
                measure_context,
                image_measure_epoch,
            )
        })
    } else {
        None
    };
    if let Some(key) = global_cache_key
        && let Some(size) = GLOBAL_MEASURE_CACHE.with(|cache| cache.borrow().get(&key).copied())
    {
        if !skip_local_cache {
            let new_entry = Some(MeasureCacheEntry {
                max_w,
                max_h,
                avail_w,
                avail_h,
                context: measure_context,
                image_measure_epoch,
                size,
            });
            let mut slots = el.measure_cache.get();
            slots[1] = slots[0];
            slots[0] = new_entry;
            el.measure_cache.set(slots);
        }
        return size;
    }

    // A wrapper measures its child against the bound alone inside `measure_uncached`,
    // which would lose distinct `avail_w`/`avail_h`. Preserve them by delegating directly.
    let size = if (max_w.unwrap_or(u16::MAX) != avail_w || max_h.unwrap_or(u16::MAX) != avail_h)
        && let Some(child) = same_size_child(el)
    {
        min_size_within(child, max_w, max_h, avail_w, avail_h)
    } else {
        measure_uncached(el, max_w, max_h, avail_w, avail_h, measure_context)
    };

    if !skip_local_cache {
        let new_entry = Some(MeasureCacheEntry {
            max_w,
            max_h,
            avail_w,
            avail_h,
            context: measure_context,
            image_measure_epoch,
            size,
        });
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

/// The child of a wrapper that always measures exactly as that child does.
fn same_size_child(el: &Element) -> Option<&Element> {
    if el.layout.max_w.is_some()
        || el.layout.max_h.is_some()
        || crate::style::MinBound::from_length(el.layout.min_w).is_set()
        || crate::style::MinBound::from_length(el.layout.min_h).is_set()
    {
        return None;
    }
    match &el.kind {
        ElementKind::Group(g) => Some(g.child.as_ref()),
        ElementKind::ThemeProvider(tp) => Some(&tp.child),
        ElementKind::ContextProvider(cp) => Some(&cp.child),
        ElementKind::EffectScope(scope) => scope.child.as_deref(),
        ElementKind::Animated(animated)
            if animated.visibility.is_none()
                && animated.visibility_layout.is_none()
                && animated.height.is_none()
                && animated.layout_height.is_none() =>
        {
            Some(animated.child.as_ref())
        }
        _ => None,
    }
}

/// The inner child of a transparent wrapper whose own measurement simply delegates to that child.
///
/// Peeling all transparent wrappers in `measure_uncached` ensures that merged min/max constraints
/// across the entire wrapper chain are resolved once against the parent's `(avail_w, avail_h)`
/// basis before measuring the wrapped target widget.
fn transparent_measure_child(el: &Element) -> Option<&Element> {
    match &el.kind {
        ElementKind::Group(g) => Some(g.child.as_ref()),
        ElementKind::ThemeProvider(tp) => Some(&tp.child),
        ElementKind::ContextProvider(cp) => Some(&cp.child),
        ElementKind::EffectScope(scope) => scope.child.as_deref(),
        ElementKind::DropTarget(target) => target.child.as_deref(),
        ElementKind::DragSource(source)
            if !drag_source_is_snapshot_collapsed(source, el.key.as_ref()) =>
        {
            source.child.as_deref()
        }
        ElementKind::MouseRegion(region) if region.fit_child => region.child.as_deref(),
        ElementKind::Animated(animated)
            if animated.visibility.is_none()
                && animated.visibility_layout.is_none()
                && animated.height.is_none()
                && animated.layout_height.is_none() =>
        {
            Some(animated.child.as_ref())
        }
        _ => None,
    }
}

/// Measure `el` using pre-merged `constraints` within `max_w`/`max_h`, resolving relative
/// bounds against `avail_w`/`avail_h`.
///
/// Used by wrapper reconcilers such as `Animated` that need to measure their child with the
/// same merged wrapper constraints and percentage basis as the main layout pass.
pub(crate) fn min_size_with_constraints(
    el: &Element,
    constraints: crate::style::EffectiveLayoutConstraints,
    max_w: Option<u16>,
    max_h: Option<u16>,
    avail_w: u16,
    avail_h: u16,
) -> (u16, u16) {
    measure_uncached_with_constraints(
        el,
        constraints,
        max_w,
        max_h,
        avail_w,
        avail_h,
        current_measure_context(),
    )
}

fn measure_uncached(
    el: &Element,
    max_w: Option<u16>,
    max_h: Option<u16>,
    avail_w: u16,
    avail_h: u16,
    measure_context: MeasureContext,
) -> (u16, u16) {
    measure_uncached_with_constraints(
        el,
        el.layout_constraints(),
        max_w,
        max_h,
        avail_w,
        avail_h,
        measure_context,
    )
}

fn measure_uncached_with_constraints(
    el: &Element,
    constraints: crate::style::EffectiveLayoutConstraints,
    max_w: Option<u16>,
    max_h: Option<u16>,
    avail_w: u16,
    avail_h: u16,
    measure_context: MeasureContext,
) -> (u16, u16) {
    // Explicit `.width(Length::Px/Percent)` / `.height(...)` must participate in
    // measurement, not only reconcile. Otherwise Auto/Center parents shrink-wrap to
    // content and the requested size is clamped away (see Input in form_validation).
    let req_w = requested_main_axis(el, Axis::Horizontal, None);
    let req_h = requested_main_axis(el, Axis::Vertical, None);

    // Peel all transparent wrappers so merged min/max constraints from the entire chain
    // apply once against `(avail_w, avail_h)` directly when measuring the inner target widget.
    // This keeps percentage constraints resolved against the parent's offered size (never
    // double-applied) and lets an outer `min_*` override an inner `max_*` during measurement,
    // matching reconciliation.
    let mut target = el;
    while let Some(inner) = transparent_measure_child(target) {
        target = inner;
    }

    // Resolve the merged min/max constraints to pixels, then take the tighter of
    // the resolved max, requested size, and the parent's offered size.
    // Minimum constraints take precedence over both `max_*` and explicit requested bounds,
    // while still remaining capped by the parent's `max_w`/`max_h`.
    let element_min_w = constraints.min_w.resolve(avail_w);
    let element_min_h = constraints.min_h.resolve(avail_h);
    let requested_max_w = requested_measure_bound(req_w, avail_w).map(|req| req.max(element_min_w));
    let requested_max_h = requested_measure_bound(req_h, avail_h).map(|req| req.max(element_min_h));
    let element_max_w = constraints
        .max_w
        .resolve(avail_w)
        .map(|max| max.max(element_min_w));
    let element_max_h = constraints
        .max_h
        .resolve(avail_h)
        .map(|max| max.max(element_min_h));
    let effective_max_w = [element_max_w, max_w, requested_max_w]
        .into_iter()
        .flatten()
        .min();
    let effective_max_h = [element_max_h, max_h, requested_max_h]
        .into_iter()
        .flatten()
        .min();

    let (w, h) = if let ElementKind::TextArea(t) = &target.kind {
        measure_text_area_constrained(t, effective_max_w)
    } else if let ElementKind::DocumentView(dv) = &target.kind {
        if measure_context == MeasureContext::NONE {
            measure_document_view_constrained(dv, effective_max_w)
        } else {
            measure_document_view_constrained_with(
                dv,
                effective_max_w,
                ParentIntegrated {
                    v: measure_context.ancestor_integrated_v,
                    h: measure_context.ancestor_integrated_h,
                },
            )
        }
    } else if let ElementKind::Frame(frame) = &target.kind {
        // The effective bound already holds the frame's own percentage size; the frame resolves
        // that percentage against the parent's offered size and stays within the bound.
        let basis = |avail: u16| (avail != u16::MAX).then_some(avail);
        measure_frame_within(
            frame,
            effective_max_w,
            effective_max_h,
            basis(avail_w),
            basis(avail_h),
        )
        .outer_size()
    } else if let ElementKind::Animated(animated) = &target.kind {
        // Preserve the parent's `(avail_w, avail_h)` percentage basis and merged horizontal
        // constraints when measuring `Animated`'s child (so percentage bounds are never applied
        // twice and outer `min_width` overrides inner `max_width`), while retaining `Animated`'s
        // height/visibility override semantics.
        let overrides_height = animated.height.is_some()
            || (animated.collapse_visibility && animated.visibility.is_some());
        let mut child_constraints = animated.child.layout_constraints();
        child_constraints.min_w = constraints.min_w;
        child_constraints.max_w = constraints.max_w;
        if !overrides_height {
            child_constraints.min_h = constraints.min_h;
            child_constraints.max_h = constraints.max_h;
        }
        measure_animated_within(
            animated,
            effective_max_w,
            effective_max_h,
            avail_w,
            avail_h,
            |child, mw, mh, aw, ah| {
                measure_uncached_with_constraints(
                    child,
                    child_constraints,
                    mw,
                    mh,
                    aw,
                    ah,
                    measure_context,
                )
            },
        )
    } else if let ElementKind::DragSource(source) = &target.kind {
        measure_drag_source_within(
            source,
            target.key.as_ref(),
            effective_max_w,
            effective_max_h,
            avail_w,
            avail_h,
            |child, mw, mh, aw, ah| {
                measure_uncached_with_constraints(
                    child,
                    constraints,
                    mw,
                    mh,
                    aw,
                    ah,
                    measure_context,
                )
            },
        )
    } else {
        // Pass the effective (tighter) bounds so children are measured within
        // both the parent's available space and this element's own max constraints.
        min_size_unconstrained_constrained(target, effective_max_w, effective_max_h)
    };

    let w = resolve_requested_measure(req_w, w, avail_w);
    let h = resolve_requested_measure(req_h, h, avail_h);

    (
        constraints.clamp_width(w, avail_w),
        constraints.clamp_height(h, avail_h),
    )
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
        ElementKind::Image(i) => measure_image_constrained(i, max_w, max_h),
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
        ElementKind::Animated(animated) => measure_animated_within(
            animated,
            max_w,
            max_h,
            max_w.unwrap_or(u16::MAX),
            max_h.unwrap_or(u16::MAX),
            min_size_within,
        ),
        ElementKind::DragSource(source) => measure_drag_source_within(
            source,
            el.key.as_ref(),
            max_w,
            max_h,
            max_w.unwrap_or(u16::MAX),
            max_h.unwrap_or(u16::MAX),
            min_size_within,
        ),
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
        ElementKind::KeyCapture(capture) => crate::widgets::internal::measure_key_capture(capture),
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
        ElementKind::DocumentView(dv) if current_measure_context() == MeasureContext::NONE => {
            measure_document_view(dv)
        }
        ElementKind::DocumentView(dv) => {
            let context = current_measure_context();
            measure_document_view_with(
                dv,
                ParentIntegrated {
                    v: context.ancestor_integrated_v,
                    h: context.ancestor_integrated_h,
                },
            )
        }
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
        Button, Center, Checkbox, ContentFormatter, DocumentView, Flow, FormatInput,
        FormattedBlock, FormattedDocument, FormattedLine, Frame, Input, Overflow, Spacer, Text,
        VStack,
    };

    use super::{intrinsic_main, min_size_constrained};

    #[test]
    fn a_percentage_width_frame_measures_its_wrap_at_its_own_width() {
        // 80% of 40 is 32, leaving 30 inner columns: four 6-letter words a row, so two rows.
        // Resolving the 80% again against 32 would leave 23 columns and three rows.
        let frame: Element = Frame::new()
            .border(true)
            .width(Length::Percent(80))
            .height(Length::Auto)
            .child(
                Text::new("aaaaaa aaaaaa aaaaaa aaaaaa aaaaaa aaaaaa aaaaaa aaaaaa")
                    .overflow(Overflow::Wrap),
            )
            .into();
        // Lift the frame's floor at its unwrapped width so the percentage decides it.
        let frame = frame.min_width(Length::Px(0));

        assert_eq!(min_size_constrained(&frame, Some(40), None), (32, 4));
    }

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

    #[test]
    fn min_size_within_uses_local_and_global_caches_keyed_by_avail() {
        use super::min_size_within;
        use crate::core::element::IntoElement;
        use crate::layout::stack::layout_scroll_content;
        use crate::widgets::internal::StackProps;

        let calls = Rc::new(Cell::new(0));
        let make_doc = || -> Element {
            DocumentView::new("a".repeat(60))
                .formatter(CountingFormatter {
                    calls: calls.clone(),
                })
                .wrap(true)
                .border(false)
                .scrollbar(false)
                .width(Length::Flex(1))
                .height(Length::Auto)
                .max_width(Length::Percent(50))
        };

        let a = make_doc();
        let b = make_doc();

        // First call computes and populates local + global caches.
        assert_eq!(min_size_within(&a, Some(50), None, 100, u16::MAX), (50, 2));
        assert_eq!(calls.get(), 1);
        let slot = a.measure_cache.get()[0].expect("local cache populated");
        assert_eq!(
            (slot.max_w, slot.avail_w, slot.size),
            (Some(50), 100, (50, 2))
        );

        // Repeated call on same element hits local cache.
        assert_eq!(min_size_within(&a, Some(50), None, 100, u16::MAX), (50, 2));
        assert_eq!(calls.get(), 1);

        // Fresh equal element hits global cache (including through ScrollView layout).
        assert_eq!(min_size_within(&b, Some(50), None, 100, u16::MAX), (50, 2));
        assert_eq!(calls.get(), 1);
        assert!(b.measure_cache.get()[0].is_some());

        let c = make_doc();
        let layout = layout_scroll_content(&StackProps::default(), &[c], 100, 20, false);
        assert_eq!((layout.rects[0].w, layout.rects[0].h), (50, 2));
        assert_eq!(calls.get(), 1);

        // Same `max_w` with a different `avail_w` must not collide in either cache:
        // Percent(50) of 80 resolves to 40, not the cached 50 from avail_w = 100.
        assert_eq!(min_size_within(&b, Some(50), None, 80, u16::MAX), (40, 2));
        let slots = b.measure_cache.get();
        assert_eq!(
            slots[0].map(|entry| (entry.max_w, entry.avail_w, entry.size)),
            Some((Some(50), 80, (40, 2)))
        );
        assert_eq!(
            slots[1].map(|entry| (entry.max_w, entry.avail_w, entry.size)),
            Some((Some(50), 100, (50, 2)))
        );
    }

    #[test]
    fn explicit_px_width_is_honored_for_leaf_widgets() {
        let input: Element = Input::new("")
            .placeholder("Enter username")
            .width(Length::Px(40))
            .into();
        assert_eq!(min_size_constrained(&input, None, None).0, 40);

        let button: Element = Button::new("Go").width(Length::Px(24)).into();
        assert_eq!(min_size_constrained(&button, None, None).0, 24);

        let text: Element = Text::new("hi").width(Length::Px(12)).into();
        assert_eq!(min_size_constrained(&text, None, None).0, 12);

        let checkbox: Element = Checkbox::new(false)
            .label("Remember me")
            .width(Length::Px(30))
            .into();
        assert_eq!(min_size_constrained(&checkbox, None, None).0, 30);

        let spacer: Element = Spacer::new()
            .width(Length::Px(8))
            .height(Length::Px(3))
            .into();
        assert_eq!(min_size_constrained(&spacer, None, None), (8, 3));
    }

    #[test]
    fn explicit_percent_width_resolves_when_parent_bound_is_known() {
        let input: Element = Input::new("x").width(Length::Percent(50)).into();
        assert_eq!(min_size_constrained(&input, Some(80), None).0, 40);
    }

    #[test]
    fn unconstrained_percent_width_falls_back_to_content() {
        let input: Element = Input::new("hello").width(Length::Percent(50)).into();
        let (w, _) = min_size_constrained(&input, None, None);
        // placeholder/value content (+ caret), not a huge percent of u16::MAX
        assert!(w < 20, "expected content-sized width, got {w}");
    }

    #[test]
    fn flex_width_still_shrink_wraps_to_content() {
        let input: Element = Input::new("")
            .placeholder("Enter username")
            .width(Length::Flex(1))
            .into();
        let (w, _) = min_size_constrained(&input, None, None);
        assert!(w < 40, "Flex must not expand Auto parents, got {w}");
    }

    #[test]
    fn auto_vstack_and_center_expand_to_child_px_width() {
        let input: Element = Input::new("")
            .placeholder("Enter username")
            .width(Length::Px(40))
            .into();
        let vstack: Element = VStack::new()
            .width(Length::Auto)
            .child(input.clone())
            .into();
        assert_eq!(min_size_constrained(&vstack, None, None).0, 40);

        let centered: Element = Center::new().child(input).into();
        assert_eq!(min_size_constrained(&centered, None, None).0, 40);
    }

    #[test]
    fn text_px_width_tightens_wrap_measurement() {
        let text: Element = Text::new("abcdefghij")
            .overflow(Overflow::Wrap)
            .width(Length::Px(5))
            .into();
        assert_eq!(min_size_constrained(&text, None, None), (5, 2));
    }
}
