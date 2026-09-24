//! Pixel images a capture carries beside its cell grid.

use std::sync::Arc;

use super::CapturedCell;
use crate::style::{Color, Rect};

/// Upper half block: its foreground paints a cell's top half, its background the bottom half.
#[cfg_attr(
    not(any(feature = "terminal-images", feature = "ui-snapshot-png")),
    allow(dead_code)
)]
pub(super) const UPPER_HALF: &str = "\u{2580}";

/// An image drawn over part of a [`CapturedFrame`](super::CapturedFrame), such as one a program in a
/// terminal pane displayed through the Kitty graphics protocol.
///
/// A cell grid cannot hold pixels, so the cells under a visible image carry a half-block
/// approximation of it (`▀` in the image's colors). Text and ANSI output show that approximation;
/// [`CapturedFrame::to_png`](super::CapturedFrame::to_png) draws the pixels themselves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedImage {
    /// Cells the image is laid out over, in frame coordinates. The pixels are scaled to fit
    /// inside, keeping their aspect ratio, from the top-left corner, as a terminal draws them.
    pub area: Rect,
    /// Width of [`Self::rgba`] in pixels.
    pub width: u32,
    /// Height of [`Self::rgba`] in pixels.
    pub height: u32,
    /// The pixels, 8-bit RGBA, row-major: `width * height * 4` bytes.
    pub rgba: Arc<[u8]>,
    /// Whether each cell of [`Self::area`] still shows the image, row-major. `false` where something
    /// drawn after it covers the cell: an overlay, a border, a pane above it.
    pub visible: Vec<bool>,
    /// The background each cell of [`Self::area`] had before its half-block stand-in replaced it,
    /// row-major. A PNG draws the image over these, so its transparent parts show what is behind
    /// it rather than the stand-in's colors.
    pub backgrounds: Vec<Color>,
}

impl CapturedImage {
    /// An image laid out over `area`, visible in every cell.
    ///
    /// # Panics
    ///
    /// If `rgba` is not `width * height * 4` bytes.
    pub fn new(area: Rect, width: u32, height: u32, rgba: Arc<[u8]>) -> Self {
        assert_eq!(
            rgba.len() as u64,
            u64::from(width) * u64::from(height) * 4,
            "rgba must hold width * height pixels"
        );
        Self {
            area,
            width,
            height,
            rgba,
            visible: vec![true; usize::from(area.w) * usize::from(area.h)],
            backgrounds: vec![Color::Reset; usize::from(area.w) * usize::from(area.h)],
        }
    }

    /// Whether the cell at frame position (`x`, `y`) shows this image.
    pub fn shows(&self, x: u16, y: u16) -> bool {
        self.area_offset(x, y)
            .and_then(|index| self.visible.get(index).copied())
            .unwrap_or(false)
    }

    /// Row-major index into [`Self::visible`] of frame cell (`x`, `y`), if the area holds it.
    pub(crate) fn area_offset(&self, x: u16, y: u16) -> Option<usize> {
        let col = i32::from(x) - i32::from(self.area.x);
        let row = i32::from(y) - i32::from(self.area.y);
        (col >= 0 && row >= 0 && col < i32::from(self.area.w) && row < i32::from(self.area.h))
            .then(|| row as usize * usize::from(self.area.w) + col as usize)
    }

