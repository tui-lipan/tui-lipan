//! Box-drawing, block-element and Powerline characters, drawn from geometry rather than a font.
//!
//! Terminals draw these themselves, and for the same reason: a font's glyph rarely meets the cell
//! edge exactly, so borders gap or overlap, and a bitmap stretched to the cell turns a rounded
//! corner into a staircase. Lines here are pixel-exact at any cell size, their weight follows the
//! underline thickness, curves and diagonals are anti-aliased, and neighbouring cells always join.
//! Powerline separators fill the cell edge to edge, so a cap meets the segment it closes.

use image::RgbImage;

use super::{CellPixels, Rgb8, blend_rgb, decoration_thickness};

/// Weight of one arm of a line character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Weight {
    None,
    Light,
    Heavy,
    Double,
}

impl Weight {
    fn from_bits(bits: u8) -> Self {
        match bits & 0b11 {
            1 => Self::Light,
            2 => Self::Heavy,
            3 => Self::Double,
            _ => Self::None,
        }
    }

    /// Width of the band this arm occupies, in pixels, for a light line `light` pixels thick.
    fn band(self, light: u32) -> u32 {
        match self {
            Self::None => 0,
            Self::Light => light,
            Self::Heavy => light * 2,
            Self::Double => light * 3,
        }
    }
}

/// The arms of U+2500..=U+257F, one byte each: up << 6 | right << 4 | down << 2 | left, with 1 for
/// light, 2 for heavy and 3 for double. Zero marks the dashes, arcs and diagonals, drawn apart.
///
/// Generated from the Unicode character names, which spell out every arm and its weight.
const ARMS: [u8; 0x80] = [
    0x11, 0x22, 0x44, 0x88, 0x00, 0x00, 0x00, 0x00, // U+2500
    0x00, 0x00, 0x00, 0x00, 0x14, 0x24, 0x18, 0x28, // U+2508
    0x05, 0x06, 0x09, 0x0A, 0x50, 0x60, 0x90, 0xA0, // U+2510
    0x41, 0x42, 0x81, 0x82, 0x54, 0x64, 0x94, 0x58, // U+2518
    0x98, 0xA4, 0x68, 0xA8, 0x45, 0x46, 0x85, 0x49, // U+2520
    0x89, 0x86, 0x4A, 0x8A, 0x15, 0x16, 0x25, 0x26, // U+2528
    0x19, 0x1A, 0x29, 0x2A, 0x51, 0x52, 0x61, 0x62, // U+2530
    0x91, 0x92, 0xA1, 0xA2, 0x55, 0x56, 0x65, 0x66, // U+2538
    0x95, 0x59, 0x99, 0x96, 0xA5, 0x5A, 0x69, 0xA6, // U+2540
    0x6A, 0x9A, 0xA9, 0xAA, 0x00, 0x00, 0x00, 0x00, // U+2548
    0x33, 0xCC, 0x34, 0x1C, 0x3C, 0x07, 0x0D, 0x0F, // U+2550
    0x70, 0xD0, 0xF0, 0x43, 0xC1, 0xC3, 0x74, 0xDC, // U+2558
    0xFC, 0x47, 0xCD, 0xCF, 0x37, 0x1D, 0x3F, 0x73, // U+2560
    0xD1, 0xF3, 0x77, 0xDD, 0xFF, 0x00, 0x00, 0x00, // U+2568
    0x00, 0x00, 0x00, 0x00, 0x01, 0x40, 0x10, 0x04, // U+2570
    0x02, 0x80, 0x20, 0x08, 0x21, 0x48, 0x12, 0x84, // U+2578
];

/// Whether [`draw`] handles `ch`. Characters mixing single and double lines are left to the font:
/// they are rare, and fonts draw them well enough.
pub(super) fn handles(ch: char) -> bool {
    match ch {
        '\u{2500}'..='\u{257F}' => match line_arms(ch) {
            Some(arms) => {
                let double = arms.contains(&Weight::Double);
                !double
                    || arms
                        .iter()
                        .all(|&arm| matches!(arm, Weight::None | Weight::Double))
            }
            None => true,
        },
        '\u{2580}'..='\u{259F}' | '\u{E0B0}'..='\u{E0BF}' => true,
        _ => false,
    }
}

