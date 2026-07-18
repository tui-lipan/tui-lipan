//! Stack layout computation logic.

use std::borrow::Borrow;
use std::sync::Arc;

use crate::core::component::FocusContext;
use crate::core::element::{Element, ElementKind};
use crate::layout::axis::{Axis, is_focus_protected, requested_main_axis};
use crate::layout::measure::{intrinsic_main, min_size_constrained};
use crate::style::{Length, ShrinkPriority};
use crate::widgets::DragSlot;
use crate::widgets::containers::FocusSizing;
use crate::widgets::internal::StackProps;

use super::axis_constraints;
use super::types::{
    FocusMode, FocusSizingContext, StackChildLayout, StackMainLayout, StackMeasuredSize,
};
use crate::layout::drag_source_layout_hint::drag_source_snapshot_collapse_key;
use crate::widgets::containers::layout::join_overlap_vector;

pub(crate) fn focus_sizing_context<C: Borrow<Element>>(
    props: &StackProps,
    children: &[C],
    axis: Axis,
    available: u16,
    focus: Option<&FocusContext>,
    pinned_key: Option<&str>,
) -> Option<FocusSizingContext> {
    if axis != Axis::Vertical {
        return None;
    }

    let FocusSizing::Accordion(policy) = props.focus_sizing else {
        return None;
    };

    let has_focus = children
        .iter()
        .any(|child| is_focus_protected(child.borrow(), focus));
    if !has_focus {
        // Activate accordion for the sticky key when no child has real focus.
        let has_sticky = pinned_key.is_some_and(|pk| {
            children
                .iter()
                .any(|c| c.borrow().key.as_ref().is_some_and(|k| k.as_ref() == pk))
        });
        if !has_sticky {
            return None;
        }
    }

    let mode = if available < policy.tiny_threshold {
        FocusMode::Tiny
    } else if available < policy.squash_threshold {
        FocusMode::Squashed
    } else {
        FocusMode::Accordion
    };

    Some(FocusSizingContext { mode, policy })
}

fn measure_child_for_axis(
    child: &Element,
    axis: Axis,
    available_cross: Option<u16>,
) -> StackMeasuredSize {
    let (w, h) = match axis {
        Axis::Vertical => min_size_constrained(child, available_cross, None),
        Axis::Horizontal => min_size_constrained(child, None, available_cross),
    };

    StackMeasuredSize { w, h }
}

pub(crate) fn scrollable_min_main(child: &Element, axis: Axis) -> Option<u16> {
    if axis != Axis::Vertical {
        return None;
    }

    match &child.kind {
        ElementKind::ScrollView(sv) => {
            let mut min_h: u16 = 1;
            min_h = min_h.saturating_add(sv.props.padding.vertical());
            if sv.props.border {
                min_h = min_h.saturating_add(2);
            }
            Some(min_h)
        }
        ElementKind::List(list) => {
            let mut min_h: u16 = 1;
            min_h = min_h.saturating_add(list.padding.vertical());
            if list.border {
                min_h = min_h.saturating_add(2);
            }
            Some(min_h)
        }
        ElementKind::TextArea(text_area) => {
            let mut min_h: u16 = 1;
            min_h = min_h.saturating_add(text_area.padding.vertical());
            if text_area.border {
                min_h = min_h.saturating_add(2);
            }
            Some(min_h)
        }
        _ => None,
    }
}

pub(crate) fn focused_child_min<C: Borrow<Element>>(
    children: &[C],
    axis: Axis,
    focus: Option<&FocusContext>,
    pinned_key: Option<&str>,
) -> Option<(usize, u16)> {
    for (idx, child) in children.iter().enumerate() {
        let el = child.borrow();
        let is_pinned =
            pinned_key.is_some_and(|pk| el.key.as_ref().is_some_and(|k| k.as_ref() == pk));
        if is_focus_protected(el, focus) || is_pinned {
            let (_, _, _, focus_min_main) = axis_constraints(el, axis);
            return Some((idx, focus_min_main));
        }
    }
    None
}

