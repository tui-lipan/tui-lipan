//! ASCII canvas widget.
//!
//! `AsciiCanvas` is a versatile widget for displaying ASCII art, cell grids,
//! and multi-frame sprite sheets. It supports:
//!
//! - **Static text lines** - simple rows of text
//! - **Cell grids** - per-cell styled character buffers
//! - **Frame sequences** - multi-frame displays with tag-based lookup
//!   (sprite sheets, interactive animations, directional sprites)

pub mod animation;

mod layout;
mod node;
mod reconcile;

pub(crate) use layout::measure_ascii_canvas;
pub use node::AsciiCanvasNode;
pub(crate) use reconcile::reconcile_ascii_canvas;

use std::sync::Arc;

use crate::core::element::{Element, ElementKind};
use crate::style::{Length, Style};
use crate::utils::gradient::{ColorGradient, GradientDirection};

use self::animation::FrameSequence;

/// A single cell in an ASCII canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AsciiCell {
    /// Cell character.
    pub ch: char,
    /// Cell style.
    pub style: Style,
}

impl Default for AsciiCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            style: Style::default(),
        }
    }
}

impl AsciiCell {
    /// Create a new cell with the given character.
    pub fn new(ch: char) -> Self {
        Self {
            ch,
            style: Style::default(),
        }
    }

    /// Set the cell style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl From<char> for AsciiCell {
    fn from(ch: char) -> Self {
        Self::new(ch)
    }
}

/// A mutable buffer for building an [`AsciiCanvas`].
#[derive(Clone, Debug)]
pub struct AsciiCanvasBuffer {
    width: u16,
    height: u16,
    cells: Vec<AsciiCell>,
}

impl Default for AsciiCanvasBuffer {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl AsciiCanvasBuffer {
    /// Create a new buffer filled with spaces.
    pub fn new(width: u16, height: u16) -> Self {
        let len = width as usize * height as usize;
        Self {
            width,
            height,
            cells: vec![AsciiCell::default(); len],
        }
    }

    /// Buffer width in cells.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Buffer height in cells.
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Fill the buffer with a single cell value.
    pub fn fill(&mut self, cell: AsciiCell) {
        self.cells.fill(cell);
    }

    /// Fill the buffer with a single character.
    pub fn fill_char(&mut self, ch: char) {
        self.fill(AsciiCell::new(ch));
    }

    /// Set a cell at a position.
    pub fn set(&mut self, x: u16, y: u16, cell: AsciiCell) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = (y as usize).saturating_mul(self.width as usize) + x as usize;
        if let Some(slot) = self.cells.get_mut(idx) {
            *slot = cell;
        }
    }

    /// Set a character at a position.
    pub fn set_char(&mut self, x: u16, y: u16, ch: char) {
        self.set(x, y, AsciiCell::new(ch));
    }

    /// Borrow the cell slice.
    pub fn cells(&self) -> &[AsciiCell] {
        &self.cells
    }

    /// Collect all unique colors (both foreground and background) used in this
    /// buffer, in order of first appearance.
    ///
    /// **Note:** if the same color value is used in both fg and bg, it appears
    /// only once in the returned list.  When you need to map the same color
    /// differently per channel, use [`collect_fg_colors`](Self::collect_fg_colors)
    /// and [`collect_bg_colors`](Self::collect_bg_colors) instead.
    pub fn collect_colors(&self) -> Vec<crate::style::Color> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for cell in &self.cells {
            for color in [cell.style.fg, cell.style.bg]
                .into_iter()
                .flatten()
                .map(crate::style::Paint::color)
            {
                if seen.insert(color) {
                    out.push(color);
                }
            }
        }
        out
    }

    /// Collect all unique **foreground** colors used in this buffer, in order
    /// of first appearance.
    pub fn collect_fg_colors(&self) -> Vec<crate::style::Color> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for cell in &self.cells {
            if let Some(color) = cell.style.fg.map(crate::style::Paint::color)
                && seen.insert(color)
            {
                out.push(color);
            }
        }
        out
    }

    /// Collect all unique **background** colors used in this buffer, in order
    /// of first appearance.
    pub fn collect_bg_colors(&self) -> Vec<crate::style::Color> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for cell in &self.cells {
            if let Some(color) = cell.style.bg.map(crate::style::Paint::color)
                && seen.insert(color)
            {
                out.push(color);
            }
        }
        out
    }
}

