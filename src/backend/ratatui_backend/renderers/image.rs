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

use crate::backend::ratatui_backend::common::{to_ratatui_rect, to_ratatui_style};
use crate::backend::ratatui_backend::image_support;
use crate::style::resolve::resolve_base_style;
use crate::style::{Rect, Theme};
use crate::widgets::internal::ImageNode;
use crate::widgets::{ImageFit, ImageProtocol};

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
    id_color: String,
    id_extra: u16,
    size: ratatui::layout::Size,
}

#[cfg(feature = "terminal-images")]
impl CompressedKitty {
    fn new(image: &image::DynamicImage, size: ratatui::layout::Size, id: u32) -> Option<Self> {
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
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(pixels).ok()?;
        let compressed = encoder.finish().ok()?;
        let transmit = kitty_transmit_compressed_format(&compressed, width, height, format, id);
        let [id_extra, id_r, id_g, id_b] = id.to_be_bytes();

        Some(Self {
            transmit: Mutex::new(Some(transmit)),
            id_color: format!("\x1b[38;2;{id_r};{id_g};{id_b}m"),
            id_extra: u16::from(id_extra),
            size,
        })
    }

    fn render(&self, f: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) {
        const UNIT_WIDTH: CellDiffOption =
            CellDiffOption::ForcedWidth(NonZeroU16::new(1).expect("one is non-zero"));

        let full_width = area.width.min(self.size.width);
        if full_width == 0 {
            return;
        }
        let width = usize::from(full_width);
        let mut transmit = self.take_transmission();
        let placeholder_row: String =
            std::iter::repeat_n(crate::widgets::KITTY_PLACEHOLDER, width - 1).collect();
        let right = area.width.saturating_sub(1);
        let down = area.height.saturating_sub(1);
        let restore_cursor = format!("\x1b[u\x1b[{right}C\x1b[{down}B");
        let height = area.height.min(self.size.height).min(297);
        let mut symbol = String::new();

        for y in 0..height {
            symbol.clear();
            if let Some(sequence) = transmit.take() {
                symbol.push_str(&sequence);
            }
            write!(
                symbol,
                "\x1b[s{}\u{10EEEE}{}{}{}",
                self.id_color,
                crate::widgets::kitty_diacritic(y),
                crate::widgets::kitty_diacritic(0),
                crate::widgets::kitty_diacritic(self.id_extra),
            )
            .expect("writing to a String cannot fail");
            symbol.push_str(&placeholder_row);
            symbol.push_str(&restore_cursor);

            for x in 1..full_width {
                if let Some(cell) = f.buffer_mut().cell_mut((area.x + x, area.y + y)) {
                    cell.set_diff_option(CellDiffOption::Skip);
                }
            }
            if let Some(cell) = f.buffer_mut().cell_mut((area.x, area.y + y)) {
                cell.set_symbol(&symbol).set_diff_option(UNIT_WIDTH);
            }
        }
    }

    fn transmission_pending(&self) -> bool {
        self.transmit
            .lock()
            .is_ok_and(|transmission| transmission.is_some())
    }

    fn take_transmission(&self) -> Option<String> {
        self.transmit
            .lock()
            .ok()
            .and_then(|mut sequence| sequence.take())
    }
}

#[cfg(feature = "terminal-images")]
fn kitty_transmit_compressed_format(
    payload: &[u8],
    width: u32,
    height: u32,
    format: u8,
    id: u32,
) -> String {
    kitty_transmit_compressed_format_for(
        payload,
        width,
        height,
        format,
        id,
        std::env::var_os("TMUX").is_some(),
    )
}

#[cfg(all(feature = "terminal-images", test))]
pub(crate) fn kitty_transmit_compressed_for(
    payload: &[u8],
    width: u32,
    height: u32,
    id: u32,
    is_tmux: bool,
) -> String {
    kitty_transmit_compressed_format_for(payload, width, height, 32, id, is_tmux)
}