pub(crate) fn total_with_gaps(entries: &[StackChildLayout], gaps: &[u16]) -> u16 {
    let mut total = 0u16;
    for entry in entries {
        total = total.saturating_add(entry.size);
    }
    for gap in gaps {
        total = total.saturating_add(*gap);
    }
    total
}

pub(crate) struct InternalLayoutContext<'a> {
    pub available: u16,
    pub available_cross: Option<u16>,
    pub focus: Option<&'a FocusContext>,
    pub focus_sizing: Option<FocusSizingContext>,
    pub policy_focus_min: u16,
    /// Key of the pseudo-focused child (sticky accordion, no real focus).
    pub pinned_key: Option<Arc<str>>,
    pub join_overlaps: Vec<bool>,
    pub join_count: u16,
    /// When the stack's main-axis budget is unknown (e.g. `measure_stack` with no
    /// parent height), flex children must size from **content** like `Length::Auto`,
    /// matching reconcile once the stack is allocated its intrinsic height.
    /// Otherwise flex uses minimal bases and grows into the definite budget.
    pub intrinsic_main_axis: bool,
}

#[derive(Clone)]
struct StackChildBaseLayout {
    len: Length,
    base: u16,
    constraint_min: u16,
    min_content: u16,
    shrink_min: u16,
    collapse_main: Option<u16>,
    force_compact: bool,
    focus_min_main: u16,
    protected: bool,
    shrinkable: bool,
    reflows: bool,
    shrink_priority: ShrinkPriority,
}

fn measured_main_at(child: &Element, axis: Axis, main: u16, cross: Option<u16>) -> u16 {
    let (w, h) = match axis {
        Axis::Vertical => min_size_constrained(child, cross, Some(main)),
        Axis::Horizontal => min_size_constrained(child, Some(main), cross),
    };
    match axis {
        Axis::Vertical => h,
        Axis::Horizontal => w,
    }
}

/// After main-axis sizing, shrink every reflowing child to the main its content
/// actually uses at its allocated size, so the freed cells become justify space.
///
/// A reflowing child (a wrapping Flow, etc.) allocated more than one wrapped row
/// needs leaves dead space inside its rect; trimming it lets e.g. `SpaceBetween`
/// push a right-hand group to the edge instead of it hugging a left group with
/// empty trailing space. `First` children may shrink below `min_content` (they
/// truncate); others stay at >= `min_content`. Gradual wrapping is handled
/// earlier by the two-tier shrink (everything reaches its `min_content` floor
/// before any `First` child truncates), so no redistribution is needed here.
fn fit_reflow_children_to_content<C: Borrow<Element>>(
    children: &[C],
    child_bases: &[StackChildBaseLayout],
    sizes: &mut [u16],
    axis: Axis,
    available_cross: Option<u16>,
) {
    for (idx, child) in children.iter().enumerate() {
        let Some(meta) = child_bases.get(idx) else {
            continue;
        };
        if !meta.reflows {
            continue;
        }
        let size = sizes[idx];
        if size == 0 {
            continue;
        }
        let floor = if meta.shrink_priority == ShrinkPriority::First {
            meta.constraint_min
        } else {
            meta.min_content.max(meta.constraint_min)
        };
        let used = measured_main_at(child.borrow(), axis, size, available_cross).max(floor);
        if used < size {
            sizes[idx] = used;
        }
    }
}

/// Returns `true` when `child` is a SourceSnapshot drag source whose element
/// key matches the active collapse hint (layout then uses `drag_slot`).
fn is_drag_source_collapse_target(child: &Element) -> bool {
    if let ElementKind::DragSource(source) = &child.kind
        && matches!(source.preview, crate::widgets::DragPreview::SourceSnapshot)
        && let (Some(ek), Some(collapse)) =
            (child.key.as_ref(), drag_source_snapshot_collapse_key())
    {
        return ek == &collapse;
    }
    false
}

