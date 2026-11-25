//! Composed rows: a [`List`] whose rows are elements.
//!
//! A `List` renders through this component when any item has
//! [`content`](crate::widgets::ListItem::content), and `Tree` renders its composed rows through
//! it, so one engine owns selection, hover, clicks, and keyboard navigation for both. Selection is
//! controlled the way a text list's is: keys and clicks report `on_select`, and the parent passes
//! the new index back.

use std::rc::Rc;
use std::sync::Arc;

use crate::app::input::drag::ClickState;
use crate::callback::{Callback, KeyHandler};
use crate::core::component::{Component, Context, KeyUpdate, Update};
use crate::core::element::{Element, IntoElement};
use crate::core::event::{KeyCode, KeyEvent, KeyMods, MouseEvent, MouseRegionEvent};
use crate::style::{Length, Style, ThemeRole, VisualEffect};
use crate::widgets::{
    Frame, HStack, List, ListEvent, ListHoverEvent, ListItem, MouseRegion, ScrollKeymap,
    ScrollView, ScrollViewportEvent, Text, VStack,
};

use super::utils::list_item_height;

/// What a row renderer knows about the row it draws.
pub(crate) struct RowView<'a> {
    pub index: usize,
    pub item: &'a ListItem,
    /// The pointer is over a row that paints hover.
    pub hovered: bool,
    /// The row's resolved style, including hover and selection.
    pub style: Style,
}

