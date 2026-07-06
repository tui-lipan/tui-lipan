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

/// Emitted when a splitter drag finishes and pane weights changed.
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
    pub(crate) on_resize: Option<Callback<SplitterResizeEvent>>,
    pub(crate) min_size: u16,
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
            on_resize: None,
            min_size: 3,
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
            on_resize: None,
            min_size: 3,
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

    /// Replace all child panes.
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

    /// Merge handles onto adjacent frame borders.
    #[deprecated(
        since = "0.1.1",
        note = "use `handle_mode(SplitterHandleMode::Border)` instead"
    )]
    pub fn join_frame(self, join: bool) -> Self {
        self.handle_mode(if join {
            SplitterHandleMode::Border
        } else {
            SplitterHandleMode::Gutter
        })
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
    for weight in weights {
        let size = ((available as f32) * (*weight / total_weight)).floor() as u16;
        sizes.push(size);
    }

    let used: u16 = sizes.iter().sum();
    let mut remaining = available.saturating_sub(used);
    let mut idx = 0usize;
    while remaining > 0 {
        sizes[idx % count] = sizes[idx % count].saturating_add(1);
        remaining = remaining.saturating_sub(1);
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
