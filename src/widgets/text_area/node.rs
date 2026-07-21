use std::rc::Rc;
use std::sync::Arc;

use crate::app::TextAreaNewlineBinding;
use crate::callback::{Callback, KeyHandler};
use crate::clipboard::ImageContent;
use crate::core::event::MouseEvent;
use crate::core::node::{ScrollbarZone, WidgetNode};
use crate::input::KeyBindings;
use crate::style::{
    BorderStyle, CaretShape, Color, Padding, Rect, ScrollbarVariant, Style, StyleSlot, Theme,
    ThemeRole,
};
use crate::text::edit::TextEditEvent;
use crate::text::line_index::LineIndex;
use crate::widgets::scroll::SmoothScrollState;
use crate::widgets::scroll_view::ScrollEvent;
use crate::widgets::text_area::{
    TextAreaClipboardTransform, TextAreaColorCache, TextAreaColorStrategy, TextAreaCursorMetrics,
    TextAreaEvent, TextAreaImageMode, TextAreaLineNumberMode, TextAreaMetrics,
    TextAreaStateChangeEvent, TextAreaVimConfig, TextAreaVimKeymap, TextAreaVimMode,
    TextAreaVimSearchFeedback,
};

use super::layout::{TextAreaGeometry, TextAreaVisualCache, logical_line_count};

#[derive(Clone)]
pub struct TextAreaNode {
    pub value: Arc<str>,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub placeholder: Option<Arc<str>>,
    pub style: Style,
    pub hover_style: StyleSlot,
    pub focus_style: StyleSlot,
    pub focus_content_style: Style,
    pub hover_border_style: Option<BorderStyle>,
    pub caret_shape: CaretShape,
    pub caret_color: Option<Color>,
    pub selection_style: StyleSlot,
    pub unfocused_selection_style: StyleSlot,
    pub show_selection_when_unfocused: bool,
    pub placeholder_style: Style,
    pub focus_placeholder_style: Style,
    pub line_numbers: bool,
    pub line_number_mode: TextAreaLineNumberMode,
    pub line_number_style: Style,
    pub min_line_number_width: u8,
    pub wrap: bool,
    pub color_strategy: Option<Rc<dyn TextAreaColorStrategy>>,
    pub language: Option<Arc<str>>,
    pub theme: Option<Arc<str>>,
    pub border: bool,
    pub border_style: BorderStyle,
    pub padding: Padding,
    pub scroll_offset: usize,
    /// Requested zero-based logical/source line for programmatic target scroll.
    pub scroll_to_line: Option<usize>,
    pub cancelled_scroll_to_line: Option<usize>,
    pub scroll_behavior: crate::widgets::ScrollBehavior,
    pub smooth_scroll: SmoothScrollState,
    pub scroll_wheel: bool,
    pub scroll_wheel_multiplier: Option<u16>,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    pub scrollbar_thumb: Option<char>,
    pub scrollbar_thumb_style: Option<Style>,
    pub scrollbar_thumb_focus_style: Option<Style>,
    pub scrollbar_track_style: Option<Style>,
    pub h_scrollbar: bool,
    pub h_scrollbar_variant: ScrollbarVariant,
    pub h_scrollbar_thumb: Option<char>,
    #[cfg(feature = "diff-view")]
    pub pin_scrollbar_focus: bool,
    pub max_line_width: usize, // Maximum line width in columns (for horizontal scrollbar)
    pub h_scroll_offset: usize,
    pub h_scroll_override: Option<usize>,
    pub visual_lines_count: usize, // Total visual lines (after wrapping)
    pub logical_lines_count: usize,
    pub scroll_override: Option<usize>,
    pub disabled: bool,

    pub content_hash: u64,
    pub visual_cache: TextAreaVisualCache,
    pub color_cache: TextAreaColorCache,
    pub geometry: TextAreaGeometry,

