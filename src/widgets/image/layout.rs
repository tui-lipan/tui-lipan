use std::path::Path;
use std::sync::{Mutex, OnceLock};

use image::GenericImageView;
use unicode_width::UnicodeWidthStr;

use super::{Image, ImageFit, ImageSource};
use crate::style::Length;

#[derive(Default)]
struct ImageMeasureCache {
    entries: Vec<(u64, (u32, u32))>,
}

impl ImageMeasureCache {
    fn get(&self, key: u64) -> Option<(u32, u32)> {
        self.entries
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
    }

    fn insert(&mut self, key: u64, value: (u32, u32)) {
        if let Some(idx) = self.entries.iter().position(|(k, _)| *k == key) {
            self.entries.remove(idx);
        }
        self.entries.push((key, value));
        if self.entries.len() > 128 {
            self.entries.remove(0);
        }
    }
}

fn measure_cache() -> &'static Mutex<ImageMeasureCache> {
    static CACHE: OnceLock<Mutex<ImageMeasureCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(ImageMeasureCache::default()))
}

/// The cell size in pixels images are encoded at, so layout reserves exactly the cells the renderer
/// draws.
fn cell_size_px() -> (u32, u32) {
    let size = crate::backend::ratatui_backend::image_support::picker_snapshot().font_size();
    (u32::from(size.width.max(1)), u32::from(size.height.max(1)))
}

/// How one axis of an image is sized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageAxis {
    /// Exactly this many cells.
    Fixed(u16),
    /// Follow the picture, within this many cells when bounded.
    Auto(Option<u16>),
}

/// Cells an image of `pixels` occupies. An auto axis follows the picture as the renderer fits it:
/// never past its natural size, and proportionally smaller when the other axis is fixed or either
/// bound is tighter, so a capped width does not leave empty rows below the picture. `Crop` keeps
/// its natural size clamped to the bounds, since it cuts rather than scales.
pub(crate) fn image_cells(
    pixels: (u32, u32),
    width: ImageAxis,
    height: ImageAxis,
    fit: ImageFit,
) -> (u16, u16) {
    let (cell_w, cell_h) = cell_size_px();
    let (pixel_w, pixel_h) = pixels;
    let to_cells =
        |pixels: f64, cell: u32| (pixels / f64::from(cell)).ceil().min(f64::from(u16::MAX)) as u16;
    let resolve = |axis: ImageAxis, natural: u16| match axis {
        ImageAxis::Fixed(cells) => cells,
        ImageAxis::Auto(bound) => bound.map_or(natural, |bound| natural.min(bound)),
    };
    if pixel_w == 0 || pixel_h == 0 {
        return (resolve(width, 0), resolve(height, 0));
    }
    let natural_w = to_cells(f64::from(pixel_w), cell_w);
    let natural_h = to_cells(f64::from(pixel_h), cell_h);
    if matches!(fit, ImageFit::Crop) {
        return (resolve(width, natural_w), resolve(height, natural_h));
    }

    let limit = |axis: ImageAxis, pixels: u32, cell: u32| match axis {
        ImageAxis::Fixed(cells) | ImageAxis::Auto(Some(cells)) => {
            Some(f64::from(cells) * f64::from(cell) / f64::from(pixels))
        }
        ImageAxis::Auto(None) => None,
    };
    let limit_w = limit(width, pixel_w, cell_w);
    let limit_h = limit(height, pixel_h, cell_h);
    let mut scale = match (limit_w, limit_h) {
        (Some(w), Some(h)) => w.min(h),
        (Some(limit), None) | (None, Some(limit)) => limit,
        (None, None) => 1.0,
    };
    // Only a fixed axis may enlarge the picture, and only a fit that scales up.
    let fixed = matches!(width, ImageAxis::Fixed(_)) || matches!(height, ImageAxis::Fixed(_));
    if !(fixed && matches!(fit, ImageFit::Scale | ImageFit::Cover)) {
        scale = scale.min(1.0);
    }
    let fitted_w = (f64::from(pixel_w) * scale).round().max(1.0);
    let fitted_h = (f64::from(pixel_h) * scale).round().max(1.0);
    let cells = |axis: ImageAxis, fitted: f64, cell: u32| match axis {
        ImageAxis::Fixed(cells) => cells,
        ImageAxis::Auto(bound) => {
            let cells = to_cells(fitted, cell).max(1);
            bound.map_or(cells, |bound| cells.min(bound))
        }
    };
    (
        cells(width, fitted_w, cell_w),
        cells(height, fitted_h, cell_h),
    )
}

