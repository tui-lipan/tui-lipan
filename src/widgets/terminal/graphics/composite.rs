//! Resolve pixel placements into disjoint cell-aligned images within each exact z plane.
//!
//! A placeholder replaces a whole cell. Sending overlapping patches independently would erase
//! the earlier image even where the later patch covers only a few pixels of that cell. Small
//! cached tiles preserve those pixels without composing or encoding the entire viewport on each
//! update. The cache belongs to the source screen and retains only the current visible tiles.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use image::{DynamicImage, GenericImageView, RgbaImage};

use super::{
    ImageSource, TerminalImage, TerminalImageCrop, TerminalImagePlacement, next_stream_namespace,
};

const TILE_COLS: i32 = 16;
const TILE_ROWS: i32 = 8;

pub(super) struct CompositionCache {
    namespace: u64,
    pub(super) alive: Arc<std::sync::atomic::AtomicBool>,
    pub(super) tiles: HashMap<(i32, i32, i32), TerminalImage>,
}

impl Default for CompositionCache {
    fn default() -> Self {
        Self {
            namespace: next_stream_namespace(),
            alive: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            tiles: HashMap::new(),
        }
    }
}

impl Drop for CompositionCache {
    fn drop(&mut self) {
        self.alive
            .store(false, std::sync::atomic::Ordering::Release);
        crate::backend::ratatui_backend::renderers::image::release_closed_terminal_images();
    }
}

#[derive(Clone, Copy)]
struct Layer<'a> {
    placement: &'a TerminalImagePlacement,
    source: TerminalImageCrop,
    // Destination in viewport pixels, before clipping.
    x: i64,
    y: i64,
    width: u32,
    height: u32,
}

impl<'a> Layer<'a> {
    fn new(placement: &'a TerminalImagePlacement, cell: (u16, u16)) -> Self {
        let image = &placement.image;
        let source = placement.source_crop.unwrap_or(TerminalImageCrop {
            x: 0,
            y: 0,
            width: image.width,
            height: image.height,
        });
        let (offset, size) = image.geometry.map_or(
            (
                (0, 0),
                (
                    u32::from(placement.cols) * u32::from(cell.0),
                    u32::from(placement.rows) * u32::from(cell.1),
                ),
            ),
            |geometry| (geometry.offset, geometry.size),
        );
        Self {
            placement,
            source,
            x: i64::from(placement.col) * i64::from(cell.0) + i64::from(offset.0),
            y: i64::from(placement.row) * i64::from(cell.1) + i64::from(offset.1),
            width: size.0,
            height: size.1,
        }
    }

    fn hash(&self, hasher: &mut impl Hasher) {
        let p = self.placement;
        (
            p.image.stream_namespace,
            p.image.source_hash,
            p.image_id,
            p.z,
        )
            .hash(hasher);
        (self.x, self.y, self.width, self.height).hash(hasher);
        (
            self.source.x,
            self.source.y,
            self.source.width,
            self.source.height,
        )
            .hash(hasher);
    }

    fn paint(&self, canvas: &mut RgbaImage, origin: (i64, i64)) {
        let Some(pixels) = self.placement.image.pixels() else {
            return;
        };
        let left = self.x.max(origin.0);
        let top = self.y.max(origin.1);
        let right = (self.x + i64::from(self.width)).min(origin.0 + i64::from(canvas.width()));
        let bottom = (self.y + i64::from(self.height)).min(origin.1 + i64::from(canvas.height()));
        if self.paint_unscaled(canvas, pixels, origin, (left, top, right, bottom)) {
            return;
        }
        for y in top..bottom {
            let sy = self.source.y
                + ((y - self.y) as u64 * u64::from(self.source.height) / u64::from(self.height))
                    as u32;
            for x in left..right {
                let sx = self.source.x
                    + ((x - self.x) as u64 * u64::from(self.source.width) / u64::from(self.width))
                        as u32;
                let pixel =
                    pixels.get_pixel(sx.min(pixels.width() - 1), sy.min(pixels.height() - 1));
                let dest = canvas.get_pixel_mut((x - origin.0) as u32, (y - origin.1) as u32);
                blend_rgba(&mut dest.0, pixel.0);
            }
        }
    }

