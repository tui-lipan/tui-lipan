//! Drawing the smear into cells.
//!
//! The smear is a convex shape in cell coordinates. Each cell it touches is sampled on a 16x16
//! grid, which gives how much of each of its 8x8 samples is covered. A cell is then drawn as the
//! glyph and pair of shades that reproduce that coverage best: the glyph's painted part in one
//! shade between the cell's background and the trail color, the rest in another. That is
//! anti-aliasing with two colors per cell, so edges are smooth at any angle rather than stepped.
//!
//! The glyphs come in sets (see [`GlyphSet`]): Block Elements every terminal draws, and the
//! sextants, wedges, triangles, and octants of Symbols for Legacy Computing, whose slopes follow a
//! diagonal far better, for terminals that draw them.
//!
//! A cell the smear covers evenly keeps its character and takes the smear in its colors, turning to
//! inverse video as it is covered, the way a block cursor shows the character under it.

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Position;
use ratatui::style::{Color as RColor, Modifier};

use super::{CursorTrailFill, Vec2};
use crate::style::Color;

/// Glyph masks are 8x8 samples per cell.
const SAMPLES: u32 = 8;
/// Each sample is itself sampled 2x2, so a sample's coverage is 0 to 4 quarters and a smear as thin
/// as a bar caret still registers in every cell it crosses.
const SUB: u32 = 2;
const FULL: u64 = u64::MAX;
/// A tint mixes at most this much trail color into the background, so text stays readable.
const TINT_MAX: f32 = 0.75;
/// Without RGB to blend, a cell at least this strong takes the trail color outright.
const SOLID_THRESHOLD: f32 = 0.45;
/// How much better than a plain tint (in squared samples) a glyph must fit before it replaces the
/// cell's character.
const GLYPH_GAIN: f32 = 0.75;
/// A glyph replaces a character where it draws a crisp edge, one part of it at least this much
/// stronger than the other, across at least [`TEXT_COVERAGE`] of the cell. See [`replaces_text`].
const TEXT_CONTRAST: f32 = 0.6;
/// See [`TEXT_CONTRAST`].
const TEXT_COVERAGE: f32 = 0.15;
/// Where the smear covers less of a cell, a glyph replaces its character only if it is drawn in
/// at least this much of the trail color over the background: a caret's bar. See
/// [`replaces_text`].
const CRISP: f32 = 0.9;
/// A cell the smear covers less than this is left alone.
const MIN_ALPHA: f32 = 0.02;
/// Either part of a glyph fainter than this is drawn as the background itself, and on a blank
/// cell an even tint fainter than [`EVEN_MIN`] is not used at all: a faint shade across a whole
/// cell shows its square outline, a halo of boxes around the smear, where the same coverage drawn
/// into the glyph alone keeps the outline smooth.
const FAINT_SHADE: f32 = 0.2;
/// See [`FAINT_SHADE`].
const EVEN_MIN: f32 = 0.5;
/// Without RGB to blend, a cell the smear touches with more samples than this always gets a
/// glyph, so the smear never breaks into pieces. Fewer are a corner it clips.
const EMPTY_MAX: u32 = 4;

/// The most vertices a hull keeps: the smear's corners at two moments.
const MAX_VERTICES: usize = 8;

/// A convex polygon, its vertices in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hull {
    points: [Vec2; MAX_VERTICES],
    len: usize,
}

impl Hull {
    /// The convex hull of up to eight points: the smear's corners, even when spring motion has
    /// twisted them out of order, or its corners at two moments, which is the shape it swept
    /// between them.
    pub(crate) fn of(corners: &[Vec2]) -> Self {
        let mut points: Vec<Vec2> = corners.iter().copied().take(MAX_VERTICES).collect();
        points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        // Andrew's monotone chain: a lower and an upper chain, each without its last point.
        let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).cross(b - o);
        let chain = |points: &mut dyn Iterator<Item = Vec2>| {
            let mut chain: Vec<Vec2> = Vec::with_capacity(MAX_VERTICES);
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
        let mut out = [Vec2::ZERO; MAX_VERTICES];
        let len = hull.len().min(MAX_VERTICES);
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

    /// Whether the two hulls touch: no edge of either separates them.
    pub(crate) fn overlaps(&self, other: &Hull) -> bool {
        let separates = |axis: Vec2| {
            let span = |hull: &Hull| {
                hull.vertices()
                    .iter()
                    .map(|point| point.dot(axis))
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), value| {
                        (low.min(value), high.max(value))
                    })
            };
            let (a, b) = (span(self), span(other));
            a.1 < b.0 - 1e-4 || b.1 < a.0 - 1e-4
        };
        let edges = |hull: &Hull| {
            let vertices = hull.vertices();
            (0..vertices.len())
                .map(|index| vertices[(index + 1) % vertices.len()] - vertices[index])
                .collect::<Vec<_>>()
        };
        if self.len == 0 || other.len == 0 {
            return false;
        }
        edges(self)
            .into_iter()
            .chain(edges(other))
            .filter(|edge| edge.dot(*edge) > f32::EPSILON)
            // An edge's normal, and the edge itself for a hull that is only a segment.
            .flat_map(|edge| [Vec2::new(-edge.y, edge.x), edge])
            .all(|axis| !separates(axis))
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

