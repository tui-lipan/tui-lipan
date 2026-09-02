mod layout;
mod node;
mod reconcile;

pub(crate) use layout::measure_splitter;
pub(crate) use node::SplitterNode;
pub(crate) use reconcile::{SplitterReconcile, reconcile_splitter};

use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind};
use crate::style::{Length, Style};
use crate::widgets::Orientation;

/// Where a [`Splitter`] places its drag handles relative to pane borders.
///
/// This is independent of whether neighboring [`Frame`](crate::widgets::Frame)s
/// merge their borders (`Frame::join_frame`). Border merging is a purely visual
/// choice owned by the frames; the handle mode only decides where the splitter's
/// drag target lives and how thick it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SplitterHandleMode {
    /// Reserve a gutter between panes and draw the handle glyph there.
    #[default]
    Gutter,
    /// Drop the gutter and ride the pane border seam: the border cells between
    /// panes become the drag target.
    ///
    /// Thickness follows the borders actually present:
    /// - neighbors that merge their borders share one wall → a 1-cell handle,
    /// - neighbors that keep separate borders expose two adjacent walls → a
    ///   2-cell handle so both are grabbed together,
    /// - borderless neighbors fall back to a synthetic 1-cell handle on the seam.
    Border,
}

/// Size bounds for one [`Splitter`] pane, in cells along the split axis.
///
/// [`Splitter::min_size`] is a single floor shared by every pane. A pane with a
/// ceiling of its own - a sidebar that stops widening at 80 columns, a preview
/// that must never swallow the window - needs bounds the splitter can enforce,
/// or the handle travels past the size the app will actually draw and leaves the
/// pane sitting in an oversized allocation with a hole beside it.
///
/// Bounds hold for a drag and for a programmatic weight change alike.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SplitterPaneLimits {
    /// Smallest size for this pane. Raises [`Splitter::min_size`] when larger.
    pub min: Option<u16>,
    /// Largest size for this pane. A `max` below the effective minimum yields to it.
    pub max: Option<u16>,
}

impl SplitterPaneLimits {
    /// No bounds beyond the splitter's own [`min_size`](Splitter::min_size).
    pub const UNBOUNDED: Self = Self {
        min: None,
        max: None,
    };

    /// Keep the pane at or above `min` cells.
    pub fn min_size(min: u16) -> Self {
        Self {
            min: Some(min),
            max: None,
        }
    }

    /// Keep the pane at or below `max` cells.
    pub fn max_size(max: u16) -> Self {
        Self {
            min: None,
            max: Some(max),
        }
    }

    /// Keep the pane within `min..=max` cells.
    pub fn range(min: u16, max: u16) -> Self {
        Self {
            min: Some(min),
            max: Some(max.max(min)),
        }
    }
}

/// Emitted by splitter resize callbacks with normalized pane weights.
#[derive(Clone, Debug)]
pub struct SplitterResizeEvent {
    /// Matches [`Splitter::split_id`] when set.
    pub split_id: Option<Arc<str>>,
    /// Normalized pane weights (sum ≈ 1).
    pub weights: Vec<f32>,
}

/// A resizable splitter container with draggable handles.
#[derive(Clone)]
pub struct Splitter {
    pub(crate) orientation: Orientation,
    pub(crate) children: Vec<Element>,
    pub(crate) weights: Vec<f32>,
    pub(crate) weights_nonce: u32,
    pub(crate) split_id: Option<Arc<str>>,
    pub(crate) on_resize_live: Option<Callback<SplitterResizeEvent>>,
    pub(crate) on_resize: Option<Callback<SplitterResizeEvent>>,
    pub(crate) min_size: u16,
    pub(crate) pane_limits: Vec<SplitterPaneLimits>,
    pub(crate) handle_size: u16,
    pub(crate) handle_mode: SplitterHandleMode,
    pub(crate) handle_symbol: char,
    pub(crate) handle_style: Style,
    pub(crate) handle_hover_style: Style,
    pub(crate) handle_active_style: Style,
    pub(crate) width: Length,
    pub(crate) height: Length,
}

