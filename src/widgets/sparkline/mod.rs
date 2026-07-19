//! Sparkline widget.

use std::sync::{Arc, Mutex, OnceLock};

use crate::style::{Length, Style};
use crate::utils::gradient::{ColorGradient, GradientRange};
use crate::widgets::Overflow;

mod layout;
mod node;
mod reconcile;

pub use layout::measure_sparkline;
pub use node::SparklineNode;
pub use reconcile::reconcile_sparkline;

pub(crate) use node::SparklineCacheKey;

static SPARKLINE_CACHE: OnceLock<Mutex<SparklineVisualCache>> = OnceLock::new();

#[derive(Clone, Debug)]
pub(crate) struct SparklineVisualCache {
    entries: Vec<(SparklineCacheKey, Arc<node::SparklineRenderOutput>)>,
}

impl SparklineVisualCache {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn get(&self, key: &SparklineCacheKey) -> Option<Arc<node::SparklineRenderOutput>> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| Arc::clone(v))
    }

    fn insert(&mut self, key: SparklineCacheKey, value: Arc<node::SparklineRenderOutput>) {
        if let Some(idx) = self.entries.iter().position(|(k, _)| k == &key) {
            self.entries.remove(idx);
        }
        self.entries.push((key, value));
        if self.entries.len() > 100 {
            self.entries.remove(0);
        }
    }
}

pub(crate) fn get_cached_output(
    key: &SparklineCacheKey,
) -> Option<Arc<node::SparklineRenderOutput>> {
    let cache_mutex = SPARKLINE_CACHE.get_or_init(|| Mutex::new(SparklineVisualCache::new()));
    if let Ok(cache) = cache_mutex.lock() {
        return cache.get(key);
    }
    None
}

pub(crate) fn insert_cached_output(
    key: SparklineCacheKey,
    output: Arc<node::SparklineRenderOutput>,
) {
    let cache_mutex = SPARKLINE_CACHE.get_or_init(|| Mutex::new(SparklineVisualCache::new()));
    if let Ok(mut cache) = cache_mutex.lock() {
        cache.insert(key, output);
    }
}

pub(crate) const DEFAULT_BARS: [char; 8] = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
pub(crate) const SHADE_BARS: [char; 5] = [' ', '░', '▒', '▓', '█'];
pub(crate) const LINE_NORTH: u8 = 0b0001;
pub(crate) const LINE_EAST: u8 = 0b0010;
pub(crate) const LINE_SOUTH: u8 = 0b0100;
pub(crate) const LINE_WEST: u8 = 0b1000;
pub(crate) const LINE_POINT: u8 = 0b1_0000;

use crate::core::element::Element;

impl From<Sparkline> for Element {
    fn from(val: Sparkline) -> Self {
        Element::new(crate::core::element::ElementKind::Sparkline(val))
    }
}

/// Visual mode for rendering sparkline data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SparklineVariant {
    /// Amplitude bars (default).
    #[default]
    Bars,
    /// Braille spike bars (pair-packed, two samples per glyph).
    Braille,
    /// Trend line glyphs (up/down/flat/turn).
    Line,
}

/// Preset bar glyph ramps for `SparklineVariant::Bars`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SparklineBarsPreset {
    /// Unicode block bars from low to high.
    #[default]
    Blocks,
    /// Shade ramp from low to high.
    Shades,
}

impl SparklineBarsPreset {
    pub(crate) fn glyphs(self) -> &'static [char] {
        match self {
            Self::Blocks => &DEFAULT_BARS,
            Self::Shades => &SHADE_BARS,
        }
    }
}

/// Downsampling aggregation strategy when `max_points` is set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SparklineAggregation {
    /// Bucket average.
    #[default]
    Average,
    /// Bucket minimum.
    Min,
    /// Bucket maximum.
    Max,
    /// First value in each bucket.
    First,
    /// Last value in each bucket.
    Last,
}

/// Rendering policy for zero values in Bars/Braille variants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SparklineZeroPolicy {
    /// Render zero as empty/background.
    #[default]
    Empty,
    /// Render zero using the smallest visible glyph.
    MinGlyph,
}

