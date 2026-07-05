use std::borrow::Borrow;

use crate::core::element::{Element, ElementKind};
use crate::layout::axis::{Axis, requested_main_axis};
use crate::layout::measure::min_size_constrained;
use crate::layout::stack::{StackLayoutParams, compute_stack_layout};
use crate::style::Length;

/// Check whether an element acts as a join-enabled bordered frame.
///
/// A plain `Frame` with `join_frame && has_border()` qualifies directly.
/// An HStack or VStack qualifies transitively when any of its children is
/// join-enabled, because all children of a cross-axis wrapper share the
/// same leading/trailing edge (e.g. all HStack children share the same
/// top/bottom border row).
pub(crate) fn frame_join_enabled(el: &Element) -> bool {
    match &el.kind {
        ElementKind::Frame(frame) => frame.props.join_frame && frame.props.has_border(),
        ElementKind::HStack(hs) => hs.children.iter().any(frame_join_enabled),
        ElementKind::VStack(vs) => vs.children.iter().any(frame_join_enabled),
        ElementKind::Grid(g) => g.items.iter().any(|i| frame_join_enabled(&i.element)),
        ElementKind::Flow(flow) => flow.children.iter().any(frame_join_enabled),
        _ => false,
    }
}

/// Build a per-gap vector indicating which adjacent pairs overlap for border sharing.
pub(crate) fn join_overlap_vector<C: Borrow<Element>>(children: &[C]) -> Vec<bool> {
    if children.len() < 2 {
        return Vec::new();
    }
    (0..children.len() - 1)
        .map(|i| {
            frame_join_enabled(children[i].borrow()) && frame_join_enabled(children[i + 1].borrow())
        })
        .collect()
}

/// Compute the minimum main axis size for a child element in a stack context.
///
/// Stack measurement uses [`compute_stack_layout`] for the main axis; this helper
/// remains for unit tests (see also `center_pin` layout docs).
///
/// For `Length::Flex` widgets, returns a minimal size instead of
/// full content size. This prevents Flex widgets from claiming all available
/// space based on content during min-size calculations, allowing them to share
/// space equally.
///
/// - Scrollable widgets (ScrollView, List, TextArea): minimal scrollable size
/// - Container widgets (Frame, VStack, HStack): use their LayoutConstraints::min
/// - Fixed widgets (Input, Text): use their natural content size
#[cfg(test)]
fn min_main_size_for_stack(
    child: &Element,
    axis: Axis,
    available_cross: Option<u16>,
    available_main: Option<u16>,
) -> u16 {
    let len = requested_main_axis(child, axis, None);
    // Resolve Percent min constraints against the available main-axis space.
    // When available_main is unknown (None), Percent resolves to 0.
    let avail_main = available_main.unwrap_or(0);

    // For Px, return the fixed value but respect the element's min constraint.
    // The element's LayoutConstraints::min may be larger than the Px value
    // when padding inflates the minimum size (e.g. HStack with height: Px(1)
    // and top padding of 2 has min_h = 3). This must match
    // compute_stack_layout_internal which uses max(px, constraint_min).
    if let Length::Px(px) = len {
        let constraints = child.layout_constraints();
        let min = match axis {
            Axis::Vertical => constraints.min_h.resolve_as_min(avail_main),
            Axis::Horizontal => constraints.min_w.resolve_as_min(avail_main),
        };
        return px.max(min);
    }

    // For Flex widgets, use minimal size to allow equal flex distribution.
    // However, when the parent is unconstrained (Auto), there is no concrete
    // size to distribute from, so Flex children must fall through to content
    // measurement - otherwise they collapse to 0.
    if matches!(len, Length::Flex(_)) && available_main.is_some() {
        // Check if it's a scrollable widget first (they don't set min_h in LayoutConstraints)
        if let Some(min) = scrollable_min_for_axis(child, axis) {
            return min;
        }
        // For other Flex widgets (Frame, VStack, HStack, etc.), use their LayoutConstraints::min
        let constraints = child.layout_constraints();
        let min = match axis {
            Axis::Vertical => constraints.min_h.resolve_as_min(avail_main),
            Axis::Horizontal => constraints.min_w.resolve_as_min(avail_main),
        };
        if min > 0 {
            return min;
        }
        // Fallback: if no min is set, use 0 (flex will distribute space)
        return 0;
    }

    // For Auto, use the full content size
    // Pass available_cross to the correct dimension based on axis:
    // - VStack (Vertical): cross = width, so pass as max_w
    // - HStack (Horizontal): cross = height, so pass as max_h
    let (w, h) = match axis {
        Axis::Vertical => min_size_constrained(child, available_cross, None),
        Axis::Horizontal => min_size_constrained(child, None, available_cross),
    };
    match axis {
        Axis::Vertical => h,
        Axis::Horizontal => w,
    }
}

