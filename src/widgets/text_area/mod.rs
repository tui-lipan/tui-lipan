mod color;
mod decorations;
mod layout;
mod node;
mod reconcile;
mod virtual_text;

#[cfg(feature = "syntax-syntect")]
mod color_syntect;
#[cfg(feature = "syntax-syntect")]
mod syntect_document_formatter;

pub(crate) use color::TextAreaColorCache;
pub use color::{TextAreaColorInput, TextAreaColorLines, TextAreaColorStrategy};
pub(crate) use decorations::{
    TEXT_AREA_LAYER_PRIORITY_CURRENT_SEARCH, TEXT_AREA_LAYER_PRIORITY_SEARCH,
    TEXT_AREA_LAYER_PRIORITY_SELECTION, TextAreaLayerKind, TextAreaRangeLayer,
    TextAreaStyledSegment, public_decoration_layers_for_visible_range, resolve_text_area_spans,
    segments_from_plain, segments_from_spans,
};
pub(crate) use layout::{
    TextAreaGeometry, TextAreaVisualCache, TextAreaVisualKeyArgs, TextAreaVisualLine,
    VirtualTextLayoutCtx, hash_peer_source_lines, layout_line_with_inline_virtual_text,
    make_text_area_visual_key, text_area_auto_height_for_width, text_area_cursor_reserve,
    text_area_pending_vim_search_row, text_area_total_gutter_width,
    text_area_visual_line_for_cursor,
};
pub use layout::{measure_text_area, measure_text_area_constrained};
pub use node::TextAreaNode;
pub use reconcile::reconcile_text_area;
pub(crate) use virtual_text::{
    eol_virtual_texts_for_visual_line, inline_virtual_insertions_for_line,
    inline_virtual_texts_for_visual_line, text_area_virtual_text_hash, virtual_text_content_width,
};

#[cfg(feature = "syntax-syntect")]
pub use color_syntect::{SyntectStrategy, apply_syntect_strategy_app_theme, language_from_path};
#[cfg(feature = "syntax-syntect")]
pub use syntect_document_formatter::SyntectDocumentFormatter;
mod metrics;
mod sentinel;
mod snapshot;
mod vim_config;

use std::collections::BTreeMap;
use std::hash::Hash;
use std::rc::Rc;
use std::sync::Arc;

use crate::animation::TransitionConfig;
use crate::app::TextAreaNewlineBinding;
use crate::callback::{Callback, KeyHandler};
use crate::clipboard::ImageContent;
use crate::core::element::{Element, ElementKind};
use crate::core::event::MouseEvent;
use crate::input::KeyBindings;
use crate::style::{
    BorderStyle, CaretShape, Color, LayoutConstraints, Length, Padding, ScrollbarConfig,
    ScrollbarVariant, Span, Style, StyleSlot,
};
use crate::text::edit::TextEditEvent;
use crate::text::editor::TextEditor;
use crate::utils::text::SentinelInfo;
use crate::widgets::scroll::{ScrollBehavior, ScrollEvent};

/// Public style decoration for byte ranges in a [`TextArea`].
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq)]
pub struct TextAreaDecoration {
    pub range: std::ops::Range<usize>,
    pub style: Style,
    /// Resolution order relative to other decorations and to selection/search.
    ///
    /// Higher priority wins **only for style attributes that two overlapping
    /// layers both set** — composition is per-attribute (`Style::patch`), not a
    /// full replacement. Raising the priority of a decoration that only sets a
    /// foreground color will not mask a lower layer's background or underline;
    /// those attributes survive because the higher layer never sets them. To
    /// hide an attribute, set it explicitly on the higher-priority decoration.
    pub priority: u16,
    pub kind: TextAreaDecorationKind,
}

/// Decoration rendering mode for [`TextAreaDecoration`].
#[non_exhaustive]
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextAreaDecorationKind {
    Range,
    WholeLine,
    Underline,
}

/// A sign/adornment rendered in a composable TextArea gutter column.
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq)]
pub struct TextAreaGutterSign {
    pub line: usize,
    pub spans: Vec<Span>,
}

#[allow(missing_docs)]
impl TextAreaGutterSign {
    pub fn new(line: usize, spans: impl Into<Vec<Span>>) -> Self {
        Self {
            line,
            spans: spans.into(),
        }
    }
}

/// One composable gutter column.
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq)]
pub struct TextAreaGutterColumn {
    kind: TextAreaGutterColumnKind,
    width: u16,
}

#[derive(Clone, Debug, PartialEq)]
enum TextAreaGutterColumnKind {
    LineNumbers(TextAreaLineNumberMode),
    Custom(Arc<Vec<Vec<Span>>>),
    Signs(Vec<TextAreaGutterSign>),
}

#[allow(missing_docs)]
impl TextAreaGutterColumn {
    pub fn line_numbers(mode: TextAreaLineNumberMode) -> Self {
        Self {
            kind: TextAreaGutterColumnKind::LineNumbers(mode),
            width: 0,
        }
    }

    pub fn custom(lines: Arc<Vec<Vec<Span>>>, width: u16) -> Self {
        Self {
            kind: TextAreaGutterColumnKind::Custom(lines),
            width,
        }
    }

    pub fn signs(signs: impl IntoIterator<Item = TextAreaGutterSign>) -> Self {
        let signs: Vec<_> = signs.into_iter().collect();
        let width = signs
            .iter()
            .flat_map(|s| s.spans.iter())
            .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()) as u16)
            .max()
            .unwrap_or(1)
            .max(1);
        Self {
            kind: TextAreaGutterColumnKind::Signs(signs),
            width,
        }
    }

    pub fn width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }
}

/// Composable TextArea gutter configuration.
#[allow(missing_docs)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextAreaGutter {
    columns: Vec<TextAreaGutterColumn>,
}