/// Preset glyph sets for `SparklineVariant::Line`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SparklineLinePreset {
    /// Unicode line glyphs.
    #[default]
    Unicode,
    /// ASCII-safe line glyphs.
    ///
    /// Single-row rendering uses pure 7-bit ASCII (`/\-^v`). Multi-row
    /// rendering uses `|-_/\\+` plus `‾` (U+203E OVERLINE) for top-row
    /// horizontals so plateaus visually hug the top of the cell; `‾` is
    /// widely supported but is the one non-7-bit-ASCII glyph in the preset.
    Ascii,
}

/// Glyph set for `SparklineVariant::Line`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SparklineLineGlyphs {
    /// Rising trend.
    pub rising: char,
    /// Falling trend.
    pub falling: char,
    /// Flat trend.
    pub flat: char,
    /// Local peak (up then down).
    pub peak: char,
    /// Local valley (down then up).
    pub valley: char,
}

impl SparklineLineGlyphs {
    /// Unicode line glyphs.
    pub const UNICODE: Self = Self {
        rising: '╱',
        falling: '╲',
        flat: '─',
        peak: '╮',
        valley: '╰',
    };

    /// ASCII-safe line glyphs.
    pub const ASCII: Self = Self {
        rising: '/',
        falling: '\\',
        flat: '-',
        peak: '^',
        valley: 'v',
    };
}

impl Default for SparklineLineGlyphs {
    fn default() -> Self {
        Self::UNICODE
    }
}

