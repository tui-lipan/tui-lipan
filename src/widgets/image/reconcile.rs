use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use web_time::Instant;

use image::AnimationDecoder;
use unicode_width::UnicodeWidthStr;

use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{apply_constraints, reuse_or_replace_kind};
use crate::style::{LayoutConstraints, Length, Rect};

use super::layout::{pixels_to_cells, source_natural_size};
use super::node::{ImageAnimation, ImageNode, source_hash};
use super::{Image, ImageSource};

type DecodedImageSource = (Option<Arc<image::DynamicImage>>, Option<ImageAnimation>);

fn source_bytes(source: &ImageSource) -> Result<Arc<[u8]>, Arc<str>> {
    match source {
        ImageSource::Path(path) => fs::read(Path::new(path.as_ref()))
            .map(Arc::<[u8]>::from)
            .map_err(|err| Arc::<str>::from(format!("image open failed: {err}"))),
        ImageSource::Bytes(bytes) => Ok(Arc::clone(bytes)),
    }
}

fn decode_gif(bytes: Arc<[u8]>) -> Result<Option<ImageAnimation>, Arc<str>> {
    if gif_preload_enabled() && bytes.len() <= gif_preload_max_bytes() {
        let preloaded = decode_gif_preloaded(bytes.as_ref())?;
        if preloaded.is_some() {
            return Ok(preloaded);
        }
    }

    ImageAnimation::from_gif_stream(bytes)
}

fn decode_gif_preloaded(bytes: &[u8]) -> Result<Option<ImageAnimation>, Arc<str>> {
    let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
        .map_err(|err| Arc::<str>::from(format!("gif decode failed: {err}")))?;
    let mut frames = decoder.into_frames();
    let mut collected = Vec::new();

    let frame_cap = gif_preload_max_frames();
    let deadline = Instant::now() + Duration::from_millis(gif_preload_budget_ms() as u64);

    loop {
        if collected.len() >= frame_cap || Instant::now() >= deadline {
            return Ok(None);
        }

        let Some(frame) = frames.next() else {
            break;
        };

        let frame =
            frame.map_err(|err| Arc::<str>::from(format!("gif frame decode failed: {err}")))?;
        collected.push(frame);
    }

    Ok(ImageAnimation::from_frames(collected))
}

fn parse_bool_env(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn gif_preload_enabled() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("TUI_LIPAN_IMAGE_GIF_PRELOAD")
            .ok()
            .as_deref()
            .and_then(parse_bool_env)
            .unwrap_or(true)
    })
}

fn gif_preload_max_bytes() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("TUI_LIPAN_IMAGE_GIF_PRELOAD_MAX_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(262_144)
            .max(1)
    })
}

fn gif_preload_max_frames() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("TUI_LIPAN_IMAGE_GIF_PRELOAD_MAX_FRAMES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(24)
            .max(2)
    })
}

fn gif_preload_budget_ms() -> u32 {
    static VALUE: OnceLock<u32> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("TUI_LIPAN_IMAGE_GIF_PRELOAD_BUDGET_MS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(16)
            .max(1)
    })
}

fn decode_webp(bytes: &[u8]) -> Result<Option<ImageAnimation>, Arc<str>> {
    let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))
        .map_err(|err| Arc::<str>::from(format!("webp decode failed: {err}")))?;
    let frames = decoder
        .into_frames()
        .collect_frames()
        .map_err(|err| Arc::<str>::from(format!("webp frame decode failed: {err}")))?;
    Ok(ImageAnimation::from_frames(frames))
}

fn decode_apng(bytes: &[u8]) -> Result<Option<ImageAnimation>, Arc<str>> {
    let decoder = image::codecs::png::PngDecoder::new(Cursor::new(bytes))
        .map_err(|err| Arc::<str>::from(format!("png decode failed: {err}")))?;

    if !decoder
        .is_apng()
        .map_err(|err| Arc::<str>::from(format!("png decode failed: {err}")))?
    {
        return Ok(None);
    }

    let frames = decoder
        .apng()
        .map_err(|err| Arc::<str>::from(format!("apng decode failed: {err}")))?
        .into_frames()
        .collect_frames()
        .map_err(|err| Arc::<str>::from(format!("apng frame decode failed: {err}")))?;
    Ok(ImageAnimation::from_frames(frames))
}

