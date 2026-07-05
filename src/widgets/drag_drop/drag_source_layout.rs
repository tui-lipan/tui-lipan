use crate::core::element::Key;
use crate::layout::drag_source_layout_hint::drag_source_snapshot_collapse_key;
use crate::layout::measure::min_size_constrained;

use super::drag_source::DragSource;
use super::payload::{DragSlot, DragSlotAxis};

pub(crate) fn measure_drag_source(
    source: &DragSource,
    element_key: Option<&Key>,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    if let Some(child) = source.child.as_deref() {
        if matches!(source.preview, crate::widgets::DragPreview::SourceSnapshot)
            && let (Some(ek), Some(collapse)) = (element_key, drag_source_snapshot_collapse_key())
            && ek == &collapse
        {
            return snapshot_collapsed_intrinsic_size(source, child, max_w, max_h);
        }
        min_size_constrained(child, max_w, max_h)
    } else {
        (0, 0)
    }
}

pub(crate) fn snapshot_collapsed_intrinsic_size(
    source: &DragSource,
    child: &crate::core::element::Element,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    let (cw, ch) = min_size_constrained(child, max_w, max_h);
    match source.drag_slot {
        DragSlot::Collapse => match source.drag_slot_axis {
            DragSlotAxis::Vertical => (cw, 0),
            DragSlotAxis::Horizontal => (0, ch),
        },
        DragSlot::Specified(len) => {
            let (avail_main, content_main) = match source.drag_slot_axis {
                DragSlotAxis::Vertical => (max_h.unwrap_or(u16::MAX), ch),
                DragSlotAxis::Horizontal => (max_w.unwrap_or(u16::MAX), cw),
            };
            let main = len.resolve(avail_main, content_main);
            match source.drag_slot_axis {
                DragSlotAxis::Vertical => (cw, main),
                DragSlotAxis::Horizontal => (main, ch),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::element::Element;
    use crate::layout::drag_source_layout_hint::set_drag_source_snapshot_collapse_key;
    use crate::widgets::Text;

    #[test]
    fn drag_source_collapses_to_zero_height_when_collapse_key_matches() {
        let child: Element = Text::new("Hello world").into();
        let source = DragSource::new().child(child).preview_snapshot();

        // Without collapse hint, should have non-zero height
        let el: Element = Element::from(source.clone()).key("test-card");
        let (_, h_normal) = min_size_constrained(&el, Some(40), None);
        assert!(h_normal > 0, "normal height should be > 0, got {h_normal}");

        // Set collapse hint matching the key
        set_drag_source_snapshot_collapse_key(Some("test-card".into()));
        // Create fresh element (simulating re-render)
        let el: Element = Element::from(source).key("test-card");
        let (_, h_collapsed) = min_size_constrained(&el, Some(40), None);
        assert_eq!(
            h_collapsed, 0,
            "collapsed height should be 0 (DragSlot::Collapse), got {h_collapsed}"
        );

        // Clean up
        set_drag_source_snapshot_collapse_key(None);
    }

    /// The layout-only render path reuses the cached element tree from the
    /// previous `view()` call. Those elements have per-element measure caches
    /// AND layout hash caches populated from the non-collapsed state. Both
    /// must be bypassed for collapse to work.
    #[test]
    fn drag_source_collapses_despite_stale_caches() {
        use crate::layout::hash::element_layout_hash;
        use crate::layout::stack::layout_vstack;
        use crate::style::Rect;
        use crate::widgets::internal::StackProps;

        let child: Element = Text::new("Hello world").into();
        let source = DragSource::new().child(child).preview_snapshot();
        let el_a: Element = Element::from(source).key("test-card");
        let el_b: Element = Text::new("other").into();

        // Pre-populate ALL per-element caches (simulating previous render)
        let (_, h_before) = min_size_constrained(&el_a, Some(40), None);
        assert!(h_before > 0, "should have non-zero height before drag");
        assert!(
            el_a.measure_cache.get()[0].is_some(),
            "measure cache populated"
        );

        let hash_before = element_layout_hash(&el_a);
        assert!(hash_before.is_some(), "layout hash should be cached");
        assert!(
            el_a.layout_hash_cache.get().is_some(),
            "hash cache populated"
        );

        // Pre-populate VStack layout (simulating previous reconcile)
        let props = StackProps {
            gap: 1,
            ..StackProps::default()
        };
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };
        let rects_before = layout_vstack(&props, &[el_a.clone(), el_b.clone()], bounds, None, None);
        assert!(rects_before[0].h > 0, "card should have height before drag");

        // Now set collapse hint and re-layout the SAME elements
        set_drag_source_snapshot_collapse_key(Some("test-card".into()));

        let hash_after = element_layout_hash(&el_a);
        assert_ne!(
            hash_before, hash_after,
            "hash must change when collapse hint is set"
        );

        let rects_after = layout_vstack(&props, &[el_a, el_b], bounds, None, None);
        assert_eq!(
            rects_after[0].h, 0,
            "card must collapse to h=0, got {}",
            rects_after[0].h
        );
        assert_eq!(
            rects_after[1].y, 0,
            "next child should start at y=0, got {}",
            rects_after[1].y
        );

        set_drag_source_snapshot_collapse_key(None);
    }

    #[test]
    fn vstack_reclaims_space_when_drag_source_collapses() {
        use crate::layout::axis::Axis;
        use crate::layout::stack::{StackLayoutParams, compute_stack_layout, layout_vstack};
        use crate::style::Rect;
        use crate::widgets::internal::StackProps;

        let card_a = DragSource::new()
            .child(Text::new("Card A"))
            .preview_snapshot();
        let card_b = DragSource::new()
            .child(Text::new("Card B"))
            .preview_snapshot();

        let el_a: Element = Element::from(card_a.clone()).key("card-a");
        let el_b: Element = Element::from(card_b.clone()).key("card-b");

        let props = StackProps {
            gap: 1,
            ..StackProps::default()
        };

        // Normal: both cards visible
        let layout_normal = compute_stack_layout(StackLayoutParams {
            props: &props,
            children: &[el_a, el_b.clone()],
            axis: Axis::Vertical,
            available: 20,
            available_cross: Some(40),
            focus: None,
            pinned_key: None,
            intrinsic_main_axis: false,
        });
        let total_normal: u16 =
            layout_normal.sizes.iter().sum::<u16>() + layout_normal.gaps.iter().sum::<u16>();
        assert!(
            total_normal > 1,
            "normal total should be > 1, got {total_normal}"
        );

        // Collapse card-a
        set_drag_source_snapshot_collapse_key(Some("card-a".into()));
        let el_a_collapsed: Element = Element::from(card_a).key("card-a");
        let el_b2: Element = Element::from(card_b).key("card-b");

        // Test compute_stack_layout
        let layout_collapsed = compute_stack_layout(StackLayoutParams {
            props: &props,
            children: &[el_a_collapsed.clone(), el_b2.clone()],
            axis: Axis::Vertical,
            available: 20,
            available_cross: Some(40),
            focus: None,
            pinned_key: None,
            intrinsic_main_axis: false,
        });
        assert_eq!(
            layout_collapsed.sizes[0], 0,
            "collapsed card should get 0 height, got {}",
            layout_collapsed.sizes[0]
        );
        assert_eq!(
            layout_collapsed.gaps.first().copied().unwrap_or(0),
            0,
            "gap after collapsed card should be 0"
        );

        // Test layout_vstack (produces actual rects)
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };
        let rects = layout_vstack(&props, &[el_a_collapsed, el_b2], bounds, None, None);
        assert_eq!(
            rects[0].h, 0,
            "collapsed card rect should have h=0, got {}",
            rects[0].h
        );
        assert_eq!(
            rects[1].y, 0,
            "second card should start at y=0, got {}",
            rects[1].y
        );

        set_drag_source_snapshot_collapse_key(None);
    }
}