#[allow(missing_docs)]
impl TextAreaGutter {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn line_numbers(mut self, mode: TextAreaLineNumberMode) -> Self {
        self.columns.push(TextAreaGutterColumn::line_numbers(mode));
        self
    }
    pub fn signs(mut self, signs: impl IntoIterator<Item = TextAreaGutterSign>) -> Self {
        self.columns.push(TextAreaGutterColumn::signs(signs));
        self
    }
    pub fn column(mut self, column: TextAreaGutterColumn) -> Self {
        self.columns.push(column);
        self
    }
}

/// Reason-tagged editor state transition emitted by [`TextArea::on_editor_state_change`].
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq)]
pub struct TextAreaStateChangeEvent {
    pub reason: TextAreaStateChangeReason,
    pub value: Arc<str>,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub edit: Option<TextEditEvent>,
    pub vim_mode: Option<TextAreaVimMode>,
}

#[non_exhaustive]
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextAreaStateChangeReason {
    Edit,
    SelectionChange,
    CursorMove,
    Scroll,
    VimModeChange,
}

/// A multi-line text input.
#[derive(Clone)]
pub struct TextArea {
    pub(crate) value: Arc<str>,
    pub(crate) cursor: usize,         // byte index
    pub(crate) anchor: Option<usize>, // selection anchor byte index
    pub(crate) placeholder: Option<Arc<str>>,
    pub(crate) style: Style,
    pub(crate) hover_style: StyleSlot,
    pub(crate) focus_style: StyleSlot,
    pub(crate) focus_content_style: Style,
    pub(crate) hover_border_style: Option<BorderStyle>,
    pub(crate) caret_shape: CaretShape,
    pub(crate) caret_color: Option<Color>,
    pub(crate) selection_style: StyleSlot,
    pub(crate) unfocused_selection_style: StyleSlot,
    /// When true, render the active anchor/cursor range even while unfocused.
    pub(crate) show_selection_when_unfocused: bool,
    pub(crate) placeholder_style: Style,
    pub(crate) focus_placeholder_style: Style,
    pub(crate) line_numbers: bool,
    pub(crate) line_number_mode: TextAreaLineNumberMode,
    pub(crate) line_number_style: Style,
    pub(crate) min_line_number_width: u8,
    pub(crate) wrap: bool,
    pub(crate) color_strategy: Option<Rc<dyn TextAreaColorStrategy>>,
    pub(crate) language: Option<Arc<str>>,
    pub(crate) theme: Option<Arc<str>>,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) padding: Padding,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) scroll_offset: Option<usize>, // Line-based visual scroll offset
    /// Zero-based logical/source line to bring to the top of the viewport.
    pub(crate) scroll_to_line: Option<usize>,
    pub(crate) scroll_behavior: ScrollBehavior,
    pub(crate) scroll_wheel: bool,
    pub(crate) scroll_wheel_multiplier: Option<u16>,
    pub(crate) on_change: Option<Callback<TextAreaEvent>>,
    pub(crate) on_edit: Option<Callback<TextEditEvent>>,
    pub(crate) on_editor_state_change: Option<Callback<TextAreaStateChangeEvent>>,
    pub(crate) on_scroll: Option<Callback<ScrollEvent>>,
    pub(crate) on_scroll_to: Option<Callback<usize>>,
    pub(crate) on_click: Option<Callback<MouseEvent>>,
    pub(crate) on_key: Option<KeyHandler>,
    pub(crate) key_interceptor: Option<KeyHandler>,
    pub(crate) clear_bindings: Option<KeyBindings>,
    pub(crate) vim_motions: bool,
    pub(crate) vim_keymap: Option<TextAreaVimKeymap>,
    pub(crate) vim_config: TextAreaVimConfig,
    pub(crate) on_vim_mode_change: Option<Callback<TextAreaVimMode>>,
    pub(crate) on_image_paste: Option<Callback<ImageContent>>,
    pub(crate) on_text_paste: Option<Callback<TextAreaPasteEvent>>,
    /// Ordered list of images associated with this text area.
    /// In `Inline` mode: index `i` maps to the sentinel char `IMAGE_SENTINEL_BASE + i` in the value.
    /// In `Attachment` mode: displayed as chip labels above the text.
    pub(crate) images: Vec<ImageContent>,
    pub(crate) on_images_change: Option<Callback<Vec<ImageContent>>>,
    pub(crate) image_mode: TextAreaImageMode,
    pub(crate) image_placeholder: Arc<str>,
    pub(crate) image_placeholder_style: Style,
    pub(crate) image_placeholder_focus_style: Style,
    pub(crate) image_placeholder_hover_style: Style,
    pub(crate) disabled: bool,
    pub(crate) disabled_style: Style,
    pub(crate) read_only: bool,
    pub(crate) focusable: bool,
    pub(crate) newline_binding: Option<TextAreaNewlineBinding>,
    pub(crate) tab_width: u8,
    pub(crate) insert_tab: bool,
    /// Display width of a literal `\t` character. Tab advances to the next
    /// multiple of `tab_stop` from the logical line start. Set to 0 to keep
    /// the historical zero-width behavior.
    pub(crate) tab_stop: u8,
    /// Vertical scrollbar visibility.
    pub(crate) scrollbar: bool,
    pub(crate) scrollbar_config: ScrollbarConfig,
    pub(crate) h_scrollbar: bool,
    pub(crate) h_scrollbar_variant: ScrollbarVariant,
    pub(crate) h_scrollbar_thumb: Option<char>,
    #[cfg(feature = "diff-view")]
    pub(crate) pin_scrollbar_focus_style: bool,
    /// Per-logical-line custom gutter spans. When set, replaces the built-in
    /// `line_numbers` gutter. Indexed by logical line (0-based); continuation
    /// visual lines render an empty gutter.
    pub(crate) gutter_lines: Option<Arc<Vec<Vec<crate::style::Span>>>>,
    /// Width reserved for the custom gutter column. When > 0, overrides the
    /// computed `line_numbers` gutter width everywhere.
    pub(crate) gutter_col_width: u16,
    /// Fixed empty cells before the gutter / line numbers.
    pub(crate) gutter_gap: u16,
    pub(crate) gutter: Option<TextAreaGutter>,
    /// Peer logical source lines for split-wrap synchronization padding.
    pub(crate) peer_source_lines: Option<Arc<Vec<Arc<str>>>>,
    #[cfg(feature = "diff-view")]
    pub(crate) split_wrap_sync: Option<crate::widgets::diff_view::SharedSplitWrapSync>,
    #[cfg(feature = "diff-view")]
    pub(crate) split_wrap_side: Option<crate::widgets::diff_view::SplitPaneSide>,
    #[cfg(feature = "diff-view")]
    pub(crate) diff_context_separator_click:
        Option<crate::widgets::diff_view::DiffContextSeparatorClickConfig>,
    /// Style used for synthetic wrap-padding gutter rows inserted for peer sync.
    pub(crate) split_wrap_padding_gutter_style: Option<Style>,
    /// Style used for synthetic wrap-padding content rows inserted for peer sync.
    pub(crate) split_wrap_padding_style: Option<Style>,
    /// Byte ranges in `value` excluded from clipboard copy (sorted, non-overlapping).
    pub(crate) copy_excluded_bytes: Option<Arc<Vec<(usize, usize)>>>,
    /// Optional transform applied to selected text immediately before clipboard write.
    pub(crate) clipboard_transform: Option<TextAreaClipboardTransform>,
    /// 0-based logical line indices whose selection highlight (the newline space) is suppressed.
    pub(crate) selection_excluded_lines: Option<Arc<Vec<usize>>>,
    /// Enable word/line selection on double/triple click (default: `true`).
    pub(crate) multi_click_select: bool,
    /// Triple-click selection behavior.
    pub(crate) triple_click_mode: crate::widgets::TripleClickSelectionMode,
    /// Ordered list of custom inline sentinels.
    /// Index `i` maps to the sentinel character `SENTINEL_BASE + i` in the value.
    pub(crate) sentinels: Vec<TextAreaSentinel>,
    /// Callback invoked when the sentinels list changes (a sentinel was deleted).
    pub(crate) on_sentinels_change: Option<Callback<Vec<TextAreaSentinel>>>,
    pub(crate) on_sentinel_event: Option<Callback<Vec<SentinelEvent>>>,
    pub(crate) on_sentinel_click: Option<Callback<TextAreaSentinelClickEvent>>,
    pub(crate) decorations: Vec<TextAreaDecoration>,
    pub(crate) virtual_texts: Vec<TextAreaVirtualText>,
}