    /// Browser patches already match the terminal's pixel size. Copy contiguous rows instead of
    /// dividing and dispatching through DynamicImage for each pixel.
    fn paint_unscaled(
        &self,
        canvas: &mut RgbaImage,
        pixels: &DynamicImage,
        origin: (i64, i64),
        bounds: (i64, i64, i64, i64),
    ) -> bool {
        if self.width != self.source.width || self.height != self.source.height {
            return false;
        }
        let (left, top, right, bottom) = bounds;
        if left >= right || top >= bottom {
            return true;
        }
        let sx = u64::from(self.source.x) + (left - self.x) as u64;
        let sy = u64::from(self.source.y) + (top - self.y) as u64;
        let width = (right - left) as usize;
        let height = (bottom - top) as usize;
        if sx + width as u64 > u64::from(pixels.width())
            || sy + height as u64 > u64::from(pixels.height())
        {
            return false;
        }
        let Some((source, channels)) = image_bytes(pixels) else {
            return false;
        };
        let source_stride = pixels.width() as usize * channels;
        let dest_stride = canvas.width() as usize * 4;
        let source_start = sy as usize * source_stride + sx as usize * channels;
        let dest_start = (top - origin.1) as usize * dest_stride + (left - origin.0) as usize * 4;
        let dest = canvas.as_flat_samples_mut().samples;
        for row in 0..height {
            let src = source_start + row * source_stride;
            let dst = dest_start + row * dest_stride;
            paint_row(
                &mut dest[dst..dst + width * 4],
                &source[src..src + width * channels],
                channels,
            );
        }
        true
    }
}

/// Compose only changed tiles. Raw placement geometry remains in snapshots and replay state.
pub(crate) fn composite_terminal_images(
    placements: &[TerminalImagePlacement],
    cols: u16,
    rows: u16,
) -> Vec<TerminalImagePlacement> {
    let Some(first) = placements.first() else {
        return Vec::new();
    };
    let Some(cache) = &first.image.composition else {
        return placements.to_vec();
    };
    let Some(geometry) = first.image.geometry else {
        return placements.to_vec();
    };
    let cell = geometry.cell;
    if placements
        .iter()
        .all(|placement| cell_aligned(placement, cell))
        && placements.iter().enumerate().all(|(index, placement)| {
            placements[index + 1..]
                .iter()
                .all(|other| !cell_overlap(placement, other))
        })
    {
        cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tiles
            .clear();
        return placements.to_vec();
    }
    let mut layers: Vec<_> = placements.iter().map(|p| Layer::new(p, cell)).collect();
    // Equal z values stack by image id, as specified by Kitty.
    layers.sort_by_key(|layer| (layer.placement.z, layer.placement.image_id));
    let buckets = tile_layers(&layers, (cols, rows), cell);
    let mut cache = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut retained = HashMap::new();
    let mut result = Vec::with_capacity(buckets.len());
    for ((z, tx, ty), layers) in buckets {
        let col = tx * TILE_COLS;
        let row = ty * TILE_ROWS;
        let width = (i32::from(cols) - col).min(TILE_COLS) as u16;
        let height = (i32::from(rows) - row).min(TILE_ROWS) as u16;
        let image = compose_tile(&cache, (z, tx, ty), &layers, cell, (width, height));
        retained.insert((z, tx, ty), image.clone());
        result.push(TerminalImagePlacement {
            image_id: ((ty as u32) << 16) | tx as u32,
            image,
            row,
            col,
            rows: height,
            cols: width,
            z,
            source_crop: None,
        });
    }
    cache.tiles = retained;
    result
}

