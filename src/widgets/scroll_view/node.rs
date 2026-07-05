use std::collections::HashMap;
use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::Key;
use crate::core::node::{ScrollbarZone, ScrollbarZonesParams, WidgetNode, compute_scrollbar_zones};
use crate::style::{Rect, ScrollbarVariant, Style};
use crate::widgets::document_view::FormatCache;
use crate::widgets::document_view::node::{DocumentTableRectSelection, VisualCache};
use crate::widgets::internal::StackProps;
use crate::widgets::scroll::{KineticScrollState, ScrollAxis, SmoothScrollState};

use super::{
    ScrollEvent, ScrollKeymap, ScrollRequest, ScrollTarget, ScrollViewportEvent,
    ScrollVisibleChild, ScrollWheelBehavior,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScrollViewportSnapshot {
    pub offset: usize,
    pub metrics: super::ScrollMetrics,
    pub viewport_width: u16,
    pub children_len: usize,
    pub first_visible_index: Option<usize>,
    pub last_visible_index: Option<usize>,
    pub visible: Vec<ScrollVisibleChild>,
    pub top_indicator: bool,
    pub bottom_indicator: bool,
    pub bottom_count: usize,
}

#[derive(Clone, Debug)]
pub struct RememberedScrollAnchor {
    pub top_child_key: Option<Key>,
    pub top_child_index: usize,
    pub top_delta: i32,
    pub old_offset: usize,
    pub center_child_key: Option<Key>,
    pub center_child_index: usize,
    pub anchor_numerator: u32,
    pub anchor_denominator: u32,
    pub pinned_top: bool,
    pub pinned_bottom: bool,
}

/// State for a scroll view node.
#[derive(Clone)]
pub struct ScrollViewNode {
    pub props: StackProps,
    /// Last element-provided `ScrollView::offset` value seen during reconcile.
    /// Used to distinguish a fresh external offset request from stale props.
    pub element_offset: Option<usize>,
    /// Last element-provided `ScrollView::scroll_request` value seen during
    /// reconcile. Used to apply request props only once until the element clears
    /// or changes them.
    pub element_scroll_request: Option<ScrollRequest>,
    /// Last element-provided semantic scroll target.
    pub scroll_target: Option<ScrollTarget>,
    /// Target suppressed by explicit offset/input until the element changes it.
    pub cancelled_scroll_target: Option<ScrollTarget>,
    pub offset: usize,
    pub smooth_scroll: SmoothScrollState,
    pub(crate) wheel_scroll: KineticScrollState,
    pub max_offset: usize,
    pub scroll_offset: u16,
    pub content_height: u16,
    pub content_width: u16,
    pub viewport_height: u16,
    pub viewport_width: u16,
    pub axis: ScrollAxis,
    pub h_offset: usize,
    pub h_max_offset: usize,
    pub h_scroll_offset: u16,
    pub h_scroll_override: Option<usize>,
    pub h_scroll_handler_dirty: bool,
    /// Last frame's width used for scroll **content** layout (child column).
    /// Includes standalone-scrollbar gutter handling; used so anchor correction
    /// sees a stable `old` width when the scrollbar toggles quickly.
    pub content_viewport_w: u16,
    pub scroll_keys: ScrollKeymap,
    pub scroll_wheel: bool,
    pub scroll_wheel_multiplier: Option<u16>,
    pub h_scroll_wheel_multiplier: Option<u16>,
    pub scroll_wheel_behavior: ScrollWheelBehavior,
    pub ambient_page_scroll: bool,
    pub focusable: bool,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    pub scrollbar_thumb: Option<char>,
    pub scrollbar_thumb_style: Option<Style>,
    pub scrollbar_thumb_focus_style: Option<Style>,
    pub scrollbar_track_style: Option<Style>,
    pub h_scrollbar: bool,
    pub h_scrollbar_variant: ScrollbarVariant,
    pub h_scrollbar_gap: u16,
    pub h_scrollbar_thumb: Option<char>,
    pub h_scrollbar_thumb_style: Option<Style>,
    pub h_scrollbar_thumb_focus_style: Option<Style>,
    pub h_scrollbar_track_style: Option<Style>,
    pub show_scroll_indicators: bool,
    pub scroll_indicator_style: Style,
    pub top_indicator: bool,
    pub bottom_indicator: bool,
    pub bottom_count: usize,
    pub scroll_override: Option<usize>,
    /// When `true`, `scroll_override` was set by an input handler (mouse
    /// wheel / keyboard) and must not be overwritten by a stale element
    /// offset during a layout-only re-reconcile.
    pub scroll_handler_dirty: bool,
    pub on_scroll: Option<Callback<ScrollEvent>>,
    pub on_scroll_to: Option<Callback<usize>>,
    pub on_viewport_change: Option<Callback<ScrollViewportEvent>>,
    pub viewport_snapshot: Option<ScrollViewportSnapshot>,
    pub layout_cache: ScrollViewLayoutCache,
    pub virtual_cache: VirtualHeightCache,
    /// Selection state saved from `DocumentView` children that scrolled
    /// off-screen. Keyed by the child element's `Key`. Restored when the
    /// child scrolls back into view and gets a fresh node.
    pub offscreen_doc_selections: HashMap<Key, OffscreenDocSelection>,
}

/// Saved selection state for off-screen `DocumentView`(s) within a single
/// scroll-view child's subtree.  Each entry corresponds to one `DocumentView`
/// found during a depth-first walk, preserving order so that restore can
/// match them back to the correct widget.
#[derive(Clone)]
pub struct OffscreenDocSelection {
    pub docs: Vec<SingleDocSelection>,
}

/// Selection state for one `DocumentView`.
#[derive(Clone)]
pub struct SingleDocSelection {
    pub selection_cursor: usize,
    pub selection_anchor: Option<usize>,
    pub table_rect_selection: Option<DocumentTableRectSelection>,
    /// `DocumentView` format cache snapshot from a virtualized row.
    pub format_cache: FormatCache,
    /// `DocumentView` visual cache snapshot from a virtualized row.
    pub visual_cache: VisualCache,
    /// `visual_cache.flat_text` snapshot when this row scrolled off (virtual scroll).
    pub flat_text: Arc<str>,
    pub shared_selection_id: Option<Arc<str>>,
}

#[derive(Clone, Debug)]
pub struct HeightCacheItem {
    /// Width at which `h` was measured (for non-flex items).
    pub natural_w: u16,
    /// Measured height of the child.
    pub h: u16,
    /// Whether the child fills the full viewport width (flex).
    pub flex_w: bool,
    /// Percentage width (0-100) when width is relative to viewport.
    pub percent_w: Option<u16>,
    /// Whether a gap should follow this item.
    pub gap_after: bool,
    /// Whether this item is a portal (zero-size placeholder).
    pub is_portal: bool,
}

#[derive(Clone, Debug)]
pub struct ScrollViewHeightCache {
    pub content_hash: u64,
    /// Whether any child in this cache can change height when the viewport
    /// width changes. When true, width-only cache reuse must be disabled and
    /// a full layout pass is required.
    pub width_sensitive: bool,
    pub items: Vec<HeightCacheItem>,
}

#[derive(Clone, Debug, Default)]
pub struct ScrollViewLayoutCache {
    entries: Vec<ScrollViewLayoutCacheEntry>,
    pub height_cache: Option<ScrollViewHeightCache>,
    pub active_content_hash: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct ScrollViewLayoutCacheEntry {
    pub viewport_w: u16,
    pub layout_hash: u64,
    pub rects: Vec<Rect>,
    pub content_height: u16,
}

impl ScrollViewLayoutCache {
    pub fn get(
        &self,
        viewport_w: u16,
        layout_hash: u64,
        children_len: usize,
    ) -> Option<(Vec<Rect>, u16)> {
        self.entries
            .iter()
            .find(|entry| {
                entry.viewport_w == viewport_w
                    && entry.layout_hash == layout_hash
                    && entry.rects.len() == children_len
            })
            .map(|entry| (entry.rects.clone(), entry.content_height))
    }

    pub fn insert(
        &mut self,
        viewport_w: u16,
        layout_hash: u64,
        rects: Vec<Rect>,
        content_height: u16,
    ) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.viewport_w == viewport_w && entry.layout_hash == layout_hash)
        {
            entry.rects = rects;
            entry.content_height = content_height;
            return;
        }

        self.entries.push(ScrollViewLayoutCacheEntry {
            viewport_w,
            layout_hash,
            rects,
            content_height,
        });
        if self.entries.len() > 2 {
            self.entries.remove(0);
        }
    }

    pub fn store_heights(
        &mut self,
        content_hash: u64,
        width_sensitive: bool,
        items: Vec<HeightCacheItem>,
    ) {
        self.height_cache = Some(ScrollViewHeightCache {
            content_hash,
            width_sensitive,
            items,
        });
    }

    /// Clear cached stack layouts so the next scroll pass recomputes extents.
    ///
    /// Used when reconciled child roots differ in height from the pre-reconcile
    /// layout prediction (e.g. `DocumentView` auto-height after themed format).
    pub fn invalidate(&mut self) {
        self.entries.clear();
        self.height_cache = None;
        self.active_content_hash = None;
    }
}

