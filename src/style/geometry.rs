/// A rectangle in terminal cell coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    /// Left edge (column).
    pub x: i16,
    /// Top edge (row).
    pub y: i16,
    /// Width in cells.
    pub w: u16,
    /// Height in cells.
    pub h: u16,
}

impl Rect {
    /// Returns `true` if the point is inside this rectangle.
    pub fn contains(&self, x: i16, y: i16) -> bool {
        let x2 = self.x as i32 + self.w as i32;
        let y2 = self.y as i32 + self.h as i32;
        (x as i32) >= (self.x as i32)
            && (x as i32) < x2
            && (y as i32) >= (self.y as i32)
            && (y as i32) < y2
    }

    /// Returns the rectangle after applying padding.
    pub fn inset(&self, padding: Padding) -> Self {
        let x = self.x.saturating_add(padding.left as i16);
        let y = self.y.saturating_add(padding.top as i16);
        let w = self
            .w
            .saturating_sub(padding.left.saturating_add(padding.right));
        let h = self
            .h
            .saturating_sub(padding.top.saturating_add(padding.bottom));
        Self { x, y, w, h }
    }

    /// Returns the inner rectangle after accounting for optional border and padding.
    ///
    /// This is the common pattern used by widgets to compute their content area:
    /// first subtract border (if present), then subtract padding.
    pub fn inner(&self, border: bool, padding: Padding) -> Self {
        let mut inner = *self;
        if border {
            inner = inner.inset(Padding::BORDER);
        }
        inner.inset(padding)
    }

    /// Returns the inner rectangle after accounting for optional border edges and padding.
    ///
    /// Unlike [`Rect::inner`], this can reserve only the cells occupied by a partial border.
    pub fn inner_with_border_edges(
        &self,
        border: bool,
        border_edges: BorderEdges,
        padding: Padding,
    ) -> Self {
        let mut inner = *self;
        if border {
            inner = inner.inset(border_edges.padding());
        }
        inner.inset(padding)
    }

    /// Returns the intersection of two rectangles.
    pub fn intersection(&self, other: &Rect) -> Rect {
        let x1 = (self.x as i32).max(other.x as i32);
        let y1 = (self.y as i32).max(other.y as i32);
        let x2 = (self.x as i32 + self.w as i32).min(other.x as i32 + other.w as i32);
        let y2 = (self.y as i32 + self.h as i32).min(other.y as i32 + other.h as i32);

        Rect {
            x: x1.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            y: y1.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            w: (x2 - x1).max(0).min(u16::MAX as i32) as u16,
            h: (y2 - y1).max(0).min(u16::MAX as i32) as u16,
        }
    }

    /// Returns `true` if the rectangle has zero area.
    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }
}

/// Which frame border edges reserve layout space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BorderEdges {
    /// Reserve all four border edges.
    #[default]
    All,
    /// Reserve only the top and bottom edges, while still rendering corner caps.
    HorizontalCaps,
}

impl BorderEdges {
    /// Padding consumed by the selected border edges.
    pub const fn padding(self) -> Padding {
        match self {
            Self::All => Padding::BORDER,
            Self::HorizontalCaps => Padding {
                left: 0,
                right: 0,
                top: 1,
                bottom: 1,
            },
        }
    }

    /// Returns `true` when the left vertical edge is present.
    pub const fn has_left(self) -> bool {
        matches!(self, Self::All)
    }

    /// Returns `true` when the right vertical edge is present.
    pub const fn has_right(self) -> bool {
        matches!(self, Self::All)
    }

    /// Returns `true` when the top horizontal edge is present.
    pub const fn has_top(self) -> bool {
        matches!(self, Self::All | Self::HorizontalCaps)
    }

    /// Returns `true` when the bottom horizontal edge is present.
    pub const fn has_bottom(self) -> bool {
        matches!(self, Self::All | Self::HorizontalCaps)
    }
}

/// A rectangle in fractional terminal cell coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FloatRect {
    /// Left edge (column).
    pub x: f32,
    /// Top edge (row).
    pub y: f32,
    /// Width in cells.
    pub w: f32,
    /// Height in cells.
    pub h: f32,
}

impl FloatRect {
    /// Converts this fractional rectangle to terminal cell coordinates.
    ///
    /// Finite values are rounded first and then clamped to the integer coordinate
    /// ranges. Non-finite values become `0`; negative sizes clamp to `0`.
    pub fn to_rect(self) -> Rect {
        Rect {
            x: round_clamp_i16(self.x),
            y: round_clamp_i16(self.y),
            w: round_clamp_u16(self.w),
            h: round_clamp_u16(self.h),
        }
    }
}

fn round_clamp_i16(value: f32) -> i16 {
    if value.is_finite() {
        value.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16
    } else {
        0
    }
}

