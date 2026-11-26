//! Context menu widget with flat items or structured entries and cascading submenus.

use std::{rc::Rc, sync::Arc, time::Duration};

use unicode_width::UnicodeWidthStr;

use crate::animation::VisibilityAnimation;
use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, IntoElement};
use crate::core::event::KeyCode;
use crate::style::{
    BorderStyle, Length, Padding, RowStylePolicy, ScrollbarConfig, Span, Style, StyleSlot,
};
use crate::widgets::hover_reveal::{HoverDelay, HoverReveal, RevealStep};
use crate::widgets::{
    List, ListHoverEvent, ListItem, ListItemGutter, Popover, PopoverOffset, PopoverPlacement,
};

/// A structured entry inside a [`ContextMenu`], supporting actions, cascading submenus,
/// horizontal separators, and non-selectable headers.
#[derive(Clone, Debug, PartialEq)]
pub enum ContextMenuEntry {
    /// An interactive leaf action.
    Action(ContextMenuItem),
    /// A parent item that opens a nested submenu panel on hover, `Right`, or `Enter`.
    Submenu(ContextMenuSubmenu),
    /// A horizontal rule separating menu groups.
    Separator,
    /// A non-selectable section header.
    Header(Arc<str>),
}

/// A leaf action entry for [`ContextMenu`].
#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenuItem {
    /// Action identifier emitted through [`ContextMenu::on_action`].
    pub id: Arc<str>,
    /// Primary label.
    pub label: Arc<str>,
    /// Optional leading icon rendered in the shared gutter column.
    pub icon: Option<Span>,
    /// Optional right-aligned detail or shortcut hint.
    pub detail: Option<Arc<str>>,
    /// Optional check state rendered in the gutter column (`Some(true)` draws the check symbol,
    /// `Some(false)` reserves an aligned blank gutter slot).
    pub checked: Option<bool>,
    /// Whether this action is destructive (styled with [`ContextMenu::destructive_style`]).
    pub destructive: bool,
    /// Whether this action is disabled (skipped by navigation and non-activatable).
    pub disabled: bool,
    /// Explicit row style override.
    pub style: Option<Style>,
}

/// A submenu entry for [`ContextMenu`].
#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenuSubmenu {
    /// Identifier for the submenu trigger.
    pub id: Arc<str>,
    /// Primary label.
    pub label: Arc<str>,
    /// Optional leading icon rendered in the shared gutter column.
    pub icon: Option<Span>,
    /// Entries displayed inside the cascading submenu panel.
    pub children: Vec<ContextMenuEntry>,
    /// Whether this submenu is disabled.
    pub disabled: bool,
}

impl ContextMenuItem {
    /// Create a leaf action entry.
    pub fn new(id: impl Into<Arc<str>>, label: impl Into<Arc<str>>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            detail: None,
            checked: None,
            destructive: false,
            disabled: false,
            style: None,
        }
    }

    /// Set a leading icon glyph.
    pub fn icon(mut self, glyph: impl Into<Arc<str>>) -> Self {
        self.icon = Some(Span::new(glyph));
        self
    }

    /// Set a styled leading icon span.
    pub fn icon_span(mut self, span: Span) -> Self {
        self.icon = Some(span);
        self
    }

    /// Set right-aligned detail or shortcut text.
    pub fn detail(mut self, detail: impl Into<Arc<str>>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Mark this item as a radio/toggle entry with a checked state.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Mark this item as destructive.
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    /// Disable this item.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set an explicit row style override.
    pub fn style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }
}

impl ContextMenuSubmenu {
    /// Create a submenu entry.
    pub fn new(
        id: impl Into<Arc<str>>,
        label: impl Into<Arc<str>>,
        children: impl IntoIterator<Item = ContextMenuEntry>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            children: children.into_iter().collect(),
            disabled: false,
        }
    }

    /// Set a leading icon glyph.
    pub fn icon(mut self, glyph: impl Into<Arc<str>>) -> Self {
        self.icon = Some(Span::new(glyph));
        self
    }

    /// Set a styled leading icon span.
    pub fn icon_span(mut self, span: Span) -> Self {
        self.icon = Some(span);
        self
    }

    /// Disable this submenu trigger.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl From<ContextMenuItem> for ContextMenuEntry {
    fn from(item: ContextMenuItem) -> Self {
        Self::Action(item)
    }
}

impl From<ContextMenuSubmenu> for ContextMenuEntry {
    fn from(submenu: ContextMenuSubmenu) -> Self {
        Self::Submenu(submenu)
    }
}

impl ContextMenuEntry {
    /// Create a leaf action entry.
    pub fn action(id: impl Into<Arc<str>>, label: impl Into<Arc<str>>) -> Self {
        Self::Action(ContextMenuItem::new(id, label))
    }

    /// Create a cascading submenu entry.
    pub fn submenu(
        id: impl Into<Arc<str>>,
        label: impl Into<Arc<str>>,
        children: impl IntoIterator<Item = ContextMenuEntry>,
    ) -> Self {
        Self::Submenu(ContextMenuSubmenu::new(id, label, children))
    }

    /// Create a horizontal separator rule.
    pub fn separator() -> Self {
        Self::Separator
    }

    /// Create a non-selectable section header.
    pub fn header(label: impl Into<Arc<str>>) -> Self {
        Self::Header(label.into())
    }

    /// Attach a leading icon glyph to an action or submenu entry.
    pub fn icon(self, glyph: impl Into<Arc<str>>) -> Self {
        match self {
            Self::Action(item) => Self::Action(item.icon(glyph)),
            Self::Submenu(sub) => Self::Submenu(sub.icon(glyph)),
            other => other,
        }
    }

    /// Attach a styled leading icon span to an action or submenu entry.
    pub fn icon_span(self, span: Span) -> Self {
        match self {
            Self::Action(item) => Self::Action(item.icon_span(span)),
            Self::Submenu(sub) => Self::Submenu(sub.icon_span(span)),
            other => other,
        }
    }

    /// Set right-aligned detail text on an action entry.
    pub fn detail(self, detail: impl Into<Arc<str>>) -> Self {
        match self {
            Self::Action(item) => Self::Action(item.detail(detail)),
            other => other,
        }
    }

    /// Set checked state on an action entry.
    pub fn checked(self, checked: bool) -> Self {
        match self {
            Self::Action(item) => Self::Action(item.checked(checked)),
            other => other,
        }
    }

    /// Mark an action entry as destructive.
    pub fn destructive(self, destructive: bool) -> Self {
        match self {
            Self::Action(item) => Self::Action(item.destructive(destructive)),
            other => other,
        }
    }

    /// Disable an action or submenu entry.
    pub fn disabled(self, disabled: bool) -> Self {
        match self {
            Self::Action(item) => Self::Action(item.disabled(disabled)),
            Self::Submenu(sub) => Self::Submenu(sub.disabled(disabled)),
            other => other,
        }
    }