/// Per-child cached measurement, keyed by the child's individual layout hash.
#[derive(Clone, Debug)]
pub struct VirtualChildEntry {
    /// The child's `element_layout_hash` at measurement time.
    pub layout_hash: u64,
    /// The child's Element `Key`, if present. Used for key-based cache
    /// migration when children are inserted/removed (index shifting).
    pub key: Option<Key>,
    /// Measured height.
    pub h: u16,
    /// Measured/resolved width.
    pub w: u16,
    /// X offset from alignment.
    pub x: i16,
    /// Whether width fills viewport (Flex).
    pub flex_w: bool,
    /// Percentage width if applicable.
    pub percent_w: Option<u16>,
    /// Whether a gap follows this item.
    pub gap_after: bool,
    /// Whether this is a portal (zero-size).
    pub is_portal: bool,
    /// Whether this cached height came from an outdated layout hash.
    pub stale: bool,
}

/// Per-child height cache that persists across frames.
///
/// Unlike `ScrollViewHeightCache`, entries are invalidated individually
/// by comparing per-child layout hashes, not a whole-content hash.
/// This allows theme changes (which don't affect layout hashes) to
/// skip measurement entirely for off-screen children.
#[derive(Clone, Debug, Default)]
pub struct VirtualHeightCache {
    /// Per-child entries indexed by child position.
    pub entries: Vec<Option<VirtualChildEntry>>,
    /// The viewport width these entries were measured at.
    pub viewport_w: u16,
    /// Whether any child is width-sensitive (height depends on viewport width).
    pub width_sensitive: bool,
    /// Running average estimator: sum of all measured children's heights.
    pub total_measured_height: u64,
    /// Running average estimator: count of measured children.
    pub measured_count: u32,
}

