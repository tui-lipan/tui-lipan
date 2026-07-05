use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use image::AnimationDecoder;

use crate::core::node::WidgetNode;
use crate::style::{Length, Style};

use super::{Image, ImageFit, ImagePlayback, ImageProtocol, ImageRepeat, ImageSource};

pub(crate) fn source_hash(source: &ImageSource) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

const MIN_ANIMATION_DELAY_MS: u32 = 16;

fn delay_to_millis(delay: image::Delay) -> u32 {
    let millis = Duration::from(delay).as_millis();
    let millis = millis.min(u32::MAX as u128) as u32;
    millis.max(MIN_ANIMATION_DELAY_MS)
}

fn gif_worker_queue_capacity() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("TUI_LIPAN_IMAGE_GIF_WORKER_QUEUE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(2)
            .clamp(1, 4)
    })
}

fn decode_gif_frames(bytes: Arc<[u8]>) -> Result<image::Frames<'static>, Arc<str>> {
    image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
        .map_err(|err| Arc::<str>::from(format!("gif decode failed: {err}")))
        .map(|decoder| decoder.into_frames())
}

#[derive(Clone)]
struct DecodedFrame {
    image: Arc<image::DynamicImage>,
    delay_ms: u32,
}

fn next_decoded_gif_frame(
    frames: &mut image::Frames<'static>,
) -> Result<Option<DecodedFrame>, Arc<str>> {
    match frames.next() {
        Some(Ok(frame)) => {
            let delay_ms = delay_to_millis(frame.delay());
            Ok(Some(DecodedFrame {
                image: Arc::new(image::DynamicImage::ImageRgba8(frame.into_buffer())),
                delay_ms,
            }))
        }
        Some(Err(err)) => Err(Arc::<str>::from(format!("gif frame decode failed: {err}"))),
        None => Ok(None),
    }
}

#[derive(Clone)]
pub struct BufferedAnimation {
    pub frames: Arc<[Arc<image::DynamicImage>]>,
    pub delays_ms: Arc<[u32]>,
    pub frame_index: usize,
    pub elapsed_ms: u32,
    speed_remainder: u64,
    ended_once: bool,
}

impl BufferedAnimation {
    pub fn from_frames(frames: Vec<image::Frame>) -> Option<Self> {
        if frames.len() < 2 {
            return None;
        }

        let mut images = Vec::with_capacity(frames.len());
        let mut delays = Vec::with_capacity(frames.len());
        for frame in frames {
            let delay_ms = delay_to_millis(frame.delay());
            images.push(Arc::new(image::DynamicImage::ImageRgba8(
                frame.into_buffer(),
            )));
            delays.push(delay_ms);
        }

        Some(Self {
            frames: images.into(),
            delays_ms: delays.into(),
            frame_index: 0,
            elapsed_ms: 0,
            speed_remainder: 0,
            ended_once: false,
        })
    }

    fn current_image(&self) -> Option<Arc<image::DynamicImage>> {
        self.frames.get(self.frame_index).cloned()
    }

    fn current_delay_ms(&self) -> Option<u32> {
        self.delays_ms
            .get(self.frame_index)
            .copied()
            .map(|delay| delay.max(MIN_ANIMATION_DELAY_MS))
    }

