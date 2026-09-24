#[cfg(feature = "terminal-images")]
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
#[cfg(feature = "terminal-images")]
use std::fmt::Write as _;
#[cfg(feature = "terminal-images")]
use std::io::Write as _;
#[cfg(feature = "terminal-images")]
use std::num::NonZeroU16;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(feature = "terminal-images")]
use base64::Engine as _;
#[cfg(feature = "terminal-images")]
use base64::engine::general_purpose::STANDARD as BASE64;
#[cfg(feature = "terminal-images")]
use flate2::Compression;
#[cfg(feature = "terminal-images")]
use flate2::write::ZlibEncoder;
#[cfg(feature = "terminal-images")]
use ratatui::buffer::CellDiffOption;
use ratatui::layout::Alignment;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui_image::Image as RatatuiImageWidget;
use ratatui_image::Resize;
use ratatui_image::picker::ProtocolType;
use ratatui_image::protocol::Protocol;

use crate::backend::ratatui_backend::common::{
    BackdropBackgroundEffect, to_ratatui_rect, to_ratatui_style,
};
use crate::backend::ratatui_backend::image_support;
use crate::backend::ratatui_backend::renderers::image_effects::{
    ReplayedEffect, for_each_pending_image_effect,
};
#[cfg(feature = "terminal-images")]
use crate::backend::ratatui_backend::shared_frame::{self, SharedFrame};
use crate::style::resolve::resolve_base_style;
use crate::style::{Rect, Theme};
use crate::widgets::internal::ImageNode;
use crate::widgets::{ImageFit, ImageProtocol};

#[cfg(feature = "terminal-images")]
thread_local! {
    static IMAGE_OCCLUSIONS: RefCell<Vec<ratatui::layout::Rect>> = const { RefCell::new(Vec::new()) };
    static PAINTED_IMAGE_OCCLUSIONS: RefCell<Vec<ratatui::layout::Rect>> = const { RefCell::new(Vec::new()) };
    static IMAGE_PLACEHOLDERS_PAINTED: Cell<bool> = const { Cell::new(false) };
    /// Images a frame capture drew, in draw order. `Some` only inside [`record_capture_images`].
    static CAPTURE_IMAGES: RefCell<Option<Vec<CaptureImageDraw>>> = const { RefCell::new(None) };
}

thread_local! {
    /// Backdrops that recolor the cells of images drawn from here on this frame, in the order they
    /// apply. See [`set_image_backdrops`].
    static IMAGE_BACKDROPS: RefCell<Vec<ImageBackdrop>> = const { RefCell::new(Vec::new()) };
}

/// A recolor the renderer will apply over `rect` after the images under it have drawn.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ImageBackdrop {
    pub(crate) rect: ratatui::layout::Rect,
    pub(crate) effect: PixelEffect,
}

/// How a layer over an image recolors the cells, and so the pixels, it covers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PixelEffect {
    /// A root overlay's backdrop.
    Backdrop(BackdropBackgroundEffect),
    /// A pass inside the layer the image draws in. See [`super::image_effects`].
    Replayed(ReplayedEffect),
}

impl From<BackdropBackgroundEffect> for PixelEffect {
    fn from(effect: BackdropBackgroundEffect) -> Self {
        Self::Backdrop(effect)
    }
}

impl PixelEffect {
    fn is_per_channel(&self) -> bool {
        match self {
            Self::Backdrop(effect) => effect.is_per_channel(),
            Self::Replayed(effect) => effect.is_per_channel(),
        }
    }

    fn apply_rgb(&self, rgb: (u8, u8, u8)) -> (u8, u8, u8) {
        match self {
            Self::Backdrop(effect) => effect.apply_rgb(rgb),
            Self::Replayed(effect) => effect.apply_rgb(rgb),
        }
    }
}

/// Tell image draws which backdrops will recolor the cells they cover.
///
/// A backdrop recolors cells after they are drawn, and an image is not cells, so its pixels have
/// to be put through the same transform before they are encoded. The renderer knows every overlay
/// before any pane draws, which is when this is set; it narrows the list before each overlay
/// draws, since a backdrop only reaches what is beneath it.
pub(crate) fn set_image_backdrops(backdrops: Vec<ImageBackdrop>) {
    IMAGE_BACKDROPS.with(|slot| *slot.borrow_mut() = backdrops);
}

/// Drop the frame's backdrop list. [`set_image_backdrops`] installs the next one.
pub(crate) fn clear_image_backdrops() {
    IMAGE_BACKDROPS.with(|slot| slot.borrow_mut().clear());
}

/// The backdrops over an image laid out on a `columns` x `rows` cell box, as cell rects relative to
/// its top-left cell. Only the pixels in covered cells change.
#[derive(Debug, PartialEq, Eq, Hash)]
struct BackdropMask {
    columns: u16,
    rows: u16,
    layers: Vec<(ratatui::layout::Rect, PixelEffect)>,
}

/// Backdrops beyond this many over one image are ignored; each one is a bit in a per-cell mask.
const MAX_BACKDROP_LAYERS: usize = 64;

/// The layers that will cover part of an image laid out over `area`, if any: passes still to come
/// in the layer the image draws in, then the backdrops of overlays above it.
fn backdrop_mask_for(area: Rect) -> Option<Arc<BackdropMask>> {
    if area.is_empty() {
        return None;
    }
    let (x0, y0) = (i32::from(area.x), i32::from(area.y));
    let (x1, y1) = (x0 + i32::from(area.w), y0 + i32::from(area.h));
    let mut layers = Vec::new();
    let mut add = |backdrop: &ImageBackdrop| {
        if layers.len() == MAX_BACKDROP_LAYERS {
            return;
        }
        let left = x0.max(i32::from(backdrop.rect.x));
        let top = y0.max(i32::from(backdrop.rect.y));
        let right = x1.min(i32::from(backdrop.rect.right()));
        let bottom = y1.min(i32::from(backdrop.rect.bottom()));
        if left < right && top < bottom {
            let covered = ratatui::layout::Rect::new(
                (left - x0) as u16,
                (top - y0) as u16,
                (right - left) as u16,
                (bottom - top) as u16,
            );
            layers.push((covered, backdrop.effect.clone()));
        }
    };
    for_each_pending_image_effect(&mut add);
    IMAGE_BACKDROPS.with(|slot| slot.borrow().iter().for_each(&mut add));
    (!layers.is_empty()).then(|| {
        Arc::new(BackdropMask {
            columns: area.w,
            rows: area.h,
            layers,
        })
    })
}

impl BackdropMask {
    /// A non-zero identity for cache and stream keys.
    fn key(&self) -> u64 {
        use std::hash::{Hash as _, Hasher as _};

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish().max(1)
    }

    /// `image` with the pixels under each covered cell recolored as the backdrop recolors that
    /// cell's background. Alpha is kept, unless `flatten` names the background a protocol without
    /// alpha composites onto: that composite is what the cell shows, so it is what has to dim.
    ///
    /// Pixels map to cells the way a terminal lays the image out: scaled to fit the box without
    /// changing shape, from the top-left corner, at `cell` pixels per cell.
    fn apply(
        &self,
        image: &image::DynamicImage,
        cell: (u32, u32),
        flatten: Option<(u8, u8, u8)>,
    ) -> image::DynamicImage {
        let (width, height) = (image.width(), image.height());
        let (columns, rows) = (u32::from(self.columns), u32::from(self.rows));
        if width == 0 || height == 0 || columns == 0 || rows == 0 {
            return image.clone();
        }
        let (cell_w, cell_h) = (cell.0.max(1), cell.1.max(1));
        let (fitted_w, fitted_h) =
            crate::capture::fitted_pixel_size((width, height), (columns * cell_w, rows * cell_h));
        let cell_of = |pixel: u32, source: u32, fitted: u32, cell: u32, cells: u32| {
            ((u64::from(pixel) * u64::from(fitted) / u64::from(source) / u64::from(cell)) as u32)
                .min(cells - 1)
        };
        let column_of: Vec<u32> = (0..width)
            .map(|x| cell_of(x, width, fitted_w.max(1), cell_w, columns))
            .collect();
        let row_of: Vec<u32> = (0..height)
            .map(|y| cell_of(y, height, fitted_h.max(1), cell_h, rows))
            .collect();

        let mut cell_layers = vec![0u64; (columns * rows) as usize];
        for (bit, (rect, _)) in self.layers.iter().enumerate() {
            for y in rect.top()..rect.bottom().min(self.rows) {
                for x in rect.left()..rect.right().min(self.columns) {
                    cell_layers[usize::from(y) * usize::from(self.columns) + usize::from(x)] |=
                        1 << bit;
                }
            }
        }

        let mut recolorer = Recolorer::new(self);
        let mut recolor = |layers: u64, rgb: [u8; 3]| recolorer.recolor(layers, rgb);
        let layers_at = |x: u32, y: u32| {
            cell_layers[(row_of[y as usize] * columns + column_of[x as usize]) as usize]
        };

        if let (image::DynamicImage::ImageRgb8(rgb), None) = (image, flatten) {
            let mut out = rgb.clone();
            for (x, y, pixel) in out.enumerate_pixels_mut() {
                let layers = layers_at(x, y);
                if layers != 0 {
                    pixel.0 = recolor(layers, pixel.0);
                }
            }
            return image::DynamicImage::ImageRgb8(out);
        }

        let mut out = image.to_rgba8();
        for (x, y, pixel) in out.enumerate_pixels_mut() {
            let layers = layers_at(x, y);
            if layers == 0 {
                continue;
            }
            let [r, g, b, a] = pixel.0;
            let (rgb, alpha) = match flatten {
                Some(background) if a < 255 => {
                    let over = |channel: u8, under: u8| {
                        ((u16::from(channel) * u16::from(a)
                            + u16::from(under) * (255 - u16::from(a))
                            + 127)
                            / 255) as u8
                    };
                    (
                        [
                            over(r, background.0),
                            over(g, background.1),
                            over(b, background.2),
                        ],
                        255,
                    )
                }
                _ => ([r, g, b], a),
            };
            let [r, g, b] = recolor(layers, rgb);
            pixel.0 = [r, g, b, alpha];
        }
        image::DynamicImage::ImageRgba8(out)
    }
}

/// Recolors pixels through the backdrop layers a cell is under, a combination at a time.
///
/// A full-screen frame is millions of pixels and [`BackdropBackgroundEffect::apply_rgb`] is color
/// arithmetic, so it runs a bounded number of times per combination. When every layer works channel
/// by channel, 256 runs fill exact per-channel tables. Otherwise the first [`Self::EXACT_BUDGET`]
/// distinct colors are computed exactly and kept, which covers screens, pages, and plots, whose
/// pictures must match the cells beside them. Past that, which only photographic pixels reach, a
/// new color is computed as its nearest one at [`Self::LEVELS`] levels per channel, at most two
/// levels away. `Elevate` jumps from lightening to dimming at a luminance threshold, so nothing that
/// interpolates between exact colors would do.
struct Recolorer<'a> {
    mask: &'a BackdropMask,
    mappings: Vec<(u64, Mapping)>,
    /// The index in `mappings` of the combination the previous pixel used.
    current: usize,
    /// Distinct colors computed exactly so far, across every combination.
    exact_len: usize,
}

enum Mapping {
    Tables(Box<[[u8; 256]; 3]>),
    Mixed {
        /// Open-addressed table of exactly computed colors, each `1 << 48 | rgb << 24 | out` and
        /// `0` when empty. It is never more than half full, so a color it holds stays for the frame.
        exact: Box<[u64]>,
        /// Once the exact budget is spent: exact colors of inputs rounded to
        /// [`Recolorer::LEVELS`] per channel, each `1 << 24 | out` and `0` until met.
        quantized: Option<Box<[u32]>>,
    },
}

fn pack_rgb(rgb: [u8; 3]) -> u64 {
    u64::from(rgb[0]) << 16 | u64::from(rgb[1]) << 8 | u64::from(rgb[2])
}

fn unpack_rgb(packed: u64) -> [u8; 3] {
    [(packed >> 16) as u8, (packed >> 8) as u8, packed as u8]
}

impl<'a> Recolorer<'a> {
    const EXACT_BUDGET: usize = 1 << 14;
    const EXACT_SLOTS: usize = Self::EXACT_BUDGET * 2;
    const LEVELS: usize = 64;

    fn new(mask: &'a BackdropMask) -> Self {
        Self {
            mask,
            mappings: Vec::new(),
            current: 0,
            exact_len: 0,
        }
    }

    fn apply_layers(&self, layers: u64, rgb: [u8; 3]) -> [u8; 3] {
        let mut color = (rgb[0], rgb[1], rgb[2]);
        for (bit, (_, effect)) in self.mask.layers.iter().enumerate() {
            if layers & (1 << bit) != 0 {
                color = effect.apply_rgb(color);
            }
        }
        [color.0, color.1, color.2]
    }

    fn mapping_index(&mut self, layers: u64) -> usize {
        if self
            .mappings
            .get(self.current)
            .is_some_and(|(seen, _)| *seen == layers)
        {
            return self.current;
        }
        self.current = match self.mappings.iter().position(|(seen, _)| *seen == layers) {
            Some(index) => index,
            None => {
                let per_channel = self
                    .mask
                    .layers
                    .iter()
                    .enumerate()
                    .all(|(bit, (_, effect))| layers & (1 << bit) == 0 || effect.is_per_channel());
                let mapping = if per_channel {
                    let mut tables = Box::new([[0u8; 256]; 3]);
                    for value in 0..=255u8 {
                        let out = self.apply_layers(layers, [value; 3]);
                        for (channel, table) in tables.iter_mut().enumerate() {
                            table[usize::from(value)] = out[channel];
                        }
                    }
                    Mapping::Tables(tables)
                } else {
                    Mapping::Mixed {
                        exact: vec![0; Self::EXACT_SLOTS].into_boxed_slice(),
                        quantized: None,
                    }
                };
                self.mappings.push((layers, mapping));
                self.mappings.len() - 1
            }
        };
        self.current
    }

    fn recolor(&mut self, layers: u64, rgb: [u8; 3]) -> [u8; 3] {
        let index = self.mapping_index(layers);
        if let Mapping::Tables(tables) = &self.mappings[index].1 {
            return [
                tables[0][usize::from(rgb[0])],
                tables[1][usize::from(rgb[1])],
                tables[2][usize::from(rgb[2])],
            ];
        }

        let packed = pack_rgb(rgb);
        let Mapping::Mixed { exact, .. } = &self.mappings[index].1 else {
            unreachable!("handled above");
        };
        let mut slot =
            (packed.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 49) as usize & (Self::EXACT_SLOTS - 1);
        while exact[slot] != 0 {
            if (exact[slot] >> 24) & 0xFF_FFFF == packed {
                return unpack_rgb(exact[slot]);
            }
            slot = (slot + 1) & (Self::EXACT_SLOTS - 1);
        }

        let exact_slot = (self.exact_len < Self::EXACT_BUDGET).then_some(slot);
        let step = 256 / Self::LEVELS;
        let center = |value: u8| (usize::from(value) / step * step + step / 2) as u8;
        let level = |value: u8| usize::from(value) / step;
        let cell = (level(rgb[0]) * Self::LEVELS + level(rgb[1])) * Self::LEVELS + level(rgb[2]);
        if exact_slot.is_none()
            && let Mapping::Mixed {
                quantized: Some(quantized),
                ..
            } = &self.mappings[index].1
            && quantized[cell] != 0
        {
            return unpack_rgb(u64::from(quantized[cell]));
        }

        let input = match exact_slot {
            Some(_) => rgb,
            None => [center(rgb[0]), center(rgb[1]), center(rgb[2])],
        };
        let out = self.apply_layers(layers, input);
        let Mapping::Mixed { exact, quantized } = &mut self.mappings[index].1 else {
            unreachable!("handled above");
        };
        match exact_slot {
            Some(slot) => {
                exact[slot] = 1 << 48 | packed << 24 | pack_rgb(out);
                self.exact_len += 1;
            }
            None => {
                quantized.get_or_insert_with(|| vec![0; Self::LEVELS.pow(3)].into_boxed_slice())
                    [cell] = 1 << 24 | pack_rgb(out) as u32;
            }
        }
        out
    }
}