fn tile_layers<'a>(
    layers: &'a [Layer<'a>],
    viewport: (u16, u16),
    cell: (u16, u16),
) -> BTreeMap<(i32, i32, i32), Vec<Layer<'a>>> {
    let tile_w = i64::from(TILE_COLS) * i64::from(cell.0);
    let tile_h = i64::from(TILE_ROWS) * i64::from(cell.1);
    let mut buckets = BTreeMap::<_, Vec<_>>::new();
    for layer in layers {
        let left = layer.x.max(0);
        let top = layer.y.max(0);
        let right =
            (layer.x + i64::from(layer.width)).min(i64::from(viewport.0) * i64::from(cell.0));
        let bottom =
            (layer.y + i64::from(layer.height)).min(i64::from(viewport.1) * i64::from(cell.1));
        if left >= right || top >= bottom {
            continue;
        }
        for ty in top / tile_h..=(bottom - 1) / tile_h {
            for tx in left / tile_w..=(right - 1) / tile_w {
                buckets
                    .entry((layer.placement.z, tx as i32, ty as i32))
                    .or_default()
                    .push(*layer);
            }
        }
    }
    buckets
}

fn compose_tile(
    cache: &CompositionCache,
    tile: (i32, i32, i32),
    layers: &[Layer<'_>],
    cell: (u16, u16),
    cells: (u16, u16),
) -> TerminalImage {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (cache.namespace, tile, cell, cells).hash(&mut hasher);
    for layer in layers {
        layer.hash(&mut hasher)
    }
    let source_hash = hasher.finish();
    if let Some(image) = cache
        .tiles
        .get(&tile)
        .filter(|image| image.source_hash == source_hash)
    {
        return image.clone();
    }
    let width = u32::from(cells.0) * u32::from(cell.0);
    let height = u32::from(cells.1) * u32::from(cell.1);
    let origin = (
        i64::from(tile.1 * TILE_COLS) * i64::from(cell.0),
        i64::from(tile.2 * TILE_ROWS) * i64::from(cell.1),
    );
    let mut pixels = RgbaImage::new(width, height);
    for layer in layers {
        layer.paint(&mut pixels, origin)
    }
    let mut stream = std::collections::hash_map::DefaultHasher::new();
    (cache.namespace, tile.0).hash(&mut stream);
    TerminalImage {
        source: ImageSource::Decoded(Arc::new(DynamicImage::ImageRgba8(pixels))),
        width,
        height,
        source_hash,
        stream_namespace: stream.finish(),
        geometry: None,
        composition: None,
    }
}

fn cell_aligned(placement: &TerminalImagePlacement, cell: (u16, u16)) -> bool {
    placement.image.geometry.is_some_and(|geometry| {
        geometry.offset == (0, 0)
            && geometry.size
                == (
                    u32::from(placement.cols) * u32::from(cell.0),
                    u32::from(placement.rows) * u32::from(cell.1),
                )
    })
}

fn cell_overlap(a: &TerminalImagePlacement, b: &TerminalImagePlacement) -> bool {
    a.col < b.col + i32::from(b.cols)
        && b.col < a.col + i32::from(a.cols)
        && a.row < b.row + i32::from(b.rows)
        && b.row < a.row + i32::from(a.rows)
}

fn image_bytes(pixels: &DynamicImage) -> Option<(&[u8], usize)> {
    match pixels {
        DynamicImage::ImageRgba8(image) => Some((image.as_raw(), 4)),
        DynamicImage::ImageRgb8(image) => Some((image.as_raw(), 3)),
        _ => None,
    }
}

fn paint_row(dest: &mut [u8], source: &[u8], channels: usize) {
    if channels == 3 {
        for (dest, source) in dest
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(source.as_chunks::<3>().0)
        {
            *dest = [source[0], source[1], source[2], 255];
        }
    } else if source
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[3] == 255)
    {
        dest.copy_from_slice(source);
    } else {
        for (dest, source) in dest
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(source.as_chunks::<4>().0)
        {
            blend_rgba(dest, *source);
        }
    }
}