    /// Set an explicit row style on an action entry.
    pub fn style(self, style: Style) -> Self {
        match self {
            Self::Action(item) => Self::Action(item.style(style)),
            other => other,
        }
    }

    fn is_selectable(&self) -> bool {
        match self {
            Self::Action(item) => !item.disabled,
            Self::Submenu(sub) => !sub.disabled && !sub.children.is_empty(),
            Self::Separator | Self::Header(_) => false,
        }
    }
}

/// A popup menu widget supporting flat items or structured entries with cascading submenus.
#[derive(Clone)]
pub struct ContextMenu {
    trigger: Element,
    items: Vec<ListItem>,
    entries: Option<Vec<ContextMenuEntry>>,
    open: bool,
    pub(crate) animation: Option<VisibilityAnimation>,
    on_select: Option<Callback<usize>>,
    on_action: Option<Callback<Arc<str>>>,
    on_close: Option<Callback<()>>,
    placement: PopoverPlacement,
    offset: PopoverOffset,
    clamp: bool,
    auto_flip: bool,
    anchor: Option<(u16, u16)>,
    width: Option<Length>,
    min_width: u16,
    max_width: Option<u16>,
    height: Length,
    border: bool,
    border_style: BorderStyle,
    padding: Padding,
    style: Style,
    selection_style: StyleSlot,
    unfocused_selection_style: StyleSlot,
    item_hover_style: Option<StyleSlot>,
    selection_full_width: Option<bool>,
    symbol_column: Option<bool>,
    selection_symbol: Option<Arc<str>>,
    selection_symbol_explicit: bool,
    selection_symbol_style: Option<Style>,
    unfocused_selection_symbol_style: Option<Style>,
    gutter_gap: Option<u16>,
    item_horizontal_padding: Padding,
    header_horizontal_padding: Padding,
    separator_style: Option<Style>,
    header_style: Option<Style>,
    detail_style: Option<Style>,
    active_detail_style: StyleSlot,
    detail_gap: u16,
    disabled_style: Option<Style>,
    destructive_style: Option<Style>,
    icon_style: Option<Style>,
    submenu_indicator: Arc<str>,
    submenu_indicator_style: Option<Style>,
    submenu_delay: Duration,
    check_symbol: Arc<str>,
    check_style: Option<Style>,
    scrollbar: bool,
    scrollbar_config: ScrollbarConfig,
}

impl ContextMenu {
    /// Animate popup content using shared visibility timing and effects.
    /// Root-portal scope only; the trigger remains active while content closes.
    pub fn animation(mut self, animation: impl Into<Option<VisibilityAnimation>>) -> Self {
        self.animation = animation.into();
        self
    }

    /// Create a new context menu.
    pub fn new(trigger: impl IntoElement) -> Self {
        Self {
            trigger: trigger.into(),
            items: Vec::new(),
            entries: None,
            open: false,
            animation: None,
            on_select: None,
            on_action: None,
            on_close: None,
            placement: PopoverPlacement::BelowStart,
            offset: PopoverOffset::ZERO,
            clamp: true,
            auto_flip: true,
            anchor: None,
            width: None,
            min_width: 16,
            max_width: Some(48),
            height: Length::Auto,
            border: true,
            border_style: BorderStyle::Plain,
            padding: Padding::default(),
            style: Style::default(),
            selection_style: StyleSlot::Inherit,
            unfocused_selection_style: StyleSlot::Inherit,
            item_hover_style: None,
            selection_full_width: None,
            symbol_column: None,
            selection_symbol: Some("> ".into()),
            selection_symbol_explicit: false,
            selection_symbol_style: None,
            unfocused_selection_symbol_style: None,
            gutter_gap: None,
            item_horizontal_padding: Padding::default(),
            header_horizontal_padding: Padding::default(),
            separator_style: None,
            header_style: None,
            detail_style: None,
            active_detail_style: StyleSlot::Inherit,
            detail_gap: 2,
            disabled_style: None,
            destructive_style: None,
            icon_style: None,
            submenu_indicator: "›".into(),
            submenu_indicator_style: None,
            submenu_delay: Duration::ZERO,
            check_symbol: "✓".into(),
            check_style: None,
            scrollbar: false,
            scrollbar_config: ScrollbarConfig::default(),
        }
    }

    /// Replace all flat menu items.
    pub fn items(mut self, items: impl IntoIterator<Item = impl Into<ListItem>>) -> Self {
        self.items = items.into_iter().map(Into::into).collect();
        self.entries = None;
        self
    }

    /// Set structured menu entries with support for cascading submenus, separators, icons,
    /// radio checkmarks, details, disabled rows, and destructive actions.
    pub fn entries(
        mut self,
        entries: impl IntoIterator<Item = impl Into<ContextMenuEntry>>,
    ) -> Self {
        self.entries = Some(entries.into_iter().map(Into::into).collect());
        self
    }

    /// Set open state.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Set activation callback for flat [`items`](Self::items) (Enter or left click).
    pub fn on_select(mut self, cb: Callback<usize>) -> Self {
        self.on_select = Some(cb);
        self
    }

    /// Set action callback for structured [`entries`](Self::entries), receiving the activated
    /// leaf item's `id`.
    pub fn on_action(mut self, cb: Callback<Arc<str>>) -> Self {
        self.on_action = Some(cb);
        self
    }

    /// Set close callback.
    pub fn on_close(mut self, cb: Callback<()>) -> Self {
        self.on_close = Some(cb);
        self
    }

