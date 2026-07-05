//! Axis-related layout utilities.

use crate::core::component::FocusContext;
use crate::core::element::{Element, ElementKind};
use crate::style::{Align, Length, Rect, Size};

/// Layout axis for stack-based containers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Axis {
    Vertical,
    Horizontal,
}

/// Resolve a Size to a concrete pixel value.
pub(crate) fn resolve_size(size: Size, available: u16, content: u16) -> u16 {
    match size {
        Size::Auto => content.min(available),
        Size::Fixed(px) => px.min(available),
        Size::Percent(p) => {
            let p = p.min(100);
            ((available as u32).saturating_mul(p as u32) / 100).min(u16::MAX as u32) as u16
        }
    }
}

/// Pick the component of a `(width, height)` pair along the given axis.
fn pick_axis((w, h): (Length, Length), axis: Axis) -> Length {
    match axis {
        Axis::Horizontal => w,
        Axis::Vertical => h,
    }
}

/// Pick the cross-axis component (perpendicular to `axis`).
fn pick_cross_axis(dims: (Length, Length), axis: Axis) -> Length {
    match axis {
        Axis::Horizontal => dims.1,
        Axis::Vertical => dims.0,
    }
}

/// Get the requested size along the main axis for an element.
pub(crate) fn requested_main_axis(
    el: &Element,
    axis: Axis,
    focus: Option<&FocusContext>,
) -> Length {
    // Handle special cases that need delegation or per-axis logic.
    match &el.kind {
        ElementKind::Frame(frame) => {
            return match axis {
                Axis::Vertical => {
                    if !is_focus_protected(el, focus) {
                        frame.props.unfocused_height.unwrap_or(frame.props.height)
                    } else {
                        frame.props.height
                    }
                }
                Axis::Horizontal => frame.props.width,
            };
        }
        ElementKind::Group(g) => return requested_main_axis(g.child.as_ref(), axis, focus),
        ElementKind::EffectScope(scope) => {
            return scope
                .child
                .as_deref()
                .map(|child| requested_main_axis(child, axis, focus))
                .unwrap_or(Length::Auto);
        }
        ElementKind::Animated(animated) => {
            return match axis {
                Axis::Vertical => animated
                    .layout_height
                    .or(animated.height)
                    .unwrap_or_else(|| requested_main_axis(animated.child.as_ref(), axis, focus)),
                Axis::Horizontal => requested_main_axis(animated.child.as_ref(), axis, focus),
            };
        }
        ElementKind::DragSource(source) => {
            return source
                .child
                .as_deref()
                .map(|child| requested_main_axis(child, axis, focus))
                .unwrap_or(Length::Auto);
        }
        ElementKind::DropTarget(target) => {
            return target
                .child
                .as_deref()
                .map(|child| requested_main_axis(child, axis, focus))
                .unwrap_or(Length::Auto);
        }
        ElementKind::MouseRegion(region) => {
            return region
                .child
                .as_deref()
                .map(|child| requested_main_axis(child, axis, focus))
                .unwrap_or(Length::Auto);
        }
        ElementKind::Popover(p) => {
            return requested_main_axis(p.trigger.as_ref(), axis, focus);
        }
        ElementKind::DocumentView(doc) => {
            return match axis {
                Axis::Horizontal => doc.width,
                Axis::Vertical => doc.resolved_height(),
            };
        }
        ElementKind::Flow(flow) => {
            return match axis {
                Axis::Horizontal => flow.width,
                Axis::Vertical => flow.height,
            };
        }
        ElementKind::ThemeProvider(tp) => return requested_main_axis(&tp.child, axis, focus),
        ElementKind::ContextProvider(cp) => return requested_main_axis(&cp.child, axis, focus),
        _ => {}
    }

    // Standard case: use dimensions().
    if let Some(dims) = el.kind.dimensions() {
        return pick_axis(dims, axis);
    }

    Length::Auto
}

/// Get the requested size along the cross axis for an element.
pub(crate) fn requested_cross_axis(el: &Element, axis: Axis) -> Length {
    // Handle special cases that need delegation.
    match &el.kind {
        ElementKind::Frame(frame) => {
            return match axis {
                Axis::Vertical => frame.props.width,
                Axis::Horizontal => frame.props.height,
            };
        }
        ElementKind::Group(g) => return requested_cross_axis(g.child.as_ref(), axis),
        ElementKind::EffectScope(scope) => {
            return scope
                .child
                .as_deref()
                .map(|child| requested_cross_axis(child, axis))
                .unwrap_or(Length::Auto);
        }
        ElementKind::Animated(animated) => {
            return requested_cross_axis(animated.child.as_ref(), axis);
        }
        ElementKind::DragSource(source) => {
            return source
                .child
                .as_deref()
                .map(|child| requested_cross_axis(child, axis))
                .unwrap_or(Length::Auto);
        }
        ElementKind::DropTarget(target) => {
            return target
                .child
                .as_deref()
                .map(|child| requested_cross_axis(child, axis))
                .unwrap_or(Length::Auto);
        }
        ElementKind::MouseRegion(region) => {
            return region
                .child
                .as_deref()
                .map(|child| requested_cross_axis(child, axis))
                .unwrap_or(Length::Auto);
        }
        ElementKind::Popover(p) => return requested_cross_axis(p.trigger.as_ref(), axis),
        ElementKind::Flow(flow) => {
            return match axis {
                Axis::Horizontal => flow.height,
                Axis::Vertical => flow.width,
            };
        }
        ElementKind::ThemeProvider(tp) => return requested_cross_axis(&tp.child, axis),
        ElementKind::ContextProvider(cp) => return requested_cross_axis(&cp.child, axis),
        _ => {}
    }

    // Standard case: use dimensions().
    if let Some(dims) = el.kind.dimensions() {
        return pick_cross_axis(dims, axis);
    }

    Length::Auto
}

/// Align a child horizontally within bounds.
pub(crate) fn align_x(bounds: Rect, child_w: u16, align: Align) -> i16 {
    match align {
        Align::Start | Align::Stretch => bounds.x,
        Align::Center => bounds
            .x
            .saturating_add((bounds.w.saturating_sub(child_w) / 2) as i16),
        Align::End => bounds
            .x
            .saturating_add(bounds.w.saturating_sub(child_w) as i16),
    }
}

/// Align a child vertically within bounds.
pub(crate) fn align_y(bounds: Rect, child_h: u16, align: Align) -> i16 {
    match align {
        Align::Start | Align::Stretch => bounds.y,
        Align::Center => bounds
            .y
            .saturating_add((bounds.h.saturating_sub(child_h) / 2) as i16),
        Align::End => bounds
            .y
            .saturating_add(bounds.h.saturating_sub(child_h) as i16),
    }
}

/// Check if an element is protected from collapsing due to focus.
pub(crate) fn is_focus_protected(child: &Element, focus: Option<&FocusContext>) -> bool {
    let Some(key) = child.key.as_ref() else {
        return false;
    };
    focus.is_some_and(|ctx| ctx.has_focus_within_key(key))
}
