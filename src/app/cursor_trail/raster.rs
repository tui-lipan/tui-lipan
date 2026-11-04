//! Drawing the smear into cells.
//!
//! The smear is a convex shape in cell coordinates. Each cell it touches is sampled on an 8x8
//! grid, and the sample mask is matched against the block glyphs a terminal font is sure to have:
//! eighth blocks from each side, halves, quadrants, and (opt in) sextants. A glyph can be drawn in
//! the trail color over the cell's background, or inverted, background over trail color, which is
//! how an "upper three quarters" comes out of a "lower quarter". The best match wins, so edges
//! come out sub-cell accurate in any direction.
//!
//! A cell the smear covers completely keeps its text, drawn in inverse colors the way a block
//! cursor shows the character under it.

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Position;
use ratatui::style::{Color as RColor, Modifier};

use super::{CursorTrailFill, Vec2};
use crate::style::Color;

const SAMPLES: u32 = 8;
const FULL: u64 = u64::MAX;
/// A tint mixes at most this much trail color into the background, so text stays readable.
const TINT_MAX: f32 = 0.75;
/// Without RGB to blend, a cell at least this strong takes the trail color outright.
const SOLID_THRESHOLD: f32 = 0.45;

/// A convex polygon, its vertices in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hull {
    points: [Vec2; 4],
    len: usize,
}

impl Hull {
    /// The convex hull of four corners, which is the smear even when spring motion has twisted
    /// the corners out of order.
    pub(crate) fn of(corners: &[Vec2; 4]) -> Self {
        let mut points = *corners;
        points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        // Andrew's monotone chain: a lower and an upper chain, each without its last point.
        let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).cross(b - o);
        let chain = |points: &mut dyn Iterator<Item = Vec2>| {
            let mut chain: Vec<Vec2> = Vec::with_capacity(4);
            for point in points {
                while chain.len() >= 2
                    && cross(chain[chain.len() - 2], chain[chain.len() - 1], point) <= 0.0
                {
                    chain.pop();
                }
                chain.push(point);
            }
            chain.pop();
            chain
        };
        let mut hull = chain(&mut points.iter().copied());
        hull.extend(chain(&mut points.iter().rev().copied()));
        let mut out = [Vec2::ZERO; 4];
        let len = hull.len().min(4);
        out[..len].copy_from_slice(&hull[..len]);
        Self { points: out, len }
    }

    fn vertices(&self) -> &[Vec2] {
        &self.points[..self.len]
    }

    #[cfg(test)]
    pub(crate) fn vertices_for_tests(&self) -> &[Vec2] {
        self.vertices()
    }

    fn contains(&self, point: Vec2) -> bool {
        let vertices = self.vertices();
        match vertices.len() {
            0 => false,
            1 => (vertices[0] - point).length() < 1e-4,
            2 => {
                let (a, b) = (vertices[0], vertices[1]);
                (b - a).cross(point - a).abs() < 1e-4
                    && (point - a).dot(b - a) >= 0.0
                    && (point - b).dot(a - b) >= 0.0
            }
            _ => (0..vertices.len()).all(|index| {
                let a = vertices[index];
                let b = vertices[(index + 1) % vertices.len()];
                (b - a).cross(point - a) >= 0.0
            }),
        }
    }

    /// The cells the hull touches, as an inclusive range of columns and rows.
    fn cell_bounds(&self) -> Option<(i32, i32, i32, i32)> {
        let vertices = self.vertices();
        if vertices.is_empty() {
            return None;
        }
        let (mut left, mut top) = (f32::INFINITY, f32::INFINITY);
        let (mut right, mut bottom) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for point in vertices {
            left = left.min(point.x);
            top = top.min(point.y);
            right = right.max(point.x);
            bottom = bottom.max(point.y);
        }
        Some((
            left.floor() as i32,
            top.floor() as i32,
            (right.ceil() as i32 - 1).max(left.floor() as i32),
            (bottom.ceil() as i32 - 1).max(top.floor() as i32),
        ))
    }

    /// Which of the cell's 8x8 samples fall inside, bit `row * 8 + column`.
    fn coverage(&self, column: i32, row: i32) -> u64 {
        let mut mask = 0;
        for sample_row in 0..SAMPLES {
            for sample_column in 0..SAMPLES {
                let point = Vec2::new(
                    column as f32 + (sample_column as f32 + 0.5) / SAMPLES as f32,
                    row as f32 + (sample_row as f32 + 0.5) / SAMPLES as f32,
                );
                if self.contains(point) {
                    mask |= 1 << (sample_row * SAMPLES + sample_column);
                }
            }
        }
        mask
    }
}

