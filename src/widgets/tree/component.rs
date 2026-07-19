//! Tree component implementation.

use super::types::*;
use crate::core::component::{Component, Context, Update};
use crate::core::element::Element;
use crate::style::{Span, Style};
use crate::widgets::ListItem;
use std::borrow::Cow;
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub(crate) struct TreeState {
    pub expanded: HashSet<TreePath>,
    pub selected: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum TreeMsg {
    Select(usize),
    Activate(usize),
    Action(TreeAction),
}

pub(crate) struct TreeComponent;

impl TreeComponent {
    pub fn new() -> Self {
        Self
    }
}

impl Component for TreeComponent {
    type Message = TreeMsg;
    type Properties = TreeProps;
    type State = TreeState;

    fn create_state(&self, props: &Self::Properties) -> Self::State {
        TreeState {
            expanded: expanded_paths_from_root(&props.root),
            selected: props.selected.unwrap_or(0),
        }
    }

    fn on_props_changed(
        &mut self,
        old_props: &Self::Properties,
        ctx: &mut Context<Self>,
    ) -> Update {
        let old_expanded = expanded_paths_from_root(&old_props.root);
        let next_expanded = expanded_paths_from_root(&ctx.props.root);

        if old_expanded != next_expanded {
            ctx.state.expanded = next_expanded;
            if let Some(selected) = ctx.props.selected {
                ctx.state.selected = selected;
            }
            return Update::full();
        }

        if old_props.selected != ctx.props.selected
            && let Some(selected) = ctx.props.selected
        {
            ctx.state.selected = selected;
            return Update::full();
        }

        Update::none()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            TreeMsg::Select(index) => {
                ctx.state.selected = index;
                if let Some(cb) = ctx.props.on_select.as_ref()
                    && let Some(entry) = entry_at_index(&ctx.props, &ctx.state, index)
                {
                    cb.emit(TreeEvent {
                        index,
                        path: entry.path.clone(),
                    });
                }
                Update::full()
            }
            TreeMsg::Activate(index) => {
                let Some(entry) = entry_at_index(&ctx.props, &ctx.state, index) else {
                    return Update::none();
                };

                if let Some(cb) = ctx.props.on_activate.as_ref() {
                    cb.emit(TreeEvent {
                        index,
                        path: entry.path.clone(),
                    });
                }

                if entry.node.children.is_empty() {
                    return Update::full();
                }

                let next = toggle_path(&mut ctx.state.expanded, &entry.path);
                if let Some(cb) = ctx.props.on_toggle.as_ref() {
                    cb.emit(TreeToggleEvent {
                        index,
                        path: entry.path.clone(),
                        expanded: next,
                    });
                }
                Update::full()
            }
            TreeMsg::Action(action) => {
                let selected = selected_index(ctx);
                let expanded_paths = resolved_expanded(ctx);
                let entries = flatten_tree(&ctx.props.root, expanded_paths.as_ref());
                let Some(entry) = entries.get(selected).cloned() else {
                    return Update::none();
                };

                let is_currently_expanded = ctx.state.expanded.contains(&entry.path);

                match action {
                    TreeAction::Toggle => {
                        let next = toggle_path(&mut ctx.state.expanded, &entry.path);
                        if let Some(cb) = ctx.props.on_toggle.as_ref() {
                            cb.emit(TreeToggleEvent {
                                index: selected,
                                path: entry.path.clone(),
                                expanded: next,
                            });
                        }
                    }
                    TreeAction::Expand => {
                        if !is_currently_expanded {
                            if !entry.node.children.is_empty() {
                                ctx.state.expanded.insert(entry.path.clone());
                                if let Some(cb) = ctx.props.on_toggle.as_ref() {
                                    cb.emit(TreeToggleEvent {
                                        index: selected,
                                        path: entry.path.clone(),
                                        expanded: true,
                                    });
                                }
                            }
                        } else {
                            // Already expanded, select first child
                            if selected + 1 < entries.len() {
                                let next_entry = &entries[selected + 1];
                                // Check if it's actually a child
                                if next_entry
                                    .path
                                    .segments()
                                    .starts_with(entry.path.segments())
                                {
                                    ctx.state.selected = selected + 1;
                                }
                            }
                        }
                    }
                    TreeAction::Collapse => {
                        if is_currently_expanded {
                            ctx.state.expanded.remove(&entry.path);
                            if let Some(cb) = ctx.props.on_toggle.as_ref() {
                                cb.emit(TreeToggleEvent {
                                    index: selected,
                                    path: entry.path.clone(),
                                    expanded: false,
                                });
                            }
                        } else {
                            // Collapse parent
                            if !entry.path.segments().is_empty() {
                                let mut parent_segments = entry.path.segments().to_vec();
                                parent_segments.pop();
                                let parent_path = TreePath::from(parent_segments);
                                ctx.state.expanded.remove(&parent_path);

                                // Select parent
                                if let Some(parent_idx) =
                                    entries.iter().position(|e| e.path == parent_path)
                                {
                                    ctx.state.selected = parent_idx;

                                    if let Some(cb) = ctx.props.on_toggle.as_ref() {
                                        cb.emit(TreeToggleEvent {
                                            index: parent_idx,
                                            path: parent_path,
                                            expanded: false,
                                        });
                                    }
                                }
                            }
                        }
                    }
                };

                Update::full()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let expanded = resolved_expanded(ctx);
        let entries = flatten_tree(&ctx.props.root, expanded.as_ref());
        let max_depth = entries.iter().map(|entry| entry.depth).max().unwrap_or(0);
        let items = entries
            .iter()
            .map(|entry| build_item(entry, &ctx.props, max_depth))
            .collect::<Vec<_>>();

        let selected = selected_index(ctx).min(items.len().saturating_sub(1));

        let on_select = ctx
            .link()
            .callback(|event: crate::widgets::ListEvent| TreeMsg::Select(event.index));
        let on_activate = ctx
            .link()
            .callback(|event: crate::widgets::ListEvent| TreeMsg::Activate(event.index));

        let keymap = ctx.props.keymap;
        let interceptor = ctx.props.key_interceptor.clone();
        let tree_on_key = ctx
            .link()
            .key_handler(move |key| tree_action_from_key(&key, keymap).map(TreeMsg::Action));
        let on_key = match interceptor {
            Some(interceptor) => crate::callback::KeyHandler::new(move |key| {
                if interceptor.handle(key) {
                    true
                } else {
                    tree_on_key.handle(key)
                }
            }),
            None => tree_on_key,
        };

        let mut list = crate::widgets::List::new()
            .items(items)
            .selected(selected)
            .style(ctx.props.style)
            .hover_style_slot(ctx.props.hover_style)
            .item_hover_style_slot(ctx.props.item_hover_style)
            .selection_full_width(ctx.props.selection_full_width)
            .unselected_symbol(ctx.props.unselected_symbol.clone())
            .width(ctx.props.width)
            .height(ctx.props.height)
            .force_scroll_to_selected(ctx.props.force_scroll_to_selected)
            .scrollbar(ctx.props.scrollbar)
            .scrollbar_config(ctx.props.scrollbar_config.clone())
            // Disable horizontal VIM keys (h/l) so Tree can use them for expand/collapse
            // while keeping vertical VIM keys (j/k) for navigation
            .scroll_keys(ctx.props.scroll_keys.without_vim_horizontal())
            .scroll_wheel(ctx.props.scroll_wheel)
            .show_scroll_indicators(ctx.props.show_scroll_indicators)
            .scroll_indicator_style(ctx.props.scroll_indicator_style)
            .activate_on_click(ctx.props.activate_on_click)
            .focusable(ctx.props.focusable)
            .tab_stop(ctx.props.tab_stop)
            .on_select(on_select)
            .on_activate(on_activate)
            .on_key(on_key);
        if let Some(cb) = ctx.props.on_focus.clone() {
            list = list.on_focus(cb);
        }
        if let Some(cb) = ctx.props.on_blur.clone() {
            list = list.on_blur(cb);
        }
        list = list
            .selection_style_slot(ctx.props.selection_style)
            .unfocused_selection_style_slot(ctx.props.unfocused_selection_style);

        if let Some(symbol) = ctx.props.selection_symbol.clone() {
            list = list.selection_symbol(Some(symbol));
        }
        if let Some(style) = ctx.props.selection_symbol_style {
            list = list.selection_symbol_style(style);
        }
        if let Some(style) = ctx.props.unfocused_selection_symbol_style {
            list = list.unfocused_selection_symbol_style(style);
        }
        if let Some(text) = ctx.props.empty_text.clone() {
            list = list
                .empty_text(text)
                .empty_text_style(ctx.props.empty_text_style);
        }

        let mut stack = crate::widgets::VStack::new().gap(ctx.props.gap);
        stack = stack.child(list);
        stack.into()
    }
}

#[derive(Clone, Debug)]
struct TreeEntry<'a> {
    path: TreePath,
    depth: usize,
    node: &'a TreeNode,
    expanded: bool,
    continued_depths: Vec<bool>,
    is_last_child: bool,
}

fn collect_expanded_paths(node: &TreeNode, path: &mut Vec<usize>, out: &mut HashSet<TreePath>) {
    if node.expanded {
        out.insert(TreePath::from(path.clone()));
    }

    for (idx, child) in node.children.iter().enumerate() {
        path.push(idx);
        collect_expanded_paths(child, path, out);
        path.pop();
    }
}

fn expanded_paths_from_root(root: &TreeNode) -> HashSet<TreePath> {
    let mut expanded = HashSet::new();
    let mut path = Vec::new();
    collect_expanded_paths(root, &mut path, &mut expanded);
    expanded
}

fn flatten_tree<'a>(root: &'a TreeNode, expanded: &HashSet<TreePath>) -> Vec<TreeEntry<'a>> {
    let mut out = Vec::new();
    let mut path = Vec::new();
    let mut continued_depths = Vec::new();
    push_entries(
        root,
        &mut path,
        0,
        expanded,
        &mut continued_depths,
        true,
        &mut out,
    );
    out
}