/// The mark stamped over the cells a captured image covers: `U+FFFF`, then the image's index as two
/// zero-width variation selectors.
///
/// A frame capture cannot hand pixels to a host terminal, so [`draw_encoded_image`] records them and
/// marks their cells instead. Whatever is drawn later replaces the mark - an overlay, a border, a
/// pane above - which is how the capture learns which cells still show the image, exactly as the
/// host's placeholder cells would.
///
/// A mark must never be something a widget can draw, or text drawn over an image would read as the
/// image still showing. Private-use code points are icons (Nerd Font keeps its Material Design set in
/// plane 15, and Kitty's placeholder is U+10EEEE), so the mark leads with a Unicode noncharacter,
/// which no text may contain. The variation selectors after it are zero width, so the whole mark is
/// one grapheme one cell wide, and a buffer diff treats it like any narrow symbol.
#[cfg(feature = "terminal-images")]
const CAPTURE_MARK_LEAD: char = '\u{FFFF}';

/// The first of the 240 variation selectors (U+E0100-U+E01EF) a mark's index is written in.
#[cfg(feature = "terminal-images")]
const CAPTURE_MARK_DIGIT: u32 = 0xE0100;

/// How many values one index digit holds.
#[cfg(feature = "terminal-images")]
const CAPTURE_MARK_BASE: usize = 240;

/// How many images one capture can mark.
#[cfg(feature = "terminal-images")]
pub(crate) const CAPTURE_IMAGE_LIMIT: usize = CAPTURE_MARK_BASE * CAPTURE_MARK_BASE;

/// The mark for the image at `index`, which must be below [`CAPTURE_IMAGE_LIMIT`].
#[cfg(feature = "terminal-images")]
pub(crate) fn capture_image_mark(index: usize) -> String {
    debug_assert!(index < CAPTURE_IMAGE_LIMIT);
    let digit = |value: usize| {
        char::from_u32(CAPTURE_MARK_DIGIT + value as u32).expect("a variation selector")
    };
    [
        CAPTURE_MARK_LEAD,
        digit(index / CAPTURE_MARK_BASE),
        digit(index % CAPTURE_MARK_BASE),
    ]
    .into_iter()
    .collect()
}

/// The image index `symbol` marks, if it is a mark.
#[cfg(feature = "terminal-images")]
pub(crate) fn capture_image_mark_index(symbol: &str) -> Option<usize> {
    let mut chars = symbol.chars();
    let digit = |ch: char| {
        u32::from(ch)
            .checked_sub(CAPTURE_MARK_DIGIT)
            .map(|value| value as usize)
            .filter(|&value| value < CAPTURE_MARK_BASE)
    };
    if chars.next()? != CAPTURE_MARK_LEAD {
        return None;
    }
    let high = digit(chars.next()?)?;
    let low = digit(chars.next()?)?;
    chars
        .next()
        .is_none()
        .then_some(high * CAPTURE_MARK_BASE + low)
}

/// One image [`draw_encoded_image`] drew during a frame capture: the cells it covers, and the
/// pixels scaled into them.
#[cfg(feature = "terminal-images")]
pub(crate) struct CaptureImageDraw {
    pub(crate) area: ratatui::layout::Rect,
    pub(crate) pixels: Arc<image::DynamicImage>,
}

/// Run `render` with image draws recorded for a frame capture instead of encoded for the host.
#[cfg(feature = "terminal-images")]
pub(crate) fn record_capture_images<R>(render: impl FnOnce() -> R) -> (R, Vec<CaptureImageDraw>) {
    struct Restore(Option<Option<Vec<CaptureImageDraw>>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(previous) = self.0.take() {
                CAPTURE_IMAGES.with(|slot| *slot.borrow_mut() = previous);
            }
        }
    }

    let mut restore = Restore(Some(
        CAPTURE_IMAGES.with(|slot| slot.replace(Some(Vec::new()))),
    ));
    let result = render();
    let previous = restore.0.take().unwrap_or_default();
    let drawn = CAPTURE_IMAGES
        .with(|slot| slot.replace(previous))
        .unwrap_or_default();
    (result, drawn)
}

/// Whether a frame capture is recording image draws on this thread.
#[cfg(feature = "terminal-images")]
fn capturing_images() -> bool {
    CAPTURE_IMAGES.with(|slot| slot.borrow().is_some())
}

/// Record an image for the frame capture in progress and mark the cells it covers.
#[cfg(feature = "terminal-images")]
fn record_capture_image(
    f: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    pixels: Arc<image::DynamicImage>,
) {
    let index = CAPTURE_IMAGES.with(|slot| {
        let mut slot = slot.borrow_mut();
        let drawn = slot.as_mut().expect("checked above");
        (drawn.len() < CAPTURE_IMAGE_LIMIT).then(|| {
            drawn.push(CaptureImageDraw { area, pixels });
            drawn.len() - 1
        })
    });
    let Some(marker) = index.map(capture_image_mark) else {
        return;
    };
    let buffer = f.buffer_mut();
    let area = area.intersection(buffer.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buffer[(x, y)].set_symbol(&marker);
        }
    }
}

/// Remember which cells a Kitty placeholder row must not cover this frame.
///
/// A placeholder row is written from its first cell as one escape sequence that walks the cursor
/// across the whole width. A later overlay or Canvas layer is painted into the buffer afterwards,
/// but unchanged cells are not re-emitted, so a new native frame can show through it on the host.
/// Subtracting the rects from the walk is what stops that; [`CellDiffOption::AlwaysUpdate`] on the
/// same cells is the belt in case a walk still races them.
#[cfg(feature = "terminal-images")]
pub(crate) fn set_image_occlusions(rects: Vec<ratatui::layout::Rect>) {
    IMAGE_OCCLUSIONS.with(|slot| *slot.borrow_mut() = rects);
    PAINTED_IMAGE_OCCLUSIONS.with(|slot| slot.borrow_mut().clear());
    IMAGE_PLACEHOLDERS_PAINTED.set(false);
}

/// Add holes for one image-owning subtree, restoring the frame-wide list on drop.
///
/// Floating terminal panes are ordinary Canvas layers rather than root-portal overlays. A lower
/// pane must walk around the panes above it, while the upper pane must still draw inside its own
/// rectangle, so those holes only apply while the lower terminal node is rendered.
#[cfg(feature = "terminal-images")]
pub(crate) fn push_image_occlusions(
    rects: impl IntoIterator<Item = ratatui::layout::Rect>,
) -> ImageOcclusionScope {
    let original_len = IMAGE_OCCLUSIONS.with(|slot| {
        let mut active = slot.borrow_mut();
        let original_len = active.len();
        active.extend(rects);
        original_len
    });
    ImageOcclusionScope(original_len)
}

/// Restores the active occlusion list even if a widget renderer panics.
#[cfg(feature = "terminal-images")]
pub(crate) struct ImageOcclusionScope(usize);

#[cfg(feature = "terminal-images")]
impl Drop for ImageOcclusionScope {
    fn drop(&mut self) {
        IMAGE_OCCLUSIONS.with(|slot| slot.borrow_mut().truncate(self.0));
    }
}

/// Drop the frame's occlusion list. [`set_image_occlusions`] installs the next one.
#[cfg(feature = "terminal-images")]
pub(crate) fn clear_image_occlusions() {
    IMAGE_OCCLUSIONS.with(|slot| slot.borrow_mut().clear());
    PAINTED_IMAGE_OCCLUSIONS.with(|slot| slot.borrow_mut().clear());
    IMAGE_PLACEHOLDERS_PAINTED.set(false);
}

/// Whether this frame wrote Kitty placeholders, so overlay cells must be forced through the diff.
#[cfg(feature = "terminal-images")]
pub(crate) fn image_placeholders_painted() -> bool {
    IMAGE_PLACEHOLDERS_PAINTED.get()
}

/// Holes used by a placeholder walk that actually painted this frame.
#[cfg(feature = "terminal-images")]
pub(crate) fn painted_image_occlusions() -> Vec<ratatui::layout::Rect> {
    PAINTED_IMAGE_OCCLUSIONS.with(|slot| slot.borrow().clone())
}

/// Whether every cell of `area` is covered by this frame's occlusions.
///
/// Worth asking before a placement is decoded rather than after: the walk that discovers it has no
/// visible span to write is the *last* step, by which point the frame has already been read out of
/// the child's file, decoded, encoded and copied into shared memory - a full pipeline for a picture
/// that goes nowhere. A pane entirely under DevTools, an overlay, or a later terminal layer costs
/// nothing now.
#[cfg(feature = "terminal-images")]
pub(crate) fn image_area_fully_occluded(area: ratatui::layout::Rect) -> bool {
    if area.width == 0 || area.height == 0 {
        return false;
    }
    IMAGE_OCCLUSIONS.with(|slot| {
        let holes = slot.borrow();
        if holes.is_empty() {
            return false;
        }
        let right = area.x.saturating_add(area.width);
        (area.y..area.y.saturating_add(area.height))
            .all(|y| uncovered_x_spans(area.x, right, y, &holes).is_empty())
    })
}

/// Columns of `y` in `[x0, x1)` that no occlusion covers, as half-open spans.
#[cfg(feature = "terminal-images")]
fn uncovered_x_spans(x0: u16, x1: u16, y: u16, holes: &[ratatui::layout::Rect]) -> Vec<(u16, u16)> {
    if x0 >= x1 {
        return Vec::new();
    }
    let mut cuts: Vec<(u16, u16)> = holes
        .iter()
        .filter_map(|hole| {
            if y < hole.y || y >= hole.y.saturating_add(hole.height) {
                return None;
            }
            let left = hole.x.max(x0);
            let right = hole.x.saturating_add(hole.width).min(x1);
            (left < right).then_some((left, right))
        })
        .collect();
    if cuts.is_empty() {
        return vec![(x0, x1)];
    }
    cuts.sort_unstable();
    let mut spans = Vec::new();
    let mut cursor = x0;
    for (left, right) in cuts {
        if cursor < left {
            spans.push((cursor, left));
        }
        cursor = cursor.max(right);
    }
    if cursor < x1 {
        spans.push((cursor, x1));
    }
    spans
}

enum EncodedProtocol {
    Ratatui {
        protocol: Protocol,
        transmission_pending: AtomicBool,
    },
    #[cfg(feature = "terminal-images")]
    CompressedKitty(CompressedKitty),
}

impl EncodedProtocol {
    fn ratatui(protocol: Protocol, resolved_protocol: ImageProtocol) -> Self {
        Self::Ratatui {
            protocol,
            transmission_pending: AtomicBool::new(matches!(
                resolved_protocol,
                ImageProtocol::Kitty
            )),
        }
    }

    fn render(&self, f: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) {
        match self {
            Self::Ratatui {
                protocol,
                transmission_pending,
            } => {
                f.render_widget(RatatuiImageWidget::new(protocol), area);
                transmission_pending.store(false, Ordering::Release);
            }
            #[cfg(feature = "terminal-images")]
            Self::CompressedKitty(protocol) => protocol.render(f, area),
        }
    }

    /// Draw the part of a full-size encode inside `clip`, when the protocol can do that without
    /// re-encoding. Returns `false` when it cannot, and nothing was drawn.
    fn render_clipped(&self, f: &mut ratatui::Frame<'_>, image_rect: Rect, clip: Rect) -> bool {
        match self {
            Self::Ratatui { .. } => {
                let _ = (f, image_rect, clip);
                false
            }
            #[cfg(feature = "terminal-images")]
            Self::CompressedKitty(protocol) => {
                protocol.render_clipped(
                    f,
                    (i32::from(image_rect.x), i32::from(image_rect.y)),
                    to_ratatui_rect(clip),
                );
                true
            }
        }
    }

    fn transmission_pending(&self) -> bool {
        match self {
            Self::Ratatui {
                transmission_pending,
                ..
            } => transmission_pending.load(Ordering::Acquire),
            #[cfg(feature = "terminal-images")]
            Self::CompressedKitty(protocol) => protocol.transmission_pending(),
        }
    }

    fn retained_estimated_bytes(&self, encoded_estimate: usize) -> usize {
        #[cfg(feature = "terminal-images")]
        if let Self::CompressedKitty(protocol) = self
            && !protocol.transmission_pending()
        {
            return encoded_estimate.min(4 * 1024);
        }

        encoded_estimate
    }
}

#[cfg(feature = "terminal-images")]
struct CompressedKitty {
    transmit: Mutex<Option<String>>,
    /// Held for as long as the transmission that names it might still be written, and unlinked on
    /// drop if it never was. `None` for an inline transmission, which carries its own pixels.
    shared: Mutex<Option<SharedFrame>>,
    id_color: String,
    id_extra: u16,
    size: ratatui::layout::Size,
}

#[cfg(feature = "terminal-images")]
impl CompressedKitty {
    fn new(
        image: &image::DynamicImage,
        size: ratatui::layout::Size,
        id: u32,
        z_index: i32,
    ) -> Option<Self> {
        let width = image.width();
        let height = image.height();
        let converted;
        let (pixels, format) = match image {
            image::DynamicImage::ImageRgb8(rgb) => (rgb.as_raw().as_slice(), 24),
            image::DynamicImage::ImageRgba8(rgba) => (rgba.as_raw().as_slice(), 32),
            _ => {
                converted = image.to_rgba8();
                (converted.as_raw().as_slice(), 32)
            }
        };
        let (transmit, shared) = match shared_frame(pixels) {
            // Naming the pixels: no deflate, no base64 of megabytes, no chunked write, and the
            // terminal reads them straight out of memory.
            Some(frame) => (
                kitty_transmit_shared_memory_at_z(
                    frame.name(),
                    width,
                    height,
                    format,
                    id,
                    size,
                    z_index,
                ),
                Some(frame),
            ),
            None => {
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
                encoder.write_all(pixels).ok()?;
                let compressed = encoder.finish().ok()?;
                (
                    kitty_transmit_compressed_format(
                        &compressed,
                        width,
                        height,
                        format,
                        id,
                        size,
                        z_index,
                    ),
                    None,
                )
            }
        };
        let [id_extra, id_r, id_g, id_b] = id.to_be_bytes();

        Some(Self {
            transmit: Mutex::new(Some(transmit)),
            shared: Mutex::new(shared),
            id_color: format!("\x1b[38;2;{id_r};{id_g};{id_b}m"),
            id_extra: u16::from(id_extra),
            size,
        })
    }

    fn render(&self, f: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) {
        self.render_clipped(f, (i32::from(area.x), i32::from(area.y)), area);
    }