    /// Whether the cell lies wholly inside the hull, wholly outside it, or across its edge.
    fn classify(&self, column: i32, row: i32) -> Overlap {
        let vertices = self.vertices();
        if vertices.len() < 3 {
            return Overlap::Edge;
        }
        let (x, y) = (column as f32, row as f32);
        let corners = [
            Vec2::new(x, y),
            Vec2::new(x + 1.0, y),
            Vec2::new(x, y + 1.0),
            Vec2::new(x + 1.0, y + 1.0),
        ];
        let mut inside = true;
        for index in 0..vertices.len() {
            let a = vertices[index];
            let b = vertices[(index + 1) % vertices.len()];
            let sides = corners.map(|corner| (b - a).cross(corner - a));
            // Every corner beyond one edge: the edge separates the cell from the hull.
            if sides.iter().all(|side| *side < 0.0) {
                return Overlap::Outside;
            }
            inside &= sides.iter().all(|side| *side >= 0.0);
        }
        if inside {
            Overlap::Inside
        } else {
            Overlap::Edge
        }
    }

    /// How much of each of the cell's 8x8 samples the hull covers. Only a cell across the hull's
    /// edge is sampled; the rest are wholly in or out.
    fn coverage(&self, column: i32, row: i32) -> Coverage {
        match self.classify(column, row) {
            Overlap::Outside => return Coverage::default(),
            Overlap::Inside => {
                return Coverage {
                    planes: [0, 0, FULL],
                };
            }
            Overlap::Edge => {}
        }
        let pitch = 1.0 / (SAMPLES * SUB) as f32;
        let mut coverage = Coverage::default();
        for sample_row in 0..SAMPLES {
            for sample_column in 0..SAMPLES {
                let mut quarters = 0u32;
                for sub_row in 0..SUB {
                    for sub_column in 0..SUB {
                        let point = Vec2::new(
                            column as f32
                                + ((sample_column * SUB + sub_column) as f32 + 0.5) * pitch,
                            row as f32 + ((sample_row * SUB + sub_row) as f32 + 0.5) * pitch,
                        );
                        quarters += u32::from(self.contains(point));
                    }
                }
                coverage.set(sample_row * SAMPLES + sample_column, quarters);
            }
        }
        coverage
    }
}

/// The cells any of `hulls` touches, as an inclusive range of columns and rows.
fn union_bounds(hulls: &[Hull]) -> Option<(i32, i32, i32, i32)> {
    hulls
        .iter()
        .filter_map(Hull::cell_bounds)
        .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
}

/// How much of each of the cell's 8x8 samples any of `hulls` covers.
fn union_coverage(hulls: &[Hull], column: i32, row: i32) -> Coverage {
    let mut across = [None; 2];
    let mut count = 0;
    for hull in hulls {
        match hull.classify(column, row) {
            Overlap::Inside => {
                return Coverage {
                    planes: [0, 0, FULL],
                };
            }
            Overlap::Outside => {}
            Overlap::Edge if count < across.len() => {
                across[count] = Some(hull);
                count += 1;
            }
            Overlap::Edge => {}
        }
    }
    match across {
        [None, _] => Coverage::default(),
        [Some(hull), None] => hull.coverage(column, row),
        [Some(a), Some(b)] => {
            let pitch = 1.0 / (SAMPLES * SUB) as f32;
            let mut coverage = Coverage::default();
            for sample_row in 0..SAMPLES {
                for sample_column in 0..SAMPLES {
                    let mut quarters = 0u32;
                    for sub_row in 0..SUB {
                        for sub_column in 0..SUB {
                            let point = Vec2::new(
                                column as f32
                                    + ((sample_column * SUB + sub_column) as f32 + 0.5) * pitch,
                                row as f32 + ((sample_row * SUB + sub_row) as f32 + 0.5) * pitch,
                            );
                            quarters += u32::from(a.contains(point) || b.contains(point));
                        }
                    }
                    coverage.set(sample_row * SAMPLES + sample_column, quarters);
                }
            }
            coverage
        }
    }
}

/// Where a cell lies against the hull.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Overlap {
    Outside,
    Inside,
    Edge,
}

/// How much of each of a cell's 64 samples is covered, 0 to 4 quarters, as three bit-planes, so
/// the coverage under any glyph mask is three population counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Coverage {
    planes: [u64; 3],
}

impl Coverage {
    fn set(&mut self, sample: u32, quarters: u32) {
        for (bit, plane) in self.planes.iter_mut().enumerate() {
            if quarters & (1 << bit) != 0 {
                *plane |= 1 << sample;
            }
        }
    }

    /// Quarters covered under `mask`.
    fn within(&self, mask: u64) -> u32 {
        (self.planes[0] & mask).count_ones()
            + 2 * (self.planes[1] & mask).count_ones()
            + 4 * (self.planes[2] & mask).count_ones()
    }

    fn total(&self) -> u32 {
        self.within(FULL)
    }

    /// Samples at least half covered, for drawing without colors to blend.
    fn mostly(&self) -> u64 {
        self.planes[1] | self.planes[2]
    }
}