fn resolved_expanded(ctx: &Context<TreeComponent>) -> Cow<'_, HashSet<TreePath>> {
    let Some(policy) = ctx.props.focus_policy else {
        return Cow::Borrowed(&ctx.state.expanded);
    };
    if !ctx.has_focus_within() {
        return Cow::Borrowed(&ctx.state.expanded);
    }

    let available = ctx.viewport().h;
    if available >= policy.squash_threshold {
        return Cow::Borrowed(&ctx.state.expanded);
    }

    let Some(entry) = entry_at_index(&ctx.props, &ctx.state, ctx.state.selected) else {
        return Cow::Borrowed(&ctx.state.expanded);
    };

    let mut constrained = HashSet::new();
    constrained.insert(TreePath::from(Vec::new()));
    let mut path = Vec::new();
    for segment in entry.path.segments() {
        path.push(*segment);
        constrained.insert(TreePath::from(path.clone()));
    }
    Cow::Owned(constrained)
}

fn selected_index(ctx: &Context<TreeComponent>) -> usize {
    ctx.props.selected.unwrap_or(ctx.state.selected)
}

fn push_entries<'a>(
    node: &'a TreeNode,
    path: &mut Vec<usize>,
    depth: usize,
    expanded: &HashSet<TreePath>,
    continued_depths: &mut Vec<bool>,
    is_last_child: bool,
    out: &mut Vec<TreeEntry<'a>>,
) {
    let path_key = TreePath::from(path.clone());
    let is_expanded = expanded.contains(&path_key);
    out.push(TreeEntry {
        path: path_key,
        depth,
        node,
        expanded: is_expanded,
        continued_depths: continued_depths.clone(),
        is_last_child,
    });

    if is_expanded {
        let child_count = node.children.len();
        for (idx, child) in node.children.iter().enumerate() {
            let last = idx == child_count - 1;
            path.push(idx);
            continued_depths.push(!last);
            push_entries(
                child,
                path,
                depth + 1,
                expanded,
                continued_depths,
                last,
                out,
            );
            continued_depths.pop();
            path.pop();
        }
    }
}