fn decode_animated(bytes: Arc<[u8]>) -> Result<Option<ImageAnimation>, Arc<str>> {
    let format = image::guess_format(bytes.as_ref()).ok();

    match format {
        Some(image::ImageFormat::Gif) => decode_gif(bytes),
        Some(image::ImageFormat::WebP) => decode_webp(bytes.as_ref()),
        Some(image::ImageFormat::Png) => decode_apng(bytes.as_ref()),
        _ => Ok(None),
    }
}

fn decode_source(source: &ImageSource) -> Result<DecodedImageSource, Arc<str>> {
    let bytes = source_bytes(source)?;

    if let Some(animation) = decode_animated(Arc::clone(&bytes))? {
        return Ok((None, Some(animation)));
    }

    image::load_from_memory(bytes.as_ref())
        .map(|img| (Some(Arc::new(img)), None))
        .map_err(|err| Arc::<str>::from(format!("image decode failed: {err}")))
}

pub fn reconcile_image(
    tree: &mut NodeTree,
    id: NodeId,
    image: &Image,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    let hash = source_hash(&image.source);

    let (mut decoded, mut animation, mut decode_error) = (None, None, None);
    {
        let node = tree.node_mut(id);
        if let NodeKind::Image(existing) = &node.kind
            && existing.source_hash == hash
        {
            decoded = existing.decoded.clone();
            animation = existing.animation.clone();
            decode_error = existing.decode_error.clone();
        }
    }

    if decoded.is_none() && animation.is_none() {
        match decode_source(&image.source) {
            Ok((img, animated)) => {
                decoded = img;
                animation = animated;
                decode_error = None;
            }
            Err(err) => decode_error = Some(err),
        }
    }

    let natural_size = if let Some(animated) = &animation {
        animated
            .current_image()
            .map(|frame| pixels_to_cells(frame.width(), frame.height()))
            .unwrap_or((0, 0))
    } else if let Some(decoded) = &decoded {
        pixels_to_cells(decoded.width(), decoded.height())
    } else {
        source_natural_size(&image.source, hash).unwrap_or((0, 0))
    };

    let alt_w = image
        .alt
        .as_ref()
        .map(|alt| UnicodeWidthStr::width(alt.as_ref()).min(u16::MAX as usize) as u16)
        .unwrap_or(0);

    let avail_w = rect.w;
    let avail_h = rect.h;
    let mut rect = rect;
    if matches!(image.width, Length::Auto) {
        rect.w = natural_size.0.max(alt_w).min(rect.w);
    }
    if matches!(image.height, Length::Auto) {
        rect.h = if natural_size.1 > 0 {
            natural_size.1
        } else if alt_w > 0 {
            1
        } else {
            0
        }
        .min(rect.h);
    }
    apply_constraints(&mut rect, constraints, avail_w, avail_h);

    let node = tree.node_mut(id);
    node.rect = rect;
    node.children.clear();
    let replace_decoded = decoded.clone();
    let replace_animation = animation.clone();
    let replace_decode_error = decode_error.clone();

    reuse_or_replace_kind(
        &mut node.kind,
        |kind| {
            if let NodeKind::Image(existing) = kind {
                existing.source = image.source.clone();
                existing.style = image.style;
                existing.width = image.width;
                existing.height = image.height;
                existing.fit = image.fit;
                existing.protocol = image.protocol;
                existing.alt = image.alt.clone();
                existing.playback = image.playback;
                existing.repeat = image.repeat;
                existing.speed_percent = image.speed_percent;
                existing.source_hash = hash;
                existing.decoded = decoded.clone();
                existing.animation = animation.clone();
                existing.decode_error = decode_error.clone();
                true
            } else {
                false
            }
        },
        || {
            NodeKind::Image(ImageNode {
                source: image.source.clone(),
                style: image.style,
                width: image.width,
                height: image.height,
                fit: image.fit,
                protocol: image.protocol,
                alt: image.alt.clone(),
                playback: image.playback,
                repeat: image.repeat,
                speed_percent: image.speed_percent,
                source_hash: hash,
                decoded: replace_decoded,
                animation: replace_animation,
                decode_error: replace_decode_error,
            })
        },
    );

    id
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Arc;

    use image::{Delay, DynamicImage, Frame, ImageFormat, Rgba, RgbaImage};

    use super::super::node::ImageAnimation;
    use super::reconcile_image;
    use crate::core::node::{NodeKind, NodeTree};
    use crate::style::{LayoutConstraints, Rect};
    use crate::widgets::{Image, ImagePlayback, ImageRepeat};

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let rgba = RgbaImage::from_pixel(width, height, Rgba([0x22, 0x44, 0x88, 0xFF]));
        let image = DynamicImage::ImageRgba8(rgba);
        let mut out = Cursor::new(Vec::new());
        image
            .write_to(&mut out, ImageFormat::Png)
            .expect("png encoding should succeed");
        out.into_inner()
    }

    fn gif_bytes_with_frames(width: u32, height: u32, frame_count: usize) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut out);
            let frames = (0..frame_count).map(|idx| {
                let mut rgba = [0x44, 0x44, 0xFF, 0xFF];
                if idx % 2 == 0 {
                    rgba = [0xFF, 0x44, 0x44, 0xFF];
                }
                Frame::from_parts(
                    RgbaImage::from_pixel(width, height, Rgba(rgba)),
                    0,
                    0,
                    Delay::from_numer_denom_ms(30, 1),
                )
            });

            encoder
                .encode_frames(frames)
                .expect("gif encoding should succeed");
        }
        out.into_inner()
    }

    fn gif_bytes(width: u32, height: u32) -> Vec<u8> {
        gif_bytes_with_frames(width, height, 2)
    }

    #[test]
    fn auto_rect_uses_decoded_image_size() {
        let mut tree = NodeTree::new();
        let id = tree.alloc();

        let widget = Image::from_bytes(png_bytes(9, 17));
        let rect = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };

        reconcile_image(&mut tree, id, &widget, rect, &LayoutConstraints::default());

        let node = tree.node(id);
        assert_eq!(node.rect.w, 2);
        assert_eq!(node.rect.h, 2);

        let NodeKind::Image(image_node) = &node.kind else {
            panic!("expected image node");
        };
        assert!(image_node.decoded.is_some());
        assert!(image_node.animation.is_none());
        assert!(image_node.decode_error.is_none());
    }

    #[test]
    fn invalid_source_falls_back_to_alt_dimensions() {
        let mut tree = NodeTree::new();
        let id = tree.alloc();

        let widget = Image::from_bytes(vec![1, 2, 3]).alt("oops");
        let rect = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };

        reconcile_image(&mut tree, id, &widget, rect, &LayoutConstraints::default());

        let node = tree.node(id);
        assert_eq!(node.rect.w, 4);
        assert_eq!(node.rect.h, 1);

        let NodeKind::Image(image_node) = &node.kind else {
            panic!("expected image node");
        };
        assert!(image_node.decoded.is_none());
        assert!(image_node.animation.is_none());
        assert!(image_node.decode_error.is_some());
    }

    #[test]
    fn gif_source_creates_animation_state() {
        let mut tree = NodeTree::new();
        let id = tree.alloc();
        let rect = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };

        let widget = Image::from_bytes(gif_bytes(16, 16));
        reconcile_image(&mut tree, id, &widget, rect, &LayoutConstraints::default());

        let node = tree.node(id);
        let NodeKind::Image(image_node) = &node.kind else {
            panic!("expected image node");
        };
        assert!(image_node.is_animated());
        assert_eq!(image_node.current_frame_index(), 0);
        assert!(image_node.decoded.is_none());
        assert!(matches!(
            image_node.animation,
            Some(ImageAnimation::Buffered(_))
        ));
    }

    #[test]
    fn large_gif_source_uses_streaming_decoder() {
        let mut tree = NodeTree::new();
        let id = tree.alloc();
        let rect = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };

        let mut big = gif_bytes(16, 16);
        while big.len() <= 1_048_576 {
            let cloned = big.clone();
            big.extend_from_slice(&cloned);
        }

        let widget = Image::from_bytes(big);
        reconcile_image(&mut tree, id, &widget, rect, &LayoutConstraints::default());

        let node = tree.node(id);
        let NodeKind::Image(image_node) = &node.kind else {
            panic!("expected image node");
        };

        assert!(matches!(
            image_node.animation,
            Some(ImageAnimation::GifStream(_))
        ));
    }

    #[test]
    fn small_many_frame_gif_uses_streaming_decoder() {
        let mut tree = NodeTree::new();
        let id = tree.alloc();
        let rect = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };

        let widget = Image::from_bytes(gif_bytes_with_frames(8, 8, 40));
        reconcile_image(&mut tree, id, &widget, rect, &LayoutConstraints::default());

        let node = tree.node(id);
        let NodeKind::Image(image_node) = &node.kind else {
            panic!("expected image node");
        };

        assert!(matches!(
            image_node.animation,
            Some(ImageAnimation::GifStream(_))
        ));
    }

    #[test]
    fn gif_animation_advances_frames_on_tick() {
        let mut tree = NodeTree::new();
        let id = tree.alloc();
        let rect = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };

        let widget = Image::from_bytes(gif_bytes(8, 8));
        reconcile_image(&mut tree, id, &widget, rect, &LayoutConstraints::default());

        let node = tree.node_mut(id);
        let NodeKind::Image(image_node) = &mut node.kind else {
            panic!("expected image node");
        };
        assert_eq!(image_node.current_frame_index(), 0);
        assert!(!image_node.tick_animation(15));
        assert_eq!(image_node.current_frame_index(), 0);
        assert!(image_node.tick_animation(16));
        assert_eq!(image_node.current_frame_index(), 1);
    }

    #[test]
    fn paused_animation_does_not_advance() {
        let mut tree = NodeTree::new();
        let id = tree.alloc();
        let rect = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };

        let widget = Image::from_bytes(gif_bytes(8, 8)).playback(ImagePlayback::Paused);
        reconcile_image(&mut tree, id, &widget, rect, &LayoutConstraints::default());

        let node = tree.node_mut(id);
        let NodeKind::Image(image_node) = &mut node.kind else {
            panic!("expected image node");
        };
        assert_eq!(image_node.current_frame_index(), 0);
        assert!(!image_node.tick_animation(100));
        assert_eq!(image_node.current_frame_index(), 0);
    }

    #[test]
    fn once_repeat_stops_on_last_frame() {
        let mut tree = NodeTree::new();
        let id = tree.alloc();
        let rect = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };

        let widget = Image::from_bytes(gif_bytes(8, 8)).repeat(ImageRepeat::Once);
        reconcile_image(&mut tree, id, &widget, rect, &LayoutConstraints::default());

        let node = tree.node_mut(id);
        let NodeKind::Image(image_node) = &mut node.kind else {
            panic!("expected image node");
        };

        assert_eq!(image_node.current_frame_index(), 0);
        assert!(image_node.tick_animation(30));
        assert_eq!(image_node.current_frame_index(), 1);
        assert!(!image_node.tick_animation(30));
        assert_eq!(image_node.current_frame_index(), 1);
    }

    #[test]
    fn speed_percent_accelerates_animation() {
        let mut tree = NodeTree::new();
        let id = tree.alloc();
        let rect = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };

        let widget = Image::from_bytes(gif_bytes(8, 8)).speed_percent(200);
        reconcile_image(&mut tree, id, &widget, rect, &LayoutConstraints::default());

        let node = tree.node_mut(id);
        let NodeKind::Image(image_node) = &mut node.kind else {
            panic!("expected image node");
        };

        assert_eq!(image_node.current_frame_index(), 0);
        assert!(!image_node.tick_animation(14));
        assert_eq!(image_node.current_frame_index(), 0);
        assert!(image_node.tick_animation(1));
        assert_eq!(image_node.current_frame_index(), 1);
    }

    #[test]
    fn constraints_apply_after_auto_sizing() {
        let mut tree = NodeTree::new();
        let id = tree.alloc();

        let widget = Image::from_bytes(png_bytes(9, 17));
        let rect = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };
        let constraints = LayoutConstraints::default()
            .min_width(crate::style::Length::Px(5))
            .max_height(crate::style::Length::Px(1));

        reconcile_image(&mut tree, id, &widget, rect, &constraints);

        let node = tree.node(id);
        assert_eq!(node.rect.w, 5);
        assert_eq!(node.rect.h, 1);
    }

    #[test]
    fn same_source_reuses_decoded_arc() {
        let mut tree = NodeTree::new();
        let id = tree.alloc();
        let rect = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };

        let widget = Image::from_bytes(png_bytes(16, 16));
        reconcile_image(&mut tree, id, &widget, rect, &LayoutConstraints::default());
        let first = match &tree.node(id).kind {
            NodeKind::Image(image_node) => image_node.decoded.clone().expect("decoded image"),
            _ => panic!("expected image node"),
        };

        reconcile_image(&mut tree, id, &widget, rect, &LayoutConstraints::default());
        let second = match &tree.node(id).kind {
            NodeKind::Image(image_node) => image_node.decoded.clone().expect("decoded image"),
            _ => panic!("expected image node"),
        };

        assert!(Arc::ptr_eq(&first, &second));
    }
}