impl Splitter {
    /// Create a splitter with a specific handle orientation.
    pub fn new(orientation: Orientation) -> Self {
        match orientation {
            Orientation::Horizontal => Self::horizontal(),
            Orientation::Vertical => Self::vertical(),
        }
    }

    /// Create a horizontal splitter (handles are horizontal; panes stacked vertically).
    pub fn horizontal() -> Self {
        Self {
            orientation: Orientation::Horizontal,
            children: Vec::new(),
            weights: Vec::new(),
            weights_nonce: 0,
            split_id: None,
            on_resize_live: None,
            on_resize: None,
            min_size: 3,
            pane_limits: Vec::new(),
            handle_size: 1,
            handle_mode: SplitterHandleMode::Gutter,
            handle_symbol: '─',
            handle_style: Style::default(),
            handle_hover_style: Style::default(),
            handle_active_style: Style::default(),
            width: Length::Flex(1),
            height: Length::Flex(1),
        }
    }

    /// Create a vertical splitter (handles are vertical; panes laid out horizontally).
    pub fn vertical() -> Self {
        Self {
            orientation: Orientation::Vertical,
            children: Vec::new(),
            weights: Vec::new(),
            weights_nonce: 0,
            split_id: None,
            on_resize_live: None,
            on_resize: None,
            min_size: 3,
            pane_limits: Vec::new(),
            handle_size: 1,
            handle_mode: SplitterHandleMode::Gutter,
            handle_symbol: '│',
            handle_style: Style::default(),
            handle_hover_style: Style::default(),
            handle_active_style: Style::default(),
            width: Length::Flex(1),
            height: Length::Flex(1),
        }
    }

