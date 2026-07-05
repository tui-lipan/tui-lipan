use std::path::Path;
use std::sync::{Mutex, OnceLock};

use image::GenericImageView;
use unicode_width::UnicodeWidthStr;

use super::{Image, ImageSource};
use crate::style::Length;

const DEFAULT_CELL_WIDTH_PX: u32 = 8;
const DEFAULT_CELL_HEIGHT_PX: u32 = 16;

#[derive(Default)]
struct ImageMeasureCache {
    entries: Vec<(u64, (u16, u16))>,
}

impl ImageMeasureCache {
    fn get(&self, key: u64) -> Option<(u16, u16)> {
        self.entries
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
    }

    fn insert(&mut self, key: u64, value: (u16, u16)) {
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

pub(crate) fn pixels_to_cells(width: u32, height: u32) -> (u16, u16) {
    let cells_w = width.div_ceil(DEFAULT_CELL_WIDTH_PX).min(u16::MAX as u32) as u16;
    let cells_h = height.div_ceil(DEFAULT_CELL_HEIGHT_PX).min(u16::MAX as u32) as u16;
    (cells_w, cells_h)
}

pub(crate) fn source_natural_size(source: &ImageSource, key: u64) -> Option<(u16, u16)> {
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

    let cells = pixels_to_cells(dims.0, dims.1);

    if let Ok(mut cache) = measure_cache().lock() {
        cache.insert(key, cells);
    }

    Some(cells)
}

pub fn measure_image(image: &Image) -> (u16, u16) {
    let source_hash = super::node::source_hash(&image.source);
    let natural = source_natural_size(&image.source, source_hash).unwrap_or((0, 0));

    let alt_w = image
        .alt
        .as_ref()
        .map(|alt| UnicodeWidthStr::width(alt.as_ref()).min(u16::MAX as usize) as u16)
        .unwrap_or(0);

    let w = match image.width {
        Length::Px(px) => px,
        _ => natural.0.max(alt_w),
    };
    let h = match image.height {
        Length::Px(px) => px,
        _ => {
            if natural.1 > 0 {
                natural.1
            } else if alt_w > 0 {
                1
            } else {
                0
            }
        }
    };

    (w, h)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

    use super::measure_image;
    use crate::style::Length;
    use crate::widgets::Image;

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
        let image = Image::from_bytes(png_bytes(9, 17));
        assert_eq!(measure_image(&image), (2, 2));
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
