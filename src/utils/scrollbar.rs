use crate::utils::math::round_mul_div;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollbarMetrics {
    pub thumb_len: usize,
    pub thumb_start: usize,
    pub max_thumb_start: usize,
    pub max_offset: usize,
}

/// Cache key for scrollbar metrics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScrollbarCacheKey {
    pub total: usize,
    pub visible: usize,
    pub offset: usize,
    pub track_size: usize,
    pub half_cell: bool,
}

/// Cache for scrollbar metrics to avoid recomputation during a single render.
#[derive(Debug, Default)]
pub(crate) struct ScrollbarMetricsCache {
    entries: Vec<(ScrollbarCacheKey, ScrollbarMetrics)>,
}

const MAX_SCROLLBAR_CACHE_ENTRIES: usize = 64;

impl ScrollbarMetricsCache {
    /// Remove all cached entries (e.g. after a layout change).
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    /// Look up cached metrics for the given parameters.
    pub(crate) fn get(
        &self,
        total: usize,
        visible: usize,
        offset: usize,
        track_size: usize,
        half_cell: bool,
    ) -> Option<ScrollbarMetrics> {
        let key = ScrollbarCacheKey {
            total,
            visible,
            offset,
            track_size,
            half_cell,
        };
        self.entries
            .iter()
            .rev()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
    }

    /// Insert metrics into the cache.
    pub(crate) fn insert(
        &mut self,
        total: usize,
        visible: usize,
        offset: usize,
        track_size: usize,
        half_cell: bool,
        metrics: ScrollbarMetrics,
    ) {
        let key = ScrollbarCacheKey {
            total,
            visible,
            offset,
            track_size,
            half_cell,
        };
        if let Some((_, value)) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            *value = metrics;
            return;
        }
        if self.entries.len() >= MAX_SCROLLBAR_CACHE_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push((key, metrics));
    }
}

impl ScrollbarMetrics {
    pub(crate) fn new_with_track(
        total: usize,
        visible: usize,
        offset: usize,
        track_height: usize,
    ) -> Self {
        Self::compute(total, visible, offset, track_height)
    }

    /// Compute metrics in half-cell units (doubles track_height internally).
    /// Returned `thumb_len`, `thumb_start`, `max_thumb_start` are in half-cell units.
    pub(crate) fn new_with_half_track(
        total: usize,
        visible: usize,
        offset: usize,
        track_height: usize,
    ) -> Self {
        Self::compute(total, visible, offset, track_height.saturating_mul(2))
    }

    fn compute(total: usize, visible: usize, offset: usize, track_height: usize) -> Self {
        if total == 0 || visible == 0 || total <= visible || track_height == 0 {
            return Self {
                thumb_len: 0,
                thumb_start: 0,
                max_thumb_start: 0,
                max_offset: 0,
            };
        }

        let max_offset = total.saturating_sub(visible);
        let clamped_offset = offset.min(max_offset);

        let mut thumb_len = round_mul_div(track_height, visible, total)
            .max(1)
            .min(track_height);

        if total > visible && track_height > 1 && thumb_len == track_height {
            thumb_len = track_height - 1;
        }

        let max_thumb_start = track_height.saturating_sub(thumb_len);

        let thumb_start = if max_offset == 0 {
            0
        } else {
            round_mul_div(max_thumb_start, clamped_offset, max_offset)
        };

        Self {
            thumb_len,
            thumb_start,
            max_thumb_start,
            max_offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ScrollbarMetrics, ScrollbarMetricsCache};

    #[test]
    fn scrollbar_metrics_prevents_full_track_thumb_when_scrollable() {
        // total=100, visible=99 -> should be scrollable (max_offset=1).
        // track_height=10.
        // Current logic:
        // thumb_len = round(10 * 99 / 100) = round(9.9) = 10.
        // If thumb_len = 10, max_thumb_start = 0. Visually locked.
        //
        // Desired:
        // If scrollable, thumb_len <= track_height - 1 (9).
        let metrics = ScrollbarMetrics::new_with_track(100, 99, 0, 10);
        assert_eq!(metrics.thumb_len, 9);
        assert_eq!(metrics.max_thumb_start, 1);

        // At end
        let metrics_end = ScrollbarMetrics::new_with_track(100, 99, 1, 10);
        assert_eq!(metrics_end.thumb_len, 9);
        assert_eq!(metrics_end.thumb_start, 1);
    }

    #[test]
    fn scrollbar_metrics_cache_overwrites_existing_key() {
        let mut cache = ScrollbarMetricsCache::default();
        let first = ScrollbarMetrics::new_with_track(100, 10, 3, 10);
        let second = ScrollbarMetrics::new_with_track(100, 10, 4, 10);

        cache.insert(100, 10, 3, 10, false, first);
        cache.insert(100, 10, 3, 10, false, second);

        let got = cache.get(100, 10, 3, 10, false).expect("cache miss");
        assert_eq!(got.thumb_start, second.thumb_start);
    }
}