fn build_child_base_layouts<C: Borrow<Element>>(
    props: &StackProps,
    children: &[C],
    axis: Axis,
    ctx: &InternalLayoutContext,
) -> (Vec<StackChildBaseLayout>, Vec<Option<StackMeasuredSize>>) {
    let mut entries = Vec::with_capacity(children.len());
    let mut measured_sizes = vec![None; children.len()];

    #[derive(Clone, Copy)]
    struct ChildSeed {
        len: Length,
        base: u16,
        constraint_min: u16,
        min_content: u16,
        shrink_min: u16,
        collapse_main: Option<u16>,
        force_compact: bool,
        focus_min_main: u16,
        protected: bool,
        shrinkable: bool,
        reflows: bool,
        shrink_priority: ShrinkPriority,
    }

    let mut seeds = Vec::with_capacity(children.len());

    for (idx, child) in children.iter().enumerate() {
        let child = child.borrow();

        let drag_snapshot_slot = if is_drag_source_collapse_target(child) {
            let ElementKind::DragSource(source) = &child.kind else {
                unreachable!("is_drag_source_collapse_target implies DragSource");
            };
            Some(source.drag_slot)
        } else {
            None
        };

        if matches!(drag_snapshot_slot, Some(DragSlot::Collapse)) {
            let seed = ChildSeed {
                len: Length::Px(0),
                base: 0,
                constraint_min: 0,
                min_content: 0,
                shrink_min: 0,
                collapse_main: Some(0),
                force_compact: true,
                focus_min_main: 0,
                protected: false,
                shrinkable: false,
                reflows: false,
                shrink_priority: ShrinkPriority::Normal,
            };
            seeds.push(seed);
            continue;
        }

        let snapshot_drag_len = match drag_snapshot_slot {
            Some(DragSlot::Specified(l)) => Some(l),
            _ => None,
        };

        let (constraint_min_len, collapse_main, force_compact, focus_min_main) =
            axis_constraints(child, axis);
        let layout_constraints = child.layout_constraints();
        let constraint_min = if ctx.intrinsic_main_axis {
            constraint_min_len.resolve_as_min(0)
        } else {
            constraint_min_len.resolve_as_min(ctx.available)
        };
        let child_is_pinned = ctx
            .pinned_key
            .as_deref()
            .is_some_and(|pk| child.key.as_ref().is_some_and(|k| k.as_ref() == pk));
        let len = if let Some(override_len) = snapshot_drag_len {
            override_len
        } else if child_is_pinned && !is_focus_protected(child, ctx.focus) {
            match &child.kind {
                ElementKind::Frame(frame) if axis == Axis::Vertical => frame.props.height,
                _ => requested_main_axis(child, axis, ctx.focus),
            }
        } else {
            requested_main_axis(child, axis, ctx.focus)
        };
        let (min_content, max_content) = intrinsic_main(child, axis, ctx.available_cross);

        let base = match len {
            Length::Px(px) => px,
            Length::Percent(percent) => {
                let percent = percent.min(100);
                let avail_ref = if ctx.intrinsic_main_axis {
                    0
                } else {
                    ctx.available
                };
                ((avail_ref as u32).saturating_mul(percent as u32) / 100).min(u16::MAX as u32)
                    as u16
            }
            Length::Auto => {
                let measured = measure_child_for_axis(child, axis, ctx.available_cross);
                let base = measured.main_axis(axis);
                measured_sizes[idx] = Some(measured);
                max_content.max(base)
            }
            Length::Flex(_) if ctx.intrinsic_main_axis => {
                let measured = measure_child_for_axis(child, axis, ctx.available_cross);
                let base = measured.main_axis(axis);
                measured_sizes[idx] = Some(measured);
                max_content.max(base)
            }
            Length::Flex(_) => match axis {
                Axis::Horizontal => 0,
                Axis::Vertical => scrollable_min_main(child, axis).unwrap_or(0),
            },
        };

        let len = if ctx.intrinsic_main_axis && matches!(len, Length::Flex(_)) {
            Length::Auto
        } else {
            len
        };

        let visible_min = u16::from(base > 0);
        let shrink_min = match len {
            Length::Auto | Length::Flex(_) => {
                if layout_constraints.reflows {
                    // A reflowing child's HARD floor is a single visible cell, so
                    // as a last resort it clips its content rather than being
                    // dropped entirely. Its readable floor (`min_content`, the
                    // widest item it can show whole) is enforced separately by the
                    // stack's tier-1 shrink, which keeps it whole until every
                    // group has reached that floor.
                    constraint_min.max(visible_min)
                } else if constraint_min == base {
                    // Non-reflowing children keep the historical auto-shrink
                    // heuristic: if the apparent min came from the child's own
                    // natural measurement, keep only a visible floor.
                    visible_min
                } else {
                    constraint_min.max(visible_min)
                }
            }
            Length::Px(_) | Length::Percent(_) => constraint_min,
        };

        seeds.push(ChildSeed {
            len,
            base,
            constraint_min,
            min_content,
            shrink_min,
            collapse_main,
            force_compact,
            focus_min_main,
            protected: is_focus_protected(child, ctx.focus) || child_is_pinned,
            shrinkable: matches!(len, Length::Auto | Length::Flex(_)),
            reflows: layout_constraints.reflows,
            shrink_priority: layout_constraints.shrink_priority,
        });
    }

    if !ctx.intrinsic_main_axis {
        let reserve_size = |seed: ChildSeed| -> u16 {
            if seed.force_compact
                && let Some(collapse) = seed.collapse_main
            {
                return collapse;
            }
            match seed.len {
                Length::Px(_) | Length::Percent(_) => seed.base.max(seed.constraint_min),
                // In bounded stacks, shrinkable children reserve only their
                // visible floor (or an explicit min), not their full intrinsic
                // auto/container measurement. Otherwise a tall Auto subtree
                // becomes unsplittable and later siblings disappear under caps.
                Length::Auto | Length::Flex(_) => seed.shrink_min,
            }
        };

        let reserve_sizes: Vec<u16> = seeds.iter().copied().map(reserve_size).collect();
        let reserve_gaps: Vec<u16> = reserve_sizes
            .windows(2)
            .map(|pair| {
                if pair[0] > 0 && pair[1] > 0 {
                    props.gap
                } else {
                    0
                }
            })
            .collect();
        let total_reserved = total_with_gaps(
            &reserve_sizes
                .iter()
                .copied()
                .map(|size| StackChildLayout {
                    flex: 0,
                    collapse_main: None,
                    protected: false,
                    size,
                    compact: false,
                    min_size: 0,
                    min_content: 0,
                    shrinkable: false,
                    shrink_priority: ShrinkPriority::Normal,
                })
                .collect::<Vec<_>>(),
            &reserve_gaps,
        );
        let effective_available = ctx.available.saturating_add(ctx.join_count);

        for (idx, _child) in children.iter().enumerate() {
            let seed = &mut seeds[idx];
            // Reflowing children keep their max-content base so the overflow
            // shrink can reduce them proportionally (largest first) and wrap them
            // gradually. Clamping their base to a fair budget here would tie their
            // size to a rigid sibling's and make one collapse fully instead.
            if seed.reflows {
                continue;
            }
            if !(matches!(seed.len, Length::Auto)
                || matches!(seed.len, Length::Flex(_)) && seed.base > 0)
            {
                continue;
            }

            let budget = effective_available
                .saturating_sub(total_reserved.saturating_sub(reserve_sizes[idx]));
            // When Px/Percent siblings oversubscribe `effective_available`, the
            // budget can drop below this child's visible floor. Don't squash
            // Auto/Flex below their intrinsic minimum here - leave the deficit
            // for `run_stack_layout_pass`, which can last-resort-shrink the
            // oversubscribed Px sibling instead of pushing Auto siblings to 0.
            let clipped = seed.base.min(budget).max(seed.shrink_min.min(seed.base));
            seed.base = clipped;
            if let Some(mut measured) = measured_sizes[idx] {
                match axis {
                    Axis::Vertical => measured.h = seed.base,
                    Axis::Horizontal => measured.w = seed.base,
                }
                measured_sizes[idx] = Some(measured);
            }
        }
    }

    for seed in seeds {
        entries.push(StackChildBaseLayout {
            len: seed.len,
            base: seed.base,
            constraint_min: seed.constraint_min,
            min_content: seed.min_content,
            shrink_min: seed.shrink_min,
            collapse_main: seed.collapse_main,
            force_compact: seed.force_compact,
            focus_min_main: seed.focus_min_main,
            protected: seed.protected,
            shrinkable: seed.shrinkable,
            reflows: seed.reflows,
            shrink_priority: seed.shrink_priority,
        });
    }

    (entries, measured_sizes)
}