    /// Set popover placement.
    pub fn placement(mut self, placement: PopoverPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Set popover offset.
    pub fn offset(mut self, offset: impl Into<PopoverOffset>) -> Self {
        self.offset = offset.into();
        self
    }

    /// Clamp popover to the viewport bounds.
    pub fn clamp(mut self, clamp: bool) -> Self {
        self.clamp = clamp;
        self
    }

    /// Automatically flip placement when it overflows the viewport.
    pub fn auto_flip(mut self, auto_flip: bool) -> Self {
        self.auto_flip = auto_flip;
        self
    }

    /// Anchor the menu to an absolute position (content coordinates).
    pub fn anchor(mut self, anchor: Option<(u16, u16)>) -> Self {
        self.anchor = anchor;
        self
    }

    /// Set width. When unset, flat [`items`](Self::items) default to `Length::Px(20)` and
    /// structured [`entries`](Self::entries) auto-size to their content clamped to
    /// [`min_width`](Self::min_width)..=[`max_width`](Self::max_width).
    pub fn width(mut self, width: Length) -> Self {
        self.width = Some(width);
        self
    }

    /// Minimum auto-sized menu width in columns (structured entries mode).
    pub fn min_width(mut self, min_width: u16) -> Self {
        self.min_width = min_width;
        self
    }

    /// Maximum auto-sized menu width in columns (structured entries mode).
    pub fn max_width(mut self, max_width: u16) -> Self {
        self.max_width = Some(max_width);
        self
    }

    /// Set height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Draw a border.
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

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set highlight style.
    pub fn selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed highlight style.
    pub fn extend_selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed highlight style.
    pub fn inherit_selection_style(mut self) -> Self {
        self.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set highlight style slot directly for composite forwarding.
    pub fn selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.selection_style = slot;
        self
    }

    /// Set highlight style while the menu list is not focused.
    pub fn unfocused_selection_style(mut self, style: Style) -> Self {
        self.unfocused_selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed highlight style while the menu list is not focused.
    pub fn extend_unfocused_selection_style(mut self, style: Style) -> Self {
        self.unfocused_selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed highlight style while the menu list is not focused.
    pub fn inherit_unfocused_selection_style(mut self) -> Self {
        self.unfocused_selection_style = StyleSlot::Inherit;
        self
    }

    /// Set unfocused highlight style slot directly for composite forwarding.
    pub fn unfocused_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.unfocused_selection_style = slot;
        self
    }

    /// Set hovered item style.
    pub fn item_hover_style(mut self, style: Style) -> Self {
        self.item_hover_style = Some(StyleSlot::Replace(style));
        self
    }

    /// Extend the themed hovered item style.
    pub fn extend_item_hover_style(mut self, style: Style) -> Self {
        self.item_hover_style = Some(StyleSlot::Extend(style));
        self
    }

    /// Inherit the themed hovered item style.
    pub fn inherit_item_hover_style(mut self) -> Self {
        self.item_hover_style = Some(StyleSlot::Inherit);
        self
    }

    /// Set hovered item style slot directly for composite forwarding.
    pub fn item_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.item_hover_style = Some(slot);
        self
    }

    /// Whether the selection highlight fills the full row width.
    pub fn selection_full_width(mut self, full_width: bool) -> Self {
        self.selection_full_width = Some(full_width);
        self
    }

    /// Enable or disable the left selection-symbol column.
    pub fn symbol_column(mut self, enabled: bool) -> Self {
        self.symbol_column = Some(enabled);
        self
    }

    /// Set highlight symbol.
    pub fn selection_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.selection_symbol = symbol.map(Into::into);
        self.selection_symbol_explicit = true;
        self
    }

    /// Set highlight symbol style.
    pub fn selection_symbol_style(mut self, style: Style) -> Self {
        self.selection_symbol_style = Some(style);
        self
    }

    /// Set highlight symbol style while the menu list is not focused.
    pub fn unfocused_selection_symbol_style(mut self, style: Style) -> Self {
        self.unfocused_selection_symbol_style = Some(style);
        self
    }

    /// Gap in columns between an entry's leading icon/check gutter and its label.
    ///
    /// Defaults to `0` in flat `.items(...)` mode (matching [`List`]) and `1` in structured
    /// `.entries(...)` mode.
    pub fn gutter_gap(mut self, gap: u16) -> Self {
        self.gutter_gap = Some(gap);
        self
    }

    /// Left/right padding applied inside normal and disabled menu rows.
    pub fn item_horizontal_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.item_horizontal_padding = padding.into();
        self
    }

    /// Left/right padding applied to separator and header rows.
    pub fn header_horizontal_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.header_horizontal_padding = padding.into();
        self
    }

    /// Style applied to horizontal separator rules.
    pub fn separator_style(mut self, style: Style) -> Self {
        self.separator_style = Some(style);
        self
    }

    /// Style applied to section header rows.
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = Some(style);
        self
    }

    /// Style applied to right-aligned detail / shortcut text.
    pub fn detail_style(mut self, style: Style) -> Self {
        self.detail_style = Some(style);
        self
    }

    /// Detail and submenu indicator style on the highlighted entry, so they stay readable on the
    /// selection fill. Inherits [`detail_style`](Self::detail_style) by default.
    pub fn active_detail_style(mut self, style: Style) -> Self {
        self.active_detail_style = StyleSlot::Replace(style);
        self
    }

    /// Patch the highlighted entry's detail style over [`detail_style`](Self::detail_style).
    pub fn extend_active_detail_style(mut self, style: Style) -> Self {
        self.active_detail_style = StyleSlot::Extend(style);
        self
    }

    /// Set the highlighted entry's detail style slot.
    pub fn active_detail_style_slot(mut self, slot: StyleSlot) -> Self {
        self.active_detail_style = slot;
        self
    }

    /// Minimum blank columns between a label and its right-aligned detail or submenu indicator.
    pub fn detail_gap(mut self, gap: u16) -> Self {
        self.detail_gap = gap;
        self
    }

    /// Style applied to disabled entries.
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = Some(style);
        self
    }

    /// Style applied to destructive entries.
    pub fn destructive_style(mut self, style: Style) -> Self {
        self.destructive_style = Some(style);
        self
    }

    /// Default style applied to entry icons.
    pub fn icon_style(mut self, style: Style) -> Self {
        self.icon_style = Some(style);
        self
    }

    /// Trailing chevron drawn on submenu entries (default `"›"`).
    pub fn submenu_indicator(mut self, glyph: impl Into<Arc<str>>) -> Self {
        self.submenu_indicator = glyph.into();
        self
    }

    /// Style applied to the submenu indicator chevron.
    pub fn submenu_indicator_style(mut self, style: Style) -> Self {
        self.submenu_indicator_style = Some(style);
        self
    }

    /// How long the pointer must rest on a submenu entry before hover opens it.
    ///
    /// Zero (the default) opens on hover immediately. Leaving the row before the delay elapses
    /// cancels it, so sweeping across a submenu row toward another item opens nothing. Keyboard
    /// (`Right` / `Enter`) and clicks always open immediately.
    pub fn submenu_delay(mut self, delay: Duration) -> Self {
        self.submenu_delay = delay;
        self
    }

    /// Glyph drawn for checked entries (default `"✓"`).
    pub fn check_symbol(mut self, glyph: impl Into<Arc<str>>) -> Self {
        self.check_symbol = glyph.into();
        self
    }

    /// Style applied to the check glyph.
    pub fn check_style(mut self, style: Style) -> Self {
        self.check_style = Some(style);
        self
    }

    /// Enable a scrollbar.
    pub fn scrollbar(mut self, scrollbar: bool) -> Self {
        self.scrollbar = scrollbar;
        self
    }

    /// Set scrollbar configuration.
    pub fn scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.scrollbar_config = config;
        self
    }
}