    /// Draw the placeholder cells of the image placed at `origin` that fall inside `clip`. Every
    /// placeholder names its own image row and column, so the host shows exactly the visible part
    /// of the one transmitted image, with nothing to re-encode as it scrolls. `origin` may lie
    /// above or left of the screen.
    fn render_clipped(
        &self,
        f: &mut ratatui::Frame<'_>,
        origin: (i32, i32),
        clip: ratatui::layout::Rect,
    ) {
        const UNIT_WIDTH: CellDiffOption =
            CellDiffOption::ForcedWidth(NonZeroU16::new(1).expect("one is non-zero"));

        let (origin_x, origin_y) = origin;
        let row_start = origin_x.max(i32::from(clip.x));
        let row_end = (origin_x + i32::from(self.size.width)).min(i32::from(clip.right()));
        if row_start >= row_end {
            return;
        }
        let (row_start, row_end) = (row_start as u16, row_end as u16);
        let height = self.size.height.min(297);
        let mut transmit = self.take_transmission();
        let mut symbol = String::new();
        let holes = IMAGE_OCCLUSIONS.with(|slot| slot.borrow().clone());
        let mut painted_placeholders = false;
        let area = ratatui::layout::Rect::new(
            row_start,
            origin_y.max(i32::from(clip.y)) as u16,
            row_end - row_start,
            (origin_y + i32::from(height))
                .min(i32::from(clip.bottom()))
                .saturating_sub(origin_y.max(i32::from(clip.y)))
                .max(0) as u16,
        );

        for y in 0..height {
            let row_y = origin_y + i32::from(y);
            if row_y < i32::from(clip.y) || row_y >= i32::from(clip.bottom()) {
                continue;
            }
            let row_y = row_y as u16;
            let spans = uncovered_x_spans(row_start, row_end, row_y, &holes);
            for &(start, end) in &spans {
                let span_width = end.saturating_sub(start);
                if span_width == 0 {
                    continue;
                }
                // One walk per span, from that span's first cell. A gap in the *same* cell's
                // sequence is invisible to the host: placeholders are a consecutive run, and
                // jumping the cursor mid-walk never starts a second one, which left everything
                // to the right of a modal black.
                symbol.clear();
                if let Some(sequence) = transmit.take() {
                    symbol.push_str(&sequence);
                }
                let col = (i32::from(start) - origin_x) as u16;
                let right = span_width.saturating_sub(1);
                let _ = write!(
                    symbol,
                    "\x1b[s{}\u{10EEEE}{}{}{}",
                    self.id_color,
                    crate::widgets::kitty_diacritic(y),
                    crate::widgets::kitty_diacritic(col),
                    crate::widgets::kitty_diacritic(self.id_extra),
                );
                for _ in 1..span_width {
                    symbol.push(crate::widgets::KITTY_PLACEHOLDER);
                }
                let _ = write!(symbol, "\x1b[u\x1b[{right}C");

                for x in start.saturating_add(1)..end {
                    if let Some(cell) = f.buffer_mut().cell_mut((x, row_y)) {
                        cell.set_diff_option(CellDiffOption::Skip);
                    }
                }
                if let Some(cell) = f.buffer_mut().cell_mut((start, row_y)) {
                    cell.set_symbol(&symbol).set_diff_option(UNIT_WIDTH);
                }
                IMAGE_PLACEHOLDERS_PAINTED.set(true);
                painted_placeholders = true;
            }
        }
        if painted_placeholders {
            PAINTED_IMAGE_OCCLUSIONS.with(|slot| {
                let mut painted = slot.borrow_mut();
                for hole in holes {
                    let intersection = area.intersection(hole);
                    if intersection.width > 0
                        && intersection.height > 0
                        && !painted.contains(&intersection)
                    {
                        painted.push(intersection);
                    }
                }
            });
        }
    }

    fn transmission_pending(&self) -> bool {
        self.transmit
            .lock()
            .is_ok_and(|transmission| transmission.is_some())
    }

    fn take_transmission(&self) -> Option<String> {
        let sequence = self
            .transmit
            .lock()
            .ok()
            .and_then(|mut sequence| sequence.take())?;
        // Written into this frame's buffer, so the host is about to be told the name and reading is
        // what unlinks the object. Until this point it was this process's to clean up.
        if let Ok(mut shared) = self.shared.lock()
            && let Some(frame) = shared.as_mut()
        {
            frame.handed_over();
        }
        Some(sequence)
    }
}

/// Pixels in shared memory, when that is a medium the host said it can read.
#[cfg(feature = "terminal-images")]
fn shared_frame(pixels: &[u8]) -> Option<SharedFrame> {
    if !shared_frame::host_reads_shared_memory() {
        return None;
    }
    // A name only means something to a terminal reading this machine's own shared memory. Under
    // tmux the reader is tmux, which passes the sequence through to a terminal that may be anywhere.
    if std::env::var_os("TMUX").is_some() {
        return None;
    }
    SharedFrame::write(pixels)
}

/// A `t=s` transmission: the pixels are in `name`, and this only says where.
#[cfg(all(feature = "terminal-images", test))]
pub(crate) fn kitty_transmit_shared_memory(
    name: &str,
    width: u32,
    height: u32,
    format: u8,
    id: u32,
    cells: ratatui::layout::Size,
) -> String {
    kitty_transmit_shared_memory_at_z(name, width, height, format, id, cells, 0)
}

#[cfg(feature = "terminal-images")]
fn kitty_transmit_shared_memory_at_z(
    name: &str,
    width: u32,
    height: u32,
    format: u8,
    id: u32,
    cells: ratatui::layout::Size,
    z_index: i32,
) -> String {
    let (columns, rows) = (cells.width, cells.height);
    let z_index = if z_index != 0 {
        format!(",z={z_index}")
    } else {
        String::new()
    };
    let mut data = format!(
        "\x1b_Gq=2,i={id},a=T,U=1,f={format},t=s,s={width},v={height},c={columns},r={rows}{z_index};"
    );
    BASE64.encode_string(name, &mut data);
    data.push_str("\x1b\\");
    data
}

#[cfg(feature = "terminal-images")]
fn kitty_transmit_compressed_format(
    payload: &[u8],
    width: u32,
    height: u32,
    format: u8,
    id: u32,
    cells: ratatui::layout::Size,
    z_index: i32,
) -> String {
    kitty_transmit_compressed_format_for(
        payload,
        (width, height),
        format,
        id,
        cells,
        z_index,
        std::env::var_os("TMUX").is_some(),
    )
}

#[cfg(all(feature = "terminal-images", test))]
pub(crate) fn kitty_transmit_compressed_for(
    payload: &[u8],
    width: u32,
    height: u32,
    id: u32,
    cells: ratatui::layout::Size,
    is_tmux: bool,
) -> String {
    kitty_transmit_compressed_format_for(payload, (width, height), 32, id, cells, 0, is_tmux)
}

#[cfg(feature = "terminal-images")]
fn kitty_transmit_compressed_format_for(
    payload: &[u8],
    dimensions: (u32, u32),
    format: u8,
    id: u32,
    cells: ratatui::layout::Size,
    z_index: i32,
    is_tmux: bool,
) -> String {
    const CHUNK_BYTES: usize = 3072;

    let (width, height) = dimensions;
    let (start, escape, end) = if is_tmux {
        ("\x1bPtmux;", "\x1b\x1b", "\x1b\\")
    } else {
        ("", "\x1b", "")
    };
    let chunk_count = payload.len().div_ceil(CHUNK_BYTES);
    let mut data = String::with_capacity(payload.len().saturating_mul(3) / 2);

    for (index, chunk) in payload.chunks(CHUNK_BYTES).enumerate() {
        data.push_str(start);
        write!(data, "{escape}_Gq=2,").expect("writing to a String cannot fail");
        if index == 0 {
            let (columns, rows) = (cells.width, cells.height);
            write!(
                data,
                "i={id},a=T,U=1,f={format},o=z,t=d,s={width},v={height},c={columns},r={rows},"
            )
            .expect("writing to a String cannot fail");
            if z_index != 0 {
                write!(data, "z={z_index},").expect("writing to a String cannot fail");
            }
        }
        let more = u8::from(index + 1 < chunk_count);
        write!(data, "m={more};").expect("writing to a String cannot fail");
        BASE64.encode_string(chunk, &mut data);
        write!(data, "{escape}\\").expect("writing to a String cannot fail");
        data.push_str(end);
    }
    data
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct RenderCacheKey {
    source_hash: u64,
    frame_index: usize,
    /// Encoded size in cells: the visible part when `crop` is set.
    width: u16,
    height: u16,
    /// The visible cells of an image partly scrolled out of view.
    crop: Option<CellCrop>,
    background_rgb: Option<(u8, u8, u8)>,
    fit: ImageFit,
    protocol: ImageProtocol,
    resolved_protocol: ImageProtocol,
    /// Kitty placement depth. It is part of the encoding because the host owns compositing.
    z_index: i32,
    /// [`BackdropMask::key`] of the backdrops dimming the pixels, or `0` for the pixels as they are.
    backdrop: u64,
}

/// Which cells of an image laid out at `full_width` x `full_height` are visible: `width` x
/// `height` cells of the key, starting at `x`, `y`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct CellCrop {
    x: u16,
    y: u16,
    full_width: u16,
    full_height: u16,
}

struct CacheEntry {
    stream_key: u64,
    key: RenderCacheKey,
    protocol: Arc<EncodedProtocol>,
    estimated_bytes: usize,
    last_used: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(feature = "terminal-images"), allow(dead_code))]
enum CacheRetention {
    Variants,
    LatestOnly,
}

#[derive(Clone)]
struct EncodeRequest {
    /// Stable identity of the thing being drawn, independent of its current pixels.
    ///
    /// Animated images and terminal applications replace their pixels repeatedly. Queueing by this
    /// key lets a newer frame supersede an older one that has not started encoding yet.
    stream_key: u64,
    key: RenderCacheKey,
    image: Arc<image::DynamicImage>,
    estimated_bytes: usize,
    retention: CacheRetention,
    /// Backdrops to recolor `image` under before it is encoded. Applied by the encoder, so a
    /// cache hit never pays for it.
    backdrop: Option<Arc<BackdropMask>>,
}

impl EncodeRequest {
    fn new(
        stream_key: u64,
        key: RenderCacheKey,
        image: Arc<image::DynamicImage>,
        retention: CacheRetention,
    ) -> Self {
        let estimated_bytes = estimate_protocol_bytes_for_request(key, image.as_ref());
        Self {
            stream_key,
            key,
            image,
            estimated_bytes,
            retention,
            backdrop: None,
        }
    }

    /// Dim the pixels under `backdrop`, as a separate cache entry from the undimmed ones.
    fn with_backdrop(mut self, backdrop: Option<Arc<BackdropMask>>) -> Self {
        self.key.backdrop = backdrop.as_ref().map_or(0, |mask| mask.key());
        self.backdrop = backdrop;
        self
    }

    /// The queue slot a newer request replaces this one in.
    ///
    /// A terminal stream draws one placement, so its newest request always wins. Image widgets
    /// share a stream per source, and two widgets showing it at once - one under a backdrop and one
    /// above it, or at different sizes - must not keep replacing each other's queued work. Their
    /// frames still collapse within one variant.
    fn queue_slot(&self) -> u64 {
        use std::hash::{Hash as _, Hasher as _};

        match self.retention {
            CacheRetention::LatestOnly => self.stream_key,
            CacheRetention::Variants => {
                let key = &self.key;
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                self.stream_key.hash(&mut hasher);
                (
                    key.width,
                    key.height,
                    key.crop,
                    key.background_rgb,
                    key.fit,
                    key.protocol,
                    key.resolved_protocol,
                    key.z_index,
                    key.backdrop,
                )
                    .hash(&mut hasher);
                hasher.finish()
            }
        }
    }
}

/// An earlier encode of the same placement, drawn while the requested one encodes.
struct StandIn {
    protocol: Arc<EncodedProtocol>,
    /// Whether it is dimmed like the request. When it is not, drawing it would leave the picture
    /// dimmed after its backdrop has gone, or undimmed under one.
    same_backdrop: bool,
}

#[derive(Default)]
struct ImageRenderCache {
    entries: Vec<CacheEntry>,
    total_estimated_bytes: usize,
}

impl ImageRenderCache {
    fn get(&mut self, key: &RenderCacheKey) -> Option<Arc<EncodedProtocol>> {
        let idx = self.entries.iter().position(|entry| &entry.key == key)?;
        let mut entry = self.entries.remove(idx);
        let retained_bytes = entry
            .protocol
            .retained_estimated_bytes(entry.estimated_bytes);
        self.total_estimated_bytes = self
            .total_estimated_bytes
            .saturating_sub(entry.estimated_bytes)
            .saturating_add(retained_bytes);
        entry.estimated_bytes = retained_bytes;
        entry.last_used = Instant::now();
        let protocol = Arc::clone(&entry.protocol);
        self.entries.push(entry);
        Some(protocol)
    }

    /// The newest entry that can stand in for `key` while it encodes, preferring one dimmed the same
    /// way.
    fn get_latest_compatible(&mut self, stream_key: u64, key: &RenderCacheKey) -> Option<StandIn> {
        let compatible = |entry: &CacheEntry| {
            entry.key != *key
                && stream_encoding_compatible(entry.stream_key, &entry.key, stream_key, key)
        };
        let idx = self
            .entries
            .iter()
            .rposition(|entry| compatible(entry) && entry.key.backdrop == key.backdrop)
            .or_else(|| self.entries.iter().rposition(compatible))?;

        let mut entry = self.entries.remove(idx);
        let same_backdrop = entry.key.backdrop == key.backdrop;
        let retained_bytes = entry
            .protocol
            .retained_estimated_bytes(entry.estimated_bytes);
        self.total_estimated_bytes = self
            .total_estimated_bytes
            .saturating_sub(entry.estimated_bytes)
            .saturating_add(retained_bytes);
        entry.estimated_bytes = retained_bytes;
        entry.last_used = Instant::now();
        let protocol = Arc::clone(&entry.protocol);
        self.entries.push(entry);
        Some(StandIn {
            protocol,
            same_backdrop,
        })
    }

    fn remove_at(&mut self, idx: usize) {
        if idx >= self.entries.len() {
            return;
        }
        let removed = self.entries.remove(idx);
        self.total_estimated_bytes = self
            .total_estimated_bytes
            .saturating_sub(removed.estimated_bytes);
    }

    fn insert(
        &mut self,
        stream_key: u64,
        key: RenderCacheKey,
        protocol: Arc<EncodedProtocol>,
        estimated_bytes: usize,
        retention: CacheRetention,
    ) {
        const MAX_ENTRIES: usize = 256;
        // Keep one already-presented frame behind the newest encode so native protocols can load
        // the replacement without blanking the placement. Its compressed payload is released
        // after transmission and discounted from the live cache budget.
        const MAX_ENTRIES_PER_STREAM: usize = 2;
        const MAX_VARIANTS_PER_STREAM: usize = 24;
        const MAX_TOTAL_ESTIMATED_BYTES: usize = 24 * 1024 * 1024;

        if let Some(idx) = self.entries.iter().position(|entry| entry.key == key) {
            self.remove_at(idx);
        }

        let max_entries_for_stream = match retention {
            CacheRetention::LatestOnly => MAX_ENTRIES_PER_STREAM,
            CacheRetention::Variants => MAX_VARIANTS_PER_STREAM,
        };
        if matches!(retention, CacheRetention::LatestOnly)
            && let Some(idx) = self.entries.iter().position(|entry| {
                entry.stream_key == stream_key && entry.protocol.transmission_pending()
            })
        {
            // A newer completed encode supersedes an unpresented predecessor. Preserve the last
            // host-ready frame instead; it is the bridge that prevents a blank during preload.
            self.remove_at(idx);
        }
        while self
            .entries
            .iter()
            .filter(|entry| entry.stream_key == stream_key)
            .count()
            >= max_entries_for_stream
        {
            let oldest_same_stream = self
                .entries
                .iter()
                .position(|entry| entry.stream_key == stream_key);
            if let Some(idx) = oldest_same_stream {
                self.remove_at(idx);
            } else {
                break;
            }
        }

        self.entries.push(CacheEntry {
            stream_key,
            key,
            protocol,
            estimated_bytes,
            last_used: Instant::now(),
        });
        self.total_estimated_bytes = self.total_estimated_bytes.saturating_add(estimated_bytes);

        while self.entries.len() > 1
            && (self.entries.len() > MAX_ENTRIES
                || self.total_estimated_bytes > MAX_TOTAL_ESTIMATED_BYTES)
        {
            self.remove_at(0);
        }
    }

    fn evict_expired(&mut self, now: Instant) {
        const IDLE_TTL: Duration = Duration::from_secs(30);

        while let Some(idx) = self
            .entries
            .iter()
            .position(|entry| now.saturating_duration_since(entry.last_used) >= IDLE_TTL)
        {
            self.remove_at(idx);
        }
    }
}

fn stream_encoding_compatible(
    cached_stream: u64,
    cached: &RenderCacheKey,
    requested_stream: u64,
    requested: &RenderCacheKey,
) -> bool {
    // `backdrop` is left out on purpose: [`AsyncEncoder::resolve_miss`] decides whether a stand-in
    // dimmed the other way may be drawn.
    cached_stream == requested_stream
        && cached.width == requested.width
        && cached.height == requested.height
        && cached.crop == requested.crop
        && cached.background_rgb == requested.background_rgb
        && cached.fit == requested.fit
        && cached.protocol == requested.protocol
        && cached.resolved_protocol == requested.resolved_protocol
        && cached.z_index == requested.z_index
}