/// A block glyph and the samples it paints.
#[derive(Clone, Copy, Debug)]
struct Glyph {
    symbol: char,
    mask: u64,
}

fn rows(range: std::ops::Range<u32>) -> u64 {
    range.fold(0, |mask, row| mask | (0xFF << (row * SAMPLES)))
}

fn columns(range: std::ops::Range<u32>) -> u64 {
    let row: u64 = range.fold(0, |mask, column| mask | (1 << column));
    (0..SAMPLES).fold(0, |mask, r| mask | (row << (r * SAMPLES)))
}

fn block(column_range: std::ops::Range<u32>, row_range: std::ops::Range<u32>) -> u64 {
    columns(column_range) & rows(row_range)
}

/// Glyphs every terminal font with box drawing has: eighths, halves, quadrants.
fn core_glyphs() -> &'static [Glyph] {
    static GLYPHS: std::sync::OnceLock<Vec<Glyph>> = std::sync::OnceLock::new();
    GLYPHS.get_or_init(|| {
        let mut glyphs = vec![Glyph {
            symbol: '█',
            mask: FULL,
        }];
        for (eighths, symbol) in (1..8).zip(['▁', '▂', '▃', '▄', '▅', '▆', '▇']) {
            glyphs.push(Glyph {
                symbol,
                mask: rows(8 - eighths..8),
            });
        }
        for (eighths, symbol) in (1..8).zip(['▏', '▎', '▍', '▌', '▋', '▊', '▉']) {
            glyphs.push(Glyph {
                symbol,
                mask: columns(0..eighths),
            });
        }
        let (left, right, top, bottom) = (0..4, 4..8, 0..4, 4..8);
        let quadrant = |tl: bool, tr: bool, bl: bool, br: bool| {
            let mut mask = 0;
            if tl {
                mask |= block(left.clone(), top.clone());
            }
            if tr {
                mask |= block(right.clone(), top.clone());
            }
            if bl {
                mask |= block(left.clone(), bottom.clone());
            }
            if br {
                mask |= block(right.clone(), bottom.clone());
            }
            mask
        };
        glyphs.extend([
            Glyph {
                symbol: '▀',
                mask: rows(0..4),
            },
            Glyph {
                symbol: '▔',
                mask: rows(0..1),
            },
            Glyph {
                symbol: '▐',
                mask: columns(4..8),
            },
            Glyph {
                symbol: '▕',
                mask: columns(7..8),
            },
            Glyph {
                symbol: '▘',
                mask: quadrant(true, false, false, false),
            },
            Glyph {
                symbol: '▝',
                mask: quadrant(false, true, false, false),
            },
            Glyph {
                symbol: '▖',
                mask: quadrant(false, false, true, false),
            },
            Glyph {
                symbol: '▗',
                mask: quadrant(false, false, false, true),
            },
            Glyph {
                symbol: '▚',
                mask: quadrant(true, false, false, true),
            },
            Glyph {
                symbol: '▞',
                mask: quadrant(false, true, true, false),
            },
            Glyph {
                symbol: '▙',
                mask: quadrant(true, false, true, true),
            },
            Glyph {
                symbol: '▛',
                mask: quadrant(true, true, true, false),
            },
            Glyph {
                symbol: '▜',
                mask: quadrant(true, true, false, true),
            },
            Glyph {
                symbol: '▟',
                mask: quadrant(false, true, true, true),
            },
        ]);
        glyphs
    })
}

/// The 2x3 sextants of Symbols for Legacy Computing (U+1FB00), for fonts that have them.
fn sextant_glyphs() -> &'static [Glyph] {
    static GLYPHS: std::sync::OnceLock<Vec<Glyph>> = std::sync::OnceLock::new();
    GLYPHS.get_or_init(|| {
        // Thirds of eight sample rows, as near as eight allows.
        let thirds = [0..3, 3..5, 5..8];
        (1u32..63)
            .filter(|pattern| *pattern != 21 && *pattern != 42)
            .map(|pattern| {
                let mut mask = 0;
                for bit in 0..6 {
                    if pattern & (1 << bit) != 0 {
                        let column = if bit % 2 == 0 { 0..4 } else { 4..8 };
                        mask |= block(column, thirds[bit as usize / 2].clone());
                    }
                }
                // Unicode leaves out the two patterns that already exist as half blocks.
                let skipped = u32::from(pattern > 21) + u32::from(pattern > 42);
                let symbol =
                    char::from_u32(0x1FB00 + pattern - 1 - skipped).expect("sextant code point");
                Glyph { symbol, mask }
            })
            .collect()
    })
}