/// Draw `ch` into `cell` in `color`. Returns `false`, drawing nothing, for a character it does not
/// handle.
pub(super) fn draw(image: &mut RgbImage, cell: CellPixels, ch: char, color: Rgb8) -> bool {
    if !handles(ch) || cell.width == 0 || cell.height == 0 {
        return false;
    }
    let light = decoration_thickness(cell);
    let mut canvas = Canvas { image, cell, color };
    match ch {
        '\u{2504}'..='\u{250B}' | '\u{254C}'..='\u{254F}' => canvas.dashes(ch, light),
        '\u{256D}'..='\u{2570}' => canvas.arc(ch, light),
        '\u{2571}'..='\u{2573}' => canvas.diagonals(ch, light),
        '\u{2580}'..='\u{259F}' => canvas.block(ch),
        '\u{E0B0}'..='\u{E0BF}' => canvas.powerline(ch, light),
        _ => match line_arms(ch) {
            Some(arms) => canvas.lines(arms, light),
            None => return false,
        },
    }
    true
}

fn line_arms(ch: char) -> Option<[Weight; 4]> {
    let index = (ch as u32).checked_sub(0x2500)? as usize;
    let bits = *ARMS.get(index)?;
    (bits != 0).then(|| {
        [
            Weight::from_bits(bits >> 6),
            Weight::from_bits(bits >> 4),
            Weight::from_bits(bits >> 2),
            Weight::from_bits(bits),
        ]
    })
}

struct Canvas<'a> {
    image: &'a mut RgbImage,
    cell: CellPixels,
    color: Rgb8,
}