    pub disabled_style: Style,
    pub read_only: bool,
    pub newline_binding: Option<TextAreaNewlineBinding>,
    pub tab_width: u8,
    pub insert_tab: bool,
    pub tab_display_width: u8,
    pub on_change: Option<Callback<TextAreaEvent>>,
    pub on_edit: Option<Callback<TextEditEvent>>,
    pub on_editor_state_change: Option<Callback<TextAreaStateChangeEvent>>,
    pub on_scroll: Option<Callback<ScrollEvent>>,
    pub on_scroll_to: Option<Callback<usize>>,
    pub on_click: Option<Callback<MouseEvent>>,
    pub on_key: Option<KeyHandler>,
    pub key_interceptor: Option<KeyHandler>,
    pub clear_bindings: Option<KeyBindings>,
    pub vim_motions: bool,
    pub vim_keymap: Option<TextAreaVimKeymap>,
    pub vim_config: TextAreaVimConfig,
    pub vim_mode: TextAreaVimMode,
    pub vim_visual_line_caret: Option<usize>,
    pub vim_search_feedback: Option<TextAreaVimSearchFeedback>,
    pub vim_yank_feedback_range: Option<(usize, usize)>,
    pub on_vim_mode_change: Option<Callback<TextAreaVimMode>>,
    pub on_image_paste: Option<Callback<ImageContent>>,
    pub on_text_paste: Option<Callback<super::TextAreaPasteEvent>>,
    pub images: Vec<ImageContent>,
    pub on_images_change: Option<Callback<Vec<ImageContent>>>,
    pub image_mode: TextAreaImageMode,
    pub image_placeholder: Arc<str>,
    pub image_placeholder_style: Style,
    pub image_placeholder_focus_style: Style,
    pub image_placeholder_hover_style: Style,
    pub focusable: bool,
    pub tab_stop: bool,
    pub on_focus: Option<Callback<()>>,
    pub on_blur: Option<Callback<()>>,
    /// Custom gutter spans (per logical line, 0-based). When set, overrides
    /// the built-in `line_numbers` gutter.
    pub gutter_lines: Option<Arc<Vec<Vec<crate::style::Span>>>>,
    /// Fixed column width for the custom gutter. Overrides `line_numbers`
    /// gutter width when > 0.
    pub gutter_col_width: u16,
    /// Fixed empty cells between gutter and text content.
    pub gutter_gap: u16,
    /// Peer logical source lines for split-wrap synchronization padding.
    pub peer_source_lines: Option<Arc<Vec<Arc<str>>>>,
    #[cfg(feature = "diff-view")]
    pub split_wrap_sync: Option<crate::widgets::diff_view::SharedSplitWrapSync>,
    #[cfg(feature = "diff-view")]
    pub split_wrap_side: Option<crate::widgets::diff_view::SplitPaneSide>,
    #[cfg(feature = "diff-view")]
    pub diff_context_separator_click:
        Option<crate::widgets::diff_view::DiffContextSeparatorClickConfig>,
    /// Style used for synthetic wrap-padding gutter rows inserted for peer sync.
    pub split_wrap_padding_gutter_style: Option<Style>,
    /// Style used for synthetic wrap-padding content rows inserted for peer sync.
    pub split_wrap_padding_style: Option<Style>,
    /// Byte ranges in `value` to exclude from clipboard copy (sorted, non-overlapping).
    pub copy_excluded_bytes: Option<Arc<Vec<(usize, usize)>>>,
    /// Optional transform applied to selected text immediately before clipboard write.
    pub clipboard_transform: Option<TextAreaClipboardTransform>,
    /// 0-based logical line indices whose selection newline highlight is suppressed.
    pub selection_excluded_lines: Option<Arc<Vec<usize>>>,
    /// Enable word/line selection on double/triple click.
    pub multi_click_select: bool,
    /// Triple-click selection behavior.
    pub triple_click_mode: crate::widgets::TripleClickSelectionMode,
    /// Ordered list of custom inline sentinels.
    pub sentinels: Vec<super::TextAreaSentinel>,
    /// Callback invoked when the sentinels list changes.
    pub on_sentinels_change: Option<crate::callback::Callback<Vec<super::TextAreaSentinel>>>,
    /// Fires [`crate::widgets::SentinelEvent`] batches (e.g. deleted sentinels).
    pub on_sentinel_event: Option<crate::callback::Callback<Vec<super::SentinelEvent>>>,
    /// Fires when an inline image or custom sentinel placeholder is clicked.
    pub on_sentinel_click: Option<crate::callback::Callback<super::TextAreaSentinelClickEvent>>,
    pub decorations: Vec<super::TextAreaDecoration>,
    pub virtual_texts: Vec<super::TextAreaVirtualText>,
}