impl From<ContextMenu> for Element {
    fn from(menu: ContextMenu) -> Self {
        let on_close = menu
            .on_close
            .clone()
            .unwrap_or_else(|| Callback::new(|_| {}));

        let is_structured = menu.entries.is_some();
        let content: Element = if let Some(entries) = menu.entries {
            let symbol_column = menu.symbol_column.unwrap_or(false);
            let selection_symbol = if menu.selection_symbol_explicit {
                menu.selection_symbol
            } else {
                None
            };
            let cfg = MenuRenderConfig {
                width: menu.width,
                min_width: menu.min_width,
                max_width: menu.max_width,
                height: menu.height,
                border: menu.border,
                border_style: menu.border_style,
                padding: menu.padding,
                style: menu.style,
                selection_style: menu.selection_style,
                unfocused_selection_style: menu.unfocused_selection_style,
                item_hover_style: menu.item_hover_style.unwrap_or(menu.selection_style),
                selection_full_width: menu.selection_full_width.unwrap_or(true),
                symbol_column,
                selection_symbol,
                selection_symbol_style: menu.selection_symbol_style,
                unfocused_selection_symbol_style: menu.unfocused_selection_symbol_style,
                gutter_gap: menu.gutter_gap.unwrap_or(1),
                item_horizontal_padding: menu.item_horizontal_padding,
                header_horizontal_padding: menu.header_horizontal_padding,
                separator_style: menu.separator_style,
                header_style: menu.header_style,
                detail_style: menu.detail_style,
                active_detail_style: menu.active_detail_style,
                detail_gap: menu.detail_gap,
                disabled_style: menu.disabled_style,
                destructive_style: menu.destructive_style,
                icon_style: menu.icon_style,
                submenu_indicator: menu.submenu_indicator,
                submenu_indicator_style: menu.submenu_indicator_style,
                submenu_delay: menu.submenu_delay,
                check_symbol: menu.check_symbol,
                check_style: menu.check_style,
                scrollbar: menu.scrollbar,
                scrollbar_config: menu.scrollbar_config,
            };
            crate::child(
                || MenuStack,
                MenuStackProps {
                    entries: Rc::new(entries),
                    cfg: Rc::new(cfg),
                    on_action: menu.on_action,
                    on_close: Some(on_close.clone()),
                },
            )
        } else {
            let mut list = List::new()
                .items(menu.items)
                .border(menu.border)
                .border_style(menu.border_style)
                .padding(menu.padding)
                .style(menu.style)
                .selection_full_width(menu.selection_full_width.unwrap_or(false))
                .symbol_column(menu.symbol_column.unwrap_or(true))
                .gutter_gap(menu.gutter_gap.unwrap_or(0))
                .item_horizontal_padding(menu.item_horizontal_padding)
                .header_horizontal_padding(menu.header_horizontal_padding)
                .selection_symbol_style(
                    menu.selection_symbol_style
                        .unwrap_or(menu.selection_style.explicit_style().unwrap_or_default()),
                )
                .unfocused_selection_symbol_style(
                    menu.unfocused_selection_symbol_style
                        .or_else(|| menu.unfocused_selection_style.explicit_style())
                        .unwrap_or(menu.selection_style.explicit_style().unwrap_or_default()),
                )
                .selection_symbol(menu.selection_symbol)
                .scrollbar(menu.scrollbar)
                .scrollbar_config(menu.scrollbar_config)
                .width(menu.width.unwrap_or(Length::Px(20)))
                .height(menu.height);
            list = list
                .selection_style_slot(menu.selection_style)
                .unfocused_selection_style_slot(menu.unfocused_selection_style)
                .item_hover_style_slot(menu.item_hover_style.unwrap_or(menu.selection_style));

            if let Some(cb) = menu.on_select {
                let activate_cb = cb.clone();
                list = list
                    .activate_on_click(true)
                    .on_activate(Callback::new(move |ev: crate::widgets::ListEvent| {
                        activate_cb.emit(ev.index);
                    }))
                    .on_context_menu(Callback::new(
                        move |(ev, _): (
                            crate::widgets::ListEvent,
                            crate::core::event::MouseEvent,
                        )| {
                            cb.emit(ev.index);
                        },
                    ));
            }
            let close_on_chrome = on_close.clone();
            crate::widgets::MouseRegion::new()
                .on_context_menu(Callback::new(move |_| close_on_chrome.emit(())))
                .child(crate::child(|| MenuList, MenuListProps(Rc::new(list))))
                .into()
        };

        Popover::new()
            .animation(menu.animation)
            .trigger(menu.trigger)
            .content(content)
            .open(menu.open)
            .placement(menu.placement)
            .offset(menu.offset)
            .clamp(menu.clamp)
            .auto_flip(menu.auto_flip)
            .min_trigger_width(false)
            .anchor(menu.anchor)
            .close_on_escape(!is_structured)
            .on_close(on_close)
            .into()
    }
}

// Keep navigation state across parent renders. List itself is controlled.
#[derive(Clone)]
struct MenuListProps(Rc<List>);
impl PartialEq for MenuListProps {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
struct MenuList;
impl crate::Component for MenuList {
    type Properties = MenuListProps;
    type State = Option<usize>;
    type Message = usize;
    fn create_state(&self, props: &Self::Properties) -> Self::State {
        List::nearest_selectable_index(&props.0.items, 0)
    }
    fn on_props_changed(
        &mut self,
        old: &Self::Properties,
        ctx: &mut crate::Context<Self>,
    ) -> crate::Update {
        if old.0.items != ctx.props.0.items {
            ctx.state = List::nearest_selectable_index(&ctx.props.0.items, ctx.state.unwrap_or(0));
        }
        crate::Update::render()
    }
    fn update(&mut self, selected: usize, ctx: &mut crate::Context<Self>) -> crate::Update {
        ctx.state = Some(selected);
        crate::Update::render()
    }
    fn view(&self, ctx: &crate::Context<Self>) -> Element {
        ctx.props
            .0
            .as_ref()
            .clone()
            .selected(ctx.state)
            .on_select(
                ctx.link()
                    .callback(|event: crate::widgets::ListEvent| event.index),
            )
            .into()
    }
}

#[derive(Clone)]
struct MenuRenderConfig {
    width: Option<Length>,
    min_width: u16,
    max_width: Option<u16>,
    height: Length,
    border: bool,
    border_style: BorderStyle,
    padding: Padding,
    style: Style,
    selection_style: StyleSlot,
    unfocused_selection_style: StyleSlot,
    item_hover_style: StyleSlot,
    selection_full_width: bool,
    symbol_column: bool,
    selection_symbol: Option<Arc<str>>,
    selection_symbol_style: Option<Style>,
    unfocused_selection_symbol_style: Option<Style>,
    gutter_gap: u16,
    item_horizontal_padding: Padding,
    header_horizontal_padding: Padding,
    separator_style: Option<Style>,
    header_style: Option<Style>,
    detail_style: Option<Style>,
    active_detail_style: StyleSlot,
    detail_gap: u16,
    disabled_style: Option<Style>,
    destructive_style: Option<Style>,
    icon_style: Option<Style>,
    submenu_indicator: Arc<str>,
    submenu_indicator_style: Option<Style>,
    submenu_delay: Duration,
    check_symbol: Arc<str>,
    check_style: Option<Style>,
    scrollbar: bool,
    scrollbar_config: ScrollbarConfig,
}

#[derive(Clone)]
struct MenuStackProps {
    entries: Rc<Vec<ContextMenuEntry>>,
    cfg: Rc<MenuRenderConfig>,
    on_action: Option<Callback<Arc<str>>>,
    on_close: Option<Callback<()>>,
}

impl PartialEq for MenuStackProps {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
            && Rc::ptr_eq(&self.cfg, &other.cfg)
            && self.on_action == other.on_action
            && self.on_close == other.on_close
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct LevelState {
    selected: Option<usize>,
    open_child: Option<usize>,
}

#[derive(Clone, Debug, Default)]
struct MenuStackState {
    levels: Vec<LevelState>,
    /// Submenu `(depth, index)` currently waiting on [`MenuRenderConfig::submenu_delay`].
    pending_hover: Option<(usize, usize)>,
    hover_reveal: HoverReveal,
}

#[derive(Clone, Debug)]
enum MenuStackMsg {
    Select {
        depth: usize,
        index: usize,
    },
    Hover {
        depth: usize,
        index: usize,
        hovered: bool,
    },
    HoverReveal {
        depth: usize,
        index: usize,
        generation: u64,
    },
    Activate {
        depth: usize,
        index: usize,
    },
    OpenSubmenu {
        depth: usize,
    },
    CloseSubmenuFrom {
        depth: usize,
    },
    CloseDeepest {
        close_root: bool,
    },
    Scrolled {
        depth: usize,
    },
    CloseAll,
}

struct MenuStack;

fn level_key(depth: usize) -> String {
    format!("__tui_lipan_context_menu_level_{depth}")
}

fn first_selectable(entries: &[ContextMenuEntry]) -> Option<usize> {
    entries.iter().position(ContextMenuEntry::is_selectable)
}

fn entries_at_depth<'a>(
    root: &'a [ContextMenuEntry],
    levels: &[LevelState],
    depth: usize,
) -> Option<&'a [ContextMenuEntry]> {
    let mut current = root;
    for level in levels.iter().take(depth) {
        let idx = level.open_child?;
        match current.get(idx)? {
            ContextMenuEntry::Submenu(sub) if !sub.disabled && !sub.children.is_empty() => {
                current = &sub.children;
            }
            _ => return None,
        }
    }
    Some(current)
}

