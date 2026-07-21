use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;

use ratatui::layout::Alignment;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui_image::Image as RatatuiImageWidget;
use ratatui_image::Resize;
use ratatui_image::picker::ProtocolType;
use ratatui_image::protocol::Protocol;

use crate::backend::ratatui_backend::common::{to_ratatui_rect, to_ratatui_style};
use crate::backend::ratatui_backend::image_support;
use crate::style::resolve::resolve_base_style;
use crate::style::{Rect, Theme};
use crate::widgets::internal::ImageNode;
use crate::widgets::{ImageFit, ImageProtocol};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct RenderCacheKey {
    source_hash: u64,
    frame_index: usize,
    width: u16,
    height: u16,
    background_rgb: Option<(u8, u8, u8)>,
    fit: ImageFit,
    protocol: ImageProtocol,
    resolved_protocol: ImageProtocol,
}

struct CacheEntry {
    key: RenderCacheKey,
    protocol: Arc<Protocol>,
    estimated_bytes: usize,
}

#[derive(Clone)]
struct EncodeRequest {
    source_hash: u64,
    key: RenderCacheKey,
    image: Arc<image::DynamicImage>,
}

#[derive(Default)]
struct ImageRenderCache {
    entries: Vec<CacheEntry>,
    total_estimated_bytes: usize,
}

impl ImageRenderCache {
    fn get(&mut self, key: &RenderCacheKey) -> Option<Arc<Protocol>> {
        let idx = self.entries.iter().position(|entry| &entry.key == key)?;
        let entry = self.entries.remove(idx);
        let protocol = Arc::clone(&entry.protocol);
        self.entries.push(entry);
        Some(protocol)
    }

    fn get_latest_compatible(&mut self, key: &RenderCacheKey) -> Option<Arc<Protocol>> {
        let idx = self.entries.iter().rposition(|entry| {
            entry.key.source_hash == key.source_hash
                && entry.key.width == key.width
                && entry.key.height == key.height
                && entry.key.background_rgb == key.background_rgb
                && entry.key.fit == key.fit
                && entry.key.protocol == key.protocol
                && entry.key.resolved_protocol == key.resolved_protocol
        })?;

        let entry = self.entries.remove(idx);
        let protocol = Arc::clone(&entry.protocol);
        self.entries.push(entry);
        Some(protocol)
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

    fn insert(&mut self, key: RenderCacheKey, protocol: Arc<Protocol>, estimated_bytes: usize) {
        const MAX_ENTRIES: usize = 256;
        const MAX_ENTRIES_PER_SOURCE: usize = 24;
        const MAX_TOTAL_ESTIMATED_BYTES: usize = 24 * 1024 * 1024;

        if let Some(idx) = self.entries.iter().position(|entry| entry.key == key) {
            self.remove_at(idx);
        }

        while self
            .entries
            .iter()
            .filter(|entry| entry.key.source_hash == key.source_hash)
            .count()
            >= MAX_ENTRIES_PER_SOURCE
        {
            let oldest_same_source = self
                .entries
                .iter()
                .position(|entry| entry.key.source_hash == key.source_hash);
            if let Some(idx) = oldest_same_source {
                self.remove_at(idx);
            } else {
                break;
            }
        }

        self.entries.push(CacheEntry {
            key,
            protocol,
            estimated_bytes,
        });
        self.total_estimated_bytes = self.total_estimated_bytes.saturating_add(estimated_bytes);

        while self.entries.len() > MAX_ENTRIES
            || self.total_estimated_bytes > MAX_TOTAL_ESTIMATED_BYTES
        {
            self.remove_at(0);
        }
    }
}

#[derive(Default)]
struct AsyncEncoderInner {
    cache: ImageRenderCache,
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
    fn cache_get(&self, key: &RenderCacheKey) -> Option<Arc<Protocol>> {
        let Ok(mut inner) = self.inner.lock() else {
            return None;
        };
        inner.cache.get(key)
    }

    fn cache_get_latest_compatible(&self, key: &RenderCacheKey) -> Option<Arc<Protocol>> {
        let Ok(mut inner) = self.inner.lock() else {
            return None;
        };
        inner.cache.get_latest_compatible(key)
    }

    fn enqueue(&self, request: EncodeRequest) {
        const MAX_QUEUED_SOURCES: usize = 48;

        let Ok(mut inner) = self.inner.lock() else {
            return;
        };

        let source_hash = request.source_hash;

        if inner
            .in_flight_keys
            .get(&source_hash)
            .is_some_and(|key| *key == request.key)
        {
            return;
        }

        if inner
            .queued
            .get(&source_hash)
            .is_some_and(|existing| existing.key == request.key)
        {
            return;
        }

        let inserted_new = inner.queued.insert(source_hash, request).is_none();
        if !inserted_new {
            return;
        }

        while inner.queue.len() >= MAX_QUEUED_SOURCES {
            let Some(evicted_source) = inner.queue.pop_front() else {
                break;
            };
            inner.queued.remove(&evicted_source);
        }

        inner.queue.push_back(source_hash);
        self.wake.notify_one();
    }

    fn next_request_blocking(&self) -> EncodeRequest {
        let mut inner = self
            .inner
            .lock()
            .expect("image async encoder lock poisoned");

        loop {
            while let Some(source_hash) = inner.queue.pop_front() {
                let Some(request) = inner.queued.remove(&source_hash) else {
                    continue;
                };

                inner.in_flight.insert(source_hash);
                inner.in_flight_keys.insert(source_hash, request.key);
                return request;
            }

            inner = self
                .wake
                .wait(inner)
                .expect("image async encoder lock poisoned");
        }
    }

