//! Select widget.

use crate::animation::VisibilityAnimation;
use std::rc::Rc;
use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::Element;
use crate::core::event::{KeyCode, KeyEvent, MouseEvent};
use crate::style::{Align, BorderStyle, Length, Padding, ScrollbarConfig, Style, StyleSlot};
use crate::widgets::button::ButtonVariant;
use crate::widgets::{
    Button, List, ListConfig, ListEvent, ListItem, Popover, PopoverPlacement, ZStack,
};

/// Selection event emitted by [`Select`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct SelectEvent {
    /// Selected option index.
    pub index: usize,
    /// Selected option label.
    pub label: Arc<str>,
}

/// A dropdown select widget (expands in-place).
#[derive(Clone)]
pub struct Select {
    pub(crate) options: Vec<Arc<str>>,
    pub(crate) selected: Option<usize>,
    pub(crate) placeholder: Arc<str>,
    pub(crate) expanded: bool,
    pub(crate) animation: Option<VisibilityAnimation>,
    pub(crate) on_toggle: Option<Callback<bool>>,
    pub(crate) on_select: Option<Callback<SelectEvent>>,
    pub(crate) width: Length,
    pub(crate) disabled: bool,
    pub(crate) button_variant: ButtonVariant,
    pub(crate) button_padding: Option<Padding>,
    pub(crate) button_align: Option<Align>,
    pub(crate) button_style: Style,
    pub(crate) button_hover_style: StyleSlot,
    pub(crate) button_focus_style: StyleSlot,
    pub(crate) button_disabled_style: Style,
    pub(crate) button_border_style: BorderStyle,
    pub(crate) button_hover_border_style: Option<BorderStyle>,
    pub(crate) button_focus_border_style: Option<BorderStyle>,
    pub(crate) button_open_suffix: Option<Arc<str>>,
    pub(crate) button_closed_suffix: Option<Arc<str>>,
    pub(crate) button_suffix_style: Style,
    pub(crate) list_title: Option<Arc<str>>,
    pub(crate) list_title_style: Style,
    pub(crate) list_config: ListConfig,

    pub(crate) list_width: Option<Length>,
    pub(crate) list_height: Length,
    pub(crate) match_button_width: bool,
    pub(crate) list_empty_text: Option<Arc<str>>,
    pub(crate) list_disabled_style: Style,
    pub(crate) focusable: bool,
    pub(crate) tab_stop: bool,
    pub(crate) on_focus: Option<Callback<()>>,
    pub(crate) on_blur: Option<Callback<()>>,
    pub(crate) on_key: Option<KeyHandler>,
}

impl Default for Select {
    fn default() -> Self {
        Self {
            options: Vec::new(),
            selected: None,
            placeholder: "Select...".into(),
            expanded: false,
            animation: None,
            on_toggle: None,
            on_select: None,
            width: Length::Auto,
            disabled: false,
            button_variant: ButtonVariant::Outlined,
            button_padding: None,
            button_align: None,
            button_style: Style::default(),
            button_hover_style: StyleSlot::Inherit,
            button_focus_style: StyleSlot::Inherit,
            button_disabled_style: Style::default(),
            button_border_style: BorderStyle::Plain,
            button_hover_border_style: None,
            button_focus_border_style: None,
            button_open_suffix: None,
            button_closed_suffix: None,
            button_suffix_style: Style::default(),
            list_title: None,
            list_title_style: Style::default(),
            list_config: ListConfig {
                border: true,
                border_style: BorderStyle::Plain,
                padding: Padding::default(),
                style: Style::default(),
                selection_style: StyleSlot::Inherit,
                unfocused_selection_style: StyleSlot::Inherit,
                selection_full_width: false,
                selection_symbol: Some("> ".into()),
                selection_symbol_right: None,
                selection_symbol_style: None,
                unfocused_selection_symbol_style: None,
                symbol_column: true,
                gutter_gap: 0,
                gutter_for_non_selectable: false,
                item_horizontal_padding: Padding::default(),
                header_horizontal_padding: Padding::default(),
                empty_text_style: Style::default(),
                empty_text_padding: Padding::default(),
                item_hover_style: None,
                scrollbar: false,
                scrollbar_config: ScrollbarConfig::default(),
            },
            list_width: None,
            list_height: Length::Px(6),
            match_button_width: false,
            list_empty_text: None,
            list_disabled_style: Style::default(),
            focusable: true,
            tab_stop: true,
            on_focus: None,
            on_blur: None,
            on_key: None,
        }
    }
}