impl crate::Component for MenuStack {
    type Properties = MenuStackProps;
    type State = MenuStackState;
    type Message = MenuStackMsg;

    fn create_state(&self, props: &Self::Properties) -> Self::State {
        MenuStackState {
            levels: vec![LevelState {
                selected: first_selectable(&props.entries),
                open_child: None,
            }],
            pending_hover: None,
            hover_reveal: HoverReveal::default(),
        }
    }

    fn on_props_changed(
        &mut self,
        old: &Self::Properties,
        ctx: &mut crate::Context<Self>,
    ) -> crate::Update {
        if old.entries != ctx.props.entries {
            ctx.state.levels = vec![LevelState {
                selected: first_selectable(&ctx.props.entries),
                open_child: None,
            }];
            ctx.state.pending_hover = None;
            let _ = ctx.state.hover_reveal.reset();
            return crate::Update::render();
        }
        if old.cfg.submenu_delay != ctx.props.cfg.submenu_delay
            && let Some((depth, index)) = ctx.state.pending_hover
        {
            let delay = ctx.props.cfg.submenu_delay;
            let env = ctx.env().clone();
            return match ctx.state.hover_reveal.retime(delay, &env) {
                RevealStep::Nothing => crate::Update::render(),
                RevealStep::Render => {
                    ctx.state.pending_hover = None;
                    open_submenu_on_hover(ctx, depth, index);
                    crate::Update::render()
                }
                RevealStep::Arm { wait, generation } => {
                    let cmd = ctx.link().command_after(wait, move |link| {
                        link.send(MenuStackMsg::HoverReveal {
                            depth,
                            index,
                            generation,
                        });
                    });
                    crate::Update::render_with_command(cmd)
                }
            };
        }
        crate::Update::render()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut crate::Context<Self>) -> crate::Update {
        match msg {
            MenuStackMsg::Select { depth, index } => {
                cancel_pending_hover(&mut ctx.state);
                if let Some(level) = ctx.state.levels.get_mut(depth) {
                    level.selected = Some(index);
                    if level.open_child.is_some() && level.open_child != Some(index) {
                        level.open_child = None;
                        ctx.state.levels.truncate(depth + 1);
                    }
                }
                crate::Update::render()
            }
            MenuStackMsg::Hover {
                depth,
                index,
                hovered,
            } => {
                if !hovered {
                    if ctx.state.pending_hover == Some((depth, index)) {
                        cancel_pending_hover(&mut ctx.state);
                    }
                    return crate::Update::none();
                }
                let Some(entries) = entries_at_depth(&ctx.props.entries, &ctx.state.levels, depth)
                else {
                    return crate::Update::none();
                };
                let Some(entry) = entries.get(index) else {
                    return crate::Update::none();
                };
                if !entry.is_selectable() {
                    return crate::Update::none();
                }
                let mut changed = false;
                if let Some(level) = ctx.state.levels.get_mut(depth)
                    && level.selected != Some(index)
                {
                    level.selected = Some(index);
                    changed = true;
                }
                match entry {
                    ContextMenuEntry::Submenu(sub) if !sub.disabled && !sub.children.is_empty() => {
                        let already_open = ctx
                            .state
                            .levels
                            .get(depth)
                            .is_some_and(|l| l.open_child == Some(index));
                        if already_open {
                            cancel_pending_hover(&mut ctx.state);
                        } else if ctx.state.pending_hover != Some((depth, index)) {
                            cancel_pending_hover(&mut ctx.state);
                            ctx.state.pending_hover = Some((depth, index));
                            let timing = HoverDelay::fixed(ctx.props.cfg.submenu_delay);
                            let env = ctx.env().clone();
                            match ctx.state.hover_reveal.hover(true, &timing, &env) {
                                RevealStep::Nothing => {}
                                RevealStep::Render => {
                                    ctx.state.pending_hover = None;
                                    open_submenu_on_hover(ctx, depth, index);
                                    changed = true;
                                }
                                RevealStep::Arm { wait, generation } => {
                                    let cmd = ctx.link().command_after(wait, move |link| {
                                        link.send(MenuStackMsg::HoverReveal {
                                            depth,
                                            index,
                                            generation,
                                        });
                                    });
                                    return if changed {
                                        crate::Update::render_with_command(cmd)
                                    } else {
                                        crate::Update::command_only(cmd)
                                    };
                                }
                            }
                        }
                    }
                    _ => {
                        cancel_pending_hover(&mut ctx.state);
                        if let Some(level) = ctx.state.levels.get_mut(depth)
                            && level.open_child.is_some()
                        {
                            level.open_child = None;
                            ctx.state.levels.truncate(depth + 1);
                            changed = true;
                        }
                    }
                }
                if changed {
                    crate::Update::render()
                } else {
                    crate::Update::none()
                }
            }
            MenuStackMsg::HoverReveal {
                depth,
                index,
                generation,
            } => {
                if ctx.state.pending_hover != Some((depth, index))
                    || !ctx.state.hover_reveal.reveal(generation)
                {
                    return crate::Update::none();
                }
                ctx.state.pending_hover = None;
                if open_submenu_on_hover(ctx, depth, index) {
                    crate::Update::render()
                } else {
                    crate::Update::none()
                }
            }
            MenuStackMsg::Activate { depth, index } => {
                cancel_pending_hover(&mut ctx.state);
                let Some(entries) = entries_at_depth(&ctx.props.entries, &ctx.state.levels, depth)
                else {
                    return crate::Update::none();
                };
                let Some(entry) = entries.get(index).cloned() else {
                    return crate::Update::none();
                };
                match entry {
                    ContextMenuEntry::Action(item) if !item.disabled => {
                        if let Some(on_action) = &ctx.props.on_action {
                            on_action.emit(item.id);
                        }
                        if let Some(on_close) = &ctx.props.on_close {
                            on_close.emit(());
                        }
                        crate::Update::none()
                    }
                    ContextMenuEntry::Submenu(sub) if !sub.disabled && !sub.children.is_empty() => {
                        open_submenu_and_focus(ctx, depth, index, &sub.children);
                        crate::Update::render()
                    }
                    _ => crate::Update::none(),
                }
            }
            MenuStackMsg::OpenSubmenu { depth } => {
                cancel_pending_hover(&mut ctx.state);
                let selected = ctx.state.levels.get(depth).and_then(|level| level.selected);
                let Some(index) = selected else {
                    return crate::Update::none();
                };
                let Some(entries) = entries_at_depth(&ctx.props.entries, &ctx.state.levels, depth)
                else {
                    return crate::Update::none();
                };
                let Some(ContextMenuEntry::Submenu(sub)) = entries.get(index).cloned() else {
                    return crate::Update::none();
                };
                if sub.disabled || sub.children.is_empty() {
                    return crate::Update::none();
                }
                open_submenu_and_focus(ctx, depth, index, &sub.children);
                crate::Update::render()
            }
            MenuStackMsg::CloseSubmenuFrom { depth } => {
                cancel_pending_hover(&mut ctx.state);
                let had_open = ctx
                    .state
                    .levels
                    .get(depth)
                    .is_some_and(|level| level.open_child.is_some());
                if !had_open {
                    return crate::Update::none();
                }
                ctx.state.levels.truncate(depth + 1);
                if let Some(parent) = ctx.state.levels.get_mut(depth) {
                    parent.open_child = None;
                }
                ctx.request_focus(level_key(depth));
                crate::Update::render()
            }
            MenuStackMsg::CloseDeepest { close_root } => {
                cancel_pending_hover(&mut ctx.state);
                if ctx.state.levels.len() > 1 {
                    let parent_depth = ctx.state.levels.len() - 2;
                    ctx.state.levels.truncate(parent_depth + 1);
                    if let Some(parent) = ctx.state.levels.get_mut(parent_depth) {
                        parent.open_child = None;
                    }
                    ctx.request_focus(level_key(parent_depth));
                    crate::Update::render()
                } else if close_root {
                    if let Some(on_close) = &ctx.props.on_close {
                        on_close.emit(());
                    }
                    crate::Update::none()
                } else {
                    crate::Update::none()
                }
            }
            MenuStackMsg::Scrolled { depth } => {
                if ctx
                    .state
                    .levels
                    .get(depth)
                    .is_some_and(|level| level.open_child.is_some())
                {
                    crate::Update::render()
                } else {
                    crate::Update::none()
                }
            }
            MenuStackMsg::CloseAll => {
                cancel_pending_hover(&mut ctx.state);
                if let Some(on_close) = &ctx.props.on_close {
                    on_close.emit(());
                }
                crate::Update::none()
            }
        }
    }

