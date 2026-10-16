//! Tree component implementation.

#[path = "rich.rs"]
mod rich;

use super::guides::{TreeBranch, TreeGuides};
use super::types::*;
use crate::core::component::{Component, Context, Update};
use crate::core::element::{Element, IntoElement};
use crate::core::event::{KeyCode, KeyEvent};
use crate::style::{Length, Span};
use crate::widgets::ListItem;
use std::borrow::Cow;
use std::collections::HashSet;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug)]
pub(crate) struct TreeState {
    pub expanded: HashSet<TreePath>,
    pub selected: Option<usize>,
    pub hovered: Option<usize>,
}

#[derive(Clone, Debug)]
pub(crate) enum TreeMsg {
    Select(usize),
    Activate(usize),
    CtrlActivate(usize),
    Action(TreeAction),
    Toggle(usize),
    Hover(usize, bool),
    PointerMove(usize),
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
            hovered: None,
            expanded: expanded_paths_from_root(&props.root),
            selected: if props.clear_selection {
                None
            } else {
                Some(props.selected.unwrap_or(0))
            },
        }
    }

    fn on_props_changed(
        &mut self,
        old_props: &Self::Properties,
        ctx: &mut Context<Self>,
    ) -> Update {
        let old_expanded = expanded_paths_from_root(&old_props.root);
        let next_expanded = expanded_paths_from_root(&ctx.props.root);

        if ctx.props.controlled_expansion || old_expanded != next_expanded {
            ctx.state.expanded = next_expanded;
            sync_selection_from_props(&ctx.props, &mut ctx.state);
            return Update::full();
        }

        if old_props.clear_selection != ctx.props.clear_selection {
            sync_selection_from_props(&ctx.props, &mut ctx.state);
            return Update::full();
        }

        if old_props.selected != ctx.props.selected
            && let Some(selected) = ctx.props.selected
        {
            ctx.state.selected = Some(selected);
            return Update::full();
        }

        Update::none()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            TreeMsg::Toggle(index) => rich::toggle(ctx, index),
            TreeMsg::Hover(index, hovered) => rich::hover(ctx, index, hovered),
            TreeMsg::PointerMove(index) => rich::pointer_move(ctx, index),
            TreeMsg::Select(index) => {
                ctx.state.selected = Some(index);
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
            TreeMsg::CtrlActivate(index) => {
                if let Some(entry) = entry_at_index(&ctx.props, &ctx.state, index)
                    .filter(|entry| entry.node.item.is_selectable())
                    && let Some(cb) = &ctx.props.on_ctrl_activate
                {
                    cb.emit(TreeEvent {
                        index,
                        path: entry.path,
                    });
                }
                Update::none()
            }
            TreeMsg::Activate(index) => {
                let Some(entry) = entry_at_index(&ctx.props, &ctx.state, index)
                    .filter(|entry| entry.node.item.is_selectable())
                else {
                    return Update::none();
                };

                if let Some(cb) = ctx.props.on_activate.as_ref() {
                    cb.emit(TreeEvent {
                        index,
                        path: entry.path.clone(),
                    });
                }

                if !ctx.props.toggle_on_activate || !entry.node.is_collapsible() {
                    return Update::full();
                }

                rich::toggle(ctx, index)
            }
            TreeMsg::Action(action) => rich::action(ctx, action),
        }
    }

    fn on_key(
        &mut self,
        key: KeyEvent,
        ctx: &mut Context<Self>,
    ) -> crate::core::component::KeyUpdate {
        if rich::has_content(&ctx.props.root) {
            return rich::on_key(self, key, ctx);
        }
        crate::core::component::KeyUpdate::unhandled(Update::none())
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        if rich::has_content(&ctx.props.root) {
            return rich::view(ctx);
        }
        let expanded = resolved_expanded(ctx);
        let entries = visible_entries(&ctx.props, expanded.as_ref());
        let max_depth = entries
            .iter()
            .map(|entry| entry.branch.depth())
            .max()
            .unwrap_or(0);
        let items = entries
            .iter()
            .map(|entry| build_item(entry, &ctx.props, max_depth))
            .collect::<Vec<_>>();

        let selected = selected_index(ctx).map(|s| s.min(items.len().saturating_sub(1)));

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
        if ctx.props.on_ctrl_activate.is_some() {
            list =
                list.on_ctrl_activate(ctx.link().callback(|event: crate::widgets::ListEvent| {
                    TreeMsg::CtrlActivate(event.index)
                }));
        }
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
        let list: Element = if let Some(key) = ctx.props.focus_key.clone() {
            list.key(key)
        } else {
            list.into()
        };
        stack = stack.child(list);
        stack.into()
    }
}

