use crate::layout::measure::min_size_constrained;
use crate::style::Rect;

pub fn resolve_popover_rect(popover: &super::Popover, trigger: Rect, bounds: Rect) -> Rect {
    let trigger = popover.anchor.map_or(trigger, |(x, y)| Rect {
        x: x as i16,
        y: y as i16,
        w: 0,
        h: 0,
    });

    let max_width = popover
        .max_width
        .and_then(|width| width.resolve_as_max(bounds.w))
        .unwrap_or(bounds.w)
        .min(bounds.w);
    let (mut width, mut height) = resolve_popover_size(popover, trigger, max_width, bounds.h);

    let mut placement = popover.placement;
    let bounds_left = bounds.x as i32;
    let bounds_top = bounds.y as i32;
    let bounds_right = bounds.x as i32 + bounds.w as i32;
    let bounds_bottom = bounds.y as i32 + bounds.h as i32;

    let mut pos = popover_position_for_placement(placement, trigger, width, height);

    if popover.auto_flip {
        let (x, y) = pos;
        let x2 = x + width as i32;
        let y2 = y + height as i32;

        placement = match placement {
            super::PopoverPlacement::BelowStart
            | super::PopoverPlacement::BelowCenter
            | super::PopoverPlacement::BelowEnd
                if y2 > bounds_bottom =>
            {
                popover_flip_vertical(placement)
            }
            super::PopoverPlacement::AboveStart
            | super::PopoverPlacement::AboveCenter
            | super::PopoverPlacement::AboveEnd
                if y < bounds_top =>
            {
                popover_flip_vertical(placement)
            }
            super::PopoverPlacement::RightStart
            | super::PopoverPlacement::RightCenter
            | super::PopoverPlacement::RightEnd
                if x2 > bounds_right =>
            {
                popover_flip_horizontal(placement)
            }
            super::PopoverPlacement::LeftStart
            | super::PopoverPlacement::LeftCenter
            | super::PopoverPlacement::LeftEnd
                if x < bounds_left =>
            {
                popover_flip_horizontal(placement)
            }
            _ => placement,
        };

        pos = popover_position_for_placement(placement, trigger, width, height);
    }

    if let Some(available_height) = vertical_space_for_placement(placement, trigger, bounds)
        && available_height < height
    {
        (width, height) = resolve_popover_size(popover, trigger, max_width, available_height);
        pos = popover_position_for_placement(placement, trigger, width, height);
    }

    let mut x = pos.0 + popover.offset.x as i32;
    let mut y = pos.1 + popover.offset.y as i32;

    if popover.clamp {
        let max_x = (bounds_right - width as i32).max(bounds_left);
        let max_y = (bounds_bottom - height as i32).max(bounds_top);
        x = x.max(bounds_left).min(max_x);
        y = y.max(bounds_top).min(max_y);
    }

    Rect {
        x: x as i16,
        y: y as i16,
        w: width,
        h: height,
    }
}

fn resolve_popover_size(
    popover: &super::Popover,
    trigger: Rect,
    max_width: u16,
    max_height: u16,
) -> (u16, u16) {
    let (cw, ch) = min_size_constrained(&popover.content, Some(max_width), Some(max_height));
    let width = if popover.fit_trigger_width {
        trigger.w.min(max_width)
    } else if popover.min_trigger_width {
        cw.max(trigger.w).min(max_width)
    } else {
        cw.min(max_width)
    };
    (width, ch.min(max_height))
}

fn vertical_space_for_placement(
    placement: super::PopoverPlacement,
    trigger: Rect,
    bounds: Rect,
) -> Option<u16> {
    let bounds_top = bounds.y as i32;
    let bounds_bottom = bounds.y as i32 + bounds.h as i32;
    let trigger_top = trigger.y as i32;
    let trigger_bottom = trigger.y as i32 + trigger.h as i32;

    match placement {
        super::PopoverPlacement::AboveStart
        | super::PopoverPlacement::AboveCenter
        | super::PopoverPlacement::AboveEnd => Some(trigger_top.saturating_sub(bounds_top) as u16),
        super::PopoverPlacement::BelowStart
        | super::PopoverPlacement::BelowCenter
        | super::PopoverPlacement::BelowEnd => {
            Some(bounds_bottom.saturating_sub(trigger_bottom) as u16)
        }
        _ => None,
    }
}

