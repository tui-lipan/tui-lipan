use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::core::element::{Element, ElementKind};
use crate::layout::measure::min_size_constrained;
use crate::style::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SplitPaneSide {
    Left,
    Right,
}

/// Shared state for split DiffView panes: inner widths from layout plus per-pane vertical
/// scrollbar columns (standalone gutter + gap), used to simulate peer **content** width.
///
/// `layout_pass`: `0` = normal (measure / single reconcile); `1` = record raw per-source
/// visual heights only; `2` = insert padding from peer's pass-1 heights (same frame).
#[derive(Default)]
pub(crate) struct SplitWrapSyncState {
    left_pane_width: Option<u16>,
    right_pane_width: Option<u16>,
    /// Standalone vertical scrollbar columns reserved on the left pane (usually 0).
    left_scrollbar_cols: u16,
    /// Standalone vertical scrollbar columns reserved on the right pane (`single_scrollbar` split).
    right_scrollbar_cols: u16,
    layout_pass: u8,
    pass1_left_heights: Option<Arc<Vec<u16>>>,
    pass1_right_heights: Option<Arc<Vec<u16>>>,
}

pub(crate) type SharedSplitWrapSync = Rc<RefCell<SplitWrapSyncState>>;

pub(crate) fn new_split_wrap_sync_state() -> SharedSplitWrapSync {
    Rc::new(RefCell::new(SplitWrapSyncState::default()))
}

pub(crate) struct SplitWrapDualPass {
    sync: SharedSplitWrapSync,
}

impl SplitWrapDualPass {
    pub(crate) const PASSES: [u8; 2] = [1, 2];

    pub(crate) fn begin_measure<'a>(
        children: impl IntoIterator<Item = (&'a Element, u16)>,
        max_cross: Option<u16>,
    ) -> Option<Self> {
        let children = children.into_iter().collect::<Vec<_>>();
        let sync = shared_split_wrap_sync_for_dual_pass(children.iter().map(|(child, _)| *child))?;

        for (child, width) in &children {
            update_split_wrap_width_hint(child, *width);
        }

        reset_split_wrap_layout_after_hstack(&sync);
        set_split_wrap_layout_pass(&sync, 1);
        for (child, width) in &children {
            let _ = min_size_constrained(child, Some(*width), max_cross);
        }
        set_split_wrap_layout_pass(&sync, 2);

        Some(Self { sync })
    }

    pub(crate) fn begin_reconcile<'a>(
        children: impl IntoIterator<Item = (&'a Element, u16)>,
    ) -> Option<Self> {
        let children = children.into_iter().collect::<Vec<_>>();
        let sync = shared_split_wrap_sync_for_dual_pass(children.iter().map(|(child, _)| *child))?;

        for (child, width) in &children {
            update_split_wrap_width_hint(child, *width);
        }

        reset_split_wrap_layout_after_hstack(&sync);
        Some(Self { sync })
    }

    pub(crate) fn set_pass(&self, pass: u8) {
        set_split_wrap_layout_pass(&self.sync, pass);
    }
}

impl Drop for SplitWrapDualPass {
    fn drop(&mut self) {
        reset_split_wrap_layout_after_hstack(&self.sync);
    }
}

pub(crate) fn update_split_wrap_pane_width(
    state: &SharedSplitWrapSync,
    side: SplitPaneSide,
    pane_width: u16,
) {
    let mut state = state.borrow_mut();
    match side {
        SplitPaneSide::Left => state.left_pane_width = Some(pane_width),
        SplitPaneSide::Right => state.right_pane_width = Some(pane_width),
    }
}

pub(crate) fn update_split_wrap_width_hint(el: &Element, available_width: u16) {
    match &el.kind {
        ElementKind::DocumentView(doc) => {
            if let (Some(sync), Some(side)) = (&doc.split_wrap_sync, doc.split_wrap_side) {
                update_split_wrap_pane_width(sync, side, available_width);
            }
        }
        ElementKind::TextArea(area) => {
            if let (Some(sync), Some(side)) = (&area.split_wrap_sync, area.split_wrap_side) {
                update_split_wrap_pane_width(sync, side, available_width);
            }
        }
        ElementKind::Frame(frame) => {
            let inner_width = crate::widgets::frame::box_metrics::frame_inner_max_size(
                frame,
                Some(available_width),
                None,
            )
            .0
            .unwrap_or(available_width);
            if let Some(child) = frame.child.as_deref() {
                update_split_wrap_width_hint(child, inner_width);
            }
        }
        ElementKind::Group(group) => {
            update_split_wrap_width_hint(group.child.as_ref(), available_width)
        }
        ElementKind::EffectScope(scope) => {
            if let Some(child) = scope.child.as_deref() {
                update_split_wrap_width_hint(child, available_width);
            }
        }
        ElementKind::MouseRegion(region) => {
            if let Some(child) = region.child.as_deref() {
                update_split_wrap_width_hint(child, available_width);
            }
        }
        ElementKind::ContextProvider(provider) => {
            update_split_wrap_width_hint(&provider.child, available_width)
        }
        _ => {}
    }
}