#[derive(Clone, Debug)]
struct TreeEntry<'a> {
    path: TreePath,
    node: &'a TreeNode,
    expanded: bool,
    branch: TreeBranch,
    /// Some node in the tree has an icon, so every row keeps the column icons share.
    icon_column: bool,
}

fn collect_expanded_paths(node: &TreeNode, path: &mut Vec<usize>, out: &mut HashSet<TreePath>) {
    // AlwaysOpen is a visibility rule, not authored expansion state. Storing it here
    // would make a mode-only prop change reset unrelated uncontrolled user expansions.
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

fn flatten_tree<'a>(
    root: &'a TreeNode,
    expanded: &HashSet<TreePath>,
    props: &TreeProps,
) -> Vec<TreeEntry<'a>> {
    let mut out = Vec::new();
    let mut path = Vec::new();
    push_entries(
        root,
        &mut path,
        TreeBranch::root(),
        expanded,
        props,
        &mut out,
    );
    out
}

fn visible_entries<'a>(props: &'a TreeProps, expanded: &HashSet<TreePath>) -> Vec<TreeEntry<'a>> {
    let mut entries = nested_entries(props, expanded);
    if props.show_icons && has_icon(&props.root) {
        for entry in &mut entries {
            entry.icon_column = true;
        }
    }
    entries
}

fn has_icon(node: &TreeNode) -> bool {
    node.icon.is_some() || node.children.iter().any(has_icon)
}

fn nested_entries<'a>(props: &'a TreeProps, expanded: &HashSet<TreePath>) -> Vec<TreeEntry<'a>> {
    if !props.forest {
        return flatten_tree(&props.root, expanded, props);
    }
    let mut entries = Vec::new();
    for (index, node) in props.root.children.iter().enumerate() {
        push_entries(
            node,
            &mut vec![index],
            TreeBranch::root(),
            expanded,
            props,
            &mut entries,
        );
    }
    entries
}

fn resolved_expanded(ctx: &Context<TreeComponent>) -> Cow<'_, HashSet<TreePath>> {
    if ctx.props.controlled_expansion {
        return Cow::Owned(expanded_paths_from_root(&ctx.props.root));
    }
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

    let Some(selected) = ctx.state.selected else {
        return Cow::Borrowed(&ctx.state.expanded);
    };
    let Some(entry) = entry_at_index(&ctx.props, &ctx.state, selected) else {
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

/// Reconcile internal selection state with the current props.
///
/// Shared by every `on_props_changed` path so an update that changes several
/// props at once cannot skip part of the sync. Leaving `clear_selection` with no
/// controlled index restores a cursor rather than staying blank.
fn sync_selection_from_props(props: &TreeProps, state: &mut TreeState) {
    if props.clear_selection {
        state.selected = None;
    } else if let Some(selected) = props.selected {
        state.selected = Some(selected);
    } else if state.selected.is_none() {
        state.selected = Some(0);
    }
}

fn selected_index(ctx: &Context<TreeComponent>) -> Option<usize> {
    if ctx.props.clear_selection {
        None
    } else {
        let preferred = ctx.props.selected.or(ctx.state.selected)?;
        let items = visible_entries(&ctx.props, &ctx.state.expanded)
            .iter()
            .map(|entry| entry.node.item.clone())
            .collect::<Vec<_>>();
        crate::widgets::List::nearest_selectable_index(&items, preferred)
    }
}

fn push_entries<'a>(
    node: &'a TreeNode,
    path: &mut Vec<usize>,
    branch: TreeBranch,
    expanded: &HashSet<TreePath>,
    props: &TreeProps,
    out: &mut Vec<TreeEntry<'a>>,
) {
    let path_key = TreePath::from(path.clone());
    let is_expanded =
        node.expansion_mode == TreeExpansionMode::AlwaysOpen || expanded.contains(&path_key);
    let children = is_expanded.then_some(&node.children);
    out.push(TreeEntry {
        path: path_key,
        node,
        expanded: is_expanded,
        branch: branch.clone(),
        icon_column: false,
    });

    if let Some(children) = children {
        let gap = child_gap(node, &branch, props);
        let child_count = children.len();
        for (idx, child) in children.iter().enumerate() {
            path.push(idx);
            push_entries(
                child,
                path,
                branch.child_with_gap(idx == child_count - 1, gap),
                expanded,
                props,
                out,
            );
            path.pop();
        }
    }
}