impl Default for TextArea {
    fn default() -> Self {
        Self {
            value: "".into(),
            cursor: 0,
            anchor: None,
            placeholder: None,
            style: Style::default(),
            hover_style: StyleSlot::Inherit,
            focus_style: StyleSlot::Inherit,
            focus_content_style: Style::default(),
            hover_border_style: None,
            caret_shape: CaretShape::default(),
            caret_color: None,
            selection_style: StyleSlot::Inherit,
            unfocused_selection_style: StyleSlot::Inherit,
            show_selection_when_unfocused: true,
            placeholder_style: Style::default(),
            focus_placeholder_style: Style::default(),
            line_numbers: false,
            line_number_mode: TextAreaLineNumberMode::default(),
            line_number_style: Style::default(),
            min_line_number_width: 0,
            wrap: true,
            color_strategy: None,
            language: None,
            theme: None,
            border: true,
            border_style: BorderStyle::Plain,
            padding: Padding::default(),
            width: Length::Flex(1),
            height: Length::Flex(1),
            scroll_offset: None,
            scroll_to_line: None,
            scroll_behavior: ScrollBehavior::Instant,
            scroll_wheel: true,
            scroll_wheel_multiplier: None,
            on_change: None,
            on_edit: None,
            on_editor_state_change: None,
            on_scroll: None,
            on_scroll_to: None,
            on_click: None,
            on_key: None,
            key_interceptor: None,
            clear_bindings: None,
            vim_motions: false,
            vim_keymap: None,
            vim_config: TextAreaVimConfig::default(),
            on_vim_mode_change: None,
            on_image_paste: None,
            on_text_paste: None,
            images: Vec::new(),
            on_images_change: None,
            image_mode: TextAreaImageMode::default(),
            image_placeholder: "[Image]".into(),
            image_placeholder_style: Style::default(),
            image_placeholder_focus_style: Style::default(),
            image_placeholder_hover_style: Style::default(),
            disabled: false,
            disabled_style: Style::default(),
            read_only: false,
            focusable: true,
            newline_binding: None,
            tab_width: 0,
            insert_tab: false,
            tab_stop: 8,
            scrollbar: true,
            scrollbar_config: ScrollbarConfig::default(),
            h_scrollbar: false,
            h_scrollbar_variant: ScrollbarVariant::default(),
            h_scrollbar_thumb: None,
            #[cfg(feature = "diff-view")]
            pin_scrollbar_focus_style: false,
            gutter_lines: None,
            gutter_col_width: 0,
            gutter_gap: 0,
            gutter: None,
            peer_source_lines: None,
            #[cfg(feature = "diff-view")]
            split_wrap_sync: None,
            #[cfg(feature = "diff-view")]
            split_wrap_side: None,
            #[cfg(feature = "diff-view")]
            diff_context_separator_click: None,
            split_wrap_padding_gutter_style: None,
            split_wrap_padding_style: None,
            copy_excluded_bytes: None,
            clipboard_transform: None,
            selection_excluded_lines: None,
            multi_click_select: true,
            triple_click_mode: crate::widgets::TripleClickSelectionMode::Line,
            sentinels: Vec::new(),
            on_sentinels_change: None,
            on_sentinel_event: None,
            on_sentinel_click: None,
            decorations: Vec::new(),
            virtual_texts: Vec::new(),
        }
    }
}

