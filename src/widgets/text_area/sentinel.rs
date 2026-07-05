use std::any::Any;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::clipboard::ImageContent;
use crate::core::event::MouseEvent;

static NEXT_SENTINEL_ID: AtomicU64 = AtomicU64::new(1);

/// Opaque stable identifier for a sentinel, unique within a TextArea lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SentinelId(u64);

impl SentinelId {
    /// Reserved when no stable id was assigned (e.g. legacy sentinel).
    pub const UNKNOWN: Self = Self(0);

    pub(crate) fn next() -> Self {
        Self(NEXT_SENTINEL_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Base codepoint for custom (non-image) inline sentinels.
///
/// Each custom sentinel at index `i` in the `sentinels` list is represented by
/// `char::from_u32(SENTINEL_BASE as u32 + i as u32).unwrap()` in the value string.
/// Uses `U+F000` to avoid collision with image sentinels at `U+E000`.
pub const SENTINEL_BASE: char = '\u{F000}';
/// A user-defined inline sentinel token embedded in [`TextArea`](crate::widgets::TextArea) text.
///
/// Each entry maps to a Private Use Area character in the value string and is
/// rendered as a styled label. Sentinels behave atomically: a single backspace
/// or delete removes the whole token.
#[derive(Clone)]
pub struct TextAreaSentinel {
    /// The label rendered in place of the sentinel character.
    pub label: std::sync::Arc<str>,
    /// Style applied when the textarea is unfocused.
    pub style: crate::style::Style,
    /// Style applied when the textarea is focused. Falls back to `style` when `None`.
    pub focus_style: Option<crate::style::Style>,
    /// Style patched over the rendered label while the pointer hovers this sentinel.
    pub hover_style: Option<crate::style::Style>,
    payload: Option<Arc<dyn Any + Send + Sync>>,
    id: Option<SentinelId>,
}

impl PartialEq for TextAreaSentinel {
    fn eq(&self, other: &Self) -> bool {
        self.label == other.label
            && self.style == other.style
            && self.focus_style == other.focus_style
            && self.hover_style == other.hover_style
            && self.id == other.id
    }
}

impl Eq for TextAreaSentinel {}

impl Hash for TextAreaSentinel {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.label.hash(state);
        self.style.hash(state);
        self.focus_style.hash(state);
        self.hover_style.hash(state);
        self.id.hash(state);
    }
}

impl fmt::Debug for TextAreaSentinel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextAreaSentinel")
            .field("label", &self.label)
            .field("style", &self.style)
            .field("focus_style", &self.focus_style)
            .field("hover_style", &self.hover_style)
            .field("id", &self.id)
            .field(
                "payload_type_id",
                &self.payload.as_ref().map(|p| Any::type_id(p.as_ref())),
            )
            .finish()
    }
}

impl TextAreaSentinel {
    /// Create a new sentinel with a label and default (empty) styles.
    pub fn new(label: impl Into<std::sync::Arc<str>>) -> Self {
        Self {
            label: label.into(),
            style: crate::style::Style::default(),
            focus_style: None,
            hover_style: None,
            payload: None,
            id: None,
        }
    }

    /// Attach type-erased user data (see [`Self::get_payload`]).
    pub fn payload<T: Send + Sync + 'static>(mut self, data: T) -> Self {
        self.payload = Some(Arc::new(data));
        self
    }

    /// Downcast the payload to `T`.
    pub fn get_payload<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.payload.as_ref()?.downcast_ref::<T>()
    }

    /// Borrow the payload as [`Any`] for custom downcasting.
    pub fn raw_payload(&self) -> Option<&Arc<dyn Any + Send + Sync>> {
        self.payload.as_ref()
    }

    /// Set a stable id (otherwise [`insert_sentinel`] assigns one).
    pub fn id(mut self, id: SentinelId) -> Self {
        self.id = Some(id);
        self
    }

    /// Stable id when set.
    pub fn sentinel_id(&self) -> Option<SentinelId> {
        self.id
    }

    /// Set the style.
    pub fn style(mut self, style: crate::style::Style) -> Self {
        self.style = style;
        self
    }

    /// Set the focused style.
    pub fn focus_style(mut self, style: crate::style::Style) -> Self {
        self.focus_style = Some(style);
        self
    }

    /// Set the hover style patched over the rendered label while the pointer is over it.
    pub fn hover_style(mut self, style: crate::style::Style) -> Self {
        self.hover_style = Some(style);
        self
    }
}

