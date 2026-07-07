use crate::core::element::{Element, ElementKind};
use crate::layout::axis::Axis;
use crate::style::Rect;
use crate::widgets::containers::layout::{frame_join_enabled, measure_stack};

use super::{Splitter, SplitterHandleMode, sizes_from_weights};

fn rides_border(splitter: &Splitter) -> bool {
    matches!(splitter.handle_mode, SplitterHandleMode::Border)
}

fn handle_gap_size(splitter: &Splitter) -> u16 {
    if rides_border(splitter) {
        0
    } else {
        splitter.handle_size.max(1)
    }
}

/// Whether `el` renders a border on the edge facing the seam.
///
/// `leading` selects the edge before the element along the split axis (left/top);
/// otherwise the edge after it (right/bottom). Only plain bordered frames are
/// inspected; anything else reports no border and falls back to a synthetic seam.
fn border_on_seam_edge(el: &Element, axis: Axis, leading: bool) -> bool {
    let ElementKind::Frame(frame) = &el.kind else {
        return false;
    };
    if !frame.props.has_border() {
        return false;
    }
    let edges = frame.props.border_edges;
    match (axis, leading) {
        (Axis::Horizontal, true) => edges.has_left(),
        (Axis::Horizontal, false) => edges.has_right(),
        (Axis::Vertical, true) => edges.has_top(),
        (Axis::Vertical, false) => edges.has_bottom(),
    }
}

/// Seam handle geometry for the gap between two panes in border-riding mode.
///
/// Returns `(offset, thickness)`, where `offset` is how many cells before the
/// shared boundary (`cursor`) the handle starts and `thickness` is its size
/// along the split axis:
/// - merged borders (both neighbors join-enabled) share one wall → `(1, 1)`,
/// - two separate borders expose adjacent walls → `(1, 2)` so both are grabbed,
/// - a single border on either side → a 1-cell handle over that wall,
/// - borderless neighbors → a synthetic 1-cell handle on the seam.
fn seam_geometry(left: &Element, right: &Element, axis: Axis) -> (u16, u16) {
    if frame_join_enabled(left) && frame_join_enabled(right) {
        return (1, 1);
    }
    let left_border = border_on_seam_edge(left, axis, false);
    let right_border = border_on_seam_edge(right, axis, true);
    match (left_border, right_border) {
        (true, true) => (1, 2),
        (true, false) => (1, 1),
        (false, true) => (0, 1),
        (false, false) => (1, 1),
    }
}

pub(crate) struct SplitterLayout {
    pub pane_rects: Vec<Rect>,
    pub handle_rects: Vec<Rect>,
    pub pane_sizes: Vec<u16>,
}

pub(crate) fn axis_for_splitter(splitter: &Splitter) -> Axis {
    match splitter.orientation {
        crate::widgets::Orientation::Horizontal => Axis::Vertical,
        crate::widgets::Orientation::Vertical => Axis::Horizontal,
    }
}

pub(crate) fn measure_splitter(
    splitter: &Splitter,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    let axis = axis_for_splitter(splitter);
    let (main_max, cross_max) = match axis {
        Axis::Horizontal => (max_w, max_h),
        Axis::Vertical => (max_h, max_w),
    };

    let (mut w, mut h) = measure_stack(
        &crate::widgets::internal::StackProps::default(),
        &splitter.children,
        axis,
        main_max,
        cross_max,
    );

    let handle_count = splitter.children.len().saturating_sub(1) as u16;
    let handle_total = handle_gap_size(splitter).saturating_mul(handle_count);
    match axis {
        Axis::Horizontal => {
            w = w.saturating_add(handle_total);
        }
        Axis::Vertical => {
            h = h.saturating_add(handle_total);
        }
    }

    (w, h)
}