    fn view(&self, ctx: &crate::Context<Self>) -> Element {
        let theme = ctx.theme_ref();
        render_menu_level(ctx, &theme, &ctx.props.entries, 0)
    }
}

fn cancel_pending_hover(state: &mut MenuStackState) {
    state.pending_hover = None;
    let _ = state.hover_reveal.reset();
}

fn open_submenu_on_hover(ctx: &mut crate::Context<MenuStack>, depth: usize, index: usize) -> bool {
    let Some(entries) = entries_at_depth(&ctx.props.entries, &ctx.state.levels, depth) else {
        return false;
    };
    let Some(ContextMenuEntry::Submenu(sub)) = entries.get(index) else {
        return false;
    };
    if sub.disabled || sub.children.is_empty() {
        return false;
    }
    let child_selected = first_selectable(&sub.children);
    if let Some(level) = ctx.state.levels.get_mut(depth) {
        level.open_child = Some(index);
    }
    ctx.state.levels.truncate(depth + 1);
    ctx.state.levels.push(LevelState {
        selected: child_selected,
        open_child: None,
    });
    true
}

fn open_submenu_and_focus(
    ctx: &mut crate::Context<MenuStack>,
    depth: usize,
    index: usize,
    children: &[ContextMenuEntry],
) {
    if let Some(level) = ctx.state.levels.get_mut(depth) {
        level.selected = Some(index);
        level.open_child = Some(index);
    }
    ctx.state.levels.truncate(depth + 1);
    ctx.state.levels.push(LevelState {
        selected: first_selectable(children),
        open_child: None,
    });
    ctx.request_focus(level_key(depth + 1));
}

