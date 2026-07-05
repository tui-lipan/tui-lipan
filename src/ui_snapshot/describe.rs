use crate::core::element::Key;
use crate::core::node::{Node, NodeId, NodeKind, NodeTree};
use crate::style::Span;
use crate::style::text::RichText;
use crate::widgets::CheckboxState;
use crate::widgets::list::ListItem;

use super::kind::UiWidgetKind;
use super::options::UiSnapshotOptions;

/// Semantic description of one realized widget for agent/design review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiWidgetDesc {
    /// Widget kind tag.
    pub kind: UiWidgetKind,
    /// Stable reconciliation key, when set on the element.
    pub key: Option<Key>,
    /// Layout rectangle in viewport coordinates.
    pub rect: crate::style::Rect,
    /// Whether this node currently has keyboard focus.
    pub focused: bool,
    /// Whether this node is the current mouse hover target.
    pub hovered: bool,
    /// Frame/panel title or equivalent primary heading.
    pub title: Option<String>,
    /// Primary label (button, checkbox, etc.).
    pub label: Option<String>,
    /// Placeholder text for inputs when set.
    pub placeholder: Option<String>,
    /// Current value for inputs/text areas when not masked.
    pub value: Option<String>,
    /// When true, `value` is intentionally omitted (e.g. masked input).
    pub value_masked: bool,
    /// Checkbox tri-state when applicable.
    pub checkbox_state: Option<CheckboxState>,
    /// Selected row/tab index when applicable.
    pub selected_index: Option<usize>,
    /// Scroll offset when applicable.
    pub scroll_offset: Option<usize>,
    /// Preview of list/table item labels (may be truncated).
    pub item_labels: Option<Vec<String>>,
    /// Full item count when `item_labels` is truncated.
    pub total_items: Option<usize>,
    /// Number of direct child nodes (structural widgets).
    pub child_count: Option<usize>,
}

pub(crate) fn describe_widgets(
    tree: &NodeTree,
    focused: Option<NodeId>,
    hovered: Option<NodeId>,
    options: &UiSnapshotOptions,
) -> Vec<UiWidgetDesc> {
    tree.iter_with_overlays()
        .filter_map(|node| describe_node(node, focused, hovered, options))
        .collect()
}