impl TextAreaNode {
    fn has_diff_context_separator_click(&self) -> bool {
        #[cfg(feature = "diff-view")]
        {
            self.diff_context_separator_click
                .as_ref()
                .is_some_and(|config| config.on_click.is_some())
        }
        #[cfg(not(feature = "diff-view"))]
        {
            false
        }
    }

    fn has_diff_context_separator_hover(&self) -> bool {
        #[cfg(feature = "diff-view")]
        {
            self.diff_context_separator_click
                .as_ref()
                .and_then(|config| config.hover_style)
                .is_some_and(|style| !style.is_empty())
        }
        #[cfg(not(feature = "diff-view"))]
        {
            false
        }
    }

    pub(crate) fn metrics(&self, rect: Rect) -> TextAreaMetrics {
        let inner = rect.inner(self.border, self.padding);
        let editor_h = self.geometry.content_viewport_h(
            self.h_scrollbar
                && matches!(self.h_scrollbar_variant, ScrollbarVariant::Integrated)
                && self.border,
        ) as usize;
        let content_rect = Rect {
            x: inner.x.saturating_add(self.geometry.gutter_width as i16),
            y: inner.y,
            w: self.geometry.content_width.min(u16::MAX as usize) as u16,
            h: editor_h.min(u16::MAX as usize) as u16,
        };
        let lines = self.visual_cache.latest_lines().unwrap_or(&[]);
        let start = self.scroll_offset.min(lines.len());
        let end = start.saturating_add(editor_h).min(lines.len());
        let visible_logical_lines = if start < end {
            let first = lines[start].line_num.saturating_sub(1);
            let last = lines[end - 1].line_num;
            first..last
        } else {
            0..0
        };
        let editor_cursor = self.cursor_metrics(self.cursor, content_rect, start, end, false);
        let cursor = self
            .vim_search_feedback
            .as_ref()
            .and_then(|feedback| {
                feedback.pending.then(|| TextAreaCursorMetrics {
                    byte_offset: self.cursor,
                    position: LineIndex::new(&self.value)
                        .byte_to_position(&self.value, self.cursor),
                    rect: Rect {
                        x: inner.x,
                        y: inner.y.saturating_add(editor_h as i16),
                        w: 1,
                        h: 1,
                    },
                    visible: true,
                })
            })
            .or_else(|| editor_cursor.clone());
        TextAreaMetrics {
            rect,
            inner_rect: inner,
            content_rect,
            gutter_width: self.geometry.gutter_width.min(u16::MAX as usize) as u16,
            scroll_offset: self.scroll_offset,
            h_scroll_offset: self.h_scroll_offset,
            visible_logical_lines,
            visible_visual_rows: start..end,
            total_logical_lines: self.logical_lines_count,
            total_visual_lines: self.geometry.total_visual_lines,
            scrollbars: crate::core::component::ScrollbarVisibility {
                v: self.geometry.v_scrollbar_visible,
                h: self.geometry.h_scrollbar_visible,
            },
            cursor,
            editor_cursor,
        }
    }