    fn advance(&mut self, delta_ms: u32, speed_percent: u16, repeat: ImageRepeat) -> bool {
        if self.frames.is_empty() || speed_percent == 0 {
            return false;
        }

        if self.ended_once {
            if matches!(repeat, ImageRepeat::Loop) {
                self.ended_once = false;
                self.frame_index = 0;
            } else {
                return false;
            }
        }

        let scaled = delta_ms as u64 * speed_percent as u64 + self.speed_remainder;
        let scaled_ms = (scaled / 100) as u32;
        self.speed_remainder = scaled % 100;

        if scaled_ms == 0 {
            return false;
        }

        self.elapsed_ms = self.elapsed_ms.saturating_add(scaled_ms);
        let mut advanced = false;

        while let Some(delay) = self.current_delay_ms() {
            if self.elapsed_ms < delay {
                break;
            }

            self.elapsed_ms = self.elapsed_ms.saturating_sub(delay);
            if self.frame_index + 1 < self.frames.len() {
                self.frame_index += 1;
                self.ended_once = false;
                advanced = true;
                continue;
            }

            if matches!(repeat, ImageRepeat::Loop) {
                self.frame_index = 0;
                self.ended_once = false;
                advanced = true;
                continue;
            }

            self.frame_index = self.frames.len().saturating_sub(1);
            self.elapsed_ms = 0;
            self.speed_remainder = 0;
            self.ended_once = true;
            break;
        }

        advanced
    }

    fn millis_until_next_frame(&self, speed_percent: u16, repeat: ImageRepeat) -> Option<u32> {
        if self.frames.is_empty() || speed_percent == 0 {
            return None;
        }

        if self.ended_once {
            return if matches!(repeat, ImageRepeat::Loop) {
                Some(1)
            } else {
                None
            };
        }

        let delay = self.current_delay_ms()?;
        let remaining_scaled = delay.saturating_sub(self.elapsed_ms);
        Some(real_millis_until(
            remaining_scaled,
            speed_percent,
            self.speed_remainder,
        ))
    }
}

#[derive(Clone)]
pub struct GifStreamAnimation {
    state: Rc<RefCell<GifStreamState>>,
}

#[derive(Clone)]
struct WorkerFrame {
    frame: DecodedFrame,
    frame_index: usize,
    loop_index: u64,
}

struct GifStreamState {
    current: DecodedFrame,
    frame_index: usize,
    loop_index: u64,
    elapsed_ms: u32,
    speed_remainder: u64,
    has_multiple_frames: bool,
    ended_once: bool,
    frame_rx: Receiver<WorkerFrame>,
}

impl GifStreamAnimation {
    fn spawn_worker(
        bytes: Arc<[u8]>,
        frame_tx: mpsc::SyncSender<WorkerFrame>,
        initial_skip: usize,
    ) -> Result<(), Arc<str>> {
        thread::Builder::new()
            .name("image-gif-worker".to_string())
            .spawn(move || {
                let mut loop_index: u64 = 0;
                let mut skip = initial_skip;

                loop {
                    let mut frames = match decode_gif_frames(Arc::clone(&bytes)) {
                        Ok(frames) => frames,
                        Err(_) => return,
                    };

                    let mut frame_index: usize = 0;
                    loop {
                        let frame = match next_decoded_gif_frame(&mut frames) {
                            Ok(Some(frame)) => frame,
                            Ok(None) => break,
                            Err(_) => return,
                        };

                        if skip > 0 {
                            skip -= 1;
                            frame_index = frame_index.saturating_add(1);
                            continue;
                        }

                        let packet = WorkerFrame {
                            frame,
                            frame_index,
                            loop_index,
                        };

                        if frame_tx.send(packet).is_err() {
                            return;
                        }
                        frame_index = frame_index.saturating_add(1);
                    }

                    if frame_index == 0 {
                        return;
                    }

                    loop_index = loop_index.saturating_add(1);
                }
            })
            .map(|_| ())
            .map_err(|err| Arc::<str>::from(format!("gif worker spawn failed: {err}")))
    }