impl Select {
    /// Animate popup content using shared visibility timing and effects.
    /// Root-portal scope only; the trigger remains active while content closes.
    pub fn animation(mut self, animation: impl Into<Option<VisibilityAnimation>>) -> Self {
        self.animation = animation.into();
        self
    }

    /// Create a new select.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set options.
    pub fn options(mut self, options: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        self.options = options.into_iter().map(Into::into).collect();
        self
    }

    /// Set selected index.
    ///
    /// Pass `None` for no current selection (shows `placeholder`). Bare integers
    /// still work via `From<T> for Option<T>` (`select.selected(0)`).
    pub fn selected(mut self, selected: impl Into<Option<usize>>) -> Self {
        self.selected = selected.into();
        self
    }

    /// Set placeholder text.
    pub fn placeholder(mut self, placeholder: impl Into<Arc<str>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Set expanded state.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// Set toggle callback.
    pub fn on_toggle(mut self, cb: Callback<bool>) -> Self {
        self.on_toggle = Some(cb);
        self
    }

    /// Set selection callback fired when the user picks an option.
    pub fn on_select(mut self, cb: Callback<SelectEvent>) -> Self {
        self.on_select = Some(cb);
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Control whether the trigger button is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Control whether the trigger button participates in tab traversal.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the trigger button gains focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the trigger button loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.on_blur = Some(cb);
        self
    }

    /// Set focused key handler. Returning `true` consumes the key before built-in navigation.
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
        self
    }

    /// Set button variant.
    pub fn button_variant(mut self, variant: ButtonVariant) -> Self {
        self.button_variant = variant;
        self
    }