/// How a cell's coverage is best drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Match {
    /// Not at all: the cell stays as it is.
    Empty,
    /// Wholly covered.
    Full,
    /// `symbol` in the trail color over the background, or (`inverted`) the background color over
    /// the trail color.
    Glyph { symbol: char, inverted: bool },
}

fn best_match(mask: u64, allow_inverted: bool, sextants: bool) -> Match {
    let mut best = (mask.count_ones(), Match::Empty);
    let mut consider = |cost: u32, candidate: Match| {
        if cost < best.0 {
            best = (cost, candidate);
        }
    };
    consider((!mask).count_ones(), Match::Full);
    let extra: &[Glyph] = if sextants { sextant_glyphs() } else { &[] };
    // Plain glyphs first: an inverted one only wins by matching strictly better, so a tie goes
    // to the glyph that also reads right where colors are approximated.
    for glyph in core_glyphs().iter().skip(1).chain(extra) {
        consider(
            (mask ^ glyph.mask).count_ones(),
            Match::Glyph {
                symbol: glyph.symbol,
                inverted: false,
            },
        );
    }
    if allow_inverted {
        for glyph in core_glyphs().iter().skip(1).chain(extra) {
            consider(
                (mask ^ !glyph.mask).count_ones(),
                Match::Glyph {
                    symbol: glyph.symbol,
                    inverted: true,
                },
            );
        }
    }
    best.1
}

/// Everything one frame of the smear needs to be painted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SmearPaint {
    pub(crate) hull: Hull,
    /// Where the smear is strongest (the caret) and where it fades from, for `taper`.
    pub(crate) head: Vec2,
    pub(crate) tail: Vec2,
    pub(crate) fill: CursorTrailFill,
    pub(crate) color: Color,
    pub(crate) opacity: f32,
    pub(crate) taper: f32,
    pub(crate) sextants: bool,
}

pub(crate) fn rgb(color: RColor) -> Option<(u8, u8, u8)> {
    match color {
        RColor::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    }
}