impl TextArea {
    /// Create a new text area.
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self {
            value: value.into(),
            ..Self::default()
        }
    }

    /// Create a new text area bound to a [`TextEditor`] state bundle.
    pub fn bound(state: &TextEditor) -> Self {
        Self::new("").bind(state)
    }

    /// Set the text content.
    pub fn value(mut self, value: impl Into<Arc<str>>) -> Self {
        self.value = value.into();
        self
    }

    /// Set placeholder text (shown when empty).
    pub fn placeholder(mut self, placeholder: impl Into<Arc<str>>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Set cursor position.
    pub fn cursor(mut self, cursor: usize) -> Self {
        self.cursor = cursor;
        self
    }

    /// Set selection anchor position (byte index).
    /// When set, text between anchor and cursor is selected.
    pub fn anchor(mut self, anchor: Option<usize>) -> Self {
        self.anchor = anchor;
        self
    }

    /// Bind the text area's value, cursor, and anchor from a [`TextEditor`] state bundle.
    pub fn bind(mut self, state: &TextEditor) -> Self {
        self.value = state.text().into();
        self.cursor = state.cursor();
        self.anchor = state.anchor();
        self
    }

    /// Show line numbers.
    pub fn line_numbers(mut self, show: bool) -> Self {
        self.line_numbers = show;
        self.gutter = None;
        self
    }

    /// Set line-number display mode for the built-in gutter.
    ///
    /// Use [`TextAreaLineNumberMode::Relative`] for Vim-style relative numbers:
    /// the cursor's logical line shows its absolute number, while lines above
    /// and below show their distance from the cursor line.
    pub fn line_number_mode(mut self, mode: TextAreaLineNumberMode) -> Self {
        self.line_number_mode = mode;
        self.gutter = None;
        self
    }

    /// Set minimum line number width (number of digits to reserve).
    pub fn min_line_number_width(mut self, width: u8) -> Self {
        self.min_line_number_width = width;
        self
    }

    /// Enable word wrapping.
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Set text coloring strategy.
    pub fn color_strategy(mut self, strategy: impl TextAreaColorStrategy + 'static) -> Self {
        self.color_strategy = Some(Rc::new(strategy));
        self
    }

    /// Set language identifier for coloring strategies.
    pub fn language(mut self, language: impl Into<Arc<str>>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Set language identifier by resolving from a file path's extension or name.
    ///
    /// Uses the default syntect syntax definitions. If no syntax matches the
    /// path, the language remains unset (plain text fallback). TypeScript/TSX
    /// paths fall back to JavaScript/JSX-compatible syntaxes when the default
    /// set does not provide exact grammars.
    #[cfg(feature = "syntax-syntect")]
    pub fn language_from_path(self, path: impl AsRef<std::path::Path>) -> Self {
        if let Some(lang) = crate::widgets::language_from_path(path) {
            self.language(lang)
        } else {
            self
        }
    }

    /// Set theme identifier for coloring strategies.
    pub fn theme(mut self, theme: impl Into<Arc<str>>) -> Self {
        self.theme = Some(theme.into());
        self
    }

    /// Enable syntect-based syntax highlighting with default strategy.
    #[cfg(feature = "syntax-syntect")]
    pub fn with_syntax(self, language: impl Into<Arc<str>>, theme: impl Into<Arc<str>>) -> Self {
        self.with_syntax_strategy(SyntectStrategy::default(), language, theme)
    }

    /// Enable syntect-based syntax highlighting with theme background colors.
    #[cfg(feature = "syntax-syntect")]
    pub fn with_syntax_bg(self, language: impl Into<Arc<str>>, theme: impl Into<Arc<str>>) -> Self {
        self.with_syntax_strategy(
            SyntectStrategy::default().use_background(true),
            language,
            theme,
        )
    }

    /// Enable syntect-based syntax highlighting with a custom theme string.
    #[cfg(feature = "syntax-syntect")]
    pub fn with_syntax_custom_theme(
        self,
        language: impl Into<Arc<str>>,
        theme_name: impl Into<Arc<str>>,
        tm_theme_xml: impl AsRef<str>,
    ) -> crate::Result<Self> {
        let theme_name = theme_name.into();
        let strategy = SyntectStrategy::default().custom_theme(theme_name.clone(), tm_theme_xml)?;
        Ok(self.with_syntax_strategy(strategy, language, theme_name))
    }

    /// Enable syntect-based syntax highlighting with custom theme bytes.
    #[cfg(feature = "syntax-syntect")]
    pub fn with_syntax_custom_theme_bytes(
        self,
        language: impl Into<Arc<str>>,
        theme_name: impl Into<Arc<str>>,
        bytes: impl AsRef<[u8]>,
    ) -> crate::Result<Self> {
        let theme_name = theme_name.into();
        let strategy = SyntectStrategy::default().custom_theme_bytes(theme_name.clone(), bytes)?;
        Ok(self.with_syntax_strategy(strategy, language, theme_name))
    }

    /// Enable syntect-based syntax highlighting with a custom theme file.
    #[cfg(feature = "syntax-syntect")]
    pub fn with_syntax_custom_theme_from_file(
        self,
        language: impl Into<Arc<str>>,
        theme_name: impl Into<Arc<str>>,
        path: impl AsRef<std::path::Path>,
    ) -> crate::Result<Self> {
        let theme_name = theme_name.into();
        let strategy =
            SyntectStrategy::default().custom_theme_from_file(theme_name.clone(), path)?;
        Ok(self.with_syntax_strategy(strategy, language, theme_name))
    }

    /// Enable syntect-based syntax highlighting with a custom strategy.
    #[cfg(feature = "syntax-syntect")]
    pub fn with_syntax_strategy(
        self,
        strategy: SyntectStrategy,
        language: impl Into<Arc<str>>,
        theme: impl Into<Arc<str>>,
    ) -> Self {
        self.color_strategy(strategy)
            .language(language)
            .theme(theme)
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set style when hovered.
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

    /// Set border style when hovered.
    pub fn hover_border_style(mut self, border_style: BorderStyle) -> Self {
        self.hover_border_style = Some(border_style);
        self
    }

    /// Set caret shape.
    pub fn caret_shape(mut self, shape: CaretShape) -> Self {
        self.caret_shape = shape;
        self
    }

    /// Set caret color (only used for block caret rendering).
    pub fn caret_color(mut self, color: Color) -> Self {
        self.caret_color = Some(color);
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

    /// Show the active selection range while the text area is unfocused.
    ///
    /// Enabled by default so keyboard/programmatic focus changes preserve the
    /// visible selection, matching [`DocumentView`](crate::widgets::DocumentView).
    /// Pass `false` to hide inactive selections.
    pub fn show_selection_when_unfocused(mut self, show: bool) -> Self {
        self.show_selection_when_unfocused = show;
        self
    }

    /// Set selection highlight style while unfocused.
    pub fn unfocused_selection_style(mut self, style: Style) -> Self {
        self.unfocused_selection_style = StyleSlot::Replace(style);
        self
    }

    /// Inherit unfocused selection style from the active theme.
    pub fn inherit_unfocused_selection_style(mut self) -> Self {
        self.unfocused_selection_style = StyleSlot::Inherit;
        self
    }

    /// Set unfocused selection style slot directly for composite forwarding.
    pub fn unfocused_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.unfocused_selection_style = slot;
        self
    }

    /// Set placeholder style.
    pub fn placeholder_style(mut self, style: Style) -> Self {
        self.placeholder_style = style;
        self
    }

    /// Set placeholder style when focused.
    pub fn focus_placeholder_style(mut self, style: Style) -> Self {
        self.focus_placeholder_style = style;
        self
    }

    /// Set line number style.
    pub fn line_number_style(mut self, style: Style) -> Self {
        self.line_number_style = style;
        self
    }

    /// Set border.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, style: BorderStyle) -> Self {
        self.border_style = style;
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

    /// Set on-change callback.
    pub fn on_change(mut self, cb: Callback<TextAreaEvent>) -> Self {
        self.on_change = Some(cb);
        self
    }

    /// Set on-edit callback.
    pub fn on_edit(mut self, cb: Callback<TextEditEvent>) -> Self {
        self.on_edit = Some(cb);
        self
    }

    /// Set a single reason-tagged editor-state callback.
    pub fn on_editor_state_change(mut self, cb: Callback<TextAreaStateChangeEvent>) -> Self {
        self.on_editor_state_change = Some(cb);
        self
    }

    /// Set on-click callback.
    pub fn on_click(mut self, cb: Callback<MouseEvent>) -> Self {
        self.on_click = Some(cb);
        self
    }

    /// Set on-key handler.
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
        self
    }

    /// Set a pre-insertion key interceptor.
    ///
    /// This handler runs after clipboard shortcuts but before newline insertion,
    /// tab expansion, or regular text editing. If it returns `true`, the key is
    /// consumed and neither the editor nor `on_key` will fire.
    pub fn key_interceptor(mut self, handler: KeyHandler) -> Self {
        self.key_interceptor = Some(handler);
        self
    }

    /// Set widget-level single-key bindings that clear the text area.
    ///
    /// Multi-step chord entries in `bindings` are ignored by the per-key text area handler.
    pub fn clear_bindings(mut self, bindings: KeyBindings) -> Self {
        self.clear_bindings = Some(bindings);
        self
    }

    /// Enable or disable TextArea-only Vim-style modal motions.
    ///
    /// Disabled by default. When enabled, the TextArea starts in normal mode.
    pub fn vim_motions(mut self, enabled: bool) -> Self {
        self.vim_motions = enabled;
        self
    }

    /// Set widget-local Vim key remaps.
    ///
    /// Remaps are only applied while Vim motions are enabled and the TextArea is
    /// not in insert mode. They translate matching keys to canonical Vim command
    /// characters before command dispatch.
    pub fn vim_keymap(mut self, keymap: TextAreaVimKeymap) -> Self {
        self.vim_keymap = Some(keymap);
        self
    }

    /// Set Vim-specific rendering options such as search feedback and
    /// current-line highlighting.
    pub fn vim_config(mut self, config: TextAreaVimConfig) -> Self {
        self.vim_config = config;
        self
    }

    /// Convenience builder for Vim current-line highlighting.
    ///
    /// Pass [`TextAreaVimCurrentLineHighlight::Full`] to include the gutter and
    /// line numbers, or [`TextAreaVimCurrentLineHighlight::Content`] to affect
    /// only the text content area.
    pub fn vim_current_line_highlight(mut self, mode: TextAreaVimCurrentLineHighlight) -> Self {
        self.vim_config.current_line_highlight = mode;
        self
    }

    /// Toggle full-row Vim current-line highlighting.
    pub fn highlight_vim_current_line(mut self, enabled: bool) -> Self {
        self.vim_config = self.vim_config.highlight_current_line(enabled);
        self
    }

    /// Observe internal Vim mode changes for status bars or mode-aware styling.
    pub fn on_vim_mode_change(mut self, cb: Callback<TextAreaVimMode>) -> Self {
        self.on_vim_mode_change = Some(cb);
        self
    }

    /// Set callback invoked when an image is pasted via `Ctrl+Shift+I`.
    pub fn on_image_paste(mut self, cb: Callback<ImageContent>) -> Self {
        self.on_image_paste = Some(cb);
        self
    }

    /// Set the ordered list of images associated with this text area.
    pub fn images(mut self, images: Vec<ImageContent>) -> Self {
        self.images = images;
        self
    }

    /// Set callback invoked when the images list changes (e.g. image pasted, sentinel deleted).
    pub fn on_images_change(mut self, cb: Callback<Vec<ImageContent>>) -> Self {
        self.on_images_change = Some(cb);
        self
    }

    /// Set the image display mode (`Inline` or `Attachment`).
    pub fn image_mode(mut self, mode: TextAreaImageMode) -> Self {
        self.image_mode = mode;
        self
    }

    /// Set the placeholder label rendered for each inline image sentinel (default: `"[Image]"`).
    pub fn image_placeholder(mut self, label: impl Into<Arc<str>>) -> Self {
        self.image_placeholder = label.into();
        self
    }

    /// Set the style for inline image placeholder labels.
    pub fn image_placeholder_style(mut self, style: Style) -> Self {
        self.image_placeholder_style = style;
        self
    }

    /// Set the style for inline image placeholder labels when the widget is focused.
    pub fn image_placeholder_focus_style(mut self, style: Style) -> Self {
        self.image_placeholder_focus_style = style;
        self
    }

    /// Set the hover style patched over inline image placeholder labels.
    pub fn image_placeholder_hover_style(mut self, style: Style) -> Self {
        self.image_placeholder_hover_style = style;
        self
    }

    /// Set disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set disabled style.
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }

    /// Set read-only mode. Allows mouse selection but blocks keyboard input.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Enable or disable word/line selection on double/triple click.
    ///
    /// When `false`, double and triple clicks behave as single clicks
    /// (no word or line selection). Drag-to-select remains unaffected.
    pub fn multi_click_select(mut self, enabled: bool) -> Self {
        self.multi_click_select = enabled;
        self
    }

    /// Set how triple-click expands selection.
    pub fn triple_click_mode(mut self, mode: crate::widgets::TripleClickSelectionMode) -> Self {
        self.triple_click_mode = mode;
        self
    }

    /// Set focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Override app-level newline key policy for this `TextArea` only.
    pub fn newline_binding(mut self, binding: TextAreaNewlineBinding) -> Self {
        self.newline_binding = Some(binding);
        self
    }

    /// When set to a non-zero value, pressing Tab inserts spaces up to the next tab stop
    /// (aligning to a multiple of `width` columns) instead of moving focus.
    pub fn tab_width(mut self, width: u8) -> Self {
        self.tab_width = width;
        self
    }

    /// When `true`, Tab inserts a tab character instead of moving focus.
    pub fn insert_tab(mut self, insert_tab: bool) -> Self {
        self.insert_tab = insert_tab;
        self
    }

    /// Display width of a literal `\t` character. `\t` advances to the next
    /// multiple of this value, measured from the logical line start.
    ///
    /// Defaults to `8` (terminal convention). Set to `0` to render `\t` as
    /// zero columns (rarely useful — mostly for parity with `unicode-width`).
    pub fn tab_stop(mut self, tab_stop: u8) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Set scroll offset (line index).
    pub fn scroll_offset(mut self, offset: usize) -> Self {
        self.scroll_offset = Some(offset);
        self
    }

    /// Scroll to a zero-based logical/source line.
    ///
    /// When wrapping is enabled, this resolves to the first visual row for the
    /// requested logical line. If the logical line is beyond the available text,
    /// reconciliation clamps to the last available visual row / maximum offset.
    pub fn scroll_to_line(mut self, line: usize) -> Self {
        self.scroll_to_line = Some(line);
        self
    }

    /// Set how explicit line scroll targets are applied.
    ///
    /// This affects [`Self::scroll_to_line`] only; controlled offsets and
    /// cursor auto-scroll remain immediate.
    pub fn scroll_behavior(mut self, behavior: ScrollBehavior) -> Self {
        self.scroll_behavior = behavior;
        self
    }

    /// Animate explicit line scroll targets with `transition`.
    pub fn scroll_transition(mut self, transition: TransitionConfig) -> Self {
        self.scroll_behavior = ScrollBehavior::smooth(transition);
        self
    }

    /// Enable mouse wheel scrolling.
    pub fn scroll_wheel(mut self, enabled: bool) -> Self {
        self.scroll_wheel = enabled;
        self
    }

    /// Override the app-wide mouse wheel step multiplier for this text area.
    pub fn scroll_wheel_multiplier(mut self, multiplier: u16) -> Self {
        self.scroll_wheel_multiplier = Some(multiplier.max(1));
        self
    }

    /// Set on-scroll callback.
    pub fn on_scroll(mut self, cb: Callback<ScrollEvent>) -> Self {
        self.on_scroll = Some(cb);
        self
    }

    /// Set on-scroll-to callback (for scrollbar dragging).
    pub fn on_scroll_to(mut self, cb: Callback<usize>) -> Self {
        self.on_scroll_to = Some(cb);
        self
    }

    /// Enable scrollbar.
    pub fn scrollbar(mut self, scrollbar: bool) -> Self {
        self.scrollbar = scrollbar;
        self
    }

    /// Set scrollbar configuration.
    pub fn scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.scrollbar_config = config;
        self
    }

    /// Set a custom gutter column.
    ///
    /// `lines` is indexed by logical line (0-based). Continuation visual lines
    /// (word-wrap overflow) show an empty gutter. `col_width` is the fixed
    /// column width reserved for the gutter; when > 0 it overrides the
    /// `line_numbers` gutter width everywhere.
    pub fn gutter_lines(
        mut self,
        lines: Arc<Vec<Vec<crate::style::Span>>>,
        col_width: u16,
    ) -> Self {
        self.gutter_lines = Some(lines);
        self.gutter_col_width = col_width;
        self.gutter = None;
        self
    }

    /// Set a composable gutter. Compatibility fields are lowered for the current renderer.
    pub fn gutter(mut self, gutter: TextAreaGutter) -> Self {
        self.apply_gutter(&gutter);
        self.gutter = Some(gutter);
        self
    }

    /// Reserve empty cells before the gutter / line numbers.
    pub fn gutter_inset(mut self, inset: u16) -> Self {
        self.gutter_gap = inset;
        self
    }

    fn apply_gutter(&mut self, gutter: &TextAreaGutter) {
        if gutter.columns.is_empty() {
            self.line_numbers = false;
            self.gutter_lines = None;
            self.gutter_col_width = 0;
            return;
        }
        if gutter.columns.len() == 1
            && let TextAreaGutterColumnKind::LineNumbers(mode) = &gutter.columns[0].kind
        {
            self.line_numbers = true;
            self.line_number_mode = *mode;
            self.gutter_lines = None;
            self.gutter_col_width = gutter.columns[0].width;
            return;
        }

        let logical_lines = self
            .value
            .as_bytes()
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
            + 1;
        let cursor = crate::utils::text::clamp_cursor(&self.value, self.cursor);
        let cursor_line = self.value[..cursor]
            .as_bytes()
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
            + 1;
        let mut rows = vec![Vec::new(); logical_lines.max(1)];
        let mut total_width = 0u16;
        for (col_idx, column) in gutter.columns.iter().enumerate() {
            if col_idx > 0 {
                for row in &mut rows {
                    row.push(Span::new(" "));
                }
                total_width = total_width.saturating_add(1);
            }
            let col_width = column_width(column, logical_lines, self.min_line_number_width);
            total_width = total_width.saturating_add(col_width);
            match &column.kind {
                TextAreaGutterColumnKind::LineNumbers(mode) => {
                    for (idx, row) in rows.iter_mut().enumerate() {
                        let line = idx + 1;
                        let n = match mode {
                            TextAreaLineNumberMode::Absolute => line,
                            TextAreaLineNumberMode::Relative => {
                                if line == cursor_line {
                                    line
                                } else {
                                    line.abs_diff(cursor_line)
                                }
                            }
                        };
                        row.push(
                            Span::new(format!(
                                "{n:>width$} │",
                                width = col_width.saturating_sub(2) as usize
                            ))
                            .style(self.line_number_style),
                        );
                    }
                }
                TextAreaGutterColumnKind::Custom(lines) => {
                    for (idx, row) in rows.iter_mut().enumerate() {
                        if let Some(spans) = lines.get(idx) {
                            row.extend(spans.iter().cloned());
                        }
                    }
                }
                TextAreaGutterColumnKind::Signs(signs) => {
                    let mut by_line: BTreeMap<usize, Vec<Span>> = BTreeMap::new();
                    for sign in signs {
                        by_line
                            .entry(sign.line)
                            .or_default()
                            .extend(sign.spans.iter().cloned());
                    }
                    for (idx, row) in rows.iter_mut().enumerate() {
                        if let Some(spans) = by_line.get(&idx) {
                            row.extend(spans.iter().cloned());
                        }
                    }
                }
            }
        }
        self.line_numbers = false;
        self.gutter_lines = Some(Arc::new(rows));
        self.gutter_col_width = total_width;
    }

    /// Set byte ranges in `value` to exclude from clipboard copy.
    pub fn copy_excluded_bytes(mut self, ranges: Arc<Vec<(usize, usize)>>) -> Self {
        self.copy_excluded_bytes = Some(ranges);
        self
    }

    /// Set an opt-in transform for selected text immediately before clipboard copy/cut.
    ///
    /// By default, TextArea copies the rendered selection unchanged.
    pub fn clipboard_transform(mut self, transform: TextAreaClipboardTransform) -> Self {
        self.clipboard_transform = Some(transform);
        self
    }

    /// Set 0-based logical line indices whose selection newline highlight is suppressed.
    pub fn selection_excluded_lines(mut self, lines: Arc<Vec<usize>>) -> Self {
        self.selection_excluded_lines = Some(lines);
        self
    }

    /// Enable horizontal scrollbar (only effective when wrap is disabled).
    pub fn h_scrollbar(mut self, h_scrollbar: bool) -> Self {
        self.h_scrollbar = h_scrollbar;
        self
    }

    /// Set horizontal scrollbar rendering style (integrated into border vs standalone row).
    pub fn h_scrollbar_variant(mut self, style: ScrollbarVariant) -> Self {
        self.h_scrollbar_variant = style;
        self
    }

    /// Set custom horizontal scrollbar thumb character (default: '█').
    pub fn h_scrollbar_thumb(mut self, ch: char) -> Self {
        self.h_scrollbar_thumb = Some(ch);
        self
    }

    /// Set the ordered list of custom inline sentinels.
    pub fn sentinels(mut self, sentinels: Vec<TextAreaSentinel>) -> Self {
        self.sentinels = sentinels;
        self
    }

    /// Set callback invoked when the sentinels list changes (a sentinel was deleted).
    pub fn on_sentinels_change(mut self, cb: Callback<Vec<TextAreaSentinel>>) -> Self {
        self.on_sentinels_change = Some(cb);
        self
    }

    /// Callback for sentinel lifecycle (e.g. user-deleted token with stable id).
    pub fn on_sentinel_event(mut self, cb: Callback<Vec<SentinelEvent>>) -> Self {
        self.on_sentinel_event = Some(cb);
        self
    }

    /// Callback invoked when an inline image or custom sentinel placeholder is clicked.
    pub fn on_sentinel_click(mut self, cb: Callback<TextAreaSentinelClickEvent>) -> Self {
        self.on_sentinel_click = Some(cb);
        self
    }

    /// Add a byte-range decoration.
    pub fn decoration(mut self, decoration: TextAreaDecoration) -> Self {
        self.decorations.push(decoration);
        self
    }

    /// Add byte-range decorations.
    pub fn decorations(
        mut self,
        decorations: impl IntoIterator<Item = TextAreaDecoration>,
    ) -> Self {
        self.decorations.extend(decorations);
        self
    }

    /// Add non-editable virtual text rendered inline or at end-of-line.
    pub fn virtual_text(mut self, virtual_text: TextAreaVirtualText) -> Self {
        self.virtual_texts.push(virtual_text);
        self
    }

    /// Add non-editable virtual text entries.
    pub fn virtual_texts(
        mut self,
        virtual_texts: impl IntoIterator<Item = TextAreaVirtualText>,
    ) -> Self {
        self.virtual_texts.extend(virtual_texts);
        self
    }

    /// Handle pasted text before the default insertion path.
    ///
    /// When set, the callback receives the pasted text plus the current cursor/selection and the
    /// text area does not insert the text itself.
    pub fn on_text_paste(mut self, cb: Callback<TextAreaPasteEvent>) -> Self {
        self.on_text_paste = Some(cb);
        self
    }

    /// Build sentinel info for width calculations on this text area.
    pub(crate) fn sentinel_info(&self) -> Option<SentinelInfo> {
        sentinel_info_for(
            self.image_mode,
            self.images.len(),
            &self.image_placeholder,
            &self.sentinels,
        )
    }
}