fn popover_position_for_placement(
    placement: super::PopoverPlacement,
    trigger: Rect,
    width: u16,
    height: u16,
) -> (i32, i32) {
    let tx = trigger.x as i32;
    let ty = trigger.y as i32;
    let tw = trigger.w as i32;
    let th = trigger.h as i32;
    let w = width as i32;
    let h = height as i32;

    let align_x_start = tx;
    let align_x_center = tx + (tw - w) / 2;
    let align_x_end = tx + tw - w;

    let align_y_start = ty;
    let align_y_center = ty + (th - h) / 2;
    let align_y_end = ty + th - h;

    match placement {
        super::PopoverPlacement::BelowStart => (align_x_start, ty + th),
        super::PopoverPlacement::BelowCenter => (align_x_center, ty + th),
        super::PopoverPlacement::BelowEnd => (align_x_end, ty + th),
        super::PopoverPlacement::AboveStart => (align_x_start, ty - h),
        super::PopoverPlacement::AboveCenter => (align_x_center, ty - h),
        super::PopoverPlacement::AboveEnd => (align_x_end, ty - h),
        super::PopoverPlacement::RightStart => (tx + tw, align_y_start),
        super::PopoverPlacement::RightCenter => (tx + tw, align_y_center),
        super::PopoverPlacement::RightEnd => (tx + tw, align_y_end),
        super::PopoverPlacement::LeftStart => (tx - w, align_y_start),
        super::PopoverPlacement::LeftCenter => (tx - w, align_y_center),
        super::PopoverPlacement::LeftEnd => (tx - w, align_y_end),
    }
}

fn popover_flip_vertical(placement: super::PopoverPlacement) -> super::PopoverPlacement {
    match placement {
        super::PopoverPlacement::BelowStart => super::PopoverPlacement::AboveStart,
        super::PopoverPlacement::BelowCenter => super::PopoverPlacement::AboveCenter,
        super::PopoverPlacement::BelowEnd => super::PopoverPlacement::AboveEnd,
        super::PopoverPlacement::AboveStart => super::PopoverPlacement::BelowStart,
        super::PopoverPlacement::AboveCenter => super::PopoverPlacement::BelowCenter,
        super::PopoverPlacement::AboveEnd => super::PopoverPlacement::BelowEnd,
        other => other,
    }
}

