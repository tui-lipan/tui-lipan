//! Hex area widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_hex_area;
pub use node::HexAreaNode;
pub use reconcile::reconcile_hex_area;

use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, ElementKind};
use crate::style::{BorderStyle, LayoutConstraints, Length, Padding, Rect, Style, StyleSlot};
use crate::widgets::ScrollEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HexAreaHitPart {
    HexHigh,
    HexLow,
    Ascii,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HexAreaPointerHit {
    pub index: usize,
    pub part: HexAreaHitPart,
}

/// Cursor movement event emitted by [`HexArea`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HexAreaCursorEvent {
    /// New cursor byte index.
    pub cursor: usize,
    /// Optional selection anchor byte index.
    pub anchor: Option<usize>,
}

/// Change event emitted by [`HexArea`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HexAreaChangeEvent {
    /// Updated bytes.
    pub bytes: Arc<[u8]>,
    /// Updated cursor byte index.
    pub cursor: usize,
    /// Updated selection anchor.
    pub anchor: Option<usize>,
}

/// Edit kind emitted by [`HexArea`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HexAreaEditKind {
    /// Replaced existing byte.
    Replace,
    /// Inserted a new byte.
    Insert,
    /// Deleted an existing byte.
    Delete,
}

/// Edit event emitted by [`HexArea`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HexAreaEditEvent {
    /// Byte index affected by this edit.
    pub index: usize,
    /// Previous byte value, if any.
    pub before: Option<u8>,
    /// New byte value, if any.
    pub after: Option<u8>,
    /// Edit kind.
    pub kind: HexAreaEditKind,
}

/// Hex/ASCII binary data viewer.
#[derive(Clone)]
pub struct HexArea {
    pub(crate) bytes: Arc<[u8]>,
    pub(crate) cursor: usize,
    pub(crate) anchor: Option<usize>,
    pub(crate) read_only: bool,
    pub(crate) bytes_per_row: u16,
    pub(crate) show_ascii: bool,
    pub(crate) show_offsets: bool,
    pub(crate) uppercase_hex: bool,
    pub(crate) scroll_offset: Option<usize>,
    pub(crate) style: Style,
    pub(crate) hover_style: StyleSlot,
    pub(crate) focus_style: StyleSlot,
    pub(crate) focus_content_style: Style,
    pub(crate) selection_style: StyleSlot,
    pub(crate) cursor_style: Style,
    pub(crate) pending_edit_style: Style,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) padding: Padding,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) disabled: bool,
    pub(crate) disabled_style: Style,
    pub(crate) focusable: bool,
    pub(crate) tab_stop: bool,
    pub(crate) on_focus: Option<Callback<()>>,
    pub(crate) on_blur: Option<Callback<()>>,
    pub(crate) on_cursor_change: Option<Callback<HexAreaCursorEvent>>,
    pub(crate) on_change: Option<Callback<HexAreaChangeEvent>>,
    pub(crate) on_edit: Option<Callback<HexAreaEditEvent>>,
    pub(crate) on_scroll: Option<Callback<ScrollEvent>>,
    pub(crate) on_key: Option<KeyHandler>,
}

impl Default for HexArea {
    fn default() -> Self {
        Self {
            bytes: Arc::from([]),
            cursor: 0,
            anchor: None,
            read_only: true,
            bytes_per_row: 16,
            show_ascii: true,
            show_offsets: true,
            uppercase_hex: true,
            scroll_offset: None,
            style: Style::default(),
            hover_style: StyleSlot::Inherit,
            focus_style: StyleSlot::Inherit,
            focus_content_style: Style::default(),
            selection_style: StyleSlot::Inherit,
            cursor_style: Style::default(),
            pending_edit_style: Style::default(),
            border: true,
            border_style: BorderStyle::Plain,
            padding: Padding::default(),
            width: Length::Flex(1),
            height: Length::Flex(1),
            disabled: false,
            disabled_style: Style::default(),
            focusable: true,
            tab_stop: true,
            on_focus: None,
            on_blur: None,
            on_cursor_change: None,
            on_change: None,
            on_edit: None,
            on_scroll: None,
            on_key: None,
        }
    }
}