fn column_width(
    column: &TextAreaGutterColumn,
    logical_lines: usize,
    min_line_number_width: u8,
) -> u16 {
    use unicode_width::UnicodeWidthStr;
    let measured = match &column.kind {
        TextAreaGutterColumnKind::LineNumbers(_) => logical_lines
            .max(1)
            .to_string()
            .len()
            .max(min_line_number_width as usize)
            .saturating_add(2) as u16,
        TextAreaGutterColumnKind::Custom(lines) => lines
            .iter()
            .map(|row| {
                row.iter()
                    .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(0) as u16,
        TextAreaGutterColumnKind::Signs(signs) => signs
            .iter()
            .flat_map(|s| s.spans.iter())
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .max()
            .unwrap_or(1) as u16,
    };
    column.width.max(measured)
}

impl From<TextArea> for Element {
    fn from(value: TextArea) -> Self {
        let mut min_w = value.padding.horizontal();
        let mut min_h = 1u16.saturating_add(value.padding.vertical());
        if value.border {
            min_w = min_w.saturating_add(2);
            min_h = min_h.saturating_add(2);
        }
        let layout = LayoutConstraints::default()
            .min_width(Length::Px(min_w))
            .min_height(Length::Px(min_h));
        Element::new(ElementKind::TextArea(Box::new(value))).with_layout(layout)
    }
}

/// A text area change event.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextAreaEvent {
    /// Updated value.
    pub value: Arc<str>,
    /// Updated cursor position.
    pub cursor: usize,
    /// Selection anchor position (byte index), if any.
    pub anchor: Option<usize>,
}

/// A text paste event emitted before default text insertion.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextAreaPasteEvent {
    /// Pasted text after clipboard/router normalization and truncation.
    pub text: Arc<str>,
    /// Cursor position before paste insertion.
    pub cursor: usize,
    /// Selection anchor before paste insertion, if any.
    pub anchor: Option<usize>,
}