/// Children hang beneath their parent's first glyph after its branch: its arrow, or, on a row
/// without an arrow column, its icon or label, one content gap further on. A row with nothing in
/// front of it has no gap, so its children hang beneath its first glyph directly.
fn child_gap(node: &TreeNode, branch: &TreeBranch, props: &TreeProps) -> u16 {
    let arrow = props.show_arrows && node.show_arrow;
    let guides = branch.depth() > 0 && props.indent > 0;
    if arrow || !guides {
        0
    } else {
        props.content_gap
    }
}

fn entry_at_index<'a>(
    props: &'a TreeProps,
    state: &TreeState,
    index: usize,
) -> Option<TreeEntry<'a>> {
    let expanded = if props.controlled_expansion {
        Cow::Owned(expanded_paths_from_root(&props.root))
    } else {
        Cow::Borrowed(&state.expanded)
    };
    let entries = visible_entries(props, expanded.as_ref());
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

/// What stands in front of a row's content, left to right: the guides, the arrow column, the
/// content gap, and the icon column with its gap. This is FileTree's row layout; text rows render
/// it as spans and composed rows draw the same cells, with the icon element in its column.
struct Prefix {
    guides: Vec<Span>,
    arrow: Vec<Span>,
    content_gap: u16,
    /// Cells of the icon column and of the gap after it, when the tree shows icons.
    icon: Option<(u16, u16)>,
}

impl Prefix {
    fn width(&self) -> u16 {
        let spans = spans_width(&self.guides) + spans_width(&self.arrow);
        let icon = self.icon.map_or(0, |(width, gap)| width + gap);
        spans.saturating_add(self.content_gap).saturating_add(icon)
    }

    fn into_spans(self) -> Vec<Span> {
        let mut spans = self.guides;
        spans.extend(self.arrow);
        if self.content_gap > 0 {
            spans.push(Span::new(" ".repeat(self.content_gap as usize)));
        }
        if let Some((width, gap)) = self.icon {
            spans.push(Span::new(" ".repeat((width + gap) as usize)));
        }
        spans
    }
}

fn spans_width(spans: &[Span]) -> u16 {
    spans
        .iter()
        .map(|span| span.content.width())
        .sum::<usize>()
        .min(u16::MAX as usize) as u16
}

fn prefix(entry: &TreeEntry<'_>, props: &TreeProps, max_depth: usize) -> Prefix {
    let guides = tree_guides(props);
    let guide_spans = guides.spans(&entry.branch, max_depth);
    let arrow = arrow_spans(entry, props, &guides, max_depth);
    // The gap separates the tree's structure from what the row shows; with nothing in front of
    // the row, there is nothing to separate.
    let structure = spans_width(&guide_spans) + spans_width(&arrow);
    Prefix {
        guides: guide_spans,
        arrow,
        content_gap: if structure > 0 { props.content_gap } else { 0 },
        icon: entry
            .icon_column
            .then_some((props.icon_width, props.icon_gap)),
    }
}

fn prefix_spans(entry: &TreeEntry<'_>, props: &TreeProps, max_depth: usize) -> Vec<Span> {
    prefix(entry, props, max_depth).into_spans()
}

fn build_item(entry: &TreeEntry<'_>, props: &TreeProps, max_depth: usize) -> ListItem {
    let mut spans = prefix_spans(entry, props, max_depth);

    // The tree's own chrome leads the label; everything else about the row is the node's. Gutter,
    // status, and prefix stay columns in front of the indentation, and extra lines move right with
    // the label so they keep reading as part of it.
    let tree_width = spans_width(&spans);
    let mut item = entry.node.item.clone();
    spans.append(&mut item.spans);
    item.spans = spans;
    item.label_prefix_width = tree_width;
    item.extra_line_indent = item.extra_line_indent.saturating_add(tree_width);
    item.primary_max_label_width = item
        .primary_max_label_width
        .map(|width| width.saturating_add(tree_width));
    item
}

/// Cells of the arrow column: the widest arrow.
fn arrow_column_width(props: &TreeProps) -> u16 {
    props
        .expanded_arrow
        .width()
        .max(props.collapsed_arrow.width())
        .max(props.leaf_arrow.as_deref().unwrap_or("").width())
        .min(u16::MAX as usize) as u16
}