fn render_menu_level(
    ctx: &crate::Context<MenuStack>,
    theme: &crate::style::Theme,
    entries: &[ContextMenuEntry],
    depth: usize,
) -> Element {
    let cfg = &ctx.props.cfg;
    let level_state = ctx.state.levels.get(depth);
    let selected = level_state.and_then(|level| level.selected);
    let open_child = level_state.and_then(|level| level.open_child);

    let has_gutter = entries.iter().any(|entry| match entry {
        ContextMenuEntry::Action(item) => item.checked.is_some() || item.icon.is_some(),
        ContextMenuEntry::Submenu(sub) => sub.icon.is_some(),
        ContextMenuEntry::Separator | ContextMenuEntry::Header(_) => false,
    });
    let gutter_left_pad = if has_gutter {
        cfg.item_horizontal_padding.left
    } else {
        0
    };
    let list_item_h_pad = if has_gutter {
        Padding {
            left: 0,
            ..cfg.item_horizontal_padding
        }
    } else {
        cfg.item_horizontal_padding
    };

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            entry_to_list_item(entry, cfg, theme, gutter_left_pad, selected == Some(index))
        })
        .collect();

    let level_width = resolve_level_width(entries, cfg);
    let width_length = match cfg.width {
        Some(length) if depth == 0 => length,
        _ => Length::Px(level_width),
    };

    let link_open = ctx.link().clone();
    let link_close = ctx.link().clone();
    let link_tab = ctx.link().clone();
    let on_key = KeyHandler::new(move |key| match key.code {
        KeyCode::Right => {
            link_open.send(MenuStackMsg::OpenSubmenu { depth });
            true
        }
        KeyCode::Left => {
            link_close.send(MenuStackMsg::CloseDeepest { close_root: false });
            true
        }
        KeyCode::Esc => {
            link_close.send(MenuStackMsg::CloseDeepest { close_root: true });
            true
        }
        KeyCode::Tab => {
            link_tab.send(MenuStackMsg::CloseAll);
            true
        }
        _ => false,
    });

    let mut list = List::new()
        .items(items)
        .selected(selected)
        .border(cfg.border)
        .border_style(cfg.border_style)
        .padding(cfg.padding)
        .style(cfg.style)
        .selection_full_width(cfg.selection_full_width)
        .symbol_column(cfg.symbol_column)
        .gutter_gap(cfg.gutter_gap)
        .item_horizontal_padding(list_item_h_pad)
        .header_horizontal_padding(cfg.header_horizontal_padding)
        .selection_symbol(cfg.selection_symbol.clone())
        .selection_symbol_style(
            cfg.selection_symbol_style
                .unwrap_or(cfg.selection_style.explicit_style().unwrap_or_default()),
        )
        .unfocused_selection_symbol_style(
            cfg.unfocused_selection_symbol_style
                .or_else(|| cfg.unfocused_selection_style.explicit_style())
                .unwrap_or(cfg.selection_style.explicit_style().unwrap_or_default()),
        )
        .scrollbar(cfg.scrollbar)
        .scrollbar_config(cfg.scrollbar_config.clone())
        .width(width_length)
        .height(cfg.height)
        .activate_on_click(true)
        .on_select(
            ctx.link()
                .callback(move |ev: crate::widgets::ListEvent| MenuStackMsg::Select {
                    depth,
                    index: ev.index,
                }),
        )
        .on_hover(
            ctx.link()
                .callback(move |ev: ListHoverEvent| MenuStackMsg::Hover {
                    depth,
                    index: ev.index,
                    hovered: ev.hovered,
                }),
        )
        .on_activate(ctx.link().callback(move |ev: crate::widgets::ListEvent| {
            MenuStackMsg::Activate {
                depth,
                index: ev.index,
            }
        }))
        .on_context_menu(ctx.link().callback(
            move |(ev, _): (crate::widgets::ListEvent, crate::core::event::MouseEvent)| {
                MenuStackMsg::Activate {
                    depth,
                    index: ev.index,
                }
            },
        ))
        .on_scroll_to(
            ctx.link()
                .callback(move |_| MenuStackMsg::Scrolled { depth }),
        )
        .on_key(on_key);
    list = list
        .selection_style_slot(cfg.selection_style)
        .unfocused_selection_style_slot(cfg.unfocused_selection_style)
        .item_hover_style_slot(cfg.item_hover_style);

    let list_el: Element = crate::widgets::MouseRegion::new()
        .on_context_menu(ctx.link().callback(|_| MenuStackMsg::CloseAll))
        .child(list.key(level_key(depth)))
        .into();

    let open_sub = open_child.and_then(|child_idx| match entries.get(child_idx) {
        Some(ContextMenuEntry::Submenu(sub)) if !sub.disabled && !sub.children.is_empty() => {
            Some((child_idx, sub))
        }
        _ => None,
    });

    let top_inset = u16::from(cfg.border).saturating_add(cfg.padding.top);
    let (submenu_open, anchor_idx, child_el) = match open_sub {
        Some((child_idx, sub)) => (
            true,
            Some(child_idx),
            render_menu_level(ctx, theme, &sub.children, depth + 1),
        ),
        None => (false, None, crate::widgets::Spacer::new().into()),
    };

    Popover::new()
        .trigger(list_el)
        .content(child_el)
        .open(submenu_open)
        .anchor_list_item(anchor_idx)
        .on_anchor_hidden(
            ctx.link()
                .callback(move |_| MenuStackMsg::CloseSubmenuFrom { depth }),
        )
        .placement(PopoverPlacement::RightStart)
        .offset((0, -(top_inset as i16)))
        .auto_flip(true)
        .clamp(true)
        .min_trigger_width(false)
        .capture_focus(false)
        .auto_focus(false)
        .capture_pointer(true)
        .into()
}

fn gutter_with_left_pad(left_pad: u16, span: Span) -> ListItemGutter {
    if left_pad == 0 {
        ListItemGutter::from_spans([span])
    } else {
        ListItemGutter::from_spans([Span::new(" ".repeat(usize::from(left_pad))), span])
    }
}

