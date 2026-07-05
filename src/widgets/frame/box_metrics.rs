use crate::style::{Length, Padding, Rect};
use crate::widgets::frame::{DecorationPlacement, Edge, FrameNode};

/// Whether the frame joins with a neighbor on the left/top edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrameJoinOverlap {
    pub left: bool,
    pub top: bool,
}

/// Complete geometric plan for a [`Frame`](crate::widgets::Frame).
///
/// Captures every inset, rect, and alignment decision so that measure,
/// reconcile, render, and scrollbar code can share a single source of truth
/// instead of re-deriving values.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameGeometry {
    /// The full allocated rect (including outside decorations).
    pub outer_rect: Rect,
    /// After subtracting outside-decoration padding.
    pub frame_rect: Rect,
    /// After subtracting border (or same as `frame_rect` when borderless).
    /// Join overlap adjustments are already applied.
    pub body_rect: Rect,
    /// Final inner area where the child is placed, after padding,
    /// decoration insets, header, and status adjustments.
    pub content_rect: Rect,
    /// Rect reserved for the header element, if any.
    pub header_rect: Option<Rect>,
    /// Rect reserved for the status line in borderless mode, if any.
    pub status_rect: Option<Rect>,
    /// Whether the frame borders join with a neighbor.
    pub join_overlap: FrameJoinOverlap,
    /// Whether the frame renders a border.
    pub has_border: bool,
    /// Padding consumed by rendered border edges.
    pub border_padding: Padding,
    /// X-coordinate of an integrated vertical scrollbar track, if any.
    pub vscrollbar_track_x: Option<i16>,
    /// Y-coordinate of an integrated horizontal scrollbar track, if any.
    pub hscrollbar_track_y: Option<i16>,
    /// Padding inside the frame.
    pub padding: Padding,
    /// Outside decoration padding.
    pub decoration_outside: Padding,
    /// Inset for border-placement decorations when borderless.
    pub border_deco_inset: Padding,
}

impl FrameGeometry {
    /// Outer frame size `(width, height)`.
    pub fn outer_size(&self) -> (u16, u16) {
        (self.outer_rect.w, self.outer_rect.h)
    }

    /// Maximum inner size available for child measurement.
    pub fn inner_max_size(
        &self,
        max_w: Option<u16>,
        max_h: Option<u16>,
    ) -> (Option<u16>, Option<u16>) {
        let mut w = max_w.map(|mw| mw.saturating_sub(self.decoration_outside.horizontal()));
        let mut h = max_h.map(|mh| mh.saturating_sub(self.decoration_outside.vertical()));
        if self.has_border {
            w = w.map(|v| v.saturating_sub(self.border_padding.horizontal()));
            h = h.map(|v| v.saturating_sub(self.border_padding.vertical()));
        }
        w = w.map(|v| v.saturating_sub(self.padding.horizontal()));
        h = h.map(|v| v.saturating_sub(self.padding.vertical()));
        w = w.map(|v| v.saturating_sub(self.border_deco_inset.horizontal()));
        h = h.map(|v| v.saturating_sub(self.border_deco_inset.vertical()));
        (w, h)
    }
}