    fn cursor_metrics(
        &self,
        byte: usize,
        content_rect: Rect,
        visible_start: usize,
        visible_end: usize,
        allow_hidden: bool,
    ) -> Option<TextAreaCursorMetrics> {
        let byte = crate::utils::text::clamp_cursor(&self.value, byte);
        let lines = self.visual_cache.latest_lines().unwrap_or(&[]);
        let mut rect = Rect {
            x: content_rect.x,
            y: content_rect.y,
            w: 1,
            h: 1,
        };
        let mut visible = false;
        for (idx, line) in lines.iter().enumerate() {
            let line_start = crate::utils::text::clamp_cursor(&self.value, line.start);
            let raw_line_end = line.end.min(self.value.len());
            let line_end = if self.value.is_char_boundary(raw_line_end) {
                raw_line_end
            } else {
                crate::utils::text::next_char_boundary(&self.value, raw_line_end)
            };
            let (line_start, line_end) = if line_start <= line_end {
                (line_start, line_end)
            } else {
                (line_end, line_start)
            };
            let next_starts_at_boundary = lines.get(idx + 1).is_some_and(|next| {
                next.line_num == line.line_num
                    && next.continuation
                    && crate::utils::text::clamp_cursor(&self.value, next.start) == byte
            });
            let contains_cursor = (byte == line_start && line.continuation)
                || (byte >= line_start && byte < line_end)
                || (byte == line_end && !next_starts_at_boundary);
            if contains_cursor {
                if idx >= visible_start && idx < visible_end {
                    rect.y = content_rect.y.saturating_add((idx - visible_start) as i16);
                    let logical_line_start =
                        self.value[..line_start].rfind('\n').map_or(0, |i| i + 1);
                    let logical_line_end = self.value[logical_line_start..]
                        .find('\n')
                        .map(|i| logical_line_start + i)
                        .unwrap_or(self.value.len());
                    let insertions = super::inline_virtual_insertions_for_line(
                        &self.value,
                        &self.virtual_texts,
                        logical_line_start,
                        logical_line_end,
                    );
                    let sentinel = crate::widgets::sentinel_info_for(
                        self.image_mode,
                        self.images.len(),
                        &self.image_placeholder,
                        &self.sentinels,
                    );
                    let logical_cursor_col = crate::utils::text::visual_col_with_virtual(
                        &self.value[logical_line_start..byte],
                        0,
                        self.tab_display_width as usize,
                        sentinel.as_ref(),
                        &insertions,
                    );
                    let full_col = if self.wrap {
                        logical_cursor_col.saturating_sub(line.visual_start_col)
                    } else {
                        logical_cursor_col
                    };
                    let is_visible_horizontally = self.wrap
                        || (full_col >= self.h_scroll_offset
                            && full_col <= self.h_scroll_offset + content_rect.w as usize);
                    if !is_visible_horizontally {
                        break;
                    }
                    let col = if self.wrap {
                        full_col
                    } else {
                        full_col - self.h_scroll_offset
                    };
                    rect.x = content_rect.x.saturating_add(col as i16);
                    visible = rect.x >= content_rect.x
                        && rect.x < content_rect.x.saturating_add(content_rect.w as i16);
                }
                break;
            }
        }
        if !visible && !allow_hidden {
            return None;
        }
        let index = LineIndex::new(&self.value);
        Some(TextAreaCursorMetrics {
            byte_offset: byte,
            position: index.byte_to_position(&self.value, byte),
            rect,
            visible,
        })
    }
}

impl WidgetNode for TextAreaNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }

    fn is_tab_stop(&self) -> bool {
        self.focusable && self.tab_stop
    }

    fn on_focus_callback(&self) -> Option<&Callback<()>> {
        self.on_focus.as_ref()
    }

    fn on_blur_callback(&self) -> Option<&Callback<()>> {
        self.on_blur.as_ref()
    }

    fn has_on_click(&self) -> bool {
        !self.disabled
            && (self.on_click.is_some()
                || self.has_diff_context_separator_click()
                || self.on_sentinel_click.is_some()
                || self.on_change.is_some()
                || self.on_scroll.is_some()
                || self.on_scroll_to.is_some()
                || self.scrollbar
                || self.h_scrollbar
                || self.scroll_wheel)
    }

    fn is_hoverable(&self) -> bool {
        // Only hoverable if explicitly styled for hover, or has an on_click handler.
        // Having on_change/on_scroll does not make the widget hoverable since there's
        // no visual feedback for those interactions.
        !self.disabled
            && (self.on_click.is_some()
                || self.on_sentinel_click.is_some()
                || self.has_diff_context_separator_hover()
                || self.hover_style.has_explicit_style()
                || self.hover_border_style.is_some()
                || self.image_placeholder_hover_style != Style::default()
                || self.sentinels.iter().any(|s| s.hover_style.is_some()))
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        !self.disabled
            && (self.on_click.is_some()
                || self.on_sentinel_click.is_some()
                || self.has_diff_context_separator_hover()
                || self.hover_style.resolves_non_empty(theme, ThemeRole::Hover)
                || self.hover_border_style.is_some()
                || self.image_placeholder_hover_style != Style::default()
                || self.sentinels.iter().any(|s| s.hover_style.is_some()))
    }

    fn scrollbar_zones(
        &self,
        id: crate::core::node::NodeId,
        rect: Rect,
        _parent_border_x: Option<i16>,
        _parent_border_y: Option<i16>,
    ) -> Vec<ScrollbarZone> {
        if self.disabled {
            return Vec::new();
        }

        if rect.w == 0 || rect.h == 0 {
            return Vec::new();
        }

        self.geometry
            .scrollbar_zones
            .iter()
            .copied()
            .map(|mut zone| {
                zone.id = id;
                zone
            })
            .collect()
    }
}