fn describe_node(
    node: &Node,
    focused: Option<NodeId>,
    hovered: Option<NodeId>,
    options: &UiSnapshotOptions,
) -> Option<UiWidgetDesc> {
    if !options.include_zero_area && (node.rect.w == 0 || node.rect.h == 0) {
        return None;
    }

    let kind = UiWidgetKind::from_node_kind(&node.kind);
    if !options.include_chrome {
        match &node.kind {
            NodeKind::Spacer(_) => return None,
            NodeKind::Divider(_) => return None,
            _ => {}
        }
    }

    let mut desc = UiWidgetDesc {
        kind,
        key: node.key.clone(),
        rect: node.rect,
        focused: focused == Some(node.id),
        hovered: hovered == Some(node.id),
        title: None,
        label: None,
        placeholder: None,
        value: None,
        value_masked: false,
        checkbox_state: None,
        selected_index: None,
        scroll_offset: None,
        item_labels: None,
        total_items: None,
        child_count: None,
    };

    match &node.kind {
        NodeKind::Text(text) => {
            desc.label = Some(spans_plain(&text.spans));
        }
        NodeKind::Button(button) => {
            desc.label = Some(button.label.to_string());
            if let Some(shortcut) = &button.shortcut {
                desc.title = Some(shortcut.to_string());
            }
        }
        NodeKind::Input(input) => {
            if input.mask.is_some() {
                desc.value_masked = true;
            } else {
                desc.value = Some(input.value.to_string());
            }
            if let Some(placeholder) = &input.placeholder {
                desc.placeholder = Some(placeholder.to_string());
            }
        }
        NodeKind::TextArea(text_area) => {
            desc.value = Some(text_area.value.to_string());
            desc.scroll_offset = Some(text_area.scroll_offset);
        }
        NodeKind::List(list) => {
            if let Some(title) = &list.title {
                desc.title = Some(title.to_string());
            }
            desc.selected_index = Some(list.selected);
            desc.scroll_offset = Some(list.offset);
            let total = list.items.len();
            if total > 0 {
                let labels = collect_list_labels(&list.items, options.max_list_items);
                desc.item_labels = Some(labels);
                desc.total_items = Some(total);
            }
        }
        NodeKind::Table(table) => {
            desc.selected_index = Some(table.selected);
            desc.scroll_offset = Some(table.offset);
            let total = table.rows.len();
            if total > 0 {
                let labels = table
                    .rows
                    .iter()
                    .take(options.max_list_items)
                    .map(table_row_label)
                    .collect();
                desc.item_labels = Some(labels);
                desc.total_items = Some(total);
            }
        }
        NodeKind::Tabs(tabs) => {
            desc.selected_index = Some(tabs.active);
            let labels: Vec<String> = tabs.tabs.iter().map(|t| t.label.to_string()).collect();
            if !labels.is_empty() {
                let total = labels.len();
                desc.item_labels = Some(truncate_labels(labels, options.max_list_items));
                desc.total_items = Some(total);
            }
        }
        NodeKind::DraggableTabBar(bar) => {
            desc.selected_index = Some(bar.active);
            let labels: Vec<String> = bar.tabs.iter().map(|t| t.label.to_string()).collect();
            if !labels.is_empty() {
                let total = labels.len();
                desc.item_labels = Some(truncate_labels(labels, options.max_list_items));
                desc.total_items = Some(total);
            }
        }
        NodeKind::Checkbox(checkbox) => {
            if let Some(label) = &checkbox.label {
                desc.label = Some(label.to_string());
            }
            desc.checkbox_state = Some(checkbox.state);
        }
        NodeKind::Frame(frame) => {
            desc.title = rich_text_plain(frame.title.as_ref());
            desc.selected_index = Some(frame.active_tab);
            if !frame.tab_titles.is_empty() {
                let labels: Vec<String> = frame
                    .tab_titles
                    .iter()
                    .filter_map(|t| rich_text_plain(Some(t)))
                    .collect();
                if !labels.is_empty() {
                    let total = labels.len();
                    desc.item_labels = Some(truncate_labels(labels, options.max_list_items));
                    desc.total_items = Some(total);
                }
            }
            if desc.title.is_none()
                && let Some(status) = rich_text_plain(frame.status.as_ref())
            {
                desc.label = Some(status);
            }
        }
        NodeKind::ScrollView(scroll) => {
            desc.scroll_offset = Some(scroll.offset);
            desc.child_count = Some(node.children.len());
        }
        NodeKind::VStack(stack) | NodeKind::HStack(stack) => {
            desc.selected_index = Some(stack.active_tab);
            desc.child_count = Some(node.children.len());
            if !stack.tab_titles.is_empty() {
                let labels: Vec<String> = stack
                    .tab_titles
                    .iter()
                    .filter_map(|t| rich_text_plain(Some(t)))
                    .collect();
                if !labels.is_empty() {
                    let total = labels.len();
                    desc.item_labels = Some(truncate_labels(labels, options.max_list_items));
                    desc.total_items = Some(total);
                }
            }
        }
        NodeKind::Group(_)
        | NodeKind::Portal(_)
        | NodeKind::Grid(_)
        | NodeKind::Flow(_)
        | NodeKind::Canvas(_)
        | NodeKind::ZStack(_)
        | NodeKind::Splitter(_)
        | NodeKind::Popover(_) => {
            desc.child_count = Some(node.children.len());
        }
        _ => {}
    }

    Some(desc)
}

pub(crate) fn key_for_node(tree: &NodeTree, id: Option<NodeId>) -> Option<Key> {
    id.filter(|id| tree.is_valid(*id))
        .and_then(|id| tree.node(id).key.clone())
}

fn spans_plain(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn rich_text_plain(text: Option<&RichText>) -> Option<String> {
    text.filter(|t| !t.is_empty())
        .map(|t| t.plain_content().into_owned())
}

fn collect_list_labels(items: &[ListItem], max: usize) -> Vec<String> {
    items
        .iter()
        .take(max)
        .map(|item| item.plain_content())
        .collect()
}

fn truncate_labels(mut labels: Vec<String>, max: usize) -> Vec<String> {
    labels.truncate(max);
    labels
}

fn table_row_label(row: &crate::widgets::TableRow) -> String {
    row.cells
        .first()
        .map(|cell| cell.content.to_string())
        .unwrap_or_default()
}