impl TextAreaEvent {
    /// Apply this event to a [`TextEditor`] state bundle.
    pub fn apply_to(&self, state: &mut TextEditor) {
        state.core.text = self.value.to_string();
        state.core.cursor = crate::utils::text::clamp_cursor(&state.core.text, self.cursor);
        state.core.anchor = self
            .anchor
            .map(|anchor| crate::utils::text::clamp_cursor(&state.core.text, anchor));
    }
}

impl crate::layout::hash::LayoutHash for TextArea {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.wrap.hash(hasher);
        self.line_numbers.hash(hasher);
        self.line_number_mode.hash(hasher);
        self.min_line_number_width.hash(hasher);
        self.border.hash(hasher);
        self.padding.hash(hasher);
        self.scrollbar.hash(hasher);
        self.scrollbar_config.gap.hash(hasher);
        self.gutter_col_width.hash(hasher);
        self.gutter_gap.hash(hasher);
        self.gutter.as_ref().map(|g| g.columns.len()).hash(hasher);
        if let Some(peer_lines) = &self.peer_source_lines {
            peer_lines.len().hash(hasher);
            for line in peer_lines.iter() {
                line.as_ref().hash(hasher);
            }
        } else {
            0usize.hash(hasher);
        }
        #[cfg(feature = "diff-view")]
        if let Some(sync) = &self.split_wrap_sync {
            self.split_wrap_side.hash(hasher);
            self.split_wrap_side
                .and_then(|side| crate::widgets::diff_view::split_wrap_pane_widths(sync, side))
                .hash(hasher);
            crate::widgets::diff_view::split_wrap_scrollbar_cols_pair(sync).hash(hasher);
            crate::widgets::diff_view::split_wrap_layout_pass(sync).hash(hasher);
        }
        self.read_only.hash(hasher);

        for s in &self.sentinels {
            s.label.hash(hasher);
            s.sentinel_id().hash(hasher);
        }
        self.images.len().hash(hasher);
        self.virtual_texts.hash(hasher);

        let needs_content =
            matches!(self.width, Length::Auto) || matches!(self.height, Length::Auto);
        if needs_content {
            self.value.hash(hasher);
        }
        Some(())
    }
}

pub use metrics::*;
pub use sentinel::*;
pub use snapshot::*;
pub use vim_config::*;

pub(crate) use vim_config::TextAreaVimSearchFeedback;

#[cfg(test)]
mod mod_tests;
