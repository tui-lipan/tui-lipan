//! Stack layout computation (VStack, HStack).

mod compute;
mod justify;
mod scroll;
mod types;

use crate::core::component::FocusContext;
use crate::core::element::{Element, ElementKind};
use crate::style::{Align, Length, Rect};
use crate::widgets::internal::StackProps;

use self::types::StackMeasuredSize;
use super::axis::{Axis, align_x, align_y, requested_cross_axis};
use super::measure::min_size_constrained;

pub(crate) use compute::{StackLayoutParams, compute_stack_layout};
pub(crate) use justify::apply_justify;
pub(crate) use scroll::{
    ScrollVirtualLayoutParams, VIRTUAL_THRESHOLD, layout_scroll_content,
    layout_scroll_content_virtual, make_scroll_content_layout, position_scroll_content_rects,
    scroll_rect_gaps_after, sync_virtual_cache_entry_widths,
};
pub(crate) use types::ScrollContentLayout;

/// Result of filtering portal children from a child slice.
///
/// When no portals are present, `refs` is empty and callers should use the
/// original `&[Element]` directly (zero-cost fast path).  When portals exist,
/// `refs` contains borrowed non-portal children.
pub(super) struct PortalFilter<'a> {
    /// Non-portal child references.  Empty when `has_portals` is false.
    pub refs: Vec<&'a Element>,
    /// Whether portals were found.
    pub has_portals: bool,
}

impl<'a> PortalFilter<'a> {
    /// Scan `children` for portals and, if any exist, collect non-portal refs.
    pub fn new(children: &'a [Element]) -> Self {
        let has_portals = children
            .iter()
            .any(|c| matches!(c.kind, ElementKind::Portal(_)));
        let refs = if has_portals {
            children
                .iter()
                .filter(|child| !matches!(child.kind, ElementKind::Portal(_)))
                .collect()
        } else {
            Vec::new()
        };
        Self { refs, has_portals }
    }

    /// Number of children that participate in layout.
    #[inline]
    pub fn layout_count(&self, original_len: usize) -> usize {
        if self.has_portals {
            self.refs.len()
        } else {
            original_len
        }
    }
}

/// Return a zero-sized `Rect` anchored at `(x, y)`.
#[inline]
pub(super) fn zero_rect(x: i16, y: i16) -> Rect {
    Rect { x, y, w: 0, h: 0 }
}

/// Returns `true` if `child` is a portal element.
#[inline]
pub(super) fn is_portal(child: &Element) -> bool {
    matches!(child.kind, ElementKind::Portal(_))
}

fn measured_cross_for_layout(
    child: &Element,
    axis: Axis,
    allocated_main: u16,
    measured: Option<StackMeasuredSize>,
    max_cross: u16,
) -> u16 {
    let cross_len = requested_cross_axis(child, axis);
    let content_cross = if !matches!(cross_len, Length::Auto) {
        0
    } else if let Some(measured) = measured {
        // Reflowing children have cross sizes that depend on final main-axis
        // allocation. Even if the cached main matches, the cached cross may have
        // come from an earlier wrap step, so always re-measure them.
        if measured.main_axis(axis) == allocated_main && !child.layout_constraints().reflows {
            measured.cross_axis(axis)
        } else {
            let (w, h) = match axis {
                Axis::Vertical => {
                    min_size_constrained(child, Some(max_cross), Some(allocated_main))
                }
                Axis::Horizontal => {
                    min_size_constrained(child, Some(allocated_main), Some(max_cross))
                }
            };
            match axis {
                Axis::Vertical => w,
                Axis::Horizontal => h,
            }
        }
    } else {
        let (w, h) = match axis {
            Axis::Vertical => min_size_constrained(child, Some(max_cross), Some(allocated_main)),
            Axis::Horizontal => min_size_constrained(child, Some(allocated_main), Some(max_cross)),
        };
        match axis {
            Axis::Vertical => w,
            Axis::Horizontal => h,
        }
    };

    cross_len.resolve(max_cross, content_cross)
}

/// Return the main-axis layout constraints for a child element.
///
/// Returns `(min_main, collapse_main, force_compact, focus_min_main)`.
/// `min_main` is a `Length` - callers must resolve it against the available
/// main-axis size before comparing it to pixel sizes.
pub(crate) fn axis_constraints(child: &Element, axis: Axis) -> (Length, Option<u16>, bool, u16) {
    let layout = child.layout_constraints();
    match axis {
        Axis::Vertical => (
            layout.min_h,
            layout.collapse_h,
            layout.force_compact,
            layout.focus_min_h,
        ),
        Axis::Horizontal => (
            layout.min_w,
            layout.collapse_w,
            layout.force_compact,
            layout.focus_min_w,
        ),
    }
}