pub(crate) fn layout_splitter(
    splitter: &Splitter,
    weights: &[f32],
    bounds: Rect,
) -> SplitterLayout {
    let axis = axis_for_splitter(splitter);
    let handle_count = splitter.children.len().saturating_sub(1) as u16;
    let handle_total = handle_gap_size(splitter).saturating_mul(handle_count);

    let available_main = match axis {
        Axis::Horizontal => bounds.w,
        Axis::Vertical => bounds.h,
    };
    let available_main = available_main.saturating_sub(handle_total);

    let pane_sizes = sizes_from_weights(weights, available_main, splitter.min_size);

    let mut pane_rects = Vec::with_capacity(splitter.children.len());
    let mut handle_rects = Vec::with_capacity(splitter.children.len().saturating_sub(1));

    let mut cursor = match axis {
        Axis::Horizontal => bounds.x,
        Axis::Vertical => bounds.y,
    };

    for (idx, size) in pane_sizes.iter().enumerate() {
        let rect = match axis {
            Axis::Horizontal => Rect {
                x: cursor,
                y: bounds.y,
                w: *size,
                h: bounds.h,
            },
            Axis::Vertical => Rect {
                x: bounds.x,
                y: cursor,
                w: bounds.w,
                h: *size,
            },
        };
        pane_rects.push(rect);
        cursor = cursor.saturating_add(*size as i16);

        if idx + 1 < pane_sizes.len() {
            let handle_rect = if rides_border(splitter) {
                let (offset, thickness) =
                    match (splitter.children.get(idx), splitter.children.get(idx + 1)) {
                        (Some(left), Some(right)) => seam_geometry(left, right, axis),
                        _ => (1, 1),
                    };
                // Clamp the seam rect to `bounds`: with collapsed panes
                // (min_size 0, tiny layouts) the cursor can sit on the bounds
                // edge and an unclamped 2-cell handle would expose an
                // out-of-bounds cell to hit-testing and junction detection.
                match axis {
                    Axis::Horizontal => {
                        let seam_x = cursor.saturating_sub(offset as i16).max(bounds.x);
                        let end = (bounds.x as i32 + bounds.w as i32)
                            .min(seam_x as i32 + thickness as i32);
                        Rect {
                            x: seam_x,
                            y: bounds.y,
                            w: (end - seam_x as i32).max(0) as u16,
                            h: bounds.h,
                        }
                    }
                    Axis::Vertical => {
                        let seam_y = cursor.saturating_sub(offset as i16).max(bounds.y);
                        let end = (bounds.y as i32 + bounds.h as i32)
                            .min(seam_y as i32 + thickness as i32);
                        Rect {
                            x: bounds.x,
                            y: seam_y,
                            w: bounds.w,
                            h: (end - seam_y as i32).max(0) as u16,
                        }
                    }
                }
            } else {
                match axis {
                    Axis::Horizontal => Rect {
                        x: cursor,
                        y: bounds.y,
                        w: splitter.handle_size,
                        h: bounds.h,
                    },
                    Axis::Vertical => Rect {
                        x: bounds.x,
                        y: cursor,
                        w: bounds.w,
                        h: splitter.handle_size,
                    },
                }
            };
            handle_rects.push(handle_rect);
            cursor = cursor.saturating_add(handle_gap_size(splitter) as i16);
        }
    }

    SplitterLayout {
        pane_rects,
        handle_rects,
        pane_sizes,
    }
}

#[cfg(test)]
mod tests {
    use super::{layout_splitter, measure_splitter};
    use crate::style::Rect;
    use crate::widgets::{Frame, Spacer, Splitter, SplitterHandleMode};

    fn base_vertical() -> Splitter {
        Splitter::vertical()
            .weights(vec![0.5, 0.5])
            .handle_size(1)
            .child(Spacer::new())
            .child(Spacer::new())
    }

    fn base_horizontal() -> Splitter {
        Splitter::horizontal()
            .weights(vec![0.5, 0.5])
            .handle_size(1)
            .child(Spacer::new())
            .child(Spacer::new())
    }

    #[test]
    fn border_mode_vertical_removes_gutter_and_places_seam_handle() {
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        };

        let regular = base_vertical();
        let joined = base_vertical().handle_mode(SplitterHandleMode::Border);

        let regular_layout = layout_splitter(&regular, &[0.5, 0.5], bounds);
        let joined_layout = layout_splitter(&joined, &[0.5, 0.5], bounds);