impl SparklineLinePreset {
    pub(crate) fn glyphs(self) -> SparklineLineGlyphs {
        match self {
            Self::Unicode => SparklineLineGlyphs::UNICODE,
            Self::Ascii => SparklineLineGlyphs::ASCII,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointTrend {
    Rising,
    Falling,
    Flat,
    Turn,
}

/// A compact sparkline chart for inline trend visualization.
#[derive(Clone)]
pub struct Sparkline {
    /// Data points to plot.
    pub data: Arc<[u64]>,
    /// Minimum value for the chart range.
    pub min: Option<u64>,
    /// Maximum value for the chart range.
    pub max: Option<u64>,
    /// Custom bar glyphs.
    pub bars: Vec<char>,
    /// Visual variant.
    pub variant: SparklineVariant,
    /// Maximum points to display (enables downsampling).
    pub max_points: Option<usize>,
    /// Aggregation strategy for downsampling.
    pub aggregation: SparklineAggregation,
    /// Policy for rendering zero values.
    pub zero_policy: SparklineZeroPolicy,
    /// Custom line glyphs.
    pub line_glyphs: SparklineLineGlyphs,
    /// Chart drawing height in rows.
    pub chart_height: u16,
    /// Whether to mirror the X axis.
    pub mirror_x: bool,
    /// Whether to mirror the Y axis.
    pub mirror_y: bool,
    /// Base style.
    pub style: Style,
    /// Style for rising segments.
    pub rising_style: Style,
    /// Style for falling segments.
    pub falling_style: Style,
    /// Style for flat segments.
    pub flat_style: Style,
    /// Style for turning segments.
    pub turn_style: Style,
    /// Value-based gradient.
    pub gradient: Option<ColorGradient>,
    /// Row-based height gradient.
    pub height_gradient: Option<ColorGradient>,
    /// Value range for gradient mapping.
    pub gradient_range: Option<GradientRange>,
    /// Requested width.
    /// Default: `Length::Auto`.
    pub width: Length,
    /// Requested height.
    /// Default: `Length::Auto`.
    pub height: Length,
    /// Overflow behavior.
    pub overflow: Overflow,
}

impl Sparkline {
    /// Create a new sparkline.
    pub fn new(data: impl IntoIterator<Item = u64>) -> Self {
        Self {
            data: data.into_iter().collect::<Vec<_>>().into(),
            min: None,
            max: None,
            bars: DEFAULT_BARS.to_vec(),
            variant: SparklineVariant::Bars,
            max_points: None,
            aggregation: SparklineAggregation::Average,
            zero_policy: SparklineZeroPolicy::default(),
            line_glyphs: SparklineLineGlyphs::default(),
            chart_height: 1,
            mirror_x: false,
            mirror_y: false,
            style: Style::default(),
            rising_style: Style::default(),
            falling_style: Style::default(),
            flat_style: Style::default(),
            turn_style: Style::default(),
            gradient: None,
            height_gradient: None,
            gradient_range: None,
            width: Length::Auto,
            height: Length::Auto,
            overflow: Overflow::Auto,
        }
    }

    /// Replace data points.
    pub fn data(mut self, data: impl IntoIterator<Item = u64>) -> Self {
        self.data = data.into_iter().collect::<Vec<_>>().into();
        self
    }

    /// Set data points from a shared slice.
    pub fn data_arc(mut self, data: Arc<[u64]>) -> Self {
        self.data = data;
        self
    }

    /// Set minimum value (defaults to data min).
    pub fn min(mut self, min: u64) -> Self {
        self.min = Some(min);
        self
    }

    /// Set maximum value (defaults to data max).
    pub fn max(mut self, max: u64) -> Self {
        self.max = Some(max);
        self
    }

    /// Set visual variant.
    pub fn variant(mut self, variant: SparklineVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Convenience: render as a trend line.
    pub fn line(mut self) -> Self {
        self.variant = SparklineVariant::Line;
        self
    }

    /// Convenience: render using pair-packed braille spike bars.
    pub fn braille(mut self) -> Self {
        self.variant = SparklineVariant::Braille;
        self
    }

    /// Set custom bar glyphs (lowest to highest).
    pub fn bars(mut self, bars: impl IntoIterator<Item = char>) -> Self {
        self.bars = bars.into_iter().collect();
        self
    }

    /// Set a bar glyph preset.
    pub fn bars_preset(mut self, preset: SparklineBarsPreset) -> Self {
        self.bars = preset.glyphs().to_vec();
        self
    }

    /// Set line glyph preset.
    pub fn line_preset(mut self, preset: SparklineLinePreset) -> Self {
        self.line_glyphs = preset.glyphs();
        self
    }

    /// Set custom line glyphs.
    pub fn line_glyphs(mut self, glyphs: SparklineLineGlyphs) -> Self {
        self.line_glyphs = glyphs;
        self
    }

    /// Set chart drawing height (in text rows) for all variants.
    ///
    /// Values below 1 are clamped to 1.
    pub fn chart_height(mut self, rows: u16) -> Self {
        self.chart_height = rows.max(1);
        self
    }

    /// Mirror chart horizontally (reverse sample/time order).
    pub fn mirror_x(mut self, mirror: bool) -> Self {
        self.mirror_x = mirror;
        self
    }

    /// Mirror chart vertically (flip value direction).
    ///
    /// - **Braille**: fully mirrored - dots flip within each glyph cell.
    /// - **Line**: fully mirrored - rising/falling glyph directions swap and
    ///   multi-row grid is flipped row-wise.
    /// - **Bars**: row order is flipped, but the default Unicode block ramp
    ///   (`▂▃▄▅▆▇█`) only fills bottom-up - Unicode offers no matching
    ///   top-down partial-fill ramp, so the leading partial-fill row of a bar
    ///   still renders from the bottom. For vertically mirrored bars, prefer
    ///   a symmetric glyph set (e.g. `SparklineBarsPreset::Shades` with
    ///   `░▒▓█`) or switch to `SparklineVariant::Braille`.
    pub fn mirror_y(mut self, mirror: bool) -> Self {
        self.mirror_y = mirror;
        self
    }

    /// Limit rendered point count by downsampling to `max_points`.
    pub fn max_points(mut self, max_points: usize) -> Self {
        self.max_points = Some(max_points.max(1));
        self
    }

    /// Set downsampling aggregation strategy.
    pub fn aggregation(mut self, aggregation: SparklineAggregation) -> Self {
        self.aggregation = aggregation;
        self
    }

    /// Control how zero values are rendered in Bars/Braille variants.
    pub fn zero_policy(mut self, policy: SparklineZeroPolicy) -> Self {
        self.zero_policy = policy;
        self
    }

    /// Apply value-based gradient coloring.
    pub fn gradient(mut self, gradient: ColorGradient) -> Self {
        self.gradient = Some(gradient);
        self
    }

    /// Apply row-based gradient coloring from top to bottom.
    pub fn height_gradient(mut self, gradient: ColorGradient) -> Self {
        self.height_gradient = Some(gradient);
        self
    }

    /// Override value range used for gradient mapping.
    pub fn gradient_range(mut self, min: u64, max: u64) -> Self {
        self.gradient_range = Some(GradientRange::new(min, max));
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Style for rising points.
    pub fn rising_style(mut self, style: Style) -> Self {
        self.rising_style = style;
        self
    }

    /// Style for falling points.
    pub fn falling_style(mut self, style: Style) -> Self {
        self.falling_style = style;
        self
    }

    /// Style for flat points.
    pub fn flat_style(mut self, style: Style) -> Self {
        self.flat_style = style;
        self
    }

    /// Style for turning points (peaks/valleys).
    pub fn turn_style(mut self, style: Style) -> Self {
        self.turn_style = style;
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set widget height constraint.
    ///
    /// This is layout height, independent from `chart_height` draw rows.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Set overflow behavior when the data buffer is longer than the allocated width.
    ///
    /// Only active for `Length::Flex`/`Length::Percent` widths and when
    /// `max_points` is not set (explicit `max_points` always bucket-downsamples).
    ///
    /// - `Auto` / `ClipStart`: keep the newest samples (scrolling, default).
    /// - `Clip` / `Ellipsis`: keep the oldest samples.
    /// - `Wrap`: bucket-aggregate the full buffer across the width.
    pub fn overflow(mut self, overflow: Overflow) -> Self {
        self.overflow = overflow;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{Sparkline, SparklineLinePreset, SparklineVariant};
    use crate::core::element::{Element, ElementKind};
    use crate::style::Length;

    fn into_node(el: Element) -> super::node::SparklineNode {
        match el.kind {
            ElementKind::Sparkline(spark) => spark.into(),
            _ => panic!("expected sparkline element"),
        }
    }

    #[test]
    fn default_bars_render_expected_ramp() {
        let node = into_node(
            Sparkline::new([0, 1, 2, 3, 4, 5, 6, 7])
                .min(0)
                .max(7)
                .into(),
        );
        let content: String = node.output.rows[0]
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(content, " ▂▃▄▅▆▇█");
    }

    #[test]
    fn chart_height_renders_multi_row_bars() {
        let node = into_node(
            Sparkline::new([0, 25, 50, 75, 100])
                .min(0)
                .max(100)
                .chart_height(3)
                .into(),
        );
        assert_eq!(node.output.rows.len(), 3);
        // In primitive mode, the requested height remains Auto if not explicitly set.
        assert_eq!(node.height, Length::Auto);
    }

    #[test]
    fn line_variant_renders_turn_glyphs() {
        let node = into_node(Sparkline::new([1, 3, 2, 4, 4, 1]).line().into());
        let content: String = node.output.rows[0]
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(content, "╱╮╰╱╲╲");
    }

    #[test]
    fn braille_variant_packs_two_samples_per_cell() {
        let node = into_node(
            Sparkline::new([0, 1, 2, 3, 4])
                .variant(SparklineVariant::Braille)
                .min(0)
                .max(4)
                .into(),
        );

        let content: String = node.output.rows[0]
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(content.chars().count(), 3);
    }

    #[test]
    fn line_ascii_preset_is_available() {
        let node = into_node(
            Sparkline::new([1, 2, 1])
                .variant(SparklineVariant::Line)
                .line_preset(SparklineLinePreset::Ascii)
                .into(),
        );
        let content: String = node.output.rows[0]
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(content, "/^\\");
    }

    #[test]
    fn mirror_y_braille_flips_fill_direction() {
        let normal_node = into_node(
            Sparkline::new([1])
                .variant(SparklineVariant::Braille)
                .min(0)
                .max(4)
                .into(),
        );
        let mirrored_node = into_node(
            Sparkline::new([1])
                .variant(SparklineVariant::Braille)
                .min(0)
                .max(4)
                .mirror_y(true)
                .into(),
        );

        let normal: String = normal_node.output.rows[0]
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        let mirrored: String = mirrored_node.output.rows[0]
            .iter()
            .map(|s| s.content.as_ref())
            .collect();

        assert_eq!(normal, "⡀");
        assert_eq!(mirrored, "⠁");
    }

    #[test]
    fn data_arc_preserves_shared_slice() {
        use std::sync::Arc;

        let data: Arc<[u64]> = Arc::from([1u64, 2, 3, 4]);
        let spark = Sparkline::new([]).data_arc(Arc::clone(&data));
        assert!(Arc::ptr_eq(&spark.data, &data));
        assert_eq!(spark.data.as_ref(), &[1, 2, 3, 4]);
    }
}