#[derive(Default)]
struct AsyncEncoderInner {
    cache: ImageRenderCache,
    /// [`EncodeRequest::queue_slot`]s waiting for a worker, oldest first.
    queue: VecDeque<u64>,
    queued: HashMap<u64, EncodeRequest>,
    in_flight: HashSet<u64>,
    in_flight_keys: HashMap<u64, RenderCacheKey>,
}

struct AsyncEncoder {
    inner: Mutex<AsyncEncoderInner>,
    wake: Condvar,
}

impl Default for AsyncEncoder {
    fn default() -> Self {
        Self {
            inner: Mutex::new(AsyncEncoderInner::default()),
            wake: Condvar::new(),
        }
    }
}

impl AsyncEncoder {
    fn cache_get(&self, key: &RenderCacheKey) -> Option<Arc<EncodedProtocol>> {
        let Ok(mut inner) = self.inner.lock() else {
            return None;
        };
        inner.cache.get(key)
    }

    fn encode_synchronously(&self, request: EncodeRequest) -> Option<Arc<EncodedProtocol>> {
        let protocol = Arc::new(encode_request(&request)?);
        let Ok(mut inner) = self.inner.lock() else {
            return Some(protocol);
        };
        inner.cache.insert(
            request.stream_key,
            request.key,
            Arc::clone(&protocol),
            request.estimated_bytes,
            request.retention,
        );
        Some(protocol)
    }

    fn cache_get_latest_compatible(
        &self,
        stream_key: u64,
        key: &RenderCacheKey,
    ) -> Option<StandIn> {
        let Ok(mut inner) = self.inner.lock() else {
            return None;
        };
        inner.cache.get_latest_compatible(stream_key, key)
    }

    /// Resolve a request the cache missed: encode it now when `synchronous`, otherwise queue it and
    /// return a stand-in.
    ///
    /// A stand-in dimmed the other way is not drawn. An overlay that opens or closes would otherwise
    /// leave the picture wrongly dimmed until the worker finishes, so the request encodes now; this
    /// happens once per backdrop change, not per frame.
    fn resolve_miss(&self, request: EncodeRequest, synchronous: bool) -> ProtocolResolve {
        let stand_in = self.cache_get_latest_compatible(request.stream_key, &request.key);
        if synchronous
            || stand_in
                .as_ref()
                .is_some_and(|stand_in| !stand_in.same_backdrop)
        {
            return synchronous_resolve(self.encode_synchronously(request), stand_in);
        }

        self.enqueue(request);
        stand_in.map_or(ProtocolResolve::Pending, |stand_in| {
            ProtocolResolve::Stale(stand_in.protocol)
        })
    }

    fn enqueue(&self, request: EncodeRequest) {
        const MAX_QUEUED_SOURCES: usize = 48;

        let Ok(mut inner) = self.inner.lock() else {
            return;
        };

        let slot = request.queue_slot();

        if inner
            .in_flight_keys
            .get(&slot)
            .is_some_and(|key| *key == request.key)
        {
            return;
        }

        if inner
            .queued
            .get(&slot)
            .is_some_and(|existing| existing.key == request.key)
        {
            return;
        }

        let inserted_new = inner.queued.insert(slot, request).is_none();
        if !inserted_new {
            // The queue already contains this slot. Its map entry now holds the newest frame,
            // while its one position in `queue` is intentionally retained.
            return;
        }

        while inner.queue.len() >= MAX_QUEUED_SOURCES {
            let Some(evicted_slot) = inner.queue.pop_front() else {
                break;
            };
            inner.queued.remove(&evicted_slot);
        }

        inner.queue.push_back(slot);
        self.wake.notify_one();
    }

    fn next_request_blocking(&self) -> EncodeRequest {
        const CACHE_SWEEP_INTERVAL: Duration = Duration::from_secs(5);

        let mut inner = self
            .inner
            .lock()
            .expect("image async encoder lock poisoned");

        loop {
            inner.cache.evict_expired(Instant::now());
            let queued_count = inner.queue.len();
            for _ in 0..queued_count {
                let Some(slot) = inner.queue.pop_front() else {
                    break;
                };
                if inner.in_flight.contains(&slot) {
                    inner.queue.push_back(slot);
                    continue;
                }
                let Some(request) = inner.queued.remove(&slot) else {
                    continue;
                };

                inner.in_flight.insert(slot);
                inner.in_flight_keys.insert(slot, request.key);
                return request;
            }

            let (next_inner, _) = self
                .wake
                .wait_timeout(inner, CACHE_SWEEP_INTERVAL)
                .expect("image async encoder lock poisoned");
            inner = next_inner;
            inner.cache.evict_expired(Instant::now());
        }
    }

    fn complete_request(&self, request: &EncodeRequest, protocol: Option<EncodedProtocol>) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };

        let slot = request.queue_slot();
        inner.in_flight.remove(&slot);
        inner.in_flight_keys.remove(&slot);
        self.wake.notify_all();

        let Some(protocol) = protocol else {
            return;
        };

        inner.cache.insert(
            request.stream_key,
            request.key,
            Arc::new(protocol),
            request.estimated_bytes,
            request.retention,
        );
        protocol_ready_epoch_counter().fetch_add(1, Ordering::Relaxed);
    }
}

/// What to draw after a synchronous encode. A failed one falls back to the stand-in only when it is
/// dimmed like the request; the wrong dimming is never drawn, even as a fallback.
fn synchronous_resolve(
    encoded: Option<Arc<EncodedProtocol>>,
    stand_in: Option<StandIn>,
) -> ProtocolResolve {
    match (encoded, stand_in) {
        (Some(protocol), _) => ProtocolResolve::Ready(protocol),
        (None, Some(stand_in)) if stand_in.same_backdrop => {
            ProtocolResolve::Stale(stand_in.protocol)
        }
        (None, _) => ProtocolResolve::Unavailable,
    }
}

fn protocol_ready_epoch_counter() -> &'static AtomicU64 {
    static EPOCH: OnceLock<AtomicU64> = OnceLock::new();
    EPOCH.get_or_init(|| AtomicU64::new(0))
}

pub(crate) fn image_protocol_ready_epoch() -> u64 {
    protocol_ready_epoch_counter().load(Ordering::Relaxed)
}

fn async_encoder() -> &'static Arc<AsyncEncoder> {
    static ENCODER: OnceLock<Arc<AsyncEncoder>> = OnceLock::new();
    ENCODER.get_or_init(|| {
        let encoder = Arc::new(AsyncEncoder::default());
        let worker_count = image_encode_worker_count();

        for idx in 0..worker_count {
            let worker_encoder = Arc::clone(&encoder);
            let worker_name = format!("image-protocol-encoder-{idx}");
            let _ = thread::Builder::new().name(worker_name).spawn(move || {
                loop {
                    let request = worker_encoder.next_request_blocking();
                    let protocol = encode_request(&request);
                    worker_encoder.complete_request(&request, protocol);
                }
            });
        }

        encoder
    })
}

/// How much bigger than its box a picture may be before scaling it here beats leaving it to the host.
///
/// A Kitty transmission names the cell box it is to fill, and the terminal scales into it on the
/// GPU for nothing. Doing the same work here means resampling every pixel of every frame, which for
/// a child redrawing its whole window is the entire frame budget - nine milliseconds against two
/// tenths. What the trade turns on is how much bigger the source is: a window drawn at twice the
/// cell resolution is worth passing on whole, while a twelve-megapixel photograph shown in a corner
/// would mean transmitting all of it for the host to discard almost all of it.
#[cfg(feature = "terminal-images")]
const HOST_SCALE_MAX_OVERSAMPLE: u32 = 2;

/// Whether the host can be left to scale this image into its cell box.
///
/// Only for the fits that mean "the whole picture, shrunk to taste": a crop has to choose which
/// pixels to keep, and choosing is not something the box dimensions can express.
#[cfg(feature = "terminal-images")]
fn host_scales_into_cells(
    fit: ImageFit,
    image: &image::DynamicImage,
    pixel_width: u32,
    pixel_height: u32,
) -> bool {
    if !matches!(fit, ImageFit::Contain | ImageFit::Scale) {
        return false;
    }
    if pixel_width == 0 || pixel_height == 0 {
        return false;
    }
    image.width() <= pixel_width.saturating_mul(HOST_SCALE_MAX_OVERSAMPLE)
        && image.height() <= pixel_height.saturating_mul(HOST_SCALE_MAX_OVERSAMPLE)
}

fn fit_to_resize(fit: ImageFit) -> Resize {
    match fit {
        ImageFit::Contain => Resize::Fit(None),
        ImageFit::Crop => Resize::Crop(None),
        ImageFit::Scale => Resize::Scale(None),
        // `encode_request` scales and crops a cover to its box's exact pixel size first, so what
        // reaches ratatui-image already fits.
        ImageFit::Cover => Resize::Fit(None),
    }
}

/// Scale `image` to cover `pixel_width` x `pixel_height`, keeping aspect ratio, and crop the
/// overflow evenly from both sides.
fn cover_image(
    image: &image::DynamicImage,
    pixel_width: u32,
    pixel_height: u32,
) -> image::DynamicImage {
    image.resize_to_fill(
        pixel_width.max(1),
        pixel_height.max(1),
        image::imageops::FilterType::Triangle,
    )
}

fn protocol_type_to_public(protocol: ProtocolType) -> ImageProtocol {
    match protocol {
        ProtocolType::Halfblocks => ImageProtocol::Halfblocks,
        ProtocolType::Sixel => ImageProtocol::Sixel,
        ProtocolType::Kitty => ImageProtocol::Kitty,
        ProtocolType::Iterm2 => ImageProtocol::Iterm2,
    }
}

fn requested_protocol_type(protocol: ImageProtocol) -> Option<ProtocolType> {
    match protocol {
        ImageProtocol::Auto => None,
        ImageProtocol::Kitty => Some(ProtocolType::Kitty),
        ImageProtocol::Iterm2 => Some(ProtocolType::Iterm2),
        ImageProtocol::Sixel => Some(ProtocolType::Sixel),
        ImageProtocol::Halfblocks => Some(ProtocolType::Halfblocks),
    }
}

fn resolved_protocol_type(protocol: ImageProtocol) -> Option<ProtocolType> {
    match protocol {
        ImageProtocol::Kitty => Some(ProtocolType::Kitty),
        ImageProtocol::Iterm2 => Some(ProtocolType::Iterm2),
        ImageProtocol::Sixel => Some(ProtocolType::Sixel),
        ImageProtocol::Halfblocks => Some(ProtocolType::Halfblocks),
        ImageProtocol::Auto => None,
    }
}

fn image_encode_worker_count() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("TUI_LIPAN_IMAGE_ENCODE_WORKERS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, 2)
    })
}

fn estimate_protocol_bytes_for_request(key: RenderCacheKey, image: &image::DynamicImage) -> usize {
    estimate_protocol_bytes_at_font(key, image, image_support::picker_snapshot().font_size())
}

fn estimate_protocol_bytes_at_font(
    key: RenderCacheKey,
    image: &image::DynamicImage,
    font_size: ratatui_image::FontSize,
) -> usize {
    let available = ratatui::layout::Size::new(key.width, key.height);
    let encoded_size = fit_to_resize(key.fit).size_for(image, font_size, available);
    let cells = usize::from(encoded_size.width).saturating_mul(usize::from(encoded_size.height));
    let pixels = cells
        .saturating_mul(usize::from(font_size.width))
        .saturating_mul(usize::from(font_size.height));
    let rgba_bytes = pixels.saturating_mul(4);

    match key.resolved_protocol {
        // Halfblocks retain one pair of colors plus a character for each cell.
        ImageProtocol::Halfblocks => cells.saturating_mul(32),
        // Kitty and iTerm2 retain a base64-encoded pixel payload. The extra margin covers protocol
        // framing and rounding without pretending that a multi-megabyte image costs a few bytes
        // per terminal cell.
        ImageProtocol::Kitty | ImageProtocol::Iterm2 => rgba_bytes.saturating_mul(3) / 2,
        // Sixel size varies with image complexity and can exceed the raw pixel count.
        ImageProtocol::Sixel | ImageProtocol::Auto => rgba_bytes.saturating_mul(2),
    }
}

fn protocol_requires_background_flatten(protocol: ImageProtocol) -> bool {
    matches!(protocol, ImageProtocol::Halfblocks | ImageProtocol::Sixel)
}

fn indexed_to_rgb(index: u8) -> (u8, u8, u8) {
    const ANSI16: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];

    if index < 16 {
        return ANSI16[index as usize];
    }
    if index >= 232 {
        let gray = 8u8.saturating_add((index - 232).saturating_mul(10));
        return (gray, gray, gray);
    }

    let idx = index - 16;
    let r = idx / 36;
    let g = (idx % 36) / 6;
    let b = idx % 6;
    let to_level = |v: u8| match v {
        0 => 0,
        1 => 95,
        2 => 135,
        3 => 175,
        4 => 215,
        _ => 255,
    };
    (to_level(r), to_level(g), to_level(b))
}

fn ratatui_color_to_rgb(color: ratatui::style::Color) -> Option<(u8, u8, u8)> {
    use ratatui::style::Color;

    match color {
        Color::Reset => None,
        Color::Black => Some((0, 0, 0)),
        Color::Red => Some((205, 0, 0)),
        Color::Green => Some((0, 205, 0)),
        Color::Yellow => Some((205, 205, 0)),
        Color::Blue => Some((0, 0, 238)),
        Color::Magenta => Some((205, 0, 205)),
        Color::Cyan => Some((0, 205, 205)),
        Color::Gray => Some((229, 229, 229)),
        Color::DarkGray => Some((127, 127, 127)),
        Color::LightRed => Some((255, 0, 0)),
        Color::LightGreen => Some((0, 255, 0)),
        Color::LightYellow => Some((255, 255, 0)),
        Color::LightBlue => Some((92, 92, 255)),
        Color::LightMagenta => Some((255, 0, 255)),
        Color::LightCyan => Some((0, 255, 255)),
        Color::White => Some((255, 255, 255)),
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(idx) => Some(indexed_to_rgb(idx)),
    }
}

fn sample_background_rgb(f: &mut ratatui::Frame<'_>, draw_rect: Rect) -> Option<(u8, u8, u8)> {
    if draw_rect.is_empty() {
        return None;
    }

    let x = draw_rect.x.max(0) as u16;
    let y = draw_rect.y.max(0) as u16;
    let color = {
        let buf = f.buffer_mut();
        buf.cell((x, y)).map(|cell| cell.bg)
    }?;

    ratatui_color_to_rgb(color)
}

fn fit_pixels_proportionally(
    width: u32,
    height: u32,
    max_width: u32,
    max_height: u32,
) -> (u32, u32) {
    if width == 0 || height == 0 || max_width == 0 || max_height == 0 {
        return (0, 0);
    }

    let wratio = max_width as f64 / width as f64;
    let hratio = max_height as f64 / height as f64;
    let ratio = wratio.min(hratio);

    let new_w = ((width as f64) * ratio).round().max(1.0) as u32;
    let new_h = ((height as f64) * ratio).round().max(1.0) as u32;
    (new_w, new_h)
}

fn resolve_image_render_rect(node: &ImageNode, bounds: Rect) -> Rect {
    let Some(image) = node.current_image() else {
        return bounds;
    };
    if bounds.is_empty() {
        return bounds;
    }

    let picker = image_support::picker_snapshot();
    let font_size = picker.font_size();
    let cell_w = u32::from(font_size.width.max(1));
    let cell_h = u32::from(font_size.height.max(1));

    let image_w = image.width();
    let image_h = image.height();
    if image_w == 0 || image_h == 0 {
        return bounds;
    }

    let max_w_px = u32::from(bounds.w).saturating_mul(cell_w);
    let max_h_px = u32::from(bounds.h).saturating_mul(cell_h);

    let (target_w_px, target_h_px) = match node.fit {
        ImageFit::Contain => {
            let cap_w = max_w_px.min(image_w);
            let cap_h = max_h_px.min(image_h);
            fit_pixels_proportionally(image_w, image_h, cap_w, cap_h)
        }
        ImageFit::Scale => fit_pixels_proportionally(image_w, image_h, max_w_px, max_h_px),
        ImageFit::Crop => (image_w.min(max_w_px), image_h.min(max_h_px)),
        ImageFit::Cover => (max_w_px, max_h_px),
    };

    let target_w_cells = target_w_px.div_ceil(cell_w).max(1).min(u32::from(bounds.w)) as u16;
    let target_h_cells = target_h_px.div_ceil(cell_h).max(1).min(u32::from(bounds.h)) as u16;

    // A picture scaled to fit sits in the middle of the room it does not fill, like CSS
    // `object-fit: contain`. A crop keeps the top-left pixels it cut, so it stays at the origin.
    let (x, y) = match node.fit {
        ImageFit::Contain | ImageFit::Scale => (
            bounds.x + ((bounds.w - target_w_cells) / 2) as i16,
            bounds.y + ((bounds.h - target_h_cells) / 2) as i16,
        ),
        ImageFit::Crop | ImageFit::Cover => (bounds.x, bounds.y),
    };
    Rect {
        x,
        y,
        w: target_w_cells,
        h: target_h_cells,
    }
}