pub(crate) fn layout_vstack(
    props: &StackProps,
    children: &[Element],
    bounds: Rect,
    focus: Option<&FocusContext>,
    pinned_key: Option<&str>,
) -> Vec<Rect> {
    if children.is_empty() {
        return Vec::new();
    }

    let filter = PortalFilter::new(children);
    let layout_count = filter.layout_count(children.len());

    if layout_count == 0 {
        return children
            .iter()
            .map(|_| zero_rect(bounds.x, bounds.y))
            .collect();
    }

    // compute_stack_layout is generic over C: Borrow<Element>.
    // No-portals path passes &[Element] directly (zero-cost).
    // Portals path passes &[&Element] (cheap pointer copies, no deep clones).
    let main_layout = if filter.has_portals {
        compute_stack_layout(StackLayoutParams {
            props,
            children: &filter.refs,
            axis: Axis::Vertical,
            available: bounds.h,
            available_cross: Some(bounds.w),
            focus,
            pinned_key,
            intrinsic_main_axis: false,
        })
    } else {
        compute_stack_layout(StackLayoutParams {
            props,
            children,
            axis: Axis::Vertical,
            available: bounds.h,
            available_cross: Some(bounds.w),
            focus,
            pinned_key,
            intrinsic_main_axis: false,
        })
    };
    let (offset, gaps) = apply_justify(
        props.justify,
        bounds.h,
        &main_layout.sizes,
        &main_layout.gaps,
        &main_layout.join_overlaps,
    );
    let mut y = bounds.y.saturating_add(offset);
    let mut out = Vec::with_capacity(children.len());
    let mut layout_idx = 0usize;

    for child in children.iter() {
        if is_portal(child) {
            out.push(zero_rect(bounds.x, bounds.y));
            continue;
        }

        let layout = child.layout_constraints();
        let mut h = main_layout.sizes[layout_idx];
        let remaining = bounds.h.saturating_sub((y.saturating_sub(bounds.y)) as u16);
        h = h.min(remaining);
        let mut w = measured_cross_for_layout(
            child,
            Axis::Vertical,
            h,
            main_layout
                .measured_sizes
                .get(layout_idx)
                .copied()
                .flatten(),
            bounds.w,
        )
        .max(layout.min_w.resolve_as_min(bounds.w))
        .min(bounds.w);

        if props.align == Align::Stretch {
            w = bounds.w;
        }

        let x = align_x(bounds, w, props.align);

        out.push(Rect { x, y, w, h });

        y = y.saturating_add(h as i16);
        if layout_idx < gaps.len() {
            y = y.saturating_add(gaps[layout_idx] as i16);
            if main_layout
                .join_overlaps
                .get(layout_idx)
                .copied()
                .unwrap_or(false)
            {
                y = y.saturating_sub(1);
            }
        }
        layout_idx = layout_idx.saturating_add(1);
    }

    out
}

pub(crate) fn layout_hstack(
    props: &StackProps,
    children: &[Element],
    bounds: Rect,
    focus: Option<&FocusContext>,
) -> Vec<Rect> {
    if children.is_empty() {
        return Vec::new();
    }

    let filter = PortalFilter::new(children);
    let layout_count = filter.layout_count(children.len());

    if layout_count == 0 {
        return children
            .iter()
            .map(|_| zero_rect(bounds.x, bounds.y))
            .collect();
    }

    let main_layout = if filter.has_portals {
        compute_stack_layout(StackLayoutParams {
            props,
            children: &filter.refs,
            axis: Axis::Horizontal,
            available: bounds.w,
            available_cross: Some(bounds.h),
            focus,
            pinned_key: None,
            intrinsic_main_axis: false,
        })
    } else {
        compute_stack_layout(StackLayoutParams {
            props,
            children,
            axis: Axis::Horizontal,
            available: bounds.w,
            available_cross: Some(bounds.h),
            focus,
            pinned_key: None,
            intrinsic_main_axis: false,
        })
    };
    #[cfg(feature = "diff-view")]
    let _split_wrap_dual_pass = crate::widgets::SplitWrapDualPass::begin_measure(
        children
            .iter()
            .filter(|child| !is_portal(child))
            .zip(main_layout.sizes.iter().copied()),
        Some(bounds.h),
    );

    let (offset, gaps) = apply_justify(
        props.justify,
        bounds.w,
        &main_layout.sizes,
        &main_layout.gaps,
        &main_layout.join_overlaps,
    );
    let mut x = bounds.x.saturating_add(offset);
    let mut out = Vec::with_capacity(children.len());
    let mut layout_idx = 0usize;

    for child in children.iter() {
        if is_portal(child) {
            out.push(zero_rect(bounds.x, bounds.y));
            continue;
        }

        let layout = child.layout_constraints();
        let remaining = bounds.w.saturating_sub((x.saturating_sub(bounds.x)) as u16);
        let w = main_layout.sizes[layout_idx];
        let w = w.min(remaining);
        let mut h = measured_cross_for_layout(
            child,
            Axis::Horizontal,
            w,
            main_layout
                .measured_sizes
                .get(layout_idx)
                .copied()
                .flatten(),
            bounds.h,
        )
        .max(layout.min_h.resolve_as_min(bounds.h))
        .min(bounds.h);

        if props.align == Align::Stretch {
            h = bounds.h;
        }

        let y = align_y(bounds, h, props.align);

        out.push(Rect { x, y, w, h });

        x = x.saturating_add(w as i16);
        if layout_idx < gaps.len() {
            let remaining = bounds.w.saturating_sub((x.saturating_sub(bounds.x)) as u16);
            let gap = gaps[layout_idx].min(remaining);
            x = x.saturating_add(gap as i16);
            if main_layout
                .join_overlaps
                .get(layout_idx)
                .copied()
                .unwrap_or(false)
            {
                x = x.saturating_sub(1);
            }
        }
        layout_idx = layout_idx.saturating_add(1);
    }

    out
}
