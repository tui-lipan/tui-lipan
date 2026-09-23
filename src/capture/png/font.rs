//! Font-backed text for PNG captures.
//!
//! Glyphs are read on demand from system fonts through `ttf-parser` and rasterized one at a time,
//! so a capture pays only for the characters it draws. Characters the preferred monospace fonts
//! lack are found by searching every installed face, which is how CJK text and emoji get drawn.
//! Everything a renderer learns is kept for the life of the process: the first capture pays for
//! the font scan, later ones reuse its choices and rendered glyphs.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use ab_glyph_rasterizer::{Point, Rasterizer, point};
use fontdb::{Database, Family, ID, Query, Source, Style};
use image::{RgbImage, Rgba, RgbaImage};
use ttf_parser::{Face, GlyphId, OutlineBuilder, RasterImageFormat};
use unicode_width::UnicodeWidthChar;

use super::{CellPixels, Rgb8, blend_rgb, bold_offset};
use crate::capture::PngOptions;

/// Monospace families tried in order before any coverage search.
const PREFERRED_FAMILIES: [&str; 9] = [
    "Symbols Nerd Font Mono",
    "JetBrainsMono Nerd Font",
    "JetBrains Mono",
    "FiraCode Nerd Font Mono",
    "Fira Code",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Noto Sans Mono",
    "monospace",
];

/// How many distinct font choices keep a renderer alive at once.
const MAX_RENDERERS: usize = 4;
/// Rendered glyphs kept per renderer before the cache starts over.
const MAX_GLYPHS: usize = 16_384;
/// Scaled color images kept per renderer before the cache starts over.
const MAX_IMAGES: usize = 1_024;

const ZERO_WIDTH_JOINER: char = '\u{200d}';
const EMOJI_PRESENTATION: char = '\u{fe0f}';

/// Renderers by font choice, shared by every capture in the process.
///
/// Encoding holds the lock for the whole frame, so concurrent PNG encodes that use fonts run one
/// at a time.
static RENDERERS: Mutex<Vec<(FontKey, Option<FontRenderer>)>> = Mutex::new(Vec::new());

/// Run `render` with the renderer for `options`' font choice, or with `None` when this system
/// has no usable fonts.
pub(super) fn with_renderer<T>(
    options: &PngOptions,
    render: impl FnOnce(Option<&mut FontRenderer>) -> T,
) -> T {
    let key = FontKey {
        path: options.font_path.clone(),
        family: options.font_family.as_deref().map(str::to_owned),
    };
    let mut renderers = RENDERERS.lock().unwrap_or_else(PoisonError::into_inner);
    let index = match renderers.iter().position(|(cached, _)| *cached == key) {
        Some(index) => index,
        None => {
            if renderers.len() == MAX_RENDERERS {
                renderers.remove(0);
            }
            let renderer = FontRenderer::new(&key);
            renderers.push((key, renderer));
            renderers.len() - 1
        }
    };
    render(renderers[index].1.as_mut())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FontKey {
    path: Option<PathBuf>,
    family: Option<String>,
}

/// Where a character's glyph comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GlyphSource {
    /// A vector outline, drawn in the cell's foreground color.
    Outline(ID, GlyphId),
    /// A color bitmap, as emoji fonts ship them.
    Image(ID, GlyphId),
}

/// What one face has for one character.
#[derive(Clone, Copy, Debug, Default)]
struct Probe {
    outline: Option<GlyphId>,
    image: Option<GlyphId>,
}

impl Probe {
    /// This face as the source of a color image, or of an outline.
    fn source(self, face: ID, image: bool) -> Option<GlyphSource> {
        if image {
            self.image.map(|glyph| GlyphSource::Image(face, glyph))
        } else {
            self.outline.map(|glyph| GlyphSource::Outline(face, glyph))
        }
    }
}

/// A rasterized outline, positioned relative to its origin on the baseline.
struct Coverage {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    advance: f32,
    alpha: Vec<u8>,
}

pub(super) struct FontRenderer {
    db: Database,
    /// Faces tried first, in order: the explicit font file, the requested family, then the
    /// preferred monospace families.
    chain: Vec<ID>,
    /// Faces a coverage search found for characters the chain lacks, tried before searching again.
    found: Vec<ID>,
    /// Every face in the order a coverage search tries them, built on the first search.
    candidates: Option<Vec<ID>>,
    sources: HashMap<(char, bool), Option<GlyphSource>>,
    glyphs: HashMap<(ID, GlyphId, u32), Option<Arc<Coverage>>>,
    images: HashMap<(ID, GlyphId, u32, u32), Option<Arc<RgbaImage>>>,
}