impl HexArea {
    /// Create a new hex area.
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            bytes: bytes.into(),
            ..Self::default()
        }
    }

    /// Set bytes to render.
    pub fn bytes(mut self, bytes: impl Into<Arc<[u8]>>) -> Self {
        self.bytes = bytes.into();
        self
    }

    /// Set cursor byte index.
    pub fn cursor(mut self, cursor: usize) -> Self {
        self.cursor = cursor;
        self
    }

    /// Set optional selection anchor byte index.
    pub fn anchor(mut self, anchor: Option<usize>) -> Self {
        self.anchor = anchor;
        self
    }

    /// Set read-only mode.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Set bytes rendered per row.
    pub fn bytes_per_row(mut self, bytes_per_row: u16) -> Self {
        self.bytes_per_row = bytes_per_row.max(1);
        self
    }

    /// Toggle ASCII preview column.
    pub fn show_ascii(mut self, show_ascii: bool) -> Self {
        self.show_ascii = show_ascii;
        self
    }

    /// Toggle offsets gutter.
    pub fn show_offsets(mut self, show_offsets: bool) -> Self {
        self.show_offsets = show_offsets;
        self
    }

    /// Toggle uppercase hex formatting.
    pub fn uppercase_hex(mut self, uppercase_hex: bool) -> Self {
        self.uppercase_hex = uppercase_hex;
        self
    }

    /// Set controlled row scroll offset.
    pub fn scroll_offset(mut self, scroll_offset: Option<usize>) -> Self {
        self.scroll_offset = scroll_offset;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set hover style.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's hover style with additional fields.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit hover style from the active theme.
    pub fn inherit_hover_style(mut self) -> Self {
        self.hover_style = StyleSlot::Inherit;
        self
    }

    /// Set hover style slot directly for composite forwarding.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.hover_style = slot;
        self
    }

    /// Set focus chrome style.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's focus style with additional fields.
    pub fn extend_focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit focus style from the active theme.
    pub fn inherit_focus_style(mut self) -> Self {
        self.focus_style = StyleSlot::Inherit;
        self
    }

    /// Set focus style slot directly for composite forwarding.
    pub fn focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.focus_style = slot;
        self
    }

    /// Set focused content text style.
    pub fn focus_content_style(mut self, style: Style) -> Self {
        self.focus_content_style = style;
        self
    }

    /// Set selection highlight style.
    pub fn selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's selection style with additional fields.
    pub fn extend_selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit selection style from the active theme.
    pub fn inherit_selection_style(mut self) -> Self {
        self.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set selection style slot directly for composite forwarding.
    pub fn selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.selection_style = slot;
        self
    }

    /// Set cursor cell style.
    pub fn cursor_style(mut self, style: Style) -> Self {
        self.cursor_style = style;
        self
    }

    /// Set style used for a half-entered nibble edit.
    pub fn pending_edit_style(mut self, style: Style) -> Self {
        self.pending_edit_style = style;
        self
    }

    /// Set border visibility.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Set disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set disabled style.
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }

    /// Control whether the node is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Control whether the hex area participates in Tab / Shift+Tab traversal.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the hex area gains focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the hex area loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.on_blur = Some(cb);
        self
    }

    /// Set cursor change callback.
    pub fn on_cursor_change(mut self, cb: Callback<HexAreaCursorEvent>) -> Self {
        self.on_cursor_change = Some(cb);
        self
    }

    /// Set change callback.
    pub fn on_change(mut self, cb: Callback<HexAreaChangeEvent>) -> Self {
        self.on_change = Some(cb);
        self
    }

    /// Set edit callback.
    pub fn on_edit(mut self, cb: Callback<HexAreaEditEvent>) -> Self {
        self.on_edit = Some(cb);
        self
    }

    /// Set scroll callback.
    pub fn on_scroll(mut self, cb: Callback<ScrollEvent>) -> Self {
        self.on_scroll = Some(cb);
        self
    }

    /// Set custom key handler.
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
        self
    }
}

impl From<HexArea> for Element {
    fn from(value: HexArea) -> Self {
        let (min_w, min_h) = measure_hex_area(&value);
        let mut layout = LayoutConstraints::default();
        if value.focusable {
            layout.focus_min_w = min_w;
            layout.focus_min_h = min_h;
        }
        Element::new(ElementKind::HexArea(Box::new(value))).with_layout(layout)
    }
}

impl crate::layout::hash::LayoutHash for HexArea {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.border.hash(hasher);
        self.border_style.hash(hasher);
        self.padding.hash(hasher);
        self.bytes_per_row.hash(hasher);
        self.show_ascii.hash(hasher);
        self.show_offsets.hash(hasher);

        let needs_content =
            matches!(self.width, Length::Auto) || matches!(self.height, Length::Auto);
        if needs_content {
            self.bytes.len().hash(hasher);
        }
        Some(())
    }
}

