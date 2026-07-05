use std::collections::HashSet;
use std::sync::Arc;

use crate::clipboard::ImageContent;
use crate::utils::text::SentinelInfo;

use super::{
    IMAGE_SENTINEL_BASE, SENTINEL_BASE, SentinelEvent, SentinelId, TextArea, TextAreaImageMode,
    TextAreaSentinel,
};

/// In-memory snapshot of text value, cursor, sentinels, and inline images.
#[derive(Clone, Debug)]
pub struct TextAreaSnapshot {
    /// Current buffer text.
    pub value: Arc<str>,
    /// Caret byte index.
    pub cursor: usize,
    /// Selection anchor, if any.
    pub anchor: Option<usize>,
    /// Custom sentinel metadata (indices follow value string).
    pub sentinels: Vec<TextAreaSentinel>,
    /// Inline / attachment images list.
    pub images: Vec<ImageContent>,
    /// How [`Self::images`] are interpreted.
    pub image_mode: TextAreaImageMode,
}

pub struct TextAreaClipboardTransformEvent<'a> {
    /// Selection text after TextArea's default sentinel-to-label rendering.
    pub text: &'a str,
    /// Raw selected editor text before sentinel-to-label rendering.
    pub raw_text: &'a str,
}

pub type TextAreaClipboardTransform =
    Arc<dyn for<'a> Fn(TextAreaClipboardTransformEvent<'a>) -> String>;

impl TextAreaSnapshot {
    /// Copy editable state from a built [`TextArea`].
    pub fn capture(ta: &TextArea) -> Self {
        Self {
            value: ta.value.clone(),
            cursor: ta.cursor,
            anchor: ta.anchor,
            sentinels: ta.sentinels.clone(),
            images: ta.images.clone(),
            image_mode: ta.image_mode,
        }
    }

    /// Apply this snapshot onto a [`TextArea`] builder (other props unchanged).
    pub fn apply(self, ta: TextArea) -> TextArea {
        ta.value(self.value)
            .cursor(self.cursor)
            .anchor(self.anchor)
            .sentinels(self.sentinels)
            .images(self.images)
            .image_mode(self.image_mode)
    }

    /// [`SentinelEvent::Deleted`] for each stable id present in `self` but not in `other`.
    pub fn diff(&self, other: &Self) -> Vec<SentinelEvent> {
        let other_ids: HashSet<SentinelId> = other
            .sentinels
            .iter()
            .filter_map(TextAreaSentinel::sentinel_id)
            .collect();
        let mut events = Vec::new();
        for s in &self.sentinels {
            if let Some(id) = s.sentinel_id()
                && !other_ids.contains(&id)
            {
                events.push(SentinelEvent::Deleted {
                    id,
                    sentinel: s.clone(),
                });
            }
        }
        events
    }
}

/// Build a [`SentinelInfo`] for sentinel-aware width calculations.
///
/// Combines the image sentinel range (when in Inline mode with images) and the
/// custom sentinel range (when sentinels are non-empty) into one struct.
/// Returns `None` when neither range is active.
pub(crate) fn sentinel_info_for(
    image_mode: TextAreaImageMode,
    images_count: usize,
    image_placeholder: &str,
    sentinels: &[TextAreaSentinel],
) -> Option<SentinelInfo> {
    let image = if image_mode == TextAreaImageMode::Inline && images_count > 0 {
        let base = IMAGE_SENTINEL_BASE as u32;
        let end = base + images_count as u32;
        let ph_width = if image_placeholder.contains('X') {
            let label = image_placeholder.replace('X', images_count.to_string().as_str());
            unicode_width::UnicodeWidthStr::width(label.as_str())
        } else {
            unicode_width::UnicodeWidthStr::width(image_placeholder)
        };
        Some((base, end, ph_width))
    } else {
        None
    };

    let custom = if !sentinels.is_empty() {
        let base = SENTINEL_BASE as u32;
        let end = base + sentinels.len() as u32;
        let widths: Vec<usize> = sentinels
            .iter()
            .map(|s| unicode_width::UnicodeWidthStr::width(s.label.as_ref()))
            .collect();
        let labels: Vec<std::sync::Arc<str>> = sentinels.iter().map(|s| s.label.clone()).collect();
        Some((base, end, widths, labels))
    } else {
        None
    };

    if image.is_none() && custom.is_none() {
        None
    } else {
        Some(SentinelInfo { image, custom })
    }
}