/// Lifecycle event emitted when sentinels change.
#[derive(Clone, Debug)]
pub enum SentinelEvent {
    /// A sentinel was deleted by user edit. Payload preserved for cleanup.
    Deleted {
        /// Stable id ([`SentinelId::UNKNOWN`] if none was assigned).
        id: SentinelId,
        /// Full sentinel including payload.
        sentinel: TextAreaSentinel,
    },
}

/// A clicked inline sentinel inside a [`TextArea`](crate::widgets::TextArea).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextAreaSentinelClickKind {
    /// An inline image sentinel (`IMAGE_SENTINEL_BASE + index`) was clicked.
    Image {
        /// Image index in the `TextArea::images` list.
        index: usize,
        /// Image payload associated with the clicked sentinel.
        image: ImageContent,
    },
    /// A custom inline sentinel (`SENTINEL_BASE + index`) was clicked.
    Custom {
        /// Sentinel index in the `TextArea::sentinels` list.
        index: usize,
        /// Stable id (`SentinelId::UNKNOWN` when unset).
        id: SentinelId,
        /// Full sentinel metadata, including payload.
        sentinel: TextAreaSentinel,
    },
}

/// Event emitted when the user clicks an inline sentinel placeholder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextAreaSentinelClickEvent {
    /// The clicked sentinel payload.
    pub kind: TextAreaSentinelClickKind,
    /// Byte range of the sentinel character in the text value.
    pub byte_range: (usize, usize),
    /// Mouse event that activated the sentinel.
    pub mouse: MouseEvent,
}

/// Insert a custom sentinel into `value` at `cursor`, appending it to `sentinels`.
///
/// Returns `(new_value, new_cursor)`. The caller is responsible for updating both
/// the value and the sentinels list on the TextArea widget.
pub fn insert_sentinel(
    value: &str,
    cursor: usize,
    sentinels: &mut Vec<TextAreaSentinel>,
    mut sentinel: TextAreaSentinel,
) -> (String, usize) {
    if sentinel.id.is_none() {
        sentinel.id = Some(SentinelId::next());
    }
    let idx = sentinels.len();
    sentinels.push(sentinel);
    let ch = char::from_u32(SENTINEL_BASE as u32 + idx as u32).unwrap_or(SENTINEL_BASE);
    let mut new_value = String::with_capacity(value.len() + ch.len_utf8());
    let cursor = crate::utils::text::clamp_cursor(value, cursor);
    new_value.push_str(&value[..cursor]);
    new_value.push(ch);
    new_value.push_str(&value[cursor..]);
    let new_cursor = cursor + ch.len_utf8();
    (new_value, new_cursor)
}

/// First Unicode Private Use Area character used as an inline image sentinel.
/// Each image at index `i` in the `images` list is represented by
/// `char::from_u32(IMAGE_SENTINEL_BASE as u32 + i as u32).unwrap()` in the value string.
pub const IMAGE_SENTINEL_BASE: char = '\u{E000}';

/// Whether images are embedded inline in the text value (sentinel chars) or
/// displayed as attachment chips above the text input area.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAreaImageMode {
    /// Images are embedded inline as sentinel characters (U+E000…).
    /// The renderer draws the `image_placeholder` label in their place.
    #[default]
    Inline,
    /// Images are shown as chip labels above the text content area.
    /// Pasting an image appends to the `images` list without touching the value.
    Attachment,
}