// Integer source-over keeps an opaque background opaque, including after many tiny patches.
fn blend_rgba(dest: &mut [u8; 4], source: [u8; 4]) {
    let alpha = u32::from(source[3]);
    if alpha == 0 {
        return;
    }
    if alpha == 255 {
        *dest = source;
        return;
    }
    let background_alpha = u32::from(dest[3]) * (255 - alpha);
    let combined = alpha * 255 + background_alpha;
    for channel in 0..3 {
        dest[channel] = ((u32::from(source[channel]) * alpha * 255
            + u32::from(dest[channel]) * background_alpha
            + combined / 2)
            / combined) as u8;
    }
    dest[3] = ((combined + 127) / 255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::{TerminalCellSize, TerminalScreen};
    use base64::Engine as _;

    fn transmit(screen: &mut TerminalScreen, keys: &str, width: u32, height: u32, color: [u8; 4]) {
        let payload = base64::engine::general_purpose::STANDARD
            .encode(color.repeat((width * height) as usize));
        screen.process_bytes(
            format!("\x1b_Ga=T,f=32,s={width},v={height},C=1,q=2,{keys};{payload}\x1b\\")
                .as_bytes(),
        );
    }

    fn canvas(screen: &mut TerminalScreen) -> (Vec<TerminalImagePlacement>, RgbaImage) {
        let snapshot = screen.render_snapshot();
        let images = composite_terminal_images(&snapshot.images, 32, 16);
        let mut pixels = RgbaImage::new(320, 320);
        for image in &images {
            let source = image.image.pixels().unwrap();
            for (x, y, pixel) in source.to_rgba8().enumerate_pixels() {
                blend_rgba(
                    &mut pixels
                        .get_pixel_mut(x + image.col as u32 * 10, y + image.row as u32 * 20)
                        .0,
                    pixel.0,
                );
            }
        }
        (images, pixels)
    }

    #[test]
    fn composition_preserves_exact_z_planes_across_text_and_cell_backgrounds() {
        for z in [-1, i32::MIN / 2 - 1] {
            let mut screen = TerminalScreen::new(16, 32, 0);
            screen.process_bytes(b"\x1b[41mtext across the base\x1b[0m\r");
            transmit(
                &mut screen,
                &format!("i=1,z={z}"),
                320,
                320,
                [0, 0, 255, 255],
            );
            transmit(&mut screen, "i=2,z=1,X=3,Y=5", 3, 2, [255, 0, 0, 255]);
            let snapshot = screen.render_snapshot();
            let images = composite_terminal_images(&snapshot.images, 32, 16);
            let base = images
                .iter()
                .find(|p| p.z == z && p.col == 0 && p.row == 0)
                .unwrap();
            let patch = images.iter().find(|p| p.z == 1).unwrap();
            assert_eq!(
                base.image.pixels().unwrap().get_pixel(3, 5).0,
                [0, 0, 255, 255]
            );
            assert_eq!(
                patch.image.pixels().unwrap().get_pixel(3, 5).0,
                [255, 0, 0, 255]
            );
            assert_eq!(
                patch.image.pixels().unwrap().get_pixel(0, 0).0,
                [0, 0, 0, 0]
            );
            assert_ne!(base.image.stream_namespace, patch.image.stream_namespace);
            assert!(images.iter().all(|p| p.z == z || p.z == 1));
        }
    }

    #[test]
    fn final_placement_removal_releases_composed_pixels_but_keeps_definitions() {
        for reflow in [false, true] {
            let mut screen = TerminalScreen::new(16, 32, 0);
            screen.process_bytes(&[b'x'; 64]);
            screen.process_bytes(b"\x1b[H");
            transmit(&mut screen, "i=1,z=0", 320, 320, [0, 0, 255, 255]);
            transmit(&mut screen, "i=2,z=0,X=3,Y=5", 3, 2, [255, 0, 0, 255]);
            let (images, _) = canvas(&mut screen);
            let pixels: Vec<_> = images
                .iter()
                .map(|p| Arc::downgrade(p.image.pixels().unwrap()))
                .collect();
            drop(images);
            assert!(pixels.iter().all(|p| p.upgrade().is_some()));
            if reflow {
                screen.resize(16, 33);
            } else {
                screen.process_bytes(b"\x1b_Ga=d,d=a,q=2;\x1b\\");
            }
            assert!(pixels.iter().all(|p| p.upgrade().is_none()));
            assert!(screen.render_snapshot().images.is_empty());
            screen.process_bytes(b"\x1b_Ga=p,i=1,q=2;\x1b\\");
            assert!(
                !screen.render_snapshot().images.is_empty(),
                "lowercase delete/reflow keeps stored images"
            );
        }
    }

    #[test]
    fn patches_preserve_cell_edges_offsets_alpha_and_the_unscaled_toolbar() {
        let mut screen = TerminalScreen::new(16, 32, 0);
        screen.set_cell_size(TerminalCellSize {
            width: 10,
            height: 20,
        });
        transmit(&mut screen, "i=1,z=0", 320, 320, [0, 0, 255, 255]);
        transmit(&mut screen, "i=100000,z=1", 256, 51, [0, 255, 0, 255]);
        screen.process_bytes(b"\x1b[3;2H");
        transmit(&mut screen, "i=2,z=2,X=3,Y=15", 3, 2, [255, 0, 0, 128]);
        let (_, pixels) = canvas(&mut screen);
        assert_eq!(pixels.get_pixel(255, 50).0, [0, 255, 0, 255]);
        assert_eq!(pixels.get_pixel(256, 50).0, [0, 0, 255, 255]);
        assert_eq!(pixels.get_pixel(13, 54).0, [0, 0, 255, 255]);
        assert_eq!(pixels.get_pixel(13, 55).0, [128, 0, 127, 255]);
        assert_eq!(pixels.get_pixel(16, 55).0, [0, 0, 255, 255]);
        screen.process_bytes(b"\x1b_Ga=d,d=I,i=2,q=2;\x1b\\");
        assert_eq!(canvas(&mut screen).1.get_pixel(13, 55).0, [0, 0, 255, 255]);
    }

    #[test]
    fn unchanged_tiles_reuse_pixels_and_only_the_changed_tile_is_rebuilt() {
        let mut screen = TerminalScreen::new(16, 32, 0);
        transmit(&mut screen, "i=1,z=0", 320, 320, [0, 0, 255, 255]);
        transmit(&mut screen, "i=2,z=1,X=3,Y=5", 3, 2, [255, 0, 0, 255]);
        let (before, _) = canvas(&mut screen);
        let (same, _) = canvas(&mut screen);
        for (a, b) in before.iter().zip(&same) {
            assert!(Arc::ptr_eq(
                a.image.pixels().unwrap(),
                b.image.pixels().unwrap()
            ));
        }
        transmit(&mut screen, "i=2,z=1,X=3,Y=5", 3, 2, [0, 255, 0, 255]);
        let (after, pixels) = canvas(&mut screen);
        assert_eq!(pixels.get_pixel(3, 5).0, [0, 255, 0, 255]);
        let changed = before
            .iter()
            .zip(&after)
            .filter(|(a, b)| !Arc::ptr_eq(a.image.pixels().unwrap(), b.image.pixels().unwrap()))
            .count();
        assert_eq!(changed, 1, "a small patch must not rebuild every tile");
    }

    #[test]
    fn explicit_cell_bounds_preserve_the_source_aspect_ratio() {
        let mut screen = TerminalScreen::new(16, 32, 0);
        transmit(&mut screen, "i=1,z=0", 320, 320, [0, 0, 255, 255]);
        transmit(&mut screen, "i=2,z=1,c=2,r=2", 10, 10, [255, 0, 0, 255]);
        let (_, pixels) = canvas(&mut screen);
        assert_eq!(pixels.get_pixel(19, 19).0, [255, 0, 0, 255]);
        assert_eq!(pixels.get_pixel(19, 20).0, [0, 0, 255, 255]);
        transmit(&mut screen, "i=2,z=1,c=2", 10, 10, [0, 255, 0, 255]);
        assert_eq!(canvas(&mut screen).1.get_pixel(19, 20).0, [0, 0, 255, 255]);
    }

    #[test]
    fn replay_preserves_natural_size_and_subcell_placement() {
        let mut screen = TerminalScreen::new(16, 32, 0);
        transmit(&mut screen, "i=1,z=0", 320, 320, [0, 0, 255, 255]);
        screen.process_bytes(b"\x1b[8;16H");
        transmit(&mut screen, "i=2,p=1,z=2,X=9,Y=19", 3, 2, [255, 0, 0, 255]);
        let mut restored = TerminalScreen::new(16, 32, 0);
        restored.process_bytes(&screen.export_replay_bytes());
        assert_eq!(canvas(&mut screen).1, canvas(&mut restored).1);
    }

    fn reference_paint(layer: &Layer<'_>, canvas: &mut RgbaImage, origin: (i64, i64), scale: u32) {
        let pixels = layer.placement.image.pixels().unwrap();
        for (x, y, dest) in canvas.enumerate_pixels_mut() {
            let sx = i64::from(x) + origin.0 - layer.x;
            let sy = i64::from(y) + origin.1 - layer.y;
            if sx >= 0 && sx < i64::from(layer.width) && sy >= 0 && sy < i64::from(layer.height) {
                blend_rgba(
                    &mut dest.0,
                    pixels
                        .get_pixel(
                            (layer.source.x + sx as u32 / scale).min(pixels.width() - 1),
                            (layer.source.y + sy as u32 / scale).min(pixels.height() - 1),
                        )
                        .0,
                );
            }
        }
    }

    #[test]
    fn row_copy_preserves_crops_clipping_and_mixed_alpha() {
        let mut screen = TerminalScreen::new(16, 32, 0);
        transmit(&mut screen, "i=1,z=0", 20, 15, [0, 0, 0, 255]);
        let mut placement = screen.render_snapshot().images[0].clone();
        for format in [0, 1, 2] {
            let pixels = RgbaImage::from_fn(20, 15, |x, y| {
                image::Rgba([
                    x as u8 * 9,
                    y as u8 * 13,
                    70,
                    [0, 128, 255][(y % 3) as usize],
                ])
            });
            let pixels = DynamicImage::ImageRgba8(pixels);
            let pixels = match format {
                0 => pixels,
                1 => DynamicImage::ImageRgb8(pixels.to_rgb8()),
                _ => DynamicImage::ImageRgba16(pixels.to_rgba16()),
            };
            placement.image.source = ImageSource::Decoded(Arc::new(pixels));
            for origin in [(-3, -5), (0, 0), (7, 6), (30, 30)] {
                for source in [
                    TerminalImageCrop {
                        x: 0,
                        y: 0,
                        width: 20,
                        height: 15,
                    },
                    TerminalImageCrop {
                        x: 4,
                        y: 3,
                        width: 9,
                        height: 8,
                    },
                    TerminalImageCrop {
                        x: 17,
                        y: 13,
                        width: 9,
                        height: 8,
                    },
                ] {
                    for scale in [1, 2] {
                        let layer = Layer {
                            placement: &placement,
                            source,
                            x: 2,
                            y: 1,
                            width: source.width * scale,
                            height: source.height * scale,
                        };
                        let mut actual =
                            RgbaImage::from_pixel(12, 9, image::Rgba([30, 80, 90, 140]));
                        let mut expected = actual.clone();
                        reference_paint(&layer, &mut expected, origin, scale);
                        layer.paint(&mut actual, origin);
                        assert_eq!(
                            actual, expected,
                            "format={format}, scale={scale}, origin={origin:?}, crop={source:?}"
                        );
                    }
                }
            }
        }
    }
}