impl From<super::TextArea> for TextAreaNode {
    fn from(value: super::TextArea) -> Self {
        let logical_lines_count = logical_line_count(&value.value);
        Self {
            value: value.value,
            cursor: value.cursor,
            anchor: value.anchor,
            placeholder: value.placeholder,
            style: value.style,
            hover_style: value.hover_style,
            focus_style: value.focus_style,
            focus_content_style: value.focus_content_style,
            hover_border_style: value.hover_border_style,
            caret_shape: value.caret_shape,
            caret_color: value.caret_color,
            selection_style: value.selection_style,
            unfocused_selection_style: value.unfocused_selection_style,
            show_selection_when_unfocused: value.show_selection_when_unfocused,
            placeholder_style: value.placeholder_style,
            focus_placeholder_style: value.focus_placeholder_style,
            line_numbers: value.line_numbers,
            line_number_mode: value.line_number_mode,
            line_number_style: value.line_number_style,
            min_line_number_width: value.min_line_number_width,
            wrap: value.wrap,
            color_strategy: value.color_strategy,
            language: value.language,
            theme: value.theme,
            border: value.border,
            border_style: value.border_style,
            padding: value.padding,
            scroll_offset: value.scroll_offset.unwrap_or(0),
            scroll_to_line: value.scroll_to_line,
            cancelled_scroll_to_line: None,
            scroll_behavior: value.scroll_behavior,
            smooth_scroll: SmoothScrollState::default(),
            scroll_wheel: value.scroll_wheel,
            scroll_wheel_multiplier: value.scroll_wheel_multiplier,
            scrollbar: value.scrollbar,
            scrollbar_variant: value.scrollbar_config.variant,
            scrollbar_gap: value.scrollbar_config.gap,
            scrollbar_thumb: value.scrollbar_config.thumb,
            scrollbar_thumb_style: value.scrollbar_config.thumb_style,
            scrollbar_thumb_focus_style: value.scrollbar_config.thumb_focus_style,
            scrollbar_track_style: value.scrollbar_config.track_style,
            h_scrollbar: value.h_scrollbar,
            h_scrollbar_variant: value.h_scrollbar_variant,
            h_scrollbar_thumb: value.h_scrollbar_thumb,
            #[cfg(feature = "diff-view")]
            pin_scrollbar_focus: value.pin_scrollbar_focus_style,
            max_line_width: 0, // Will be computed in layout
            h_scroll_offset: 0,
            h_scroll_override: None,
            visual_lines_count: 0, // Will be computed in layout
            logical_lines_count,
            scroll_override: None,
            disabled: value.disabled,
            content_hash: 0,
            visual_cache: TextAreaVisualCache::default(),
            color_cache: TextAreaColorCache::default(),
            geometry: TextAreaGeometry::default(),
            disabled_style: value.disabled_style,
            read_only: value.read_only,
            newline_binding: value.newline_binding,
            tab_width: value.tab_width,
            insert_tab: value.insert_tab,
            tab_display_width: value.tab_display_width,
            on_change: value.on_change,
            on_edit: value.on_edit,
            on_editor_state_change: value.on_editor_state_change,
            on_scroll: value.on_scroll,
            on_scroll_to: value.on_scroll_to,
            on_click: value.on_click,
            on_key: value.on_key,
            key_interceptor: value.key_interceptor,
            clear_bindings: value.clear_bindings,
            vim_motions: value.vim_motions,
            vim_keymap: value.vim_keymap,
            vim_config: value.vim_config,
            vim_mode: TextAreaVimMode::Normal,
            vim_visual_line_caret: None,
            vim_search_feedback: None,
            vim_yank_feedback_range: None,
            on_vim_mode_change: value.on_vim_mode_change,
            on_image_paste: value.on_image_paste,
            on_text_paste: value.on_text_paste,
            images: value.images,
            on_images_change: value.on_images_change,
            image_mode: value.image_mode,
            image_placeholder: value.image_placeholder,
            image_placeholder_style: value.image_placeholder_style,
            image_placeholder_focus_style: value.image_placeholder_focus_style,
            image_placeholder_hover_style: value.image_placeholder_hover_style,
            focusable: value.focusable,
            tab_stop: value.tab_stop,
            on_focus: value.on_focus,
            on_blur: value.on_blur,
            gutter_lines: value.gutter_lines,
            gutter_col_width: value.gutter_col_width,
            gutter_gap: value.gutter_gap,
            peer_source_lines: value.peer_source_lines,
            #[cfg(feature = "diff-view")]
            split_wrap_sync: value.split_wrap_sync,
            #[cfg(feature = "diff-view")]
            split_wrap_side: value.split_wrap_side,
            #[cfg(feature = "diff-view")]
            diff_context_separator_click: value.diff_context_separator_click,
            split_wrap_padding_gutter_style: value.split_wrap_padding_gutter_style,
            split_wrap_padding_style: value.split_wrap_padding_style,
            copy_excluded_bytes: value.copy_excluded_bytes,
            clipboard_transform: value.clipboard_transform,
            selection_excluded_lines: value.selection_excluded_lines,
            multi_click_select: value.multi_click_select,
            triple_click_mode: value.triple_click_mode,
            sentinels: value.sentinels,
            on_sentinels_change: value.on_sentinels_change,
            on_sentinel_event: value.on_sentinel_event,
            on_sentinel_click: value.on_sentinel_click,
            decorations: value.decorations,
            virtual_texts: value.virtual_texts,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::style::Rect;
    use crate::widgets::TextArea;
    use crate::widgets::text_area::layout::{
        TextAreaVisualKeyArgs, TextAreaVisualLine, hash_text, make_text_area_visual_key,
    };

    use super::*;

    #[test]
    fn metrics_tolerates_visual_cache_offsets_inside_unicode_characters() {
        let value = "Please add new spinner variant - \"claude\" which is the same as Claude Code is using.\n\nIt has this flow of 1char of symbols cycling in order: \n·\n✢\n✶\n*\n✻\n✳";
        let dot_start = value
            .find('·')
            .expect("test fixture should contain middle dot");
        let inside_dot = dot_start + 1;
        assert!(!value.is_char_boundary(inside_dot));

        let mut node = TextAreaNode::from(TextArea::new(value).cursor(inside_dot));
        node.geometry.inner_w = 40;
        node.geometry.inner_h = 5;
        node.geometry.content_width = 40;
        node.geometry.viewport_height = 5;
        node.geometry.total_visual_lines = 1;

        let key = make_text_area_visual_key(
            hash_text(value),
            0,
            TextAreaVisualKeyArgs {
                inner_w: 40,
                wrap: true,
                line_numbers: false,
                min_line_number_width: 0,
                scrollbar: false,
                scrollbar_over_border: false,
                scrollbar_gap: 0,
                read_only: false,
                cursor: node.cursor,
                tab_stop: node.tab_display_width,
                sentinel_ph_width: 0,
                sentinel_count: 0,
                custom_sentinel_hash: 0,
                virtual_text_hash: 0,
                gutter_col_width: 0,
                gutter_gap: 0,
                #[cfg(feature = "diff-view")]
                split_wrap_pane_widths: None,
                #[cfg(feature = "diff-view")]
                split_wrap_scrollbar_cols: None,
                #[cfg(feature = "diff-view")]
                split_wrap_layout_pass: 0,
            },
        );
        node.visual_cache.insert_with_lines(
            key,
            node.geometry.clone(),
            vec![TextAreaVisualLine {
                line_num: 4,
                continuation: false,
                start: inside_dot,
                end: inside_dot,
                visual_start_col: 0,
                visual_end_col: 1,
                starts_with_virtual_text: false,
                ends_with_virtual_text: false,
            }],
            #[cfg(feature = "diff-view")]
            None,
        );

        let metrics = node.metrics(Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 5,
        });
        let cursor = metrics
            .editor_cursor
            .expect("clamped stale cache line should still produce cursor metrics");

        assert_eq!(cursor.byte_offset, dot_start);
        assert!(cursor.visible);
    }
}