        assert_eq!(regular_layout.pane_rects[1].x, 11);
        assert_eq!(regular_layout.handle_rects[0].x, 10);

        // Borderless panes fall back to a synthetic 1-cell handle on the seam.
        assert_eq!(joined_layout.pane_rects[1].x, 10);
        assert_eq!(joined_layout.handle_rects[0].x, 9);
        assert_eq!(joined_layout.handle_rects[0].w, 1);
    }

    #[test]
    fn border_mode_horizontal_removes_gutter_and_places_seam_handle() {
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 20,
        };

        let regular = base_horizontal();
        let joined = base_horizontal().handle_mode(SplitterHandleMode::Border);

        let regular_layout = layout_splitter(&regular, &[0.5, 0.5], bounds);
        let joined_layout = layout_splitter(&joined, &[0.5, 0.5], bounds);

        assert_eq!(regular_layout.pane_rects[1].y, 11);
        assert_eq!(regular_layout.handle_rects[0].y, 10);

        assert_eq!(joined_layout.pane_rects[1].y, 10);
        assert_eq!(joined_layout.handle_rects[0].y, 9);
        assert_eq!(joined_layout.handle_rects[0].h, 1);
    }

    #[test]
    fn border_mode_measure_excludes_gutter() {
        let regular = base_vertical();
        let joined = base_vertical().handle_mode(SplitterHandleMode::Border);

        let (regular_w, regular_h) = measure_splitter(&regular, None, None);
        let (joined_w, joined_h) = measure_splitter(&joined, None, None);

        assert_eq!(regular_h, joined_h);
        assert_eq!(regular_w.saturating_sub(joined_w), 1);
    }

    #[test]
    fn separate_bordered_panes_grab_both_walls() {
        // Two bordered frames that do NOT merge their borders: the seam is two
        // adjacent wall columns, so the integrated handle spans both.
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        };
        let splitter = Splitter::vertical()
            .weights(vec![0.5, 0.5])
            .handle_mode(SplitterHandleMode::Border)
            .child(Frame::new().border(true))
            .child(Frame::new().border(true));

        let layout = layout_splitter(&splitter, &[0.5, 0.5], bounds);
        // Left pane's right border at x=9, right pane's left border at x=10.
        assert_eq!(layout.pane_rects[1].x, 10);
        assert_eq!(layout.handle_rects[0].x, 9);
        assert_eq!(layout.handle_rects[0].w, 2);
    }

    #[test]
    fn collapsed_pane_seam_handle_stays_within_bounds() {
        // With min_size 0 the trailing pane can collapse to zero width, which
        // puts the seam cursor on the bounds edge; the 2-cell handle for
        // separate borders must be clamped instead of poking past the edge.
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        };
        let splitter = Splitter::vertical()
            .min_size(0)
            .handle_mode(SplitterHandleMode::Border)
            .child(Frame::new().border(true))
            .child(Frame::new().border(true));

        let layout = layout_splitter(&splitter, &[1.0, 0.0], bounds);
        assert_eq!(layout.pane_sizes, vec![20, 0]);
        let handle = layout.handle_rects[0];
        assert_eq!(handle.x, 19);
        assert_eq!(handle.w, 1);
        assert!(handle.x.saturating_add(handle.w as i16) <= bounds.x + bounds.w as i16);
    }

    #[test]
    fn merged_bordered_panes_share_one_wall() {
        // Two join-enabled frames merge their borders into one shared wall, so
        // the integrated handle is a single cell on that wall.
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        };
        let splitter = Splitter::vertical()
            .weights(vec![0.5, 0.5])
            .handle_mode(SplitterHandleMode::Border)
            .child(Frame::new().border(true).join_frame(true))
            .child(Frame::new().border(true).join_frame(true));

        let layout = layout_splitter(&splitter, &[0.5, 0.5], bounds);
        assert_eq!(layout.pane_rects[1].x, 10);
        assert_eq!(layout.handle_rects[0].x, 9);
        assert_eq!(layout.handle_rects[0].w, 1);
    }
}