fn clear_image_region(f: &mut ratatui::Frame<'_>, draw_rect: Rect, style: ratatui::style::Style) {
    let area = to_ratatui_rect(draw_rect);
    f.render_widget(Clear, area);
    if style.bg.is_some_and(|c| c != ratatui::style::Color::Reset) {
        f.render_widget(Block::default().style(style), area);
    }
}

use super::super::common::{render_placeholder_frame, render_placeholder_frame_clipped};

fn build_encode_request(
    node: &ImageNode,
    draw_rect: Rect,
    background_rgb: Option<(u8, u8, u8)>,
) -> Option<EncodeRequest> {
    let decoded = node.current_image()?;
    if draw_rect.w == 0 || draw_rect.h == 0 {
        return None;
    }

    let mut picker = image_support::picker_snapshot();
    let requested_protocol = requested_protocol_type(node.protocol);

    if let Some(protocol_type) = requested_protocol {
        picker.set_protocol_type(protocol_type);
    }
    let resolved = protocol_type_to_public(picker.protocol_type());
    let effective_background_rgb = if protocol_requires_background_flatten(resolved) {
        background_rgb
    } else {
        None
    };
    if let Some((r, g, b)) = effective_background_rgb {
        picker.set_background_color(Some(image::Rgba([r, g, b, 255])));
    }
    let key = RenderCacheKey {
        source_hash: node.source_hash,
        frame_index: node.current_frame_index(),
        width: draw_rect.w,
        height: draw_rect.h,
        crop: None,
        background_rgb: effective_background_rgb,
        fit: node.fit,
        protocol: node.protocol,
        resolved_protocol: resolved,
        z_index: 0,
        backdrop: 0,
    };

    Some(
        EncodeRequest::new(node.source_hash, key, decoded, CacheRetention::Variants)
            .with_backdrop(live_backdrop_mask(draw_rect, resolved)),
    )
}

/// The backdrops to dim an image's pixels under before encoding it for `protocol`.
///
/// Half blocks are cells, which the backdrop dims itself; dimming their pixels too would dim them
/// twice.
fn live_backdrop_mask(area: Rect, protocol: ImageProtocol) -> Option<Arc<BackdropMask>> {
    if matches!(protocol, ImageProtocol::Halfblocks) {
        return None;
    }
    backdrop_mask_for(area)
}

#[cfg(feature = "terminal-images")]
fn kitty_image_id(request: &EncodeRequest) -> u32 {
    use std::hash::{Hash as _, Hasher as _};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    match request.retention {
        CacheRetention::LatestOnly => {
            // Terminal placements replace their pixels continuously. Keep their host image id
            // stable so native terminals replace one image instead of allocating a new image and
            // repainting differently-colored Unicode placeholders for every producer frame.
            b"tui-lipan-terminal-image-stream".hash(&mut hasher);
            request.stream_key.hash(&mut hasher);
            // The dimmed variant needs its own host image. Sharing the id would overwrite the
            // undimmed pixels on the host, and closing the overlay would switch back to a cached
            // encode that no longer transmits them.
            if request.key.backdrop != 0 {
                request.key.backdrop.hash(&mut hasher);
            }
        }
        CacheRetention::Variants => {
            // Image widgets may render the same source independently at the same size. Preserve
            // the frame-specific identity until those widgets have their own stream namespace.
            request.key.hash(&mut hasher);
        }
    }
    (hasher.finish() as u32).max(1)
}

fn encode_request(request: &EncodeRequest) -> Option<EncodedProtocol> {
    let mut picker = image_support::picker_snapshot();
    if let Some(protocol_type) = resolved_protocol_type(request.key.resolved_protocol) {
        picker.set_protocol_type(protocol_type);
    }
    if let Some((r, g, b)) = request.key.background_rgb {
        picker.set_background_color(Some(image::Rgba([r, g, b, 255])));
    }

    let size = ratatui::layout::Size::new(request.key.width, request.key.height);
    let resize = fit_to_resize(request.key.fit);
    let cell = (
        u32::from(picker.font_size().width),
        u32::from(picker.font_size().height),
    );
    // A crop or a cover is dimmed after it is cut to its box, where pixels land exactly on cells.
    let dim = |image: image::DynamicImage| match &request.backdrop {
        Some(mask) => mask.apply(&image, cell, request.key.background_rgb),
        None => image,
    };
    if let Some(crop) = request.key.crop {
        let cropped = dim(crop_to_visible_cells(request, picker.font_size(), crop));
        #[cfg(feature = "terminal-images")]
        if matches!(request.key.resolved_protocol, ImageProtocol::Kitty) {
            let id = kitty_image_id(request);
            return CompressedKitty::new(&cropped, size, id, request.key.z_index)
                .map(EncodedProtocol::CompressedKitty);
        }
        return picker
            .new_protocol(cropped, size, Resize::Fit(None))
            .map(|protocol| EncodedProtocol::ratatui(protocol, request.key.resolved_protocol))
            .ok();
    }
    if matches!(request.key.fit, ImageFit::Cover) {
        let font_size = picker.font_size();
        let covered = dim(cover_image(
            request.image.as_ref(),
            u32::from(size.width) * u32::from(font_size.width.max(1)),
            u32::from(size.height) * u32::from(font_size.height.max(1)),
        ));
        #[cfg(feature = "terminal-images")]
        if matches!(request.key.resolved_protocol, ImageProtocol::Kitty) {
            let id = kitty_image_id(request);
            return CompressedKitty::new(&covered, size, id, request.key.z_index)
                .map(EncodedProtocol::CompressedKitty);
        }
        return picker
            .new_protocol(covered, size, resize)
            .map(|protocol| EncodedProtocol::ratatui(protocol, request.key.resolved_protocol))
            .ok();
    }
    let dimmed = request
        .backdrop
        .as_ref()
        .map(|mask| mask.apply(&request.image, cell, request.key.background_rgb));
    let source = dimmed.as_ref().unwrap_or(request.image.as_ref());
    #[cfg(feature = "terminal-images")]
    if matches!(request.key.resolved_protocol, ImageProtocol::Kitty) {
        let encoded_size = resize.size_for(source, picker.font_size(), size);
        let pixel_width = u32::from(encoded_size.width) * u32::from(picker.font_size().width);
        let pixel_height = u32::from(encoded_size.height) * u32::from(picker.font_size().height);
        let background = request
            .key
            .background_rgb
            .map(|(r, g, b)| image::Rgba([r, g, b, 255]));
        let resized = (!host_scales_into_cells(request.key.fit, source, pixel_width, pixel_height)
            && (source.width() != pixel_width || source.height() != pixel_height))
            .then(|| resize.resize(source, picker.font_size(), encoded_size, background));
        let image = resized.as_ref().unwrap_or(source);
        let id = kitty_image_id(request);
        return CompressedKitty::new(image, encoded_size, id, request.key.z_index)
            .map(EncodedProtocol::CompressedKitty);
    }

    if matches!(request.key.fit, ImageFit::Scale) {
        let encoded_size = resize.size_for(source, picker.font_size(), size);
        let background = request
            .key
            .background_rgb
            .map(|(r, g, b)| image::Rgba([r, g, b, 255]));
        let resized = resize.resize(source, picker.font_size(), encoded_size, background);
        picker
            .new_protocol(resized, encoded_size, Resize::Fit(None))
            .map(|protocol| EncodedProtocol::ratatui(protocol, request.key.resolved_protocol))
            .ok()
    } else {
        picker
            .new_protocol(source.clone(), size, resize)
            .map(|protocol| EncodedProtocol::ratatui(protocol, request.key.resolved_protocol))
            .ok()
    }
}

/// The pixels of the cells `crop` leaves visible, from the image drawn the way an unclipped encode
/// would lay it out over `crop.full_width` x `crop.full_height` cells. Cells the fitted image does
/// not reach stay transparent, or the flattening background for protocols without alpha.
fn crop_to_visible_cells(
    request: &EncodeRequest,
    font_size: ratatui_image::FontSize,
    crop: CellCrop,
) -> image::DynamicImage {
    let cell_w = u32::from(font_size.width.max(1));
    let cell_h = u32::from(font_size.height.max(1));
    let full = ratatui::layout::Size::new(crop.full_width, crop.full_height);
    let background = request
        .key
        .background_rgb
        .map_or(image::Rgba([0, 0, 0, 0]), |(r, g, b)| {
            image::Rgba([r, g, b, 255])
        });

    let fitted = match request.key.fit {
        ImageFit::Cover => cover_image(
            request.image.as_ref(),
            u32::from(full.width) * cell_w,
            u32::from(full.height) * cell_h,
        ),
        fit => {
            let resize = fit_to_resize(fit);
            let encoded = resize.size_for(request.image.as_ref(), font_size, full);
            resize.resize(request.image.as_ref(), font_size, encoded, Some(background))
        }
    };

    let mut canvas = image::RgbaImage::from_pixel(
        u32::from(full.width) * cell_w,
        u32::from(full.height) * cell_h,
        background,
    );
    image::imageops::overlay(&mut canvas, &fitted.to_rgba8(), 0, 0);
    image::DynamicImage::ImageRgba8(canvas).crop_imm(
        u32::from(crop.x) * cell_w,
        u32::from(crop.y) * cell_h,
        u32::from(request.key.width) * cell_w,
        u32::from(request.key.height) * cell_h,
    )
}

enum ProtocolResolve {
    Ready(Arc<EncodedProtocol>),
    Stale(Arc<EncodedProtocol>),
    Pending,
    Unavailable,
}

fn resolve_protocol_async(
    node: &ImageNode,
    draw_rect: Rect,
    background_rgb: Option<(u8, u8, u8)>,
) -> ProtocolResolve {
    let Some(request) = build_encode_request(node, draw_rect, background_rgb) else {
        return ProtocolResolve::Unavailable;
    };

    let encoder = async_encoder();
    if let Some(protocol) = encoder.cache_get(&request.key) {
        return ProtocolResolve::Ready(protocol);
    }
    encoder.resolve_miss(request, false)
}

/// Resolve an encoded protocol for pixels the caller already holds.
///
/// Terminal panes come through here rather than through an [`ImageNode`]: their pixels arrive as
/// the child program's own graphics escapes, but the encode queue, worker pool, and cache are the
/// ones the [`Image`](crate::widgets::Image) widget already uses, so a pane full of plots competes
/// for that budget instead of standing up a second one beside it.
///
/// `stream_key` identifies the placement across changing frames. `source_hash` must cover
/// everything about the current pixels, cropping included. `pixels` is called only on a miss,
/// which is what keeps a cropped placement from re-cropping on every frame once its encode has
/// landed.
///
/// Nothing is drawn while the stream's first encode is still running. After that, the last encoded
/// frame remains visible until its replacement is ready, so a graphics-heavy terminal cannot blink
/// between every producer frame. Returns whether the frame was given something to draw.
#[cfg(feature = "terminal-images")]
pub(crate) fn draw_encoded_image(
    f: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    stream_key: u64,
    source_hash: u64,
    z_index: i32,
    pixels: impl FnOnce() -> Arc<image::DynamicImage>,
) -> bool {
    if area.width == 0 || area.height == 0 {
        return false;
    }
    let cells = Rect {
        x: area.x as i16,
        y: area.y as i16,
        w: area.width,
        h: area.height,
    };
    #[cfg(feature = "terminal-images")]
    if capturing_images() {
        // The capture paints its half-block stand-ins from these pixels after the backdrop has
        // run, so they are dimmed here whatever the host protocol would be.
        let pixels = match backdrop_mask_for(cells) {
            Some(mask) => {
                let font = image_support::picker_snapshot().font_size();
                Arc::new(mask.apply(
                    &pixels(),
                    (u32::from(font.width), u32::from(font.height)),
                    None,
                ))
            }
            None => pixels(),
        };
        record_capture_image(f, area, pixels);
        return true;
    }
    if image_support::image_rendering_suspended() {
        return false;
    }

    let resolved_protocol =
        protocol_type_to_public(image_support::picker_snapshot().protocol_type());
    let backdrop = live_backdrop_mask(cells, resolved_protocol);
    let key = RenderCacheKey {
        source_hash,
        frame_index: 0,
        width: area.width,
        height: area.height,
        crop: None,
        background_rgb: None,
        fit: ImageFit::Scale,
        protocol: ImageProtocol::Auto,
        resolved_protocol,
        z_index,
        backdrop: backdrop.as_ref().map_or(0, |mask| mask.key()),
    };

    let encoder = async_encoder();
    if let Some(protocol) = encoder.cache_get(&key) {
        protocol.render(f, area);
        return true;
    }

    let request = EncodeRequest::new(stream_key, key, pixels(), CacheRetention::LatestOnly)
        .with_backdrop(backdrop);

    // A terminal application has already paced and decoded this frame. Native Kitty encoding is
    // fast enough to finish inside that paint, which avoids coupling visible frame cadence to the
    // worker-completion poll. Other protocols stay asynchronous because their encoders can be much
    // more expensive and do not have Kitty's one-transmission-per-frame replacement semantics.
    let synchronous = matches!(key.resolved_protocol, ImageProtocol::Kitty);
    match encoder.resolve_miss(request, synchronous) {
        ProtocolResolve::Ready(protocol) | ProtocolResolve::Stale(protocol) => {
            protocol.render(f, area);
            true
        }
        ProtocolResolve::Pending | ProtocolResolve::Unavailable => false,
    }
}

pub(crate) fn render_image(
    f: &mut ratatui::Frame<'_>,
    node: &ImageNode,
    theme: &Theme,
    rect: Rect,
    clip_rect: Option<Rect>,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let draw_rect = clip_rect
        .map(|clip| rect.intersection(&clip))
        .unwrap_or(rect);
    if draw_rect.is_empty() {
        return;
    }

    // Always compute the image render rect from the full (unclipped) rect so
    // that the image keeps its natural size.  Using the clipped draw_rect
    // would shrink the image when it is partially scrolled out of a
    // ScrollView.
    let image_rect = resolve_image_render_rect(node, rect);

    // The clip rect cuts into the image when it is partly scrolled out of a ScrollView. Only the
    // visible cells are drawn, cut from the image laid out at its full size.
    let visible_rect = clip_rect.map(|clip| image_rect.intersection(&clip));
    let image_clipped =
        visible_rect.is_some_and(|visible| visible.w < image_rect.w || visible.h < image_rect.h);

    let lipan_style = resolve_base_style(theme, node.style);
    let mut style = to_ratatui_style(lipan_style);
    let background_rgb = lipan_style
        .bg
        .and_then(|paint| paint.color().to_rgb())
        .or_else(|| sample_background_rgb(f, draw_rect));
    if style.bg.is_none()
        && let Some((r, g, b)) = background_rgb
    {
        style.bg = Some(ratatui::style::Color::Rgb(r, g, b));
    }

    if image_support::image_rendering_suspended() {
        clear_image_region(f, draw_rect, style);
        render_placeholder_frame_clipped(f, image_rect, draw_rect, style, None);
        return;
    }

    if node.decode_error.is_some() {
        clear_image_region(f, draw_rect, style);
        render_placeholder_frame_clipped(
            f,
            image_rect,
            draw_rect,
            style,
            Some("image decode error"),
        );
        return;
    }

    if image_clipped {
        let visible = visible_rect.unwrap_or(image_rect);
        if visible.is_empty() {
            return;
        }
        render_clipped_image(
            f,
            node,
            image_rect,
            visible,
            draw_rect,
            style,
            background_rgb,
        );
        return;
    }

    match resolve_protocol_async(node, image_rect, background_rgb) {
        ProtocolResolve::Ready(protocol) | ProtocolResolve::Stale(protocol) => {
            protocol.render(f, to_ratatui_rect(image_rect));
        }
        ProtocolResolve::Pending => {
            clear_image_region(f, draw_rect, style);
            render_placeholder_frame(f, image_rect, style, None);
        }
        ProtocolResolve::Unavailable => {
            clear_image_region(f, draw_rect, style);

            let fallback = node
                .alt
                .as_deref()
                .or(node.decode_error.as_deref())
                .unwrap_or("[image]");
            let line = Line::from(vec![Span::styled(fallback.to_string(), style)]);
            f.render_widget(Paragraph::new(line), to_ratatui_rect(image_rect));
        }
    }
}