impl Canvas<'_> {
    /// Fill `[x0, x1) x [y0, y1)`, in pixels relative to the cell, clipped to it.
    fn fill(&mut self, x0: u32, y0: u32, x1: u32, y1: u32, alpha: u8) {
        for y in y0..y1.min(self.cell.height) {
            for x in x0..x1.min(self.cell.width) {
                blend_rgb(
                    self.image,
                    self.cell.x0 + x,
                    self.cell.y0 + y,
                    self.color,
                    alpha,
                );
            }
        }
    }

    /// Straight arms, light, heavy or double. Strokes are collected in a mask first, so a double
    /// line's gap can be carved out of the bands before anything is painted.
    fn lines(&mut self, arms: [Weight; 4], light: u32) {
        let (w, h) = (self.cell.width, self.cell.height);
        let [up, right, down, left] = arms;
        // Bands are centered the same way whatever their width, so a light line lands in the middle
        // of a heavy or double one and every weight joins its neighbours.
        let vertical = up.band(light).max(down.band(light));
        let horizontal = left.band(light).max(right.band(light));
        let vstart = (w.saturating_sub(vertical)) / 2;
        let hstart = (h.saturating_sub(horizontal)) / 2;

        let mut mask = vec![false; (w * h) as usize];
        let mut set = |x0: u32, y0: u32, x1: u32, y1: u32, value: bool| {
            for y in y0..y1.min(h) {
                for x in x0..x1.min(w) {
                    mask[(y * w + x) as usize] = value;
                }
            }
        };
        let band = |weight: Weight, size: u32| {
            let width = weight.band(light);
            let start = size.saturating_sub(width) / 2;
            (start, start + width)
        };

        // Each arm runs from its edge into the center, far enough to cover the band crossing it.
        if right != Weight::None {
            let (y0, y1) = band(right, h);
            set(vstart, y0, w, y1, true);
        }
        if left != Weight::None {
            let (y0, y1) = band(left, h);
            set(0, y0, vstart + vertical, y1, true);
        }
        if down != Weight::None {
            let (x0, x1) = band(down, w);
            set(x0, hstart, x1, h, true);
        }
        if up != Weight::None {
            let (x0, x1) = band(up, w);
            set(x0, 0, x1, hstart + horizontal, true);
        }

        // A double arm is a triple-width band with its middle carved out. Carving from where the
        // crossing band's own gap starts keeps the outer strokes whole around a corner and breaks
        // the inner ones where two double lines meet.
        let vertical_double = up == Weight::Double || down == Weight::Double;
        let horizontal_double = left == Weight::Double || right == Weight::Double;
        let gap = |size: u32| {
            let start = size.saturating_sub(light * 3) / 2 + light;
            (start, start + light)
        };
        if right == Weight::Double {
            let (y0, y1) = gap(h);
            let from = if vertical_double {
                vstart + light
            } else {
                vstart
            };
            set(from, y0, w, y1, false);
        }
        if left == Weight::Double {
            let (y0, y1) = gap(h);
            let to = if vertical_double {
                vstart + light * 2
            } else {
                vstart + vertical
            };
            set(0, y0, to, y1, false);
        }
        if down == Weight::Double {
            let (x0, x1) = gap(w);
            let from = if horizontal_double {
                hstart + light
            } else {
                hstart
            };
            set(x0, from, x1, h, false);
        }
        if up == Weight::Double {
            let (x0, x1) = gap(w);
            let to = if horizontal_double {
                hstart + light * 2
            } else {
                hstart + horizontal
            };
            set(x0, 0, x1, to, false);
        }

        for y in 0..h {
            for x in 0..w {
                if mask[(y * w + x) as usize] {
                    blend_rgb(
                        self.image,
                        self.cell.x0 + x,
                        self.cell.y0 + y,
                        self.color,
                        u8::MAX,
                    );
                }
            }
        }
    }

    /// `┄ ┅ ┆ ┇ ┈ ┉ ┊ ┋ ╌ ╍ ╎ ╏`: a line broken into two, three or four dashes.
    fn dashes(&mut self, ch: char, light: u32) {
        let (count, heavy, vertical) = match ch {
            '\u{2504}' => (3, false, false),
            '\u{2505}' => (3, true, false),
            '\u{2506}' => (3, false, true),
            '\u{2507}' => (3, true, true),
            '\u{2508}' => (4, false, false),
            '\u{2509}' => (4, true, false),
            '\u{250A}' => (4, false, true),
            '\u{250B}' => (4, true, true),
            '\u{254C}' => (2, false, false),
            '\u{254D}' => (2, true, false),
            '\u{254E}' => (2, false, true),
            _ => (2, true, true),
        };
        let thickness = if heavy { light * 2 } else { light };
        let (w, h) = (self.cell.width, self.cell.height);
        let length = if vertical { h } else { w };
        let across = if vertical { w } else { h };
        let start = across.saturating_sub(thickness) / 2;
        for dash in 0..count {
            let from = dash * length / count;
            let to = (dash + 1) * length / count;
            // The gap sits at the end of each segment, so dashes stay evenly spaced across cells.
            let gap = ((to - from) / 3).max(1);
            let (a, b) = (from, to.saturating_sub(gap).max(from + 1));
            if vertical {
                self.fill(start, a, start + thickness, b, u8::MAX);
            } else {
                self.fill(a, start, b, start + thickness, u8::MAX);
            }
        }
    }

    /// `╭ ╮ ╯ ╰`: a quarter circle joining the two arms, anti-aliased.
    fn arc(&mut self, ch: char, light: u32) {
        let (w, h) = (self.cell.width as f32, self.cell.height as f32);
        let t = light as f32;
        // The line centers match the straight characters' bands exactly.
        let cx = ((self.cell.width - light.min(self.cell.width)) / 2) as f32 + t / 2.0;
        let cy = ((self.cell.height - light.min(self.cell.height)) / 2) as f32 + t / 2.0;
        // Which way the arms leave the cell: right (+1) or left (-1), down (+1) or up (-1).
        let (sx, sy) = match ch {
            '\u{256D}' => (1.0, 1.0),
            '\u{256E}' => (-1.0, 1.0),
            '\u{256F}' => (-1.0, -1.0),
            _ => (1.0, -1.0),
        };
        let reach_x = if sx > 0.0 { w - cx } else { cx };
        let reach_y = if sy > 0.0 { h - cy } else { cy };
        let radius = reach_x.min(reach_y);
        let (ax, ay) = (cx + sx * radius, cy + sy * radius);
        self.supersample(|x, y| {
            let (dx, dy) = (x - ax, y - ay);
            if dx * sx <= 0.0 && dy * sy <= 0.0 {
                // The quarter of the circle facing the corner the arms leave from.
                ((dx * dx + dy * dy).sqrt() - radius).abs() <= t / 2.0
            } else if dx * sx > 0.0 && dy * sy <= 0.0 {
                // Past the arc sideways: the horizontal arm.
                (y - cy).abs() <= t / 2.0
            } else if dy * sy > 0.0 && dx * sx <= 0.0 {
                // Past the arc lengthways: the vertical arm.
                (x - cx).abs() <= t / 2.0
            } else {
                false
            }
        });
    }

    /// `╱ ╲ ╳`: corner-to-corner lines, anti-aliased, so diagonal runs join across cells.
    fn diagonals(&mut self, ch: char, light: u32) {
        let (w, h) = (self.cell.width as f32, self.cell.height as f32);
        let half = light as f32 / 2.0;
        let length = (w * w + h * h).sqrt();
        // Distance from (x, y) to the line through the two corners, measured perpendicular to it.
        let rising = move |x: f32, y: f32| (h * x + w * y - w * h).abs() / length <= half;
        let falling = move |x: f32, y: f32| (h * x - w * y).abs() / length <= half;
        match ch {
            '\u{2571}' => self.supersample(rising),
            '\u{2572}' => self.supersample(falling),
            _ => self.supersample(move |x, y| rising(x, y) || falling(x, y)),
        }
    }

    /// U+E0B0..=U+E0BF: Powerline arrows, half circles and corner triangles, solid or thin.
    ///
    /// Solid shapes span the full cell height and reach the edge they point from, where they meet
    /// the segment they close. Thin ones trace the same outline at the light line weight.
    fn powerline(&mut self, ch: char, light: u32) {
        let (w, h) = (self.cell.width as f32, self.cell.height as f32);
        let half = light as f32 / 2.0;
        let mid = h / 2.0;
        // Shapes are described pointing right or leaning one way; `mirror` flips them left.
        let mirror = matches!(ch, '\u{E0B2}' | '\u{E0B3}' | '\u{E0B6}' | '\u{E0B7}');
        let flip = move |x: f32| if mirror { w - x } else { x };
        match ch {
            // U+E0B0 and U+E0B2: from the full-height back edge to a point at mid height.
            '\u{E0B0}' | '\u{E0B2}' => {
                self.supersample(move |x, y| flip(x) <= w * (1.0 - (y - mid).abs() / mid));
            }
            // U+E0B1 and U+E0B3: the same arrow's two slanted sides.
            '\u{E0B1}' | '\u{E0B3}' => self.supersample(move |x, y| {
                let x = flip(x);
                segment_distance(x, y, (0.0, 0.0), (w, mid)) <= half
                    || segment_distance(x, y, (w, mid), (0.0, h)) <= half
            }),
            // U+E0B4 and U+E0B6: half an ellipse whose flat side is the back edge.
            '\u{E0B4}' | '\u{E0B6}' => {
                self.supersample(move |x, y| ellipse(flip(x), y - mid, w, mid) <= 1.0);
            }
            // U+E0B5 and U+E0B7: its outline, a ring `light` pixels thick.
            '\u{E0B5}' | '\u{E0B7}' => self.supersample(move |x, y| {
                let (x, y) = (flip(x), y - mid);
                ellipse(x, y, w, mid) <= 1.0
                    && ellipse(
                        x,
                        y,
                        (w - light as f32).max(0.0),
                        (mid - light as f32).max(0.0),
                    ) > 1.0
            }),
            // U+E0B8, U+E0BA, U+E0BC and U+E0BE: the triangles on one side of a cell diagonal.
            '\u{E0B8}' => self.supersample(move |x, y| x / w <= y / h),
            '\u{E0BA}' => self.supersample(move |x, y| x / w >= 1.0 - y / h),
            '\u{E0BC}' => self.supersample(move |x, y| x / w <= 1.0 - y / h),
            '\u{E0BE}' => self.supersample(move |x, y| x / w >= y / h),
            // U+E0B9, U+E0BB, U+E0BD and U+E0BF: those diagonals alone.
            '\u{E0B9}' | '\u{E0BF}' => {
                self.supersample(move |x, y| segment_distance(x, y, (0.0, 0.0), (w, h)) <= half);
            }
            _ => self.supersample(move |x, y| segment_distance(x, y, (0.0, h), (w, 0.0)) <= half),
        }
    }

    /// Paint each pixel by the share of a 4x4 grid of samples inside `covered`, in cell pixels.
    fn supersample(&mut self, covered: impl Fn(f32, f32) -> bool) {
        const GRID: u32 = 4;
        for y in 0..self.cell.height {
            for x in 0..self.cell.width {
                let mut hits = 0u32;
                for sy in 0..GRID {
                    for sx in 0..GRID {
                        let px = x as f32 + (sx as f32 + 0.5) / GRID as f32;
                        let py = y as f32 + (sy as f32 + 0.5) / GRID as f32;
                        hits += u32::from(covered(px, py));
                    }
                }
                if hits > 0 {
                    let alpha = (hits * u32::from(u8::MAX) / (GRID * GRID)) as u8;
                    blend_rgb(
                        self.image,
                        self.cell.x0 + x,
                        self.cell.y0 + y,
                        self.color,
                        alpha,
                    );
                }
            }
        }
    }

    /// U+2580..=U+259F: eighths, halves, quadrants and shades, filled exactly.
    fn block(&mut self, ch: char) {
        let (w, h) = (self.cell.width, self.cell.height);
        let x_at = |eighths: u32| (w * eighths + 4) / 8;
        let y_at = |eighths: u32| (h * eighths + 4) / 8;
        let (mid_x, mid_y) = (x_at(4), y_at(4));
        let code = ch as u32;
        match ch {
            '\u{2580}' => self.fill(0, 0, w, mid_y, u8::MAX),
            '\u{2581}'..='\u{2588}' => {
                let eighths = code - 0x2580;
                self.fill(0, y_at(8 - eighths), w, h, u8::MAX);
            }
            '\u{2589}'..='\u{258F}' => {
                let eighths = 0x2590 - code;
                self.fill(0, 0, x_at(eighths), h, u8::MAX);
            }
            '\u{2590}' => self.fill(mid_x, 0, w, h, u8::MAX),
            '\u{2591}' => self.fill(0, 0, w, h, 64),
            '\u{2592}' => self.fill(0, 0, w, h, 128),
            '\u{2593}' => self.fill(0, 0, w, h, 191),
            '\u{2594}' => self.fill(0, 0, w, y_at(1), u8::MAX),
            '\u{2595}' => self.fill(x_at(7), 0, w, h, u8::MAX),
            _ => {
                // Quadrants: upper left, upper right, lower left, lower right.
                let [ul, ur, ll, lr] = match ch {
                    '\u{2596}' => [false, false, true, false],
                    '\u{2597}' => [false, false, false, true],
                    '\u{2598}' => [true, false, false, false],
                    '\u{2599}' => [true, false, true, true],
                    '\u{259A}' => [true, false, false, true],
                    '\u{259B}' => [true, true, true, false],
                    '\u{259C}' => [true, true, false, true],
                    '\u{259D}' => [false, true, false, false],
                    '\u{259E}' => [false, true, true, false],
                    _ => [false, true, true, true],
                };
                for (on, x0, y0, x1, y1) in [
                    (ul, 0, 0, mid_x, mid_y),
                    (ur, mid_x, 0, w, mid_y),
                    (ll, 0, mid_y, mid_x, h),
                    (lr, mid_x, mid_y, w, h),
                ] {
                    if on {
                        self.fill(x0, y0, x1, y1, u8::MAX);
                    }
                }
            }
        }
    }
}