pub(crate) fn mix(from: (u8, u8, u8), to: (u8, u8, u8), amount: f32) -> (u8, u8, u8) {
    let amount = amount.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| {
        (f32::from(a) + (f32::from(b) - f32::from(a)) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (
        channel(from.0, to.0),
        channel(from.1, to.1),
        channel(from.2, to.2),
    )
}

/// The cell's background as RGB: its own, or the host's for the terminal default.
pub(crate) fn under(cell: &Cell, host_bg: Option<(u8, u8, u8)>) -> Option<(u8, u8, u8)> {
    match cell.bg {
        RColor::Reset => host_bg,
        other => rgb(other),
    }
}

fn single_width(cell: &Cell) -> bool {
    unicode_width::UnicodeWidthStr::width(cell.symbol()) == 1 || cell.symbol() == " "
}

impl SmearPaint {
    fn strength(&self, column: i32, row: i32) -> f32 {
        let axis = self.head - self.tail;
        let length_squared = axis.dot(axis);
        let along = if length_squared < f32::EPSILON {
            1.0
        } else {
            let point = Vec2::new(column as f32 + 0.5, row as f32 + 0.5);
            ((point - self.tail).dot(axis) / length_squared).clamp(0.0, 1.0)
        };
        self.opacity.clamp(0.0, 1.0) * (1.0 - self.taper.clamp(0.0, 1.0) * (1.0 - along))
    }

    /// Whether the smear covers the centre of the cell at `position`.
    pub(crate) fn covers(&self, position: Position) -> bool {
        self.hull.contains(Vec2::new(
            f32::from(position.x) + 0.5,
            f32::from(position.y) + 0.5,
        ))
    }

    pub(crate) fn paint(&self, buffer: &mut Buffer, host_bg: Option<(u8, u8, u8)>) {
        let Some((left, top, right, bottom)) = self.hull.cell_bounds() else {
            return;
        };
        let area = buffer.area;
        let trail = crate::backend::ratatui_backend::common::to_ratatui_color(self.color);
        let trail_rgb = rgb(trail);
        for row in top.max(i32::from(area.top()))..=bottom.min(i32::from(area.bottom()) - 1) {
            for column in left.max(i32::from(area.left()))..=right.min(i32::from(area.right()) - 1)
            {
                let mask = self.hull.coverage(column, row);
                if mask == 0 {
                    continue;
                }
                let strength = self.strength(column, row);
                if strength <= 0.0 {
                    continue;
                }
                let position = Position::new(column as u16, row as u16);
                let Some(cell) = buffer.cell_mut(position) else {
                    continue;
                };
                let under = under(cell, host_bg);
                match self.fill {
                    CursorTrailFill::Tint => {
                        let alpha = mask.count_ones() as f32 / 64.0 * strength * TINT_MAX;
                        tint(cell, under, trail, trail_rgb, alpha);
                    }
                    CursorTrailFill::Solid => {
                        let color = match (trail_rgb, under) {
                            (Some(trail), Some(under)) if strength < 1.0 => {
                                let (r, g, b) = mix(under, trail, strength);
                                RColor::Rgb(r, g, b)
                            }
                            _ if strength < SOLID_THRESHOLD && trail_rgb.is_none() => continue,
                            _ => trail,
                        };
                        solid(cell, mask, under, color, self.sextants);
                    }
                }
            }
        }
    }
}

fn tint(
    cell: &mut Cell,
    under: Option<(u8, u8, u8)>,
    trail: RColor,
    trail_rgb: Option<(u8, u8, u8)>,
    alpha: f32,
) {
    match (trail_rgb, under) {
        (Some(trail), Some(under)) => {
            let (r, g, b) = mix(under, trail, alpha);
            cell.bg = RColor::Rgb(r, g, b);
        }
        _ if alpha >= SOLID_THRESHOLD => cell.bg = trail,
        _ => {}
    }
}

fn solid(cell: &mut Cell, mask: u64, under: Option<(u8, u8, u8)>, color: RColor, sextants: bool) {
    let under_color = under.map(|(r, g, b)| RColor::Rgb(r, g, b));
    if !single_width(cell) {
        // A wide glyph or its second half: replacing the symbol would tear it, so only the
        // background takes the smear.
        if mask.count_ones() >= 32 {
            cell.bg = color;
        }
        return;
    }
    match best_match(mask, under_color.is_some(), sextants) {
        Match::Empty => {}
        Match::Full => match under_color {
            // Inverse video, as a block cursor shows the character under it.
            Some(under) => {
                cell.modifier.remove(Modifier::REVERSED);
                cell.fg = under;
                cell.bg = color;
            }
            None => {
                cell.set_char('█');
                cell.modifier = Modifier::empty();
                cell.fg = color;
            }
        },
        Match::Glyph { symbol, inverted } => {
            cell.set_char(symbol);
            cell.modifier = Modifier::empty();
            match (inverted, under_color) {
                (true, Some(under)) => {
                    cell.fg = under;
                    cell.bg = color;
                }
                _ => cell.fg = color,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn rect(left: f32, top: f32, right: f32, bottom: f32) -> Hull {
        Hull::of(&[
            Vec2::new(left, top),
            Vec2::new(right, top),
            Vec2::new(right, bottom),
            Vec2::new(left, bottom),
        ])
    }

    #[test]
    fn coverage_is_matched_to_the_nearest_block_glyph() {
        let glyph = |mask, inverted| best_match(mask, inverted, false);
        assert_eq!(glyph(0, true), Match::Empty);
        assert_eq!(glyph(FULL, true), Match::Full);
        assert_eq!(
            glyph(rows(4..8), true),
            Match::Glyph {
                symbol: '▄',
                inverted: false
            }
        );
        assert_eq!(
            glyph(columns(0..3), true),
            Match::Glyph {
                symbol: '▍',
                inverted: false
            }
        );
        // The top three quarters is the lower quarter, inverted.
        assert_eq!(
            glyph(rows(0..6), true),
            Match::Glyph {
                symbol: '▂',
                inverted: true
            }
        );
        // Without a known background there is nothing to invert with.
        assert!(!matches!(
            glyph(rows(0..6), false),
            Match::Glyph { inverted: true, .. }
        ));
        assert_eq!(
            glyph(block(0..4, 0..4), true),
            Match::Glyph {
                symbol: '▘',
                inverted: false
            }
        );
    }

    #[test]
    fn sextants_are_used_only_when_asked_for() {
        let mask = block(0..4, 0..3);
        assert_eq!(
            best_match(mask, true, true),
            Match::Glyph {
                symbol: '\u{1FB00}',
                inverted: false
            }
        );
        assert!(matches!(
            best_match(mask, true, false),
            Match::Glyph { symbol, .. } if symbol != '\u{1FB00}'
        ));
    }

    #[test]
    fn the_hull_of_twisted_corners_is_still_the_smear() {
        // Corners out of order, as a spring can leave them.
        let hull = Hull::of(&[
            Vec2::new(4.0, 0.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(4.0, 1.0),
            Vec2::new(0.0, 1.0),
        ]);
        assert!(hull.contains(Vec2::new(2.0, 0.5)));
        assert!(!hull.contains(Vec2::new(4.5, 0.5)));
        assert_eq!(hull.coverage(1, 0), FULL);
    }

    fn paint(fill: CursorTrailFill, hull: Hull) -> Buffer {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 3));
        for cell in buffer.content.iter_mut() {
            cell.set_char('x');
            cell.bg = RColor::Rgb(0, 0, 0);
            cell.fg = RColor::Rgb(200, 200, 200);
        }
        SmearPaint {
            hull,
            head: Vec2::new(9.5, 1.5),
            tail: Vec2::new(2.0, 1.5),
            fill,
            color: Color::Rgb(255, 0, 0),
            opacity: 1.0,
            taper: 0.0,
            sextants: false,
        }
        .paint(&mut buffer, None);
        buffer
    }

    #[test]
    fn a_solid_smear_inverts_covered_cells_and_draws_its_edges_in_blocks() {
        // From the middle of column 2 to the end of column 9, the lower half of row 1 and all of
        // row 1: a horizontal smear one row tall with a half-cell left edge.
        let buffer = paint(CursorTrailFill::Solid, rect(2.5, 1.0, 10.0, 2.0));
        let red = RColor::Rgb(255, 0, 0);
        let covered = &buffer[(5, 1)];
        assert_eq!(covered.symbol(), "x", "a covered cell keeps its text");
        assert_eq!((covered.fg, covered.bg), (RColor::Rgb(0, 0, 0), red));
        let edge = &buffer[(2, 1)];
        assert_eq!(edge.symbol(), "▐", "the half-covered edge");
        assert_eq!(edge.fg, red);
        assert_eq!(
            buffer[(5, 0)].symbol(),
            "x",
            "rows outside the smear are untouched"
        );
        assert_eq!(buffer[(10, 1)].symbol(), "x");
    }

    #[test]
    fn a_tint_keeps_every_symbol() {
        let buffer = paint(CursorTrailFill::Tint, rect(2.5, 1.0, 10.0, 2.0));
        for column in 0..12 {
            assert_eq!(buffer[(column, 1)].symbol(), "x");
        }
        assert!(matches!(buffer[(5, 1)].bg, RColor::Rgb(r, 0, 0) if r > 150));
        assert!(matches!(buffer[(2, 1)].bg, RColor::Rgb(r, 0, 0) if r < 150 && r > 0));
    }

    #[test]
    fn a_bar_smear_ends_as_a_bar() {
        let buffer = paint(CursorTrailFill::Solid, rect(4.0, 1.0, 4.125, 2.0));
        assert_eq!(buffer[(4, 1)].symbol(), "▏");
    }

    #[test]
    fn taper_fades_the_tail() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 3));
        for cell in buffer.content.iter_mut() {
            cell.bg = RColor::Rgb(0, 0, 0);
        }
        SmearPaint {
            hull: rect(2.0, 1.0, 10.0, 2.0),
            head: Vec2::new(9.5, 1.5),
            tail: Vec2::new(2.0, 1.5),
            fill: CursorTrailFill::Solid,
            color: Color::Rgb(255, 0, 0),
            opacity: 1.0,
            taper: 1.0,
            sextants: false,
        }
        .paint(&mut buffer, None);
        let red = |x: u16| rgb(buffer[(x, 1)].bg).map_or(0, |(r, _, _)| r);
        assert!(
            red(2) < red(5) && red(5) < red(9),
            "{} {} {}",
            red(2),
            red(5),
            red(9)
        );
    }
}