impl FontRenderer {
    fn new(key: &FontKey) -> Option<Self> {
        let mut db = Database::new();
        let mut chain = Vec::new();
        if let Some(bytes) = key.path.as_ref().and_then(|path| std::fs::read(path).ok()) {
            chain.extend(db.load_font_source(Source::Binary(Arc::new(bytes))));
        }
        db.load_system_fonts();
        if db.is_empty() {
            return None;
        }
        let families = key.family.as_deref().into_iter().chain(PREFERRED_FAMILIES);
        for family in families {
            if let Some(id) = query_family(&db, family)
                && !chain.contains(&id)
            {
                chain.push(id);
            }
        }
        Some(Self {
            db,
            chain,
            found: Vec::new(),
            candidates: None,
            sources: HashMap::new(),
            glyphs: HashMap::new(),
            images: HashMap::new(),
        })
    }

    /// Draw `grapheme` into `cell_rect`, returning whether a font could draw it.
    pub(super) fn draw(
        &mut self,
        image: &mut RgbImage,
        cell_rect: CellPixels,
        grapheme: &str,
        color: Rgb8,
        bold: bool,
    ) -> bool {
        if cell_rect.width == 0 || cell_rect.height == 0 {
            return false;
        }
        let mut chars = grapheme.chars();
        let Some(base) = chars.next() else {
            return false;
        };
        let prefer_image =
            grapheme.contains(EMOJI_PRESENTATION) || matches!(base, '\u{1f000}'..='\u{1faff}');
        let Some(source) = self.source(base, prefer_image) else {
            return false;
        };
        let size = font_size(cell_rect);
        let baseline = cell_rect.y0 as i32 + (cell_rect.height as f32 * 0.78).round() as i32;
        let (face, glyph) = match source {
            GlyphSource::Image(face, glyph) => {
                return self.draw_image(image, cell_rect, face, glyph);
            }
            GlyphSource::Outline(face, glyph) => (face, glyph),
        };
        let Some(coverage) = self.outline(face, glyph, size) else {
            return false;
        };
        let origin = cell_rect.x0 as i32
            + ((cell_rect.width as f32 - coverage.advance).max(0.0) / 2.0).round() as i32;
        let bold_offset = bold.then(|| bold_offset(cell_rect).max(1) as i32);
        blit(
            image,
            cell_rect,
            &coverage,
            origin,
            baseline,
            color,
            bold_offset,
        );

        // Fonts place a combining mark after the base glyph's advance, reaching back over it.
        let pen = origin + coverage.advance.round() as i32;
        for mark in chars {
            if mark == ZERO_WIDTH_JOINER {
                // Joining the rest of a sequence needs shaping; the first component stands in.
                break;
            }
            if is_variation_selector(mark) || mark.width() != Some(0) {
                continue;
            }
            let in_base_face = self.probe(face, mark).outline.map(|g| (face, g));
            let Some((mark_face, mark_glyph)) =
                in_base_face.or_else(|| match self.source(mark, false)? {
                    GlyphSource::Outline(face, glyph) => Some((face, glyph)),
                    GlyphSource::Image(..) => None,
                })
            else {
                continue;
            };
            if let Some(mark_coverage) = self.outline(mark_face, mark_glyph, size) {
                blit(
                    image,
                    cell_rect,
                    &mark_coverage,
                    pen,
                    baseline,
                    color,
                    bold_offset,
                );
            }
        }
        true
    }

    fn draw_image(
        &mut self,
        image: &mut RgbImage,
        cell_rect: CellPixels,
        face: ID,
        glyph: GlyphId,
    ) -> bool {
        let Some(picture) = self.image(face, glyph, cell_rect.width, cell_rect.height) else {
            return false;
        };
        let x0 = cell_rect.x0 + cell_rect.width.saturating_sub(picture.width()) / 2;
        let y0 = cell_rect.y0 + cell_rect.height.saturating_sub(picture.height()) / 2;
        for (x, y, pixel) in picture.enumerate_pixels() {
            let [r, g, b, a] = pixel.0;
            if a > 0 {
                blend_rgb(image, x0 + x, y0 + y, (r, g, b), a);
            }
        }
        true
    }