    /// The size in pixels the image is drawn at, given cells of `cell_w` x `cell_h` pixels: as large
    /// as fits in [`Self::area`] without changing its aspect ratio.
    #[cfg_attr(
        not(any(feature = "terminal-images", feature = "ui-snapshot-png")),
        allow(dead_code)
    )]
    pub(crate) fn fitted_size(&self, cell_w: u32, cell_h: u32) -> (u32, u32) {
        let max_w = u32::from(self.area.w) * cell_w;
        let max_h = u32::from(self.area.h) * cell_h;
        if self.width == 0 || self.height == 0 || max_w == 0 || max_h == 0 {
            return (0, 0);
        }
        let ratio = (f64::from(max_w) / f64::from(self.width))
            .min(f64::from(max_h) / f64::from(self.height));
        (
            ((f64::from(self.width) * ratio).round() as u32).clamp(1, max_w),
            ((f64::from(self.height) * ratio).round() as u32).clamp(1, max_h),
        )
    }

    /// Average color of the source pixels a destination box covers, when the image is drawn at
    /// `fitted` pixels. The box is `[x0, x1) x [y0, y1)` in drawn pixels. Returns straight RGB plus
    /// the mean alpha; `None` for an empty box.
    #[cfg_attr(
        not(any(feature = "terminal-images", feature = "ui-snapshot-png")),
        allow(dead_code)
    )]
    pub(crate) fn sample(
        &self,
        fitted: (u32, u32),
        (x0, y0): (u32, u32),
        (x1, y1): (u32, u32),
    ) -> Option<[u8; 4]> {
        let (fw, fh) = fitted;
        if fw == 0 || fh == 0 || x1 <= x0 || y1 <= y0 {
            return None;
        }
        let map = |d: u32, drawn: u32, source: u32| -> u32 {
            (u64::from(d) * u64::from(source) / u64::from(drawn)) as u32
        };
        let sx0 = map(x0, fw, self.width).min(self.width - 1);
        let sy0 = map(y0, fh, self.height).min(self.height - 1);
        // At least one source pixel, so enlarging an image repeats pixels rather than finding none.
        let sx1 = map(x1, fw, self.width).clamp(sx0 + 1, self.width);
        let sy1 = map(y1, fh, self.height).clamp(sy0 + 1, self.height);

        let mut sum = [0u64; 3];
        let mut alpha_sum = 0u64;
        for sy in sy0..sy1 {
            let row = sy as usize * self.width as usize;
            for sx in sx0..sx1 {
                let px = &self.rgba[(row + sx as usize) * 4..][..4];
                let alpha = u64::from(px[3]);
                for (total, &channel) in sum.iter_mut().zip(&px[..3]) {
                    *total += u64::from(channel) * alpha;
                }
                alpha_sum += alpha;
            }
        }
        let count = u64::from(sx1 - sx0) * u64::from(sy1 - sy0);
        if alpha_sum == 0 {
            return Some([0, 0, 0, 0]);
        }
        Some([
            (sum[0] / alpha_sum) as u8,
            (sum[1] / alpha_sum) as u8,
            (sum[2] / alpha_sum) as u8,
            (alpha_sum / count) as u8,
        ])
    }

    /// Replace the visible cells this image covers with a half-block approximation of it, drawn with
    /// cells of `cell_w` x `cell_h` pixels, remembering each cell's background in
    /// [`Self::backgrounds`]. A half the image leaves transparent keeps the cell's background; a cell
    /// outside the drawn image is left as it is.
    #[cfg_attr(not(feature = "terminal-images"), allow(dead_code))]
    pub(crate) fn paint_half_blocks(
        &mut self,
        cells: &mut [CapturedCell],
        frame_width: u16,
        cell_w: u32,
        cell_h: u32,
    ) {
        let (cell_w, cell_h) = (cell_w.max(1), cell_h.max(2));
        let fitted = self.fitted_size(cell_w, cell_h);
        for row in 0..self.area.h {
            for col in 0..self.area.w {
                if !self.visible[usize::from(row) * usize::from(self.area.w) + usize::from(col)] {
                    continue;
                }
                let x = i32::from(self.area.x) + i32::from(col);
                let y = i32::from(self.area.y) + i32::from(row);
                if x < 0 || y < 0 || x >= i32::from(frame_width) {
                    continue;
                }
                let Some(cell) = cells.get_mut(y as usize * usize::from(frame_width) + x as usize)
                else {
                    continue;
                };
                self.backgrounds[usize::from(row) * usize::from(self.area.w) + usize::from(col)] =
                    cell.bg;
                let left = (u32::from(col) * cell_w).min(fitted.0);
                let right = ((u32::from(col) + 1) * cell_w).min(fitted.0);
                let top = (u32::from(row) * cell_h).min(fitted.1);
                let middle = (u32::from(row) * cell_h + cell_h / 2).min(fitted.1);
                let bottom = ((u32::from(row) + 1) * cell_h).min(fitted.1);
                let opaque = |sample: Option<[u8; 4]>| sample.filter(|&[.., alpha]| alpha >= 128);
                let upper = opaque(self.sample(fitted, (left, top), (right, middle)));
                let lower = opaque(self.sample(fitted, (left, middle), (right, bottom)));
                // A cell the image leaves clear keeps what it showed, so text output marks only
                // where the picture actually is.
                if upper.is_none() && lower.is_none() {
                    continue;
                }
                let background = cell.bg;
                let color = |sample: Option<[u8; 4]>| match sample {
                    Some([r, g, b, _]) => Color::Rgb(r, g, b),
                    None => background,
                };
                cell.symbol = UPPER_HALF.to_string();
                cell.fg = color(upper);
                cell.bg = color(lower);
                cell.modifiers = super::CellModifiers::default();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(
        area: Rect,
        width: u32,
        height: u32,
        pixel: impl Fn(u32, u32) -> [u8; 4],
    ) -> CapturedImage {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&pixel(x, y));
            }
        }
        CapturedImage::new(area, width, height, rgba.into())
    }

    fn area(x: i16, y: i16, w: u16, h: u16) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn an_image_fits_its_area_without_changing_shape() {
        let wide = image(area(0, 0, 4, 4), 200, 100, |_, _| [0, 0, 0, 255]);
        assert_eq!(wide.fitted_size(10, 20), (40, 20));
        let tall = image(area(0, 0, 4, 1), 10, 40, |_, _| [0, 0, 0, 255]);
        assert_eq!(tall.fitted_size(10, 20), (5, 20));
    }

    #[test]
    fn half_blocks_carry_the_top_and_bottom_colors_of_each_cell() {
        // Red above green, drawn over two rows of one cell each: every cell is half of each.
        let mut two_tone = image(area(1, 0, 1, 1), 10, 20, |_, y| {
            if y < 10 {
                [255, 0, 0, 255]
            } else {
                [0, 255, 0, 255]
            }
        });
        let blank = CapturedCell {
            symbol: " ".to_string(),
            fg: Color::Reset,
            bg: Color::Blue,
            underline_color: Color::Reset,
            modifiers: super::super::CellModifiers::default(),
        };
        let mut cells = vec![blank.clone(); 3];
        two_tone.paint_half_blocks(&mut cells, 3, 10, 20);

        assert_eq!(cells[0], blank);
        assert_eq!(cells[1].symbol, UPPER_HALF);
        assert_eq!(cells[1].fg, Color::Rgb(255, 0, 0));
        assert_eq!(cells[1].bg, Color::Rgb(0, 255, 0));
        assert_eq!(cells[2], blank);
        assert_eq!(two_tone.backgrounds, vec![Color::Blue]);
    }

    #[test]
    fn a_transparent_half_keeps_the_cell_background_and_hidden_cells_are_untouched() {
        let mut clear_below = image(area(0, 0, 2, 1), 20, 20, |_, y| {
            if y < 10 { [9, 9, 9, 255] } else { [0, 0, 0, 0] }
        });
        clear_below.visible[1] = false;
        let blank = CapturedCell {
            symbol: "x".to_string(),
            fg: Color::Reset,
            bg: Color::Blue,
            underline_color: Color::Reset,
            modifiers: super::super::CellModifiers::default(),
        };
        let mut cells = vec![blank.clone(); 2];
        clear_below.paint_half_blocks(&mut cells, 2, 10, 20);

        assert_eq!(cells[0].fg, Color::Rgb(9, 9, 9));
        assert_eq!(cells[0].bg, Color::Blue);
        assert_eq!(cells[1], blank);

        // A cell the image leaves wholly transparent keeps its text.
        let mut clear = image(area(0, 0, 1, 1), 10, 20, |_, _| [0, 0, 0, 0]);
        let mut cells = vec![blank.clone()];
        clear.paint_half_blocks(&mut cells, 1, 10, 20);
        assert_eq!(cells[0], blank);
        assert_eq!(clear.backgrounds, vec![Color::Blue]);
        assert!(clear_below.shows(0, 0));
        assert!(!clear_below.shows(1, 0));
        assert!(!clear_below.shows(2, 0));
    }
}
