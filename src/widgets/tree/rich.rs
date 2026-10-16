//! Composed rows use the same tree model and selection events as text rows.
use super::*;
use crate::core::component::KeyUpdate;
use crate::core::event::{KeyMods, MouseRegionEvent};
use crate::widgets::scroll::{ScrollAction, has_command_modifier, scroll_action_from_key};
use crate::widgets::{HStack, MouseRegion, ScrollKeymap, ScrollView, Text, VStack};

pub(super) fn has_content(node: &TreeNode) -> bool {
    node.content.is_some()
        || node.leading.is_some()
        || node.icon.is_some()
        || node.children.iter().any(has_content)
}

pub(super) fn toggle(ctx: &mut Context<TreeComponent>, index: usize) -> Update {
    let Some(entry) = entry_at_index(&ctx.props, &ctx.state, index) else {
        return Update::none();
    };
    if !entry.node.is_collapsible() {
        return Update::none();
    }
    let path = entry.path.clone();
    let expanded = if ctx.props.controlled_expansion {
        !entry.expanded
    } else {
        toggle_path(&mut ctx.state.expanded, &path)
    };
    if let Some(cb) = &ctx.props.on_toggle {
        cb.emit(TreeToggleEvent {
            index,
            path,
            expanded,
        });
    }
    Update::full()
}

pub(super) fn hover(ctx: &mut Context<TreeComponent>, index: usize, hovered: bool) -> Update {
    if hovered {
        ctx.state.hovered = Some(index);
    } else if ctx.state.hovered == Some(index) {
        ctx.state.hovered = None;
    }
    if let Some(cb) = &ctx.props.on_hover
        && let Some(entry) = entry_at_index(&ctx.props, &ctx.state, index)
    {
        cb.emit(TreeHoverEvent {
            index,
            path: entry.path,
            hovered,
        });
    }
    Update::full()
}

pub(super) fn pointer_move(ctx: &Context<TreeComponent>, index: usize) -> Update {
    if let Some(cb) = &ctx.props.on_pointer_move
        && let Some(entry) = entry_at_index(&ctx.props, &ctx.state, index)
    {
        cb.emit(TreeEvent {
            index,
            path: entry.path,
        });
    }
    Update::none()
}

pub(super) fn action(ctx: &mut Context<TreeComponent>, action: TreeAction) -> Update {
    let Some(index) = selected_index(ctx) else {
        return Update::none();
    };
    let expanded = resolved_expanded(ctx);
    let entries = visible_entries(&ctx.props, expanded.as_ref());
    let Some(entry) = entries.get(index) else {
        return Update::none();
    };
    if should_toggle(entry, action) {
        return toggle(ctx, index);
    }
    let next = match action {
        TreeAction::Expand if entry.expanded => child_index(&entries, index),
        TreeAction::Collapse => parent_index(&entries, entry),
        _ => None,
    };
    next.map(|index| select(ctx, index))
        .unwrap_or_else(Update::none)
}

fn should_toggle(entry: &TreeEntry<'_>, action: TreeAction) -> bool {
    matches!(
        (action, entry.expanded, entry.node.is_collapsible()),
        (TreeAction::Toggle, _, true)
            | (TreeAction::Expand, false, true)
            | (TreeAction::Collapse, true, true)
    )
}

