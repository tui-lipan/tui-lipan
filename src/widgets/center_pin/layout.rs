use crate::core::element::{Element, ElementKind};
use crate::layout::axis::{Axis, requested_main_axis};
use crate::layout::measure::min_size_constrained;
use crate::style::{Length, Padding};

/// Measure a center child at its natural content size.
///
/// Unlike `min_size_constrained`, this function recursively measures VStack/HStack
/// children by calling `min_size_constrained` directly on each child rather than
/// routing through `min_main_size_for_stack`.
///
/// **Why this matters**: `min_main_size_for_stack` intentionally truncates Flex
/// Frame/ScrollView children to chrome-only (e.g., 2 rows for a bordered Frame)
/// so that flex distribution can share space fairly. But CenterPin's center child
/// must be sized to its **actual content height**, not its flex-minimum, so the
/// center zone is tall enough to render content.
///
/// Example: `center: VStack { Frame { TextArea } }` - the Frame is Flex(1) by
/// default. `min_main_size_for_stack(Frame{Flex})` returns 2 (border), making
/// center_h=2 and inner area=0. `measure_center_child` calls
/// `min_size_constrained(Frame)` → `measure_frame` which always measures content,
/// returning the correct height.
pub(crate) fn measure_center_child(
    el: &Element,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    match &el.kind {
        ElementKind::VStack(vs) => measure_natural_stack(
            vs.props.gap,
            vs.props.border,
            vs.props.padding,
            &vs.children,
            Axis::Vertical,
            max_h,
            max_w,
        ),
        ElementKind::HStack(hs) => measure_natural_stack(
            hs.props.gap,
            hs.props.border,
            hs.props.padding,
            &hs.children,
            Axis::Horizontal,
            max_w,
            max_h,
        ),
        _ => min_size_constrained(el, max_w, max_h),
    }
}

/// Get the main-axis natural size of a child for CenterPin center measurement.
///
/// Unlike `min_main_size_for_stack`, this function:
/// - Respects `Px` explicitly (since `measure_spacer` always returns 0, ignoring Px)
/// - Uses full content measurement for `Auto` and `Flex` elements via `measure_center_child`
///   (bypassing the flex-minimum truncation in `min_main_size_for_stack`)
fn natural_child_main_size(child: &Element, axis: Axis, available_cross: Option<u16>) -> u16 {
    let len = requested_main_axis(child, axis, None);
    match len {
        Length::Px(px) => {
            // Respect the explicit Px size. `min_size_constrained` ignores Px for Spacer
            // (measure_spacer always returns 0), so we must check this directly.
            // Resolve Percent min constraints against the element's own Px size.
            let constraints = child.layout_constraints();
            let min = match axis {
                Axis::Vertical => constraints.min_h.resolve_as_min(px),
                Axis::Horizontal => constraints.min_w.resolve_as_min(px),
            };
            px.max(min)
        }
        _ => {
            // For Auto and Flex, use measure_center_child which returns full content height.
            let (cw, ch) = measure_center_child(child, available_cross, None);
            match axis {
                Axis::Vertical => ch,
                Axis::Horizontal => cw,
            }
        }
    }
}

/// Sum children's full content sizes (not flex-minimum sizes).
///
/// Used exclusively for `measure_center_child` so CenterPin's center zone is
/// sized to the actual content height regardless of Flex height modes.
fn measure_natural_stack(
    gap: u16,
    border: bool,
    padding: Padding,
    children: &[Element],
    axis: Axis,
    available_main: Option<u16>,
    available_cross: Option<u16>,
) -> (u16, u16) {
    let chrome_main = if border { 2u16 } else { 0 }.saturating_add(match axis {
        Axis::Vertical => padding.vertical(),
        Axis::Horizontal => padding.horizontal(),
    });
    let chrome_cross = if border { 2u16 } else { 0 }.saturating_add(match axis {
        Axis::Vertical => padding.horizontal(),
        Axis::Horizontal => padding.vertical(),
    });

    if children.is_empty() {
        return match axis {
            Axis::Vertical => (chrome_cross, chrome_main),
            Axis::Horizontal => (chrome_main, chrome_cross),
        };
    }

    let inner_cross = available_cross.map(|c| c.saturating_sub(chrome_cross));

    let mut main = 0u16;
    let mut cross = 0u16;

    for child in children {
        let child_main = natural_child_main_size(child, axis, inner_cross);
        let (cw, ch) = measure_center_child(
            child,
            match axis {
                Axis::Vertical => inner_cross,
                Axis::Horizontal => available_main,
            },
            match axis {
                Axis::Vertical => None,
                Axis::Horizontal => inner_cross,
            },
        );
        let child_cross = match axis {
            Axis::Vertical => cw,
            Axis::Horizontal => ch,
        };
        main = main.saturating_add(child_main);
        cross = cross.max(child_cross);
    }

    let gaps = gap.saturating_mul(children.len().saturating_sub(1) as u16);
    main = main.saturating_add(gaps).saturating_add(chrome_main);
    cross = cross.saturating_add(chrome_cross);

    match axis {
        Axis::Vertical => (cross, main),
        Axis::Horizontal => (main, cross),
    }
}