fn entry_at_index<'a>(
    props: &'a TreeProps,
    state: &TreeState,
    index: usize,
) -> Option<TreeEntry<'a>> {
    let entries = flatten_tree(&props.root, &state.expanded);
    entries.into_iter().nth(index)
}

fn toggle_path(expanded: &mut HashSet<TreePath>, path: &TreePath) -> bool {
    if expanded.contains(path) {
        expanded.remove(path);
        false
    } else {
        expanded.insert(path.clone());
        true
    }
}

fn build_item(entry: &TreeEntry<'_>, props: &TreeProps, max_depth: usize) -> ListItem {
    let mut spans = Vec::new();

    if props.indent_style != IndentStyle::None && entry.depth > 0 {
        for i in 0..entry.depth - 1 {
            if entry.continued_depths[i] {
                spans.push(Span::new("│").style(depth_guide_style(props, i + 1, max_depth)));
            } else {
                spans.push(Span::new(" "));
            }
            let gap = entry.node.indent.saturating_sub(1);
            if gap > 0 {
                spans.push(Span::new(" ".repeat(gap as usize)));
            }
        }

        // Branch connector for the current level
        match props.indent_style {
            IndentStyle::None => {} // Handled by outer if
            IndentStyle::Line => {
                spans.push(Span::new("│").style(depth_guide_style(props, entry.depth, max_depth)));
            }
            IndentStyle::Short => {
                if entry.is_last_child {
                    spans.push(Span::new("└").style(depth_guide_style(
                        props,
                        entry.depth,
                        max_depth,
                    )));
                } else {
                    spans.push(Span::new("├").style(depth_guide_style(
                        props,
                        entry.depth,
                        max_depth,
                    )));
                }
            }
            IndentStyle::ShortRounded => {
                if entry.is_last_child {
                    spans.push(Span::new("╰").style(depth_guide_style(
                        props,
                        entry.depth,
                        max_depth,
                    )));
                } else {
                    spans.push(Span::new("├").style(depth_guide_style(
                        props,
                        entry.depth,
                        max_depth,
                    )));
                }
            }
            IndentStyle::Long => {
                if entry.is_last_child {
                    spans.push(Span::new("└─").style(depth_guide_style(
                        props,
                        entry.depth,
                        max_depth,
                    )));
                } else {
                    spans.push(Span::new("├─").style(depth_guide_style(
                        props,
                        entry.depth,
                        max_depth,
                    )));
                }
            }
            IndentStyle::LongRounded => {
                if entry.is_last_child {
                    spans.push(Span::new("╰─").style(depth_guide_style(
                        props,
                        entry.depth,
                        max_depth,
                    )));
                } else {
                    spans.push(Span::new("├─").style(depth_guide_style(
                        props,
                        entry.depth,
                        max_depth,
                    )));
                }
            }
        }

        let consumed = match props.indent_style {
            IndentStyle::Long | IndentStyle::LongRounded => 2,
            IndentStyle::Short | IndentStyle::ShortRounded | IndentStyle::Line => 1,
            _ => 0,
        };

        let gap = entry.node.indent.saturating_sub(consumed);
        if gap > 0 {
            spans.push(indent_connector_gap_span(entry, props, max_depth, gap));
        }
    } else {
        let indent = entry.node.indent.saturating_mul(entry.depth as u16);
        if indent > 0 {
            spans.push(Span::new(" ".repeat(indent as usize)));
        }
    }

    if entry.node.leading_guide_fill_cells > 0 {
        spans.extend(leading_guide_fill_spans(entry, props, max_depth));
    }

    if props.show_icons {
        let icon = if entry.node.children.is_empty() {
            props.leaf_icon.clone().unwrap_or_else(|| "".into())
        } else if entry.expanded {
            props.expanded_icon.clone()
        } else {
            props.collapsed_icon.clone()
        };

        if !icon.is_empty() {
            spans.push(Span::new(icon).style(props.icon_style));
        }

        if props.icon_gap > 0 {
            spans.push(Span::new(" ".repeat(props.icon_gap as usize)));
        }
    }

    spans.extend(entry.node.item.spans.iter().cloned());

    let mut item = ListItem::from_spans(spans)
        .description_spans(entry.node.item.description_spans.clone())
        .style(entry.node.item.style)
        .role(entry.node.item.role)
        .active(entry.node.item.active)
        .primary_selection_label(entry.node.item.primary_selection_label)
        .primary_selection_description(entry.node.item.primary_selection_description)
        .primary_hover_label(entry.node.item.primary_hover_label)
        .primary_hover_description(entry.node.item.primary_hover_description)
        .primary_truncate_description_first(entry.node.item.primary_truncate_description_first)
        .primary_wrap_description(entry.node.item.primary_wrap_description)
        .symbol_line(entry.node.item.symbol_line);

    for line in &entry.node.item.extra_lines {
        item = item.line(line.clone());
    }

    item
}