fn child_index(entries: &[TreeEntry<'_>], index: usize) -> Option<usize> {
    let parent = entries.get(index)?.path.segments();
    entries
        .iter()
        .enumerate()
        .skip(index + 1)
        .take_while(|(_, entry)| entry.path.segments().starts_with(parent))
        .find(|(_, entry)| entry.node.item.is_selectable())
        .map(|(index, _)| index)
}

fn parent_index(entries: &[TreeEntry<'_>], entry: &TreeEntry<'_>) -> Option<usize> {
    let (_, parent) = entry.path.segments().split_last()?;
    entries.iter().position(|candidate| {
        candidate.path.segments() == parent && candidate.node.item.is_selectable()
    })
}

fn select(ctx: &mut Context<TreeComponent>, index: usize) -> Update {
    ctx.state.selected = Some(index);
    if let Some(cb) = &ctx.props.on_select
        && let Some(entry) = entry_at_index(&ctx.props, &ctx.state, index)
    {
        cb.emit(TreeEvent {
            index,
            path: entry.path,
        });
    }
    Update::full()
}

pub(super) fn on_key(
    component: &mut TreeComponent,
    key: KeyEvent,
    ctx: &mut Context<TreeComponent>,
) -> KeyUpdate {
    if ctx
        .props
        .key_interceptor
        .as_ref()
        .is_some_and(|cb| cb.handle(key))
    {
        return KeyUpdate::handled(Update::none());
    }
    if let Some(action) = tree_action_from_key(&key, ctx.props.keymap) {
        return KeyUpdate::handled(self::action(ctx, action));
    }
    if key.code == KeyCode::Enter
        && key.mods == KeyMods::CTRL
        && ctx.props.on_ctrl_activate.is_some()
        && let Some(index) = selected_index(ctx)
    {
        return KeyUpdate::handled(component.update(TreeMsg::CtrlActivate(index), ctx));
    }
    if has_command_modifier(&key) {
        return KeyUpdate::unhandled(Update::none());
    }
    if key.code == KeyCode::Enter {
        return match selected_index(ctx) {
            Some(index) => KeyUpdate::handled(component.update(TreeMsg::Activate(index), ctx)),
            None => KeyUpdate::unhandled(Update::none()),
        };
    }
    let entries = visible_entries(&ctx.props, &ctx.state.expanded);
    let Some((target, forward)) = navigation_target(ctx, &entries, key) else {
        return KeyUpdate::unhandled(Update::none());
    };
    let items = entries
        .iter()
        .map(|entry| entry.node.item.clone())
        .collect::<Vec<_>>();
    let next = if forward {
        crate::widgets::List::selectable_at_or_after(&items, target)
            .or_else(|| crate::widgets::List::selectable_at_or_before(&items, target))
    } else {
        crate::widgets::List::selectable_at_or_before(&items, target)
            .or_else(|| crate::widgets::List::selectable_at_or_after(&items, target))
    };
    KeyUpdate::handled(
        next.map(|index| select(ctx, index))
            .unwrap_or_else(Update::none),
    )
}

fn navigation_target(
    ctx: &Context<TreeComponent>,
    entries: &[TreeEntry<'_>],
    key: KeyEvent,
) -> Option<(usize, bool)> {
    let last = entries.len().checked_sub(1)?;
    let selected = selected_index(ctx);
    let current = selected.unwrap_or(0).min(last);
    let step = match key.code {
        KeyCode::PageDown | KeyCode::PageUp => {
            return Some((
                page_target(ctx, entries, current, key.code == KeyCode::PageDown),
                key.code == KeyCode::PageDown,
            ));
        }
        _ => scroll_action_from_key(&key, ctx.props.scroll_keys.without_vim_horizontal())?,
    };
    match step {
        ScrollAction::Home => Some((0, true)),
        ScrollAction::End => Some((last, false)),
        ScrollAction::LineDown(n) => {
            Some((selected.map(|_| (current + n).min(last)).unwrap_or(0), true))
        }
        ScrollAction::LineUp(n) => Some((
            selected.map(|_| current.saturating_sub(n)).unwrap_or(last),
            false,
        )),
        _ => None,
    }
}

fn page_target(
    ctx: &Context<TreeComponent>,
    entries: &[TreeEntry<'_>],
    current: usize,
    forward: bool,
) -> usize {
    let mut remaining = ctx.viewport().h.saturating_sub(1).max(1) as usize;
    let mut target = current;
    while remaining > 0 {
        let next = if forward {
            (target + 1).min(entries.len() - 1)
        } else {
            target.saturating_sub(1)
        };
        if next == target {
            break;
        }
        target = next;
        remaining = remaining.saturating_sub(row_height(entries[target].node) as usize);
    }
    target
}

fn row_height(node: &TreeNode) -> u16 {
    if node.content.is_some() {
        node.content_height
    } else {
        node.item
            .extra_lines
            .len()
            .saturating_add(1)
            .min(u16::MAX as usize) as u16
    }
}

fn row_key(entry: &TreeEntry<'_>) -> String {
    entry
        .node
        .key
        .as_ref()
        .map(|key| format!("tree-row-{key}"))
        .unwrap_or_else(|| format!("tree-row-{:?}", entry.path.segments()))
}

pub(super) fn view(ctx: &Context<TreeComponent>) -> Element {
    let expanded = resolved_expanded(ctx);
    let entries = visible_entries(&ctx.props, expanded.as_ref());
    let max_depth = entries
        .iter()
        .map(|entry| entry.branch.depth())
        .max()
        .unwrap_or(0);
    let selected = selected_index(ctx);
    let mut view = scroll_view(ctx);
    if let Some(entry) = selected.and_then(|index| entries.get(index))
        && (ctx.has_focus_within() || ctx.props.force_scroll_to_selected)
    {
        view = view.scroll_to_key(row_key(entry));
    }
    for (index, entry) in entries.iter().enumerate() {
        view = view.child(row(ctx, entry, index, max_depth, selected).key(row_key(entry)));
    }
    if entries.is_empty()
        && let Some(text) = &ctx.props.empty_text
    {
        view = view.child(Text::new(text.clone()).style(ctx.props.empty_text_style));
    }
    match &ctx.props.focus_key {
        Some(key) => view.key(key.clone()),
        None => view.into(),
    }
}

fn scroll_view(ctx: &Context<TreeComponent>) -> ScrollView {
    let mut view = ScrollView::new()
        .gap(ctx.props.gap)
        .width(ctx.props.width)
        .height(ctx.props.height)
        .style(ctx.props.style)
        .scrollbar(ctx.props.scrollbar)
        .scrollbar_config(ctx.props.scrollbar_config.clone())
        .show_scroll_indicators(ctx.props.show_scroll_indicators)
        .scroll_indicator_style(ctx.props.scroll_indicator_style)
        .focusable(ctx.props.focusable)
        .tab_stop(ctx.props.tab_stop)
        .scroll_keys(ScrollKeymap::NONE)
        .scroll_wheel(ctx.props.scroll_wheel);
    if let Some(cb) = &ctx.props.on_focus {
        view = view.on_focus(cb.clone());
    }
    if let Some(cb) = &ctx.props.on_blur {
        view = view.on_blur(cb.clone());
    }
    if let Some(cb) = &ctx.props.on_viewport_change {
        view = view.on_viewport_change(cb.clone());
    }
    view
}

fn row(
    ctx: &Context<TreeComponent>,
    entry: &TreeEntry<'_>,
    index: usize,
    max_depth: usize,
    selected: Option<usize>,
) -> Element {
    let content = entry
        .node
        .content
        .as_ref()
        .map(|content| content.element())
        .unwrap_or_else(|| {
            crate::widgets::List::new()
                .items([entry.node.item.clone()])
                .height(Length::Auto)
                .focusable(false)
                .tab_stop(false)
                .symbol_column(false)
                .selected(None)
                .scroll_keys(ScrollKeymap::NONE)
                // A wheel-scrollable list is a hit target and would keep clicks on the label
                // from reaching the row. The tree's own scroll view owns the wheel.
                .scroll_wheel(false)
                .into()
        });
    let style = row_style(ctx, entry, index, selected);
    let content = composed_content(ctx, entry, index, max_depth, content, style);
    let mut region = MouseRegion::new()
        .child(
            HStack::new()
                .gap(0)
                .height(Length::Auto)
                .style(style)
                .child(content),
        )
        .on_hover_change(
            ctx.link()
                .callback(move |hovered| TreeMsg::Hover(index, hovered)),
        )
        .on_mouse_move(ctx.link().callback(move |_| TreeMsg::PointerMove(index)));
    if entry.node.item.is_selectable() {
        let link = ctx.link().clone();
        let activate = ctx.props.activate_on_click;
        let ctrl = ctx.props.on_ctrl_activate.is_some();
        region = region.on_click(crate::callback::Callback::new(
            move |mouse: MouseRegionEvent| {
                link.send(TreeMsg::Select(index));
                if ctrl && mouse.mods == KeyMods::CTRL {
                    link.send(TreeMsg::CtrlActivate(index));
                } else if activate {
                    link.send(TreeMsg::Activate(index));
                }
            },
        ));
    }
    if let Some(effect) = &ctx.props.item_hover_effect {
        region = region.hover_effect(effect.clone());
    }
    region.into()
}

fn composed_content(
    ctx: &Context<TreeComponent>,
    entry: &TreeEntry<'_>,
    index: usize,
    max_depth: usize,
    content: Element,
    style: crate::style::Style,
) -> Element {
    let height = row_height(entry.node);
    let hovered = ctx.state.hovered == Some(index);
    let prefix = prefix(entry, &ctx.props, max_depth);
    let width = prefix.width();
    let blank = |cells: u16| {
        chrome(
            vec![crate::style::Span::new(" ".repeat(cells as usize))],
            style,
            hovered,
        )
    };

    let mut first =
        HStack::new()
            .gap(0)
            .height(Length::Px(1))
            .child(chrome(prefix.guides, style, hovered));
    if !prefix.arrow.is_empty() {
        let arrow = chrome(prefix.arrow, style, hovered);
        first = if entry.node.is_collapsible() {
            let ctrl = ctx.props.on_ctrl_activate.is_some();
            let selectable = entry.node.item.is_selectable();
            let link = ctx.link().clone();
            first.child(
                MouseRegion::new()
                    .child(arrow)
                    .on_click(ctx.link().callback(move |mouse: MouseRegionEvent| {
                        if ctrl && mouse.mods == KeyMods::CTRL {
                            if selectable {
                                link.send(TreeMsg::Select(index));
                            }
                            TreeMsg::CtrlActivate(index)
                        } else {
                            TreeMsg::Toggle(index)
                        }
                    })),
            )
        } else {
            first.child(arrow)
        };
    }
    if prefix.content_gap > 0 {
        first = first.child(blank(prefix.content_gap));
    }
    if let Some((icon_width, icon_gap)) = prefix.icon {
        let mut cell = HStack::new()
            .gap(0)
            .width(Length::Px(icon_width))
            .height(Length::Px(1));
        if let Some(icon) = &entry.node.icon {
            cell = cell.child(icon.element());
        }
        first = first.child(cell);
        if icon_gap > 0 {
            first = first.child(blank(icon_gap));
        }
    }

    let mut column = VStack::new()
        .gap(0)
        .width(Length::Px(width))
        .height(Length::Px(height))
        .child(first);
    for _ in 1..height {
        column = column.child(chrome(
            continuation_spans(entry, &ctx.props, max_depth, width),
            style,
            hovered,
        ));
    }
    let mut row = HStack::new().gap(0).height(Length::Px(height));
    if let Some(leading) = &entry.node.leading {
        row = row.child(leading.element());
    }
    row.child(column).child(content).into()
}

/// Hover adjusts chrome contrast without recoloring guides on selected or active rows alone.
/// The list is a noninteractive text line; the enclosing row owns all input.
fn chrome(spans: Vec<crate::style::Span>, style: crate::style::Style, hovered: bool) -> Element {
    if !hovered {
        return Text::from_spans(spans).height(Length::Px(1)).into();
    }
    crate::widgets::List::new()
        .items([crate::widgets::ListItem::from_spans(spans)])
        .selected(0)
        .selection_style(style)
        .unfocused_selection_style(style)
        .hover_style(crate::style::Style::default())
        .item_hover_style(crate::style::Style::default())
        .focusable(false)
        .tab_stop(false)
        .symbol_column(false)
        .scroll_wheel(false)
        .scroll_keys(ScrollKeymap::NONE)
        .width(Length::Auto)
        .height(Length::Px(1))
        .into()
}

fn row_style(
    ctx: &Context<TreeComponent>,
    entry: &TreeEntry<'_>,
    index: usize,
    selected: Option<usize>,
) -> crate::style::Style {
    let mut style = entry.node.item.style;
    if ctx.state.hovered == Some(index) {
        style = style.patch(crate::style::resolve_slot(
            &ctx.theme(),
            crate::style::ThemeRole::ItemHover,
            &ctx.props.item_hover_style,
        ));
    }
    if selected == Some(index) {
        style = style.patch(crate::style::resolve_selection_slot(
            &ctx.theme(),
            &ctx.props.selection_style,
            &ctx.props.unfocused_selection_style,
            ctx.has_focus_within(),
        ));
    }
    style
}