    pub fn new(bytes: Arc<[u8]>) -> Result<Option<Self>, Arc<str>> {
        let mut frames = decode_gif_frames(Arc::clone(&bytes))?;
        let Some(first) = next_decoded_gif_frame(&mut frames)? else {
            return Ok(None);
        };

        let prefetched_next = next_decoded_gif_frame(&mut frames)?;
        let has_multiple_frames = prefetched_next.is_some();
        if !has_multiple_frames {
            return Ok(None);
        }

        let (frame_tx, frame_rx) = mpsc::sync_channel::<WorkerFrame>(gif_worker_queue_capacity());

        if let Some(frame) = prefetched_next {
            let _ = frame_tx.send(WorkerFrame {
                frame,
                frame_index: 1,
                loop_index: 0,
            });
        }

        Self::spawn_worker(bytes, frame_tx, 2)?;

        Ok(Some(Self {
            state: Rc::new(RefCell::new(GifStreamState {
                current: first,
                frame_index: 0,
                loop_index: 0,
                elapsed_ms: 0,
                speed_remainder: 0,
                has_multiple_frames,
                ended_once: false,
                frame_rx,
            })),
        }))
    }

    fn current_image(&self) -> Option<Arc<image::DynamicImage>> {
        Some(Arc::clone(&self.state.borrow().current.image))
    }

    fn current_frame_index(&self) -> usize {
        self.state.borrow().frame_index
    }

    fn is_animated(&self) -> bool {
        self.state.borrow().has_multiple_frames
    }

    fn advance(&mut self, delta_ms: u32, speed_percent: u16, repeat: ImageRepeat) -> bool {
        let mut state = self.state.borrow_mut();

        if speed_percent == 0 {
            return false;
        }

        if state.ended_once {
            if matches!(repeat, ImageRepeat::Loop) {
                state.ended_once = false;
            } else {
                return false;
            }
        }

        let scaled = delta_ms as u64 * speed_percent as u64 + state.speed_remainder;
        let scaled_ms = (scaled / 100) as u32;
        state.speed_remainder = scaled % 100;

        if scaled_ms == 0 {
            return false;
        }

        state.elapsed_ms = state.elapsed_ms.saturating_add(scaled_ms);
        let mut advanced = false;

        loop {
            let delay = state.current.delay_ms.max(MIN_ANIMATION_DELAY_MS);
            if state.elapsed_ms < delay {
                break;
            }

            let packet = match state.frame_rx.try_recv() {
                Ok(packet) => packet,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    state.ended_once = true;
                    break;
                }
            };

            if packet.loop_index > state.loop_index
                && packet.frame_index == 0
                && matches!(repeat, ImageRepeat::Once)
            {
                state.ended_once = true;
                state.elapsed_ms = 0;
                state.speed_remainder = 0;
                break;
            }

            state.elapsed_ms = state.elapsed_ms.saturating_sub(delay);
            state.current = packet.frame;
            state.frame_index = packet.frame_index;
            state.loop_index = packet.loop_index;
            state.ended_once = false;
            advanced = true;
        }

        advanced
    }

    fn millis_until_next_frame(&self, speed_percent: u16, repeat: ImageRepeat) -> Option<u32> {
        let state = self.state.borrow();
        if speed_percent == 0 {
            return None;
        }

        if state.ended_once {
            return if matches!(repeat, ImageRepeat::Loop) {
                Some(1)
            } else {
                None
            };
        }

        let remaining_scaled = state
            .current
            .delay_ms
            .max(MIN_ANIMATION_DELAY_MS)
            .saturating_sub(state.elapsed_ms);
        Some(real_millis_until(
            remaining_scaled,
            speed_percent,
            state.speed_remainder,
        ))
    }
}

#[derive(Clone)]
pub enum ImageAnimation {
    Buffered(BufferedAnimation),
    GifStream(GifStreamAnimation),
}

impl ImageAnimation {
    pub fn from_frames(frames: Vec<image::Frame>) -> Option<Self> {
        BufferedAnimation::from_frames(frames).map(Self::Buffered)
    }

    pub fn from_gif_stream(bytes: Arc<[u8]>) -> Result<Option<Self>, Arc<str>> {
        GifStreamAnimation::new(bytes).map(|value| value.map(Self::GifStream))
    }

    pub fn current_image(&self) -> Option<Arc<image::DynamicImage>> {
        match self {
            Self::Buffered(animation) => animation.current_image(),
            Self::GifStream(animation) => animation.current_image(),
        }
    }