pub(crate) fn measure_center_pin(
    cp: &super::CenterPin,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    let (center_w, center_h) = cp
        .center
        .as_deref()
        .map(|c| measure_center_child(c, max_w, max_h))
        .unwrap_or((0, 0));

    let (top_w, top_h) = cp
        .top
        .as_deref()
        .map(|c| min_size_constrained(c, max_w, max_h))
        .unwrap_or((0, 0));

    let (bottom_w, bottom_h) = cp
        .bottom
        .as_deref()
        .map(|c| min_size_constrained(c, max_w, max_h))
        .unwrap_or((0, 0));

    let w = center_w.max(top_w).max(bottom_w);
    // Minimum height: all three zones stacked. At runtime the container will
    // receive the full available height (Flex(1)) and the center will be truly
    // centered within it.
    let h = center_h.saturating_add(top_h).saturating_add(bottom_h);

    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Length;
    use crate::widgets::{CenterPin, Frame, HStack, Spacer, Text, VStack};

    /// Regression: Spacer{Px(2)} must contribute 2 rows to center_h.
    ///
    /// `min_size_constrained(Spacer{Px(2)})` returns 0 because `measure_spacer` always
    /// returns (0, 0) regardless of the Px size. `natural_child_main_size` checks the
    /// child's `requested_main_axis` directly and returns 2 for Px(2).
    #[test]
    fn spacer_px_height_contributes_to_center_h() {
        let center_el: crate::core::element::Element = VStack::new()
            .child(Text::new("logo")) // Auto height → 1 row
            .child(Spacer::new().height(Length::Px(2))) // Px(2) → must be 2
            .child(Frame::new().child(Text::new("dialog"))) // border+1 → 3 rows
            .into();

        let (_, center_h) = measure_center_child(&center_el, Some(80), None);

        // Before fix: Spacer{Px(2)} returned 0 via min_size_constrained → center_h = 1+0+3 = 4
        // After fix:  Spacer{Px(2)} = 2 via natural_child_main_size  → center_h = 1+2+3 = 6
        assert_eq!(
            center_h, 6,
            "Spacer{{Px(2)}} must contribute 2 rows; got center_h={}",
            center_h
        );
    }

    /// Regression test: VStack{Flex} wrapping a Frame{Flex} with content must
    /// yield center_h equal to the full content height, not just Frame chrome.
    ///
    /// Bug: `min_size_constrained(VStack)` routes through `min_main_size_for_stack`
    /// which truncates Flex Frame children to 2 (border only), so the center zone
    /// collapses to 2 rows with 0 inner area. `measure_center_child` bypasses this
    /// by calling `min_size_constrained` directly on each child.
    #[test]
    fn flex_frame_inside_flex_vstack_measures_full_content() {
        // Frame{Flex} with a Text child: measure_frame returns border(2) + content(1) = 3
        let center_el: crate::core::element::Element = VStack::new()
            .child(Frame::new().child(Text::new("prompt")))
            .into();

        let (_, center_h) = measure_center_child(&center_el, Some(80), None);

        // Before fix: center_h = 2 (min_main_size_for_stack truncates Frame to chrome)
        // After fix:  center_h = 3 (border=2 + content=1)
        assert_eq!(
            center_h, 3,
            "Frame{{Flex}} inside VStack{{Flex}} must measure at full content height; got {}",
            center_h
        );
    }

    /// Verify that CenterPin with a Flex-height VStack center child still
    /// produces a non-zero center_h, enabling the center zone to render.
    #[test]
    fn center_pin_flex_vstack_center_has_nonzero_height() {
        let center_el: crate::core::element::Element = VStack::new()
            .child(Text::new("logo placeholder"))
            .child(Spacer::new().height(Length::Px(2)))
            .child(
                Frame::new()
                    .height(Length::Auto)
                    .child(Text::new("content")),
            )
            .into();

        let (_, center_h) = measure_center_child(&center_el, Some(80), Some(48));

        assert!(
            center_h > 0,
            "center_h must be non-zero so the center zone renders; got {}",
            center_h
        );
    }

    /// Verify CenterPin total min_h is non-zero with a Flex-wrapped center child.
    #[test]
    fn measure_center_pin_with_flex_center_is_nonzero() {
        let cp = CenterPin::new()
            .center(
                VStack::new()
                    .child(Text::new("logo"))
                    .child(Spacer::new().height(Length::Px(2)))
                    .child(Frame::new().height(Length::Auto).child(Text::new("dialog"))),
            )
            .bottom(HStack::new().child(Text::new("shortcuts")));

        let (_, min_h) = measure_center_pin(&cp, None, None);

        assert!(min_h > 0, "CenterPin min_h must be non-zero; got {}", min_h);
    }

    /// Verify that all-Flex children (VStack + Frame with no explicit content)
    /// still produce a non-zero center_h via measure_center_child.
    ///
    /// With the fix, even Flex Frame children are measured at full content height.
    /// An empty Frame{border=true} returns h=2. A Flex VStack{Text} returns h=1.
    #[test]
    fn all_flex_center_children_still_measure_content() {
        let center_el: crate::core::element::Element = VStack::new()
            .child(VStack::new().child(Text::new("logo"))) // Flex VStack → h=1
            .child(Frame::new()) // Flex Frame, no child → h=2 (border only)
            .into();

        let (_, center_h) = measure_center_child(&center_el, Some(80), Some(48));

        // Flex VStack{Text} → 1, Frame{} → 2, sum = 3
        assert!(
            center_h > 0,
            "measure_center_child must return non-zero even for all-Flex children; got {}",
            center_h
        );
    }
}