    /// Add a child pane.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.children.push(child.into());
        self
    }

    /// Set handle orientation.
    pub fn orientation(mut self, orientation: Orientation) -> Self {
        if self.orientation != orientation {
            self.orientation = orientation;
            self.handle_symbol = match orientation {
                Orientation::Horizontal => '─',
                Orientation::Vertical => '│',
            };
        }
        self
    }

    /// Replace all children, discarding anything already added with
    /// [`child`](Self::child). Call `child` repeatedly to append instead.
    pub fn children<I>(mut self, children: I) -> Self
    where
        I: IntoIterator<Item = Element>,
    {
        self.children = children.into_iter().collect();
        self
    }

    /// Set pane weights (length must match number of panes).
    pub fn weights(mut self, weights: impl Into<Vec<f32>>) -> Self {
        self.weights = weights.into();
        self
    }

    /// Bump when pane weights should override the last reconciled split.
    pub fn weights_nonce(mut self, nonce: u32) -> Self {
        self.weights_nonce = nonce;
        self
    }

    /// Optional id included in [`SplitterResizeEvent`] after a drag.
    pub fn split_id(mut self, id: impl Into<Arc<str>>) -> Self {
        self.split_id = Some(id.into());
        self
    }

    /// Called while a drag resize changes pane weights.
    pub fn on_resize_live(mut self, cb: Callback<SplitterResizeEvent>) -> Self {
        self.on_resize_live = Some(cb);
        self
    }

    /// Called when a drag resize finishes with the final normalized pane weights.
    pub fn on_resize(mut self, cb: Callback<SplitterResizeEvent>) -> Self {
        self.on_resize = Some(cb);
        self
    }

    /// Set minimum size per pane (in cells).
    pub fn min_size(mut self, min_size: u16) -> Self {
        self.min_size = min_size;
        self
    }

    /// Set per-pane size bounds, indexed like the children.
    ///
    /// Entries past the last pane are ignored, and panes past the last entry keep
    /// [`min_size`](Self::min_size) as their only bound. A handle stops where the
    /// bounds of the two panes it separates run out, and a pane clamped by them
    /// hands its cells to the panes that can still take them - the split always
    /// covers the splitter.
    ///
    /// ```
    /// # use tui_lipan::prelude::*;
    /// // The sidebar never goes below 16 columns or past 80, however far the
    /// // handle is dragged; the content pane takes whatever is left.
    /// Splitter::vertical()
    ///     .pane_limits(vec![
    ///         SplitterPaneLimits::range(16, 80),
    ///         SplitterPaneLimits::UNBOUNDED,
    ///     ])
    ///     .child(Spacer::new())
    ///     .child(Spacer::new());
    /// ```
    pub fn pane_limits(mut self, limits: impl Into<Vec<SplitterPaneLimits>>) -> Self {
        self.pane_limits = limits.into();
        self
    }

    /// Set handle thickness (in cells).
    pub fn handle_size(mut self, size: u16) -> Self {
        self.handle_size = size.max(1);
        self
    }

    /// Set how handles are placed relative to pane borders.
    ///
    /// [`SplitterHandleMode::Gutter`] (default) reserves a gutter and draws the
    /// handle glyph there. [`SplitterHandleMode::Border`] drops the gutter and
    /// rides the pane border seam, hit-testing the border cells between panes as
    /// a single handle. This is orthogonal to whether the neighboring frames
    /// merge their borders (`Frame::join_frame`): separate borders are grabbed
    /// together as a 2-cell handle, a merged border as a 1-cell handle.
    pub fn handle_mode(mut self, mode: SplitterHandleMode) -> Self {
        self.handle_mode = mode;
        self
    }

    /// Set handle symbol.
    pub fn handle_symbol(mut self, symbol: char) -> Self {
        self.handle_symbol = symbol;
        self
    }

    /// Set handle style.
    pub fn handle_style(mut self, style: Style) -> Self {
        self.handle_style = style;
        self
    }

    /// Set handle hover style.
    pub fn handle_hover_style(mut self, style: Style) -> Self {
        self.handle_hover_style = style;
        self
    }

    /// Set handle active style (while dragging).
    pub fn handle_active_style(mut self, style: Style) -> Self {
        self.handle_active_style = style;
        self
    }

    /// Override requested width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Override requested height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }
}

impl From<Splitter> for Element {
    fn from(value: Splitter) -> Self {
        Element::new(ElementKind::Splitter(value))
    }
}

impl crate::layout::hash::LayoutHash for Splitter {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.orientation.hash(hasher);
        self.min_size.hash(hasher);
        self.pane_limits.hash(hasher);
        self.handle_size.hash(hasher);
        self.handle_mode.hash(hasher);
        self.handle_symbol.hash(hasher);
        self.weights.len().hash(hasher);
        for weight in &self.weights {
            weight.to_bits().hash(hasher);
        }
        self.weights_nonce.hash(hasher);

        let needs_content =
            matches!(self.width, Length::Auto) || matches!(self.height, Length::Auto);
        if needs_content {
            crate::layout::hash::hash_children(&self.children, hasher, recurse)?;
        }
        Some(())
    }
}

impl Default for Splitter {
    fn default() -> Self {
        Self::horizontal()
    }
}

pub(crate) fn resolve_weights(explicit: &[f32], previous: &[f32], len: usize) -> Vec<f32> {
    let mut weights = if previous.len() == len && !previous.is_empty() {
        previous.to_vec()
    } else if explicit.len() == len && !explicit.is_empty() {
        explicit.to_vec()
    } else {
        vec![1.0; len]
    };

    for weight in &mut weights {
        if *weight < 0.0 {
            *weight = 0.0;
        }
    }

    let sum: f32 = weights.iter().sum();
    if sum <= f32::EPSILON {
        return vec![1.0; len];
    }

    for weight in &mut weights {
        *weight /= sum;
    }

    weights
}

