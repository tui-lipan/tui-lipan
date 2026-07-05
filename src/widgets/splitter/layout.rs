use crate::layout::axis::Axis;
use crate::style::Rect;
use crate::widgets::containers::layout::measure_stack;

use super::{Splitter, sizes_from_weights};

fn handle_gap_size(splitter: &Splitter) -> u16 {
    if splitter.join_frame {
        0
    } else {
        splitter.handle_size.max(1)
    }
}

fn handle_hit_size(splitter: &Splitter) -> u16 {
    if splitter.join_frame {
        1
    } else {
        splitter.handle_size.max(1)
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
    let handle_thickness = handle_hit_size(splitter);

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
            let handle_rect = if splitter.join_frame {
                match axis {
                    Axis::Horizontal => {
                        let seam_x = cursor.saturating_sub(1).max(bounds.x);
                        Rect {
                            x: seam_x,
                            y: bounds.y,
                            w: handle_thickness,
                            h: bounds.h,
                        }
                    }
                    Axis::Vertical => {
                        let seam_y = cursor.saturating_sub(1).max(bounds.y);
                        Rect {
                            x: bounds.x,
                            y: seam_y,
                            w: bounds.w,
                            h: handle_thickness,
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
    use crate::widgets::{Spacer, Splitter};

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
    fn join_frame_vertical_removes_gutter_and_places_seam_handle() {
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        };

        let regular = base_vertical();
        let joined = base_vertical().join_frame(true);

        let regular_layout = layout_splitter(&regular, &[0.5, 0.5], bounds);
        let joined_layout = layout_splitter(&joined, &[0.5, 0.5], bounds);

        assert_eq!(regular_layout.pane_rects[1].x, 11);
        assert_eq!(regular_layout.handle_rects[0].x, 10);

        assert_eq!(joined_layout.pane_rects[1].x, 10);
        assert_eq!(joined_layout.handle_rects[0].x, 9);
        assert_eq!(joined_layout.handle_rects[0].w, 1);
    }

    #[test]
    fn join_frame_horizontal_removes_gutter_and_places_seam_handle() {
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 20,
        };

        let regular = base_horizontal();
        let joined = base_horizontal().join_frame(true);

        let regular_layout = layout_splitter(&regular, &[0.5, 0.5], bounds);
        let joined_layout = layout_splitter(&joined, &[0.5, 0.5], bounds);

        assert_eq!(regular_layout.pane_rects[1].y, 11);
        assert_eq!(regular_layout.handle_rects[0].y, 10);

        assert_eq!(joined_layout.pane_rects[1].y, 10);
        assert_eq!(joined_layout.handle_rects[0].y, 9);
        assert_eq!(joined_layout.handle_rects[0].h, 1);
    }

    #[test]
    fn join_frame_splitter_measure_excludes_gutter() {
        let regular = base_vertical();
        let joined = base_vertical().join_frame(true);

        let (regular_w, regular_h) = measure_splitter(&regular, None, None);
        let (joined_w, joined_h) = measure_splitter(&joined, None, None);

        assert_eq!(regular_h, joined_h);
        assert_eq!(regular_w.saturating_sub(joined_w), 1);
    }
}