fn round_clamp_u16(value: f32) -> u16 {
    if value.is_finite() {
        value.round().clamp(0.0, u16::MAX as f32) as u16
    } else {
        0
    }
}

/// Padding in terminal cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Padding {
    /// Left padding.
    pub left: u16,
    /// Right padding.
    pub right: u16,
    /// Top padding.
    pub top: u16,
    /// Bottom padding.
    pub bottom: u16,
}

impl Padding {
    /// Standard border inset (1 cell on each side).
    pub const BORDER: Self = Self {
        left: 1,
        right: 1,
        top: 1,
        bottom: 1,
    };

    /// Horizontal padding (left + right).
    pub fn horizontal(&self) -> u16 {
        self.left.saturating_add(self.right)
    }

    /// Vertical padding (top + bottom).
    pub fn vertical(&self) -> u16 {
        self.top.saturating_add(self.bottom)
    }
}

impl From<u16> for Padding {
    /// One value applies to all sides.
    fn from(v: u16) -> Self {
        Self {
            left: v,
            right: v,
            top: v,
            bottom: v,
        }
    }
}

impl From<(u16, u16)> for Padding {
    /// Two tuple values: vertical, then horizontal padding.
    fn from((v, h): (u16, u16)) -> Self {
        Self {
            left: h,
            right: h,
            top: v,
            bottom: v,
        }
    }
}

impl From<(u16, u16, u16, u16)> for Padding {
    /// Four tuple values in CSS order: top, right, bottom, left.
    fn from((t, r, b, l): (u16, u16, u16, u16)) -> Self {
        Self {
            top: t,
            right: r,
            bottom: b,
            left: l,
        }
    }
}