pub(crate) fn sizes_from_weights(weights: &[f32], available: u16, min_size: u16) -> Vec<u16> {
    let count = weights.len();
    if count == 0 {
        return Vec::new();
    }
    if available == 0 {
        return vec![0; count];
    }

    let total_weight: f32 = weights.iter().sum();
    let total_weight = if total_weight <= f32::EPSILON {
        count as f32
    } else {
        total_weight
    };

    let mut sizes = Vec::with_capacity(count);
    let mut fractions = Vec::with_capacity(count);
    for weight in weights {
        let exact = (available as f32) * (*weight / total_weight);
        let floored = exact.floor();
        sizes.push(floored as u16);
        fractions.push(exact - floored);
    }

    // Largest-remainder apportionment: each leftover column goes to the pane
    // with the biggest dropped fraction, ties resolving to the lower index.
    //
    // Handing them out in plain index order instead would park a column on the
    // leftmost pane that a later pane actually earned. Because a drag round
    // trips through sizes -> weights -> sizes every frame, that misplacement
    // reappears each tick and a pane the drag never touched visibly bounces by
    // a column. Largest remainder makes the round trip exact, so panes only
    // move when the drag moves them.
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|a, b| {
        fractions[*b]
            .partial_cmp(&fractions[*a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(b))
    });

    let used: u16 = sizes.iter().sum();
    let mut remaining = available.saturating_sub(used) as usize;
    let mut idx = 0usize;
    while remaining > 0 {
        let target = order[idx % count];
        sizes[target] = sizes[target].saturating_add(1);
        remaining -= 1;
        idx += 1;
    }

    if min_size == 0 {
        return sizes;
    }

    let required = min_size.saturating_mul(count as u16);
    if available < required {
        return sizes;
    }

    loop {
        let mut updated = false;
        for idx in 0..count {
            if sizes[idx] < min_size {
                let deficit = min_size - sizes[idx];
                sizes[idx] = min_size;
                let mut remaining = deficit;
                for size in sizes.iter_mut().take(count) {
                    if remaining == 0 {
                        break;
                    }
                    if *size > min_size {
                        let take = (*size - min_size).min(remaining);
                        *size = size.saturating_sub(take);
                        remaining = remaining.saturating_sub(take);
                    }
                }
                updated = true;
                break;
            }
        }
        if !updated {
            break;
        }
    }

    sizes
}

/// The size window for pane `index`: the shared `min_size` raised by the pane's
/// own floor, and its ceiling.
///
/// A ceiling below the floor is not a size any pane can take, so the floor wins
/// and the pane is pinned there rather than collapsing to an impossible width.
pub(crate) fn pane_bounds(
    limits: &[SplitterPaneLimits],
    index: usize,
    min_size: u16,
) -> (u16, u16) {
    let limit = limits.get(index).copied().unwrap_or_default();
    let min = limit.min.unwrap_or(0).max(min_size);
    let max = limit.max.unwrap_or(u16::MAX).max(min);
    (min, max)
}