impl VirtualHeightCache {
    /// Best estimate for an unmeasured child's height.
    /// Uses the running average of measured children, falling back to
    /// `fallback` when no measurements exist yet.
    pub fn estimated_height(&self, fallback: u16) -> u16 {
        if self.measured_count == 0 {
            fallback
        } else {
            (self.total_measured_height / self.measured_count as u64) as u16
        }
    }

    /// Returns true when the virtual cache has missing or stale entries in the
    /// same buffered vertical zone used by virtual scroll measurement:
    /// `[offset - viewport_h, offset + 2 * viewport_h]`.
    pub fn has_unresolved_in_zone(
        &self,
        offset: usize,
        viewport_h: u16,
        gap: u16,
        estimate: u16,
        children_len: usize,
    ) -> bool {
        if self.entries.len() != children_len {
            return true;
        }

        let zone_start = offset.saturating_sub(viewport_h as usize);
        let zone_end = offset.saturating_add((viewport_h as usize).saturating_mul(2));
        let mut y = 0usize;

        for entry in &self.entries {
            let (h, has_gap, unresolved) = match entry {
                Some(entry) => (entry.h, entry.gap_after, entry.stale),
                None => (estimate, true, true),
            };
            let row_start = y;
            let row_end = y.saturating_add(h as usize);
            if unresolved && row_end >= zone_start && row_start <= zone_end {
                return true;
            }
            y = row_end;
            if has_gap {
                y = y.saturating_add(gap as usize);
            }
        }

        false
    }