    pub fn current_frame_index(&self) -> usize {
        match self {
            Self::Buffered(animation) => animation.frame_index,
            Self::GifStream(animation) => animation.current_frame_index(),
        }
    }

    pub fn is_animated(&self) -> bool {
        match self {
            Self::Buffered(animation) => animation.frames.len() > 1,
            Self::GifStream(animation) => animation.is_animated(),
        }
    }

    pub fn advance(&mut self, delta_ms: u32, speed_percent: u16, repeat: ImageRepeat) -> bool {
        match self {
            Self::Buffered(animation) => animation.advance(delta_ms, speed_percent, repeat),
            Self::GifStream(animation) => animation.advance(delta_ms, speed_percent, repeat),
        }
    }

    pub fn millis_until_next_frame(&self, speed_percent: u16, repeat: ImageRepeat) -> Option<u32> {
        match self {
            Self::Buffered(animation) => animation.millis_until_next_frame(speed_percent, repeat),
            Self::GifStream(animation) => animation.millis_until_next_frame(speed_percent, repeat),
        }
    }
}

fn real_millis_until(remaining_scaled_ms: u32, speed_percent: u16, speed_remainder: u64) -> u32 {
    let speed_percent = speed_percent.max(1) as u64;
    let remaining_scaled = remaining_scaled_ms.max(1) as u64;
    let required = remaining_scaled.saturating_mul(100);
    let adjusted_required = required.saturating_sub(speed_remainder.min(required));

    adjusted_required.div_ceil(speed_percent).max(1) as u32
}

/// Internal runtime node for [`super::Image`].
#[derive(Clone)]
pub struct ImageNode {
    pub source: ImageSource,
    pub style: Style,
    pub width: Length,
    pub height: Length,
    pub fit: ImageFit,
    pub protocol: ImageProtocol,
    pub alt: Option<Arc<str>>,
    pub playback: ImagePlayback,
    pub repeat: ImageRepeat,
    pub speed_percent: u16,
    pub source_hash: u64,
    pub decoded: Option<Arc<image::DynamicImage>>,
    pub animation: Option<ImageAnimation>,
    pub decode_error: Option<Arc<str>>,
}

impl ImageNode {
    pub fn current_image(&self) -> Option<Arc<image::DynamicImage>> {
        self.animation
            .as_ref()
            .and_then(ImageAnimation::current_image)
            .or_else(|| self.decoded.clone())
    }

    pub fn current_frame_index(&self) -> usize {
        self.animation
            .as_ref()
            .map(ImageAnimation::current_frame_index)
            .unwrap_or(0)
    }

    pub fn is_animated(&self) -> bool {
        self.animation
            .as_ref()
            .is_some_and(ImageAnimation::is_animated)
    }

    pub fn next_frame_due_in_ms(&self) -> Option<u32> {
        if !matches!(self.playback, ImagePlayback::Playing) || self.speed_percent == 0 {
            return None;
        }

        self.animation.as_ref().and_then(|animation| {
            animation.millis_until_next_frame(self.speed_percent, self.repeat)
        })
    }

    pub fn tick_animation(&mut self, delta_ms: u32) -> bool {
        if !matches!(self.playback, ImagePlayback::Playing) || self.speed_percent == 0 {
            return false;
        }

        self.animation
            .as_mut()
            .is_some_and(|animation| animation.advance(delta_ms, self.speed_percent, self.repeat))
    }
}

impl WidgetNode for ImageNode {}

impl From<Image> for ImageNode {
    fn from(value: Image) -> Self {
        Self {
            source_hash: source_hash(&value.source),
            source: value.source,
            style: value.style,
            width: value.width,
            height: value.height,
            fit: value.fit,
            protocol: value.protocol,
            alt: value.alt,
            playback: value.playback,
            repeat: value.repeat,
            speed_percent: value.speed_percent,
            decoded: None,
            animation: None,
            decode_error: None,
        }
    }
}