fn indent_connector_gap_span(
    entry: &TreeEntry<'_>,
    props: &TreeProps,
    max_depth: usize,
    gap: u16,
) -> Span {
    let draw_solid_gap = props.solid_indent_connector_gap
        && matches!(
            props.indent_style,
            IndentStyle::Short
                | IndentStyle::ShortRounded
                | IndentStyle::Long
                | IndentStyle::LongRounded
        );
    let content = if draw_solid_gap {
        "─".repeat(gap as usize)
    } else {
        " ".repeat(gap as usize)
    };

    let span = Span::new(content);
    if draw_solid_gap {
        span.style(depth_guide_style(props, entry.depth, max_depth))
    } else {
        span
    }
}

fn leading_guide_fill_spans(
    entry: &TreeEntry<'_>,
    props: &TreeProps,
    max_depth: usize,
) -> Vec<Span> {
    let cells = entry.node.leading_guide_fill_cells as usize;
    if cells == 0 {
        return Vec::new();
    }

    match props.indent_style {
        IndentStyle::Short
        | IndentStyle::ShortRounded
        | IndentStyle::Long
        | IndentStyle::LongRounded => {
            let fill_cells = cells.saturating_sub(1);
            let mut spans = Vec::new();
            if fill_cells > 0 {
                spans.push(Span::new("─".repeat(fill_cells)).style(depth_guide_style(
                    props,
                    entry.depth,
                    max_depth,
                )));
            }
            spans.push(Span::new(" "));
            spans
        }
        IndentStyle::Line => {
            let fill_cells = cells.saturating_sub(1);
            let mut spans = Vec::new();
            if fill_cells > 0 {
                spans.push(Span::new("│".repeat(fill_cells)).style(depth_guide_style(
                    props,
                    entry.depth,
                    max_depth,
                )));
            }
            spans.push(Span::new(" "));
            spans
        }
        IndentStyle::None => vec![Span::new(" ".repeat(cells))],
    }
}