/// Draw the `visible` cells of an image laid out over `image_rect`.
///
/// Kitty placeholders clip a full-size encode in place. Every other protocol draws one picture per
/// escape, so the visible cells are cut from the pixels and encoded on their own - synchronously
/// while that stays cheap, so scrolling a thumbnail does not blank it for a frame at every step.
fn render_clipped_image(
    f: &mut ratatui::Frame<'_>,
    node: &ImageNode,
    image_rect: Rect,
    visible: Rect,
    draw_rect: Rect,
    style: ratatui::style::Style,
    background_rgb: Option<(u8, u8, u8)>,
) {
    /// Visible pixels up to which a crop is encoded inside the paint.
    const SYNC_CROP_MAX_PIXELS: u32 = 512 * 512;

    let encoder = async_encoder();
    if let Some(full) = build_encode_request(node, image_rect, background_rgb)
        && let Some(protocol) = encoder.cache_get(&full.key)
        && protocol.render_clipped(f, image_rect, visible)
    {
        return;
    }

    let Some(mut request) = build_encode_request(node, visible, background_rgb) else {
        clear_image_region(f, draw_rect, style);
        return;
    };
    request.key.crop = Some(CellCrop {
        x: (visible.x - image_rect.x) as u16,
        y: (visible.y - image_rect.y) as u16,
        full_width: image_rect.w,
        full_height: image_rect.h,
    });

    let protocol = encoder.cache_get(&request.key).or_else(|| {
        let font_size = image_support::picker_snapshot().font_size();
        let pixels = u32::from(visible.w)
            * u32::from(font_size.width)
            * u32::from(visible.h)
            * u32::from(font_size.height);
        if pixels <= SYNC_CROP_MAX_PIXELS {
            encoder.encode_synchronously(request)
        } else {
            encoder.enqueue(request);
            None
        }
    });
    match protocol {
        Some(protocol) => protocol.render(f, to_ratatui_rect(visible)),
        None => {
            clear_image_region(f, draw_rect, style);
            render_placeholder_frame_clipped(f, image_rect, draw_rect, style, None);
        }
    }
}