/// Pixel dimensions of an undecoded source, cached by `key`.
pub(crate) fn source_pixel_size(source: &ImageSource, key: u64) -> Option<(u32, u32)> {
    if let Ok(cache) = measure_cache().lock()
        && let Some(size) = cache.get(key)
    {
        return Some(size);
    }

    let dims = match source {
        ImageSource::Path(path) => image::ImageReader::open(Path::new(path.as_ref()))
            .ok()?
            .decode()
            .ok()
            .map(|img| img.dimensions()),
        ImageSource::Bytes(bytes) => image::load_from_memory(bytes.as_ref())
            .ok()
            .map(|img| img.dimensions()),
    }?;

    if let Ok(mut cache) = measure_cache().lock() {
        cache.insert(key, dims);
    }

    Some(dims)
}

/// Measure an image within `max_w` x `max_h` cells.
pub(crate) fn measure_image_constrained(
    image: &Image,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    let source_hash = super::node::source_hash(&image.source);
    let pixels = source_pixel_size(&image.source, source_hash);
    let axis = |length: Length, bound: Option<u16>| match length {
        Length::Px(px) => ImageAxis::Fixed(px),
        _ => ImageAxis::Auto(bound),
    };
    sized_with_alt(
        image,
        pixels,
        axis(image.width, max_w),
        axis(image.height, max_h),
    )
}

/// [`image_cells`] for a decoded picture, or room for the alt text when there is none.
pub(crate) fn sized_with_alt(
    image: &Image,
    pixels: Option<(u32, u32)>,
    width: ImageAxis,
    height: ImageAxis,
) -> (u16, u16) {
    if let Some(pixels) = pixels.filter(|(w, h)| *w > 0 && *h > 0) {
        return image_cells(pixels, width, height, image.fit);
    }
    let alt_w = image
        .alt
        .as_ref()
        .map(|alt| UnicodeWidthStr::width(alt.as_ref()).min(u16::MAX as usize) as u16)
        .unwrap_or(0);
    let auto = |axis: ImageAxis, natural: u16| match axis {
        ImageAxis::Fixed(cells) => cells,
        ImageAxis::Auto(bound) => bound.map_or(natural, |bound| natural.min(bound)),
    };
    (auto(width, alt_w), auto(height, u16::from(alt_w > 0)))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

    use super::measure_image_constrained;
    use crate::style::Length;
    use crate::widgets::{Image, ImageFit};

    // The picker these tests see is the 10x20 pixel half-block default.
    fn measure_image(image: &Image) -> (u16, u16) {
        measure_image_constrained(image, None, None)
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let rgba = RgbaImage::from_pixel(width, height, Rgba([0x22, 0x44, 0x88, 0xFF]));
        let image = DynamicImage::ImageRgba8(rgba);
        let mut out = Cursor::new(Vec::new());
        image
            .write_to(&mut out, ImageFormat::Png)
            .expect("png encoding should succeed");
        out.into_inner()
    }

    #[test]
    fn auto_size_uses_natural_dimensions() {
        let image = Image::from_bytes(png_bytes(19, 39));
        assert_eq!(measure_image(&image), (2, 2));
    }

    /// A width cap scales the whole picture down, so the height does not keep rows it cannot use.
    #[test]
    fn capped_width_shrinks_auto_height_with_the_picture() {
        let image = Image::from_bytes(png_bytes(400, 400));
        assert_eq!(measure_image(&image), (40, 20));
        assert_eq!(
            measure_image_constrained(&image, Some(20), Some(15)),
            (20, 10)
        );
        assert_eq!(
            measure_image_constrained(&image, Some(38), Some(13)),
            (26, 13)
        );
    }

    /// A fixed width sizes the auto height to the picture's aspect ratio; `Scale` may enlarge it,
    /// `Contain` never past its natural size, and `Crop` keeps its natural rows.
    #[test]
    fn fixed_width_derives_auto_height_per_fit() {
        let scale = Image::from_bytes(png_bytes(100, 100))
            .fit(ImageFit::Scale)
            .width(Length::Px(20));
        assert_eq!(measure_image(&scale), (20, 10));
        let contain = Image::from_bytes(png_bytes(100, 100)).width(Length::Px(20));
        assert_eq!(measure_image(&contain), (20, 5));
        let crop = Image::from_bytes(png_bytes(400, 400))
            .fit(ImageFit::Crop)
            .width(Length::Px(10));
        assert_eq!(measure_image(&crop), (10, 20));
    }

    #[test]
    fn alt_text_measures_when_decode_fails() {
        let image = Image::from_bytes(vec![1, 2, 3]).alt("broken");
        assert_eq!(measure_image(&image), (6, 1));
    }

    #[test]
    fn fixed_px_overrides_natural_measurement() {
        let image = Image::from_bytes(png_bytes(9, 17))
            .width(Length::Px(10))
            .height(Length::Px(4));
        assert_eq!(measure_image(&image), (10, 4));
    }
}