    fn complete_request(&self, request: &EncodeRequest, protocol: Option<Protocol>) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };

        inner.in_flight.remove(&request.source_hash);
        inner.in_flight_keys.remove(&request.source_hash);

        let Some(protocol) = protocol else {
            return;
        };

        let estimated_bytes = estimate_protocol_bytes_from_key(request.key);
        inner
            .cache
            .insert(request.key, Arc::new(protocol), estimated_bytes);
        protocol_ready_epoch_counter().fetch_add(1, Ordering::Relaxed);
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

fn fit_to_resize(fit: ImageFit) -> Resize {
    match fit {
        ImageFit::Contain => Resize::Fit(None),
        ImageFit::Crop => Resize::Crop(None),
        ImageFit::Scale => Resize::Scale(None),
    }
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

fn parse_bool_env(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn auto_anim_halfblocks_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("TUI_LIPAN_IMAGE_AUTO_ANIM_HALF_BLOCKS")
            .ok()
            .as_deref()
            .and_then(parse_bool_env)
            .unwrap_or(false)
    })
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

fn should_prefer_halfblocks_for_animation(node: &ImageNode, draw_rect: Rect) -> bool {
    const FAST_ANIMATION_SPEED_PERCENT: u16 = 240;
    const LARGE_ANIMATION_AREA_CELLS: u32 = 3200;

    if !auto_anim_halfblocks_enabled() || !node.is_animated() {
        return false;
    }

    let area = u32::from(draw_rect.w).saturating_mul(u32::from(draw_rect.h));
    node.speed_percent >= FAST_ANIMATION_SPEED_PERCENT && area >= LARGE_ANIMATION_AREA_CELLS
}

fn estimate_protocol_bytes(draw_rect: Rect, resolved: ImageProtocol) -> usize {
    let area = usize::from(draw_rect.w).saturating_mul(usize::from(draw_rect.h));
    let per_cell = match resolved {
        ImageProtocol::Halfblocks => 8,
        ImageProtocol::Kitty
        | ImageProtocol::Iterm2
        | ImageProtocol::Sixel
        | ImageProtocol::Auto => 16,
    };
    area.saturating_mul(per_cell)
}

fn estimate_protocol_bytes_from_key(key: RenderCacheKey) -> usize {
    estimate_protocol_bytes(
        Rect {
            x: 0,
            y: 0,
            w: key.width,
            h: key.height,
        },
        key.resolved_protocol,
    )
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
    };

    let target_w_cells = target_w_px.div_ceil(cell_w).max(1).min(u32::from(bounds.w)) as u16;
    let target_h_cells = target_h_px.div_ceil(cell_h).max(1).min(u32::from(bounds.h)) as u16;

    Rect {
        x: bounds.x,
        y: bounds.y,
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
    let requested_protocol = if matches!(node.protocol, ImageProtocol::Auto)
        && should_prefer_halfblocks_for_animation(node, draw_rect)
    {
        Some(ProtocolType::Halfblocks)
    } else {
        requested_protocol_type(node.protocol)
    };

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
        background_rgb: effective_background_rgb,
        fit: node.fit,
        protocol: node.protocol,
        resolved_protocol: resolved,
    };

    Some(EncodeRequest {
        source_hash: node.source_hash,
        key,
        image: decoded,
    })
}

fn encode_request(request: &EncodeRequest) -> Option<Protocol> {
    let mut picker = image_support::picker_snapshot();
    if let Some(protocol_type) = resolved_protocol_type(request.key.resolved_protocol) {
        picker.set_protocol_type(protocol_type);
    }
    if let Some((r, g, b)) = request.key.background_rgb {
        picker.set_background_color(Some(image::Rgba([r, g, b, 255])));
    }

    let size = ratatui::layout::Size::new(request.key.width, request.key.height);
    let resize = fit_to_resize(request.key.fit);
    picker
        .new_protocol((*request.image).clone(), size, resize)
        .ok()
}

enum ProtocolResolve {
    Ready(Arc<Protocol>),
    Stale(Arc<Protocol>),
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

    let stale = encoder.cache_get_latest_compatible(&request.key);

    encoder.enqueue(request);
    if let Some(protocol) = stale {
        ProtocolResolve::Stale(protocol)
    } else {
        ProtocolResolve::Pending
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

    // If the clip rect cuts into the image rect, the image is only partially
    // visible (e.g. scrolled halfway out of a ScrollView).  Terminal image
    // protocols cannot crop an already-encoded image, so render a placeholder
    // instead of showing a shrunk version.
    let image_clipped = clip_rect.is_some_and(|clip| {
        let visible = image_rect.intersection(&clip);
        visible.w < image_rect.w || visible.h < image_rect.h
    });

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
        clear_image_region(f, draw_rect, style);
        render_placeholder_frame_clipped(f, image_rect, draw_rect, style, None);
        return;
    }

    match resolve_protocol_async(node, image_rect, background_rgb) {
        ProtocolResolve::Ready(protocol) | ProtocolResolve::Stale(protocol) => {
            let widget = RatatuiImageWidget::new(protocol.as_ref());
            f.render_widget(widget, to_ratatui_rect(image_rect));
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