    /// Which face draws `ch`, searching every installed face when the known ones cannot.
    fn source(&mut self, ch: char, prefer_image: bool) -> Option<GlyphSource> {
        if let Some(source) = self.sources.get(&(ch, prefer_image)) {
            return *source;
        }
        let source = self.find_source(ch, prefer_image);
        self.sources.insert((ch, prefer_image), source);
        source
    }

    fn find_source(&mut self, ch: char, prefer_image: bool) -> Option<GlyphSource> {
        let known: Vec<ID> = self.chain.iter().chain(&self.found).copied().collect();
        let probes: Vec<_> = known.iter().map(|&id| (id, self.probe(id, ch))).collect();
        let pick = |image: bool| {
            probes
                .iter()
                .find_map(|&(id, probe)| probe.source(id, image))
        };
        let known_source = pick(prefer_image).or_else(|| pick(!prefer_image));
        if let Some(source) = known_source
            && (!prefer_image || matches!(source, GlyphSource::Image(..)))
        {
            return Some(source);
        }

        // Search the rest, stopping at the first face with the preferred kind and keeping the
        // first of the other kind in case none has it.
        let candidates = self
            .candidates
            .get_or_insert_with(|| candidate_order(&self.db));
        let candidates: Vec<ID> = candidates
            .iter()
            .copied()
            .filter(|id| !known.contains(id))
            .collect();
        let mut fallback = known_source;
        for id in candidates {
            let probe = self.probe(id, ch);
            if let Some(source) = probe.source(id, prefer_image) {
                self.found.push(id);
                return Some(source);
            }
            if fallback.is_none() {
                fallback = probe.source(id, !prefer_image);
            }
        }
        if let Some(GlyphSource::Outline(id, _) | GlyphSource::Image(id, _)) = fallback
            && !known.contains(&id)
        {
            self.found.push(id);
        }
        fallback
    }

    /// The glyph `ch` has in face `id`, as an outline and as a color image.
    fn probe(&self, id: ID, ch: char) -> Probe {
        self.db
            .with_face_data(id, |data, index| {
                let face = Face::parse(data, index).ok()?;
                let glyph = face.glyph_index(ch)?;
                Some(Probe {
                    outline: face.outline_glyph(glyph, &mut NoOutline).map(|_| glyph),
                    image: face
                        .glyph_raster_image(glyph, u16::MAX)
                        .filter(|picture| picture.format == RasterImageFormat::PNG)
                        .map(|_| glyph),
                })
            })
            .flatten()
            .unwrap_or_default()
    }

    fn outline(&mut self, face: ID, glyph: GlyphId, size: u32) -> Option<Arc<Coverage>> {
        if self.glyphs.len() >= MAX_GLYPHS {
            self.glyphs.clear();
        }
        self.glyphs
            .entry((face, glyph, size))
            .or_insert_with(|| {
                self.db
                    .with_face_data(face, |data, index| {
                        rasterize(&Face::parse(data, index).ok()?, glyph, size)
                    })
                    .flatten()
                    .map(Arc::new)
            })
            .clone()
    }

    fn image(
        &mut self,
        face: ID,
        glyph: GlyphId,
        width: u32,
        height: u32,
    ) -> Option<Arc<RgbaImage>> {
        if self.images.len() >= MAX_IMAGES {
            self.images.clear();
        }
        self.images
            .entry((face, glyph, width, height))
            .or_insert_with(|| {
                let decoded = self
                    .db
                    .with_face_data(face, |data, index| {
                        let face = Face::parse(data, index).ok()?;
                        decode_png(face.glyph_raster_image(glyph, u16::MAX)?.data)
                    })
                    .flatten()?;
                Some(Arc::new(fit_image(&decoded, width, height)))
            })
            .clone()
    }
}

fn query_family(db: &Database, family: &str) -> Option<ID> {
    let families = if family.eq_ignore_ascii_case("monospace") {
        [Family::Monospace]
    } else {
        [Family::Name(family)]
    };
    db.query(&Query {
        families: &families,
        ..Query::default()
    })
}

/// Every face, monospace first, then upright, then nearest a regular weight.
fn candidate_order(db: &Database) -> Vec<ID> {
    let mut faces: Vec<_> = db.faces().collect();
    faces.sort_by_key(|face| {
        (
            !face.monospaced,
            face.style != Style::Normal,
            face.weight.0.abs_diff(400),
        )
    });
    faces.into_iter().map(|face| face.id).collect()
}

fn font_size(cell_rect: CellPixels) -> u32 {
    (cell_rect.height * 82 / 100).max(1)
}

fn is_variation_selector(ch: char) -> bool {
    matches!(ch, '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}')
}

