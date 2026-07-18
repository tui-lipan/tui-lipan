//! Draggable tab bar widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_draggable_tab_bar;
pub use node::DraggableTabBarNode;
pub use reconcile::reconcile_draggable_tab_bar;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, ElementKind};
use crate::core::event::MouseEvent;
use crate::style::{BorderStyle, Color, FileIconPalette, Length, Padding, Span, Style, StyleSlot};
use crate::utils::file_icons::FileIconOverride;
use crate::utils::file_icons::file_icon;
use crate::widgets::file_tree::FileIconStyle;
use crate::widgets::{Spinner, TabsEvent, Text};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Visual variant for [`DraggableTabBar`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum DraggableTabBarVariant {
    /// Classic segmented tabs with optional border.
    #[default]
    Bordered,
    /// One-line frame-like tabs with left accent markers.
    FrameLine,
}

/// Reorder behavior while dragging tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum DragReorderMode {
    /// Emit reorder events as soon as drag crosses a tab boundary.
    #[default]
    Live,
    /// Emit a single reorder event when mouse is released.
    OnDrop,
}

/// Overflow behavior for a [`DraggableTabBar`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum DraggableTabBarOverflow {
    /// Keep natural tab widths and enable horizontal scrolling when tabs overflow.
    #[default]
    Scroll,
    /// Shrink tab labels down to `min_tab_width` cells before enabling scrolling.
    ShrinkThenScroll {
        /// Minimum total tab width in terminal cells.
        min_tab_width: u16,
    },
}

/// Behavior kind for a [`DraggableTab`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum DraggableTabKind {
    /// A regular selectable, draggable tab.
    #[default]
    Tab,
    /// A pinned action item inside the tab strip, such as a `+` new-tab button.
    Action,
}

/// Event emitted when an action tab is clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DraggableTabActionEvent {
    /// Action tab index in the rendered tab list.
    pub index: usize,
}

/// Event emitted when a tab close button is clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DraggableTabCloseEvent {
    /// Closed tab index.
    pub index: usize,
}

/// Event emitted when a tab is reordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DraggableTabReorderEvent {
    /// Source tab index in the current order.
    pub from: usize,
    /// Destination tab index in the current order.
    pub to: usize,
}

/// Event emitted when a tab is transferred to another connected bar.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DraggableTabTransferEvent {
    /// Source bar identifier.
    pub from_bar: Arc<str>,
    /// Destination bar identifier.
    pub to_bar: Arc<str>,
    /// Source index in `from_bar` before transfer.
    pub from: usize,
    /// Destination index in `to_bar` after transfer.
    pub to: usize,
}

/// Which part of a tab was hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DraggableTabHitPart {
    /// Main tab body.
    Body,
    /// Close affordance (`x`) area.
    Close,
}

/// Hit-test result for a tab bar column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DraggableTabHit {
    /// Tab index.
    pub index: usize,
    /// Hit part.
    pub part: DraggableTabHitPart,
}

/// Inline content rendered before the tab label.
#[derive(Clone, Debug)]
pub(crate) enum TabLeadingContent {
    Spinner(TabLeadingSpinner),
    Text(Text),
}

#[derive(Clone, Debug)]
pub(crate) struct TabLeadingSpinner {
    pub spinner: Spinner,
    pub auto_frame: bool,
}

impl TabLeadingContent {
    pub(crate) fn from_element(element: Element) -> Self {
        match element.kind {
            ElementKind::Spinner(spinner) => Self::Spinner(TabLeadingSpinner {
                auto_frame: spinner.frame.is_none(),
                spinner,
            }),
            ElementKind::Text(text) => Self::Text(text),
            _ => panic!("DraggableTab::leading only supports Spinner or Text elements"),
        }
    }

    pub(crate) fn spinner_mut(&mut self) -> Option<&mut TabLeadingSpinner> {
        match self {
            Self::Spinner(spinner) => Some(spinner),
            Self::Text(_) => None,
        }
    }

    pub(crate) fn spinner_frame(&self) -> Option<usize> {
        match self {
            Self::Spinner(spinner) => spinner.spinner.frame,
            Self::Text(_) => None,
        }
    }

    pub(crate) fn has_spinner(&self) -> bool {
        matches!(self, Self::Spinner(_))
    }

    pub(crate) fn to_span(&self) -> Span {
        match self {
            Self::Spinner(spinner) => {
                let frames = spinner.spinner.spinner_style.frames();
                let frame_str = frames[spinner.spinner.frame.unwrap_or(0) % frames.len()];
                let mut span = Span::new(frame_str);
                span.style = spinner.spinner.style;
                span
            }
            Self::Text(text) => {
                let mut span = Span::new(text.plain_content());
                let span_style = text
                    .spans
                    .first()
                    .map(|span| span.style)
                    .unwrap_or_default();
                span.style = text.style.patch(span_style);
                span
            }
        }
    }
}

/// A single draggable tab item.
#[derive(Clone, Debug)]
pub struct DraggableTab {
    pub(crate) label: Arc<str>,
    pub(crate) kind: DraggableTabKind,
    pub(crate) style: Style,
    pub(crate) hover_style: Style,
    pub(crate) active_style: Style,
    pub(crate) accent_style: Style,
    pub(crate) active_accent_style: Style,
    pub(crate) closeable: bool,
    pub(crate) icon: Option<Span>,
    pub(crate) leading: Option<TabLeadingContent>,
    pub(crate) path: Option<Arc<str>>,
    pub(crate) right_badge: Option<Span>,
}

impl DraggableTab {
    /// Create a tab with label.
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            kind: DraggableTabKind::Tab,
            style: Style::default(),
            hover_style: Style::default(),
            active_style: Style::default(),
            accent_style: Style::default(),
            active_accent_style: Style::default(),
            closeable: false,
            icon: None,
            leading: None,
            path: None,
            right_badge: None,
        }
    }

    /// Create a pinned action tab, such as a `+` new-tab button.
    ///
    /// Action tabs emit [`DraggableTabBar::on_action`] instead of changing the
    /// active tab, and do not participate in drag reordering.
    pub fn action(label: impl Into<Arc<str>>) -> Self {
        Self::new(label).kind(DraggableTabKind::Action)
    }

    /// Set tab behavior kind.
    pub fn kind(mut self, kind: DraggableTabKind) -> Self {
        self.kind = kind;
        self
    }

    /// Set tab style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set this tab's hover style.
    ///
    /// This patches over [`DraggableTabBar::tab_hover_style`] for inactive tabs.
    /// Active tabs keep active styling and do not receive hover styling.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = style;
        self
    }

    /// Set this tab's active style.
    ///
    /// This patches over [`DraggableTabBar::active_style`] when this tab is selected.
    pub fn active_style(mut self, style: Style) -> Self {
        self.active_style = style;
        self
    }

    /// Set this tab's accent style for the `FrameLine` variant.
    ///
    /// This patches over [`DraggableTabBar::accent_style`] for this tab's accent marker.
    pub fn accent_style(mut self, style: Style) -> Self {
        self.accent_style = style;
        self
    }

    /// Set this tab's active accent style for the `FrameLine` variant.
    ///
    /// This patches over [`DraggableTabBar::active_accent_style`] when this tab is selected.
    pub fn active_accent_style(mut self, style: Style) -> Self {
        self.active_accent_style = style;
        self
    }

    /// Enable or disable close affordance for this tab.
    pub fn closeable(mut self, closeable: bool) -> Self {
        self.closeable = closeable;
        self
    }

    /// Set a custom icon rendered before the label.
    pub fn icon(mut self, icon: impl Into<Span>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Set inline content rendered before the label (replaces icon when set).
    ///
    /// Supports [`Spinner`] and [`Text`] elements. Spinner label/layout
    /// properties and text layout properties are ignored because the tab owns
    /// its own label and sizing.
    pub fn leading(mut self, leading: Element) -> Self {
        self.leading = Some(TabLeadingContent::from_element(leading));
        self
    }

    /// Set file path used for automatic file-icon resolution.
    pub fn path(mut self, path: impl Into<Arc<str>>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Set a generic right-side badge rendered after the label.
    pub fn right_badge(mut self, badge: impl Into<Span>) -> Self {
        self.right_badge = Some(badge.into());
        self
    }
}

impl From<&'static str> for DraggableTab {
    fn from(value: &'static str) -> Self {
        Self::new(value)
    }
}