/// A 2D ASCII canvas element.
///
/// Supports three display modes:
///
/// 1. **Text lines** - created with [`AsciiCanvas::new`]
/// 2. **Cell grid** - created with [`AsciiCanvas::from_cells`] or [`From<AsciiCanvasBuffer>`]
/// 3. **Frame sequence** - created with [`AsciiCanvas::from_sequence`], displays one frame
///    at a time from a [`FrameSequence`] (sprite sheet / animation)
///
/// # Examples
///
/// ```rust,ignore
/// // Static text
/// AsciiCanvas::new(["  ██  ", " ████ ", "██████"])
///
/// // Cell grid
/// AsciiCanvas::from(buffer)
///
/// // Multi-frame sprite sheet
/// AsciiCanvas::from_sequence(sequence)
///     .frame_by_tag("direction", "left")
///     .style(Style::new().fg(Color::White))
/// ```
#[derive(Clone)]
pub struct AsciiCanvas {
    /// Rows of text for the canvas.
    pub lines: Vec<Arc<str>>,
    /// Optional styled cells (row-major, len == width * height).
    pub cells: Option<Arc<[AsciiCell]>>,
    /// Explicit grid size for cell layouts.
    pub grid_size: Option<(u16, u16)>,
    /// Optional frame sequence for multi-frame display.
    pub sequence: Option<Arc<FrameSequence>>,
    /// Current frame index (only used when `sequence` is set).
    pub current_frame: usize,
    /// Base style applied to all cells.
    pub style: Style,
    /// Background fill style (only bg is respected).
    pub background: Option<Style>,
    /// Requested width.
    /// Default: `Length::Auto`.
    pub width: Length,
    /// Requested height.
    /// Default: `Length::Auto`.
    pub height: Length,
    /// Optional color gradient applied at render time.
    pub gradient: Option<(ColorGradient, GradientDirection)>,
    /// Optional color remapping applied at render time to **both** fg and bg
    /// channels.
    ///
    /// Each entry maps a source color to a replacement color.  The mapping is
    /// applied to both foreground and background channels - if a cell's fg or
    /// bg matches a source entry, it is replaced with the corresponding target
    /// color.  Colors not present in the map are rendered unchanged.
    ///
    /// When the same source color appears in both fg and bg but needs
    /// different replacements, use [`fg_color_map`](Self::fg_color_map) and/or
    /// [`bg_color_map`](Self::bg_color_map) instead (they take precedence over
    /// this field for their respective channel).
    ///
    /// See [`FrameSequence::collect_colors`] to discover which colors an
    /// asset uses.
    pub color_map: Option<Arc<[(crate::style::Color, crate::style::Color)]>>,
    /// Optional color remapping applied **only** to the foreground channel.
    ///
    /// Takes precedence over [`color_map`](Self::color_map) for fg lookups.
    /// See [`FrameSequence::collect_fg_colors`] to discover fg-specific colors.
    pub fg_color_map: Option<Arc<[(crate::style::Color, crate::style::Color)]>>,
    /// Optional color remapping applied **only** to the background channel.
    ///
    /// Takes precedence over [`color_map`](Self::color_map) for bg lookups.
    /// See [`FrameSequence::collect_bg_colors`] to discover bg-specific colors.
    pub bg_color_map: Option<Arc<[(crate::style::Color, crate::style::Color)]>>,
}

impl Default for AsciiCanvas {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            cells: None,
            grid_size: None,
            sequence: None,
            current_frame: 0,
            style: Style::default(),
            background: None,
            width: Length::Auto,
            height: Length::Auto,
            gradient: None,
            color_map: None,
            fg_color_map: None,
            bg_color_map: None,
        }
    }
}

