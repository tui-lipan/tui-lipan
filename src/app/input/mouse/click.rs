use crate::app::input::drag::ClickState;
use crate::app::input::mouse::types::{InputChange, TextAreaChange};
use crate::app::input::text::{
    TextAreaCursorCoords, TextAreaCursorLayout, TextAreaCursorParams, paragraph_bounds_at_byte,
    word_at_byte,
};
use crate::core::event::MouseEvent;
use crate::style::Rect;
use crate::widgets::{
    IMAGE_SENTINEL_BASE, SENTINEL_BASE, SentinelId, TextAreaImageMode, TextAreaSentinelClickEvent,
    TextAreaSentinelClickKind, text_area_cursor_reserve, text_area_total_gutter_width,
};
use std::time::Duration;
use web_time::Instant;

pub(crate) fn click_count_at(
    last_click: &mut Option<ClickState>,
    x: u16,
    y: u16,
    compare_y: bool,
) -> u8 {
    let now = Instant::now();
    let click_count = if let Some(last) = last_click.as_ref() {
        let same_pos = last.x == x && (!compare_y || last.y == y);
        if same_pos && now.duration_since(last.time) < Duration::from_millis(400) {
            (last.count % 3) + 1
        } else {
            1
        }
    } else {
        1
    };

    *last_click = Some(ClickState {
        x,
        y,
        time: now,
        count: click_count,
    });

    click_count
}

fn textarea_cursor_params<'a>(
    change: &'a TextAreaChange,
    x: u16,
    y: u16,
    inner: Rect,
    clamp_to_inner: bool,
) -> TextAreaCursorParams<'a> {
    TextAreaCursorParams {
        value: change.value.as_ref(),
        current_cursor: change.cursor,
        coords: TextAreaCursorCoords {
            x,
            y,
            inner,
            clamp_to_inner,
        },
        layout: TextAreaCursorLayout {
            line_numbers: change.line_numbers,
            min_line_number_width: change.min_line_number_width,
            wrap: change.wrap,
            scroll_offset: change.scroll_offset,
            scrollbar: change.scrollbar,
            scrollbar_variant: change.scrollbar_variant,
            scrollbar_gap: change.scrollbar_gap,
            scrollbar_over_border: change.scrollbar_over_border,
            h_scrollbar: change.h_scrollbar,
            h_scrollbar_variant: change.h_scrollbar_variant,
            h_scrollbar_over_border: change.h_scrollbar_over_border,
            max_line_width: change.max_line_width,
            h_scroll_offset: change.h_scroll_offset,
            tab_stop: change.tab_stop,
            gutter_col_width: change.gutter_col_width,
            gutter_gap: change.gutter_gap,
            logical_lines_count: change.logical_lines_count,
        },
        read_only: change.read_only,
        sentinel: change.sentinel_info.clone(),
        visual_lines: change.visual_lines.as_deref(),
        virtual_texts: &change.virtual_texts,
    }
}

