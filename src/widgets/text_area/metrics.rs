use crate::core::component::ScrollbarVisibility;
use crate::style::{Rect, Span};
use crate::text::line_index::TextPosition;

use super::virtual_text;

/// Line-number display mode for the built-in `TextArea` gutter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAreaLineNumberMode {
    /// Show absolute one-based logical line numbers.
    #[default]
    Absolute,
    /// Show Vim-style relative numbers: the cursor line is absolute and other
    /// lines show their distance from the cursor line.
    Relative,
}

/// Previous-frame resolved layout and cursor metrics for a keyed [`TextArea`](crate::widgets::TextArea).
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextAreaMetrics {
    pub rect: Rect,
    pub inner_rect: Rect,
    pub content_rect: Rect,
    pub gutter_width: u16,
    pub scroll_offset: usize,
    pub h_scroll_offset: usize,
    pub visible_logical_lines: std::ops::Range<usize>,
    pub visible_visual_rows: std::ops::Range<usize>,
    pub total_logical_lines: usize,
    pub total_visual_lines: usize,
    pub scrollbars: ScrollbarVisibility,
    pub cursor: Option<TextAreaCursorMetrics>,
    pub editor_cursor: Option<TextAreaCursorMetrics>,
}

/// Cursor cell metrics for a [`TextAreaMetrics`] snapshot.
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextAreaCursorMetrics {
    pub byte_offset: usize,
    /// Buffer-based line/column position derived from the underlying value.
    /// Virtual text affects [`rect`](Self::rect), but never this buffer position.
    pub position: TextPosition,
    pub rect: Rect,
    pub visible: bool,
}

/// Non-editable styled text rendered by a [`TextArea`](crate::widgets::TextArea) without entering its value.
///
/// Inline virtual text is anchored before `anchor` and shifts subsequent visual
/// columns. End-of-line virtual text is rendered after the final visual row of
/// the anchor's logical line and does not participate in wrapping.
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextAreaVirtualText {
    pub anchor: usize,
    pub content: Vec<Span>,
    pub placement: VirtualTextPlacement,
    pub priority: u16,
}

impl TextAreaVirtualText {
    /// Create virtual text with explicit placement.
    pub fn new(
        anchor: usize,
        content: impl Into<Vec<Span>>,
        placement: VirtualTextPlacement,
    ) -> Self {
        Self {
            anchor,
            content: virtual_text::sanitize_virtual_text_spans(content.into()),
            placement,
            priority: 0,
        }
    }

    /// Create inline virtual text inserted before the byte at `anchor`.
    pub fn inline(anchor: usize, content: impl Into<Vec<Span>>) -> Self {
        Self::new(anchor, content, VirtualTextPlacement::Inline)
    }

    /// Create end-of-line virtual text for the logical line containing `anchor`.
    pub fn eol(anchor: usize, content: impl Into<Vec<Span>>) -> Self {
        Self::new(anchor, content, VirtualTextPlacement::Eol)
    }

    /// Set ordering among virtual text segments sharing an anchor.
    pub fn priority(mut self, priority: u16) -> Self {
        self.priority = priority;
        self
    }
}

/// Placement for [`TextAreaVirtualText`].
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VirtualTextPlacement {
    Inline,
    Eol,
}