impl AsciiCanvas {
    /// Create a new canvas from text lines.
    pub fn new(lines: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        Self {
            lines: lines.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// Create a blank canvas with an empty cell grid.
    pub fn blank(width: u16, height: u16) -> Self {
        let len = width as usize * height as usize;
        Self::from_cells(width, height, vec![AsciiCell::default(); len])
    }

    /// Create a canvas by generating each cell from a callback.
    pub fn with_cell_fn(width: u16, height: u16, mut f: impl FnMut(u16, u16) -> AsciiCell) -> Self {
        let mut cells = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height {
            for x in 0..width {
                cells.push(f(x, y));
            }
        }
        Self::from_cells(width, height, cells)
    }

    /// Create a canvas from a flat row-major grid of cells.
    pub fn from_cells(width: u16, height: u16, cells: impl Into<Arc<[AsciiCell]>>) -> Self {
        Self {
            lines: Vec::new(),
            cells: Some(cells.into()),
            grid_size: Some((width, height)),
            ..Self::default()
        }
    }

    /// Create a canvas from a frame sequence (sprite sheet / animation).
    ///
    /// Displays the first frame by default. Use [`frame`](Self::frame) or
    /// [`frame_by_tag`](Self::frame_by_tag) to select a specific frame.
    pub fn from_sequence(sequence: Arc<FrameSequence>) -> Self {
        Self {
            sequence: Some(sequence),
            ..Self::default()
        }
    }

    /// Provide a flat row-major grid of cells.
    pub fn cells(mut self, cells: impl Into<Arc<[AsciiCell]>>) -> Self {
        self.cells = Some(cells.into());
        self
    }

    /// Set the explicit grid size for cell rendering.
    pub fn grid_size(mut self, width: u16, height: u16) -> Self {
        self.grid_size = Some((width, height));
        self
    }

    /// Set the current frame index (for sequence mode).
    pub fn frame(mut self, idx: usize) -> Self {
        if let Some(ref seq) = self.sequence {
            self.current_frame = idx.min(seq.len().saturating_sub(1));
        }
        self
    }

    /// Set the current frame by tag lookup (for sequence mode).
    ///
    /// If no frame with the given tag is found, the current frame is unchanged.
    pub fn frame_by_tag(mut self, key: &str, value: &str) -> Self {
        if let Some(ref seq) = self.sequence
            && let Some(idx) = seq.find_by_tag(key, value)
        {
            self.current_frame = idx;
        }
        self
    }

    /// Set the base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set background style (only bg is used).
    pub fn background(mut self, style: Style) -> Self {
        self.background = Some(style);
        self
    }

    /// Set requested width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set requested height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Apply a color remapping over the rendered output to **both** fg and bg
    /// channels.
    ///
    /// Each entry maps a source color to a replacement color.  The mapping is
    /// applied to both foreground and background channels - if a cell's fg or
    /// bg matches a source entry, it is replaced with the corresponding target
    /// color.  Colors not present in the map are rendered unchanged.
    ///
    /// When the same source color appears in both fg and bg but needs
    /// different replacements, use [`fg_color_map`](Self::fg_color_map) and/or
    /// [`bg_color_map`](Self::bg_color_map) instead.
    ///
    /// Use [`FrameSequence::collect_colors`] or [`AsciiCanvasBuffer::collect_colors`]
    /// to discover the colors an asset contains, then build a mapping from them
    /// to your theme colors:
    ///
    /// ```rust,ignore
    /// let colors = sequence.collect_colors(); // [Color::Red, Color::Blue]
    /// let canvas = AsciiCanvas::from_sequence(arc_seq)
    ///     .color_map(vec![
    ///         (colors[0], theme.selection.fg.unwrap_or(Color::White)),
    ///         (colors[1], theme.primary.fg.unwrap_or(Color::Gray)),
    ///     ]);
    /// ```
    pub fn color_map(
        mut self,
        map: impl Into<Arc<[(crate::style::Color, crate::style::Color)]>>,
    ) -> Self {
        self.color_map = Some(map.into());
        self
    }

    /// Apply a color remapping **only** to the foreground channel.
    ///
    /// Takes precedence over [`color_map`](Self::color_map) for fg lookups.
    /// Use [`FrameSequence::collect_fg_colors`] to discover fg-specific colors.
    ///
    /// ```rust,ignore
    /// let fg_colors = sequence.collect_fg_colors();
    /// let bg_colors = sequence.collect_bg_colors();
    /// let canvas = AsciiCanvas::from_sequence(arc_seq)
    ///     .fg_color_map(vec![
    ///         (fg_colors[0], Color::White),
    ///         (fg_colors[1], Color::Gray),
    ///     ])
    ///     .bg_color_map(vec![
    ///         (bg_colors[0], Color::Black),
    ///     ]);
    /// ```
    pub fn fg_color_map(
        mut self,
        map: impl Into<Arc<[(crate::style::Color, crate::style::Color)]>>,
    ) -> Self {
        self.fg_color_map = Some(map.into());
        self
    }

    /// Apply a color remapping **only** to the background channel.
    ///
    /// Takes precedence over [`color_map`](Self::color_map) for bg lookups.
    /// See [`FrameSequence::collect_bg_colors`] to discover bg-specific colors.
    pub fn bg_color_map(
        mut self,
        map: impl Into<Arc<[(crate::style::Color, crate::style::Color)]>>,
    ) -> Self {
        self.bg_color_map = Some(map.into());
        self
    }

    /// Apply a color gradient over the rendered output.
    ///
    /// For cell-grid and sequence mode the gradient overrides each cell's
    /// foreground base before per-cell style is patched on top, so cells with
    /// an explicit `fg` color will still take priority.
    ///
    /// For text-lines mode the gradient is the sole foreground color.
    pub fn gradient(mut self, gradient: ColorGradient, direction: GradientDirection) -> Self {
        self.gradient = Some((gradient, direction));
        self
    }

    /// Get the resolved cells and grid size for the current display mode.
    ///
    /// Returns `(cells, grid_width, grid_height)` if in cell/sequence mode,
    /// or `None` if in text-lines mode.
    pub fn resolved_cells(&self) -> Option<(&[AsciiCell], u16, u16)> {
        if let Some(ref seq) = self.sequence {
            let frame = seq.get(self.current_frame)?;
            let buf = &frame.buffer;
            Some((buf.cells(), buf.width(), buf.height()))
        } else {
            let cells = self.cells.as_ref()?;
            let (w, h) = self.grid_size.unwrap_or((0, 0));
            Some((cells, w, h))
        }
    }

    /// Get the effective width of the content.
    pub fn content_width(&self) -> u16 {
        if let Some(ref seq) = self.sequence {
            return seq.width();
        }
        if let Some((w, _)) = self.grid_size {
            return w;
        }
        self.lines
            .iter()
            .map(|l| l.chars().count() as u16)
            .max()
            .unwrap_or(0)
    }

    /// Get the effective height of the content.
    pub fn content_height(&self) -> u16 {
        if let Some(ref seq) = self.sequence {
            return seq.height();
        }
        if let Some((_, h)) = self.grid_size {
            return h;
        }
        self.lines.len() as u16
    }
}

impl From<AsciiCanvasBuffer> for AsciiCanvas {
    fn from(buffer: AsciiCanvasBuffer) -> Self {
        AsciiCanvas::from_cells(buffer.width, buffer.height, buffer.cells)
    }
}

impl From<AsciiCanvas> for Element {
    fn from(value: AsciiCanvas) -> Self {
        Element::new(ElementKind::AsciiCanvas(value))
    }
}

impl crate::layout::hash::LayoutHash for AsciiCanvas {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&crate::core::element::Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.grid_size.hash(hasher);

        // Hash sequence identity + current frame for multi-frame mode
        if let Some(ref seq) = self.sequence {
            std::sync::Arc::as_ptr(seq).hash(hasher);
            self.current_frame.hash(hasher);
        }

        let needs_content =
            matches!(self.width, Length::Auto) || matches!(self.height, Length::Auto);
        if needs_content {
            self.lines.hash(hasher);
            self.cells.hash(hasher);
        }
        Some(())
    }
}