/// Decode a PNG color glyph to RGBA.
///
/// This goes to the `png` crate directly: `image`'s format-generic loading and resizing would
/// roughly double what PNG capture adds to a binary.
fn decode_png(data: &[u8]) -> Option<RgbaImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buffer).ok()?;
    let pixels = &buffer[..frame.buffer_size()];
    let rgba: Vec<u8> = match frame.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[v, a]| [v, v, v, a])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    RgbaImage::from_raw(frame.width, frame.height, rgba)
}

/// Scale `picture` to fit inside `width` x `height`, keeping its proportions.
///
/// Each output pixel averages the source pixels it covers, weighted by alpha so transparent
/// edges do not darken; enlarging repeats the nearest source pixel.
fn fit_image(picture: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let (source_width, source_height) = picture.dimensions();
    if source_width == 0 || source_height == 0 {
        return RgbaImage::new(0, 0);
    }
    let scale = (width as f32 / source_width as f32).min(height as f32 / source_height as f32);
    let fitted_width = ((source_width as f32 * scale).round() as u32).clamp(1, width.max(1));
    let fitted_height = ((source_height as f32 * scale).round() as u32).clamp(1, height.max(1));
    let span = |index: u32, fitted: u32, source: u32| {
        let start = index * source / fitted;
        start..((index + 1) * source / fitted).max(start + 1)
    };
    RgbaImage::from_fn(fitted_width, fitted_height, |x, y| {
        let mut sums = [0_u64; 4];
        let mut count = 0_u64;
        for sy in span(y, fitted_height, source_height) {
            for sx in span(x, fitted_width, source_width) {
                let [r, g, b, a] = picture.get_pixel(sx, sy).0;
                let a = u64::from(a);
                sums[0] += u64::from(r) * a;
                sums[1] += u64::from(g) * a;
                sums[2] += u64::from(b) * a;
                sums[3] += a;
                count += 1;
            }
        }
        if sums[3] == 0 {
            return Rgba([0, 0, 0, 0]);
        }
        let channel = |sum: u64| (sum / sums[3]) as u8;
        Rgba([
            channel(sums[0]),
            channel(sums[1]),
            channel(sums[2]),
            (sums[3] / count) as u8,
        ])
    })
}

fn rasterize(face: &Face<'_>, glyph: GlyphId, size: u32) -> Option<Coverage> {
    let scale = size as f32 / f32::from(face.units_per_em());
    let advance = face
        .glyph_hor_advance(glyph)
        .map_or(0.0, |advance| f32::from(advance) * scale);
    let Some(bounds) = face.glyph_bounding_box(glyph) else {
        // A glyph with no outline, such as a space, still has an advance.
        return Some(Coverage {
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            advance,
            alpha: Vec::new(),
        });
    };
    let left = (f32::from(bounds.x_min) * scale).floor() as i32;
    let right = (f32::from(bounds.x_max) * scale).ceil() as i32;
    let top = -(f32::from(bounds.y_max) * scale).ceil() as i32;
    let bottom = -(f32::from(bounds.y_min) * scale).floor() as i32;
    let width = u32::try_from(right - left).ok()?;
    let height = u32::try_from(bottom - top).ok()?;
    let mut builder = RasterBuilder::new(width, height, scale, left as f32, top as f32);
    face.outline_glyph(glyph, &mut builder)?;
    Some(Coverage {
        left,
        top,
        width,
        height,
        advance,
        alpha: builder.coverage(),
    })
}

/// Draw `coverage` with its origin at `origin` on `baseline`, clipped to `clip`.
fn blit(
    image: &mut RgbImage,
    clip: CellPixels,
    coverage: &Coverage,
    origin: i32,
    baseline: i32,
    color: Rgb8,
    bold_offset: Option<i32>,
) {
    let clip_x = clip.x0 as i32..clip.x0.saturating_add(clip.width) as i32;
    let clip_y = clip.y0 as i32..clip.y0.saturating_add(clip.height) as i32;
    for offset in std::iter::once(0).chain(bold_offset) {
        for glyph_y in 0..coverage.height {
            for glyph_x in 0..coverage.width {
                let alpha = coverage.alpha[(glyph_y * coverage.width + glyph_x) as usize];
                let x = origin + coverage.left + glyph_x as i32 + offset;
                let y = baseline + coverage.top + glyph_y as i32;
                if alpha > 0 && clip_x.contains(&x) && clip_y.contains(&y) {
                    blend_rgb(image, x as u32, y as u32, color, alpha);
                }
            }
        }
    }
}