/// Edge of a rectangle (for accent bars, etc.).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Edge {
    /// Left edge.
    #[default]
    Left,
    /// Right edge.
    Right,
    /// Top edge.
    Top,
    /// Bottom edge.
    Bottom,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── FloatRect::to_rect ──────────────────────────────────────────

    #[test]
    fn float_rect_to_rect_rounds_to_nearest_cells() {
        let rect = FloatRect {
            x: 1.4,
            y: -2.6,
            w: 10.5,
            h: 3.49,
        }
        .to_rect();

        assert_eq!(
            rect,
            Rect {
                x: 1,
                y: -3,
                w: 11,
                h: 3,
            }
        );
    }

    #[test]
    fn float_rect_to_rect_clamps_coordinates_and_sizes() {
        let rect = FloatRect {
            x: i16::MIN as f32 - 100.0,
            y: i16::MAX as f32 + 100.0,
            w: -12.0,
            h: u16::MAX as f32 + 10.0,
        }
        .to_rect();

        assert_eq!(
            rect,
            Rect {
                x: i16::MIN,
                y: i16::MAX,
                w: 0,
                h: u16::MAX,
            }
        );
    }

    #[test]
    fn float_rect_to_rect_converts_non_finite_values_to_zero() {
        let rect = FloatRect {
            x: f32::NAN,
            y: f32::INFINITY,
            w: f32::NEG_INFINITY,
            h: f32::NAN,
        }
        .to_rect();

        assert_eq!(rect, Rect::default());
    }

    // ── Rect::contains ──────────────────────────────────────────────

    #[test]
    fn contains_point_inside_and_boundary() {
        let r = Rect {
            x: 5,
            y: 10,
            w: 20,
            h: 10,
        };
        // Interior point
        assert!(r.contains(15, 15));
        // Top-left corner (inclusive)
        assert!(r.contains(5, 10));
        // Just inside bottom-right (exclusive boundary: x < x+w, y < y+h)
        assert!(r.contains(24, 19));
        // Right edge is exclusive (x+w = 25)
        assert!(!r.contains(25, 15));
        // Bottom edge is exclusive (y+h = 20)
        assert!(!r.contains(15, 20));
    }

    #[test]
    fn contains_point_outside() {
        let r = Rect {
            x: 5,
            y: 10,
            w: 20,
            h: 10,
        };
        assert!(!r.contains(4, 15)); // left of rect
        assert!(!r.contains(15, 9)); // above rect
        assert!(!r.contains(26, 15)); // right of rect
        assert!(!r.contains(15, 21)); // below rect
    }

    #[test]
    fn contains_zero_size_rect_rejects_all() {
        let zero_w = Rect {
            x: 0,
            y: 0,
            w: 0,
            h: 10,
        };
        assert!(!zero_w.contains(0, 0));

        let zero_h = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 0,
        };
        assert!(!zero_h.contains(0, 0));

        let zero_both = Rect::default();
        assert!(!zero_both.contains(0, 0));
    }

    // ── Rect::inset / Rect::inner ───────────────────────────────────

    #[test]
    fn inset_normal_and_saturating() {
        let r = Rect {
            x: 10,
            y: 20,
            w: 40,
            h: 30,
        };
        let p = Padding {
            left: 3,
            right: 5,
            top: 2,
            bottom: 4,
        };
        let i = r.inset(p);
        assert_eq!(
            i,
            Rect {
                x: 13,
                y: 22,
                w: 32,
                h: 24
            }
        );

        // Padding larger than dimensions saturates w/h to 0
        let big = Padding {
            left: 25,
            right: 25,
            top: 20,
            bottom: 20,
        };
        let i2 = r.inset(big);
        assert_eq!(i2.w, 0);
        assert_eq!(i2.h, 0);
        // x/y still shift (saturating_add)
        assert_eq!(i2.x, 35);
        assert_eq!(i2.y, 40);
    }

    #[test]
    fn inner_with_border_and_padding() {
        let r = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 10,
        };
        // border=true adds Padding::BORDER (1 each side), then user padding
        let user_pad = Padding {
            left: 1,
            right: 1,
            top: 0,
            bottom: 0,
        };
        let result = r.inner(true, user_pad);
        // After border: x=1, y=1, w=18, h=8
        // After user_pad: x=2, y=1, w=16, h=8
        assert_eq!(
            result,
            Rect {
                x: 2,
                y: 1,
                w: 16,
                h: 8
            }
        );

        // border=false skips the border inset
        let no_border = r.inner(false, user_pad);
        assert_eq!(
            no_border,
            Rect {
                x: 1,
                y: 0,
                w: 18,
                h: 10
            }
        );
    }

    // ── Rect::intersection ──────────────────────────────────────────

    #[test]
    fn intersection_overlapping_and_identical() {
        let a = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        };
        let b = Rect {
            x: 5,
            y: 5,
            w: 10,
            h: 10,
        };
        // Overlap is the region [5..10) x [5..10)
        assert_eq!(
            a.intersection(&b),
            Rect {
                x: 5,
                y: 5,
                w: 5,
                h: 5
            }
        );
        // Intersection with self returns self
        assert_eq!(a.intersection(&a), a);
    }

    #[test]
    fn intersection_non_overlapping_and_contained() {
        // Disjoint - no overlap
        let a = Rect {
            x: 0,
            y: 0,
            w: 5,
            h: 5,
        };
        let b = Rect {
            x: 10,
            y: 10,
            w: 5,
            h: 5,
        };
        let empty = a.intersection(&b);
        assert!(empty.is_empty());

        // b fully inside a
        let big = Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 100,
        };
        let small = Rect {
            x: 10,
            y: 10,
            w: 5,
            h: 5,
        };
        assert_eq!(big.intersection(&small), small);
    }

    #[test]
    fn intersection_zero_size_and_negative_coords() {
        // Zero-size rect intersected with anything is empty
        let z = Rect {
            x: 5,
            y: 5,
            w: 0,
            h: 0,
        };
        let r = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 20,
        };
        assert!(z.intersection(&r).is_empty());

        // Negative coordinates - rects that overlap across origin
        let neg = Rect {
            x: -5,
            y: -5,
            w: 10,
            h: 10,
        }; // covers -5..5
        let pos = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        }; // covers 0..10
        let inter = neg.intersection(&pos);
        assert_eq!(
            inter,
            Rect {
                x: 0,
                y: 0,
                w: 5,
                h: 5
            }
        );
    }

    // ── Padding conversions ─────────────────────────────────────────

    #[test]
    fn padding_from_conversions_and_helpers() {
        // From single value
        let p1 = Padding::from(3u16);
        assert_eq!(
            p1,
            Padding {
                left: 3,
                right: 3,
                top: 3,
                bottom: 3
            }
        );

        // From (vertical, horizontal)
        let p2 = Padding::from((2u16, 5u16));
        assert_eq!(
            p2,
            Padding {
                left: 5,
                right: 5,
                top: 2,
                bottom: 2
            }
        );

        // From (top, right, bottom, left) - CSS order
        let p3 = Padding::from((1u16, 2u16, 3u16, 4u16));
        assert_eq!(
            p3,
            Padding {
                top: 1,
                right: 2,
                bottom: 3,
                left: 4
            }
        );
        assert_eq!(p3.horizontal(), 6);
        assert_eq!(p3.vertical(), 4);

        // Saturating helpers at u16::MAX
        let pmax = Padding {
            left: u16::MAX,
            right: 1,
            top: 1,
            bottom: u16::MAX,
        };
        assert_eq!(pmax.horizontal(), u16::MAX);
        assert_eq!(pmax.vertical(), u16::MAX);
    }
}