/// Draws a row's content from its [`RowView`]. Without one, a row draws its item's content, or
/// its label as a one-row text list.
#[derive(Clone)]
pub(crate) struct RowRenderer(Rc<dyn Fn(RowView<'_>) -> Element>);

impl RowRenderer {
    pub(crate) fn new(render: impl Fn(RowView<'_>) -> Element + 'static) -> Self {
        Self(Rc::new(render))
    }
}

impl PartialEq for RowRenderer {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct ComposedListProps {
    pub list: List,
    pub render: Option<RowRenderer>,
    /// Rows that paint hover; selectable rows when unset.
    pub hoverable: Option<Arc<[bool]>>,
    pub hover_effect: Option<VisualEffect>,
    pub on_pointer_move: Option<Callback<usize>>,
    pub on_viewport_change: Option<Callback<ScrollViewportEvent>>,
    /// Runs before navigation, unlike `List::on_key`, which runs after it.
    pub key_interceptor: Option<KeyHandler>,
    /// Key of the focusable scroll view, for programmatic focus.
    pub focus_key: Option<Arc<str>>,
    pub gap: u16,
}

impl ComposedListProps {
    pub(crate) fn new(list: List) -> Self {
        Self {
            list,
            render: None,
            hoverable: None,
            hover_effect: None,
            on_pointer_move: None,
            on_viewport_change: None,
            key_interceptor: None,
            focus_key: None,
            gap: 0,
        }
    }

    pub(crate) fn into_element(self) -> Element {
        crate::child(ComposedList::new, self).semantic_role(crate::automation::SemanticRole::List)
    }

    fn selected(&self) -> Option<usize> {
        let items = &self.list.items;
        self.list
            .selected
            .and_then(|selected| List::nearest_selectable_index(items, selected))
    }

    fn hoverable(&self, index: usize) -> bool {
        !self.list.disabled
            && match &self.hoverable {
                Some(rows) => rows.get(index).copied().unwrap_or(false),
                None => List::is_selectable_index(&self.list.items, index),
            }
    }
}

pub(crate) struct ComposedList;

impl ComposedList {
    fn new() -> Self {
        Self
    }
}

#[derive(Default)]
pub(crate) struct ComposedListState {
    hovered: Option<usize>,
    last_click: Option<ClickState>,
}

#[derive(Clone, Debug)]
pub(crate) enum ComposedListMsg {
    Hover(usize, bool),
    PointerMove(usize),
    Click(usize, MouseRegionEvent),
    ContextMenu(usize, MouseRegionEvent),
}

impl Component for ComposedList {
    type Message = ComposedListMsg;
    type Properties = ComposedListProps;
    type State = ComposedListState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        ComposedListState::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            ComposedListMsg::Hover(index, hovered) => hover(ctx, index, hovered),
            ComposedListMsg::PointerMove(index) => {
                if let Some(cb) = &ctx.props.on_pointer_move {
                    cb.emit(index);
                }
                Update::none()
            }
            ComposedListMsg::Click(index, mouse) => click(ctx, index, mouse),
            ComposedListMsg::ContextMenu(index, mouse) => {
                if let Some(cb) = &ctx.props.list.on_context_menu {
                    cb.emit((ListEvent::click(index, mouse.mods, 1), mouse_event(mouse)));
                }
                Update::none()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if on_key(ctx, key) {
            KeyUpdate::handled(Update::none())
        } else {
            KeyUpdate::unhandled(Update::none())
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        view(ctx)
    }
}

fn hover(ctx: &mut Context<ComposedList>, index: usize, hovered: bool) -> Update {
    if hovered {
        ctx.state.hovered = Some(index);
    } else if ctx.state.hovered == Some(index) {
        ctx.state.hovered = None;
    }
    if let Some(cb) = &ctx.props.list.on_hover {
        cb.emit(ListHoverEvent { index, hovered });
    }
    if ctx.props.hoverable(index) {
        Update::full()
    } else {
        Update::none()
    }
}

/// A row click reports what a click on a text row does, in the same order.
fn click(ctx: &mut Context<ComposedList>, index: usize, mouse: MouseRegionEvent) -> Update {
    let list = &ctx.props.list;
    if list.disabled || !List::is_selectable_index(&list.items, index) {
        return Update::none();
    }
    let now = ctx.env().now();
    let clicks = crate::app::input::mouse::click_count_at(
        &mut ctx.state.last_click,
        mouse.x,
        mouse.y,
        true,
        now,
    );
    let event = ListEvent::click(index, mouse.mods, clicks);
    if let Some(cb) = &list.on_item_click {
        cb.emit(event);
    }
    if let Some(cb) = &list.on_select {
        cb.emit(event);
    }
    let activation = if mouse.mods == KeyMods::CTRL && list.on_ctrl_activate.is_some() {
        list.on_ctrl_activate.as_ref()
    } else if list.activate_on_click || event.clicks == 2 {
        list.on_activate.as_ref()
    } else {
        None
    };
    if let Some(cb) = activation {
        cb.emit(event);
    }
    Update::none()
}

fn mouse_event(mouse: MouseRegionEvent) -> MouseEvent {
    MouseEvent {
        x: mouse.x,
        y: mouse.y,
        kind: mouse.kind,
        mods: mouse.mods,
    }
}

/// Navigation and activation keys, by the same rules as a text list.
fn on_key(ctx: &Context<ComposedList>, key: KeyEvent) -> bool {
    let props = &ctx.props;
    let list = &props.list;
    if list.disabled {
        return false;
    }
    if props
        .key_interceptor
        .as_ref()
        .is_some_and(|handler| handler.handle(key))
    {
        return true;
    }
    let selected = props.selected();
    if let Some(cb) = &list.on_select {
        let forward = key.code != KeyCode::PageUp;
        let page = page_rows(&list.items, selected, ctx.viewport().h, forward);
        if let Some(next) = List::navigation_target(
            selected,
            &list.items,
            &key,
            list.scroll_keys,
            list.navigation_wrap,
            page,
        ) {
            if Some(next) != selected {
                cb.emit(ListEvent::key(next));
            }
            return true;
        }
    }
    if key.code == KeyCode::Enter
        && let Some(selected) = selected
    {
        let ctrl = key.mods == KeyMods::CTRL;
        let activation = if ctrl && list.on_ctrl_activate.is_some() {
            list.on_ctrl_activate.as_ref()
        } else {
            list.on_activate.as_ref()
        };
        if let Some(cb) = activation {
            cb.emit(ListEvent::key(selected));
            return true;
        }
    }
    list.on_key
        .as_ref()
        .is_some_and(|handler| handler.handle(key))
}

/// How many rows a page key moves past: the rows a viewport, less one line, holds beyond the
/// selection in the key's direction. The row that crosses the edge counts, as on a text list.
fn page_rows(items: &[ListItem], selected: Option<usize>, height: u16, forward: bool) -> usize {
    let mut remaining = height.saturating_sub(1).max(1) as usize;
    let mut at = selected.unwrap_or(0);
    let mut rows = 0;
    while remaining > 0 {
        let next = if forward {
            at.checked_add(1).filter(|next| *next < items.len())
        } else {
            at.checked_sub(1)
        };
        let Some(next) = next else {
            break;
        };
        at = next;
        rows += 1;
        remaining = remaining.saturating_sub(list_item_height(&items[at]));
    }
    rows.max(1)
}

/// The style of a composed row: its own, then active, hover, and selection layered on top.
pub(crate) fn row_style(
    ctx: &Context<ComposedList>,
    item: &ListItem,
    hovered: bool,
    selected: bool,
) -> Style {
    let theme = ctx.theme();
    let list = &ctx.props.list;
    let mut style = item.style;
    if item.active {
        style = style.patch(crate::style::resolve_slot(
            &theme,
            ThemeRole::Active,
            &list.active_style,
        ));
    }
    if hovered {
        style = style.patch(crate::style::resolve_slot(
            &theme,
            ThemeRole::ItemHover,
            &list.item_hover_style,
        ));
    }
    if selected {
        style = style.patch(crate::style::resolve_selection_slot(
            &theme,
            &list.selection_style,
            &list.unfocused_selection_style,
            ctx.has_focus_within(),
        ));
    }
    if list.disabled {
        style = style.patch(list.disabled_style);
    }
    style
}

fn row_key(index: usize, item: &ListItem) -> String {
    match &item.key {
        Some(key) => format!("row-key-{key}"),
        None => format!("row-at-{index}"),
    }
}

/// A row without composed content: its label, drawn by a one-row text list that takes no input.
pub(crate) fn text_row(item: &ListItem) -> Element {
    List::new()
        .items([item.clone()])
        .height(Length::Auto)
        .focusable(false)
        .tab_stop(false)
        .symbol_column(false)
        .selected(None)
        .scroll_keys(ScrollKeymap::NONE)
        // A wheel-scrollable list is a hit target and would keep clicks on the label from
        // reaching the row. The composed list's scroll view owns the wheel.
        .scroll_wheel(false)
        .into()
}

fn view(ctx: &Context<ComposedList>) -> Element {
    let props = &ctx.props;
    let list = &props.list;
    let selected = props.selected();
    let mut view = scroll_view(props);
    if let Some(index) = selected
        && (ctx.has_focus_within() || list.force_scroll_to_selected)
    {
        // Follow the cursor the way a text list does: scroll only when it nears an edge.
        view = view
            .reveal_key(row_key(index, &list.items[index]))
            .reveal_margin(2);
    }
    for (index, item) in list.items.iter().enumerate() {
        view = view.child(row(ctx, index, item, selected == Some(index)));
    }
    if list.items.is_empty()
        && let Some(text) = &list.empty_text
    {
        view = view.child(
            VStack::new()
                .padding(list.empty_text_padding)
                .height(Length::Auto)
                .child(Text::new(text.clone()).style(list.empty_text_style)),
        );
    }
    let view: Element = match &props.focus_key {
        Some(key) => view.key(key.clone()),
        None => view.into(),
    };
    let view = if list.border {
        let mut frame = Frame::new()
            .border(true)
            .border_style(list.border_style)
            .title_style(list.title_style)
            .width(list.width)
            .height(list.height)
            .child(view);
        if let Some(title) = &list.title {
            frame = frame.title(title.clone());
        }
        frame.into()
    } else {
        view
    };
    match &list.on_click {
        Some(cb) => {
            let cb = cb.clone();
            MouseRegion::new()
                .child(view)
                .on_click(Callback::new(move |mouse| cb.emit(mouse_event(mouse))))
                .into()
        }
        None => view,
    }
}

fn scroll_view(props: &ComposedListProps) -> ScrollView {
    let list = &props.list;
    let (width, height) = if list.border {
        (Length::Flex(1), Length::Flex(1))
    } else {
        (list.width, list.height)
    };
    let mut view = ScrollView::new()
        .gap(props.gap)
        .padding(list.padding)
        .width(width)
        .height(height)
        .style(list.style)
        .scrollbar(list.scrollbar)
        .scrollbar_config(list.scrollbar_config.clone())
        .show_scroll_indicators(list.show_scroll_indicators)
        .scroll_indicator_style(list.scroll_indicator_style)
        .focusable(list.focusable && !list.disabled)
        .tab_stop(list.tab_stop)
        .scroll_keys(ScrollKeymap::NONE)
        .scroll_wheel(list.scroll_wheel);
    if let Some(cb) = &list.on_focus {
        view = view.on_focus(cb.clone());
    }
    if let Some(cb) = &list.on_blur {
        view = view.on_blur(cb.clone());
    }
    if let Some(cb) = &list.on_scroll_to {
        view = view.on_scroll_to(cb.clone());
    }
    if let Some(cb) = &props.on_viewport_change {
        view = view.on_viewport_change(cb.clone());
    }
    view
}

fn row(ctx: &Context<ComposedList>, index: usize, item: &ListItem, selected: bool) -> Element {
    let props = &ctx.props;
    let list = &props.list;
    let hoverable = props.hoverable(index);
    let hovered = hoverable && ctx.state.hovered == Some(index);
    let style = row_style(ctx, item, hovered, selected);
    let content = match &props.render {
        Some(render) => (render.0)(RowView {
            index,
            item,
            hovered,
            style,
        }),
        None => item
            .content
            .as_ref()
            .map(|content| content.element())
            .unwrap_or_else(|| text_row(item)),
    };
    let mut region = MouseRegion::new()
        .child(
            HStack::new()
                .gap(0)
                .height(match item.content {
                    Some(_) => Length::Px(item.content_height),
                    None => Length::Auto,
                })
                .style(style)
                .child(content),
        )
        .on_hover_change(
            ctx.link()
                .callback(move |hovered| ComposedListMsg::Hover(index, hovered)),
        );
    if props.on_pointer_move.is_some() {
        region = region.on_mouse_move(
            ctx.link()
                .callback(move |_| ComposedListMsg::PointerMove(index)),
        );
    }
    if item.is_selectable() && !list.disabled {
        region = region.on_click(
            ctx.link()
                .callback(move |mouse| ComposedListMsg::Click(index, mouse)),
        );
        if list.on_context_menu.is_some() {
            region = region.on_context_menu(
                ctx.link()
                    .callback(move |mouse| ComposedListMsg::ContextMenu(index, mouse)),
            );
        }
    }
    if hoverable && let Some(effect) = &props.hover_effect {
        region = region.hover_effect(effect.clone());
    }
    let mut element: Element = region.into();
    if let Some(drag) = list.host_drag.clone() {
        element = element.host_drag(crate::dnd::HostDragSource::new(move |_| drag.offer(index)));
    }
    element.key(row_key(index, item))
}