/// Reports whether a glyph has an outline without drawing it.
struct NoOutline;

impl OutlineBuilder for NoOutline {
    fn move_to(&mut self, _: f32, _: f32) {}
    fn line_to(&mut self, _: f32, _: f32) {}
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
    fn close(&mut self) {}
}

/// Feeds a glyph outline, in font units with y up, to a coverage rasterizer in pixels with y down.
struct RasterBuilder {
    raster: Rasterizer,
    width: u32,
    scale: f32,
    left: f32,
    top: f32,
    start: Point,
    last: Point,
}

impl RasterBuilder {
    fn new(width: u32, height: u32, scale: f32, left: f32, top: f32) -> Self {
        Self {
            raster: Rasterizer::new(width as usize, height as usize),
            width,
            scale,
            left,
            top,
            start: point(0.0, 0.0),
            last: point(0.0, 0.0),
        }
    }

    fn map(&self, x: f32, y: f32) -> Point {
        point(x * self.scale - self.left, -y * self.scale - self.top)
    }

    fn coverage(&self) -> Vec<u8> {
        let (width, height) = self.raster.dimensions();
        let mut alpha = vec![0; width * height];
        self.raster.for_each_pixel_2d(|x, y, value| {
            alpha[y as usize * self.width as usize + x as usize] =
                (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        });
        alpha
    }
}

impl OutlineBuilder for RasterBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = self.map(x, y);
        self.last = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.map(x, y);
        self.raster.draw_line(self.last, to);
        self.last = to;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (control, to) = (self.map(x1, y1), self.map(x, y));
        self.raster.draw_quad(self.last, control, to);
        self.last = to;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (first, second, to) = (self.map(x1, y1), self.map(x2, y2), self.map(x, y));
        self.raster.draw_cubic(self.last, first, second, to);
        self.last = to;
    }

    fn close(&mut self) {
        if self.last != self.start {
            self.raster.draw_line(self.last, self.start);
        }
        self.last = self.start;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raster_builder_fills_a_closed_outline() {
        // A 4x4 font-unit square at scale 1, spanning y 0..4 (y up) into a 4x4 pixel grid.
        let mut builder = RasterBuilder::new(4, 4, 1.0, 0.0, -4.0);
        builder.move_to(1.0, 1.0);
        builder.line_to(3.0, 1.0);
        builder.line_to(3.0, 3.0);
        builder.line_to(1.0, 3.0);
        builder.close();
        let alpha = builder.coverage();

        let inside = |x: usize, y: usize| alpha[y * 4 + x];
        assert_eq!(inside(1, 1), 255);
        assert_eq!(inside(2, 2), 255);
        assert_eq!(inside(0, 0), 0);
        assert_eq!(inside(3, 3), 0);
    }

    #[test]
    fn fit_image_averages_without_darkening_transparent_edges() {
        // Opaque red beside fully transparent black: the average stays pure red at half alpha.
        let picture = RgbaImage::from_fn(2, 2, |x, _| {
            if x == 0 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        assert_eq!(
            fit_image(&picture, 1, 1).get_pixel(0, 0).0,
            [255, 0, 0, 127]
        );
    }

    #[test]
    fn decode_png_reads_rgba() {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            writer.write_image_data(&[1, 2, 3, 4]).expect("data");
        }
        let decoded = decode_png(&encoded).expect("png decodes");
        assert_eq!(decoded.get_pixel(0, 0).0, [1, 2, 3, 4]);
    }

    #[test]
    fn fit_image_keeps_proportions_inside_the_box() {
        let wide = RgbaImage::new(40, 20);
        assert_eq!(fit_image(&wide, 16, 16).dimensions(), (16, 8));
        let tall = RgbaImage::new(10, 30);
        assert_eq!(fit_image(&tall, 16, 16).dimensions(), (5, 16));
        assert_eq!(
            fit_image(&RgbaImage::new(0, 0), 16, 16).dimensions(),
            (0, 0)
        );
    }

    #[test]
    fn candidates_prefer_monospace_upright_regular_faces() {
        let mut db = Database::new();
        db.load_system_fonts();
        let order = candidate_order(&db);
        assert_eq!(order.len(), db.len());
        let keys: Vec<_> = order
            .iter()
            .filter_map(|id| db.face(*id))
            .map(|face| (!face.monospaced, face.style != Style::Normal))
            .collect();
        assert!(keys.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
