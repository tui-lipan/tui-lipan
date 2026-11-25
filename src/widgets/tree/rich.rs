//! Composed rows use the same tree model and selection events as text rows.
use std::rc::Rc;

use super::*;
use crate::callback::Link;
use crate::core::event::{KeyMods, MouseRegionEvent};
use crate::widgets::list::ElementSlot;
use crate::widgets::list::composed::{ComposedListProps, RowRenderer, RowView, text_row};
use crate::widgets::{HStack, MouseRegion, ScrollKeymap, Text, VStack};

pub(super) fn has_content(node: &TreeNode) -> bool {
    node.item.content.is_some()
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
    if let Some(cb) = &ctx.props.on_hover
        && let Some(entry) = entry_at_index(&ctx.props, &ctx.state, index)
    {
        cb.emit(TreeHoverEvent {
            index,
            path: entry.path,
            hovered,
        });
    }
    Update::none()
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

/// A composed row's item: the node's, keyed by its path unless the node has a key of its own, so
/// a row keeps its widgets' state as rows above it expand and collapse.
pub(super) fn item(entry: &TreeEntry<'_>) -> ListItem {
    let mut item = entry.node.item.clone();
    if item.key.is_none() {
        item.key = Some(format!("tree-path-{:?}", entry.path.segments()).into());
    }
    item
}

/// Composed rows render through the list's composed-row engine, which owns selection, hover,
/// clicks, and navigation. The tree adds its guides, arrows, and icons in front of each row.
pub(super) fn view(
    ctx: &Context<TreeComponent>,
    list: crate::widgets::List,
    entries: &[TreeEntry<'_>],
    rows: &ListRows,
    max_depth: usize,
) -> Element {
    let mut parts: Vec<Option<Rc<RowParts>>> = vec![None; list.items.len()];
    let mut hoverable = vec![false; list.items.len()];
    for (index, entry) in entries.iter().enumerate() {
        if let Some(position) = rows.position(index) {
            parts[position] = Some(Rc::new(RowParts::new(ctx, entry, index, max_depth)));
            hoverable[position] = row_interactive(entry);
        }
    }
    let parts: Rc<[Option<Rc<RowParts>>]> = parts.into();
    let link = ctx.link().clone();
    let ctrl = ctx.props.on_ctrl_activate.is_some();
    let mut props = ComposedListProps::new(list);
    props.render = Some(RowRenderer::new(move |row: RowView<'_>| {
        match parts.get(row.index).cloned().flatten() {
            Some(parts) => parts.render(&row, &link, ctrl),
            None => text_row(row.item),
        }
    }));
    props.hoverable = Some(hoverable.into());
    props.hover_effect = ctx.props.item_hover_effect.clone();
    props.on_viewport_change = ctx.props.on_viewport_change.clone();
    props.key_interceptor = ctx.props.key_interceptor.clone();
    props.focus_key = ctx.props.focus_key.clone();
    props.gap = ctx.props.gap;
    if ctx.props.on_pointer_move.is_some() {
        let rows = rows.clone();
        props.on_pointer_move = Some(
            ctx.link()
                .callback_opt(move |position| rows.row_at(position).map(TreeMsg::PointerMove)),
        );
    }
    props.into_element()
}

/// What stands in front of a composed row's content, resolved when the tree renders. Hover and
/// selection arrive later, with the row.
struct RowParts {
    index: usize,
    prefix: Prefix,
    collapsible: bool,
    selectable: bool,
    icon: Option<ElementSlot>,
    leading: Option<ElementSlot>,
    /// The prefix of each extra line.
    continuation: Vec<Span>,
}

impl RowParts {
    fn new(
        ctx: &Context<TreeComponent>,
        entry: &TreeEntry<'_>,
        index: usize,
        max_depth: usize,
    ) -> Self {
        let prefix = prefix(entry, &ctx.props, max_depth);
        let continuation = continuation_spans(entry, &ctx.props, max_depth, prefix.width());
        Self {
            index,
            prefix,
            collapsible: entry.node.is_collapsible(),
            selectable: entry.node.item.is_selectable(),
            icon: entry.node.icon.clone(),
            leading: entry.node.leading.clone(),
            continuation,
        }
    }

    fn render(&self, row: &RowView<'_>, link: &Link<TreeMsg>, ctrl: bool) -> Element {
        let content = row
            .item
            .content
            .as_ref()
            .map(|content| content.element())
            .unwrap_or_else(|| text_row(row.item));
        let height = row.item.line_count().min(u16::MAX as usize) as u16;
        let (style, hovered) = (row.style, row.hovered);
        let prefix = &self.prefix;
        let blank =
            |cells: u16| chrome(vec![Span::new(" ".repeat(cells as usize))], style, hovered);

        let mut first = HStack::new().gap(0).height(Length::Px(1)).child(chrome(
            prefix.guides.clone(),
            style,
            hovered,
        ));
        if !prefix.arrow.is_empty() {
            let arrow = chrome(prefix.arrow.clone(), style, hovered);
            first = if self.collapsible {
                first.child(self.arrow_toggle(arrow, link, ctrl))
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
            if let Some(icon) = &self.icon {
                cell = cell.child(icon.element());
            }
            first = first.child(cell);
            if icon_gap > 0 {
                first = first.child(blank(icon_gap));
            }
        }

        let mut column = VStack::new()
            .gap(0)
            .width(Length::Px(prefix.width()))
            .height(Length::Px(height))
            .child(first);
        for _ in 1..height {
            column = column.child(chrome(self.continuation.clone(), style, hovered));
        }
        let mut row = HStack::new().gap(0).height(Length::Px(height));
        if let Some(leading) = &self.leading {
            row = row.child(leading.element());
        }
        row.child(column).child(content).into()
    }

    /// The arrow of a collapsible row toggles it; with Ctrl activation, a Ctrl+click activates it.
    fn arrow_toggle(&self, arrow: Element, link: &Link<TreeMsg>, ctrl: bool) -> Element {
        let (index, selectable) = (self.index, self.selectable);
        let link = link.clone();
        MouseRegion::new()
            .child(arrow)
            .on_click(crate::callback::Callback::new(
                move |mouse: MouseRegionEvent| {
                    if ctrl && mouse.mods == KeyMods::CTRL {
                        if selectable {
                            link.send(TreeMsg::Select(index));
                        }
                        link.send(TreeMsg::CtrlActivate(index));
                    } else {
                        link.send(TreeMsg::Toggle(index));
                    }
                },
            ))
            .into()
    }
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

fn row_interactive(entry: &TreeEntry<'_>) -> bool {
    entry.node.item.is_selectable() || entry.node.is_collapsible()
}
