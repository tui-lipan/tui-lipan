/// Layout constraints for stack sizing.
///
/// These constraints control how elements behave during layout:
///
/// - `min_w`/`min_h`: Hard minimum sizes (prevents "squishing"). Accepts any
///   [`Length`] variant:
///   - `Px(n)` - absolute minimum in cells (the default is `Px(0)`, meaning no minimum).
///   - `Percent(p)` - minimum as a fraction of the parent's available size, resolved
///     at layout time. During measurement passes where the parent size is not yet
///     known, `Percent` mins have no effect (treated as 0).
///   - `Auto` / `Flex(_)` - treated as 0 (no minimum).
///
///   These are hard author constraints only. Intrinsic content floors such as
///   min-content and max-content are reported by the layout measurement query,
///   not encoded in `min_w`/`min_h`.
///
/// - `max_w`/`max_h`: Hard maximum sizes, capped after natural size computation.
///   Same [`Length`] variants; `None` means no cap, `Auto`/`Flex` also mean no cap.
///   `Percent` is resolved against the parent's offered size at measurement time.
///
/// - `focus_min_w`/`focus_min_h`: Absolute minimum sizes (cells) when this element
///   is focused. These are always `u16` - focus sizing is context-driven, not
///   percentage-relative.
///
/// - `collapse_w`/`collapse_h` / `force_compact`: Space-pressure sizing (accordion
///   focus mode). Always absolute `u16` values.
///
/// - `reflows`: The element's cross-axis size can change when its main-axis
///   allocation changes (for example a wrapping row flow).
///
/// - `shrink_priority`: Whether this element should yield space before normal
///   siblings under main-axis pressure.
///
/// Priority during layout: `min` > `max` > natural size.
/// If the resolved min exceeds the resolved max, the minimum wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayoutConstraints {
    /// Minimum width used for flexible layout.
    pub min_w: Length,
    /// Minimum height used for flexible layout.
    pub min_h: Length,
    /// Maximum width (clamped after natural size computation).
    pub max_w: Option<Length>,
    /// Maximum height (clamped after natural size computation).
    pub max_h: Option<Length>,
    /// Minimum width when this element is focused.
    pub focus_min_w: u16,
    /// Minimum height when this element is focused.
    pub focus_min_h: u16,
    /// Optional collapsed width when space is constrained.
    pub collapse_w: Option<u16>,
    /// Optional collapsed height when space is constrained.
    pub collapse_h: Option<u16>,
    /// Force compact sizing regardless of available space.
    pub force_compact: bool,
    /// Cross-axis size depends on the final main-axis allocation.
    pub reflows: bool,
    /// Main-axis shrink order under pressure.
    pub shrink_priority: ShrinkPriority,
}

/// Main-axis shrink priority for stack children.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ShrinkPriority {
    /// Normal stack shrink behavior.
    #[default]
    Normal,
    /// Yield space before normal siblings; useful for lower-priority wrapping
    /// groups that may truncate before rigid siblings wrap or collapse.
    First,
}

impl Default for LayoutConstraints {
    fn default() -> Self {
        Self {
            min_w: Length::Px(0),
            min_h: Length::Px(0),
            max_w: None,
            max_h: None,
            focus_min_w: 0,
            focus_min_h: 0,
            collapse_w: None,
            collapse_h: None,
            force_compact: false,
            reflows: false,
            shrink_priority: ShrinkPriority::Normal,
        }
    }
}

impl LayoutConstraints {
    /// Set the minimum width. Accepts any [`Length`] variant; `Auto`/`Flex` mean no minimum.
    pub fn min_width(mut self, v: Length) -> Self {
        self.min_w = v;
        self
    }

    /// Set the minimum height. Accepts any [`Length`] variant; `Auto`/`Flex` mean no minimum.
    pub fn min_height(mut self, v: Length) -> Self {
        self.min_h = v;
        self
    }

    /// Set the maximum width. Accepts any [`Length`] variant; `Auto`/`Flex` mean no cap.
    pub fn max_width(mut self, v: Length) -> Self {
        self.max_w = Some(v);
        self
    }

    /// Set the maximum height. Accepts any [`Length`] variant; `Auto`/`Flex` mean no cap.
    pub fn max_height(mut self, v: Length) -> Self {
        self.max_h = Some(v);
        self
    }

    /// Mark whether this element reflows when its main-axis allocation changes.
    pub fn reflows(mut self, v: bool) -> Self {
        self.reflows = v;
        self
    }

    /// Set this element's stack shrink priority.
    pub fn shrink_priority(mut self, v: ShrinkPriority) -> Self {
        self.shrink_priority = v;
        self
    }