/// For scrollable or framed widgets, return a minimal size
/// (1 line + chrome) to prevent them from claiming full content size.
fn scrollable_min_for_axis(child: &Element, axis: Axis) -> Option<u16> {
    match &child.kind {
        ElementKind::Frame(frame) => {
            let mut min: u16 = 0;
            match axis {
                Axis::Vertical => {
                    if frame.props.border {
                        min += 2;
                    }
                    min = min.saturating_add(frame.props.padding.vertical());
                }
                Axis::Horizontal => {
                    if frame.props.border {
                        min += 2;
                    }
                    min = min.saturating_add(frame.props.padding.horizontal());
                }
            }
            Some(min)
        }
        ElementKind::ScrollView(sv) => {
            let mut min: u16 = 1;
            match axis {
                Axis::Vertical => {
                    min = min.saturating_add(sv.props.padding.vertical());
                    if sv.props.border {
                        min = min.saturating_add(2);
                    }
                }
                Axis::Horizontal => {
                    min = min.saturating_add(sv.props.padding.horizontal());
                    if sv.props.border {
                        min = min.saturating_add(2);
                    }
                }
            }
            Some(min)
        }
        ElementKind::List(list) => {
            let mut min: u16 = 1;
            match axis {
                Axis::Vertical => {
                    min = min.saturating_add(list.padding.vertical());
                    if list.border {
                        min = min.saturating_add(2);
                    }
                }
                Axis::Horizontal => {
                    min = min.saturating_add(list.padding.horizontal());
                    if list.border {
                        min = min.saturating_add(2);
                    }
                }
            }
            Some(min)
        }
        ElementKind::TextArea(text_area) => {
            let mut min: u16 = 1;
            match axis {
                Axis::Vertical => {
                    min = min.saturating_add(text_area.padding.vertical());
                    if text_area.border {
                        min = min.saturating_add(2);
                    }
                }
                Axis::Horizontal => {
                    min = min.saturating_add(text_area.padding.horizontal());
                    if text_area.border {
                        min = min.saturating_add(2);
                    }
                }
            }
            Some(min)
        }
        ElementKind::Input(input) => {
            let mut min: u16 = 1; // Content height is always 1
            match axis {
                Axis::Vertical => {
                    min = min.saturating_add(input.padding.vertical());
                    if input.border {
                        min = min.saturating_add(2);
                    }
                }
                Axis::Horizontal => {
                    // For horizontal, minimal is just chrome + 1 cell for cursor if possible
                    min = input.padding.horizontal();
                    if input.border {
                        min = min.saturating_add(2);
                    }
                    // min = min.saturating_add(1); // Optional: at least 1 cell for content?
                }
            }
            Some(min)
        }
        _ => None,
    }
}