/// Process a textarea click with double/triple-click detection.
pub(crate) fn process_textarea_click(
    change: &TextAreaChange,
    x: u16,
    y: u16,
    last_click: &mut Option<ClickState>,
) -> (usize, Option<usize>, usize) {
    let inner = change.rect.inner(change.border, change.padding);

    let use_standalone_scrollbar = change.scrollbar && !change.scrollbar_over_border;
    let scrollbar_x = if use_standalone_scrollbar {
        inner.x.saturating_add(inner.w.saturating_sub(1) as i16)
    } else {
        i16::MAX // Out of bounds
    };

    // Skip if clicking on standalone scrollbar
    if use_standalone_scrollbar && (x as i16) == scrollbar_x {
        return (change.cursor, change.anchor, change.cursor);
    }

    let next_cursor = crate::app::input::text::textarea_cursor_from_coords(textarea_cursor_params(
        change, x, y, inner, true,
    ));

    let click_count = click_count_at(last_click, x, y, true);
    let click_count = if change.multi_click_select {
        click_count
    } else {
        click_count.min(1)
    };

    let (new_cursor, new_anchor) = match click_count {
        2 => {
            // Double-click: select word (or atomic sentinel)
            let text = change.value.as_ref();
            let (start, end) = word_at_byte(text, next_cursor, change.sentinel_info.clone());
            (end, Some(start))
        }
        3 => {
            let text = change.value.as_ref();
            let (start, end) = match change.triple_click_mode {
                crate::widgets::TripleClickSelectionMode::Line => {
                    let line_start = text[..next_cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
                    let line_end = text[next_cursor..]
                        .find('\n')
                        .map(|i| next_cursor + i)
                        .unwrap_or(text.len());
                    (line_start, line_end)
                }
                crate::widgets::TripleClickSelectionMode::Paragraph => {
                    paragraph_bounds_at_byte(text, next_cursor)
                }
            };
            (end, Some(start))
        }
        _ => {
            // Single click: move cursor, clear selection
            (next_cursor, None)
        }
    };

    (new_cursor, new_anchor, new_anchor.unwrap_or(new_cursor))
}

/// Emit a sentinel click event when the pointer is on an inline image or custom sentinel.
pub(crate) fn process_textarea_sentinel_click(
    change: &TextAreaChange,
    mouse: MouseEvent,
    x: u16,
    y: u16,
) -> bool {
    let Some(cb) = change.on_sentinel_click.as_ref() else {
        return false;
    };

    let Some(event) = textarea_sentinel_event_at_coords(change, mouse, x, y) else {
        return false;
    };

    cb.emit(event);
    true
}

fn textarea_sentinel_event_at_coords(
    change: &TextAreaChange,
    mouse: MouseEvent,
    x: u16,
    y: u16,
) -> Option<TextAreaSentinelClickEvent> {
    let inner = change.rect.inner(change.border, change.padding);
    if inner.w == 0 || inner.h == 0 {
        return None;
    }

    let gutter_width = text_area_total_gutter_width(
        change.logical_lines_count,
        change.line_numbers,
        change.min_line_number_width,
        change.gutter_col_width,
        change.gutter_gap,
    ) as usize;

    let scrollbar_cols = if change.scrollbar && !change.scrollbar_over_border {
        1u16.saturating_add(change.scrollbar_gap)
    } else {
        0
    };
    let content_width = inner
        .w
        .saturating_sub(gutter_width as u16)
        .saturating_sub(scrollbar_cols)
        .saturating_sub(text_area_cursor_reserve(change.wrap, change.read_only));
    if content_width == 0 {
        return None;
    }

    let h_scrollbar_visible =
        change.h_scrollbar && !change.wrap && change.max_line_width > content_width as usize;
    let content_height = inner.h.saturating_sub(u16::from(
        h_scrollbar_visible && !change.h_scrollbar_over_border,
    ));
    if content_height == 0 {
        return None;
    }

    let content_x = inner.x.saturating_add(gutter_width as i16);
    let content_right = content_x.saturating_add(content_width as i16);
    let content_bottom = inner.y.saturating_add(content_height as i16);
    let x_i16 = x as i16;
    let y_i16 = y as i16;
    if x_i16 < content_x || x_i16 >= content_right || y_i16 < inner.y || y_i16 >= content_bottom {
        return None;
    }

    let cursor = crate::app::input::text::textarea_cursor_from_coords(textarea_cursor_params(
        change, x, y, inner, false,
    ));

    let ch = change.value.get(cursor..)?.chars().next()?;
    let byte_range = (cursor, cursor.saturating_add(ch.len_utf8()));
    let cp = ch as u32;

    if change.image_mode == TextAreaImageMode::Inline {
        let base = IMAGE_SENTINEL_BASE as u32;
        let idx = cp.checked_sub(base)? as usize;
        if let Some(image) = change.images.get(idx) {
            return Some(TextAreaSentinelClickEvent {
                kind: TextAreaSentinelClickKind::Image {
                    index: idx,
                    image: image.clone(),
                },
                byte_range,
                mouse,
            });
        }
    }

    let base = SENTINEL_BASE as u32;
    let idx = cp.checked_sub(base)? as usize;
    let sentinel = change.sentinels.get(idx)?.clone();
    Some(TextAreaSentinelClickEvent {
        kind: TextAreaSentinelClickKind::Custom {
            index: idx,
            id: sentinel.sentinel_id().unwrap_or(SentinelId::UNKNOWN),
            sentinel,
        },
        byte_range,
        mouse,
    })
}

/// Process an input click with double/triple-click detection.
pub(crate) fn process_input_click(
    change: &InputChange,
    x: u16,
    last_click: &mut Option<ClickState>,
) -> (usize, Option<usize>, usize) {
    let inner = change.rect.inner(change.border, change.padding);

    if inner.w == 0 {
        return (change.cursor, change.anchor, change.cursor);
    }

    let next_cursor = crate::app::input::text::input_cursor_from_coords(
        &change.value,
        change.prefix.as_deref(),
        x,
        change.cursor,
        inner,
    );

    let click_count = click_count_at(last_click, x, 0, false);

    let text = change.value.as_ref();
    let line = text.lines().next().unwrap_or("");

    let (new_cursor, new_anchor) = if change.masked {
        match click_count {
            2 | 3 => {
                // Treat masked input as a single word.
                (line.len(), Some(0))
            }
            _ => (next_cursor, None),
        }
    } else {
        match click_count {
            2 => {
                // Double-click: select word
                let (start, end) = word_at_byte(line, next_cursor, None);
                (end, Some(start))
            }
            3 => {
                // Triple-click: select all (for single-line input)
                (line.len(), Some(0))
            }
            _ => {
                // Single click: move cursor, clear selection
                (next_cursor, None)
            }
        }
    };

    (new_cursor, new_anchor, new_anchor.unwrap_or(new_cursor))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use crate::callback::Callback;
    use crate::clipboard::ImageContent;
    use crate::core::event::{KeyMods, MouseButton, MouseEvent, MouseKind};
    use crate::core::node::NodeId;
    use crate::style::{Padding, Rect, ScrollbarVariant};
    use crate::widgets::{
        IMAGE_SENTINEL_BASE, SENTINEL_BASE, TextAreaImageMode, TextAreaSentinel,
        TextAreaSentinelClickEvent, TextAreaSentinelClickKind, sentinel_info_for,
    };

    use super::{TextAreaChange, process_textarea_sentinel_click};

    fn mouse() -> MouseEvent {
        MouseEvent {
            x: 0,
            y: 0,
            kind: MouseKind::Up(MouseButton::Left),
            mods: KeyMods::NONE,
        }
    }

    fn change(
        value: &str,
        images: Vec<ImageContent>,
        image_mode: TextAreaImageMode,
        sentinels: Vec<TextAreaSentinel>,
        on_sentinel_click: Callback<TextAreaSentinelClickEvent>,
    ) -> TextAreaChange {
        TextAreaChange {
            on_change: None,
            on_editor_state_change: None,
            value: Arc::from(value),
            cursor: 0,
            anchor: None,
            focusable: true,
            border: false,
            padding: Padding::default(),
            line_numbers: false,
            min_line_number_width: 0,
            wrap: true,
            scroll_offset: 0,
            scrollbar: false,
            scrollbar_variant: ScrollbarVariant::default(),
            scrollbar_gap: 0,
            scrollbar_over_border: false,
            h_scrollbar: false,
            h_scrollbar_variant: ScrollbarVariant::default(),
            h_scrollbar_over_border: false,
            max_line_width: 0,
            h_scroll_offset: 0,
            rect: Rect {
                x: 0,
                y: 0,
                w: 30,
                h: 3,
            },
            node_id: NodeId::INVALID,
            read_only: false,
            vim_motions: false,
            on_vim_mode_change: None,
            sentinel_info: sentinel_info_for(image_mode, images.len(), "[Image]", &sentinels),
            tab_stop: 8,
            gutter_col_width: 0,
            gutter_gap: 0,
            logical_lines_count: 1,
            visual_lines: None,
            virtual_texts: Vec::new(),
            multi_click_select: true,
            triple_click_mode: crate::widgets::TripleClickSelectionMode::Line,
            on_sentinel_click: Some(on_sentinel_click),
            #[cfg(feature = "diff-view")]
            diff_context_separator_click: None,
            images,
            image_mode,
            sentinels,
        }
    }

    #[test]
    fn clicking_custom_sentinel_emits_event() {
        let value = format!("a{}b", SENTINEL_BASE);
        let events = Rc::new(RefCell::new(Vec::new()));
        let events_cb = events.clone();
        let cb = Callback::new(move |event| events_cb.borrow_mut().push(event));
        let sentinel = TextAreaSentinel::new("[More]").payload(String::from("expanded text"));
        let change = change(
            &value,
            Vec::new(),
            TextAreaImageMode::Inline,
            vec![sentinel],
            cb,
        );

        assert!(process_textarea_sentinel_click(&change, mouse(), 2, 0));

        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].byte_range, (1, 4));
        assert!(matches!(
            &events[0].kind,
            TextAreaSentinelClickKind::Custom { index: 0, .. }
        ));
    }

    #[test]
    fn clicking_inline_image_sentinel_emits_event() {
        let value = format!("a{}b", IMAGE_SENTINEL_BASE);
        let events = Rc::new(RefCell::new(Vec::new()));
        let events_cb = events.clone();
        let cb = Callback::new(move |event| events_cb.borrow_mut().push(event));
        let image = ImageContent {
            data: "abc".to_string(),
            mime: "image/png",
            filename: Some("img.png".to_string()),
        };
        let change = change(
            &value,
            vec![image.clone()],
            TextAreaImageMode::Inline,
            Vec::new(),
            cb,
        );

        assert!(process_textarea_sentinel_click(&change, mouse(), 3, 0));

        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].byte_range, (1, 4));
        assert_eq!(
            &events[0].kind,
            &TextAreaSentinelClickKind::Image { index: 0, image }
        );
    }

    #[test]
    fn clicking_gutter_does_not_emit_sentinel_event() {
        let value = SENTINEL_BASE.to_string();
        let events = Rc::new(RefCell::new(Vec::new()));
        let events_cb = events.clone();
        let cb = Callback::new(move |event| events_cb.borrow_mut().push(event));
        let sentinel = TextAreaSentinel::new("[More]");
        let mut change = change(
            &value,
            Vec::new(),
            TextAreaImageMode::Inline,
            vec![sentinel],
            cb,
        );
        change.line_numbers = true;
        change.sentinel_info = sentinel_info_for(
            change.image_mode,
            change.images.len(),
            "[Image]",
            &change.sentinels,
        );

        assert!(!process_textarea_sentinel_click(&change, mouse(), 0, 0));
        assert!(events.borrow().is_empty());
    }
}
