//! Color gradient helpers for value-to-color mapping.

use crate::style::Color;

/// Numeric range used when mapping values onto a gradient.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct GradientRange {
    /// Minimum value.
    pub min: u64,
    /// Maximum value.
    pub max: u64,
}

impl GradientRange {
    /// Create a new range.
    pub fn new(min: u64, max: u64) -> Self {
        Self { min, max }
    }

    /// Normalize a value into `[0.0, 1.0]` within this range.
    pub fn normalize(self, value: u64) -> f64 {
        let mut min = self.min;
        let mut max = self.max;
        if min > max {
            std::mem::swap(&mut min, &mut max);
        }
        if min == max {
            return 1.0;
        }
        let v = value.clamp(min, max);
        (v.saturating_sub(min) as f64) / (max.saturating_sub(min) as f64)
    }
}

impl From<(u64, u64)> for GradientRange {
    fn from((min, max): (u64, u64)) -> Self {
        Self { min, max }
    }
}

/// Direction for gradient rendering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GradientDirection {
    /// Color flows left to right across columns.
    #[default]
    Horizontal,
    /// Color flows top to bottom across rows.
    Vertical,
}

/// RGB gradient that supports two-stop and three-stop ramps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColorGradient {
    /// Color at the minimum end.
    pub min: Color,
    /// Optional center color used at `t = 0.5`.
    pub center: Option<Color>,
    /// Color at the maximum end.
    pub max: Color,
}

impl ColorGradient {
    /// Create a two-stop gradient from `min` to `max`.
    pub fn new(min: Color, max: Color) -> Self {
        Self {
            min,
            center: None,
            max,
        }
    }

    /// Add a center color stop.
    pub fn with_center(mut self, center: Color) -> Self {
        self.center = Some(center);
        self
    }

    /// Resolve a color at normalized position `t` in `[0.0, 1.0]`.
    pub fn color_at(self, t: f64) -> Color {
        let t = t.clamp(0.0, 1.0);
        if let Some(center) = self.center {
            if t <= 0.5 {
                return lerp_color(self.min, center, t * 2.0);
            }
            return lerp_color(center, self.max, (t - 0.5) * 2.0);
        }
        lerp_color(self.min, self.max, t)
    }

    /// Resolve a color for `value` using an explicit numeric range.
    pub fn color_for(self, value: u64, range: impl Into<GradientRange>) -> Color {
        let range = range.into();
        self.color_at(range.normalize(value))
    }

    /// Precompute a lookup table of `n` evenly-spaced colors across this gradient.
    ///
    /// Renderers call this once before a loop and index the result per row or
    /// column, avoiding repeated `f64` arithmetic per cell.
    pub fn precompute(self, n: usize) -> Vec<Color> {
        match n {
            0 => vec![],
            1 => vec![self.color_at(0.0)],
            _ => (0..n)
                .map(|i| self.color_at(i as f64 / (n - 1) as f64))
                .collect(),
        }
    }
}

fn lerp_color(a: Color, b: Color, t: f64) -> Color {
    let (ar, ag, ab) = a.to_rgb().unwrap_or((0, 0, 0));
    let (br, bg, bb) = b.to_rgb().unwrap_or((0, 0, 0));
    Color::Rgb(lerp_u8(ar, br, t), lerp_u8(ag, bg, t), lerp_u8(ab, bb, t))
}

fn lerp_u8(a: u8, b: u8, t: f64) -> u8 {
    let af = a as f64;
    let bf = b as f64;
    (af + (bf - af) * t).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::{ColorGradient, GradientRange};
    use crate::style::Color;

    #[test]
    fn range_normalize_handles_equal_bounds() {
        let range = GradientRange::new(10, 10);
        assert_eq!(range.normalize(10), 1.0);
        assert_eq!(range.normalize(5), 1.0);
    }

    #[test]
    fn two_stop_gradient_hits_endpoints() {
        let gradient = ColorGradient::new(Color::Rgb(0, 0, 0), Color::Rgb(255, 255, 255));
        assert_eq!(gradient.color_at(0.0), Color::Rgb(0, 0, 0));
        assert_eq!(gradient.color_at(1.0), Color::Rgb(255, 255, 255));
    }

    #[test]
    fn three_stop_gradient_uses_center() {
        let gradient = ColorGradient::new(Color::Rgb(255, 0, 0), Color::Rgb(0, 0, 255))
            .with_center(Color::Rgb(255, 255, 0));
        assert_eq!(gradient.color_at(0.5), Color::Rgb(255, 255, 0));
    }

    #[test]
    fn gradient_maps_value_by_range() {
        let gradient = ColorGradient::new(Color::Rgb(0, 0, 0), Color::Rgb(255, 0, 0));
        let color = gradient.color_for(50, (0, 100));
        assert_eq!(color, Color::Rgb(128, 0, 0));
    }
}