fn popover_flip_horizontal(placement: super::PopoverPlacement) -> super::PopoverPlacement {
    match placement {
        super::PopoverPlacement::LeftStart => super::PopoverPlacement::RightStart,
        super::PopoverPlacement::LeftCenter => super::PopoverPlacement::RightCenter,
        super::PopoverPlacement::LeftEnd => super::PopoverPlacement::RightEnd,
        super::PopoverPlacement::RightStart => super::PopoverPlacement::LeftStart,
        super::PopoverPlacement::RightCenter => super::PopoverPlacement::LeftCenter,
        super::PopoverPlacement::RightEnd => super::PopoverPlacement::LeftEnd,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_popover_rect;
    use crate::style::{Length, Rect};
    use crate::widgets::{Popover, PopoverPlacement, Spacer, Text};

    #[test]
    fn min_trigger_width_allows_content_to_exceed_trigger_width() {
        let popover = Popover::new()
            .content(Text::new("012345678901234567890123456789"))
            .placement(PopoverPlacement::BelowStart);

        let rect = resolve_popover_rect(
            &popover,
            Rect {
                x: 2,
                y: 2,
                w: 12,
                h: 1,
            },
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 24,
            },
        );

        assert_eq!(rect.w, 30);
    }

    #[test]
    fn fit_trigger_width_uses_exact_trigger_width() {
        let popover = Popover::new()
            .content(Spacer::new().width(Length::Px(30)))
            .fit_trigger_width(true)
            .placement(PopoverPlacement::BelowStart);

        let rect = resolve_popover_rect(
            &popover,
            Rect {
                x: 2,
                y: 2,
                w: 12,
                h: 1,
            },
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 24,
            },
        );

        assert_eq!(rect.w, 12);
    }

    #[test]
    fn max_width_caps_content_and_trigger_min_width() {
        let popover = Popover::new()
            .content(Text::new("012345678901234567890123456789"))
            .max_width(Length::Px(14))
            .placement(PopoverPlacement::BelowStart);

        let rect = resolve_popover_rect(
            &popover,
            Rect {
                x: 2,
                y: 2,
                w: 20,
                h: 1,
            },
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 24,
            },
        );

        assert_eq!(rect.w, 14);
    }

    #[test]
    fn below_start_uses_trigger_left_and_bottom_edge() {
        let popover = Popover::new()
            .content(Text::new("wide"))
            .min_trigger_width(false)
            .placement(PopoverPlacement::BelowStart)
            .auto_flip(false)
            .clamp(false);

        let trigger = Rect {
            x: 10,
            y: 4,
            w: 6,
            h: 3,
        };
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 24,
        };
        let rect = resolve_popover_rect(&popover, trigger, bounds);

        assert_eq!(rect.x, 10);
        assert_eq!(rect.y, 7);
        assert_eq!(rect.w, 4);
        assert_eq!(rect.h, 1);
    }

    #[test]
    fn above_end_aligns_right_edge_with_trigger() {
        let popover = Popover::new()
            .content(Text::new("wide"))
            .min_trigger_width(false)
            .placement(PopoverPlacement::AboveEnd)
            .auto_flip(false)
            .clamp(false);

        let trigger = Rect {
            x: 10,
            y: 4,
            w: 6,
            h: 3,
        };
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 24,
        };
        let rect = resolve_popover_rect(&popover, trigger, bounds);

        assert_eq!(rect.x + rect.w as i16, trigger.x + trigger.w as i16);
        assert_eq!(rect.y + rect.h as i16, trigger.y);
    }

    #[test]
    fn right_center_centers_popover_vertically_on_trigger() {
        let popover = Popover::new()
            .content(Text::new("x"))
            .min_trigger_width(false)
            .placement(PopoverPlacement::RightCenter)
            .auto_flip(false)
            .clamp(false);

        let trigger = Rect {
            x: 5,
            y: 5,
            w: 3,
            h: 5,
        };
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 24,
        };
        let rect = resolve_popover_rect(&popover, trigger, bounds);

        assert_eq!(rect.x, trigger.x + trigger.w as i16);
        assert_eq!(rect.y, trigger.y + ((trigger.h - rect.h) / 2) as i16);
    }

    #[test]
    fn auto_flip_moves_below_start_to_above_start_when_overflowing_bottom() {
        let trigger = Rect {
            x: 2,
            y: 9,
            w: 3,
            h: 2,
        };
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 10,
        };

        let no_flip = resolve_popover_rect(
            &Popover::new()
                .content(Text::new("abc"))
                .min_trigger_width(false)
                .placement(PopoverPlacement::BelowStart)
                .auto_flip(false)
                .clamp(false),
            trigger,
            bounds,
        );
        let flipped = resolve_popover_rect(
            &Popover::new()
                .content(Text::new("abc"))
                .min_trigger_width(false)
                .placement(PopoverPlacement::BelowStart)
                .auto_flip(true)
                .clamp(false),
            trigger,
            bounds,
        );

        assert_eq!(no_flip.y, trigger.y + trigger.h as i16);
        assert_eq!(flipped.y + flipped.h as i16, trigger.y);
    }

    #[test]
    fn above_start_shrinks_to_available_space_when_auto_flip_disabled() {
        let trigger = Rect {
            x: 2,
            y: 4,
            w: 8,
            h: 2,
        };
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 12,
        };

        let rect = resolve_popover_rect(
            &Popover::new()
                .content(Text::new("1\n2\n3\n4\n5\n6\n7\n8\n9\n10"))
                .fit_trigger_width(true)
                .placement(PopoverPlacement::AboveStart)
                .auto_flip(false)
                .clamp(false),
            trigger,
            bounds,
        );

        assert_eq!(rect.y, bounds.y);
        assert_eq!(rect.h, trigger.y as u16);
        assert_eq!(rect.y + rect.h as i16, trigger.y);
    }

    #[test]
    fn below_start_shrinks_to_available_space_when_auto_flip_disabled() {
        let trigger = Rect {
            x: 2,
            y: 7,
            w: 8,
            h: 2,
        };
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 12,
        };

        let rect = resolve_popover_rect(
            &Popover::new()
                .content(Text::new("1\n2\n3\n4\n5\n6\n7\n8\n9\n10"))
                .fit_trigger_width(true)
                .placement(PopoverPlacement::BelowStart)
                .auto_flip(false)
                .clamp(false),
            trigger,
            bounds,
        );

        assert_eq!(rect.y, trigger.y + trigger.h as i16);
        assert_eq!(rect.h, bounds.h - (trigger.y as u16 + trigger.h));
        assert_eq!(rect.y + rect.h as i16, bounds.y + bounds.h as i16);
    }

    #[test]
    fn offset_is_applied_then_position_is_clamped_to_bounds() {
        let trigger = Rect {
            x: 8,
            y: 4,
            w: 2,
            h: 1,
        };
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 6,
        };

        let unclamped = resolve_popover_rect(
            &Popover::new()
                .content(Text::new("wide"))
                .min_trigger_width(false)
                .placement(PopoverPlacement::BelowStart)
                .offset((2, 2))
                .auto_flip(false)
                .clamp(false),
            trigger,
            bounds,
        );
        let clamped = resolve_popover_rect(
            &Popover::new()
                .content(Text::new("wide"))
                .min_trigger_width(false)
                .placement(PopoverPlacement::BelowStart)
                .offset((2, 2))
                .auto_flip(false)
                .clamp(true),
            trigger,
            bounds,
        );

        assert_eq!(unclamped.x, 10);
        assert_eq!(unclamped.y, 7);
        assert_eq!(clamped.x, 6);
        assert_eq!(clamped.y, 5);
        assert!(clamped.x >= bounds.x);
        assert!(clamped.y >= bounds.y);
        assert!(clamped.x + clamped.w as i16 <= bounds.x + bounds.w as i16);
        assert!(clamped.y + clamped.h as i16 <= bounds.y + bounds.h as i16);
    }
}