pub(crate) fn split_wrap_pane_widths(
    state: &SharedSplitWrapSync,
    side: SplitPaneSide,
) -> Option<(u16, u16)> {
    let state = state.borrow();
    match side {
        SplitPaneSide::Left => Some((state.left_pane_width?, state.right_pane_width?)),
        SplitPaneSide::Right => Some((state.right_pane_width?, state.left_pane_width?)),
    }
}

pub(crate) fn set_split_wrap_scrollbar_cols(
    state: &SharedSplitWrapSync,
    left_cols: u16,
    right_cols: u16,
) {
    let mut s = state.borrow_mut();
    s.left_scrollbar_cols = left_cols;
    s.right_scrollbar_cols = right_cols;
}

/// `(left, right)` scrollbar column counts (for visual cache keys).
pub(crate) fn split_wrap_scrollbar_cols_pair(state: &SharedSplitWrapSync) -> (u16, u16) {
    let s = state.borrow();
    (s.left_scrollbar_cols, s.right_scrollbar_cols)
}

pub(crate) fn split_wrap_layout_pass(state: &SharedSplitWrapSync) -> u8 {
    state.borrow().layout_pass
}

pub(crate) fn set_split_wrap_layout_pass(state: &SharedSplitWrapSync, pass: u8) {
    state.borrow_mut().layout_pass = pass;
}

/// Reset `layout_pass` to `0` and drop pass-1 height buffers.
///
/// Call **before** a dual-pass split-wrap reconcile (clean stale state) and **after** (so
/// measure and single-pass paths see `layout_pass == 0`).
pub(crate) fn reset_split_wrap_layout_after_hstack(state: &SharedSplitWrapSync) {
    let mut s = state.borrow_mut();
    s.layout_pass = 0;
    s.pass1_left_heights = None;
    s.pass1_right_heights = None;
}

pub(crate) fn record_pass1_source_heights(
    state: &SharedSplitWrapSync,
    side: SplitPaneSide,
    heights: &[u16],
) {
    let mut s = state.borrow_mut();
    let arc = Arc::new(heights.to_vec());
    match side {
        SplitPaneSide::Left => s.pass1_left_heights = Some(arc),
        SplitPaneSide::Right => s.pass1_right_heights = Some(arc),
    }
}

pub(crate) fn peer_pass1_source_heights(
    state: &SharedSplitWrapSync,
    side: SplitPaneSide,
) -> Option<Arc<Vec<u16>>> {
    let s = state.borrow();
    match side {
        SplitPaneSide::Left => s.pass1_right_heights.clone(),
        SplitPaneSide::Right => s.pass1_left_heights.clone(),
    }
}

pub(crate) fn compute_split_wrap_padding_from_heights(
    own_source_heights: &[u16],
    peer_source_heights: &[u16],
) -> Vec<u16> {
    if own_source_heights.is_empty() {
        return Vec::new();
    }
    let mut padding = Vec::with_capacity(own_source_heights.len());
    for (idx, own_h) in own_source_heights.iter().copied().enumerate() {
        let peer_h = peer_source_heights.get(idx).copied().unwrap_or(0);
        padding.push(peer_h.saturating_sub(own_h));
    }
    padding
}

/// First `DocumentView` / `TextArea` under `el` with split-wrap sync (DFS: typical pane `Frame`).
pub(crate) fn split_wrap_sync_from_element(el: &Element) -> Option<SharedSplitWrapSync> {
    match &el.kind {
        ElementKind::DocumentView(d) => d.split_wrap_sync.clone(),
        ElementKind::TextArea(t) => t.split_wrap_sync.clone(),
        ElementKind::Frame(f) => f.child.as_deref().and_then(split_wrap_sync_from_element),
        ElementKind::Group(g) => split_wrap_sync_from_element(g.child.as_ref()),
        ElementKind::EffectScope(e) => e.child.as_deref().and_then(split_wrap_sync_from_element),
        ElementKind::MouseRegion(m) => m.child.as_deref().and_then(split_wrap_sync_from_element),
        ElementKind::ContextProvider(c) => split_wrap_sync_from_element(&c.child),
        _ => None,
    }
}

/// Returns `true` when `el` or any descendant participates in split-wrap sync.
///
/// Results are memoized on [`Element::split_wrap_probe_cache`] because this is queried from
/// [`crate::layout::measure::min_size_constrained`] for **every** node; without caching, each
/// query rescans the full subtree (e.g. every row under a large [`ScrollView`]).
pub(crate) fn element_subtree_has_split_wrap_sync(el: &Element) -> bool {
    if let Some(cached) = el.split_wrap_probe_cache.get() {
        return cached;
    }
    let out = element_subtree_has_split_wrap_sync_impl(el);
    el.split_wrap_probe_cache.set(Some(out));
    out
}