fn entry_to_list_item(
    entry: &ContextMenuEntry,
    cfg: &MenuRenderConfig,
    theme: &crate::style::Theme,
    gutter_left_pad: u16,
    active: bool,
) -> ListItem {
    let muted_style = cfg.disabled_style.unwrap_or(Style {
        bg: None,
        ..theme.muted
    });
    let icon_base = cfg.icon_style.unwrap_or(Style {
        bg: None,
        ..theme.muted
    });
    let detail_style = cfg.detail_style.unwrap_or(Style {
        bg: None,
        ..theme.muted
    });
    let (detail_style, active_detail) = match (active, cfg.active_detail_style) {
        (false, _) | (true, StyleSlot::Inherit) => (detail_style, None),
        (true, StyleSlot::Extend(style)) => {
            let style = detail_style.patch(style);
            (style, Some(style))
        }
        (true, StyleSlot::Replace(style)) => (style, Some(style)),
    };
    let destructive_style = cfg
        .destructive_style
        .unwrap_or(Style::new().fg(theme.status.error));

    match entry {
        ContextMenuEntry::Separator => {
            let rule_style = cfg.separator_style.unwrap_or(theme.border);
            ListItem::divider("").rule(rule_style)
        }
        ContextMenuEntry::Header(label) => {
            let header_style = cfg.header_style.unwrap_or(muted_style);
            ListItem::header(label.clone()).style(header_style)
        }
        ContextMenuEntry::Action(item) => {
            let base_label_style = if item.disabled {
                muted_style
            } else if let Some(style) = item.style {
                style
            } else if item.destructive {
                destructive_style
            } else {
                Style {
                    bg: None,
                    ..theme.primary
                }
            };
            let label_span = if item.destructive && !item.disabled {
                Span::new(item.label.clone())
                    .style(base_label_style)
                    .row_style_policy(RowStylePolicy::PreserveForeground)
            } else {
                Span::new(item.label.clone()).style(base_label_style)
            };
            let mut list_item = ListItem::from_spans([label_span]);
            if item.disabled {
                list_item = list_item.disabled(true);
            }
            if let Some(checked) = item.checked {
                let check_style = if item.disabled {
                    muted_style
                } else {
                    cfg.check_style.unwrap_or(base_label_style)
                };
                let glyph = if checked {
                    cfg.check_symbol.clone()
                } else {
                    Arc::from(" ".repeat(UnicodeWidthStr::width(cfg.check_symbol.as_ref()).max(1)))
                };
                list_item = list_item.gutter(gutter_with_left_pad(
                    gutter_left_pad,
                    Span::new(glyph)
                        .style(check_style)
                        .row_style_policy(RowStylePolicy::PreserveForeground),
                ));
            } else if let Some(icon) = &item.icon {
                let default_icon_style = if item.disabled {
                    muted_style
                } else if item.destructive {
                    destructive_style
                } else {
                    icon_base
                };
                let style = if icon.style == Style::default() {
                    default_icon_style
                } else {
                    icon.style
                };
                list_item = list_item.gutter(gutter_with_left_pad(
                    gutter_left_pad,
                    Span::new(icon.content.clone())
                        .style(style)
                        .row_style_policy(RowStylePolicy::PreserveForeground),
                ));
            }
            if let Some(detail) = &item.detail {
                let style = if item.disabled {
                    muted_style
                } else {
                    detail_style
                };
                list_item = list_item
                    .description_spans([Span::new(detail.clone())
                        .style(style)
                        .row_style_policy(RowStylePolicy::PreserveForeground)])
                    .primary_description_gap(cfg.detail_gap);
            }
            list_item
        }
        ContextMenuEntry::Submenu(sub) => {
            let disabled = sub.disabled || sub.children.is_empty();
            let label_style = if disabled {
                muted_style
            } else {
                Style {
                    bg: None,
                    ..theme.primary
                }
            };
            let chevron_style = if disabled {
                muted_style
            } else {
                active_detail.unwrap_or(cfg.submenu_indicator_style.unwrap_or(detail_style))
            };
            let mut list_item =
                ListItem::from_spans([Span::new(sub.label.clone()).style(label_style)])
                    .description_spans([Span::new(cfg.submenu_indicator.clone())
                        .style(chevron_style)
                        .row_style_policy(RowStylePolicy::PreserveForeground)])
                    .primary_description_gap(cfg.detail_gap);
            if disabled {
                list_item = list_item.disabled(true);
            }
            if let Some(icon) = &sub.icon {
                let default_icon_style = if disabled { muted_style } else { icon_base };
                let style = if icon.style == Style::default() {
                    default_icon_style
                } else {
                    icon.style
                };
                list_item = list_item.gutter(gutter_with_left_pad(
                    gutter_left_pad,
                    Span::new(icon.content.clone())
                        .style(style)
                        .row_style_policy(RowStylePolicy::PreserveForeground),
                ));
            }
            list_item
        }
    }
}

fn resolve_level_width(entries: &[ContextMenuEntry], cfg: &MenuRenderConfig) -> u16 {
    let check_w = UnicodeWidthStr::width(cfg.check_symbol.as_ref()).max(1);
    let chevron_w = UnicodeWidthStr::width(cfg.submenu_indicator.as_ref());
    let gutter_w = entries
        .iter()
        .map(|entry| match entry {
            ContextMenuEntry::Action(item) => {
                if item.checked.is_some() {
                    check_w
                } else if let Some(icon) = &item.icon {
                    UnicodeWidthStr::width(icon.content.as_ref())
                } else {
                    0
                }
            }
            ContextMenuEntry::Submenu(sub) => sub
                .icon
                .as_ref()
                .map_or(0, |icon| UnicodeWidthStr::width(icon.content.as_ref())),
            ContextMenuEntry::Separator | ContextMenuEntry::Header(_) => 0,
        })
        .max()
        .unwrap_or(0);
    let gutter_total = if gutter_w > 0 {
        gutter_w + usize::from(cfg.gutter_gap)
    } else {
        0
    };
    let symbol_w = if cfg.symbol_column {
        cfg.selection_symbol
            .as_deref()
            .map_or(0, UnicodeWidthStr::width)
    } else {
        0
    };

    let row_pad = usize::from(cfg.item_horizontal_padding.horizontal());
    let header_pad = usize::from(cfg.header_horizontal_padding.horizontal());
    let mut content_w = 0usize;
    for entry in entries {
        let w = match entry {
            ContextMenuEntry::Separator => 0,
            ContextMenuEntry::Header(label) => header_pad + UnicodeWidthStr::width(label.as_ref()),
            ContextMenuEntry::Action(item) => {
                let label_w = UnicodeWidthStr::width(item.label.as_ref());
                let detail_w = item.detail.as_deref().map_or(0, |detail| {
                    usize::from(cfg.detail_gap) + UnicodeWidthStr::width(detail)
                });
                row_pad + symbol_w + gutter_total + label_w + detail_w
            }
            ContextMenuEntry::Submenu(sub) => {
                let label_w = UnicodeWidthStr::width(sub.label.as_ref());
                row_pad
                    + symbol_w
                    + gutter_total
                    + label_w
                    + usize::from(cfg.detail_gap)
                    + chevron_w
            }
        };
        content_w = content_w.max(w);
    }

    let chrome = usize::from(cfg.padding.horizontal()) + if cfg.border { 2 } else { 0 };
    let total = u16::try_from(content_w.saturating_add(chrome)).unwrap_or(u16::MAX);
    let clamped = total.max(cfg.min_width);
    cfg.max_width.map_or(clamped, |max| clamped.min(max))
}

/// Non-emitting target resolution shared by document/editor picking and dispatch.
pub(crate) enum ContextMenuHit<T> {
    Widget(T),
    #[cfg(feature = "diff-view")]
    DiffSource(usize),
}

/// Emit a secondary-click event only when a semantic target was hit.
pub(crate) fn emit_context_menu<T>(
    callback: Option<&crate::callback::Callback<(T, crate::core::event::MouseEvent)>>,
    mouse: crate::core::event::MouseEvent,
    target: impl FnOnce() -> Option<T>,
) -> bool {
    let Some(callback) = callback else {
        return false;
    };
    let Some(target) = target() else {
        return false;
    };
    callback.emit((target, mouse));
    true
}

/// Emit a coordinate-only context menu within a widget's content viewport.
pub(crate) fn emit_mouse_context_menu(
    callback: Option<&crate::callback::Callback<crate::core::event::MouseEvent>>,
    mouse: crate::core::event::MouseEvent,
    content: crate::style::Rect,
) -> bool {
    let Some(callback) = callback else {
        return false;
    };
    if !content.contains(mouse.x as i16, mouse.y as i16) {
        return false;
    }
    callback.emit(mouse);
    true
}