/// Compute the full geometric plan for a frame given its allocated rect.
///
/// `has_parent` controls whether `Length::Percent` constraints are re-resolved:
/// when `true` the parent has already resolved them into `rect`; when `false`
/// (root frame) they are resolved here.
pub(crate) fn compute_frame_geometry(
    props: &FrameNode,
    rect: Rect,
    join_overlap: FrameJoinOverlap,
    has_parent: bool,
) -> FrameGeometry {
    let decoration_outside = props.decoration_outside_padding();
    let border_deco_inset = props.decoration_border_content_inset();
    let has_border = props.has_border();
    let border_padding = props.border_padding();

    let frame_rect = rect.inset(decoration_outside);

    // Body rect: after border (with join adjustments)
    let mut body_rect = frame_rect;
    if has_border {
        body_rect = frame_rect.inset(border_padding);
        if props.join_frame {
            if props.border_edges.has_left() && join_overlap.left {
                body_rect.x = body_rect.x.saturating_sub(1);
                body_rect.w = body_rect.w.saturating_add(1);
            }
            if props.border_edges.has_top() && join_overlap.top {
                body_rect.y = body_rect.y.saturating_sub(1);
                body_rect.h = body_rect.h.saturating_add(1);
            }
        }
    }

    // Resolve width/height constraints for the inner area
    let mut inner = frame_rect;
    inner.w = match props.width {
        Length::Percent(_) if has_parent => frame_rect.w,
        len => len.resolve(frame_rect.w, frame_rect.w).min(frame_rect.w),
    };
    inner.h = match props.height {
        Length::Percent(_) if has_parent => frame_rect.h,
        len => len.resolve(frame_rect.h, frame_rect.h).min(frame_rect.h),
    };

    if has_border {
        inner = inner.inset(border_padding);
        if props.join_frame {
            if props.border_edges.has_left() && join_overlap.left {
                inner.x = inner.x.saturating_sub(1);
                inner.w = inner.w.saturating_add(1);
            }
            if props.border_edges.has_top() && join_overlap.top {
                inner.y = inner.y.saturating_sub(1);
                inner.h = inner.h.saturating_add(1);
            }
        }
    }

    let has_status =
        props.status.is_some() || props.status_center.is_some() || props.status_right.is_some();

    let mut content_rect = inner;
    if has_status && !has_border {
        content_rect.h = content_rect.h.saturating_sub(1);
    }

    content_rect = content_rect.inset(props.padding);
    content_rect = content_rect.inset(border_deco_inset);

    // Header rect
    let mut header_rect = None;
    let has_header = props.has_header && !props.compact && frame_rect.h > 0;
    if has_header {
        if has_border {
            let join_left = props.join_frame && props.border_edges.has_left() && join_overlap.left;
            let join_top = props.join_frame && props.border_edges.has_top() && join_overlap.top;
            let mut header = Rect {
                x: frame_rect.x.saturating_add(if join_left { 0 } else { 1 }),
                y: frame_rect.y.saturating_sub(if join_top { 1 } else { 0 }),
                w: frame_rect
                    .w
                    .saturating_sub(2)
                    .saturating_add(if join_left { 1 } else { 0 }),
                h: 1,
            };
            let pad = props.header_padding;
            header = header.inset(Padding {
                left: pad.left,
                right: pad.right,
                top: 0,
                bottom: 0,
            });
            if header.w > 0 && header.h > 0 {
                header_rect = Some(header);
            }
        } else if content_rect.h > 0 {
            let mut header = Rect {
                x: content_rect.x,
                y: content_rect.y,
                w: content_rect.w,
                h: 1,
            };
            let pad = props.header_padding;
            header = header.inset(Padding {
                left: pad.left,
                right: pad.right,
                top: 0,
                bottom: 0,
            });
            if header.w > 0 && header.h > 0 {
                header_rect = Some(header);
            }
            content_rect.y = content_rect.y.saturating_add(1);
            content_rect.h = content_rect.h.saturating_sub(1);
        }
    }

    // Status rect (for borderless frames)
    let mut status_rect = None;
    if has_status && !has_border {
        let mut status = Rect {
            x: inner.x,
            y: inner.y.saturating_add(inner.h as i16).saturating_sub(1),
            w: inner.w,
            h: 1,
        };
        let pad = props.footer_padding;
        status = status.inset(props.padding);
        status = status.inset(border_deco_inset);
        status = status.inset(Padding {
            left: pad.left,
            right: pad.right,
            top: 0,
            bottom: 0,
        });
        if status.w > 0 && status.h > 0 {
            status_rect = Some(status);
        }
    }

    // Scrollbar track positions
    let vscrollbar_track_x = if has_border && props.border_edges.has_right() {
        Some(
            frame_rect
                .x
                .saturating_add(frame_rect.w as i16)
                .saturating_sub(1),
        )
    } else {
        props
            .decorations
            .iter()
            .find(|d| d.placement == DecorationPlacement::Border && d.edge == Edge::Right)
            .or_else(|| {
                props
                    .decorations
                    .iter()
                    .find(|d| d.placement == DecorationPlacement::Border && d.edge == Edge::Left)
            })
            .map(|d| match d.edge {
                Edge::Left => body_rect.x,
                _ => body_rect
                    .x
                    .saturating_add(body_rect.w as i16)
                    .saturating_sub(1),
            })
    };

    let hscrollbar_track_y = if has_border && props.border_edges.has_bottom() {
        Some(
            frame_rect
                .y
                .saturating_add(frame_rect.h as i16)
                .saturating_sub(1),
        )
    } else {
        props
            .decorations
            .iter()
            .find(|d| d.placement == DecorationPlacement::Border && d.edge == Edge::Bottom)
            .or_else(|| {
                props
                    .decorations
                    .iter()
                    .find(|d| d.placement == DecorationPlacement::Border && d.edge == Edge::Top)
            })
            .map(|d| match d.edge {
                Edge::Top => body_rect.y,
                _ => body_rect
                    .y
                    .saturating_add(body_rect.h as i16)
                    .saturating_sub(1),
            })
    };

    FrameGeometry {
        outer_rect: rect,
        frame_rect,
        body_rect,
        content_rect,
        header_rect,
        status_rect,
        join_overlap,
        has_border,
        border_padding,
        vscrollbar_track_x,
        hscrollbar_track_y,
        padding: props.padding,
        decoration_outside,
        border_deco_inset,
    }
}

// Keep `frame_inner_max_size` as a thin wrapper for external callers.
pub(crate) fn frame_inner_max_size(
    frame: &crate::widgets::Frame,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (Option<u16>, Option<u16>) {
    let geometry = compute_frame_geometry(
        &frame.props,
        Rect::default(),
        FrameJoinOverlap::default(),
        true,
    );
    let (mut inner_max_w, inner_max_h) = geometry.inner_max_size(max_w, max_h);

    let border_w = if frame.props.has_border() {
        frame.props.border_edges.padding().horizontal()
    } else {
        0
    };
    match frame.props.width {
        Length::Px(px) => {
            let frame_inner_w = px
                .saturating_sub(geometry.decoration_outside.horizontal())
                .saturating_sub(border_w)
                .saturating_sub(frame.props.padding.horizontal())
                .saturating_sub(geometry.border_deco_inset.horizontal());
            inner_max_w = Some(
                inner_max_w
                    .map(|w| w.min(frame_inner_w))
                    .unwrap_or(frame_inner_w),
            );
        }
        Length::Percent(p) => {
            if let Some(parent_w) = max_w {
                let percent = p.min(100) as u32;
                let frame_w = ((parent_w as u32 * percent) / 100).min(u16::MAX as u32) as u16;
                let frame_inner_w = frame_w
                    .saturating_sub(geometry.decoration_outside.horizontal())
                    .saturating_sub(border_w)
                    .saturating_sub(frame.props.padding.horizontal())
                    .saturating_sub(geometry.border_deco_inset.horizontal());
                inner_max_w = Some(
                    inner_max_w
                        .map(|w| w.min(frame_inner_w))
                        .unwrap_or(frame_inner_w),
                );
            }
        }
        _ => {}
    }

    (inner_max_w, inner_max_h)
}