/// The arrow column of one row: its arrow, the leaf arrow, or unused cells the branch guide runs
/// through. Every row keeps the column while arrows show, so sibling labels line up.
fn arrow_spans(
    entry: &TreeEntry<'_>,
    props: &TreeProps,
    guides: &TreeGuides,
    max_depth: usize,
) -> Vec<Span> {
    if !props.show_arrows || !entry.node.show_arrow {
        return Vec::new();
    }
    let width = arrow_column_width(props);
    // An always-open row is a parent that shows no arrow, not a leaf: `leaf_arrow` is not its glyph.
    let arrow = match (
        entry.node.expansion_mode,
        entry.node.is_collapsible(),
        entry.expanded,
    ) {
        (TreeExpansionMode::AlwaysOpen, ..) => "",
        (_, true, true) => props.expanded_arrow.as_ref(),
        (_, true, false) => props.collapsed_arrow.as_ref(),
        (_, false, _) => props.leaf_arrow.as_deref().unwrap_or(""),
    };
    if arrow.is_empty() {
        let unused = guides.arrow_placeholder_spans(&entry.branch, max_depth, width);
        // An always-open row has no arrow to show where its children start: the branch running
        // through its column turns down to meet them instead.
        if entry.expanded && !entry.node.children.is_empty() {
            return branch_down(unused);
        }
        return unused;
    }
    vec![
        Span::new(arrow).style(entry.node.arrow_style.unwrap_or(props.arrow_style)),
        Span::new(" ".repeat((width as usize).saturating_sub(arrow.width()))),
    ]
}

/// Turn a branch that runs through an unused arrow column down into the children beneath it.
fn branch_down(mut spans: Vec<Span>) -> Vec<Span> {
    if let Some(first) = spans.first_mut()
        && let Some(rest) = first.content.strip_prefix('─')
    {
        first.content = format!("┬{rest}").into();
    }
    spans
}

/// The prefix of a row's extra lines, `width` cells wide: the guides continue, and where the
/// row's children hang, a guide carries down from the row to meet them.
fn continuation_spans(
    entry: &TreeEntry<'_>,
    props: &TreeProps,
    max_depth: usize,
    width: u16,
) -> Vec<Span> {
    let guides = tree_guides(props);
    let mut spans = guides.continuation_spans(&entry.branch, max_depth);
    let mut used = spans_width(&spans);
    let depth = entry.branch.depth() + 1;
    let hangs = entry.expanded
        && !entry.node.children.is_empty()
        && props.indent > 0
        && !matches!(props.indent_style, IndentStyle::None)
        && depth >= props.indent_guide_start_depth.max(1);
    if hangs {
        let at = guides.width(&entry.branch) + child_gap(entry.node, &entry.branch, props);
        if at >= used && at < width {
            spans.push(Span::new(" ".repeat((at - used) as usize)));
            spans.push(Span::new("│").style(guides.style_at(depth, max_depth)));
            used = at + 1;
        }
    }
    if used < width {
        spans.push(Span::new(" ".repeat((width - used) as usize)));
    }
    spans
}