    /// Clamp `natural` width to the min/max constraints resolved against `available`.
    ///
    /// `available` is the width offered by the parent container. `Percent` constraints
    /// are resolved relative to it; `Auto`/`Flex` constraints act as 0 (min) or no cap (max).
    pub(crate) fn clamp_width(&self, natural: u16, available: u16) -> u16 {
        let min = self.min_w.resolve_as_min(available);
        let mut w = natural.max(min);
        if let Some(max) = self.max_w.and_then(|l| l.resolve_as_max(available)) {
            w = w.min(max).max(min);
        }
        w
    }

    /// Clamp `natural` height to the min/max constraints resolved against `available`.
    ///
    /// `available` is the height offered by the parent container. `Percent` constraints
    /// are resolved relative to it; `Auto`/`Flex` constraints act as 0 (min) or no cap (max).
    pub(crate) fn clamp_height(&self, natural: u16, available: u16) -> u16 {
        let min = self.min_h.resolve_as_min(available);
        let mut h = natural.max(min);
        if let Some(max) = self.max_h.and_then(|l| l.resolve_as_max(available)) {
            h = h.min(max).max(min);
        }
        h
    }
}

/// Flexbox-inspired sizing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Length {
    /// Size to content.
    #[default]
    Auto,
    /// Fixed size in cells.
    Px(u16),
    /// Percentage of available space (0-100).
    Percent(u16),
    /// Fill remaining space proportionally.
    Flex(u16),
}

impl Length {
    /// Resolve the length against available space and content size.
    pub fn resolve(self, available: u16, content: u16) -> u16 {
        match self {
            Self::Auto => content,
            Self::Px(px) => px,
            Self::Percent(percent) => {
                let percent = percent.min(100);
                ((available as u32).saturating_mul(percent as u32) / 100).min(u16::MAX as u32)
                    as u16
            }
            Self::Flex(_) => available,
        }
    }

    /// Resolve this length as a **minimum** constraint against `available` space.
    ///
    /// - `Px(n)` → `n` (absolute).
    /// - `Percent(p)` → `p% of available`.
    /// - `Auto` / `Flex(_)` → `0` (no minimum enforced).
    pub(crate) fn resolve_as_min(self, available: u16) -> u16 {
        match self {
            Self::Px(px) => px,
            Self::Percent(p) => {
                if available == u16::MAX {
                    0
                } else {
                    ((available as u32 * p.min(100) as u32) / 100).min(u16::MAX as u32) as u16
                }
            }
            Self::Auto | Self::Flex(_) => 0,
        }
    }

    /// Resolve this length as a **maximum** constraint against `available` space.
    ///
    /// - `Px(n)` → `Some(n)` (absolute cap).
    /// - `Percent(p)` → `Some(p% of available)`.
    /// - `Auto` / `Flex(_)` → `None` (no cap).
    pub(crate) fn resolve_as_max(self, available: u16) -> Option<u16> {
        match self {
            Self::Px(px) => Some(px),
            Self::Percent(p) => {
                if available == u16::MAX {
                    None
                } else {
                    Some(((available as u32 * p.min(100) as u32) / 100).min(u16::MAX as u32) as u16)
                }
            }
            Self::Auto | Self::Flex(_) => None,
        }
    }
}

/// Sizing constraint for overlay helpers (e.g. `Center`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Size {
    /// Size to content.
    #[default]
    Auto,
    /// Fixed size in cells.
    Fixed(u16),
    /// Percentage of available space (0–100).
    Percent(u16),
}

/// Alignment of children on the cross axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    /// Start (left/top).
    #[default]
    Start,
    /// Center.
    Center,
    /// End (right/bottom).
    End,
    /// Stretch to fill.
    Stretch,
}