pub(crate) fn render_image_inline_fallback(
    f: &mut ratatui::Frame<'_>,
    node: &ImageNode,
    theme: &Theme,
    rect: Rect,
    clip_rect: Option<Rect>,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let draw_rect = clip_rect
        .map(|clip| rect.intersection(&clip))
        .unwrap_or(rect);
    if draw_rect.is_empty() {
        return;
    }

    let image_rect = resolve_image_render_rect(node, rect);
    let fallback_rect = image_rect.intersection(&draw_rect);
    let mut style = to_ratatui_style(resolve_base_style(theme, node.style));
    if style.bg.is_none()
        && let Some((r, g, b)) = sample_background_rgb(f, draw_rect)
    {
        style.bg = Some(ratatui::style::Color::Rgb(r, g, b));
    }

    clear_image_region(f, draw_rect, style);

    let fallback = node
        .alt
        .as_deref()
        .unwrap_or("[image unavailable in inline mode]");
    let line = Line::from(vec![Span::styled(fallback.to_string(), style)]);
    let paragraph = Paragraph::new(line).alignment(Alignment::Center);
    f.render_widget(paragraph, to_ratatui_rect(fallback_rect));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mark must survive a buffer diff as one narrow cell, round-trip its index, and never match
    /// anything a widget draws, private-use icons included.
    #[cfg(feature = "terminal-images")]
    #[test]
    fn capture_image_marks_are_one_cell_and_never_text() {
        use unicode_width::UnicodeWidthStr as _;

        for index in [0, 1, 239, 240, CAPTURE_IMAGE_LIMIT - 1] {
            let mark = capture_image_mark(index);
            assert_eq!(mark.width(), 1, "mark {index} must be one cell wide");
            assert_eq!(capture_image_mark_index(&mark), Some(index));
        }
        for text in [
            " ",
            "a",
            "\u{F0000}",
            "\u{F05B2}",
            "\u{10F000}",
            "\u{10EEEE}",
            "\u{FFFF}",
            "",
        ] {
            assert_eq!(capture_image_mark_index(text), None, "{text:?}");
        }
        let mut longer = capture_image_mark(3);
        longer.push('x');
        assert_eq!(capture_image_mark_index(&longer), None);
    }

    /// A hole in the middle of a row must split it, and stacked holes must merge, so a placeholder
    /// walk never writes into a cell an overlay is about to own.
    #[cfg(feature = "terminal-images")]
    #[test]
    fn overlay_rects_punch_holes_in_a_placeholder_row() {
        let hole = ratatui::layout::Rect {
            x: 4,
            y: 1,
            width: 3,
            height: 1,
        };
        assert_eq!(
            uncovered_x_spans(0, 10, 1, &[hole]),
            vec![(0, 4), (7, 10)],
            "the walk has to resume after the overlay, not cover it"
        );
        assert_eq!(
            uncovered_x_spans(0, 10, 0, &[hole]),
            vec![(0, 10)],
            "a hole on another row must not punch this one"
        );
        let overlap = ratatui::layout::Rect {
            x: 5,
            y: 1,
            width: 4,
            height: 1,
        };
        assert_eq!(
            uncovered_x_spans(0, 10, 1, &[hole, overlap]),
            vec![(0, 4), (9, 10)],
            "overlapping overlays are one cut, not a gap between them"
        );
        let full = ratatui::layout::Rect {
            x: 0,
            y: 1,
            width: 10,
            height: 1,
        };
        assert!(
            uncovered_x_spans(0, 10, 1, &[full]).is_empty(),
            "a row the overlay owns entirely has no placeholders"
        );
    }

    /// A placement with nowhere to draw must be found out before it is decoded, not after: the
    /// discovery is worth a whole frame of pipeline.
    #[cfg(feature = "terminal-images")]
    #[test]
    fn a_placement_entirely_under_an_overlay_is_known_to_be_invisible() {
        let area = ratatui::layout::Rect {
            x: 2,
            y: 2,
            width: 6,
            height: 3,
        };
        clear_image_occlusions();
        assert!(
            !image_area_fully_occluded(area),
            "nothing is covered when nothing occludes"
        );

        set_image_occlusions(vec![ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 4,
        }]);
        assert!(
            !image_area_fully_occluded(area),
            "a placement whose last row shows is still worth drawing"
        );

        set_image_occlusions(vec![ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 20,
        }]);
        assert!(
            image_area_fully_occluded(area),
            "every row covered means the frame is decoded for nobody"
        );

        // Two overlays that only cover it between them still cover it.
        set_image_occlusions(vec![
            ratatui::layout::Rect {
                x: 0,
                y: 0,
                width: 5,
                height: 20,
            },
            ratatui::layout::Rect {
                x: 5,
                y: 0,
                width: 15,
                height: 20,
            },
        ]);
        assert!(
            image_area_fully_occluded(area),
            "adjacent overlays leave no gap to draw into"
        );
        clear_image_occlusions();
    }

    #[cfg(feature = "terminal-images")]
    #[test]
    fn scoped_terminal_layer_holes_restore_the_frame_wide_occlusions() {
        let area = ratatui::layout::Rect::new(0, 0, 2, 1);
        set_image_occlusions(vec![ratatui::layout::Rect::new(0, 0, 1, 1)]);
        assert!(!image_area_fully_occluded(area));

        {
            let _scope = push_image_occlusions([ratatui::layout::Rect::new(1, 0, 1, 1)]);
            assert!(
                image_area_fully_occluded(area),
                "the lower terminal sees both the overlay and floating-pane holes"
            );
        }

        assert!(
            !image_area_fully_occluded(area),
            "the upper terminal must not inherit the lower terminal's pane hole"
        );
        clear_image_occlusions();
    }

    /// Overlay columns stay writable so later paint can put text there; uncovered cells stay Skip
    /// so the first-cell walk is the only thing that draws them.
    #[cfg(feature = "terminal-images")]
    #[test]
    fn a_kitty_row_does_not_skip_cells_under_an_opaque_overlay() {
        let kitty = CompressedKitty {
            transmit: Mutex::new(None),
            shared: Mutex::new(None),
            id_color: "\x1b[38;2;1;2;3m".into(),
            id_extra: 0,
            size: ratatui::layout::Size {
                width: 10,
                height: 2,
            },
        };
        set_image_occlusions(vec![ratatui::layout::Rect {
            x: 4,
            y: 0,
            width: 3,
            height: 1,
        }]);
        let backend = ratatui::backend::TestBackend::new(10, 2);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| {
                kitty.render(
                    f,
                    ratatui::layout::Rect {
                        x: 0,
                        y: 0,
                        width: 10,
                        height: 2,
                    },
                );
                let buffer = f.buffer_mut();
                assert!(
                    matches!(
                        buffer.cell((0, 0)).map(|cell| cell.diff_option),
                        Some(CellDiffOption::ForcedWidth(_))
                    ),
                    "the first uncovered cell carries the row walk"
                );
                for x in [1, 2, 3] {
                    assert_eq!(
                        buffer.cell((x, 0)).map(|cell| cell.diff_option),
                        Some(CellDiffOption::Skip),
                        "uncovered cell {x} should be drawn by the walk, not the diff"
                    );
                }
                for x in 4..7 {
                    assert_ne!(
                        buffer.cell((x, 0)).map(|cell| cell.diff_option),
                        Some(CellDiffOption::Skip),
                        "overlay cell {x} must remain writable for the modal"
                    );
                }
                assert!(
                    matches!(
                        buffer.cell((7, 0)).map(|cell| cell.diff_option),
                        Some(CellDiffOption::ForcedWidth(_))
                    ),
                    "the run to the right of the overlay is a new walk, not a gap in the first one"
                );
                for x in [8, 9] {
                    assert_eq!(
                        buffer.cell((x, 0)).map(|cell| cell.diff_option),
                        Some(CellDiffOption::Skip),
                        "uncovered cell {x} should be drawn by the walk, not the diff"
                    );
                }
            })
            .expect("draw");
        assert_eq!(
            painted_image_occlusions(),
            vec![ratatui::layout::Rect::new(4, 0, 3, 1)],
            "only holes touched by a real placeholder walk need forced repaint"
        );
        clear_image_occlusions();
    }

    /// A picture the size of its box, or a little over, is the host's to scale; a picture many times
    /// the size of its box is not.
    ///
    /// This is what a program redrawing its whole window costs or does not cost. Scaling here means
    /// resampling every pixel of every frame - nine milliseconds against two tenths for handing the
    /// frame over as it arrived - so the case that has to stay on the cheap side is a window drawn
    /// at some multiple of the cell resolution, which is what any program asking the terminal how
    /// big a cell is will produce.
    #[cfg(feature = "terminal-images")]
    #[test]
    fn a_frame_near_its_box_is_scaled_by_the_host_and_a_far_larger_one_here() {
        fn image(width: u32, height: u32) -> image::DynamicImage {
            image::DynamicImage::ImageRgb8(image::RgbImage::new(width, height))
        }

        assert!(
            host_scales_into_cells(ImageFit::Scale, &image(880, 440), 880, 440),
            "a frame already the size of its box needs nothing done to it"
        );
        assert!(
            host_scales_into_cells(ImageFit::Contain, &image(1760, 880), 880, 440),
            "twice the cell resolution is the case worth passing on whole"
        );
        assert!(
            !host_scales_into_cells(ImageFit::Scale, &image(4000, 3000), 880, 440),
            "a photograph shown small is worth shrinking once here"
        );
        assert!(
            !host_scales_into_cells(ImageFit::Crop, &image(900, 450), 880, 440),
            "a crop chooses which pixels to keep, which a box size cannot express"
        );
        assert!(
            !host_scales_into_cells(ImageFit::Scale, &image(880, 440), 0, 0),
            "a box with no pixels in it is not a box to scale into"
        );
    }

    /// A cover fills its box at any source size: a small wide image is scaled up to the box
    /// height and loses its sides, a tall one loses its top and bottom.
    #[test]
    fn cover_fills_the_box_and_crops_the_overflow_evenly() {
        let mut wide = image::RgbImage::new(40, 10);
        for (x, _, pixel) in wide.enumerate_pixels_mut() {
            *pixel = if (10..30).contains(&x) {
                image::Rgb([255, 255, 255])
            } else {
                image::Rgb([0, 0, 0])
            };
        }
        let covered = cover_image(&image::DynamicImage::ImageRgb8(wide), 80, 80);
        assert_eq!((covered.width(), covered.height()), (80, 80));
        let covered = covered.to_rgb8();
        assert_eq!(covered.get_pixel(0, 40), &image::Rgb([255, 255, 255]));
        assert_eq!(covered.get_pixel(79, 40), &image::Rgb([255, 255, 255]));

        let tall = image::DynamicImage::ImageRgb8(image::RgbImage::new(10, 400));
        let covered = cover_image(&tall, 90, 30);
        assert_eq!((covered.width(), covered.height()), (90, 30));
    }

    /// An image scrolled one row out of the top of its view keeps the rows below: the crop is cut
    /// from the image as laid out at full size, not squeezed into the visible rows.
    #[test]
    fn a_crop_keeps_the_visible_cells_of_the_full_layout() {
        let mut halves = image::RgbaImage::new(20, 40);
        for (_, y, pixel) in halves.enumerate_pixels_mut() {
            *pixel = if y < 20 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            };
        }
        let crop = CellCrop {
            x: 0,
            y: 1,
            full_width: 2,
            full_height: 2,
        };
        let mut key = key(1);
        (key.width, key.height, key.crop, key.fit) = (2, 1, Some(crop), ImageFit::Crop);
        let request = EncodeRequest::new(
            1,
            key,
            Arc::new(image::DynamicImage::ImageRgba8(halves)),
            CacheRetention::Variants,
        );

        let cropped = crop_to_visible_cells(&request, (10, 20).into(), crop).to_rgba8();
        assert_eq!((cropped.width(), cropped.height()), (20, 20));
        assert!(
            cropped
                .pixels()
                .all(|pixel| *pixel == image::Rgba([0, 0, 255, 255]))
        );
    }

    /// A Kitty placement whose top row is above the screen starts its first visible row at image
    /// row one, and draws nothing outside the clip.
    #[cfg(feature = "terminal-images")]
    #[test]
    fn clipped_kitty_placeholders_name_the_rows_they_show() {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::new(40, 60));
        let protocol = CompressedKitty::new(&image, ratatui::layout::Size::new(4, 3), 7, 0)
            .expect("kitty encode should succeed");
        let area = ratatui::layout::Rect::new(0, 0, 6, 3);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(area.width, area.height))
                .expect("terminal should init");
        terminal
            .draw(|f| {
                protocol.render_clipped(f, (1, -1), ratatui::layout::Rect::new(0, 0, 6, 1));
            })
            .expect("render should succeed");
        let buffer = terminal.backend().buffer();

        let first = buffer[(1, 0)].symbol();
        let row_one = format!(
            "{}{}",
            crate::widgets::kitty_diacritic(1),
            crate::widgets::kitty_diacritic(0)
        );
        assert!(
            first.contains(&row_one),
            "first visible row should name image row 1, got {first:?}"
        );
        assert_eq!(
            buffer[(0, 0)].symbol(),
            " ",
            "left of the image stays empty"
        );
        for x in 0..area.width {
            assert_eq!(
                buffer[(x, 1)].symbol(),
                " ",
                "row below the clip stays empty"
            );
        }
    }

    /// A picture scaled into a box it does not fill is centered on the spare axis; a crop stays at
    /// the top-left, where the pixels it kept came from.
    #[test]
    fn fitted_pictures_are_centered_in_their_box() {
        let rect_for = |width: u32, height: u32, fit: ImageFit| {
            let mut node = ImageNode::from(crate::widgets::Image::from_bytes(Vec::new()).fit(fit));
            node.decoded = Some(Arc::new(image::DynamicImage::new_rgba8(width, height)));
            let bounds = Rect {
                x: 2,
                y: 1,
                w: 40,
                h: 20,
            };
            let rect = resolve_image_render_rect(&node, bounds);
            (rect.x, rect.y, rect.w, rect.h)
        };
        // 10x20 pixel cells: a 2:1 picture fills the width and half the height.
        assert_eq!(rect_for(200, 100, ImageFit::Scale), (2, 6, 40, 10));
        assert_eq!(rect_for(100, 200, ImageFit::Scale), (12, 1, 20, 20));
        assert_eq!(rect_for(100, 100, ImageFit::Contain), (17, 8, 10, 5));
        assert_eq!(rect_for(100, 100, ImageFit::Crop), (2, 1, 10, 5));
    }

    fn dim_half() -> PixelEffect {
        BackdropBackgroundEffect::from_style(crate::style::Style::new().dim_by(0.5), None)
            .expect("a dim changes backgrounds")
            .into()
    }

    /// The fast paths in [`Recolorer`] give exactly the colors of the effects they stand in for,
    /// alone and stacked, whether they use per-channel tables or not.
    #[test]
    fn the_recolorer_matches_the_effects_it_stands_in_for() {
        use crate::style::{Color, ColorTransform, Style};

        let terminal_bg = Some(ratatui::style::Color::Rgb(18, 20, 28));
        let styles = [
            Style::new().dim_by(0.6),
            Style::new().tint_by(Color::Rgb(0, 0, 40), 0.5),
            Style::new().lighten_by(0.3),
            Style::new().transform_bg(ColorTransform::Elevate(0.5)),
            Style::new().transform_bg(ColorTransform::Opacity(0.4)),
            Style::new().transform_bg(ColorTransform::OpacityToward {
                factor: 0.3,
                target: Color::Rgb(200, 10, 90),
            }),
            Style::new().bg(Color::Rgb(40, 40, 40)).dim_by(0.2),
            Style::new()
                .bg(Color::Reset)
                .tint_by(Color::Rgb(90, 0, 0), 0.3),
            Style::new().bg(Color::Blue).dim_by(0.4),
        ];
        let effects: Vec<_> = styles
            .iter()
            .map(|style| BackdropBackgroundEffect::from_style(*style, terminal_bg).unwrap())
            .collect();
        let mut seed = 0x2545_f491_u32;
        let colors: Vec<[u8; 3]> = (0..2000)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                [seed as u8, (seed >> 8) as u8, (seed >> 16) as u8]
            })
            .chain([[0; 3], [255; 3], [255, 0, 0], [0, 255, 0], [0, 0, 255]])
            .collect();

        let full = ratatui::layout::Rect::new(0, 0, 1, 1);
        let mut stacks: Vec<Vec<BackdropBackgroundEffect>> =
            effects.iter().map(|effect| vec![*effect]).collect();
        stacks.push(vec![effects[0], effects[1]]);
        stacks.push(vec![effects[1], effects[3], effects[0]]);
        for stack in stacks {
            let mask = BackdropMask {
                columns: 1,
                rows: 1,
                layers: stack
                    .iter()
                    .map(|effect| (full, (*effect).into()))
                    .collect(),
            };
            let layers = (1u64 << stack.len()) - 1;
            let mut recolorer = Recolorer::new(&mask);
            for &rgb in &colors {
                let expected = stack
                    .iter()
                    .fold((rgb[0], rgb[1], rgb[2]), |color, effect| {
                        effect.apply_rgb(color)
                    });
                assert_eq!(
                    recolorer.recolor(layers, rgb),
                    [expected.0, expected.1, expected.2],
                    "{rgb:?} under {stack:?}"
                );
            }
        }
    }

    /// A photo under an effect that mixes channels has more colors than are worth computing one by
    /// one: the first ones stay exact, and each later one is the exact color of an input at most two
    /// levels away per channel.
    #[test]
    fn photographic_pixels_past_the_budget_are_computed_at_fewer_levels() {
        use crate::style::{ColorTransform, Style};

        let elevate = BackdropBackgroundEffect::from_style(
            Style::new().transform_bg(ColorTransform::Elevate(0.5)),
            None,
        )
        .unwrap();
        assert!(!elevate.is_per_channel());
        let mask = BackdropMask {
            columns: 1,
            rows: 1,
            layers: vec![(ratatui::layout::Rect::new(0, 0, 1, 1), elevate.into())],
        };
        let exact = |rgb: [u8; 3]| {
            let (r, g, b) = elevate.apply_rgb((rgb[0], rgb[1], rgb[2]));
            [r, g, b]
        };
        let mut recolorer = Recolorer::new(&mask);
        let color = |index: usize| {
            let packed = (index as u32).wrapping_mul(2_654_435_761) >> 8;
            [packed as u8, (packed >> 8) as u8, (packed >> 16) as u8]
        };

        for index in 0..Recolorer::EXACT_BUDGET {
            assert_eq!(recolorer.recolor(1, color(index)), exact(color(index)));
        }
        let nearest = |rgb: [u8; 3]| rgb.map(|value| value / 4 * 4 + 2);
        for index in Recolorer::EXACT_BUDGET..Recolorer::EXACT_BUDGET + 20_000 {
            let rgb = color(index);
            assert!(
                rgb.iter()
                    .zip(nearest(rgb))
                    .all(|(value, near)| value.abs_diff(near) <= 2)
            );
            assert_eq!(recolorer.recolor(1, rgb), exact(nearest(rgb)), "{rgb:?}");
        }
        assert!(matches!(
            recolorer.mappings[0].1,
            Mapping::Mixed {
                quantized: Some(_),
                ..
            }
        ));
    }

    /// The exact budget counts distinct colors, so a small palette whose colors share a hash slot
    /// stays exact however many pixels repeat them.
    #[test]
    fn a_small_palette_stays_exact_however_often_its_colors_collide() {
        use crate::style::{ColorTransform, Style};

        let elevate = BackdropBackgroundEffect::from_style(
            Style::new().transform_bg(ColorTransform::Elevate(0.5)),
            None,
        )
        .unwrap();
        let mask = BackdropMask {
            columns: 1,
            rows: 1,
            layers: vec![(ratatui::layout::Rect::new(0, 0, 1, 1), elevate.into())],
        };
        let exact = |rgb: [u8; 3]| {
            let (r, g, b) = elevate.apply_rgb((rgb[0], rgb[1], rgb[2]));
            [r, g, b]
        };
        let mut recolorer = Recolorer::new(&mask);
        // `[0, 0, 4]` and `[0, 69, 51]` hash to the same slot; `[0, 42, 198]` shared one with the
        // first in an earlier direct-mapped cache.
        let palette = [[0, 0, 4], [0, 69, 51], [0, 42, 198]];
        for i in 0..=Recolorer::EXACT_BUDGET * 2 {
            let rgb = palette[i % palette.len()];
            assert_eq!(
                recolorer.recolor(1, rgb),
                exact(rgb),
                "{rgb:?} at pixel {i}"
            );
        }
        assert_eq!(recolorer.exact_len, palette.len());
        assert!(matches!(
            recolorer.mappings[0].1,
            Mapping::Mixed {
                quantized: None,
                ..
            }
        ));
    }

    fn cells(x: i16, y: i16, w: u16, h: u16) -> Rect {
        Rect { x, y, w, h }
    }

    /// Runs `body` with `backdrops` installed, and clears them even if it panics.
    fn with_backdrops<R>(backdrops: Vec<ImageBackdrop>, body: impl FnOnce() -> R) -> R {
        struct Clear;
        impl Drop for Clear {
            fn drop(&mut self) {
                clear_image_backdrops();
            }
        }
        let _clear = Clear;
        set_image_backdrops(backdrops);
        body()
    }

    /// A backdrop over the right half of an image dims the pixels of the cells it covers and
    /// leaves the rest, down to the cell edge, as they were.
    #[test]
    fn only_the_cells_a_backdrop_covers_dim() {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            40,
            20,
            image::Rgb([200, 100, 50]),
        ));
        let mask = with_backdrops(
            vec![ImageBackdrop {
                rect: ratatui::layout::Rect::new(12, 0, 10, 10),
                effect: dim_half(),
            }],
            || backdrop_mask_for(cells(10, 3, 4, 1)).expect("half the image is covered"),
        );
        assert_eq!(mask.layers[0].0, ratatui::layout::Rect::new(2, 0, 2, 1));

        let dimmed = mask.apply(&image, (10, 20), None).to_rgb8();
        let lit = image::Rgb([200, 100, 50]);
        let dark = image::Rgb([100, 50, 25]);
        for x in 0..40 {
            let expected = if x < 20 { lit } else { dark };
            assert_eq!(*dimmed.get_pixel(x, 10), expected, "pixel column {x}");
        }
    }

    /// Pixels map to cells the way the host lays the image out: fitted from the top-left, so a
    /// half-height picture ends inside the first row and a backdrop over the second row misses it.
    #[test]
    fn a_mask_follows_the_fitted_layout_not_a_stretch() {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            20,
            10,
            image::Rgba([200, 100, 50, 255]),
        ));
        let mask = BackdropMask {
            columns: 2,
            rows: 2,
            layers: vec![(ratatui::layout::Rect::new(0, 1, 2, 1), dim_half())],
        };
        let dimmed = mask.apply(&image, (10, 20), None).to_rgba8();
        assert!(
            dimmed.pixels().all(|pixel| pixel.0 == [200, 100, 50, 255]),
            "a 20x10 picture in a 20x40 box is drawn in the top 10 pixel rows only"
        );
    }

    /// Alpha is the picture's shape, not its color, so it is kept; a protocol without alpha
    /// composites onto its background first, and that composite is what dims.
    #[test]
    fn dimming_keeps_alpha_unless_the_protocol_flattens() {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            10,
            20,
            image::Rgba([200, 100, 50, 0]),
        ));
        let mask = BackdropMask {
            columns: 1,
            rows: 1,
            layers: vec![(ratatui::layout::Rect::new(0, 0, 1, 1), dim_half())],
        };
        let kept = mask.apply(&image, (10, 20), None).to_rgba8();
        assert_eq!(kept.get_pixel(0, 0).0[3], 0);
        let flattened = mask.apply(&image, (10, 20), Some((80, 40, 20))).to_rgba8();
        assert_eq!(flattened.get_pixel(0, 0).0, [40, 20, 10, 255]);
    }

    #[test]
    fn nothing_is_masked_outside_a_backdrop_or_without_one() {
        assert!(backdrop_mask_for(cells(0, 0, 4, 2)).is_none());
        let mask = with_backdrops(
            vec![ImageBackdrop {
                rect: ratatui::layout::Rect::new(10, 10, 5, 5),
                effect: dim_half(),
            }],
            || backdrop_mask_for(cells(0, 0, 4, 2)),
        );
        assert!(
            mask.is_none(),
            "a backdrop elsewhere does not touch the image"
        );
    }

    /// Half blocks are cells the backdrop dims itself, so their pixels must not be dimmed as well.
    #[test]
    fn half_blocks_are_left_for_the_backdrop_to_dim() {
        let backdrops = vec![ImageBackdrop {
            rect: ratatui::layout::Rect::new(0, 0, 20, 10),
            effect: dim_half(),
        }];
        with_backdrops(backdrops, || {
            let area = cells(0, 0, 4, 2);
            assert!(live_backdrop_mask(area, ImageProtocol::Halfblocks).is_none());
            for protocol in [
                ImageProtocol::Kitty,
                ImageProtocol::Sixel,
                ImageProtocol::Iterm2,
            ] {
                assert!(
                    live_backdrop_mask(area, protocol).is_some(),
                    "{protocol:?} pixels must be dimmed before encoding"
                );
            }
        });
    }

    /// The dimmed encode is its own cache entry beside the undimmed one, so closing an overlay over
    /// unchanged pixels is a cache hit.
    #[test]
    fn dimmed_and_undimmed_encodes_are_cached_side_by_side() {
        let mask = Arc::new(BackdropMask {
            columns: 80,
            rows: 24,
            layers: vec![(ratatui::layout::Rect::new(0, 0, 80, 24), dim_half())],
        });
        let plain = request(7, 10);
        let dimmed = request(7, 10).with_backdrop(Some(Arc::clone(&mask)));
        assert_ne!(plain.key, dimmed.key);
        assert_ne!(dimmed.key.backdrop, 0);
        assert_eq!(
            request(7, 10).with_backdrop(None).key,
            plain.key,
            "no backdrop is the plain key"
        );

        let mut cache = ImageRenderCache::default();
        cache.insert(7, plain.key, protocol(), 10, CacheRetention::LatestOnly);
        cache.insert(7, dimmed.key, protocol(), 10, CacheRetention::LatestOnly);
        assert!(cache.get(&dimmed.key).is_some(), "the overlay is open");
        assert!(
            cache.get(&plain.key).is_some(),
            "closing the overlay finds the undimmed encode still cached"
        );
        assert!(
            stream_encoding_compatible(7, &plain.key, 7, &dimmed.key),
            "the other variant bridges while one encodes"
        );
    }

    /// A terminal stream keeps one host image id across frames, so the dimmed variant needs its
    /// own: transmitting it under the shared id would overwrite the undimmed pixels on the host.
    #[cfg(feature = "terminal-images")]
    #[test]
    fn a_dimmed_terminal_stream_gets_its_own_kitty_image() {
        let mask = Arc::new(BackdropMask {
            columns: 80,
            rows: 24,
            layers: vec![(ratatui::layout::Rect::new(0, 0, 80, 24), dim_half())],
        });
        let plain = request(7, 10);
        let dimmed = request(7, 10).with_backdrop(Some(mask));
        assert_ne!(kitty_image_id(&plain), kitty_image_id(&dimmed));
        assert_eq!(
            kitty_image_id(&dimmed),
            kitty_image_id(&request(7, 11).with_backdrop(dimmed.backdrop.clone())),
            "a dimmed stream is still one image across frames"
        );
    }

    /// The `Image` widget draws through the same host protocols as a terminal pane, so its pixels
    /// dim under a backdrop too, except as half blocks.
    #[test]
    fn an_image_widget_under_a_backdrop_encodes_a_dimmed_variant() {
        let request_for = |protocol: ImageProtocol| {
            let mut node = ImageNode::from(
                crate::widgets::Image::from_bytes(Vec::new())
                    .fit(ImageFit::Scale)
                    .protocol(protocol),
            );
            node.decoded = Some(Arc::new(image::DynamicImage::new_rgba8(40, 40)));
            build_encode_request(&node, cells(2, 1, 4, 2), None).expect("a decoded image")
        };
        let plain = request_for(ImageProtocol::Kitty);
        assert_eq!(plain.key.backdrop, 0);

        with_backdrops(
            vec![ImageBackdrop {
                rect: ratatui::layout::Rect::new(0, 0, 20, 10),
                effect: dim_half(),
            }],
            || {
                let dimmed = request_for(ImageProtocol::Kitty);
                assert!(dimmed.backdrop.is_some());
                assert_ne!(dimmed.key, plain.key, "the dimmed encode caches apart");
                let half_blocks = request_for(ImageProtocol::Halfblocks);
                assert!(half_blocks.backdrop.is_none());
                assert_eq!(half_blocks.key.backdrop, 0);
            },
        );
    }

    /// The encoder puts the pixels through the mask before encoding, whatever the protocol.
    #[test]
    fn the_encoder_dims_before_it_encodes() {
        let red = Arc::new(image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            10,
            20,
            image::Rgb([200, 0, 0]),
        )));
        let mask = Arc::new(BackdropMask {
            columns: 1,
            rows: 1,
            layers: vec![(ratatui::layout::Rect::new(0, 0, 1, 1), dim_half())],
        });
        let mut key = key(1);
        (key.width, key.height) = (1, 1);
        let request =
            EncodeRequest::new(1, key, red, CacheRetention::LatestOnly).with_backdrop(Some(mask));
        let encoded = encode_request(&request).expect("half blocks encode");

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(1, 1)).expect("terminal");
        terminal
            .draw(|f| encoded.render(f, f.area()))
            .expect("draw");
        let cell = terminal.backend().buffer()[(0, 0)].clone();
        assert!(
            [cell.fg, cell.bg].contains(&ratatui::style::Color::Rgb(100, 0, 0)),
            "the encoded cell carries the dimmed red, got {cell:?}"
        );
    }

    fn key(source_hash: u64) -> RenderCacheKey {
        RenderCacheKey {
            source_hash,
            frame_index: 0,
            width: 80,
            height: 24,
            crop: None,
            background_rgb: None,
            fit: ImageFit::Scale,
            protocol: ImageProtocol::Auto,
            resolved_protocol: ImageProtocol::Halfblocks,
            z_index: 0,
            backdrop: 0,
        }
    }

    fn request(stream_key: u64, source_hash: u64) -> EncodeRequest {
        EncodeRequest::new(
            stream_key,
            key(source_hash),
            Arc::new(image::DynamicImage::new_rgba8(1, 1)),
            CacheRetention::LatestOnly,
        )
    }

    fn protocol() -> Arc<EncodedProtocol> {
        Arc::new(EncodedProtocol::ratatui(
            Protocol::Halfblocks(Default::default()),
            ImageProtocol::Halfblocks,
        ))
    }

    fn pending_protocol() -> Arc<EncodedProtocol> {
        Arc::new(EncodedProtocol::ratatui(
            Protocol::Halfblocks(Default::default()),
            ImageProtocol::Kitty,
        ))
    }

    #[test]
    fn newer_frame_replaces_queued_work_for_the_same_stream() {
        let encoder = AsyncEncoder::default();
        encoder.enqueue(request(7, 10));
        encoder.enqueue(request(7, 11));

        let inner = encoder.inner.lock().unwrap();
        assert_eq!(inner.queue.iter().copied().collect::<Vec<_>>(), vec![7]);
        assert_eq!(inner.queued.len(), 1);
        assert_eq!(inner.queued.get(&7).unwrap().key.source_hash, 11);
    }

    #[test]
    fn previous_pixels_are_compatible_with_the_same_stream_only() {
        let previous = key(10);
        let next = key(11);

        assert!(stream_encoding_compatible(7, &previous, 7, &next));
        assert!(!stream_encoding_compatible(7, &previous, 8, &next));
    }

    #[test]
    fn previous_pixels_with_another_z_index_are_not_compatible() {
        let previous = key(10);
        let mut moved_behind_text = key(11);
        moved_behind_text.z_index = -1;

        assert!(!stream_encoding_compatible(
            7,
            &previous,
            7,
            &moved_behind_text
        ));
    }

    #[cfg(feature = "terminal-images")]
    #[test]
    fn terminal_kitty_image_id_stays_stable_across_frames() {
        assert_eq!(
            kitty_image_id(&request(7, 10)),
            kitty_image_id(&request(7, 11))
        );
    }

    #[cfg(feature = "terminal-images")]
    #[test]
    fn terminal_kitty_image_id_is_isolated_per_stream() {
        assert_ne!(
            kitty_image_id(&request(7, 10)),
            kitty_image_id(&request(8, 10))
        );
    }

    #[cfg(feature = "terminal-images")]
    #[test]
    fn terminal_kitty_frame_can_encode_without_worker_round_trip() {
        let encoder = AsyncEncoder::default();
        let mut request = request(7, 10);
        request.key.width = 1;
        request.key.height = 1;
        request.key.resolved_protocol = ImageProtocol::Kitty;
        let key = request.key;

        let encoded = encoder.encode_synchronously(request).unwrap();

        assert!(encoded.transmission_pending());
        assert!(encoder.cache_get(&key).is_some());
    }

    #[cfg(feature = "terminal-images")]
    #[test]
    fn terminal_kitty_encoding_preserves_the_child_z_index() {
        let mut request = request(7, 10);
        request.key.width = 1;
        request.key.height = 1;
        request.key.resolved_protocol = ImageProtocol::Kitty;
        request.key.z_index = -1_500_000_000;

        let EncodedProtocol::CompressedKitty(encoded) =
            encode_request(&request).expect("Kitty encoding")
        else {
            panic!("terminal Kitty pixels should use the compressed encoder");
        };
        let transmission = encoded.transmit.lock().expect("transmission lock");
        let transmission = transmission.as_deref().expect("pending transmission");

        assert!(transmission.contains("a=T,U=1"));
        assert!(transmission.contains(",z=-1500000000"));
    }

    #[cfg(feature = "terminal-images")]
    #[test]
    fn widget_kitty_image_id_remains_frame_specific() {
        let mut first = request(7, 10);
        let mut second = request(7, 11);
        first.retention = CacheRetention::Variants;
        second.retention = CacheRetention::Variants;

        assert_ne!(kitty_image_id(&first), kitty_image_id(&second));
    }

    #[test]
    fn latest_only_cache_keeps_one_previous_frame_for_replacement() {
        let mut cache = ImageRenderCache::default();
        cache.insert(7, key(10), protocol(), 10, CacheRetention::LatestOnly);
        cache.insert(7, key(11), protocol(), 20, CacheRetention::LatestOnly);
        cache.insert(7, key(12), protocol(), 30, CacheRetention::LatestOnly);

        assert_eq!(cache.entries.len(), 2);
        assert_eq!(cache.entries[0].key.source_hash, 11);
        assert_eq!(cache.entries[1].key.source_hash, 12);
        assert_eq!(cache.total_estimated_bytes, 50);
    }

    #[test]
    fn latest_only_cache_replaces_pending_work_before_presented_pixels() {
        let mut cache = ImageRenderCache::default();
        cache.insert(7, key(10), protocol(), 10, CacheRetention::LatestOnly);
        cache.insert(
            7,
            key(11),
            pending_protocol(),
            20,
            CacheRetention::LatestOnly,
        );
        cache.insert(
            7,
            key(12),
            pending_protocol(),
            30,
            CacheRetention::LatestOnly,
        );

        assert_eq!(cache.entries.len(), 2);
        assert_eq!(cache.entries[0].key.source_hash, 10);
        assert_eq!(cache.entries[1].key.source_hash, 12);
        assert_eq!(cache.total_estimated_bytes, 40);
    }

    #[test]
    fn pending_compatible_frame_bootstraps_before_exact_frame_is_ready() {
        let mut cache = ImageRenderCache::default();
        cache.insert(7, key(10), pending_protocol(), 10, CacheRetention::Variants);

        let bootstrap = cache.get_latest_compatible(7, &key(11)).unwrap();

        assert!(bootstrap.protocol.transmission_pending());
    }

    fn dimmed(mut key: RenderCacheKey) -> RenderCacheKey {
        key.backdrop = 5;
        key
    }

    fn widget_request(key: RenderCacheKey) -> EncodeRequest {
        EncodeRequest::new(
            7,
            key,
            Arc::new(image::DynamicImage::new_rgba8(1, 1)),
            CacheRetention::Variants,
        )
    }

    /// One animated source shown twice, under a modal and above it: both copies keep advancing
    /// frames, and neither may keep replacing the other's queued work.
    #[test]
    fn backdrop_variants_of_one_image_queue_independently() {
        let frame = |index: usize, backdrop: bool| {
            let mut key = key(10);
            key.frame_index = index;
            widget_request(if backdrop { dimmed(key) } else { key })
        };
        let encoder = AsyncEncoder::default();
        for index in 17..20 {
            encoder.enqueue(frame(index, true));
            encoder.enqueue(frame(index, false));
        }
        assert_eq!(encoder.inner.lock().unwrap().queue.len(), 2);

        let first = encoder.next_request_blocking();
        encoder.enqueue(frame(20, true));
        encoder.enqueue(frame(20, false));
        let second = encoder.next_request_blocking();

        assert_eq!(first.key, frame(19, true).key);
        assert_eq!(
            second.key,
            frame(20, false).key,
            "the undimmed copy encodes while the dimmed one is in flight"
        );
        encoder.complete_request(&first, None);
        assert_eq!(
            encoder.next_request_blocking().key,
            frame(20, true).key,
            "the dimmed copy's newest frame waited for its own worker"
        );
    }

    #[test]
    fn a_terminal_stream_keeps_one_queue_slot_across_backdrops() {
        let encoder = AsyncEncoder::default();
        encoder.enqueue(request(7, 10));
        let mut under_modal = request(7, 11);
        under_modal.key = dimmed(under_modal.key);
        encoder.enqueue(under_modal);

        let inner = encoder.inner.lock().unwrap();
        assert_eq!(inner.queue.len(), 1);
        assert_eq!(inner.queued.get(&7).unwrap().key.backdrop, 5);
    }

    #[test]
    fn a_stand_in_dimmed_the_same_way_is_preferred() {
        let mut cache = ImageRenderCache::default();
        cache.insert(7, key(10), protocol(), 10, CacheRetention::Variants);
        cache.insert(7, dimmed(key(11)), protocol(), 10, CacheRetention::Variants);

        let stand_in = cache.get_latest_compatible(7, &key(12)).unwrap();
        assert!(stand_in.same_backdrop);
        assert_eq!(cache.entries.last().unwrap().key, key(10));

        let stand_in = cache.get_latest_compatible(7, &dimmed(key(12))).unwrap();
        assert!(stand_in.same_backdrop);
        assert_eq!(cache.entries.last().unwrap().key, dimmed(key(11)));
    }

    /// Pixels that changed while a modal was open have only dimmed encodes. Closing it encodes the
    /// undimmed frame at once instead of queuing it behind a dimmed stand-in.
    #[test]
    fn a_stand_in_dimmed_the_other_way_is_replaced_by_an_encode() {
        let small = |source_hash| {
            let mut key = key(source_hash);
            key.width = 1;
            key.height = 1;
            key
        };
        let encoder = AsyncEncoder::default();
        encoder.inner.lock().unwrap().cache.insert(
            7,
            dimmed(small(11)),
            protocol(),
            10,
            CacheRetention::LatestOnly,
        );

        let closed = request_for_key(small(12));
        assert!(matches!(
            encoder.resolve_miss(closed, false),
            ProtocolResolve::Ready(_)
        ));
        assert!(encoder.cache_get(&small(12)).is_some());
        assert!(encoder.inner.lock().unwrap().queue.is_empty());

        let next = request_for_key(small(13));
        assert!(
            matches!(encoder.resolve_miss(next, false), ProtocolResolve::Stale(_)),
            "later frames queue behind the undimmed stand-in as usual"
        );
        assert_eq!(encoder.inner.lock().unwrap().queue.len(), 1);
    }

    #[test]
    fn a_failed_encode_never_falls_back_to_the_wrong_dimming() {
        let stand_in = |same_backdrop| {
            Some(StandIn {
                protocol: protocol(),
                same_backdrop,
            })
        };

        assert!(matches!(
            synchronous_resolve(None, stand_in(false)),
            ProtocolResolve::Unavailable
        ));
        assert!(matches!(
            synchronous_resolve(None, stand_in(true)),
            ProtocolResolve::Stale(_)
        ));
        assert!(matches!(
            synchronous_resolve(None, None),
            ProtocolResolve::Unavailable
        ));
        assert!(matches!(
            synchronous_resolve(Some(protocol()), stand_in(false)),
            ProtocolResolve::Ready(_)
        ));
    }

    fn request_for_key(key: RenderCacheKey) -> EncodeRequest {
        EncodeRequest::new(
            7,
            key,
            Arc::new(image::DynamicImage::new_rgba8(1, 1)),
            CacheRetention::LatestOnly,
        )
    }

    #[test]
    fn workers_do_not_encode_two_frames_of_one_stream_concurrently() {
        let encoder = AsyncEncoder::default();
        encoder.enqueue(request(7, 10));
        let first = encoder.next_request_blocking();
        encoder.enqueue(request(7, 11));
        encoder.enqueue(request(8, 20));

        let second = encoder.next_request_blocking();

        assert_eq!(first.stream_key, 7);
        assert_eq!(second.stream_key, 8);
    }

    #[test]
    fn cache_can_retain_size_variants_for_static_images() {
        let mut cache = ImageRenderCache::default();
        let mut resized = key(10);
        resized.width = 40;
        cache.insert(7, key(10), protocol(), 10, CacheRetention::Variants);
        cache.insert(7, resized, protocol(), 20, CacheRetention::Variants);

        assert_eq!(cache.entries.len(), 2);
        assert_eq!(cache.total_estimated_bytes, 30);
    }

    #[test]
    fn kitty_cache_accounting_uses_encoded_pixel_footprint() {
        let mut kitty_key = key(10);
        kitty_key.resolved_protocol = ImageProtocol::Kitty;
        let image = image::DynamicImage::new_rgba8(1600, 900);

        let estimated = estimate_protocol_bytes_at_font(
            kitty_key,
            &image,
            ratatui_image::FontSize::new(10, 20),
        );

        assert!(estimated > 2_000_000);
    }

    #[test]
    fn cache_expires_inactive_streams() {
        let mut cache = ImageRenderCache::default();
        cache.insert(7, key(10), protocol(), 10, CacheRetention::LatestOnly);
        let after_ttl = cache.entries[0].last_used + Duration::from_secs(31);

        cache.evict_expired(after_ttl);

        assert!(cache.entries.is_empty());
        assert_eq!(cache.total_estimated_bytes, 0);
    }

    #[test]
    fn ratatui_kitty_transmits_before_switching_native_placeholders() {
        use ratatui_image::protocol::kitty::Kitty;

        let image = image::DynamicImage::new_rgb8(10, 20);
        let size = ratatui::layout::Size::new(1, 1);
        let next = EncodedProtocol::ratatui(
            Protocol::Kitty(Kitty::new(image, size, 8, false, false).unwrap()),
            ImageProtocol::Kitty,
        );
        let backend = ratatui::backend::TestBackend::new(1, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| next.render(frame, frame.area()))
            .unwrap();
        let cell = terminal.backend().buffer().cell((0, 0)).unwrap();
        let symbol = cell.symbol();
        let transmission = symbol.find("i=8").expect("kitty transmission");
        let placeholder = symbol
            .find('\u{10EEEE}')
            .expect("kitty unicode placeholder");
        assert!(
            transmission < placeholder,
            "transmission must precede the placeholder; got {symbol:?}"
        );
        assert_eq!(cell.fg, ratatui::style::Color::Rgb(0, 0, 8));
    }

    #[cfg(feature = "terminal-images")]
    #[test]
    fn compressed_kitty_releases_transmission_after_render() {
        let image = image::DynamicImage::new_rgba8(400, 200);
        let protocol =
            CompressedKitty::new(&image, ratatui::layout::Size::new(40, 10), 7, 0).unwrap();
        let encoded_len = protocol
            .transmit
            .lock()
            .unwrap()
            .as_ref()
            .map(String::len)
            .unwrap();
        assert!(encoded_len < 400 * 200 * 4 / 4);

        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| protocol.render(frame, frame.area()))
            .unwrap();

        assert!(protocol.transmit.lock().unwrap().is_none());
    }

    #[cfg(feature = "terminal-images")]
    #[test]
    fn compressed_kitty_transmits_before_switching_native_placeholders() {
        let image = image::DynamicImage::new_rgb8(10, 20);
        let size = ratatui::layout::Size::new(1, 1);
        let next =
            EncodedProtocol::CompressedKitty(CompressedKitty::new(&image, size, 8, 0).unwrap());
        let backend = ratatui::backend::TestBackend::new(1, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| next.render(frame, frame.area()))
            .unwrap();
        let symbol = terminal.backend().buffer().cell((0, 0)).unwrap().symbol();
        let transmission = symbol.find("i=8").unwrap();
        let placeholders = symbol.find("\x1b[s").unwrap();
        assert!(transmission < placeholders);
        assert!(symbol.contains("\x1b[38;2;0;0;8m"));
    }
}