fn element_subtree_has_split_wrap_sync_impl(el: &Element) -> bool {
    match &el.kind {
        ElementKind::DocumentView(d) => d.split_wrap_sync.is_some(),
        ElementKind::TextArea(t) => t.split_wrap_sync.is_some(),
        _ => el
            .kind
            .children()
            .iter()
            .any(|c| element_subtree_has_split_wrap_sync(c)),
    }
}

/// When the HStack has ≥2 children sharing the same [`SharedSplitWrapSync`], run dual reconcile
/// passes so each pane can read the peer's **actual** pass-1 wrap counts in pass 2.
pub(crate) fn shared_split_wrap_sync_for_dual_pass<'a>(
    children: impl IntoIterator<Item = &'a Element>,
) -> Option<SharedSplitWrapSync> {
    let mut collected: Vec<SharedSplitWrapSync> = Vec::new();
    for c in children {
        if let Some(s) = split_wrap_sync_from_element(c) {
            collected.push(s);
        }
    }
    let first = collected.first()?.clone();
    if collected.len() >= 2 && collected.iter().all(|s| Rc::ptr_eq(&first, s)) {
        Some(first)
    } else {
        None
    }
}

/// Simulated peer **text** wrap width: matches [`compute_split_wrap_padding`]'s `content_w`.
///
/// Outer pane inner widths can match while peer content is narrower (e.g. right pane keeps a
/// scrollbar column under `single_scrollbar`); subtract `(sb_peer - sb_own)`.
pub(crate) fn peer_simulated_content_width(
    state: &SharedSplitWrapSync,
    side: SplitPaneSide,
    own_content_w: u16,
) -> Option<u16> {
    let (own_pane_w, peer_pane_w) = split_wrap_pane_widths(state, side)?;
    let delta = peer_pane_w as i32 - own_pane_w as i32;
    let st = state.borrow();
    let sb_own = match side {
        SplitPaneSide::Left => st.left_scrollbar_cols,
        SplitPaneSide::Right => st.right_scrollbar_cols,
    };
    let sb_peer = match side {
        SplitPaneSide::Left => st.right_scrollbar_cols,
        SplitPaneSide::Right => st.left_scrollbar_cols,
    };
    let w = (own_content_w as i32 + delta - (sb_peer as i32 - sb_own as i32)).max(1) as u16;
    Some(w)
}

pub(crate) fn compute_visual_line_count(text: &str, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    if text.is_empty() {
        return 1;
    }

    // Count-only: mirrors `wrap_spans_for_budgets(..).len()` without
    // materializing/allocating the wrapped span vectors. This runs once per peer
    // source line per measure pass, so it is hot while resizing split diffs.
    let spans = [Span::new(text)];
    crate::utils::text::count_wrapped_lines_for_budgets(&spans, width, width) as u16
}

pub(crate) fn compute_split_wrap_padding(
    own_source_heights: &[u16],
    peer_lines: &[Arc<str>],
    content_w: u16,
) -> Vec<u16> {
    if own_source_heights.is_empty() {
        return Vec::new();
    }

    let mut padding = Vec::with_capacity(own_source_heights.len());
    for (idx, own_h) in own_source_heights.iter().copied().enumerate() {
        let peer_h = peer_lines.get(idx).map_or(0, |line| {
            compute_visual_line_count(line.as_ref(), content_w)
        });
        padding.push(peer_h.saturating_sub(own_h));
    }

    padding
}

#[cfg(test)]
mod tests {
    use super::{
        SplitPaneSide, compute_split_wrap_padding, compute_split_wrap_padding_from_heights,
        compute_visual_line_count, new_split_wrap_sync_state, peer_simulated_content_width,
    };
    use std::sync::Arc;

    #[test]
    fn split_wrap_padding_simulation_covers_unicode_empty_and_overflow() {
        assert_eq!(compute_visual_line_count("", 4), 1);
        assert_eq!(compute_visual_line_count("abcdef", 3), 2);
        assert_eq!(compute_visual_line_count("ab界d", 3), 2);

        let own = vec![1, 2, 1];
        let peer = vec![
            Arc::<str>::from("abcd"),
            Arc::<str>::from("ab"),
            Arc::<str>::from("abcdef"),
        ];
        assert_eq!(compute_split_wrap_padding(&own, &peer, 3), vec![1, 0, 1]);
    }

    #[test]
    fn split_wrap_padding_from_heights_is_pairwise_saturating() {
        assert_eq!(
            compute_split_wrap_padding_from_heights(&[1, 3], &[2, 1]),
            vec![1, 0]
        );
    }

    #[test]
    fn peer_sim_narrows_when_peer_has_extra_scrollbar_column() {
        let sync = new_split_wrap_sync_state();
        {
            let mut s = sync.borrow_mut();
            s.left_pane_width = Some(40);
            s.right_pane_width = Some(40);
            s.left_scrollbar_cols = 0;
            s.right_scrollbar_cols = 1;
        }
        assert_eq!(
            peer_simulated_content_width(&sync, SplitPaneSide::Left, 30).unwrap(),
            29
        );
        assert_eq!(
            peer_simulated_content_width(&sync, SplitPaneSide::Right, 29).unwrap(),
            30
        );
    }
}