/// Alignment of children along the main axis.
///
/// Note that `SpaceBetween`, `SpaceAround`, and `SpaceEvenly` only have a
/// visible effect when there is slack on the main axis to distribute. In an
/// `HStack`/`VStack` whose default child sizing is `Length::Flex(1)` on the
/// main axis, flex children consume all remaining space and leave nothing for
/// the spacer math, so the layout looks identical to `Start`. To use these
/// variants, give each child an explicit non-flex main-axis size (e.g.
/// `Length::Auto` or `Length::Px(_)`) so the container has slack to distribute.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Justify {
    /// Pack children toward the start edge.
    #[default]
    Start,
    /// Center children in the available space.
    Center,
    /// Pack children toward the end edge.
    End,
    /// Evenly distribute extra space between children.
    ///
    /// Requires children with non-flex main-axis sizing — see the enum-level
    /// note.
    SpaceBetween,
    /// Evenly distribute extra space around children.
    ///
    /// Requires children with non-flex main-axis sizing — see the enum-level
    /// note.
    SpaceAround,
    /// Evenly distribute extra space between and around children.
    ///
    /// Requires children with non-flex main-axis sizing — see the enum-level
    /// note.
    SpaceEvenly,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_width_enforces_min_and_max() {
        let c = LayoutConstraints::default()
            .min_width(Length::Px(10))
            .max_width(Length::Px(50));

        // Natural within range - unchanged
        assert_eq!(c.clamp_width(30, 100), 30);
        // Natural below min - raised to min
        assert_eq!(c.clamp_width(5, 100), 10);
        // Natural above max - capped to max
        assert_eq!(c.clamp_width(80, 100), 50);
        // No max set - only min enforced
        let uncapped = LayoutConstraints::default().min_width(Length::Px(10));
        assert_eq!(uncapped.clamp_width(u16::MAX, 1000), u16::MAX);
    }

    #[test]
    fn clamp_height_min_wins_over_max() {
        // When min > max, the minimum takes priority (documented invariant).
        let c = LayoutConstraints::default()
            .min_height(Length::Px(40))
            .max_height(Length::Px(20));

        // Any natural value should resolve to min because min > max.
        assert_eq!(c.clamp_height(0, 100), 40);
        assert_eq!(c.clamp_height(30, 100), 40);
        assert_eq!(c.clamp_height(100, 100), 40);
    }

    #[test]
    fn clamp_zero_constraints_and_min_eq_max() {
        // All-zero constraints pass through zero
        let zero = LayoutConstraints::default();
        assert_eq!(zero.clamp_width(0, 100), 0);
        assert_eq!(zero.clamp_height(0, 100), 0);

        // min == max pins to that exact value
        let pinned = LayoutConstraints::default()
            .min_width(Length::Px(25))
            .max_width(Length::Px(25));
        assert_eq!(pinned.clamp_width(0, 100), 25);
        assert_eq!(pinned.clamp_width(25, 100), 25);
        assert_eq!(pinned.clamp_width(100, 100), 25);
    }

    #[test]
    fn clamp_percent_constraints() {
        // Percent(50) min with available=100 → min=50
        let c = LayoutConstraints::default()
            .min_width(Length::Percent(50))
            .max_width(Length::Percent(80));
        assert_eq!(c.clamp_width(0, 100), 50); // below min → clamped up
        assert_eq!(c.clamp_width(60, 100), 60); // in range
        assert_eq!(c.clamp_width(90, 100), 80); // above max → clamped down
        // With a different available
        assert_eq!(c.clamp_width(0, 200), 100); // 50% of 200
        assert_eq!(c.clamp_width(200, 200), 160); // capped at 80% of 200
    }

    #[test]
    fn clamp_auto_flex_constraints_mean_no_constraint() {
        // Auto/Flex min → 0, Auto/Flex max → no cap
        let c = LayoutConstraints::default()
            .min_width(Length::Auto)
            .max_width(Length::Flex(1));
        assert_eq!(c.clamp_width(0, 100), 0);
        assert_eq!(c.clamp_width(9999, 100), 9999);
    }

    #[test]
    fn length_resolve_variants() {
        // Auto returns content size regardless of available space
        assert_eq!(Length::Auto.resolve(100, 42), 42);
        assert_eq!(Length::Auto.resolve(0, 7), 7);

        // Px returns fixed pixel value regardless of available/content
        assert_eq!(Length::Px(60).resolve(100, 42), 60);
        assert_eq!(Length::Px(0).resolve(100, 42), 0);

        // Percent returns percentage of available space (clamped to 100)
        assert_eq!(Length::Percent(50).resolve(100, 42), 50);
        assert_eq!(Length::Percent(33).resolve(300, 42), 99);
        assert_eq!(Length::Percent(0).resolve(100, 42), 0);
        assert_eq!(Length::Percent(150).resolve(80, 42), 80);

        // Flex returns available space regardless of content or weight
        assert_eq!(Length::Flex(1).resolve(100, 42), 100);
        assert_eq!(Length::Flex(3).resolve(200, 10), 200);
        assert_eq!(Length::Flex(0).resolve(50, 50), 50);
    }

    #[test]
    fn layout_constraints_builder_and_defaults() {
        let defaults = LayoutConstraints::default();
        assert_eq!(defaults.min_w, Length::Px(0));
        assert_eq!(defaults.min_h, Length::Px(0));
        assert_eq!(defaults.max_w, None);
        assert_eq!(defaults.max_h, None);
        assert!(!defaults.force_compact);
        assert!(!defaults.reflows);
        assert_eq!(defaults.shrink_priority, ShrinkPriority::Normal);

        // Builder methods compose and set the right fields
        let c = LayoutConstraints::default()
            .min_width(Length::Px(5))
            .min_height(Length::Px(10))
            .max_width(Length::Px(100))
            .max_height(Length::Px(200))
            .reflows(true)
            .shrink_priority(ShrinkPriority::First);
        assert_eq!(c.min_w, Length::Px(5));
        assert_eq!(c.min_h, Length::Px(10));
        assert_eq!(c.max_w, Some(Length::Px(100)));
        assert_eq!(c.max_h, Some(Length::Px(200)));
        assert!(c.reflows);
        assert_eq!(c.shrink_priority, ShrinkPriority::First);
    }
}