/// Distance from `(x, y)` to the segment from `a` to `b`.
fn segment_distance(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((x - a.0) * dx + (y - a.1) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (px, py) = (a.0 + t * dx - x, a.1 + t * dy - y);
    (px * px + py * py).sqrt()
}

/// Where `(x, y)` lies against the ellipse centered on the origin with radii `rx` and `ry`: at most
/// `1.0` inside it. A zero radius leaves nothing inside.
fn ellipse(x: f32, y: f32, rx: f32, ry: f32) -> f32 {
    if rx <= 0.0 || ry <= 0.0 {
        return f32::INFINITY;
    }
    (x / rx).powi(2) + (y / ry).powi(2)
}

#[cfg(test)]
mod tests {
    use image::Rgb;

    use super::*;

    const INK: Rgb8 = (255, 255, 255);

    /// Draw `ch` into one `w` x `h` cell and return, per row, which pixels have any ink.
    fn render(ch: char, w: u32, h: u32) -> Vec<String> {
        let mut image = RgbImage::new(w, h);
        let cell = CellPixels {
            x0: 0,
            y0: 0,
            width: w,
            height: h,
        };
        assert!(draw(&mut image, cell, ch, INK), "{ch:?} should be drawn");
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| match image.get_pixel(x, y) {
                        Rgb([0, 0, 0]) => '.',
                        Rgb([255, 255, 255]) => '#',
                        _ => '+',
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn light_lines_meet_every_edge_they_name_and_join_in_the_middle() {
        assert_eq!(
            render('┼', 8, 8),
            [
                "...#....", "...#....", "...#....", "########", "...#....", "...#....", "...#....",
                "...#....",
            ]
        );
        assert_eq!(
            render('┌', 8, 8),
            [
                "........", "........", "........", "...#####", "...#....", "...#....", "...#....",
                "...#....",
            ]
        );
    }

    #[test]
    fn a_heavy_line_is_twice_as_thick_and_centered_on_the_light_one() {
        let heavy = render('━', 8, 16);
        assert_eq!(heavy.iter().filter(|row| row.contains('#')).count(), 2);
        let light = render('─', 8, 16);
        let row = |rows: &[String]| rows.iter().position(|row| row.contains('#')).unwrap();
        assert!((row(&heavy)..row(&heavy) + 2).contains(&row(&light)));
    }

    #[test]
    fn double_lines_keep_their_outer_strokes_around_a_corner() {
        assert_eq!(
            render('╔', 9, 9),
            [
                ".........",
                ".........",
                ".........",
                "...######",
                "...#.....",
                "...#.####",
                "...#.#...",
                "...#.#...",
                "...#.#...",
            ]
        );
        // Four double lines meeting leave four separate corners: both gaps run straight through.
        let crossing = render('╬', 9, 9);
        assert_eq!(crossing[4], ".........");
        assert!(crossing.iter().all(|row| row.chars().nth(4) == Some('.')));
        assert_eq!(crossing[3], "####.####");
    }

    #[test]
    fn a_rounded_corner_is_smooth_and_meets_the_straight_lines() {
        let rows = render('╭', 8, 16);
        // The arc is anti-aliased: some pixels are partly covered.
        assert!(rows.iter().any(|row| row.contains('+')), "{rows:#?}");
        // It leaves the cell exactly where `─` and `│` would continue it.
        let horizontal = render('─', 8, 16);
        let line_row = horizontal.iter().position(|row| row.contains('#')).unwrap();
        assert_ne!(rows[line_row].chars().last(), Some('.'), "{rows:#?}");
        let vertical = render('│', 8, 16);
        let line_col = vertical[0].find('#').unwrap();
        assert_ne!(rows[15].chars().nth(line_col), Some('.'), "{rows:#?}");
        assert!(rows[0].chars().all(|c| c == '.'), "nothing above the arc");
    }

    #[test]
    fn block_elements_fill_exact_fractions_of_the_cell() {
        let half = render('▀', 4, 8);
        assert!(half[..4].iter().all(|row| row == "####"));
        assert!(half[4..].iter().all(|row| row == "...."));
        let eighth = render('▁', 4, 8);
        assert_eq!(eighth.iter().filter(|row| row.contains('#')).count(), 1);
        let quadrants = render('▚', 4, 4);
        assert_eq!(quadrants, ["##..", "##..", "..##", "..##"]);
        assert!(render('░', 2, 2).iter().all(|row| row == "++"));
    }

    #[test]
    fn powerline_arrows_fill_the_edge_they_point_away_from() {
        // An odd width leaves one pixel of slack, which centering a font glyph would put beside
        // the segment the cap closes.
        let right = render('\u{E0B0}', 15, 32);
        // The back edge is solid down its whole height; only the corner pixels, where the slope
        // leaves less than a pixel of ink, are partly covered.
        assert!(
            right[1..31].iter().all(|row| row.starts_with('#')),
            "{right:#?}"
        );
        assert!(right[0].starts_with('+') && right[31].starts_with('+'));
        assert!(!right[16].ends_with('.'), "the point reaches the far edge");
        assert!(right[0].ends_with('.') && right[31].ends_with('.'));

        let left = render('\u{E0B2}', 15, 32);
        assert!(
            left[1..31].iter().all(|row| row.ends_with('#')),
            "{left:#?}"
        );
        assert!(!left[16].starts_with('.'));
        let mirrored: Vec<String> = right
            .iter()
            .map(|row| row.chars().rev().collect())
            .collect();
        assert_eq!(left, mirrored);
    }

    #[test]
    fn powerline_half_circles_span_the_full_height_of_their_flat_edge() {
        let right = render('\u{E0B4}', 15, 32);
        assert!(
            right[1..31].iter().all(|row| row.starts_with('#')),
            "{right:#?}"
        );
        assert!(!right[16].ends_with('.'));
        let left = render('\u{E0B6}', 15, 32);
        assert!(
            left[1..31].iter().all(|row| row.ends_with('#')),
            "{left:#?}"
        );

        // The thin variants are outlines: their middle stays empty.
        let ring = render('\u{E0B5}', 15, 32);
        assert_eq!(ring[16].chars().nth(4), Some('.'), "{ring:#?}");
        assert!(!ring[16].ends_with('.'));
    }

    #[test]
    fn powerline_triangles_split_the_cell_on_a_diagonal() {
        let lower_left = render('\u{E0B8}', 8, 8);
        assert!(lower_left[7].starts_with("#######"), "{lower_left:#?}");
        assert!(lower_left[0].ends_with("#######".replace('#', ".").as_str()));
        let upper_right = render('\u{E0BE}', 8, 8);
        assert!(upper_right[0].ends_with("#######"), "{upper_right:#?}");
        assert!(upper_right[7].starts_with("......."));
        let thin = render('\u{E0B9}', 8, 8);
        assert_ne!(thin[0].chars().next(), Some('.'));
        assert_ne!(thin[7].chars().last(), Some('.'));
        assert_eq!(thin[7].chars().next(), Some('.'));
    }

    #[test]
    fn characters_mixing_single_and_double_lines_are_left_to_the_font() {
        assert!(!handles('╒'));
        assert!(!handles('╫'));
        assert!(handles('═') && handles('╬') && handles('┄') && handles('╳'));
        assert!(handles('\u{E0B0}') && handles('\u{E0BF}'));
        assert!(!handles('a') && !handles('\u{E0A0}') && !handles('\u{E0C0}'));
    }
}