pub(crate) fn measure_stack(
    props: &crate::widgets::internal::StackProps,
    children: &[Element],
    axis: Axis,
    available_main: Option<u16>,
    available_cross: Option<u16>,
) -> (u16, u16) {
    if children.is_empty() {
        return (0, 0);
    }

    let layout_children: Vec<&Element> = children
        .iter()
        .filter(|child| !matches!(child.kind, ElementKind::Portal(_)))
        .collect();
    if layout_children.is_empty() {
        return (0, 0);
    }

    let mut cross = 0u16;

    let inner_cross = if props.border {
        available_cross.map(|c| c.saturating_sub(2))
    } else {
        available_cross
    };
    let inner_cross = match axis {
        Axis::Vertical => inner_cross.map(|c| c.saturating_sub(props.padding.horizontal())),
        Axis::Horizontal => inner_cross.map(|c| c.saturating_sub(props.padding.vertical())),
    };

    let inner_main = if props.border {
        available_main.map(|c| c.saturating_sub(2))
    } else {
        available_main
    };
    let inner_main = match axis {
        Axis::Vertical => inner_main.map(|c| c.saturating_sub(props.padding.vertical())),
        Axis::Horizontal => inner_main.map(|c| c.saturating_sub(props.padding.horizontal())),
    };

    /// Budget used only to run the same flex/gap/join pass as reconcile when the
    /// parent did not supply a main-axis size. Slack must exceed any realistic
    /// intrinsic total so flex children (coerced to auto-height intrinsics) do not
    /// trigger spurious shrink logic.
    const INTRINSIC_MAIN_SLACK: u16 = 16_000;

    let intrinsic_main_axis = inner_main.is_none();
    let main_budget = inner_main.unwrap_or(INTRINSIC_MAIN_SLACK);

    let measured_main_layout = compute_stack_layout(StackLayoutParams {
        props,
        children: &layout_children,
        axis,
        available: main_budget,
        available_cross: inner_cross,
        focus: None,
        pinned_key: None,
        intrinsic_main_axis,
    });

    #[cfg(feature = "diff-view")]
    let _split_wrap_dual_pass = (axis == Axis::Horizontal)
        .then(|| {
            crate::widgets::SplitWrapDualPass::begin_measure(
                layout_children
                    .iter()
                    .copied()
                    .zip(measured_main_layout.sizes.iter().copied()),
                inner_cross,
            )
        })
        .flatten();

    for (idx, child) in layout_children.iter().enumerate() {
        let child = *child;
        let measured_main = measured_main_layout.sizes[idx];

        // Get the cross axis size
        let cross_axis = match axis {
            Axis::Vertical => Axis::Horizontal,
            Axis::Horizontal => Axis::Vertical,
        };
        let cross_len = requested_main_axis(child, cross_axis, None);

        let cross_size = if matches!(cross_len, Length::Flex(_)) {
            // For Flex widgets in the cross axis, use minimal size to prevent pushing parent containers.
            // Resolve Percent constraints against the available cross-axis space.
            let avail_cross = inner_cross.unwrap_or(0);
            let constraint_min = scrollable_min_for_axis(child, cross_axis).unwrap_or_else(|| {
                let constraints = child.layout_constraints();
                match cross_axis {
                    Axis::Vertical => constraints.min_h.resolve_as_min(avail_cross),
                    Axis::Horizontal => constraints.min_w.resolve_as_min(avail_cross),
                }
            });

            // If constraint_min is just chrome (no content), also measure actual content size
            // and use the larger of the two. This prevents containers with default Flex sizing
            // from collapsing to just padding when they have actual content.
            let is_container = matches!(
                child.kind,
                ElementKind::Frame(_)
                    | ElementKind::VStack(_)
                    | ElementKind::HStack(_)
                    | ElementKind::Flow(_)
                    | ElementKind::ZStack(_)
                    | ElementKind::Center(_)
                    | ElementKind::Group(_)
                    | ElementKind::Memo(_)
                    | ElementKind::EffectScope(_)
                    | ElementKind::Tabs(_)
                    | ElementKind::DraggableTabBar(_)
            );
            let content_size = if constraint_min <= 4 || is_container {
                let (cw, ch) = match axis {
                    Axis::Vertical => min_size_constrained(child, inner_cross, Some(measured_main)),
                    Axis::Horizontal => {
                        min_size_constrained(child, Some(measured_main), inner_cross)
                    }
                };
                match axis {
                    Axis::Vertical => cw,
                    Axis::Horizontal => ch,
                }
            } else {
                0
            };

            constraint_min.max(content_size)
        } else {
            let (cw, ch) = match axis {
                Axis::Vertical => min_size_constrained(child, inner_cross, Some(measured_main)),
                Axis::Horizontal => min_size_constrained(child, Some(measured_main), inner_cross),
            };
            match axis {
                Axis::Vertical => cw,
                Axis::Horizontal => ch,
            }
        };

        cross = cross.max(cross_size);
    }

    let layout = &measured_main_layout;
    let mut main = layout
        .sizes
        .iter()
        .fold(0u16, |acc, sz| acc.saturating_add(*sz));
    main = layout
        .gaps
        .iter()
        .fold(main, |acc, gap| acc.saturating_add(*gap));
    main = main.saturating_sub(layout.join_count);

    if props.border {
        main = main.saturating_add(2);
        cross = cross.saturating_add(2);
    }

    match axis {
        Axis::Vertical => {
            cross = cross.saturating_add(props.padding.horizontal());
            main = main.saturating_add(props.padding.vertical());
            (cross, main)
        }
        Axis::Horizontal => {
            main = main.saturating_add(props.padding.horizontal());
            cross = cross.saturating_add(props.padding.vertical());
            (main, cross)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::element::Element;
    use crate::layout::axis::Axis;
    use crate::style::{Length, Padding};
    use crate::widgets::{DocumentView, HStack, Text, VStack};
    /// Regression test: VStack inside Center should measure width based on content,
    /// not collapse to just padding when child has Flex cross-axis sizing.
    /// See: Center widget making VStack super slim (~4 chars wide) bug.
    #[test]
    fn vstack_measures_width_from_content_not_just_chrome() {
        // Create an HStack (defaults to width: Flex(1)) with actual text content
        let hstack: Element = HStack::new().child(Text::new("Hello World")).into();

        // Create a VStack containing the HStack
        let vstack: Element = VStack::new().width(Length::Auto).child(hstack).into();

        // Measure the VStack - width should be based on "Hello World" content (~11 chars)
        // not just padding/border (~0-4 chars)
        let (w, _h) = super::measure_stack(
            &crate::widgets::internal::StackProps {
                gap: 0,
                border: false,
                padding: Padding::from(0u16),
                ..Default::default()
            },
            &[vstack],
            Axis::Vertical, // VStack axis
            None,           // available_main (height)
            None,           // available_cross (width)
        );

        // Width should be at least the length of "Hello World" (11 chars)
        assert!(
            w >= 11,
            "VStack width should be at least 11 chars (content width), but got {}",
            w
        );
    }

    /// Test that Flex children in cross-axis use constraint minimum when content is small
    #[test]
    fn flex_child_uses_constraint_min_for_small_content() {
        // HStack with very short text - should still respect content
        let hstack: Element = HStack::new().child(Text::new("Hi")).into();

        let vstack: Element = VStack::new().width(Length::Auto).child(hstack).into();

        let (w, _h) = super::measure_stack(
            &crate::widgets::internal::StackProps {
                gap: 0,
                border: false,
                padding: Padding::from(0u16),
                ..Default::default()
            },
            &[vstack],
            Axis::Vertical,
            None,
            None,
        );

        // Should be at least 2 chars for "Hi"
        assert!(w >= 2, "Width should be at least 2 chars, but got {}", w);
    }

    /// `DocumentView` defaults to `height: Flex(1)`. Unbounded stack measure must not
    /// treat that as a zero-height flex slot that then absorbs slack (which used to
    /// desync scroll content height from reconcile and clip auto `DocumentView`).
    #[test]
    fn unbounded_main_measure_uses_intrinsic_height_for_default_flex_document_view() {
        let props = crate::widgets::internal::StackProps {
            gap: 0,
            border: false,
            padding: Padding::from(0u16),
            ..Default::default()
        };
        let doc: Element = DocumentView::new("one\ntwo\nthree")
            .wrap(true)
            .border(false)
            .scrollbar(false)
            .into();
        let (_w, h) = super::measure_stack(&props, &[doc], Axis::Vertical, None, Some(20));
        assert!(
            (3..100).contains(&h),
            "expected a few wrapped lines, not flex-slack inflation: h={h}"
        );
    }

    /// Regression test: min_main_size_for_stack must respect min_h for Px children.
    /// When an HStack has height: Px(1) but min_h = 3 (from padding), both
    /// measurement and layout must agree on the allocated size (3, not 1).
    #[test]
    fn px_height_measurement_respects_min_h_constraint() {
        // HStack with height: Px(1) and top padding of 2.
        // min_h = 3 (1 content + 2 top padding), which exceeds the Px value.
        let hstack: Element = HStack::new()
            .height(Length::Px(1))
            .padding((2u16, 0u16, 0u16, 0u16))
            .child(Text::new("Tip text"))
            .into();

        let constraints = hstack.layout_constraints();
        assert_eq!(
            constraints.min_h,
            Length::Px(3),
            "min_h should reflect padding (2 top + 1 content = 3)"
        );

        // Measurement should return max(px, min_h) = 3, matching layout behavior
        let measured = super::min_main_size_for_stack(&hstack, Axis::Vertical, None, None);
        assert_eq!(
            measured, 3,
            "min_main_size_for_stack should return max(Px(1), min_h=3) = 3"
        );
    }

    /// Verify that measurement and layout allocate the same size for Px-height
    /// HStack children inside a VStack. Before the fix, measurement counted
    /// Px(1) as 1 line but layout allocated 3 lines (due to min_h=3 from padding).
    #[test]
    fn px_height_hstack_consistent_measurement_and_layout() {
        use crate::layout::stack::{StackLayoutParams, compute_stack_layout};
        use crate::widgets::internal::StackProps;

        let tips: Element = HStack::new()
            .height(Length::Px(1))
            .padding((2u16, 0u16, 0u16, 0u16))
            .child(Text::new("Tip text"))
            .into();

        let frame_content: Element = VStack::new()
            .height(Length::Auto)
            .child(Text::new("Input"))
            .child(Text::new("Status"))
            .into();

        // Measure the VStack containing both children
        let (_, measured_h) = super::measure_stack(
            &crate::widgets::internal::StackProps {
                gap: 0,
                border: false,
                padding: Padding::from(0u16),
                ..Default::default()
            },
            &[frame_content.clone(), tips.clone()],
            Axis::Vertical,
            None,
            None,
        );

        // Layout the same children in the measured space
        let layout = compute_stack_layout(StackLayoutParams {
            props: &StackProps {
                gap: 0,
                ..StackProps::default()
            },
            children: &[frame_content, tips],
            axis: Axis::Vertical,
            available: measured_h,
            available_cross: None,
            focus: None,
            pinned_key: None,
            intrinsic_main_axis: false,
        });

        let total_allocated: u16 = layout.sizes.iter().sum();
        assert_eq!(
            total_allocated, measured_h,
            "Layout total ({}) should equal measured height ({}). \
             Sizes: {:?}",
            total_allocated, measured_h, layout.sizes
        );
    }

    /// Gap must not be applied around zero-size children.
    /// [Text, empty HStack, Text] with gap=2 should measure as 2 lines + 1 gap = 4,
    /// not 2 lines + 2 gaps = 6.
    #[test]
    fn gap_skips_zero_size_children() {
        let children: Vec<Element> = vec![
            Text::new("A").into(),
            HStack::new().into(), // empty → 0 height
            Text::new("B").into(),
        ];
        let props = crate::widgets::internal::StackProps {
            gap: 2,
            border: false,
            padding: Padding::from(0u16),
            ..Default::default()
        };

        // Verify empty HStack measures as 0 height.
        let empty: Element = HStack::new().into();
        let (_, eh) = super::measure_stack(
            &crate::widgets::internal::StackProps::default(),
            &[empty],
            Axis::Vertical,
            None,
            None,
        );
        assert_eq!(eh, 0, "Empty HStack should have 0 height");

        // VStack measurement: A(1) + gap(2) + B(1) = 4, not 6.
        let (_, h) = super::measure_stack(&props, &children, Axis::Vertical, None, None);
        assert_eq!(h, 4, "Gap should only apply between non-zero children");
    }

    /// StatusBar-like HStack with Px(1) and bottom padding must get enough
    /// space for padding to not clip the content.
    #[test]
    fn statusbar_px_height_with_padding_gets_enough_space() {
        // StatusBar pattern: height: Px(1), padding: (0, 2, 1, 2)
        // vertical padding = 1, so needs 2 lines total
        let statusbar: Element = HStack::new()
            .height(Length::Px(1))
            .padding((0u16, 2u16, 1u16, 2u16))
            .child(Text::new("status"))
            .into();

        let measured = super::min_main_size_for_stack(&statusbar, Axis::Vertical, None, None);
        assert_eq!(
            measured, 2,
            "StatusBar-like HStack with bottom padding should measure as 2 lines"
        );
    }

    /// Test that nested VStack/HStack properly propagates content width
    /// This specifically tests the cross-axis measurement fix for Flex children.
    #[test]
    fn nested_stacks_measure_width_correctly() {
        // Create a more complex layout similar to opencode_home.rs
        let inner_hstack: Element = HStack::new()
            .child(Text::new("Short"))
            .child(Text::new("Text"))
            .into();

        let outer_vstack: Element = VStack::new()
            .width(Length::Auto)
            .gap(1)
            .child(inner_hstack)
            .child(Text::new("Another longer text here"))
            .into();

        let (w, _h) = super::measure_stack(
            &crate::widgets::internal::StackProps {
                gap: 0,
                border: false,
                padding: Padding::from(0u16),
                ..Default::default()
            },
            &[outer_vstack],
            Axis::Vertical,
            None,
            None,
        );

        // Width should be based on the longer text "Another longer text here" (~24 chars)
        // This tests that the HStack (Flex width) doesn't collapse to just chrome
        assert!(
            w >= 24,
            "Width should be at least 24 chars (longest content), but got {}",
            w
        );
    }
}