fn run_stack_layout_pass(
    props: &StackProps,
    _axis: Axis,
    ctx: &InternalLayoutContext,
    child_bases: &[StackChildBaseLayout],
    measured_sizes: Vec<Option<StackMeasuredSize>>,
    enforce_focus_min: bool,
) -> StackMainLayout {
    let mut entries = Vec::with_capacity(child_bases.len());
    for child in child_bases {
        let mut min_main = child.base.max(child.constraint_min);
        let visible_min = match child.len {
            Length::Auto if child.base > 0 => 1,
            _ => 0,
        };
        if enforce_focus_min && child.protected {
            let focus_min = if child.focus_min_main > 0 {
                child.focus_min_main
            } else {
                ctx.policy_focus_min
            };
            if focus_min > 0 {
                min_main = min_main.max(focus_min);
            }
        }
        let mut flex = match child.len {
            Length::Flex(f) => f.max(1),
            _ => 0,
        };
        let mut size = min_main;
        let mut compact = false;
        if child.force_compact
            && let Some(collapse) = child.collapse_main
        {
            size = collapse;
            compact = true;
        }

        if let Some(policy_ctx) = ctx.focus_sizing
            && !compact
        {
            match policy_ctx.mode {
                FocusMode::Accordion => {
                    if child.protected && flex > 0 {
                        let weight = policy_ctx.policy.expanded_weight.max(1);
                        flex = flex.saturating_mul(weight);
                    }
                }
                FocusMode::Squashed | FocusMode::Tiny => {
                    if child.protected {
                        if flex == 0 && matches!(child.len, Length::Auto) {
                            flex = 1;
                        }
                    } else {
                        let target = match policy_ctx.mode {
                            FocusMode::Tiny => policy_ctx.policy.tiny_collapsed,
                            _ => policy_ctx.policy.collapsed,
                        };
                        if target > 0 {
                            if child.collapse_main.is_some() {
                                size = target;
                            } else {
                                size = size.max(target);
                            }
                            flex = 0;
                            if matches!(policy_ctx.mode, FocusMode::Tiny) && target <= 1 {
                                compact = true;
                            }
                        }
                    }
                }
            }
        }
        entries.push(StackChildLayout {
            flex,
            collapse_main: child.collapse_main,
            protected: child.protected,
            size,
            compact,
            min_size: child.shrink_min.max(visible_min),
            min_content: if child.reflows {
                child.min_content.max(child.shrink_min).max(visible_min)
            } else {
                child.shrink_min.max(visible_min)
            },
            shrinkable: child.shrinkable,
            shrink_priority: child.shrink_priority,
        });
    }

    let base_gap = props.gap;
    let mut gaps = vec![0u16; child_bases.len().saturating_sub(1)];
    let effective_available = ctx.available.saturating_add(ctx.join_count);
    let update_gaps = |gaps: &mut Vec<u16>, entries: &[StackChildLayout]| {
        let occupies = |e: &StackChildLayout| e.size > 0 || (e.flex > 0 && !e.compact);
        let compact_suppresses_gap = |e: &StackChildLayout| e.compact && e.size > 0;

        // Reset before recomputing - callers may invoke this repeatedly as
        // sizes change (e.g. drop-on-overflow), and stale `base_gap` values
        // adjacent to newly-zeroed children would otherwise inflate
        // `total_with_gaps` and trick overflow loops into over-dropping.
        for gap in gaps.iter_mut() {
            *gap = 0;
        }

        let occ: Vec<usize> = (0..entries.len())
            .filter(|&i| occupies(&entries[i]))
            .collect();
        for w in occ.windows(2) {
            let a = w[0];
            let b = w[1];
            if a + 1 == b {
                let idx = a;
                if compact_suppresses_gap(&entries[idx])
                    || compact_suppresses_gap(&entries[idx + 1])
                {
                    continue;
                }
                gaps[idx] = base_gap;
            } else {
                if compact_suppresses_gap(&entries[b - 1]) || compact_suppresses_gap(&entries[b]) {
                    continue;
                }
                gaps[b - 1] = base_gap;
            }
        }
    };
    update_gaps(&mut gaps, &entries);

    let mut total = total_with_gaps(&entries, &gaps);
    if total > effective_available {
        let mut overflow = total.saturating_sub(effective_available);

        // Tier 1: shrink every child down to its readable (`min_content`) floor,
        // largest first. Reflowing groups thus wrap gradually *together* (one
        // item at a time) instead of one group collapsing fully before the other
        // moves. For non-reflowing children `min_content == min_size`, so this is
        // the historical single-pass shrink.
        let mut tier1: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.compact && e.shrinkable && e.size > e.min_content)
            .map(|(idx, _)| idx)
            .collect();
        tier1.sort_by_key(|&idx| std::cmp::Reverse(entries[idx].size));
        for idx in tier1 {
            if overflow == 0 {
                break;
            }
            let entry = &mut entries[idx];
            let cap = entry.size.saturating_sub(entry.min_content);
            if cap == 0 {
                continue;
            }
            let take = overflow.min(cap);
            entry.size = entry.size.saturating_sub(take);
            overflow = overflow.saturating_sub(take);
        }

        // Tier 2: still overflowing — let children that may go below their
        // readable floor (reflowing groups, which then truncate) yield the rest.
        // `First` priority truncates before `Normal`; within a class, largest
        // first.
        if overflow > 0 {
            let mut tier2: Vec<usize> = entries
                .iter()
                .enumerate()
                .filter(|(_, e)| !e.compact && e.shrinkable && e.size > e.min_size)
                .map(|(idx, _)| idx)
                .collect();
            tier2.sort_by_key(|&idx| {
                (
                    std::cmp::Reverse(entries[idx].shrink_priority),
                    std::cmp::Reverse(entries[idx].size),
                )
            });
            for idx in tier2 {
                if overflow == 0 {
                    break;
                }
                let entry = &mut entries[idx];
                let cap = entry.size.saturating_sub(entry.min_size);
                if cap == 0 {
                    continue;
                }
                let take = overflow.min(cap);
                entry.size = entry.size.saturating_sub(take);
                overflow = overflow.saturating_sub(take);
            }
        }

        // Collapse is a coarse fallback for panels like bordered Frames. Use it
        // only after normal shrink has failed; otherwise a one-line deficit can
        // collapse every panel and leave most of the stack empty.
        if overflow > 0 {
            let mut collapse_indices: Vec<usize> = entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    !entry.compact
                        && !entry.protected
                        && entry
                            .collapse_main
                            .is_some_and(|collapse| entry.size > collapse)
                })
                .map(|(idx, _)| idx)
                .collect();

            collapse_indices.sort_by_key(|&idx| {
                let collapse = entries[idx].collapse_main.unwrap_or(0);
                entries[idx].size.saturating_sub(collapse)
            });

            for idx in collapse_indices {
                if overflow == 0 {
                    break;
                }
                let prev_total = total_with_gaps(&entries, &gaps);
                let collapse = entries[idx].collapse_main.unwrap_or(0);
                entries[idx].size = entries[idx].size.min(collapse);
                entries[idx].compact = true;
                update_gaps(&mut gaps, &entries);
                let new_total = total_with_gaps(&entries, &gaps);
                let saved = prev_total.saturating_sub(new_total);
                overflow = overflow.saturating_sub(saved);
            }
        }

        // If shrinking/collapse can't absorb the deficit, drop non-protected shrinkable
        // children entirely (size 0, gap suppressed) one at a time. Forward
        // source order: the children closest to the rigid Px/Percent anchor
        // (typically last) stay visible longest. This honors Px as rigid -
        // rather than squashing it below its requested size, whole Auto/Flex
        // siblings hide progressively from the top.
        if overflow > 0 {
            let mut drop_order: Vec<usize> = (0..entries.len())
                .filter(|&idx| {
                    let e = &entries[idx];
                    !e.compact && !e.protected && e.shrinkable && e.size > 0
                })
                .collect();
            // Drop yielding (`First`) children before rigid ones, mirroring the
            // shrink order, so a priority group (e.g. action buttons) stays
            // visible longest. Stable: source order is kept within a priority.
            drop_order.sort_by_key(|&idx| std::cmp::Reverse(entries[idx].shrink_priority));

            for idx in drop_order {
                if overflow == 0 {
                    break;
                }
                let prev_total = total_with_gaps(&entries, &gaps);
                entries[idx].size = 0;
                update_gaps(&mut gaps, &entries);
                let new_total = total_with_gaps(&entries, &gaps);
                let saved = prev_total.saturating_sub(new_total);
                overflow = overflow.saturating_sub(saved);
            }
        }
    }

    total = total_with_gaps(&entries, &gaps);
    if total < effective_available {
        let extra = effective_available - total;
        let mut flex_sum = 0u16;
        for entry in &entries {
            if entry.flex > 0 && !entry.compact {
                flex_sum = flex_sum.saturating_add(entry.flex);
            }
        }
        if flex_sum > 0 {
            let mut used = 0u16;
            let mut flex_indices = Vec::new();
            for (idx, entry) in entries.iter_mut().enumerate() {
                if entry.flex == 0 || entry.compact {
                    continue;
                }
                let add = (extra as u32 * entry.flex as u32 / flex_sum as u32).min(u16::MAX as u32)
                    as u16;
                entry.size = entry.size.saturating_add(add);
                used = used.saturating_add(add);
                flex_indices.push(idx);
            }
            let mut remainder = extra.saturating_sub(used);
            if !flex_indices.is_empty() {
                let remainder_order: Vec<usize> = if props.even_flex {
                    flex_indices.iter().rev().copied().collect()
                } else {
                    flex_indices.clone()
                };
                let mut i = 0usize;
                while remainder > 0 {
                    let idx = remainder_order[i % remainder_order.len()];
                    entries[idx].size = entries[idx].size.saturating_add(1);
                    remainder = remainder.saturating_sub(1);
                    i = i.saturating_add(1);
                }
            }
        }
    }

    let sizes: Vec<u16> = entries.into_iter().map(|entry| entry.size).collect();

    // Zero-size children should not contribute gaps. Walk the children and
    // suppress the gap between a zero-size child and its neighbour, but
    // preserve exactly one gap between any two consecutive non-zero children
    // even when separated by zero-size children.
    {
        let mut prev_nonzero = false;
        for idx in 0..gaps.len() {
            let cur_nonzero = sizes[idx] > 0;
            let next_nonzero = sizes[idx + 1] > 0;
            if cur_nonzero {
                prev_nonzero = true;
            }
            // Keep the gap only when both the preceding non-zero child and
            // the following child are non-zero.
            if !(prev_nonzero && next_nonzero) {
                gaps[idx] = 0;
            }
            // If we placed a gap before the next non-zero child, reset
            // so that zero-size children between the next pair don't
            // duplicate it.
            if next_nonzero {
                prev_nonzero = false;
            }
        }
    }

    StackMainLayout {
        sizes,
        gaps,
        measured_sizes,
        join_overlaps: ctx.join_overlaps.clone(),
        join_count: ctx.join_count,
    }
}