fn tree_guides(props: &TreeProps) -> TreeGuides {
    let mut guides = TreeGuides::new(props.indent_style)
        .indent(props.indent)
        .guide_style(props.indent_guide_style)
        .start_depth(props.indent_guide_start_depth);
    if let Some(gradient) = props.indent_gradient {
        guides = guides.gradient(gradient);
    }
    guides
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Style;
    use crate::widgets::{ListItemGutter, ListItemLine};

    /// A tree row is its node's `ListItem` with the tree's indentation in front of the label.
    /// Nothing else about the item may be lost on the way: gutter, status, and prefix stay columns
    /// in front of the indentation, and extra lines and label width limits move right with the
    /// label.
    #[test]
    fn build_item_preserves_the_node_item_around_the_indentation() {
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
        let leaf = TreeNode::new(source.clone());
        let props = crate::widgets::Tree::new(leaf.clone())
            .show_arrows(false)
            .content_gap(0)
            .indent_style(IndentStyle::Branch)
            .props;
        let entry = TreeEntry {
            path: TreePath::from(vec![0]),
            node: &leaf,
            expanded: false,
            branch: TreeBranch::root().child(true),
            icon_column: false,
        };

        let out = build_item(&entry, &props, 1);

        let content: Vec<&str> = out.spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(content, vec!["└", "─", "Node"]);
        assert_eq!(out.status, source.status);
        assert_eq!(out.gutter, source.gutter);
        assert_eq!(out.gutter_line, 1);
        assert_eq!(out.prefix, source.prefix);
        assert_eq!(out.prefix_style, source.prefix_style);
        assert_eq!(out.extra_line_indent, 4 + 2);
        assert!(out.primary_wrap_label);
        assert!(out.primary_wrap_description);
        assert_eq!(out.primary_max_label_width, Some(3 + 2));
        assert_eq!(out.primary_max_description_width, Some(4));
        assert_eq!(out.symbol_line, 1);
        assert_eq!(out.extra_lines, source.extra_lines);
    }

    #[test]
    fn build_item_keeps_primary_description_fitting() {
        let root = TreeNode::new(
            ListItem::new("branch")
                .description("feat/some-long-name")
                .primary_description_truncation(crate::widgets::ListTruncation::Start)
                .primary_description_gap(3),
        );
        let props = crate::widgets::Tree::new(root.clone())
            .show_arrows(false)
            .props;
        let entry = TreeEntry {
            path: TreePath::from(Vec::new()),
            node: &root,
            expanded: false,
            branch: TreeBranch {
                depth: 0,
                last_child: true,
                continued: Vec::new(),
                level_gaps: Vec::new(),
            },
            icon_column: false,
        };

        let out = build_item(&entry, &props, 0);

        assert_eq!(
            out.primary_description_truncation,
            crate::widgets::ListTruncation::Start
        );
        assert_eq!(out.primary_description_gap, 3);
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

        assert_eq!(state.selected, Some(2));
    }

    #[test]
    fn activation_collapses_an_expandable_node_without_children() {
        let root = TreeNode::new("empty")
            .expanded(true)
            .expansion_mode(TreeExpansionMode::Collapsible);
        let tree = crate::widgets::Tree::new(root);
        let mut backend = crate::TestBackend::new_with_props(TreeComponent::new(), tree.props);

        assert!(
            backend
                .state()
                .expanded
                .contains(&TreePath::from(Vec::new()))
        );
        backend
            .dispatch(TreeMsg::Activate(0))
            .expect("activate empty expandable node");

        assert!(
            !backend
                .state()
                .expanded
                .contains(&TreePath::from(Vec::new()))
        );
    }

    /// Render a `Tree` through the real component and report the selection the
    /// inner `List` ended up with.
    fn inner_list_selection(tree: crate::widgets::Tree) -> Option<usize> {
        let mut backend =
            crate::TestBackend::new_with_props(TreeComponent::new(), tree.props.clone());
        backend.set_viewport(crate::style::Rect {
            x: 0,
            y: 0,
            w: 30,
            h: 10,
        });
        backend.render();

        let snapshot = backend.capture_ui_snapshot();
        snapshot
            .widgets
            .iter()
            .find(|w| w.kind == crate::ui_snapshot::UiWidgetKind::List)
            .expect("tree renders an inner list")
            .selected_index
    }

    fn sample_tree() -> TreeNode {
        TreeNode::new("root")
            .expanded(true)
            .child(TreeNode::new("a"))
            .child(TreeNode::new("b"))
    }

    #[test]
    fn clear_selection_suppresses_inner_list_highlight() {
        // Baseline: the controlled prop reaches the inner list.
        assert_eq!(
            inner_list_selection(crate::widgets::Tree::new(sample_tree()).selected(1)),
            Some(1)
        );

        // clear_selection wins over the controlled prop.
        assert_eq!(
            inner_list_selection(
                crate::widgets::Tree::new(sample_tree())
                    .selected(1)
                    .clear_selection(true)
            ),
            None
        );
    }

    #[test]
    fn leaving_clear_selection_restores_a_cursor_even_when_expansion_also_changed() {
        // Both branches of on_props_changed must run the same sync; the
        // expansion branch used to skip the restore and leave the tree blank.
        let mut state = TreeState {
            hovered: None,
            expanded: HashSet::new(),
            selected: None,
        };

        let cleared = crate::widgets::Tree::new(sample_tree())
            .clear_selection(true)
            .props;
        sync_selection_from_props(&cleared, &mut state);
        assert_eq!(state.selected, None);

        let restored = crate::widgets::Tree::new(sample_tree()).props;
        sync_selection_from_props(&restored, &mut state);
        assert_eq!(state.selected, Some(0));
    }

    #[test]
    fn clear_selection_survives_a_select_message() {
        let tree = crate::widgets::Tree::new(sample_tree()).clear_selection(true);
        let mut backend =
            crate::TestBackend::new_with_props(TreeComponent::new(), tree.props.clone());
        backend.set_viewport(crate::style::Rect {
            x: 0,
            y: 0,
            w: 30,
            h: 10,
        });
        backend.render();

        // The inner list may still adopt and emit a selection; clear_selection
        // must keep the highlight suppressed regardless of recorded state.
        backend
            .dispatch(TreeMsg::Select(2))
            .expect("dispatch tree select");
        backend.render();

        assert_eq!(backend.state().selected, Some(2));

        let snapshot = backend.capture_ui_snapshot();
        let list = snapshot
            .widgets
            .iter()
            .find(|w| w.kind == crate::ui_snapshot::UiWidgetKind::List)
            .expect("tree renders an inner list");
        assert_eq!(list.selected_index, None);
    }

    #[test]
    fn reserved_columns_do_not_change_the_guide_geometry() {
        for style in [
            IndentStyle::None,
            IndentStyle::Line,
            IndentStyle::LineCapped,
            IndentStyle::LineCappedRounded,
            IndentStyle::Branch,
            IndentStyle::BranchRounded,
        ] {
            for indent in [0, 1, 3] {
                let child = TreeNode::new("leaf");
                let props = crate::widgets::Tree::new(child.clone())
                    .indent(indent)
                    .indent_style(style)
                    .props;
                let branch = TreeBranch::root().child(true);
                let entry = TreeEntry {
                    path: TreePath::from(vec![0]),
                    node: &child,
                    expanded: false,
                    branch: branch.clone(),
                    icon_column: false,
                };
                let item = build_item(&entry, &props, 1);
                let actual: String = item
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect();
                let guides = TreeGuides::new(style).indent(indent);
                let prefix: String = guides
                    .spans(&branch, 1)
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect();
                let fill = if indent > 0
                    && matches!(style, IndentStyle::Branch | IndentStyle::BranchRounded)
                {
                    "─ "
                } else {
                    "  "
                };
                assert_eq!(actual, format!("{prefix}{fill}leaf"));
                assert_eq!(item.label_prefix_width, indent + 2);
            }
        }
    }

    #[test]
    fn nested_headings_hang_their_children_beneath_their_label() {
        let heading = |label| {
            TreeNode::new(label)
                .expansion_mode(TreeExpansionMode::AlwaysOpen)
                .show_arrow(false)
        };
        let props = crate::widgets::Tree::new(
            heading("root")
                .child(heading("group").child(heading("nested").child(TreeNode::new("leaf")))),
        )
        .indent_style(IndentStyle::Branch)
        .content_gap(3)
        .props;
        let entries = visible_entries(&props, &HashSet::new());
        let prefixes: Vec<String> = entries
            .iter()
            .map(|entry| {
                prefix_spans(entry, &props, 3)
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();
        // Without an arrow column, each heading's children hang beneath its label, one content
        // gap after its branch, so every level steps by the indent and the gap.
        assert_eq!(prefixes, ["", "└─   ", "     └─   ", "          └──   "]);
        let props = crate::widgets::Tree::new(
            TreeNode::new("root")
                .expansion_mode(TreeExpansionMode::AlwaysOpen)
                .child(
                    TreeNode::new("group")
                        .expansion_mode(TreeExpansionMode::AlwaysOpen)
                        .child(TreeNode::new("leaf")),
                ),
        )
        .indent_style(IndentStyle::Branch)
        .content_gap(3)
        .props;
        let prefixes: Vec<String> = visible_entries(&props, &HashSet::new())
            .iter()
            .map(|entry| {
                prefix_spans(entry, &props, 2)
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();
        assert_eq!(
            prefixes,
            ["    ", "└─┬   ", "  └──   "],
            "every row keeps its arrow column, a root's blank, and an always-open row turns its branch down into its children"
        );
        for entry in &entries {
            let guides = tree_guides(&props);
            let first: String = guides
                .spans(&entry.branch, 3)
                .iter()
                .map(|span| span.content.as_ref())
                .collect();
            let continuation: String = guides
                .continuation_spans(&entry.branch, 3)
                .iter()
                .map(|span| span.content.as_ref())
                .collect();
            assert_eq!(first.width(), guides.width(&entry.branch) as usize);
            assert_eq!(continuation.width(), first.width());
        }
    }
}