fn depth_guide_style(props: &TreeProps, depth: usize, max_depth: usize) -> Style {
    let mut style = props.indent_guide_style;
    if let Some(gradient) = props.indent_gradient {
        let t = if max_depth <= 1 {
            1.0
        } else {
            let depth_idx = depth.saturating_sub(1).min(max_depth.saturating_sub(1));
            depth_idx as f64 / max_depth.saturating_sub(1) as f64
        };
        style = style.patch(Style::new().fg(gradient.color_at(t)));
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::{ListItemGutter, ListItemLine};

    #[test]
    fn build_item_documents_current_left_side_fields_dropped() {
        // Tracking test for the List leading-column refactor: Tree currently
        // rebuilds ListItem rows and intentionally does not copy the full
        // left-side ListItem model through. A future Tree fix should update
        // these assertions deliberately rather than changing rendering
        // incidentally.
        let source = ListItem::new("Node")
            .status_symbol("!")
            .gutter(ListItemGutter::text("●"))
            .gutter_line(1)
            .prefix("2 ")
            .prefix_style(Style::new().bold())
            .extra_line_indent(4)
            .primary_wrap_label(true)
            .primary_wrap_description(true)
            .primary_max_label_width(3)
            .primary_max_description_width(4)
            .symbol_line(1)
            .line(
                ListItemLine::new("extra")
                    .wrap_label(true)
                    .max_label_width(2),
            );
        let root = TreeNode::new(source).expanded(true);
        let props = crate::widgets::Tree::new(root.clone())
            .show_icons(false)
            .props;
        let entry = TreeEntry {
            path: TreePath::from(Vec::new()),
            depth: 0,
            node: &root,
            expanded: true,
            continued_depths: Vec::new(),
            is_last_child: true,
        };

        let out = build_item(&entry, &props, 0);

        assert!(out.status.is_none());
        assert!(out.gutter.is_none());
        assert_eq!(out.gutter_line, 0);
        assert!(out.prefix.is_none());
        assert!(out.prefix_style.is_none());
        assert_eq!(out.extra_line_indent, 0);
        assert!(!out.primary_wrap_label);
        assert!(out.primary_wrap_description);
        assert_eq!(out.primary_max_label_width, None);
        assert_eq!(out.primary_max_description_width, None);
        assert_eq!(out.symbol_line, 1);
        assert_eq!(out.extra_lines.len(), 1);
        assert!(out.extra_lines[0].wrap_label);
        assert_eq!(out.extra_lines[0].max_label_width, Some(2));
    }

    #[test]
    fn selected_prop_controls_inner_list_selection() {
        let root = TreeNode::new("root")
            .expanded(true)
            .child(TreeNode::new("a"))
            .child(TreeNode::new("b"));
        let props = crate::widgets::Tree::new(root).selected(2).props;
        let component = TreeComponent::new();
        let state = component.create_state(&props);

        assert_eq!(state.selected, 2);
    }

    #[test]
    fn solid_indent_connector_gap_fills_short_connector_spacing() {
        let child = TreeNode::new("leaf").leading_guide_fill_cells(2);
        let props = crate::widgets::Tree::new(child.clone())
            .show_icons(false)
            .indent_style(IndentStyle::Short)
            .indent_guide_style(Style::new().bold())
            .solid_indent_connector_gap(true)
            .props;
        let entry = TreeEntry {
            path: TreePath::from(vec![0]),
            depth: 1,
            node: &child,
            expanded: false,
            continued_depths: vec![false],
            is_last_child: false,
        };

        let out = build_item(&entry, &props, 1);
        let content: Vec<&str> = out.spans.iter().map(|span| span.content.as_ref()).collect();

        assert_eq!(content, vec!["├", "─", "─", " ", "leaf"]);
        assert_eq!(out.spans[1].style, props.indent_guide_style);
        assert_eq!(out.spans[2].style, props.indent_guide_style);
    }

    #[test]
    fn short_rounded_indent_uses_rounded_last_child_elbow() {
        let child = TreeNode::new("leaf");
        let props = crate::widgets::Tree::new(child.clone())
            .show_icons(false)
            .indent_style(IndentStyle::ShortRounded)
            .props;
        let entry = TreeEntry {
            path: TreePath::from(vec![0]),
            depth: 1,
            node: &child,
            expanded: false,
            continued_depths: vec![false],
            is_last_child: true,
        };

        let out = build_item(&entry, &props, 1);
        let content: Vec<&str> = out.spans.iter().map(|span| span.content.as_ref()).collect();

        assert_eq!(content, vec!["╰", " ", "leaf"]);
    }

    #[test]
    fn short_rounded_indent_uses_square_non_last_branch() {
        let child = TreeNode::new("leaf");
        let props = crate::widgets::Tree::new(child.clone())
            .show_icons(false)
            .indent_style(IndentStyle::ShortRounded)
            .props;
        let entry = TreeEntry {
            path: TreePath::from(vec![0]),
            depth: 1,
            node: &child,
            expanded: false,
            continued_depths: vec![false],
            is_last_child: false,
        };

        let out = build_item(&entry, &props, 1);
        let content: Vec<&str> = out.spans.iter().map(|span| span.content.as_ref()).collect();

        assert_eq!(content, vec!["├", " ", "leaf"]);
    }

    #[test]
    fn long_rounded_indent_uses_rounded_last_child_elbow() {
        let child = TreeNode::new("leaf");
        let props = crate::widgets::Tree::new(child.clone())
            .show_icons(false)
            .indent_style(IndentStyle::LongRounded)
            .props;
        let entry = TreeEntry {
            path: TreePath::from(vec![0]),
            depth: 1,
            node: &child,
            expanded: false,
            continued_depths: vec![false],
            is_last_child: true,
        };

        let out = build_item(&entry, &props, 1);
        let content: Vec<&str> = out.spans.iter().map(|span| span.content.as_ref()).collect();

        assert_eq!(content, vec!["╰─", "leaf"]);
    }

    #[test]
    fn long_rounded_indent_uses_square_non_last_branch() {
        let child = TreeNode::new("leaf");
        let props = crate::widgets::Tree::new(child.clone())
            .show_icons(false)
            .indent_style(IndentStyle::LongRounded)
            .props;
        let entry = TreeEntry {
            path: TreePath::from(vec![0]),
            depth: 1,
            node: &child,
            expanded: false,
            continued_depths: vec![false],
            is_last_child: false,
        };

        let out = build_item(&entry, &props, 1);
        let content: Vec<&str> = out.spans.iter().map(|span| span.content.as_ref()).collect();

        assert_eq!(content, vec!["├─", "leaf"]);
    }

    #[test]
    fn long_rounded_leading_guide_fill_stays_solid_before_content() {
        let child = TreeNode::new("leaf").leading_guide_fill_cells(2);
        let props = crate::widgets::Tree::new(child.clone())
            .show_icons(false)
            .indent_style(IndentStyle::LongRounded)
            .indent_guide_style(Style::new().bold())
            .solid_indent_connector_gap(true)
            .props;
        let entry = TreeEntry {
            path: TreePath::from(vec![0]),
            depth: 1,
            node: &child,
            expanded: false,
            continued_depths: vec![false],
            is_last_child: true,
        };

        let out = build_item(&entry, &props, 1);
        let content: Vec<&str> = out.spans.iter().map(|span| span.content.as_ref()).collect();

        assert_eq!(content, vec!["╰─", "─", " ", "leaf"]);
        assert_eq!(out.spans[1].style, props.indent_guide_style);
    }

    #[test]
    fn leading_guide_fill_makes_long_connector_solid_before_content() {
        let child = TreeNode::new("leaf").leading_guide_fill_cells(2);
        let props = crate::widgets::Tree::new(child.clone())
            .show_icons(false)
            .indent_style(IndentStyle::Long)
            .indent_guide_style(Style::new().bold())
            .solid_indent_connector_gap(true)
            .props;
        let entry = TreeEntry {
            path: TreePath::from(vec![0]),
            depth: 1,
            node: &child,
            expanded: false,
            continued_depths: vec![false],
            is_last_child: false,
        };

        let out = build_item(&entry, &props, 1);
        let content: Vec<&str> = out.spans.iter().map(|span| span.content.as_ref()).collect();

        assert_eq!(content, vec!["├─", "─", " ", "leaf"]);
        assert_eq!(out.spans[1].style, props.indent_guide_style);
    }

    #[test]
    fn leading_guide_fill_keeps_plain_spacing_without_guides() {
        let child = TreeNode::new("leaf").leading_guide_fill_cells(2);
        let props = crate::widgets::Tree::new(child.clone())
            .show_icons(false)
            .indent_style(IndentStyle::None)
            .solid_indent_connector_gap(true)
            .props;
        let entry = TreeEntry {
            path: TreePath::from(vec![0]),
            depth: 1,
            node: &child,
            expanded: false,
            continued_depths: vec![false],
            is_last_child: false,
        };

        let out = build_item(&entry, &props, 1);
        let content: Vec<&str> = out.spans.iter().map(|span| span.content.as_ref()).collect();

        assert_eq!(content, vec!["  ", "  ", "leaf"]);
    }

    #[test]
    fn leading_guide_fill_uses_line_guide_without_collapsing_connector_gap() {
        let child = TreeNode::new("leaf").leading_guide_fill_cells(2);
        let props = crate::widgets::Tree::new(child.clone())
            .show_icons(false)
            .indent_style(IndentStyle::Line)
            .indent_guide_style(Style::new().bold())
            .solid_indent_connector_gap(true)
            .props;
        let entry = TreeEntry {
            path: TreePath::from(vec![0]),
            depth: 1,
            node: &child,
            expanded: false,
            continued_depths: vec![false],
            is_last_child: false,
        };

        let out = build_item(&entry, &props, 1);
        let content: Vec<&str> = out.spans.iter().map(|span| span.content.as_ref()).collect();

        assert_eq!(content, vec!["│", " ", "│", " ", "leaf"]);
        assert_eq!(out.spans[2].style, props.indent_guide_style);
    }
}