pub(crate) struct StackLayoutParams<'a, C> {
    pub props: &'a StackProps,
    pub children: &'a [C],
    pub axis: Axis,
    pub available: u16,
    pub available_cross: Option<u16>,
    pub focus: Option<&'a FocusContext>,
    pub pinned_key: Option<&'a str>,
    pub intrinsic_main_axis: bool,
}

pub(crate) fn compute_stack_layout<C: Borrow<Element>>(
    params: StackLayoutParams<'_, C>,
) -> StackMainLayout {
    let StackLayoutParams {
        props,
        children,
        axis,
        available,
        available_cross,
        focus,
        pinned_key,
        intrinsic_main_axis,
    } = params;
    // Only apply sticky pinning when no child has real focus - prevents marking
    // two children as protected simultaneously.
    let effective_pinned = if children
        .iter()
        .any(|c| is_focus_protected(c.borrow(), focus))
    {
        None
    } else {
        pinned_key
    };
    let focus_sizing =
        focus_sizing_context(props, children, axis, available, focus, effective_pinned);
    let policy_focus_min = focus_sizing.map(|ctx| ctx.policy.focused_min).unwrap_or(0);
    let mut ctx = InternalLayoutContext {
        available,
        available_cross,
        focus,
        focus_sizing,
        policy_focus_min,
        pinned_key: effective_pinned.map(Arc::from),
        join_overlaps: join_overlap_vector(children),
        join_count: 0,
        intrinsic_main_axis,
    };
    ctx.join_count = ctx.join_overlaps.iter().filter(|&&j| j).count() as u16;

    let (child_bases, measured_sizes) = build_child_base_layouts(props, children, axis, &ctx);

    let mut base = run_stack_layout_pass(
        props,
        axis,
        &ctx,
        &child_bases,
        measured_sizes.clone(),
        false,
    );
    fit_reflow_children_to_content(
        children,
        &child_bases,
        &mut base.sizes,
        axis,
        available_cross,
    );
    let Some((focus_idx, focus_min)) =
        focused_child_min(children, axis, focus, ctx.pinned_key.as_deref())
    else {
        return base;
    };
    let effective_focus_min = if focus_min > 0 {
        focus_min
    } else {
        policy_focus_min
    };
    if effective_focus_min == 0 {
        return base;
    }
    let focused_size = base.sizes.get(focus_idx).copied().unwrap_or(0);
    if focused_size >= effective_focus_min {
        return base;
    }

    let mut enforced = run_stack_layout_pass(props, axis, &ctx, &child_bases, measured_sizes, true);
    fit_reflow_children_to_content(
        children,
        &child_bases,
        &mut enforced.sizes,
        axis,
        available_cross,
    );

    let mut enforced_total = 0u16;
    for size in &enforced.sizes {
        enforced_total = enforced_total.saturating_add(*size);
    }
    for gap in &enforced.gaps {
        enforced_total = enforced_total.saturating_add(*gap);
    }
    let effective_available = available.saturating_add(enforced.join_count);

    if enforced_total > effective_available {
        return base;
    }

    enforced
}