    /// Best estimate for an unmeasured child's cross-axis width, used when
    /// horizontal overflow is enabled. Mirrors [`Self::estimated_height`]:
    /// averages the widths of children measured so far, returning 0 only when
    /// nothing has been measured yet (off-screen children then size to their
    /// natural width as soon as they enter the measure buffer).
    pub fn estimated_width(&self) -> u16 {
        let (sum, count) = self
            .entries
            .iter()
            .flatten()
            .filter(|e| !e.is_portal && e.w > 0)
            .fold((0u64, 0u32), |(s, c), e| (s + e.w as u64, c + 1));
        if count == 0 {
            0
        } else {
            (sum / count as u64) as u16
        }
    }

    /// Record a new measurement into the running average.
    pub fn record_measurement(&mut self, h: u16) {
        self.total_measured_height += h as u64;
        self.measured_count += 1;
    }

    /// Remove a measurement from the running average (when invalidating).
    pub fn unrecord_measurement(&mut self, h: u16) {
        self.total_measured_height = self.total_measured_height.saturating_sub(h as u64);
        self.measured_count = self.measured_count.saturating_sub(1);
    }

    /// Reset the entire cache (e.g. on width-sensitive viewport change).
    pub fn reset(&mut self) {
        self.entries.clear();
        self.total_measured_height = 0;
        self.measured_count = 0;
    }
}

impl WidgetNode for ScrollViewNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }

    fn has_on_click(&self) -> bool {
        self.on_scroll.is_some()
            || self.on_scroll_to.is_some()
            || self.scrollbar
            || self.scroll_wheel
    }

    fn scrollbar_zones(
        &self,
        id: crate::core::node::NodeId,
        rect: Rect,
        parent_border_x: Option<i16>,
        parent_border_y: Option<i16>,
    ) -> Vec<ScrollbarZone> {
        if !self.scrollbar && !self.h_scrollbar {
            return Vec::new();
        }

        let inner = rect.inner(self.props.border, self.props.padding);
        if inner.w == 0 || inner.h == 0 {
            return Vec::new();
        }

        let use_standalone_v =
            self.scrollbar && matches!(self.scrollbar_variant, ScrollbarVariant::Standalone);
        let content_w = if use_standalone_v {
            inner
                .w
                .saturating_sub(1u16.saturating_add(self.scrollbar_gap))
        } else {
            inner.w
        };

        compute_scrollbar_zones(ScrollbarZonesParams {
            id,
            rect,
            inner,
            border: self.props.border,
            scrollbar: self.scrollbar,
            scrollbar_variant: self.scrollbar_variant,
            scrollbar_gap: self.scrollbar_gap,
            h_scrollbar: self.h_scrollbar && self.axis.horizontal_enabled(),
            h_scrollbar_variant: self.h_scrollbar_variant,
            content_x: inner.x,
            content_width: content_w,
            max_content_width: self.content_width as usize,
            wrap: false,
            parent_border_x,
            parent_border_y,
        })
    }
}