    /// Set the trigger button's padding, replacing the button's default.
    pub fn button_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.button_padding = Some(padding.into());
        self
    }

    /// Set the trigger label's horizontal alignment within a wider trigger (the button's default:
    /// centered).
    pub fn button_align(mut self, align: Align) -> Self {
        self.button_align = Some(align);
        self
    }

    /// Set button style.
    pub fn button_style(mut self, style: Style) -> Self {
        self.button_style = style;
        self
    }

    /// Set button hover style.
    pub fn button_hover_style(mut self, style: Style) -> Self {
        self.button_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed button hover style.
    pub fn extend_button_hover_style(mut self, style: Style) -> Self {
        self.button_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed button hover style.
    pub fn inherit_button_hover_style(mut self) -> Self {
        self.button_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set button hover style slot directly for composite forwarding.
    pub fn button_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.button_hover_style = slot;
        self
    }

    /// Set button focus style.
    pub fn button_focus_style(mut self, style: Style) -> Self {
        self.button_focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed button focus style.
    pub fn extend_button_focus_style(mut self, style: Style) -> Self {
        self.button_focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed button focus style.
    pub fn inherit_button_focus_style(mut self) -> Self {
        self.button_focus_style = StyleSlot::Inherit;
        self
    }

    /// Set button focus style slot directly for composite forwarding.
    pub fn button_focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.button_focus_style = slot;
        self
    }

    /// Set button disabled style.
    pub fn button_disabled_style(mut self, style: Style) -> Self {
        self.button_disabled_style = style;
        self
    }

    /// Set button border style (used for outlined variant).
    pub fn button_border_style(mut self, style: BorderStyle) -> Self {
        self.button_border_style = style;
        self
    }

    /// Set button border style while hovered.
    pub fn button_hover_border_style(mut self, style: BorderStyle) -> Self {
        self.button_hover_border_style = Some(style);
        self
    }

    /// Set button border style while focused.
    pub fn button_focus_border_style(mut self, style: BorderStyle) -> Self {
        self.button_focus_border_style = Some(style);
        self
    }

    /// Set suffix shown when dropdown is open.
    pub fn button_open_suffix(mut self, suffix: impl Into<Arc<str>>) -> Self {
        self.button_open_suffix = Some(suffix.into());
        self
    }

    /// Set suffix shown when dropdown is closed.
    pub fn button_closed_suffix(mut self, suffix: impl Into<Arc<str>>) -> Self {
        self.button_closed_suffix = Some(suffix.into());
        self
    }

    /// Set suffix style.
    pub fn button_suffix_style(mut self, style: Style) -> Self {
        self.button_suffix_style = style;
        self
    }

    /// Set dropdown title.
    pub fn list_title(mut self, title: impl Into<Arc<str>>) -> Self {
        self.list_title = Some(title.into());
        self
    }

    /// Set dropdown title style.
    pub fn list_title_style(mut self, style: Style) -> Self {
        self.list_title_style = style;
        self
    }

    /// Set list config.
    pub fn list_config(mut self, config: ListConfig) -> Self {
        self.list_config = config;
        self
    }

    /// Set list border.
    pub fn list_border(mut self, border: bool) -> Self {
        self.list_config.border = border;
        self
    }

    /// Set list border style.
    pub fn list_border_style(mut self, style: BorderStyle) -> Self {
        self.list_config.border_style = style;
        self
    }

    /// Set list padding.
    pub fn list_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.list_config.padding = padding.into();
        self
    }

    /// Set list style.
    pub fn list_style(mut self, style: Style) -> Self {
        self.list_config.style = style;
        self
    }

    /// Set list highlight style.
    pub fn list_selection_style(mut self, style: Style) -> Self {
        self.list_config.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed dropdown highlight style.
    pub fn extend_list_selection_style(mut self, style: Style) -> Self {
        self.list_config.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed dropdown highlight style.
    pub fn inherit_list_selection_style(mut self) -> Self {
        self.list_config.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set list highlight style slot directly for composite forwarding.
    pub fn list_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.list_config.selection_style = slot;
        self
    }

    /// Set dropdown highlight style while the list is not focused.
    pub fn list_unfocused_selection_style(mut self, style: Style) -> Self {
        self.list_config.unfocused_selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed dropdown highlight style while the list is not focused.
    pub fn extend_list_unfocused_selection_style(mut self, style: Style) -> Self {
        self.list_config.unfocused_selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed dropdown highlight style while the list is not focused.
    pub fn inherit_list_unfocused_selection_style(mut self) -> Self {
        self.list_config.unfocused_selection_style = StyleSlot::Inherit;
        self
    }

    /// Set list unfocused highlight style slot directly for composite forwarding.
    pub fn list_unfocused_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.list_config.unfocused_selection_style = slot;
        self
    }

    /// Set whether selection style spans full list row width.
    pub fn list_selection_full_width(mut self, full_width: bool) -> Self {
        self.list_config.selection_full_width = full_width;
        self
    }

    /// Set list highlight symbol.
    pub fn list_selection_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.list_config.selection_symbol = symbol.map(Into::into);
        self
    }

    /// Set the trailing list selection symbol (right "pill" cap). Pairs with
    /// [`Self::list_selection_symbol`] and shares the selection symbol style.
    pub fn list_selection_symbol_right(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.list_config.selection_symbol_right = symbol.map(Into::into);
        self
    }

    /// Set list highlight symbol style.
    pub fn list_selection_symbol_style(mut self, style: Style) -> Self {
        self.list_config.selection_symbol_style = Some(style);
        self
    }

    /// Set list highlight symbol style while the list is not focused.
    pub fn list_unfocused_selection_symbol_style(mut self, style: Style) -> Self {
        self.list_config.unfocused_selection_symbol_style = Some(style);
        self
    }

    /// Set list hover style.
    pub fn list_hover_style(mut self, style: Style) -> Self {
        self.list_config.item_hover_style = Some(StyleSlot::Replace(style));
        self
    }

    /// Extend the themed list hover style.
    pub fn extend_list_hover_style(mut self, style: Style) -> Self {
        self.list_config.item_hover_style = Some(StyleSlot::Extend(style));
        self
    }

    /// Inherit the themed list hover style.
    pub fn inherit_list_hover_style(mut self) -> Self {
        self.list_config.item_hover_style = Some(StyleSlot::Inherit);
        self
    }

    /// Set list hover style slot directly for composite forwarding.
    pub fn list_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.list_config.item_hover_style = Some(slot);
        self
    }

    /// Set list width.
    pub fn list_width(mut self, width: Length) -> Self {
        self.list_width = Some(width);
        self
    }

    /// Set list height.
    pub fn list_height(mut self, height: Length) -> Self {
        self.list_height = height;
        self
    }

    /// Force dropdown width to exactly match trigger button width.
    pub fn match_button_width(mut self, match_button_width: bool) -> Self {
        self.match_button_width = match_button_width;
        self
    }

    /// Enable list scrollbar.
    pub fn list_scrollbar(mut self, scrollbar: bool) -> Self {
        self.list_config.scrollbar = scrollbar;
        self
    }

    /// Set list scrollbar configuration.
    pub fn list_scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.list_config.scrollbar_config = config;
        self
    }

    /// Set dropdown empty text.
    pub fn list_empty_text(mut self, text: impl Into<Arc<str>>) -> Self {
        self.list_empty_text = Some(text.into());
        self
    }

    /// Set dropdown empty text style.
    pub fn list_empty_text_style(mut self, style: Style) -> Self {
        self.list_config.empty_text_style = style;
        self
    }

    /// Set dropdown disabled style.
    pub fn list_disabled_style(mut self, style: Style) -> Self {
        self.list_disabled_style = style;
        self
    }
}

impl From<Select> for Element {
    fn from(select: Select) -> Self {
        let label = if let Some(idx) = select.selected {
            select
                .options
                .get(idx)
                .cloned()
                .unwrap_or(select.placeholder.clone())
        } else {
            select.placeholder.clone()
        };
        let semantic_name = label.clone();
        let semantic_selected = select.selected.is_some();
        let semantic_expanded = select.expanded;

        let mut button = Button::new(label)
            .variant(select.button_variant)
            .width(select.width)
            .style(select.button_style)
            .hover_style_slot(select.button_hover_style)
            .focus_style_slot(select.button_focus_style)
            .disabled_style(select.button_disabled_style)
            .disabled(select.disabled)
            .focusable(select.focusable)
            .tab_stop(select.tab_stop);

        if let Some(padding) = select.button_padding {
            button = button.padding(padding);
        }
        if let Some(align) = select.button_align {
            button = button.align(align);
        }
        if let Some(cb) = select.on_focus.clone() {
            button = button.on_focus(cb);
        }
        if let Some(cb) = select.on_blur.clone() {
            button = button.on_blur(cb);
        }

        if matches!(select.button_variant, ButtonVariant::Outlined) {
            button = button.border_style(select.button_border_style);
        }
        if let Some(style) = select.button_hover_border_style {
            button = button.hover_border_style(Some(style));
        }
        if let Some(style) = select.button_focus_border_style {
            button = button.focus_border_style(Some(style));
        }

        let suffix = if select.expanded {
            select.button_open_suffix.clone()
        } else {
            select.button_closed_suffix.clone()
        };
        if let Some(suffix) = suffix {
            button = button
                .shortcut(suffix)
                .shortcut_style(select.button_suffix_style);
        }

        if let Some(cb) = select.on_toggle.clone()
            && !select.disabled
        {
            let expanded = select.expanded;
            button = button.on_click(Callback::new(move |_: MouseEvent| cb.emit(!expanded)));
        }

        if select.expanded && !select.disabled {
            let on_toggle = select.on_toggle.clone();
            let caller_on_key = select.on_key.clone();
            button = button.on_key(KeyHandler::new(move |key: KeyEvent| {
                if caller_on_key
                    .as_ref()
                    .is_some_and(|handler| handler.handle(key))
                {
                    return true;
                }

                if key.code == KeyCode::Esc
                    && let Some(toggle) = &on_toggle
                {
                    toggle.emit(false);
                    return true;
                }

                false
            }));
        } else if let Some(handler) = select.on_key {
            button = button.on_key(handler);
        }

        let list_hover_slot = select
            .list_config
            .item_hover_style
            .unwrap_or(select.list_config.selection_style);

        let options: Arc<[Arc<str>]> = select.options.into();
        let mut list = List::new()
            .items(options.iter().map(|s| ListItem::new(s.clone())))
            .selected(select.selected)
            .title_style(select.list_title_style)
            .border(select.list_config.border)
            .border_style(select.list_config.border_style)
            .padding(select.list_config.padding)
            .style(select.list_config.style)
            .selection_full_width(select.list_config.selection_full_width)
            .selection_symbol_style(
                select.list_config.selection_symbol_style.unwrap_or(
                    select
                        .list_config
                        .selection_style
                        .explicit_style()
                        .unwrap_or_default(),
                ),
            )
            .unfocused_selection_symbol_style(
                select
                    .list_config
                    .unfocused_selection_symbol_style
                    .or_else(|| {
                        select
                            .list_config
                            .unfocused_selection_style
                            .explicit_style()
                    })
                    .unwrap_or(
                        select
                            .list_config
                            .selection_style
                            .explicit_style()
                            .unwrap_or_default(),
                    ),
            )
            .selection_symbol(select.list_config.selection_symbol)
            .selection_symbol_right(select.list_config.selection_symbol_right)
            .symbol_column(select.list_config.symbol_column)
            .gutter_gap(select.list_config.gutter_gap)
            .gutter_for_non_selectable(select.list_config.gutter_for_non_selectable)
            .scrollbar(select.list_config.scrollbar)
            .scrollbar_config(select.list_config.scrollbar_config)
            .width(select.list_width.unwrap_or(select.width))
            .height(select.list_height)
            .item_horizontal_padding(select.list_config.item_horizontal_padding)
            .header_horizontal_padding(select.list_config.header_horizontal_padding)
            .empty_text_style(select.list_config.empty_text_style)
            .empty_text_padding(select.list_config.empty_text_padding)
            .disabled_style(select.list_disabled_style)
            .disabled(select.disabled);
        list = list
            .selection_style_slot(select.list_config.selection_style)
            .unfocused_selection_style_slot(select.list_config.unfocused_selection_style)
            .item_hover_style_slot(list_hover_slot);

        if let Some(title) = select.list_title {
            list = list.title(title);
        }
        if let Some(empty_text) = select.list_empty_text {
            list = list.empty_text(empty_text);
        }

        if select.on_select.is_some() || select.on_toggle.is_some() {
            let on_select = select.on_select.clone();
            let on_toggle = select.on_toggle.clone();
            list = list.on_activate(Callback::new(move |ev: ListEvent| {
                if let Some(cb) = &on_select
                    && let Some(label) = options.get(ev.index).cloned()
                {
                    cb.emit(SelectEvent {
                        index: ev.index,
                        label,
                    });
                }
                if let Some(toggle) = &on_toggle {
                    toggle.emit(false);
                }
            }));
        }

        let overlay = ZStack::new().child(crate::child(
            || SelectList,
            SelectListProps {
                list: Rc::new(list),
                selected: select.selected,
                expanded: select.expanded,
            },
        ));
        let mut popover = Popover::new()
            .animation(select.animation.clone())
            .trigger(button)
            .content(overlay)
            .open(select.expanded && !select.disabled)
            .fit_trigger_width(select.match_button_width && select.list_width.is_none())
            .placement(PopoverPlacement::BelowStart);

        if let Some(cb) = select.on_toggle.clone() {
            popover = popover.on_close(Callback::new(move |_| cb.emit(false)));
        }

        Element::from(popover)
            .semantic_role(crate::automation::SemanticRole::ComboBox)
            .semantic_name(semantic_name)
            .semantic_selected(semantic_selected)
            .semantic_expanded(semantic_expanded)
    }
}

// Track popup keyboard highlight internally across renders so moving with
// arrows does not fire `Select::on_select` until the user commits a pick.
#[derive(Clone)]
struct SelectListProps {
    list: Rc<List>,
    selected: Option<usize>,
    expanded: bool,
}

impl PartialEq for SelectListProps {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.list, &other.list)
            && self.selected == other.selected
            && self.expanded == other.expanded
    }
}

struct SelectList;

impl crate::Component for SelectList {
    type Properties = SelectListProps;
    type State = Option<usize>;
    type Message = usize;

    fn create_state(&self, props: &Self::Properties) -> Self::State {
        props.selected.filter(|&i| i < props.list.items.len())
    }

    fn on_props_changed(
        &mut self,
        old: &Self::Properties,
        ctx: &mut crate::Context<Self>,
    ) -> crate::Update {
        let len = ctx.props.list.items.len();
        if (!old.expanded && ctx.props.expanded) || old.selected != ctx.props.selected {
            ctx.state = ctx.props.selected.filter(|&i| i < len);
        } else if old.list.items.len() != len {
            ctx.state = ctx.state.filter(|&i| i < len);
        }
        crate::Update::render()
    }

    fn update(&mut self, highlighted: usize, ctx: &mut crate::Context<Self>) -> crate::Update {
        ctx.state = Some(highlighted);
        crate::Update::render()
    }

    fn view(&self, ctx: &crate::Context<Self>) -> Element {
        ctx.props
            .list
            .as_ref()
            .clone()
            .selected(ctx.state)
            .on_select(ctx.link().callback(|ev: ListEvent| ev.index))
            .into()
    }
}