/// Which glyphs a smear may draw with. Each set adds to the one before.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum GlyphSet {
    /// Block Elements: halves, eighths, quadrants. Every terminal draws them.
    Blocks,
    /// Also the sextants, wedges, and triangles of Symbols for Legacy Computing (Unicode 13).
    Sextants,
    /// Also its octants (Unicode 16) and the geometric half-cell triangles.
    Octants,
}

/// A glyph and the samples it paints.
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

/// The samples whose centres `inside` accepts, in cell units (0..1 across and down).
fn mask_where(inside: impl Fn(f32, f32) -> bool) -> u64 {
    let mut mask = 0;
    for sample_row in 0..SAMPLES {
        for sample_column in 0..SAMPLES {
            let x = (sample_column as f32 + 0.5) / SAMPLES as f32;
            let y = (sample_row as f32 + 0.5) / SAMPLES as f32;
            if inside(x, y) {
                mask |= 1 << (sample_row * SAMPLES + sample_column);
            }
        }
    }
    mask
}

/// Inside the triangle `a`, `b`, `c` (cell units), edges included.
fn in_triangle(x: f32, y: f32, [a, b, c]: [(f32, f32); 3]) -> bool {
    let side =
        |(x1, y1): (f32, f32), (x2, y2): (f32, f32)| (x2 - x1) * (y - y1) - (y2 - y1) * (x - x1);
    let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

/// Block Elements: every terminal font with box drawing has them, and most terminals draw them.
fn block_glyphs() -> Vec<Glyph> {
    let mut glyphs = Vec::new();
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
    let quadrant = |tl: bool, tr: bool, bl: bool, br: bool| {
        let mut mask = 0;
        for (on, columns, rows) in [
            (tl, 0..4, 0..4),
            (tr, 4..8, 0..4),
            (bl, 0..4, 4..8),
            (br, 4..8, 4..8),
        ] {
            if on {
                mask |= block(columns, rows);
            }
        }
        mask
    };
    for (symbol, mask) in [
        ('▀', rows(0..4)),
        ('▔', rows(0..1)),
        ('▐', columns(4..8)),
        ('▕', columns(7..8)),
        ('▘', quadrant(true, false, false, false)),
        ('▝', quadrant(false, true, false, false)),
        ('▖', quadrant(false, false, true, false)),
        ('▗', quadrant(false, false, false, true)),
        ('▚', quadrant(true, false, false, true)),
        ('▞', quadrant(false, true, true, false)),
        ('▙', quadrant(true, false, true, true)),
        ('▛', quadrant(true, true, true, false)),
        ('▜', quadrant(true, true, false, true)),
        ('▟', quadrant(false, true, true, true)),
    ] {
        glyphs.push(Glyph { symbol, mask });
    }
    glyphs
}

/// The 2x3 sextants (U+1FB00), the wedges between points of their lattice (U+1FB3C), and the
/// triangular quarter and three-quarter blocks (U+1FB68).
fn sextant_glyphs() -> Vec<Glyph> {
    use super::legacy_glyphs::{Corner, WEDGES};
    // Thirds of eight sample rows, as near as eight allows.
    let thirds = [0..3, 3..5, 5..8];
    let mut glyphs: Vec<Glyph> = (1u32..63)
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
        .collect();
    let lattice = |(x, y): (u8, u8)| (f32::from(x) / 2.0, f32::from(y) / 3.0);
    for &(symbol, corner, from, to) in WEDGES {
        let (a, b) = (lattice(from), lattice(to));
        let corner = match corner {
            Corner::UpperLeft => (0.0, 0.0),
            Corner::UpperRight => (1.0, 0.0),
            Corner::LowerLeft => (0.0, 1.0),
            Corner::LowerRight => (1.0, 1.0),
        };
        let side = |(x, y): (f32, f32)| (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
        let wanted = side(corner).signum();
        let mask = mask_where(|x, y| {
            let here = side((x, y));
            here == 0.0 || here.signum() == wanted
        });
        glyphs.push(Glyph { symbol, mask });
    }
    let centre = (0.5, 0.5);
    let quarters = [
        ('\u{1FB6C}', [(0.0, 0.0), (0.0, 1.0), centre]),
        ('\u{1FB6D}', [(0.0, 0.0), (1.0, 0.0), centre]),
        ('\u{1FB6E}', [(1.0, 0.0), (1.0, 1.0), centre]),
        ('\u{1FB6F}', [(0.0, 1.0), (1.0, 1.0), centre]),
    ];
    for (symbol, triangle) in quarters {
        glyphs.push(Glyph {
            symbol,
            mask: mask_where(|x, y| in_triangle(x, y, triangle)),
        });
    }
    // Three quarters: everything but the left, upper, right, and lower quarter, in that order.
    for (symbol, (_, triangle)) in ['\u{1FB68}', '\u{1FB69}', '\u{1FB6A}', '\u{1FB6B}']
        .into_iter()
        .zip(quarters)
    {
        glyphs.push(Glyph {
            symbol,
            mask: !mask_where(|x, y| in_triangle(x, y, triangle)),
        });
    }
    glyphs
}

/// The 2x4 octants (U+1CD00) and the half-cell triangles (U+25E2).
fn octant_glyphs() -> Vec<Glyph> {
    use super::legacy_glyphs::OCTANTS;
    let mut glyphs: Vec<Glyph> = OCTANTS
        .iter()
        .map(|&(symbol, eighths)| {
            let mut mask = 0;
            for octant in 0..8u32 {
                if eighths & (1 << octant) != 0 {
                    let column = if octant % 2 == 0 { 0..4 } else { 4..8 };
                    let row = octant / 2 * 2;
                    mask |= block(column, row..row + 2);
                }
            }
            Glyph { symbol, mask }
        })
        .collect();
    for (symbol, triangle) in [
        ('◢', [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        ('◣', [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0)]),
        ('◤', [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]),
        ('◥', [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]),
    ] {
        glyphs.push(Glyph {
            symbol,
            mask: mask_where(|x, y| in_triangle(x, y, triangle)),
        });
    }
    glyphs
}

/// The glyphs of `set`, built once.
fn glyphs(set: GlyphSet) -> &'static [Glyph] {
    static BLOCKS: std::sync::OnceLock<Vec<Glyph>> = std::sync::OnceLock::new();
    static SEXTANTS: std::sync::OnceLock<Vec<Glyph>> = std::sync::OnceLock::new();
    static OCTANTS: std::sync::OnceLock<Vec<Glyph>> = std::sync::OnceLock::new();
    match set {
        GlyphSet::Blocks => BLOCKS.get_or_init(block_glyphs),
        GlyphSet::Sextants => SEXTANTS.get_or_init(|| {
            let mut glyphs = block_glyphs();
            glyphs.extend(sextant_glyphs());
            glyphs
        }),
        GlyphSet::Octants => OCTANTS.get_or_init(|| {
            let mut glyphs = block_glyphs();
            glyphs.extend(sextant_glyphs());
            glyphs.extend(octant_glyphs());
            glyphs
        }),
    }
}

/// How a cell's coverage is best drawn when its colors can be blended.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fit {
    /// One shade across the cell: its character stays, tinted `alpha` of the way to the trail.
    Even { alpha: f32 },
    /// `symbol` with its painted part `fg` and the rest `bg` of the way to the trail color.
    Glyph { symbol: char, fg: f32, bg: f32 },
}

/// The glyph and two shades that reproduce `coverage` best, in the least-squares sense: for each
/// glyph the best foreground shade is the mean coverage under it and the best background shade the
/// mean of the rest, so the glyph whose two means explain the most coverage wins. A glyph has to
/// beat an even tint by [`GLYPH_GAIN`] to replace the character. Faint shades are kept inside the
/// glyph, and on a `blank` cell out of an even tint (see [`FAINT_SHADE`]).
fn fit(coverage: Coverage, set: GlyphSet, blank: bool) -> Fit {
    let total = coverage.total() as f32 / 4.0;
    let even = Fit::Even {
        alpha: total / 64.0,
    };
    let mut best = if blank && total / 64.0 < EVEN_MIN {
        (f32::NEG_INFINITY, even)
    } else {
        (total * total / 64.0 + GLYPH_GAIN, even)
    };
    for glyph in glyphs(set) {
        let painted = glyph.mask.count_ones() as f32;
        let inside = coverage.within(glyph.mask) as f32 / 4.0;
        let outside = total - inside;
        // Each part explains its mean times its sum of the coverage; a part drawn as the
        // background explains nothing.
        let shade = |mean: f32| if mean < FAINT_SHADE { 0.0 } else { mean };
        let fg = shade(inside / painted);
        let bg = shade(outside / (64.0 - painted));
        let score = inside * fg + outside * bg;
        // A glyph and its complement fit alike; the one whose painted part is the covered part
        // wins, so the trail color is the glyph's own.
        let leads = inside / painted >= outside / (64.0 - painted);
        let better = score > best.0 + 1e-3
            || (score > best.0 - 1e-3
                && leads
                && matches!(best.1, Fit::Glyph { fg, bg, .. } if fg < bg));
        if better {
            best = (
                score,
                Fit::Glyph {
                    symbol: glyph.symbol,
                    fg,
                    bg,
                },
            );
        }
    }
    best.1
}

/// How a cell's coverage is drawn when its colors cannot be blended.
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

fn best_match(mask: u64, allow_inverted: bool, set: GlyphSet) -> Match {
    if mask.count_ones() <= EMPTY_MAX {
        return Match::Empty;
    }
    // Leaving a touched cell empty is never the answer: that is what breaks a smear into pieces.
    let mut best = (u32::MAX, Match::Empty);
    let mut consider = |cost: u32, candidate: Match| {
        if cost < best.0 {
            best = (cost, candidate);
        }
    };
    consider((!mask).count_ones(), Match::Full);
    // Plain glyphs first: an inverted one only wins by matching strictly better, so a tie goes
    // to the glyph that also reads right where colors are approximated.
    for glyph in glyphs(set) {
        consider(
            (mask ^ glyph.mask).count_ones(),
            Match::Glyph {
                symbol: glyph.symbol,
                inverted: false,
            },
        );
    }
    if allow_inverted {
        for glyph in glyphs(set) {
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
    /// What joins the smear to where the last frame showed it, when it has left that behind.
    pub(crate) bridge: Option<Hull>,
    /// Where the smear is strongest (the caret) and where it fades from, for `taper`.
    pub(crate) head: Vec2,
    pub(crate) tail: Vec2,
    pub(crate) fill: CursorTrailFill,
    pub(crate) color: Color,
    pub(crate) opacity: f32,
    pub(crate) taper: f32,
    pub(crate) glyphs: GlyphSet,
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

fn blend(from: (u8, u8, u8), to: (u8, u8, u8), amount: f32) -> RColor {
    let (r, g, b) = mix(from, to, amount);
    RColor::Rgb(r, g, b)
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

    fn pieces(&self) -> ([Hull; 2], usize) {
        match self.bridge {
            Some(bridge) => ([self.hull, bridge], 2),
            None => ([self.hull, self.hull], 1),
        }
    }

    /// Whether the smear covers the centre of the cell at `position`.
    pub(crate) fn covers(&self, position: Position) -> bool {
        let (pieces, count) = self.pieces();
        let centre = Vec2::new(f32::from(position.x) + 0.5, f32::from(position.y) + 0.5);
        pieces[..count].iter().any(|hull| hull.contains(centre))
    }

    pub(crate) fn paint(&self, buffer: &mut Buffer, host_bg: Option<(u8, u8, u8)>) {
        let (pieces, count) = self.pieces();
        let pieces = &pieces[..count];
        let Some((left, top, right, bottom)) = union_bounds(pieces) else {
            return;
        };
        let area = buffer.area;
        let trail = crate::backend::ratatui_backend::common::to_ratatui_color(self.color);
        let trail_rgb = rgb(trail);
        for row in top.max(i32::from(area.top()))..=bottom.min(i32::from(area.bottom()) - 1) {
            for column in left.max(i32::from(area.left()))..=right.min(i32::from(area.right()) - 1)
            {
                let coverage = union_coverage(pieces, column, row);
                if coverage.total() == 0 {
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
                match (self.fill, trail_rgb, under) {
                    // There is no character to keep in a blank cell, so a tint draws its edges as
                    // smoothly as a solid smear does, only fainter.
                    (CursorTrailFill::Tint, Some(trail), Some(under))
                        if single_width(cell) && cell.symbol().trim().is_empty() =>
                    {
                        smooth(
                            cell,
                            coverage,
                            under,
                            trail,
                            strength * TINT_MAX,
                            self.glyphs,
                        );
                    }
                    (CursorTrailFill::Tint, _, _) => {
                        let alpha = coverage.total() as f32 / 256.0 * strength * TINT_MAX;
                        tint(cell, under, trail, trail_rgb, alpha);
                    }
                    (CursorTrailFill::Solid, Some(trail), Some(under)) => {
                        smooth(cell, coverage, under, trail, strength, self.glyphs);
                    }
                    (CursorTrailFill::Solid, _, _) => {
                        if strength < SOLID_THRESHOLD && trail_rgb.is_none() {
                            continue;
                        }
                        let color = match (trail_rgb, under) {
                            (Some(trail_rgb), Some(under)) if strength < 1.0 => {
                                blend(under, trail_rgb, strength)
                            }
                            _ => trail,
                        };
                        blocky(cell, coverage.mostly(), under, color, self.glyphs);
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
        (Some(trail), Some(under)) => cell.bg = blend(under, trail, alpha),
        _ if alpha >= SOLID_THRESHOLD => cell.bg = trail,
        _ => {}
    }
}

/// Whether a glyph fit with shades `fg` and `bg` replaces the character of a cell the smear covers
/// `alpha` of: where it draws the smear's outline across a fair part of the cell, or a crisp shape
/// such as a caret's bar. An even tint across a partly covered cell shows as a box, so the smear's
/// outline is drawn over the character instead. Where the smear merely clips a cell, though, the
/// glyph would erase the character, a letter or a border line, for a sliver of trail, so the
/// character stays and takes a faint tint.
fn replaces_text(alpha: f32, fg: f32, bg: f32) -> bool {
    (alpha >= TEXT_COVERAGE && (fg - bg).abs() >= TEXT_CONTRAST)
        || (fg >= CRISP && bg <= 1.0 - CRISP)
}

/// A solid smear where colors blend: the best-fitting glyph and shades, so edges are smooth.
fn smooth(
    cell: &mut Cell,
    coverage: Coverage,
    under: (u8, u8, u8),
    trail: (u8, u8, u8),
    strength: f32,
    set: GlyphSet,
) {
    let fit = if single_width(cell) {
        let blank = cell.symbol().trim().is_empty();
        let alpha = coverage.total() as f32 / 256.0;
        match fit(coverage, set, blank) {
            Fit::Glyph { fg, bg, .. } if !blank && !replaces_text(alpha, fg, bg) => {
                Fit::Even { alpha }
            }
            fit => fit,
        }
    } else {
        // A wide glyph or its second half: replacing the symbol would tear it, so only its colors
        // take the smear.
        Fit::Even {
            alpha: coverage.total() as f32 / 256.0,
        }
    };
    match fit {
        Fit::Even { alpha } => {
            let alpha = alpha * strength;
            if alpha < MIN_ALPHA {
                return;
            }
            // The text goes to inverse video as the smear covers it, the way a block cursor shows
            // the character under it.
            cell.modifier.remove(Modifier::REVERSED);
            cell.bg = blend(under, trail, alpha);
            match rgb(cell.fg) {
                Some(fg) => cell.fg = blend(fg, under, alpha),
                None if alpha >= 0.5 => cell.fg = RColor::Rgb(under.0, under.1, under.2),
                None => {}
            }
        }
        Fit::Glyph { symbol, fg, bg } => {
            cell.set_char(symbol);
            cell.modifier = Modifier::empty();
            cell.fg = blend(under, trail, fg * strength);
            cell.bg = blend(under, trail, bg * strength);
        }
    }
}

/// A solid smear where colors cannot blend: the nearest glyph, in the trail color or inverted.
fn blocky(cell: &mut Cell, mask: u64, under: Option<(u8, u8, u8)>, color: RColor, set: GlyphSet) {
    let under_color = under.map(|(r, g, b)| RColor::Rgb(r, g, b));
    if !single_width(cell) {
        if mask.count_ones() >= 32 {
            cell.bg = color;
        }
        return;
    }
    match best_match(mask, under_color.is_some(), set) {
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

    fn glyph(set: GlyphSet, symbol: char) -> u64 {
        glyphs(set)
            .iter()
            .find(|glyph| glyph.symbol == symbol)
            .unwrap_or_else(|| panic!("{symbol:?} in {set:?}"))
            .mask
    }

    fn sample(x: f32, y: f32) -> u64 {
        1 << ((y * 8.0) as u32 * SAMPLES + (x * 8.0) as u32)
    }

    /// The generated shapes say what their Unicode names say.
    #[test]
    fn legacy_glyphs_have_the_shapes_their_names_give() {
        // LOWER LEFT BLOCK DIAGONAL LOWER MIDDLE LEFT TO LOWER RIGHT: below the line from two
        // thirds down the left edge to the bottom-right corner.
        let wedge = glyph(GlyphSet::Sextants, '\u{1FB3D}');
        assert_ne!(wedge & sample(0.1, 0.9), 0);
        assert_eq!(wedge & sample(0.1, 0.4), 0);
        assert_eq!(wedge & sample(0.9, 0.5), 0);
        // UPPER RIGHT BLOCK DIAGONAL UPPER MIDDLE LEFT TO LOWER MIDDLE RIGHT: above a shallow line.
        let wedge = glyph(GlyphSet::Sextants, '\u{1FB67}');
        assert_ne!(wedge & sample(0.9, 0.1), 0);
        assert_eq!(wedge & sample(0.1, 0.9), 0);
        // BLOCK OCTANT-3: the left half of the second of four rows.
        assert_eq!(glyph(GlyphSet::Octants, '\u{1CD00}'), block(0..4, 2..4));
        // The half-cell triangle.
        let triangle = glyph(GlyphSet::Octants, '◢');
        assert_ne!(triangle & sample(0.9, 0.9), 0);
        assert_eq!(triangle & sample(0.1, 0.1), 0);
        // Richer sets keep everything the plainer ones have.
        assert!(glyphs(GlyphSet::Blocks).len() < glyphs(GlyphSet::Sextants).len());
        assert!(glyphs(GlyphSet::Sextants).len() < glyphs(GlyphSet::Octants).len());
    }

    /// Coverage becomes the glyph and two shades that reproduce it best: a half-cell diagonal is
    /// the triangle itself where the terminal draws one, and a block glyph shaded in between where
    /// it does not; a cell the smear only grazes keeps its character and takes a faint tint.
    #[test]
    fn coverage_is_drawn_as_the_best_glyph_and_shades() {
        let diagonal = Hull::of(&[
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ])
        .coverage(0, 0);
        match fit(diagonal, GlyphSet::Octants, true) {
            Fit::Glyph { symbol, fg, bg } => {
                assert_eq!(symbol, '◢');
                assert!(fg > 0.9 && bg < 0.1, "{fg} {bg}");
            }
            other => panic!("{other:?}"),
        }
        match fit(diagonal, GlyphSet::Blocks, true) {
            Fit::Glyph { fg, bg, .. } => {
                assert!(fg > bg && fg > 0.5 && fg < 1.0, "a shaded block: {fg} {bg}")
            }
            other => panic!("{other:?}"),
        }

        let grazed = rect(0.0, 0.0, 1.0, 0.1).coverage(0, 0);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        buffer[(0, 0)].set_char('x');
        buffer[(0, 0)].bg = RColor::Rgb(0, 0, 0);
        buffer[(0, 0)].fg = RColor::Rgb(200, 200, 200);
        smooth(
            &mut buffer[(0, 0)],
            grazed,
            (0, 0, 0),
            (255, 0, 0),
            1.0,
            GlyphSet::Octants,
        );
        let cell = &buffer[(0, 0)];
        assert!(
            cell.symbol() == "x" || cell.symbol() == "▔",
            "{}",
            cell.symbol()
        );
        assert!(matches!(rgb(cell.bg), Some((r, 0, 0)) if r < 128) || cell.symbol() == "▔");
    }

    /// A blank cell the smear only clips keeps the exact background outside the glyph, so the
    /// smear's edge has no halo of faintly tinted squares around it.
    #[test]
    fn a_clipped_blank_cell_keeps_its_background_around_the_glyph() {
        for (set, hull) in [
            (GlyphSet::Octants, rect(0.0, 0.85, 1.0, 1.0)),
            (GlyphSet::Blocks, rect(0.0, 0.85, 1.0, 1.0)),
            (
                GlyphSet::Octants,
                Hull::of(&[
                    Vec2::new(0.6, 1.0),
                    Vec2::new(1.0, 0.7),
                    Vec2::new(1.0, 1.0),
                ]),
            ),
            (
                GlyphSet::Blocks,
                Hull::of(&[
                    Vec2::new(0.6, 1.0),
                    Vec2::new(1.0, 0.7),
                    Vec2::new(1.0, 1.0),
                ]),
            ),
        ] {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
            buffer[(0, 0)].bg = RColor::Rgb(6, 7, 15);
            smooth(
                &mut buffer[(0, 0)],
                hull.coverage(0, 0),
                (6, 7, 15),
                (230, 150, 190),
                1.0,
                set,
            );
            let cell = &buffer[(0, 0)];
            assert_eq!(rgb(cell.bg), Some((6, 7, 15)), "{set:?} {}", cell.symbol());
            assert_ne!(cell.symbol(), " ", "{set:?}");
        }
    }

    /// A character the smear clips keeps its shape: a border line is not swapped for the glyph of a
    /// sliver of trail.
    #[test]
    fn a_clipped_character_keeps_its_shape() {
        let corner = Hull::of(&[
            Vec2::new(0.6, 1.0),
            Vec2::new(1.0, 0.7),
            Vec2::new(1.0, 1.0),
        ]);
        for set in [GlyphSet::Blocks, GlyphSet::Sextants, GlyphSet::Octants] {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
            buffer[(0, 0)].set_char('│');
            buffer[(0, 0)].fg = RColor::Rgb(250, 70, 130);
            buffer[(0, 0)].bg = RColor::Rgb(6, 7, 15);
            smooth(
                &mut buffer[(0, 0)],
                corner.coverage(0, 0),
                (6, 7, 15),
                (230, 150, 190),
                1.0,
                set,
            );
            assert_eq!(buffer[(0, 0)].symbol(), "│", "{set:?}");
        }
    }

    /// Along a diagonal smear's edges, every part of every cell is either the background or a clear
    /// shade of the trail: a faint shade spread over most of a cell would show as a box.
    #[test]
    fn a_diagonal_smear_has_no_faint_boxes_along_its_edges() {
        let under = (6, 7, 15);
        let trail = (230, 150, 190);
        // A block caret's travel up and to the right: the hull of its rect at both ends.
        let points = [
            Vec2::new(2.0, 9.0),
            Vec2::new(3.0, 9.0),
            Vec2::new(3.0, 10.0),
            Vec2::new(2.0, 10.0),
            Vec2::new(22.0, 1.0),
            Vec2::new(23.0, 1.0),
            Vec2::new(23.0, 2.0),
            Vec2::new(22.0, 2.0),
        ];
        for set in [GlyphSet::Blocks, GlyphSet::Sextants, GlyphSet::Octants] {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 26, 12));
            for cell in buffer.content.iter_mut() {
                cell.bg = RColor::Rgb(under.0, under.1, under.2);
            }
            SmearPaint {
                hull: Hull::of(&points),
                bridge: None,
                head: Vec2::new(22.5, 1.5),
                tail: Vec2::new(2.5, 9.5),
                fill: CursorTrailFill::Solid,
                color: Color::Rgb(trail.0, trail.1, trail.2),
                opacity: 1.0,
                taper: 0.0,
                glyphs: set,
            }
            .paint(&mut buffer, None);
            let shade = |color: RColor| {
                let (r, _, _) = rgb(color).expect("an RGB color");
                (f32::from(r) - f32::from(under.0)) / f32::from(trail.0 - under.0)
            };
            let mut painted = 0;
            for (index, cell) in buffer.content.iter().enumerate() {
                let parts = if cell.symbol() == " " {
                    vec![shade(cell.bg)]
                } else {
                    vec![shade(cell.fg), shade(cell.bg)]
                };
                for part in parts {
                    assert!(
                        !(0.01..0.19).contains(&part),
                        "{set:?} cell {index} {:?}: shade {part}",
                        cell.symbol()
                    );
                    if part >= 0.19 {
                        painted += 1;
                    }
                }
            }
            assert!(painted > 40, "{set:?}: the smear is drawn ({painted})");
        }
    }

    /// A diagonal smear, as thick as a block caret or as thin as a bar caret, paints every row it
    /// crosses as one unbroken run of cells, and each row's run touches the next: no gaps.
    #[test]
    fn diagonal_smears_have_no_gaps() {
        for (name, corners) in [
            (
                "block",
                [
                    Vec2::new(2.0, 1.0),
                    Vec2::new(3.0, 1.0),
                    Vec2::new(23.0, 9.0),
                    Vec2::new(22.0, 9.0),
                ],
            ),
            (
                "bar",
                [
                    Vec2::new(2.0, 1.0),
                    Vec2::new(2.125, 1.0),
                    Vec2::new(23.125, 9.0),
                    Vec2::new(23.0, 9.0),
                ],
            ),
            (
                "steep bar",
                [
                    Vec2::new(3.0, 0.0),
                    Vec2::new(3.125, 0.0),
                    Vec2::new(9.125, 10.0),
                    Vec2::new(9.0, 10.0),
                ],
            ),
        ] {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 26, 11));
            for cell in buffer.content.iter_mut() {
                cell.bg = RColor::Rgb(0, 0, 0);
                cell.set_char('.');
            }
            SmearPaint {
                hull: Hull::of(&corners),
                bridge: None,
                head: corners[2],
                tail: corners[0],
                fill: CursorTrailFill::Solid,
                color: Color::Rgb(255, 0, 0),
                opacity: 1.0,
                taper: 0.0,
                glyphs: GlyphSet::Blocks,
            }
            .paint(&mut buffer, None);
            let picture = (0..11)
                .map(|y| (0..26).map(|x| buffer[(x, y)].symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            // Painted: a glyph in place of the character, or the character tinted.
            let painted = |y: u16| -> Vec<u16> {
                (0..26)
                    .filter(|&x| {
                        buffer[(x, y)].symbol() != "." || buffer[(x, y)].bg != RColor::Rgb(0, 0, 0)
                    })
                    .collect()
            };
            let top = corners[0].y as u16;
            let bottom = corners[2].y.ceil() as u16;
            let mut previous: Option<(u16, u16)> = None;
            for y in top..bottom {
                let columns = painted(y);
                assert!(!columns.is_empty(), "{name}: row {y} is empty\n{picture}");
                let (first, last) = (columns[0], *columns.last().unwrap());
                assert_eq!(
                    usize::from(last - first) + 1,
                    columns.len(),
                    "{name}: row {y} has a gap\n{picture}"
                );
                if let Some((before_first, before_last)) = previous {
                    assert!(
                        first <= before_last + 1 && last + 1 >= before_first,
                        "{name}: rows {} and {y} do not touch\n{picture}",
                        y - 1
                    );
                }
                previous = Some((first, last));
            }
        }
    }

    #[test]
    fn coverage_is_matched_to_the_nearest_block_glyph() {
        let glyph = |mask, inverted| best_match(mask, inverted, GlyphSet::Blocks);
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
            best_match(mask, true, GlyphSet::Sextants),
            Match::Glyph {
                symbol: '\u{1FB00}',
                inverted: false
            }
        );
        assert!(matches!(
            best_match(mask, true, GlyphSet::Blocks),
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
        assert_eq!(hull.coverage(1, 0).total(), 256);
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
            bridge: None,
            head: Vec2::new(9.5, 1.5),
            tail: Vec2::new(2.0, 1.5),
            fill,
            color: Color::Rgb(255, 0, 0),
            opacity: 1.0,
            taper: 0.0,
            glyphs: GlyphSet::Blocks,
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

    /// Where there is no text to keep, a tint's edges are as smooth as a solid smear's, only
    /// fainter; text keeps its character.
    #[test]
    fn a_tint_draws_smooth_edges_over_blank_cells() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 3));
        for (index, cell) in buffer.content.iter_mut().enumerate() {
            if index % 12 < 6 {
                cell.set_char('x');
            }
            cell.bg = RColor::Rgb(0, 0, 0);
            cell.fg = RColor::Rgb(200, 200, 200);
        }
        SmearPaint {
            hull: rect(2.5, 1.0, 10.5, 2.0),
            bridge: None,
            head: Vec2::new(10.0, 1.5),
            tail: Vec2::new(2.5, 1.5),
            fill: CursorTrailFill::Tint,
            color: Color::Rgb(255, 0, 0),
            opacity: 1.0,
            taper: 0.0,
            glyphs: GlyphSet::Blocks,
        }
        .paint(&mut buffer, None);
        for column in 0..6 {
            assert_eq!(buffer[(column, 1)].symbol(), "x", "text stays");
        }
        assert!(matches!(buffer[(2, 1)].bg, RColor::Rgb(r, 0, 0) if r > 0 && r < 128));
        let edge = &buffer[(10, 1)];
        assert_eq!(edge.symbol(), "▌", "a blank edge cell is drawn as a glyph");
        assert!(matches!(edge.fg, RColor::Rgb(r, 0, 0) if r > 128 && r < 255));
        assert_eq!(edge.bg, RColor::Rgb(0, 0, 0));
        let covered = &buffer[(8, 1)];
        assert_eq!(covered.symbol(), " ");
        assert!(matches!(covered.bg, RColor::Rgb(r, 0, 0) if r > 128 && r < 255));
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
            bridge: None,
            head: Vec2::new(9.5, 1.5),
            tail: Vec2::new(2.0, 1.5),
            fill: CursorTrailFill::Solid,
            color: Color::Rgb(255, 0, 0),
            opacity: 1.0,
            taper: 1.0,
            glyphs: GlyphSet::Blocks,
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