#[cfg(feature = "terminal-images")]
fn kitty_transmit_compressed_format_for(
    payload: &[u8],
    width: u32,
    height: u32,
    format: u8,
    id: u32,
    is_tmux: bool,
) -> String {
    const CHUNK_BYTES: usize = 3072;

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
            write!(
                data,
                "i={id},a=T,U=1,f={format},o=z,t=d,s={width},v={height},"
            )
            .expect("writing to a String cannot fail");
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
    width: u16,
    height: u16,
    background_rgb: Option<(u8, u8, u8)>,
    fit: ImageFit,
    protocol: ImageProtocol,
    resolved_protocol: ImageProtocol,
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
        }
    }
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

    fn get_latest_compatible(
        &mut self,
        stream_key: u64,
        key: &RenderCacheKey,
    ) -> Option<Arc<EncodedProtocol>> {
        let idx = self.entries.iter().rposition(|entry| {
            entry.key != *key
                && stream_encoding_compatible(entry.stream_key, &entry.key, stream_key, key)
        })?;

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
    cached_stream == requested_stream
        && cached.width == requested.width
        && cached.height == requested.height
        && cached.background_rgb == requested.background_rgb
        && cached.fit == requested.fit
        && cached.protocol == requested.protocol
        && cached.resolved_protocol == requested.resolved_protocol
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
    fn cache_get(&self, key: &RenderCacheKey) -> Option<Arc<EncodedProtocol>> {
        let Ok(mut inner) = self.inner.lock() else {
            return None;
        };
        inner.cache.get(key)
    }

    fn cache_get_latest_compatible(
        &self,
        stream_key: u64,
        key: &RenderCacheKey,
    ) -> Option<Arc<EncodedProtocol>> {
        let Ok(mut inner) = self.inner.lock() else {
            return None;
        };
        inner.cache.get_latest_compatible(stream_key, key)
    }

    fn enqueue(&self, request: EncodeRequest) {
        const MAX_QUEUED_SOURCES: usize = 48;

        let Ok(mut inner) = self.inner.lock() else {
            return;
        };

        let stream_key = request.stream_key;

        if inner
            .in_flight_keys
            .get(&stream_key)
            .is_some_and(|key| *key == request.key)
        {
            return;
        }

        if inner
            .queued
            .get(&stream_key)
            .is_some_and(|existing| existing.key == request.key)
        {
            return;
        }

        let inserted_new = inner.queued.insert(stream_key, request).is_none();
        if !inserted_new {
            // The queue already contains this stream. Its map entry now holds the newest frame,
            // while its one position in `queue` is intentionally retained.
            return;
        }

        while inner.queue.len() >= MAX_QUEUED_SOURCES {
            let Some(evicted_stream) = inner.queue.pop_front() else {
                break;
            };
            inner.queued.remove(&evicted_stream);
        }

        inner.queue.push_back(stream_key);
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
                let Some(stream_key) = inner.queue.pop_front() else {
                    break;
                };
                if inner.in_flight.contains(&stream_key) {
                    inner.queue.push_back(stream_key);
                    continue;
                }
                let Some(request) = inner.queued.remove(&stream_key) else {
                    continue;
                };

                inner.in_flight.insert(stream_key);
                inner.in_flight_keys.insert(stream_key, request.key);
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

        inner.in_flight.remove(&request.stream_key);
        inner.in_flight_keys.remove(&request.stream_key);
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
        background_rgb: effective_background_rgb,
        fit: node.fit,
        protocol: node.protocol,
        resolved_protocol: resolved,
    };

    Some(EncodeRequest::new(
        node.source_hash,
        key,
        decoded,
        CacheRetention::Variants,
    ))
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
    #[cfg(feature = "terminal-images")]
    if matches!(request.key.resolved_protocol, ImageProtocol::Kitty) {
        use std::hash::{Hash as _, Hasher as _};

        let encoded_size = resize.size_for(request.image.as_ref(), picker.font_size(), size);
        let pixel_width = u32::from(encoded_size.width) * u32::from(picker.font_size().width);
        let pixel_height = u32::from(encoded_size.height) * u32::from(picker.font_size().height);
        let background = request
            .key
            .background_rgb
            .map(|(r, g, b)| image::Rgba([r, g, b, 255]));
        let resized = (request.image.width() != pixel_width
            || request.image.height() != pixel_height)
            .then(|| {
                resize.resize(
                    request.image.as_ref(),
                    picker.font_size(),
                    encoded_size,
                    background,
                )
            });
        let image = resized.as_ref().unwrap_or(request.image.as_ref());
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        request.key.hash(&mut hasher);
        let id = (hasher.finish() as u32).max(1);
        return CompressedKitty::new(image, encoded_size, id).map(EncodedProtocol::CompressedKitty);
    }

    if matches!(request.key.fit, ImageFit::Scale) {
        let encoded_size = resize.size_for(request.image.as_ref(), picker.font_size(), size);
        let background = request
            .key
            .background_rgb
            .map(|(r, g, b)| image::Rgba([r, g, b, 255]));
        let resized = resize.resize(
            request.image.as_ref(),
            picker.font_size(),
            encoded_size,
            background,
        );
        picker
            .new_protocol(resized, encoded_size, Resize::Fit(None))
            .map(|protocol| EncodedProtocol::ratatui(protocol, request.key.resolved_protocol))
            .ok()
    } else {
        picker
            .new_protocol((*request.image).clone(), size, resize)
            .map(|protocol| EncodedProtocol::ratatui(protocol, request.key.resolved_protocol))
            .ok()
    }
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

    let stale = encoder.cache_get_latest_compatible(request.stream_key, &request.key);

    encoder.enqueue(request);
    if let Some(protocol) = stale {
        ProtocolResolve::Stale(protocol)
    } else {
        ProtocolResolve::Pending
    }
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
    pixels: impl FnOnce() -> Arc<image::DynamicImage>,
) -> bool {
    if area.width == 0 || area.height == 0 || image_support::image_rendering_suspended() {
        return false;
    }

    let key = RenderCacheKey {
        source_hash,
        frame_index: 0,
        width: area.width,
        height: area.height,
        background_rgb: None,
        fit: ImageFit::Scale,
        protocol: ImageProtocol::Auto,
        resolved_protocol: protocol_type_to_public(
            image_support::picker_snapshot().protocol_type(),
        ),
    };

    let encoder = async_encoder();
    if let Some(protocol) = encoder.cache_get(&key) {
        protocol.render(f, area);
        return true;
    }

    let protocol = {
        let stale = encoder.cache_get_latest_compatible(stream_key, &key);
        encoder.enqueue(EncodeRequest::new(
            stream_key,
            key,
            pixels(),
            CacheRetention::LatestOnly,
        ));
        stale
    };

    let Some(protocol) = protocol else {
        return false;
    };
    protocol.render(f, area);
    true
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

    fn key(source_hash: u64) -> RenderCacheKey {
        RenderCacheKey {
            source_hash,
            frame_index: 0,
            width: 80,
            height: 24,
            background_rgb: None,
            fit: ImageFit::Scale,
            protocol: ImageProtocol::Auto,
            resolved_protocol: ImageProtocol::Halfblocks,
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

        assert!(bootstrap.transmission_pending());
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
            Protocol::Kitty(Kitty::new(image, size, 8, false).unwrap()),
            ImageProtocol::Kitty,
        );
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

    #[cfg(feature = "terminal-images")]
    #[test]
    fn compressed_kitty_releases_transmission_after_render() {
        let image = image::DynamicImage::new_rgba8(400, 200);
        let protocol = CompressedKitty::new(&image, ratatui::layout::Size::new(40, 10), 7).unwrap();
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
        let next = EncodedProtocol::CompressedKitty(CompressedKitty::new(&image, size, 8).unwrap());
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