/// Clamp `sizes` into their per-pane bounds and move the difference to the panes
/// that can still absorb it.
///
/// [`sizes_from_weights`] has already spent every available cell, so a pane held
/// back by its ceiling leaves a surplus that something else must take - left
/// alone it is a hole in the layout, which is exactly the gap this bound exists
/// to prevent. Cells go to (and come from) the pane with the most room first, so
/// an unbounded neighbour absorbs the whole difference and no pane is pushed
/// through a bound to satisfy another.
///
/// Floors that cannot all be met at this size are dropped rather than
/// overflowing the splitter: the ceilings still hold.
pub(crate) fn apply_pane_limits(
    sizes: &mut [u16],
    limits: &[SplitterPaneLimits],
    min_size: u16,
    available: u16,
) {
    if sizes.is_empty() || limits.is_empty() {
        return;
    }

    let bounds: Vec<(u16, u16)> = (0..sizes.len())
        .map(|index| pane_bounds(limits, index, min_size))
        .collect();
    let floors: u32 = bounds.iter().map(|(min, _)| u32::from(*min)).sum();
    let honour_floors = floors <= u32::from(available);

    for (size, (min, max)) in sizes.iter_mut().zip(&bounds) {
        if honour_floors {
            *size = (*size).max(*min);
        }
        *size = (*size).min(*max);
    }

    let total: u32 = sizes.iter().map(|size| u32::from(*size)).sum();
    let target = u32::from(available);
    match total.cmp(&target) {
        std::cmp::Ordering::Less => {
            let mut surplus = target - total;
            for index in by_room(sizes, &bounds, |size, (_, max)| {
                u32::from(max.saturating_sub(size))
            }) {
                if surplus == 0 {
                    break;
                }
                let room = u32::from(bounds[index].1.saturating_sub(sizes[index])).min(surplus);
                sizes[index] += room as u16;
                surplus -= room;
            }
        }
        std::cmp::Ordering::Greater => {
            let mut excess = total - target;
            for index in by_room(sizes, &bounds, |size, (min, _)| {
                u32::from(size.saturating_sub(min))
            }) {
                if excess == 0 {
                    break;
                }
                let room = u32::from(sizes[index].saturating_sub(bounds[index].0)).min(excess);
                sizes[index] -= room as u16;
                excess -= room;
            }
        }
        std::cmp::Ordering::Equal => {}
    }
}

/// Pane indices ordered by how much `room` each has, most first, ties by index.
fn by_room(
    sizes: &[u16],
    bounds: &[(u16, u16)],
    room: impl Fn(u16, (u16, u16)) -> u32,
) -> Vec<usize> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|index| {
        (
            std::cmp::Reverse(room(sizes[*index], bounds[*index])),
            *index,
        )
    });
    order
}

pub(crate) fn sizes_to_weights(sizes: &[u16]) -> Vec<f32> {
    let total: u16 = sizes.iter().sum();
    if total == 0 {
        return vec![1.0; sizes.len()];
    }
    sizes
        .iter()
        .map(|size| (*size as f32) / (total as f32))
        .collect()
}

#[cfg(test)]
mod limit_tests {
    use super::{SplitterPaneLimits, apply_pane_limits, pane_bounds, sizes_from_weights};

    fn limited(
        weights: &[f32],
        available: u16,
        min_size: u16,
        limits: &[SplitterPaneLimits],
    ) -> Vec<u16> {
        let mut sizes = sizes_from_weights(weights, available, min_size);
        apply_pane_limits(&mut sizes, limits, min_size, available);
        sizes
    }

    /// The bug this exists for: a pane held at its ceiling must hand the cells it
    /// cannot use to its neighbour, or the split stops covering the splitter and
    /// the space between the two panes is drawn by nobody.
    #[test]
    fn a_capped_pane_gives_its_extra_cells_to_the_neighbour() {
        let limits = [
            SplitterPaneLimits::range(16, 80),
            SplitterPaneLimits::UNBOUNDED,
        ];
        // Weights that ask for 130 columns of sidebar in a 199-column split.
        let sizes = limited(&[130.0, 69.0], 199, 1, &limits);
        assert_eq!(sizes, vec![80, 119]);
        assert_eq!(sizes.iter().sum::<u16>(), 199);

        // And the same from below: the floor holds and the neighbour pays for it.
        let sizes = limited(&[3.0, 196.0], 199, 1, &limits);
        assert_eq!(sizes, vec![16, 183]);
        assert_eq!(sizes.iter().sum::<u16>(), 199);
    }

    /// Cells go to the pane with the most room, so a second capped pane cannot
    /// swallow what only an unbounded pane can hold.
    #[test]
    fn surplus_follows_the_room_a_pane_actually_has() {
        let limits = [
            SplitterPaneLimits::max_size(10),
            SplitterPaneLimits::max_size(12),
            SplitterPaneLimits::UNBOUNDED,
        ];
        let sizes = limited(&[1.0, 1.0, 1.0], 60, 0, &limits);
        assert_eq!(sizes, vec![10, 12, 38]);
    }