#[cfg(test)]
mod tests {
    use crate::core::element::Element;
    use crate::layout::axis::Axis;
    use crate::style::{Length, ShrinkPriority};
    use crate::widgets::{Flow, Text};

    use super::{StackChildBaseLayout, fit_reflow_children_to_content};

    fn base(min_content: u16, shrink_min: u16, priority: ShrinkPriority) -> StackChildBaseLayout {
        StackChildBaseLayout {
            len: Length::Auto,
            base: 6,
            constraint_min: 0,
            min_content,
            shrink_min,
            collapse_main: None,
            force_compact: false,
            focus_min_main: 0,
            protected: false,
            shrinkable: true,
            reflows: true,
            shrink_priority: priority,
        }
    }

    #[test]
    fn fit_to_content_trims_a_reflowing_child_via_the_generic_reflows_flag() {
        // A reflowing child allocated wider than its wrapped content uses is
        // trimmed to that content; the freed cells are left for justify, NOT
        // handed to the sibling. The function keys off the generic `reflows`
        // flag, not any Flow-specific knowledge.
        let donor: Element = Flow::new()
            .child(Text::new("aaa"))
            .child(Text::new("bbb"))
            .into();
        let other: Element = Text::new("right").into();
        let children = vec![donor, other];
        // donor min-content 3 (widest child); allocated 4 wraps to two rows of
        // width 3, so it trims to 3. The non-reflow sibling is left alone.
        let bases = vec![
            base(3, 3, ShrinkPriority::Normal),
            StackChildBaseLayout {
                reflows: false,
                ..base(5, 5, ShrinkPriority::Normal)
            },
        ];
        let mut sizes = vec![4, 5];

        fit_reflow_children_to_content(&children, &bases, &mut sizes, Axis::Horizontal, None);

        assert_eq!(sizes, vec![3, 5]);
    }
}