pub(crate) struct HexAreaPointerHitArgs {
    pub bytes_len: usize,
    pub cursor: usize,
    pub bytes_per_row: u16,
    pub show_offsets: bool,
    pub show_ascii: bool,
    pub scroll_offset: Option<usize>,
    pub border: bool,
    pub padding: Padding,
}

pub(crate) fn pointer_hit(
    rect: Rect,
    args: HexAreaPointerHitArgs,
    x: u16,
    y: u16,
) -> Option<HexAreaPointerHit> {
    let HexAreaPointerHitArgs {
        bytes_len,
        cursor,
        bytes_per_row,
        show_offsets,
        show_ascii,
        scroll_offset,
        border,
        padding,
    } = args;
    if bytes_len == 0 {
        return None;
    }

    let inner = rect.inner(border, padding);
    if inner.w == 0 || inner.h == 0 || !inner.contains(x as i16, y as i16) {
        return None;
    }

    let bytes_per_row = bytes_per_row.max(1) as usize;
    let total_rows = bytes_len.div_ceil(bytes_per_row).max(1);
    let visible_rows = inner.h as usize;
    let clamped_cursor = cursor.min(bytes_len.saturating_sub(1));
    let start_row = scroll_offset.map_or_else(
        || {
            if visible_rows == 0 {
                0
            } else {
                let cursor_row = clamped_cursor / bytes_per_row;
                cursor_row.saturating_sub(visible_rows.saturating_sub(1))
            }
        },
        |offset| offset.min(total_rows.saturating_sub(1)),
    );

    let rel_y = (y as i16).saturating_sub(inner.y) as usize;
    let row = start_row.saturating_add(rel_y);
    if row >= total_rows {
        return None;
    }

    let row_start = row.saturating_mul(bytes_per_row);
    let rel_x = (x as i16).saturating_sub(inner.x) as usize;

    let offsets_w: usize = if show_offsets { 10 } else { 0 };
    let hex_w = bytes_per_row.saturating_mul(3).saturating_sub(1);
    let ascii_start = offsets_w.saturating_add(hex_w).saturating_add(2);

    if rel_x >= offsets_w {
        let local = rel_x - offsets_w;
        if local < hex_w {
            let col = local / 3;
            let in_cell = local % 3;
            if in_cell != 2 {
                let index = row_start.saturating_add(col);
                if index < bytes_len {
                    let part = if in_cell == 0 {
                        HexAreaHitPart::HexHigh
                    } else {
                        HexAreaHitPart::HexLow
                    };
                    return Some(HexAreaPointerHit { index, part });
                }
            }
        }
    }

    if show_ascii && rel_x >= ascii_start {
        let col = rel_x - ascii_start;
        if col < bytes_per_row {
            let index = row_start.saturating_add(col);
            if index < bytes_len {
                return Some(HexAreaPointerHit {
                    index,
                    part: HexAreaHitPart::Ascii,
                });
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{HexAreaHitPart, HexAreaPointerHitArgs, pointer_hit};
    use crate::style::{Padding, Rect};

    #[test]
    fn pointer_hit_maps_hex_cells() {
        let hit = pointer_hit(
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 4,
            },
            HexAreaPointerHitArgs {
                bytes_len: 32,
                cursor: 0,
                bytes_per_row: 16,
                show_offsets: true,
                show_ascii: true,
                scroll_offset: Some(0),
                border: false,
                padding: Padding::default(),
            },
            10,
            0,
        )
        .expect("expected hex hit");

        assert_eq!(hit.index, 0);
        assert_eq!(hit.part, HexAreaHitPart::HexHigh);

        let hit_low = pointer_hit(
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 4,
            },
            HexAreaPointerHitArgs {
                bytes_len: 32,
                cursor: 0,
                bytes_per_row: 16,
                show_offsets: true,
                show_ascii: true,
                scroll_offset: Some(0),
                border: false,
                padding: Padding::default(),
            },
            11,
            0,
        )
        .expect("expected low nibble hit");

        assert_eq!(hit_low.index, 0);
        assert_eq!(hit_low.part, HexAreaHitPart::HexLow);
    }

    #[test]
    fn pointer_hit_maps_ascii_cells() {
        let hit = pointer_hit(
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 4,
            },
            HexAreaPointerHitArgs {
                bytes_len: 32,
                cursor: 0,
                bytes_per_row: 16,
                show_offsets: true,
                show_ascii: true,
                scroll_offset: Some(0),
                border: false,
                padding: Padding::default(),
            },
            59,
            0,
        )
        .expect("expected ascii hit");

        assert_eq!(hit.index, 0);
        assert_eq!(hit.part, HexAreaHitPart::Ascii);
    }
}