    /// Floors that do not fit are not floors. The splitter still has to fill its
    /// bounds exactly, so the ceilings hold alone rather than overflowing it.
    #[test]
    fn floors_that_cannot_fit_are_dropped_not_overflowed() {
        let limits = [
            SplitterPaneLimits::min_size(40),
            SplitterPaneLimits::min_size(40),
        ];
        let sizes = limited(&[1.0, 1.0], 20, 0, &limits);
        assert_eq!(sizes.iter().sum::<u16>(), 20);
    }

    /// Panes past the last entry, and a splitter given no entries at all, keep
    /// the behavior they had before pane limits existed.
    #[test]
    fn unlisted_panes_keep_the_shared_minimum() {
        assert_eq!(pane_bounds(&[], 0, 3), (3, u16::MAX));
        assert_eq!(
            pane_bounds(&[SplitterPaneLimits::max_size(9)], 0, 3),
            (3, 9)
        );
        // A ceiling under the shared floor is not a width any pane can take.
        assert_eq!(
            pane_bounds(&[SplitterPaneLimits::max_size(1)], 0, 3),
            (3, 3)
        );

        let plain = sizes_from_weights(&[1.0, 1.0], 21, 3);
        assert_eq!(limited(&[1.0, 1.0], 21, 3, &[]), plain);
    }
}

#[cfg(test)]
mod size_tests {
    use super::{sizes_from_weights, sizes_to_weights};

    /// A drag stores exact column counts, publishes them as weights, and the
    /// next layout turns them back into columns. That round trip has to be the
    /// identity, or panes drift by a column every frame while dragging.
    #[test]
    fn sizes_survive_a_round_trip_through_weights() {
        let cases: &[&[u16]] = &[
            &[39, 39, 79],
            &[40, 38, 79],
            &[1, 1, 155],
            &[52, 52, 53],
            &[10, 20, 30, 40],
            &[7, 11, 13, 17, 19],
            &[100, 1, 1],
            &[3, 3],
        ];

        for sizes in cases {
            let available: u16 = sizes.iter().sum();
            let weights = sizes_to_weights(sizes);
            let restored = sizes_from_weights(&weights, available, 0);
            assert_eq!(
                restored, *sizes,
                "round trip changed {sizes:?} (available {available})"
            );
        }
    }

    /// The leftover column belongs to the pane that earned it, not to pane 0.
    #[test]
    fn leftover_columns_follow_the_largest_fraction() {
        // Exact shares are 3.33, 3.33, 3.33 -> one leftover column, and with
        // equal fractions the lowest index wins.
        assert_eq!(sizes_from_weights(&[1.0, 1.0, 1.0], 10, 0), vec![4, 3, 3]);

        // Exact shares are 1.0, 4.0, 5.0 - nothing is dropped, so nothing moves.
        assert_eq!(sizes_from_weights(&[0.1, 0.4, 0.5], 10, 0), vec![1, 4, 5]);

        // Exact shares are 0.9, 4.5, 4.6: floors 0, 4, 4 leave two columns for
        // the two largest fractions (.9 and .6), not for the first two panes.
        assert_eq!(
            sizes_from_weights(&[0.09, 0.45, 0.46], 10, 0),
            vec![1, 4, 5]
        );
    }

    #[test]
    fn sizes_always_fill_the_available_space() {
        for available in [1u16, 7, 13, 80, 157, 999] {
            for weights in [
                vec![1.0, 1.0, 2.0],
                vec![0.3333, 0.3333, 0.3334],
                vec![0.01, 0.98, 0.01],
            ] {
                let sizes = sizes_from_weights(&weights, available, 0);
                assert_eq!(
                    sizes.iter().sum::<u16>(),
                    available,
                    "weights {weights:?} at {available} left a gap"
                );
            }
        }
    }
}