impl From<String> for DraggableTab {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<Arc<str>> for DraggableTab {
    fn from(value: Arc<str>) -> Self {
        Self::new(value)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TabMetrics {
    pub width: usize,
    pub close_start: Option<usize>,
    pub close_end: Option<usize>,
    pub label_width: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum OverflowControlSide {
    Left,
    Right,
}

#[derive(Clone, Debug)]
pub(crate) struct OverflowControl {
    pub start: usize,
    pub end: usize,
    pub label: Arc<str>,
}

#[derive(Clone, Debug)]
pub(crate) struct VisibleTab {
    pub index: usize,
    pub start: usize,
    pub end: usize,
    pub metrics: TabMetrics,
    pub clip_left: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct TabViewportLayout {
    pub offset: usize,
    pub visible_tabs: Vec<VisibleTab>,
    pub hidden_left: usize,
    pub hidden_right: usize,
    pub content_start: usize,
    pub content_width: usize,
    pub left_control: Option<OverflowControl>,
    pub right_control: Option<OverflowControl>,
}

pub(crate) const TAB_SCROLL_STEP_CHARS: usize = 12;
pub(crate) const TAB_SCROLL_BUTTON_STEP_CHARS: usize = TAB_SCROLL_STEP_CHARS * 2;

pub(crate) struct TabDisplayOptions<'a> {
    pub variant: DraggableTabBarVariant,
    pub divider: char,
    pub accent_symbol: char,
    pub close_symbol: &'a str,
    pub show_close_buttons: bool,
    pub tab_max_width: Option<u16>,
    pub overflow: DraggableTabBarOverflow,
    pub show_file_icons: bool,
    pub file_icon_style: FileIconStyle,
    pub file_icon_palette: &'a FileIconPalette,
    pub file_icon_overrides: &'a HashMap<Arc<str>, FileIconOverride>,
}

pub(crate) struct TabViewportOptions {
    pub scroll_offset: usize,
    pub viewport_width: usize,
    pub show_overflow_controls: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DraggableTabHitTarget {
    Tab(DraggableTabHit),
    Overflow(OverflowControlSide),
}

/// A draggable tab bar suitable for editor-like UIs.
#[derive(Clone)]
pub struct DraggableTabBar {
    pub(crate) tabs: Arc<[DraggableTab]>,
    pub(crate) active: usize,
    pub(crate) style: Style,
    pub(crate) focus_style: StyleSlot,
    pub(crate) hover_style: StyleSlot,
    pub(crate) tab_hover_style: StyleSlot,
    pub(crate) active_style: StyleSlot,
    pub(crate) close_style: Style,
    pub(crate) close_hover_style: Style,
    pub(crate) divider: char,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) padding: Padding,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) variant: DraggableTabBarVariant,
    pub(crate) accent_symbol: char,
    pub(crate) active_accent_symbol: char,
    pub(crate) accent_style: Style,
    pub(crate) active_accent_style: Style,
    pub(crate) close_symbol: Arc<str>,
    pub(crate) show_close_buttons: bool,
    pub(crate) close_on_hover_only: bool,
    pub(crate) tab_max_width: Option<u16>,
    pub(crate) overflow: DraggableTabBarOverflow,
    pub(crate) scroll_wheel: bool,
    pub(crate) show_overflow_controls: bool,
    pub(crate) overflow_style: Style,
    pub(crate) overflow_hover_style: Style,
    pub(crate) scroll_offset: usize,
    pub(crate) show_file_icons: bool,
    pub(crate) file_icon_style: FileIconStyle,
    pub(crate) file_icon_palette: FileIconPalette,
    pub(crate) file_icon_overrides: HashMap<Arc<str>, FileIconOverride>,
    pub(crate) bar_id: Option<Arc<str>>,
    pub(crate) drag_group: Option<Arc<str>>,
    pub(crate) draggable: bool,
    pub(crate) drag_preview: bool,
    pub(crate) reorder_mode: DragReorderMode,
    pub(crate) drag_threshold: u16,
    pub(crate) on_change: Option<Callback<TabsEvent>>,
    pub(crate) on_action: Option<Callback<DraggableTabActionEvent>>,
    pub(crate) on_close: Option<Callback<DraggableTabCloseEvent>>,
    pub(crate) on_reorder: Option<Callback<DraggableTabReorderEvent>>,
    pub(crate) on_transfer: Option<Callback<DraggableTabTransferEvent>>,
    pub(crate) on_click: Option<Callback<MouseEvent>>,
    pub(crate) on_key: Option<KeyHandler>,
    pub(crate) disabled: bool,
    pub(crate) disabled_style: Style,
    pub(crate) focusable: bool,
    pub(crate) tab_stop: bool,
    pub(crate) on_focus: Option<Callback<()>>,
    pub(crate) on_blur: Option<Callback<()>>,
}

impl Default for DraggableTabBar {
    fn default() -> Self {
        Self {
            tabs: Arc::new([]),
            active: 0,
            style: Style::default(),
            focus_style: StyleSlot::Inherit,
            hover_style: StyleSlot::Inherit,
            tab_hover_style: StyleSlot::Inherit,
            active_style: StyleSlot::Inherit,
            close_style: Style::default(),
            close_hover_style: Style::default(),
            divider: '│',
            border: false,
            border_style: BorderStyle::Plain,
            padding: Padding::default(),
            width: Length::Flex(1),
            height: Length::Auto,
            variant: DraggableTabBarVariant::Bordered,
            accent_symbol: '▏',
            active_accent_symbol: '▎',
            accent_style: Style::default(),
            active_accent_style: Style::default(),
            close_symbol: Arc::from(""),
            show_close_buttons: true,
            close_on_hover_only: false,
            tab_max_width: None,
            overflow: DraggableTabBarOverflow::Scroll,
            scroll_wheel: true,
            show_overflow_controls: true,
            overflow_style: Style::default(),
            overflow_hover_style: Style::default(),
            scroll_offset: 0,
            show_file_icons: false,
            file_icon_style: FileIconStyle::NerdFont,
            file_icon_palette: FileIconPalette::default(),
            file_icon_overrides: HashMap::new(),
            bar_id: None,
            drag_group: None,
            draggable: true,
            drag_preview: true,
            reorder_mode: DragReorderMode::Live,
            drag_threshold: 1,
            on_change: None,
            on_action: None,
            on_close: None,
            on_reorder: None,
            on_transfer: None,
            on_click: None,
            on_key: None,
            disabled: false,
            disabled_style: Style::default(),
            focusable: false,
            tab_stop: true,
            on_focus: None,
            on_blur: None,
        }
    }
}

impl DraggableTabBar {
    /// Create an empty draggable tab bar.
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn display_options(&self) -> TabDisplayOptions<'_> {
        TabDisplayOptions {
            variant: self.variant,
            divider: self.divider,
            accent_symbol: self.accent_symbol,
            close_symbol: &self.close_symbol,
            show_close_buttons: self.show_close_buttons,
            tab_max_width: self.tab_max_width,
            overflow: self.overflow,
            show_file_icons: self.show_file_icons,
            file_icon_style: self.file_icon_style,
            file_icon_palette: &self.file_icon_palette,
            file_icon_overrides: &self.file_icon_overrides,
        }
    }

    /// Replace tabs.
    pub fn tabs<I>(mut self, tabs: I) -> Self
    where
        I: IntoIterator<Item = DraggableTab>,
    {
        self.tabs = tabs.into_iter().collect::<Vec<_>>().into();
        self
    }

    /// Add one tab.
    pub fn tab(mut self, tab: impl Into<DraggableTab>) -> Self {
        let mut tabs = self.tabs.to_vec();
        tabs.push(tab.into());
        self.tabs = tabs.into();
        self
    }

    /// Set active tab index.
    pub fn active(mut self, active: usize) -> Self {
        self.active = active;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set focus style for the whole widget.
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

    /// Set hover style for the whole widget.
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

    /// Set style for hovered tab.
    pub fn tab_hover_style(mut self, style: Style) -> Self {
        self.tab_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's tab hover style with additional fields.
    pub fn extend_tab_hover_style(mut self, style: Style) -> Self {
        self.tab_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit tab hover style from the active theme.
    pub fn inherit_tab_hover_style(mut self) -> Self {
        self.tab_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set active tab style.
    pub fn active_style(mut self, style: Style) -> Self {
        self.active_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's active-tab style with additional fields.
    pub fn extend_active_style(mut self, style: Style) -> Self {
        self.active_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit active-tab style from the active theme.
    pub fn inherit_active_style(mut self) -> Self {
        self.active_style = StyleSlot::Inherit;
        self
    }

    /// Set close symbol style.
    pub fn close_style(mut self, style: Style) -> Self {
        self.close_style = style;
        self
    }

    /// Set close symbol hover style.
    pub fn close_hover_style(mut self, style: Style) -> Self {
        self.close_hover_style = style;
        self
    }

    /// Set divider character for bordered variant.
    pub fn divider(mut self, ch: char) -> Self {
        self.divider = ch;
        self
    }

    /// Draw outer border.
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

    /// Set visual variant.
    pub fn variant(mut self, variant: DraggableTabBarVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set left accent symbol for frame-line variant.
    pub fn accent_symbol(mut self, symbol: char) -> Self {
        self.accent_symbol = symbol;
        self
    }

    /// Set active tab accent symbol for frame-line variant.
    pub fn active_accent_symbol(mut self, symbol: char) -> Self {
        self.active_accent_symbol = symbol;
        self
    }

    /// Set inactive accent style for frame-line variant.
    pub fn accent_style(mut self, style: Style) -> Self {
        self.accent_style = style;
        self
    }

    /// Set active accent style for frame-line variant.
    pub fn active_accent_style(mut self, style: Style) -> Self {
        self.active_accent_style = style;
        self
    }

    /// Set close symbol.
    pub fn close_symbol(mut self, symbol: impl Into<Arc<str>>) -> Self {
        self.close_symbol = symbol.into();
        self
    }

    /// Toggle rendering of close buttons for closeable tabs.
    pub fn show_close_buttons(mut self, show: bool) -> Self {
        self.show_close_buttons = show;
        self
    }

    /// Show close symbols only while the tab is hovered.
    ///
    /// Layout width remains stable (close slot is reserved) to avoid jitter.
    pub fn close_on_hover_only(mut self, only_on_hover: bool) -> Self {
        self.close_on_hover_only = only_on_hover;
        self
    }

    /// Clamp per-tab label width, truncating with right-side ellipsis.
    pub fn tab_max_width(mut self, width: Option<u16>) -> Self {
        self.tab_max_width = width;
        self
    }

    /// Set tab overflow behavior.
    pub fn overflow(mut self, overflow: DraggableTabBarOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Enable mouse wheel horizontal scrolling.
    pub fn scroll_wheel(mut self, enabled: bool) -> Self {
        self.scroll_wheel = enabled;
        self
    }

    /// Show overflow controls when tabs are clipped horizontally.
    pub fn show_overflow_controls(mut self, show: bool) -> Self {
        self.show_overflow_controls = show;
        self
    }

    /// Set style for overflow controls.
    pub fn overflow_style(mut self, style: Style) -> Self {
        self.overflow_style = style;
        self
    }

    /// Set hover style for overflow controls.
    pub fn overflow_hover_style(mut self, style: Style) -> Self {
        self.overflow_hover_style = style;
        self
    }

    /// Set initial horizontal tab scroll offset (tab index).
    pub fn scroll_offset(mut self, offset: usize) -> Self {
        self.scroll_offset = offset;
        self
    }

    /// Enable automatic file icons before tab titles.
    pub fn show_file_icons(mut self, show: bool) -> Self {
        self.show_file_icons = show;
        self
    }

    /// Set file icon style used by automatic tab icons.
    pub fn file_icon_style(mut self, style: FileIconStyle) -> Self {
        self.file_icon_style = style;
        self
    }

    /// Set file icon palette used by automatic tab icons.
    pub fn file_icon_palette(mut self, palette: FileIconPalette) -> Self {
        self.file_icon_palette = palette;
        self
    }

    /// Add file icon override by filename or extension.
    pub fn file_icon_override(
        mut self,
        pattern: impl Into<Arc<str>>,
        icon: impl Into<Arc<str>>,
        color: Option<Color>,
    ) -> Self {
        self.file_icon_overrides.insert(
            pattern.into(),
            FileIconOverride {
                icon: icon.into(),
                color,
            },
        );
        self
    }

    /// Set a stable identifier for this tab bar.
    ///
    /// Required for cross-bar tab transfers.
    pub fn bar_id(mut self, id: impl Into<Arc<str>>) -> Self {
        self.bar_id = Some(id.into());
        self
    }

    /// Set drag group for cross-bar transfers.
    ///
    /// Tabs can transfer only between bars with the same group.
    pub fn drag_group(mut self, group: impl Into<Arc<str>>) -> Self {
        self.drag_group = Some(group.into());
        self
    }

    /// Toggle drag reordering.
    pub fn draggable(mut self, draggable: bool) -> Self {
        self.draggable = draggable;
        self
    }

    /// Show a floating label near the pointer while dragging a tab (default: `true`).
    pub fn drag_preview(mut self, enabled: bool) -> Self {
        self.drag_preview = enabled;
        self
    }

    /// Set drag reorder mode.
    pub fn reorder_mode(mut self, mode: DragReorderMode) -> Self {
        self.reorder_mode = mode;
        self
    }

    /// Set drag start threshold in columns.
    pub fn drag_threshold(mut self, threshold: u16) -> Self {
        self.drag_threshold = threshold;
        self
    }

    /// Callback fired when active tab changes.
    pub fn on_change(mut self, cb: Callback<TabsEvent>) -> Self {
        self.on_change = Some(cb);
        self
    }

    /// Callback fired when an action tab is clicked.
    pub fn on_action(mut self, cb: Callback<DraggableTabActionEvent>) -> Self {
        self.on_action = Some(cb);
        self
    }

    /// Callback fired when a close button is clicked.
    pub fn on_close(mut self, cb: Callback<DraggableTabCloseEvent>) -> Self {
        self.on_close = Some(cb);
        self
    }

    /// Callback fired when tab order changes.
    pub fn on_reorder(mut self, cb: Callback<DraggableTabReorderEvent>) -> Self {
        self.on_reorder = Some(cb);
        self
    }

    /// Callback fired when a tab is moved between connected bars.
    pub fn on_transfer(mut self, cb: Callback<DraggableTabTransferEvent>) -> Self {
        self.on_transfer = Some(cb);
        self
    }

    /// Set on-click handler.
    pub fn on_click(mut self, cb: Callback<MouseEvent>) -> Self {
        self.on_click = Some(cb);
        self
    }

    /// Set on-key handler.
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
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

    /// Control whether node is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Control whether the node participates in tab traversal.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the node gains focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the node loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.on_blur = Some(cb);
        self
    }

    #[cfg(test)]
    pub(crate) fn content_width(
        tabs: &[DraggableTab],
        variant: DraggableTabBarVariant,
        divider: char,
        accent_symbol: char,
        close_symbol: &str,
        show_close_buttons: bool,
    ) -> usize {
        Self::content_width_with_options(
            tabs,
            &TabDisplayOptions {
                variant,
                divider,
                accent_symbol,
                close_symbol,
                show_close_buttons,
                tab_max_width: None,
                overflow: DraggableTabBarOverflow::Scroll,
                show_file_icons: false,
                file_icon_style: FileIconStyle::NerdFont,
                file_icon_palette: &FileIconPalette::default(),
                file_icon_overrides: &HashMap::new(),
            },
        )
    }

    pub(crate) fn content_width_with_options(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
    ) -> usize {
        let mut width = 0usize;
        for (i, tab) in tabs.iter().enumerate() {
            width = width.saturating_add(tab_metrics_with_options(tab, opts).width);
            if i + 1 < tabs.len() {
                width = width.saturating_add(separator_width(opts.variant, opts.divider));
            }
        }
        width
    }

    pub(crate) fn content_width_for_viewport(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        viewport_width: usize,
    ) -> usize {
        let metrics = tab_metrics_for_viewport(tabs, opts, Some(viewport_width));
        total_width_for_metrics(&metrics, opts)
    }

    #[cfg(test)]
    pub(crate) fn hit_at_col(
        tabs: &[DraggableTab],
        variant: DraggableTabBarVariant,
        divider: char,
        accent_symbol: char,
        close_symbol: &str,
        show_close_buttons: bool,
        col: usize,
    ) -> Option<DraggableTabHit> {
        Self::hit_at_col_with_options(
            tabs,
            &TabDisplayOptions {
                variant,
                divider,
                accent_symbol,
                close_symbol,
                show_close_buttons,
                tab_max_width: None,
                overflow: DraggableTabBarOverflow::Scroll,
                show_file_icons: false,
                file_icon_style: FileIconStyle::NerdFont,
                file_icon_palette: &FileIconPalette::default(),
                file_icon_overrides: &HashMap::new(),
            },
            col,
        )
    }

    #[cfg(test)]
    pub(crate) fn hit_at_col_with_options(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        col: usize,
    ) -> Option<DraggableTabHit> {
        let mut x = 0usize;
        for (i, tab) in tabs.iter().enumerate() {
            let metrics = tab_metrics_with_options(tab, opts);
            let end = x.saturating_add(metrics.width);
            if col < end {
                let part = if let (Some(close_start), Some(close_end)) =
                    (metrics.close_start, metrics.close_end)
                {
                    let c_start = x.saturating_add(close_start);
                    let c_end = x.saturating_add(close_end);
                    if col >= c_start && col < c_end {
                        DraggableTabHitPart::Close
                    } else {
                        DraggableTabHitPart::Body
                    }
                } else {
                    DraggableTabHitPart::Body
                };
                return Some(DraggableTabHit { index: i, part });
            }
            x = end;

            if i + 1 < tabs.len() {
                let sep_w = separator_width(opts.variant, opts.divider);
                if col < x.saturating_add(sep_w) {
                    return match opts.variant {
                        DraggableTabBarVariant::Bordered => None,
                        DraggableTabBarVariant::FrameLine => Some(DraggableTabHit {
                            index: i,
                            part: DraggableTabHitPart::Body,
                        }),
                    };
                }
                x = x.saturating_add(sep_w);
            }
        }

        None
    }

    #[cfg(test)]
    pub(crate) fn reorder_index_at_col(
        tabs: &[DraggableTab],
        variant: DraggableTabBarVariant,
        divider: char,
        accent_symbol: char,
        close_symbol: &str,
        show_close_buttons: bool,
        col: usize,
    ) -> Option<usize> {
        Self::reorder_index_at_col_with_options(
            tabs,
            &TabDisplayOptions {
                variant,
                divider,
                accent_symbol,
                close_symbol,
                show_close_buttons,
                tab_max_width: None,
                overflow: DraggableTabBarOverflow::Scroll,
                show_file_icons: false,
                file_icon_style: FileIconStyle::NerdFont,
                file_icon_palette: &FileIconPalette::default(),
                file_icon_overrides: &HashMap::new(),
            },
            col,
        )
    }

    #[cfg(test)]
    pub(crate) fn reorder_index_at_col_with_options(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        col: usize,
    ) -> Option<usize> {
        let metrics = tab_metrics_for_viewport(tabs, opts, None);
        Self::reorder_index_at_col_with_metrics(tabs, opts, &metrics, col)
    }

    fn reorder_index_at_col_with_metrics(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        metrics: &[TabMetrics],
        col: usize,
    ) -> Option<usize> {
        let mut x = 0usize;
        let len = tabs.len();
        for (i, metrics) in metrics.iter().enumerate() {
            let end = x.saturating_add(metrics.width);
            if col < end {
                return Some(i);
            }
            x = end;

            if i + 1 < len {
                let sep_w = separator_width(opts.variant, opts.divider);
                if col < x.saturating_add(sep_w) {
                    return Some((i + 1).min(len.saturating_sub(1)));
                }
                x = x.saturating_add(sep_w);
            }
        }
        None
    }

    #[cfg(test)]
    pub(crate) fn adjacent_reorder_target(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        current_index: usize,
        col: usize,
    ) -> Option<usize> {
        Self::adjacent_reorder_target_with_options(tabs, opts, current_index, col)
    }

    #[cfg(test)]
    pub(crate) fn adjacent_reorder_target_with_options(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        current_index: usize,
        col: usize,
    ) -> Option<usize> {
        let metrics = tab_metrics_for_viewport(tabs, opts, None);
        Self::adjacent_reorder_target_with_metrics(tabs, opts, &metrics, current_index, col)
    }

    fn adjacent_reorder_target_with_metrics(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        metrics: &[TabMetrics],
        current_index: usize,
        col: usize,
    ) -> Option<usize> {
        if tabs.is_empty() || current_index >= tabs.len() {
            return None;
        }

        if !tabs.get(current_index).is_some_and(is_reorderable_tab) {
            return None;
        }

        let mut starts = Vec::with_capacity(tabs.len());
        let mut widths = Vec::with_capacity(tabs.len());
        let mut reorder_indices = Vec::new();
        let mut x = 0usize;
        for (i, (tab, metrics)) in tabs.iter().zip(metrics).enumerate() {
            starts.push(x);
            widths.push(metrics.width);
            if is_reorderable_tab(tab) {
                reorder_indices.push(i);
            }
            x = x.saturating_add(metrics.width);
            if i + 1 < tabs.len() {
                x = x.saturating_add(separator_width(opts.variant, opts.divider));
            }
        }

        let mut position = reorder_indices
            .iter()
            .position(|&index| index == current_index)?;
        let mut run_start = position;
        while run_start > 0 && reorder_indices[run_start - 1] + 1 == reorder_indices[run_start] {
            run_start -= 1;
        }
        let mut run_end = position;
        while run_end + 1 < reorder_indices.len()
            && reorder_indices[run_end] + 1 == reorder_indices[run_end + 1]
        {
            run_end += 1;
        }

        let midpoint = |idx: usize| -> usize { starts[idx].saturating_add(widths[idx] / 2) };

        while position < run_end && col >= midpoint(reorder_indices[position + 1]) {
            position += 1;
        }

        while position > run_start && col < midpoint(reorder_indices[position - 1]) {
            position -= 1;
        }

        let target = reorder_indices[position];
        (target != current_index).then_some(target)
    }

    pub(crate) fn viewport_layout(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        vp: &TabViewportOptions,
    ) -> TabViewportLayout {
        let len = tabs.len();
        if len == 0 || vp.viewport_width == 0 {
            return TabViewportLayout {
                offset: 0,
                visible_tabs: Vec::new(),
                hidden_left: 0,
                hidden_right: 0,
                content_start: 0,
                content_width: 0,
                left_control: None,
                right_control: None,
            };
        }

        let (runs, total_width) = tab_runs_for_viewport(tabs, opts, Some(vp.viewport_width));

        let requested_offset = vp.scroll_offset;
        let mut offset = vp.scroll_offset;
        let mut align_right = false;
        let mut left_width = 0usize;
        let mut right_width = 0usize;
        let mut visible_tabs = Vec::new();
        let mut hidden_left = 0usize;
        let mut hidden_right = 0usize;

        for _ in 0..8 {
            visible_tabs.clear();
            let mut available = vp.viewport_width.saturating_sub(left_width + right_width);
            if available == 0 {
                if right_width > 0 {
                    right_width = 0;
                    available = vp.viewport_width.saturating_sub(left_width);
                } else if left_width > 0 {
                    left_width = 0;
                    available = vp.viewport_width;
                }
            }

            let max_scroll = total_width.saturating_sub(available);
            if align_right || requested_offset >= max_scroll {
                align_right = true;
                offset = max_scroll;
            } else {
                offset = requested_offset.min(max_scroll);
            }
            let view_start = offset;
            let view_end = view_start.saturating_add(available);

            hidden_left = 0;
            hidden_right = 0;
            for (idx, (start, end, metrics)) in runs.iter().enumerate() {
                if *end <= view_start {
                    hidden_left = idx.saturating_add(1);
                    continue;
                }
                if *start >= view_end {
                    hidden_right = len.saturating_sub(idx);
                    break;
                }

                let visible_start = (*start).max(view_start);
                let visible_end = (*end).min(view_end);
                if visible_start >= visible_end {
                    continue;
                }

                if visible_start > *start {
                    hidden_left = idx.saturating_add(1);
                }

                visible_tabs.push(VisibleTab {
                    index: idx,
                    start: visible_start.saturating_sub(view_start),
                    end: visible_end.saturating_sub(view_start),
                    metrics: *metrics,
                    clip_left: visible_start.saturating_sub(*start),
                });

                if visible_end < *end {
                    hidden_right = len.saturating_sub(idx);
                    break;
                }
            }

            if visible_tabs.is_empty() && offset > 0 {
                offset = offset.saturating_sub(1);
                continue;
            }

            if hidden_right == 0 {
                hidden_right = if let Some(last) = visible_tabs.last() {
                    len.saturating_sub(last.index + 1)
                } else {
                    len.saturating_sub(offset)
                };
            }

            let next_left = if vp.show_overflow_controls && hidden_left > 0 {
                overflow_control_width(OverflowControlSide::Left, hidden_left)
            } else {
                0
            };
            let next_right = if vp.show_overflow_controls && hidden_right > 0 {
                overflow_control_width(OverflowControlSide::Right, hidden_right)
            } else {
                0
            };

            if next_left == left_width && next_right == right_width {
                break;
            }

            left_width = next_left;
            right_width = next_right;
        }

        let content_start = left_width.min(vp.viewport_width);
        let content_width = vp.viewport_width.saturating_sub(left_width + right_width);

        let mut shifted_tabs = Vec::with_capacity(visible_tabs.len());
        for tab in visible_tabs {
            shifted_tabs.push(VisibleTab {
                start: tab.start.saturating_add(content_start),
                end: tab.end.saturating_add(content_start),
                ..tab
            });
        }

        let left_control = if left_width > 0 {
            Some(OverflowControl {
                start: 0,
                end: left_width,
                label: overflow_control_label(OverflowControlSide::Left, hidden_left),
            })
        } else {
            None
        };

        let right_control = if right_width > 0 {
            let start = vp.viewport_width.saturating_sub(right_width);
            Some(OverflowControl {
                start,
                end: vp.viewport_width,
                label: overflow_control_label(OverflowControlSide::Right, hidden_right),
            })
        } else {
            None
        };

        TabViewportLayout {
            offset,
            visible_tabs: shifted_tabs,
            hidden_left,
            hidden_right,
            content_start,
            content_width,
            left_control,
            right_control,
        }
    }

    pub(crate) fn hit_target_at_view_col(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        vp: &TabViewportOptions,
        col: usize,
    ) -> Option<DraggableTabHitTarget> {
        let layout = Self::viewport_layout(tabs, opts, vp);

        if let Some(left) = &layout.left_control
            && col >= left.start
            && col < left.end
        {
            return Some(DraggableTabHitTarget::Overflow(OverflowControlSide::Left));
        }
        if let Some(right) = &layout.right_control
            && col >= right.start
            && col < right.end
        {
            return Some(DraggableTabHitTarget::Overflow(OverflowControlSide::Right));
        }

        for tab in &layout.visible_tabs {
            if col < tab.start || col >= tab.end {
                continue;
            }
            let local = col.saturating_sub(tab.start).saturating_add(tab.clip_left);
            let part = if let (Some(close_start), Some(close_end)) =
                (tab.metrics.close_start, tab.metrics.close_end)
            {
                if local >= close_start && local < close_end {
                    DraggableTabHitPart::Close
                } else {
                    DraggableTabHitPart::Body
                }
            } else {
                DraggableTabHitPart::Body
            };
            return Some(DraggableTabHitTarget::Tab(DraggableTabHit {
                index: tab.index,
                part,
            }));
        }

        None
    }

    pub(crate) fn global_col_from_view_col(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        vp: &TabViewportOptions,
        col: usize,
    ) -> Option<usize> {
        let layout = Self::viewport_layout(tabs, opts, vp);

        if col < layout.content_start
            || col >= layout.content_start.saturating_add(layout.content_width)
        {
            return None;
        }

        let view_col = col.saturating_sub(layout.content_start);
        Some(layout.offset.saturating_add(view_col))
    }

    pub(crate) fn reorder_index_at_view_col(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        vp: &TabViewportOptions,
        col: usize,
    ) -> Option<usize> {
        let global_col = Self::global_col_from_view_col(tabs, opts, vp, col)?;
        let metrics = tab_metrics_for_viewport(tabs, opts, Some(vp.viewport_width));
        Self::reorder_index_at_col_with_metrics(tabs, opts, &metrics, global_col)
    }

    pub(crate) fn adjacent_reorder_target_at_view_col(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        vp: &TabViewportOptions,
        current_index: usize,
        col: usize,
    ) -> Option<usize> {
        let global_col = Self::global_col_from_view_col(tabs, opts, vp, col)?;
        let metrics = tab_metrics_for_viewport(tabs, opts, Some(vp.viewport_width));
        Self::adjacent_reorder_target_with_metrics(tabs, opts, &metrics, current_index, global_col)
    }

    pub(crate) fn scroll_offset_for_step(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        vp: &TabViewportOptions,
        step_right: bool,
        step_chars: usize,
    ) -> usize {
        if tabs.is_empty() || vp.viewport_width == 0 {
            return 0;
        }

        let layout = Self::viewport_layout(tabs, opts, vp);
        let current = layout.offset;

        if (step_right && layout.hidden_right == 0) || (!step_right && layout.hidden_left == 0) {
            return current;
        }

        let step = step_chars.max(1);
        let requested = if step_right {
            current.saturating_add(step)
        } else {
            current.saturating_sub(step)
        };

        let canonical = Self::viewport_layout(
            tabs,
            opts,
            &TabViewportOptions {
                scroll_offset: requested,
                viewport_width: vp.viewport_width,
                show_overflow_controls: vp.show_overflow_controls,
            },
        )
        .offset;

        if canonical != current {
            return canonical;
        }

        if step_right {
            let total_width = Self::content_width_for_viewport(tabs, opts, vp.viewport_width);
            return Self::viewport_layout(
                tabs,
                opts,
                &TabViewportOptions {
                    scroll_offset: total_width.saturating_sub(1),
                    viewport_width: vp.viewport_width,
                    show_overflow_controls: vp.show_overflow_controls,
                },
            )
            .offset;
        }

        0
    }

    pub(crate) fn scroll_offset_to_reveal_tab(
        tabs: &[DraggableTab],
        opts: &TabDisplayOptions<'_>,
        vp: &TabViewportOptions,
        tab_index: usize,
    ) -> usize {
        if tabs.is_empty() || vp.viewport_width == 0 {
            return 0;
        }
        let target = tab_index.min(tabs.len().saturating_sub(1));
        let metrics = tab_metrics_for_viewport(tabs, opts, Some(vp.viewport_width));
        let target_start = tabs_prefix_width_from_metrics(&metrics, opts, target);
        let target_width = metrics[target].width;
        let target_end = target_start.saturating_add(target_width);

        let mut offset = vp.scroll_offset;
        for _ in 0..8 {
            let layout = Self::viewport_layout(
                tabs,
                opts,
                &TabViewportOptions {
                    scroll_offset: offset,
                    viewport_width: vp.viewport_width,
                    show_overflow_controls: vp.show_overflow_controls,
                },
            );
            let view_start = layout.offset;
            let view_end = view_start.saturating_add(layout.content_width);

            let next = if target_width > layout.content_width || target_start < view_start {
                target_start
            } else if target_end > view_end {
                target_end.saturating_sub(layout.content_width)
            } else {
                return view_start;
            };

            if next == offset {
                return layout.offset;
            }
            offset = next;
        }

        offset
    }
}

pub(crate) fn is_reorderable_tab(tab: &DraggableTab) -> bool {
    tab.kind == DraggableTabKind::Tab
}

#[cfg(test)]
pub(crate) fn reorder_target_at_col_with_options(
    tabs: &[DraggableTab],
    opts: &TabDisplayOptions<'_>,
    col: usize,
) -> Option<usize> {
    let candidate = DraggableTabBar::reorder_index_at_col_with_options(tabs, opts, col)?;
    if tabs.get(candidate).is_some_and(is_reorderable_tab) {
        return Some(candidate);
    }

    (0..candidate)
        .rev()
        .find(|&index| tabs.get(index).is_some_and(is_reorderable_tab))
        .or_else(|| {
            ((candidate + 1)..tabs.len())
                .find(|&index| tabs.get(index).is_some_and(is_reorderable_tab))
        })
}

pub(crate) fn reorder_target_at_view_col_with_options(
    tabs: &[DraggableTab],
    opts: &TabDisplayOptions<'_>,
    vp: &TabViewportOptions,
    col: usize,
) -> Option<usize> {
    let candidate = DraggableTabBar::reorder_index_at_view_col(tabs, opts, vp, col)?;
    if tabs.get(candidate).is_some_and(is_reorderable_tab) {
        return Some(candidate);
    }

    (0..candidate)
        .rev()
        .find(|&index| tabs.get(index).is_some_and(is_reorderable_tab))
        .or_else(|| {
            ((candidate + 1)..tabs.len())
                .find(|&index| tabs.get(index).is_some_and(is_reorderable_tab))
        })
}

fn separator_width(variant: DraggableTabBarVariant, divider: char) -> usize {
    match variant {
        DraggableTabBarVariant::Bordered => UnicodeWidthChar::width(divider).unwrap_or(1),
        DraggableTabBarVariant::FrameLine => 0,
    }
}

#[cfg(test)]
pub(crate) fn tab_metrics(
    tab: &DraggableTab,
    variant: DraggableTabBarVariant,
    accent_symbol: char,
    close_symbol: &str,
    show_close_buttons: bool,
) -> TabMetrics {
    tab_metrics_with_options(
        tab,
        &TabDisplayOptions {
            variant,
            divider: '|',
            accent_symbol,
            close_symbol,
            show_close_buttons,
            tab_max_width: None,
            overflow: DraggableTabBarOverflow::Scroll,
            show_file_icons: false,
            file_icon_style: FileIconStyle::NerdFont,
            file_icon_palette: &FileIconPalette::default(),
            file_icon_overrides: &HashMap::new(),
        },
    )
}

pub(crate) fn tab_metrics_with_options(
    tab: &DraggableTab,
    opts: &TabDisplayOptions<'_>,
) -> TabMetrics {
    let label_w = tab_label_width(tab, opts);
    tab_metrics_with_label_width(tab, opts, label_w)
}

fn tab_label_width(tab: &DraggableTab, opts: &TabDisplayOptions<'_>) -> usize {
    let mut label_w = UnicodeWidthStr::width(tab.label.as_ref());
    if let Some(max) = opts.tab_max_width {
        let max = (max as usize).max(1);
        label_w = label_w.min(max);
    }
    label_w
}

fn tab_metrics_with_label_width(
    tab: &DraggableTab,
    opts: &TabDisplayOptions<'_>,
    label_w: usize,
) -> TabMetrics {
    let icon_w = resolve_tab_icon(
        tab,
        opts.show_file_icons,
        opts.file_icon_style,
        opts.file_icon_palette,
        opts.file_icon_overrides,
    )
    .map(|icon| UnicodeWidthStr::width(icon.content.as_ref()).saturating_add(1))
    .unwrap_or(0);

    let badge_w = tab
        .right_badge
        .as_ref()
        .map(|badge| UnicodeWidthStr::width(badge.content.as_ref()))
        .unwrap_or(0);
    let badge_gap_w = if badge_w > 0 { 1 } else { 0 };

    let close_symbol_w = UnicodeWidthStr::width(opts.close_symbol).max(1);
    let has_close = opts.show_close_buttons && tab.closeable && is_reorderable_tab(tab);
    let close_gap_w = if has_close { 1 } else { 0 };
    let close_zone_w = if has_close { close_symbol_w } else { 0 };

    match opts.variant {
        DraggableTabBarVariant::Bordered => {
            let close_start =
                has_close.then_some(1 + icon_w + label_w + badge_gap_w + badge_w + close_gap_w);
            let close_end = has_close.then_some(
                1 + icon_w + label_w + badge_gap_w + badge_w + close_gap_w + close_zone_w,
            );
            TabMetrics {
                width: 1
                    + icon_w
                    + label_w
                    + badge_gap_w
                    + badge_w
                    + close_gap_w
                    + close_zone_w
                    + 1,
                close_start,
                close_end,
                label_width: label_w,
            }
        }
        DraggableTabBarVariant::FrameLine => {
            let accent_w = UnicodeWidthChar::width(opts.accent_symbol).unwrap_or(1);
            let close_start = has_close
                .then_some(accent_w + 1 + icon_w + label_w + badge_gap_w + badge_w + close_gap_w);
            let close_end = has_close.then_some(
                accent_w
                    + 1
                    + icon_w
                    + label_w
                    + badge_gap_w
                    + badge_w
                    + close_gap_w
                    + close_zone_w,
            );
            TabMetrics {
                width: accent_w
                    + 1
                    + icon_w
                    + label_w
                    + badge_gap_w
                    + badge_w
                    + close_gap_w
                    + close_zone_w
                    + 1,
                close_start,
                close_end,
                label_width: label_w,
            }
        }
    }
}

fn natural_tab_metrics(tabs: &[DraggableTab], opts: &TabDisplayOptions<'_>) -> Vec<TabMetrics> {
    tabs.iter()
        .map(|tab| tab_metrics_with_options(tab, opts))
        .collect()
}

fn tab_metrics_for_viewport(
    tabs: &[DraggableTab],
    opts: &TabDisplayOptions<'_>,
    viewport_width: Option<usize>,
) -> Vec<TabMetrics> {
    let natural = natural_tab_metrics(tabs, opts);
    let Some(viewport_width) = viewport_width else {
        return natural;
    };

    let DraggableTabBarOverflow::ShrinkThenScroll { min_tab_width } = opts.overflow else {
        return natural;
    };

    if tabs.is_empty() || viewport_width == 0 {
        return natural;
    }

    let natural_total = total_width_for_metrics(&natural, opts);
    if natural_total <= viewport_width {
        return natural;
    }

    let min_tab_width = (min_tab_width as usize).max(1);
    let min_label_widths = natural
        .iter()
        .map(|metrics| {
            let fixed_width = metrics.width.saturating_sub(metrics.label_width);
            metrics
                .label_width
                .min(min_tab_width.saturating_sub(fixed_width))
        })
        .collect::<Vec<_>>();
    let fixed_total = natural
        .iter()
        .map(|metrics| metrics.width.saturating_sub(metrics.label_width))
        .sum::<usize>()
        .saturating_add(separator_width(opts.variant, opts.divider) * tabs.len().saturating_sub(1));
    let min_total = fixed_total.saturating_add(min_label_widths.iter().sum::<usize>());

    if min_total >= natural_total {
        return natural;
    }

    if min_total > viewport_width {
        return tabs
            .iter()
            .zip(min_label_widths)
            .map(|(tab, label_width)| tab_metrics_with_label_width(tab, opts, label_width))
            .collect();
    }

    let max_label_width = natural
        .iter()
        .map(|metrics| metrics.label_width)
        .max()
        .unwrap_or(0);
    let mut low = 0usize;
    let mut high = max_label_width;
    while low < high {
        let mid = (low + high).div_ceil(2);
        let total = total_width_for_label_cap(&natural, &min_label_widths, fixed_total, mid);
        if total <= viewport_width {
            low = mid;
        } else {
            high = mid.saturating_sub(1);
        }
    }

    let cap = low;
    let mut label_widths = natural
        .iter()
        .zip(&min_label_widths)
        .map(|(metrics, &min_label_width)| metrics.label_width.min(cap.max(min_label_width)))
        .collect::<Vec<_>>();
    let total = fixed_total.saturating_add(label_widths.iter().sum::<usize>());
    let mut spare = viewport_width.saturating_sub(total);
    if spare > 0 {
        for (label_width, natural_metrics) in label_widths.iter_mut().zip(&natural) {
            if spare == 0 {
                break;
            }
            if *label_width < natural_metrics.label_width {
                *label_width += 1;
                spare -= 1;
            }
        }
    }

    tabs.iter()
        .zip(label_widths)
        .map(|(tab, label_width)| tab_metrics_with_label_width(tab, opts, label_width))
        .collect()
}

fn total_width_for_label_cap(
    natural: &[TabMetrics],
    min_label_widths: &[usize],
    fixed_total: usize,
    cap: usize,
) -> usize {
    fixed_total.saturating_add(
        natural
            .iter()
            .zip(min_label_widths)
            .map(|(metrics, &min_label_width)| metrics.label_width.min(cap.max(min_label_width)))
            .sum::<usize>(),
    )
}

fn total_width_for_metrics(metrics: &[TabMetrics], opts: &TabDisplayOptions<'_>) -> usize {
    let tabs_width = metrics.iter().map(|metrics| metrics.width).sum::<usize>();
    tabs_width.saturating_add(
        separator_width(opts.variant, opts.divider) * metrics.len().saturating_sub(1),
    )
}

fn tab_runs_for_metrics(
    metrics: &[TabMetrics],
    opts: &TabDisplayOptions<'_>,
) -> (Vec<(usize, usize, TabMetrics)>, usize) {
    let mut runs = Vec::with_capacity(metrics.len());
    let mut total_width = 0usize;
    for (i, tab_metrics) in metrics.iter().copied().enumerate() {
        let start = total_width;
        let end = start.saturating_add(tab_metrics.width);
        runs.push((start, end, tab_metrics));
        total_width = end;
        if i + 1 < metrics.len() {
            total_width = total_width.saturating_add(separator_width(opts.variant, opts.divider));
        }
    }
    (runs, total_width)
}

fn tab_runs_for_viewport(
    tabs: &[DraggableTab],
    opts: &TabDisplayOptions<'_>,
    viewport_width: Option<usize>,
) -> (Vec<(usize, usize, TabMetrics)>, usize) {
    let metrics = tab_metrics_for_viewport(tabs, opts, viewport_width);
    tab_runs_for_metrics(&metrics, opts)
}

fn tabs_prefix_width_from_metrics(
    metrics: &[TabMetrics],
    opts: &TabDisplayOptions<'_>,
    offset: usize,
) -> usize {
    let upto = offset.min(metrics.len());
    let tabs_width = metrics
        .iter()
        .take(upto)
        .map(|metrics| metrics.width)
        .sum::<usize>();
    tabs_width.saturating_add(separator_width(opts.variant, opts.divider) * upto.saturating_sub(1))
}

#[cfg(test)]
fn tabs_prefix_width(tabs: &[DraggableTab], opts: &TabDisplayOptions<'_>, offset: usize) -> usize {
    let metrics = tab_metrics_for_viewport(tabs, opts, None);
    tabs_prefix_width_from_metrics(&metrics, opts, offset)
}

pub(crate) fn tab_fully_visible_at_offset(
    tabs: &[DraggableTab],
    opts: &TabDisplayOptions<'_>,
    tab_index: usize,
    offset: usize,
    viewport_width: usize,
) -> bool {
    if tabs.is_empty() || tab_index >= tabs.len() || viewport_width == 0 {
        return false;
    }
    let metrics = tab_metrics_for_viewport(tabs, opts, Some(viewport_width));
    let tab_start = tabs_prefix_width_from_metrics(&metrics, opts, tab_index);
    let tab_width = metrics[tab_index].width;
    let tab_end = tab_start.saturating_add(tab_width);
    tab_start >= offset && tab_end <= offset.saturating_add(viewport_width)
}

fn overflow_control_label(side: OverflowControlSide, hidden_count: usize) -> Arc<str> {
    match side {
        OverflowControlSide::Left => Arc::from(format!(" {} ", hidden_count)),
        OverflowControlSide::Right => Arc::from(format!("  {}", hidden_count)),
    }
}

fn overflow_control_width(side: OverflowControlSide, hidden_count: usize) -> usize {
    UnicodeWidthStr::width(overflow_control_label(side, hidden_count).as_ref())
}

fn lookup_icon_override<'a>(
    key: &str,
    overrides: &'a HashMap<Arc<str>, FileIconOverride>,
) -> Option<&'a FileIconOverride> {
    let path = Path::new(key);
    if let Some(name) = path.file_name().and_then(|n| n.to_str())
        && let Some(override_icon) = overrides.get(name)
    {
        return Some(override_icon);
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        return overrides.get(ext);
    }
    None
}

pub(crate) fn resolve_tab_icon(
    tab: &DraggableTab,
    show_file_icons: bool,
    file_icon_style: FileIconStyle,
    file_icon_palette: &FileIconPalette,
    file_icon_overrides: &HashMap<Arc<str>, FileIconOverride>,
) -> Option<Span> {
    if let Some(leading) = &tab.leading {
        return Some(leading.to_span());
    }

    if let Some(icon) = &tab.icon {
        return Some(icon.clone());
    }

    if !show_file_icons {
        return None;
    }

    let key = tab.path.as_deref().unwrap_or(tab.label.as_ref());
    if let Some(override_icon) = lookup_icon_override(key, file_icon_overrides) {
        let mut span = Span::new(override_icon.icon.clone());
        if let Some(color) = override_icon.color {
            span = span.fg(color);
        }
        return Some(span);
    }

    match file_icon_style {
        FileIconStyle::Text => Some(Span::new("[F]")),
        FileIconStyle::NerdFont | FileIconStyle::NerdFontColored => {
            let (icon, color) = file_icon(key, file_icon_palette);
            let mut span = Span::new(icon);
            if matches!(file_icon_style, FileIconStyle::NerdFontColored)
                && let Some(color) = color
            {
                span = span.fg(color);
            }
            Some(span)
        }
    }
}

impl From<DraggableTabBar> for Element {
    fn from(value: DraggableTabBar) -> Self {
        Element::new(ElementKind::DraggableTabBar(Box::new(value)))
    }
}

impl crate::layout::hash::LayoutHash for DraggableTabBar {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&crate::core::element::Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.border.hash(hasher);
        self.border_style.hash(hasher);
        self.padding.hash(hasher);
        self.tabs.len().hash(hasher);
        self.active.hash(hasher);
        self.divider.hash(hasher);
        self.close_symbol.hash(hasher);
        self.accent_symbol.hash(hasher);
        self.active_accent_symbol.hash(hasher);
        self.close_on_hover_only.hash(hasher);
        self.tab_max_width.hash(hasher);
        self.overflow.hash(hasher);
        self.show_overflow_controls.hash(hasher);
        self.scroll_offset.hash(hasher);
        self.show_file_icons.hash(hasher);
        self.file_icon_style.hash(hasher);
        self.variant.hash(hasher);
        self.show_close_buttons.hash(hasher);
        self.draggable.hash(hasher);
        self.drag_preview.hash(hasher);
        self.reorder_mode.hash(hasher);
        self.drag_threshold.hash(hasher);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::style::{FileIconPalette, Span};

    use super::{
        DraggableTab, DraggableTabBar, DraggableTabBarOverflow, DraggableTabBarVariant,
        DraggableTabHitPart,
    };
    use crate::widgets::FileIconStyle;

    #[test]
    fn hit_at_col_detects_close_region() {
        let tabs = vec![DraggableTab::new("main.rs").closeable(true)];
        let label_w = "main.rs".chars().count();
        let hit = DraggableTabBar::hit_at_col(
            &tabs,
            DraggableTabBarVariant::Bordered,
            '|',
            '|',
            "x",
            true,
            1 + label_w + 1,
        )
        .expect("expected hit");
        assert_eq!(hit.index, 0);
        assert_eq!(hit.part, DraggableTabHitPart::Close);
    }

    #[test]
    fn hit_at_col_detects_close_region_with_badge() {
        let tab = DraggableTab::new("main.rs")
            .right_badge(Span::new("M"))
            .closeable(true);
        let tabs = vec![tab];
        let metrics =
            super::tab_metrics(&tabs[0], DraggableTabBarVariant::Bordered, '|', "x", true);
        let close_col = metrics.close_start.expect("close start");
        let hit = DraggableTabBar::hit_at_col(
            &tabs,
            DraggableTabBarVariant::Bordered,
            '|',
            '|',
            "x",
            true,
            close_col,
        )
        .expect("expected hit");
        assert_eq!(hit.index, 0);
        assert_eq!(hit.part, DraggableTabHitPart::Close);
    }

    #[test]
    fn action_tab_does_not_reserve_close_region() {
        let tab = DraggableTab::action("+").closeable(true);
        let metrics = super::tab_metrics(&tab, DraggableTabBarVariant::Bordered, '|', "x", true);

        assert_eq!(metrics.close_start, None);
        assert_eq!(metrics.close_end, None);
        assert_eq!(metrics.width, 3);
    }

    #[test]
    fn hit_at_col_returns_none_on_separator() {
        let tabs = vec![DraggableTab::new("a"), DraggableTab::new("b")];
        let separator_col = 3; // first tab " a " is width 3
        let hit = DraggableTabBar::hit_at_col(
            &tabs,
            DraggableTabBarVariant::Bordered,
            '|',
            '|',
            "x",
            true,
            separator_col,
        );
        assert!(hit.is_none());
    }

    #[test]
    fn frame_line_content_width_is_nonzero() {
        let tabs = vec![DraggableTab::new("file.rs").closeable(true)];
        let width = DraggableTabBar::content_width(
            &tabs,
            DraggableTabBarVariant::FrameLine,
            '|',
            '|',
            "x",
            true,
        );
        assert!(width > 0);
    }

    #[test]
    fn reorder_index_maps_separator_to_adjacent_tab() {
        let tabs = vec![DraggableTab::new("a"), DraggableTab::new("b")];
        let separator_col = 3; // first tab " a " is width 3
        let idx = DraggableTabBar::reorder_index_at_col(
            &tabs,
            DraggableTabBarVariant::Bordered,
            '|',
            '|',
            "x",
            true,
            separator_col,
        )
        .expect("expected mapped index");
        assert_eq!(idx, 1);
    }

    #[test]
    fn adjacent_reorder_waits_until_midpoint_for_wider_neighbor() {
        let tabs = vec![DraggableTab::new("a"), DraggableTab::new("very-long-name")];
        let at_divider = 3; // after " a " in bordered variant

        let opts = super::TabDisplayOptions {
            variant: DraggableTabBarVariant::Bordered,
            divider: '|',
            accent_symbol: '|',
            close_symbol: "x",
            show_close_buttons: false,
            tab_max_width: None,
            overflow: super::DraggableTabBarOverflow::Scroll,
            show_file_icons: false,
            file_icon_style: FileIconStyle::NerdFont,
            file_icon_palette: &FileIconPalette::default(),
            file_icon_overrides: &HashMap::new(),
        };
        let target = DraggableTabBar::adjacent_reorder_target(&tabs, &opts, 0, at_divider);
        assert!(target.is_none());

        let second_start =
            super::tab_metrics(&tabs[0], DraggableTabBarVariant::Bordered, '|', "x", false).width
                + super::separator_width(DraggableTabBarVariant::Bordered, '|');
        let second_mid = second_start
            + super::tab_metrics(&tabs[1], DraggableTabBarVariant::Bordered, '|', "x", false).width
                / 2;
        let target = DraggableTabBar::adjacent_reorder_target(&tabs, &opts, 0, second_mid);
        assert_eq!(target, Some(1));
    }

    #[test]
    fn adjacent_reorder_does_not_target_trailing_action_tab() {
        let tabs = vec![
            DraggableTab::new("a"),
            DraggableTab::new("b"),
            DraggableTab::action("+"),
        ];
        let opts = super::TabDisplayOptions {
            variant: DraggableTabBarVariant::Bordered,
            divider: '|',
            accent_symbol: '|',
            close_symbol: "x",
            show_close_buttons: false,
            tab_max_width: None,
            overflow: super::DraggableTabBarOverflow::Scroll,
            show_file_icons: false,
            file_icon_style: FileIconStyle::NerdFont,
            file_icon_palette: &FileIconPalette::default(),
            file_icon_overrides: &HashMap::new(),
        };
        let action_mid = super::tabs_prefix_width(&tabs, &opts, 2)
            + super::tab_metrics(&tabs[2], DraggableTabBarVariant::Bordered, '|', "x", false).width
                / 2;

        let target = DraggableTabBar::adjacent_reorder_target(&tabs, &opts, 1, action_mid);

        assert_eq!(target, None);
    }

    #[test]
    fn on_drop_reorder_maps_action_hit_to_previous_tab() {
        let tabs = vec![
            DraggableTab::new("a"),
            DraggableTab::new("b"),
            DraggableTab::action("+"),
        ];
        let opts = super::TabDisplayOptions {
            variant: DraggableTabBarVariant::Bordered,
            divider: '|',
            accent_symbol: '|',
            close_symbol: "x",
            show_close_buttons: false,
            tab_max_width: None,
            overflow: super::DraggableTabBarOverflow::Scroll,
            show_file_icons: false,
            file_icon_style: FileIconStyle::NerdFont,
            file_icon_palette: &FileIconPalette::default(),
            file_icon_overrides: &HashMap::new(),
        };
        let action_col = super::tabs_prefix_width(&tabs, &opts, 2);

        let raw_target =
            DraggableTabBar::reorder_index_at_col_with_options(&tabs, &opts, action_col);
        let reorder_target = super::reorder_target_at_col_with_options(&tabs, &opts, action_col);

        assert_eq!(raw_target, Some(2));
        assert_eq!(reorder_target, Some(1));
    }

    #[test]
    fn viewport_layout_keeps_partially_visible_tab() {
        let tabs = vec![
            DraggableTab::new("alpha"),
            DraggableTab::new("beta-gamma"),
            DraggableTab::new("delta"),
        ];
        let first =
            super::tab_metrics(&tabs[0], DraggableTabBarVariant::Bordered, '|', "x", false).width;
        let sep = super::separator_width(DraggableTabBarVariant::Bordered, '|');
        let second =
            super::tab_metrics(&tabs[1], DraggableTabBarVariant::Bordered, '|', "x", false).width;
        let viewport_width = first + sep + (second / 2).max(1);

        let overrides: HashMap<std::sync::Arc<str>, super::FileIconOverride> = HashMap::new();
        let layout = DraggableTabBar::viewport_layout(
            &tabs,
            &super::TabDisplayOptions {
                variant: DraggableTabBarVariant::Bordered,
                divider: '|',
                accent_symbol: '|',
                close_symbol: "x",
                show_close_buttons: false,
                tab_max_width: None,
                overflow: super::DraggableTabBarOverflow::Scroll,
                show_file_icons: false,
                file_icon_style: crate::widgets::FileIconStyle::NerdFont,
                file_icon_palette: &FileIconPalette::default(),
                file_icon_overrides: &overrides,
            },
            &super::TabViewportOptions {
                scroll_offset: 0,
                viewport_width,
                show_overflow_controls: true,
            },
        );

        assert_eq!(layout.visible_tabs.len(), 2);
        assert_eq!(layout.visible_tabs[0].index, 0);
        assert_eq!(layout.visible_tabs[1].index, 1);
        assert!(
            layout.visible_tabs[1]
                .end
                .saturating_sub(layout.visible_tabs[1].start)
                < second
        );
        assert_eq!(layout.hidden_right, 2);
    }

    #[test]
    fn viewport_layout_can_clip_left_tab() {
        let tabs = vec![
            DraggableTab::new("alpha"),
            DraggableTab::new("beta"),
            DraggableTab::new("gamma"),
        ];
        let overrides: HashMap<std::sync::Arc<str>, super::FileIconOverride> = HashMap::new();
        let layout = DraggableTabBar::viewport_layout(
            &tabs,
            &super::TabDisplayOptions {
                variant: DraggableTabBarVariant::Bordered,
                divider: '|',
                accent_symbol: '|',
                close_symbol: "x",
                show_close_buttons: false,
                tab_max_width: None,
                overflow: super::DraggableTabBarOverflow::Scroll,
                show_file_icons: false,
                file_icon_style: crate::widgets::FileIconStyle::NerdFont,
                file_icon_palette: &FileIconPalette::default(),
                file_icon_overrides: &overrides,
            },
            &super::TabViewportOptions {
                scroll_offset: 1,
                viewport_width: 10,
                show_overflow_controls: true,
            },
        );

        assert!(layout.hidden_left > 0);
        let first = layout.visible_tabs.first().expect("expected visible tabs");
        assert_eq!(first.index, 0);
        assert!(first.clip_left > 0);
    }

    #[test]
    fn shrink_then_scroll_fits_tabs_before_scrolling() {
        let tabs = vec![
            DraggableTab::new("abcdef"),
            DraggableTab::new("abcdef"),
            DraggableTab::new("abcdef"),
        ];
        let overrides: HashMap<std::sync::Arc<str>, super::FileIconOverride> = HashMap::new();
        let opts = super::TabDisplayOptions {
            variant: DraggableTabBarVariant::Bordered,
            divider: '|',
            accent_symbol: '|',
            close_symbol: "x",
            show_close_buttons: false,
            tab_max_width: None,
            overflow: DraggableTabBarOverflow::ShrinkThenScroll { min_tab_width: 5 },
            show_file_icons: false,
            file_icon_style: crate::widgets::FileIconStyle::NerdFont,
            file_icon_palette: &FileIconPalette::default(),
            file_icon_overrides: &overrides,
        };

        let layout = DraggableTabBar::viewport_layout(
            &tabs,
            &opts,
            &super::TabViewportOptions {
                scroll_offset: 0,
                viewport_width: 20,
                show_overflow_controls: true,
            },
        );

        assert_eq!(layout.hidden_left, 0);
        assert_eq!(layout.hidden_right, 0);
        assert!(layout.left_control.is_none());
        assert!(layout.right_control.is_none());
        assert_eq!(layout.visible_tabs.len(), 3);
        assert_eq!(layout.visible_tabs[0].metrics.label_width, 4);
        assert_eq!(layout.visible_tabs.last().expect("last tab").end, 20);
    }

    #[test]
    fn shrink_then_scroll_scrolls_after_min_widths() {
        let tabs = vec![
            DraggableTab::new("abcdef"),
            DraggableTab::new("abcdef"),
            DraggableTab::new("abcdef"),
        ];
        let overrides: HashMap<std::sync::Arc<str>, super::FileIconOverride> = HashMap::new();
        let opts = super::TabDisplayOptions {
            variant: DraggableTabBarVariant::Bordered,
            divider: '|',
            accent_symbol: '|',
            close_symbol: "x",
            show_close_buttons: false,
            tab_max_width: None,
            overflow: DraggableTabBarOverflow::ShrinkThenScroll { min_tab_width: 5 },
            show_file_icons: false,
            file_icon_style: crate::widgets::FileIconStyle::NerdFont,
            file_icon_palette: &FileIconPalette::default(),
            file_icon_overrides: &overrides,
        };

        let layout = DraggableTabBar::viewport_layout(
            &tabs,
            &opts,
            &super::TabViewportOptions {
                scroll_offset: 0,
                viewport_width: 14,
                show_overflow_controls: true,
            },
        );

        assert_eq!(layout.visible_tabs[0].metrics.width, 5);
        assert_eq!(layout.visible_tabs[0].metrics.label_width, 3);
        assert_eq!(layout.hidden_left, 0);
        assert!(layout.hidden_right > 0);
        assert!(layout.right_control.is_some());
    }

    #[test]
    fn shrink_then_scroll_hit_testing_uses_shrunken_widths() {
        let tabs = vec![
            DraggableTab::new("abcdef"),
            DraggableTab::new("abcdef"),
            DraggableTab::new("abcdef"),
        ];
        let overrides: HashMap<std::sync::Arc<str>, super::FileIconOverride> = HashMap::new();
        let opts = super::TabDisplayOptions {
            variant: DraggableTabBarVariant::Bordered,
            divider: '|',
            accent_symbol: '|',
            close_symbol: "x",
            show_close_buttons: false,
            tab_max_width: None,
            overflow: DraggableTabBarOverflow::ShrinkThenScroll { min_tab_width: 5 },
            show_file_icons: false,
            file_icon_style: crate::widgets::FileIconStyle::NerdFont,
            file_icon_palette: &FileIconPalette::default(),
            file_icon_overrides: &overrides,
        };

        let hit = DraggableTabBar::hit_target_at_view_col(
            &tabs,
            &opts,
            &super::TabViewportOptions {
                scroll_offset: 0,
                viewport_width: 20,
                show_overflow_controls: true,
            },
            7,
        );

        assert!(matches!(
            hit,
            Some(super::DraggableTabHitTarget::Tab(super::DraggableTabHit {
                index: 1,
                part: DraggableTabHitPart::Body,
            }))
        ));
    }

    #[test]
    fn overflow_right_label_has_left_padding() {
        assert_eq!(
            super::overflow_control_label(super::OverflowControlSide::Right, 1).as_ref(),
            "  1"
        );
    }

    #[test]
    fn stepping_right_reaches_visible_right_edge() {
        let tabs = vec![
            DraggableTab::new("alpha.rs"),
            DraggableTab::new("beta-long-file-name.rs"),
            DraggableTab::new("gamma.rs"),
            DraggableTab::new("delta.rs"),
            DraggableTab::new("epsilon.rs"),
        ];
        let overrides: HashMap<std::sync::Arc<str>, super::FileIconOverride> = HashMap::new();
        let mut offset = 0usize;
        let viewport_width = 24usize;
        let disp_opts = super::TabDisplayOptions {
            variant: DraggableTabBarVariant::Bordered,
            divider: '|',
            accent_symbol: '|',
            close_symbol: "x",
            show_close_buttons: false,
            tab_max_width: None,
            overflow: super::DraggableTabBarOverflow::Scroll,
            show_file_icons: false,
            file_icon_style: crate::widgets::FileIconStyle::NerdFont,
            file_icon_palette: &FileIconPalette::default(),
            file_icon_overrides: &overrides,
        };

        for _ in 0..64 {
            let next = DraggableTabBar::scroll_offset_for_step(
                &tabs,
                &disp_opts,
                &super::TabViewportOptions {
                    scroll_offset: offset,
                    viewport_width,
                    show_overflow_controls: true,
                },
                true,
                super::TAB_SCROLL_STEP_CHARS,
            );
            if next == offset {
                break;
            }
            offset = next;
        }

        let layout = DraggableTabBar::viewport_layout(
            &tabs,
            &disp_opts,
            &super::TabViewportOptions {
                scroll_offset: offset,
                viewport_width,
                show_overflow_controls: true,
            },
        );

        assert_eq!(
            layout.hidden_right, 0,
            "offset={} content_width={} hidden_left={} layout={:?}",
            layout.offset, layout.content_width, layout.hidden_left, layout
        );
    }
}
