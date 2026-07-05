//! Image widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_image;
pub use node::ImageNode;
pub use reconcile::reconcile_image;

use std::sync::Arc;

use crate::core::element::{Element, ElementKind};
use crate::style::{Length, Style};

/// Source data for an [`Image`] widget.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ImageSource {
    /// File path to an image on disk.
    Path(Arc<str>),
    /// In-memory encoded image bytes (for example PNG/JPEG/WebP data).
    Bytes(Arc<[u8]>),
}

/// Resize behavior when fitting an image into widget bounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImageFit {
    /// Keep aspect ratio and fit inside available area.
    #[default]
    Contain,
    /// Crop image to fill available area.
    Crop,
    /// Keep aspect ratio and scale both up and down to fit.
    Scale,
}

/// Requested terminal image protocol.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImageProtocol {
    /// Auto-detect supported protocol, fallback to block rendering.
    #[default]
    Auto,
    /// Force Kitty graphics protocol.
    Kitty,
    /// Force iTerm2 inline-image protocol.
    Iterm2,
    /// Force Sixel protocol.
    Sixel,
    /// Force unicode half-block rendering.
    Halfblocks,
}

/// Playback state for animated images.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImagePlayback {
    /// Frames advance according to animation timing.
    #[default]
    Playing,
    /// Keep displaying the current frame.
    Paused,
}

/// Loop mode for animated images.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImageRepeat {
    /// Restart from the first frame after the last frame.
    #[default]
    Loop,
    /// Stop on the last frame.
    Once,
}

/// A terminal image widget with protocol-aware rendering.
#[derive(Clone)]
pub struct Image {
    /// Source of image data.
    pub source: ImageSource,
    /// Base style used for textual fallback rendering.
    pub style: Style,
    /// Requested width.
    /// Default: `Length::Auto`.
    pub width: Length,
    /// Requested height.
    /// Default: `Length::Auto`.
    pub height: Length,
    /// Resize behavior.
    pub fit: ImageFit,
    /// Preferred image protocol.
    pub protocol: ImageProtocol,
    /// Optional fallback text shown on decode/render failure.
    pub alt: Option<Arc<str>>,
    /// Playback state for animated formats.
    pub playback: ImagePlayback,
    /// Loop behavior for animated formats.
    pub repeat: ImageRepeat,
    /// Playback speed in percent (100 = normal speed).
    pub speed_percent: u16,
}

impl Image {
    /// Create an image from file path.
    pub fn new(src: impl Into<Arc<str>>) -> Self {
        Self {
            source: ImageSource::Path(src.into()),
            style: Style::default(),
            width: Length::Auto,
            height: Length::Auto,
            fit: ImageFit::default(),
            protocol: ImageProtocol::default(),
            alt: None,
            playback: ImagePlayback::default(),
            repeat: ImageRepeat::default(),
            speed_percent: 100,
        }
    }

    /// Create an image from encoded in-memory bytes.
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            source: ImageSource::Bytes(bytes.into()),
            style: Style::default(),
            width: Length::Auto,
            height: Length::Auto,
            fit: ImageFit::default(),
            protocol: ImageProtocol::default(),
            alt: None,
            playback: ImagePlayback::default(),
            repeat: ImageRepeat::default(),
            speed_percent: 100,
        }
    }

    /// Replace image source with file path.
    pub fn src(mut self, src: impl Into<Arc<str>>) -> Self {
        self.source = ImageSource::Path(src.into());
        self
    }

    /// Replace image source with encoded bytes.
    pub fn bytes(mut self, bytes: impl Into<Arc<[u8]>>) -> Self {
        self.source = ImageSource::Bytes(bytes.into());
        self
    }

    /// Set base style used by fallback rendering.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set requested width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set requested height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Set resize behavior.
    pub fn fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    /// Set preferred image protocol.
    pub fn protocol(mut self, protocol: ImageProtocol) -> Self {
        self.protocol = protocol;
        self
    }

    /// Set fallback text for decode/render failures.
    pub fn alt(mut self, alt: impl Into<Arc<str>>) -> Self {
        self.alt = Some(alt.into());
        self
    }

    /// Set playback mode for animated formats.
    pub fn playback(mut self, playback: ImagePlayback) -> Self {
        self.playback = playback;
        self
    }

    /// Convenience toggle for play/pause.
    pub fn paused(mut self, paused: bool) -> Self {
        self.playback = if paused {
            ImagePlayback::Paused
        } else {
            ImagePlayback::Playing
        };
        self
    }

    /// Set loop mode for animated formats.
    pub fn repeat(mut self, repeat: ImageRepeat) -> Self {
        self.repeat = repeat;
        self
    }

    /// Convenience toggle for loop mode.
    pub fn looping(mut self, looping: bool) -> Self {
        self.repeat = if looping {
            ImageRepeat::Loop
        } else {
            ImageRepeat::Once
        };
        self
    }

    /// Set playback speed in percent (`100` = normal speed).
    pub fn speed_percent(mut self, speed_percent: u16) -> Self {
        self.speed_percent = speed_percent.max(1);
        self
    }
}

impl From<Image> for Element {
    fn from(value: Image) -> Self {
        Element::new(ElementKind::Image(value))
    }
}

impl crate::layout::hash::LayoutHash for Image {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&crate::core::element::Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.fit.hash(hasher);
        self.protocol.hash(hasher);
        self.playback.hash(hasher);
        self.repeat.hash(hasher);
        self.speed_percent.hash(hasher);
        self.source.hash(hasher);
        self.alt.hash(hasher);
        Some(())
    }
}
