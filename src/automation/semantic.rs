use std::collections::HashSet;

use crate::core::node::{Node, NodeId, NodeKind, NodeTree, WidgetNode};
use crate::style::Rect;

use super::AutomationId;

/// Stable semantic role exposed to automation and accessibility-oriented tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SemanticRole {
    /// Synthetic root of a semantic tree.
    Application,
    /// Structural node without a more specific role.
    Generic,
    /// Static text.
    Text,
    /// Heading or panel title.
    Heading,
    /// Push button.
    Button,
    /// Single-line text input.
    TextBox,
    /// Multi-line text editor.
    TextArea,
    /// Checkbox.
    Checkbox,
    /// Group of radio options.
    RadioGroup,
    /// One radio option.
    Radio,
    /// Select or combo box.
    ComboBox,
    /// List.
    List,
    /// List row.
    ListItem,
    /// Table.
    Table,
    /// Table row.
    Row,
    /// Table cell.
    Cell,
    /// Tab collection.
    TabList,
    /// Tab.
    Tab,
    /// Tree.
    Tree,
    /// Tree item.
    TreeItem,
    /// Modal dialog.
    Dialog,
    /// Alert or toast.
    Alert,
    /// Progress indicator.
    ProgressBar,
    /// Adjustable numeric value.
    Slider,
    /// Scrollable region.
    ScrollView,
    /// Separator.
    Separator,
    /// Status information.
    Status,
    /// Hyperlink.
    Link,
    /// Embedded terminal.
    Terminal,
    /// Image or image-like canvas.
    Image,
    /// Menu.
    Menu,
    /// Menu item.
    MenuItem,
    /// Toolbar.
    Toolbar,
    /// Named or unnamed group.
    Group,
}

/// Why a semantic value is, or is not, shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueSensitivity {
    /// The value may be emitted.
    Public,
    /// The app explicitly marked the value sensitive.
    Sensitive,
    /// The widget masks the value.
    Masked,
}

/// Redaction-safe representation of a widget value.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SemanticValue {
    /// Public value text. This is `None` for sensitive and masked values.
    pub text: Option<String>,
    /// Sensitivity policy applied before this value entered the semantic tree.
    pub sensitivity: ValueSensitivity,
}

impl SemanticValue {
    pub(crate) fn public(value: impl Into<String>) -> Self {
        Self {
            text: Some(value.into()),
            sensitivity: ValueSensitivity::Public,
        }
    }

    pub(crate) fn redacted(sensitivity: ValueSensitivity) -> Self {
        Self {
            text: None,
            sensitivity,
        }
    }
}

/// Tri-state checked value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SemanticChecked {
    /// Not checked.
    False,
    /// Checked.
    True,
    /// Mixed or indeterminate.
    Mixed,
}

/// Operation a semantic node accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SemanticAction {
    /// Activate or click.
    Click,
    /// Move keyboard focus here.
    Focus,
    /// Replace or edit its value.
    SetValue,
    /// Toggle a binary or tri-state control.
    Toggle,
    /// Scroll the control.
    Scroll,
    /// Begin or receive a drag-and-drop operation.
    Drag,
    /// Expand hidden content.
    Expand,
    /// Collapse shown content.
    Collapse,
}

/// One node in a committed semantic tree.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SemanticNode {
    /// App-authored automation identity.
    pub automation_id: Option<AutomationId>,
    /// Stable semantic role.
    pub role: SemanticRole,
    /// Accessible name.
    pub name: Option<String>,
    /// Redaction-safe value.
    pub value: Option<SemanticValue>,
    /// Whether this node has keyboard focus.
    pub focused: bool,
    /// Whether this node accepts interaction.
    pub enabled: bool,
    /// Selection state where applicable.
    pub selected: Option<bool>,
    /// Expansion state where applicable.
    pub expanded: Option<bool>,
    /// Checked state where applicable.
    pub checked: Option<SemanticChecked>,
    /// Layout bounds before clipping.
    pub bounds: Rect,
    /// Bounds after ancestor and viewport clipping.
    pub clipped_bounds: Rect,
    /// Whether `clipped_bounds` has positive area.
    pub in_view: bool,
    /// Whether the node is enabled and supports an action target.
    pub actionable: bool,
    /// Supported operations.
    pub actions: Vec<SemanticAction>,
    /// Child semantic nodes.
    pub children: Vec<SemanticNode>,
    /// Overlay stacking index. Set only on semantic overlay roots.
    pub overlay_order: Option<u64>,
    pub(crate) runtime_node: Option<NodeId>,
}

impl SemanticNode {
    /// Return all nodes in depth-first order, including this node.
    pub fn descendants(&self) -> SemanticIter<'_> {
        SemanticIter { stack: vec![self] }
    }

    /// Concatenate this node's accessible text for selector matching.
    pub fn searchable_text(&self) -> String {
        let mut parts = Vec::new();
        if let Some(name) = &self.name {
            parts.push(name.as_str());
        }
        if let Some(value) = &self.value
            && let Some(text) = &value.text
        {
            parts.push(text.as_str());
        }
        parts.join(" ")
    }
}

/// Depth-first semantic-node iterator.
pub struct SemanticIter<'a> {
    stack: Vec<&'a SemanticNode>,
}

impl<'a> Iterator for SemanticIter<'a> {
    type Item = &'a SemanticNode;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.stack.pop()?;
        self.stack.extend(node.children.iter().rev());
        Some(node)
    }
}

/// Coherent semantic projection committed by one session generation.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SemanticTree {
    /// Session generation represented by this tree.
    pub generation: u64,
    /// Viewport used for layout and clipping.
    pub viewport: Rect,
    /// Synthetic application root.
    pub root: SemanticNode,
}

impl SemanticTree {
    /// Iterate over every realized semantic node. The synthetic root is omitted.
    pub fn nodes(&self) -> impl Iterator<Item = &SemanticNode> {
        self.root
            .children
            .iter()
            .flat_map(SemanticNode::descendants)
    }

    /// Find all nodes carrying `id`.
    pub fn by_id(&self, id: &AutomationId) -> Vec<&SemanticNode> {
        self.nodes()
            .filter(|node| node.automation_id.as_ref() == Some(id))
            .collect()
    }
}

pub(crate) fn project_semantic_tree(
    tree: &NodeTree,
    viewport: Rect,
    focused: Option<NodeId>,
    generation: u64,
) -> SemanticTree {
    let mut seen = HashSet::new();
    let overlay_ids: HashSet<NodeId> = tree.overlay_roots().iter().map(|root| root.id).collect();
    let mut children = Vec::new();
    if tree.is_valid(tree.root)
        && let Some(root) = project_node(
            tree,
            tree.root,
            viewport,
            viewport,
            focused,
            &overlay_ids,
            &mut seen,
            None,
        )
    {
        children.push(root);
    }
    for overlay in tree.overlay_roots() {
        if let Some(node) = project_node(
            tree,
            overlay.id,
            viewport,
            viewport,
            focused,
            &HashSet::new(),
            &mut seen,
            Some(overlay.order),
        ) {
            children.push(node);
        }
    }

    SemanticTree {
        generation,
        viewport,
        root: SemanticNode {
            automation_id: None,
            role: SemanticRole::Application,
            name: None,
            value: None,
            focused: false,
            enabled: true,
            selected: None,
            expanded: None,
            checked: None,
            bounds: viewport,
            clipped_bounds: viewport,
            in_view: viewport.w > 0 && viewport.h > 0,
            actionable: false,
            actions: Vec::new(),
            children,
            overlay_order: None,
            runtime_node: None,
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn project_node(
    tree: &NodeTree,
    id: NodeId,
    viewport: Rect,
    ancestor_clip: Rect,
    focused: Option<NodeId>,
    skipped_overlay_roots: &HashSet<NodeId>,
    seen: &mut HashSet<NodeId>,
    overlay_order: Option<u64>,
) -> Option<SemanticNode> {
    if !tree.is_valid(id) || !seen.insert(id) {
        return None;
    }
    let node = tree.node(id);
    let clipped_bounds = node
        .rect
        .intersection(&ancestor_clip)
        .intersection(&viewport);
    let mut actions = semantic_actions(node);
    let enabled = !node.kind.is_disabled();
    let mut children = project_children(
        tree,
        node,
        viewport,
        clipped_bounds,
        focused,
        skipped_overlay_roots,
        seen,
        overlay_order,
    );
    children.extend(synthetic_semantic_children(
        node,
        clipped_bounds,
        viewport,
        enabled,
        overlay_order,
    ));
    let role = node.semantic_role.unwrap_or_else(|| role_for_node(node));
    let name = node
        .semantic_name
        .as_ref()
        .map(ToString::to_string)
        .or_else(|| name_for_node(node));
    let value = value_for_node(node);
    let (derived_selected, derived_expanded, checked) = states_for_node(node);
    let selected = node.semantic_selected.or(derived_selected);
    let expanded = node.semantic_expanded.or(derived_expanded);
    let runtime_node = resolve_semantic_actions(node, id, expanded, &children, &mut actions);
    actions.sort_by_key(|action| *action as u8);
    actions.dedup();
    let in_view = clipped_bounds.w > 0 && clipped_bounds.h > 0;
    let actionable = enabled && in_view && !actions.is_empty();

    Some(SemanticNode {
        automation_id: node.automation_id.clone(),
        role,
        name,
        value,
        focused: focused == Some(id),
        enabled,
        selected,
        expanded,
        checked,
        bounds: node.rect,
        clipped_bounds,
        in_view,
        actionable,
        actions,
        children,
        overlay_order,
        runtime_node,
    })
}

#[allow(clippy::too_many_arguments)]
fn project_children(
    tree: &NodeTree,
    node: &Node,
    viewport: Rect,
    clipped_bounds: Rect,
    focused: Option<NodeId>,
    skipped_overlay_roots: &HashSet<NodeId>,
    seen: &mut HashSet<NodeId>,
    overlay_order: Option<u64>,
) -> Vec<SemanticNode> {
    node.children
        .iter()
        .filter(|child| !skipped_overlay_roots.contains(child))
        .filter_map(|child| {
            project_node(
                tree,
                *child,
                viewport,
                clipped_bounds,
                focused,
                skipped_overlay_roots,
                seen,
                overlay_order,
            )
        })
        .collect()
}

fn resolve_semantic_actions(
    node: &Node,
    id: NodeId,
    expanded: Option<bool>,
    children: &[SemanticNode],
    actions: &mut Vec<SemanticAction>,
) -> Option<NodeId> {
    if let Some(expanded) = expanded {
        actions.push(if expanded {
            SemanticAction::Collapse
        } else {
            SemanticAction::Expand
        });
    }
    if node.semantic_role.is_some()
        && actions.is_empty()
        && let Some(target) = children.iter().find_map(first_action_target)
    {
        *actions = target.actions.clone();
        return target.runtime_node;
    }
    Some(id)
}

fn synthetic_semantic_children(
    node: &Node,
    clipped_bounds: Rect,
    viewport: Rect,
    enabled: bool,
    overlay_order: Option<u64>,
) -> Vec<SemanticNode> {
    match &node.kind {
        NodeKind::List(list) => {
            let content = node.rect.inner(list.border, list.padding);
            let top_indicator = u16::from(list.show_scroll_indicators && list.top_indicator);
            list.items
                .iter()
                .enumerate()
                .skip(list.offset)
                .scan(top_indicator, |line, (index, item)| {
                    let height = u16::try_from(crate::widgets::list::utils::list_item_height(item))
                        .unwrap_or(u16::MAX)
                        .max(1);
                    let bounds = Rect {
                        x: content.x,
                        y: content
                            .y
                            .saturating_add(i16::try_from(*line).unwrap_or(i16::MAX)),
                        w: content.w,
                        h: height,
                    };
                    *line = line.saturating_add(height);
                    let name = item
                        .spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>();
                    Some(synthetic_node(
                        SemanticRole::ListItem,
                        Some(name),
                        list.selected.map(|selected| selected == index),
                        bounds,
                        clipped_bounds,
                        viewport,
                        enabled,
                        semantic_actions(node),
                        node.id,
                        overlay_order,
                        Vec::new(),
                    ))
                })
                .collect()
        }
        NodeKind::Table(table) => {
            let content = node.rect.inner(table.border, table.padding);
            let header_height = table.header.as_ref().map_or(0, |header| {
                header
                    .height
                    .saturating_add(header.bottom_margin)
                    .saturating_add(table.row_gap)
            });
            table
                .rows
                .iter()
                .enumerate()
                .skip(table.offset)
                .scan(header_height, |line, (index, row)| {
                    let row_height = row.height.max(1);
                    let bounds = Rect {
                        x: content.x,
                        y: content
                            .y
                            .saturating_add(i16::try_from(*line).unwrap_or(i16::MAX)),
                        w: content.w,
                        h: row_height,
                    };
                    *line = line
                        .saturating_add(row_height)
                        .saturating_add(row.bottom_margin)
                        .saturating_add(table.row_gap);
                    let cell_width = if row.cells.is_empty() {
                        bounds.w
                    } else {
                        bounds.w / u16::try_from(row.cells.len()).unwrap_or(u16::MAX).max(1)
                    };
                    let cells = row
                        .cells
                        .iter()
                        .enumerate()
                        .map(|(cell_index, cell)| {
                            let x_offset = u16::try_from(cell_index)
                                .unwrap_or(u16::MAX)
                                .saturating_mul(cell_width);
                            synthetic_node(
                                SemanticRole::Cell,
                                Some(cell.content.to_string()),
                                None,
                                Rect {
                                    x: bounds.x.saturating_add(
                                        i16::try_from(x_offset).unwrap_or(i16::MAX),
                                    ),
                                    y: bounds.y,
                                    w: cell_width,
                                    h: bounds.h,
                                },
                                clipped_bounds,
                                viewport,
                                enabled,
                                semantic_actions(node),
                                node.id,
                                overlay_order,
                                Vec::new(),
                            )
                        })
                        .collect();
                    Some(synthetic_node(
                        SemanticRole::Row,
                        None,
                        table.selected.map(|selected| selected == index),
                        bounds,
                        clipped_bounds,
                        viewport,
                        enabled,
                        semantic_actions(node),
                        node.id,
                        overlay_order,
                        cells,
                    ))
                })
                .collect()
        }
        NodeKind::Tabs(tabs) => {
            let content = node.rect.inner(tabs.border, tabs.padding);
            let width = content.w / u16::try_from(tabs.tabs.len()).unwrap_or(u16::MAX).max(1);
            tabs.tabs
                .iter()
                .enumerate()
                .map(|(index, tab)| {
                    let offset = u16::try_from(index)
                        .unwrap_or(u16::MAX)
                        .saturating_mul(width);
                    synthetic_node(
                        SemanticRole::Tab,
                        Some(tab.label.to_string()),
                        Some(tabs.active == index),
                        Rect {
                            x: content
                                .x
                                .saturating_add(i16::try_from(offset).unwrap_or(i16::MAX)),
                            y: content.y,
                            w: width,
                            h: 1,
                        },
                        clipped_bounds,
                        viewport,
                        enabled,
                        semantic_actions(node),
                        node.id,
                        overlay_order,
                        Vec::new(),
                    )
                })
                .collect()
        }
        NodeKind::DraggableTabBar(tabs) => {
            let content = node.rect.inner(tabs.border, tabs.padding);
            let width = content.w / u16::try_from(tabs.tabs.len()).unwrap_or(u16::MAX).max(1);
            tabs.tabs
                .iter()
                .enumerate()
                .map(|(index, tab)| {
                    let offset = u16::try_from(index)
                        .unwrap_or(u16::MAX)
                        .saturating_mul(width);
                    synthetic_node(
                        SemanticRole::Tab,
                        Some(tab.label.to_string()),
                        Some(tabs.active == index),
                        Rect {
                            x: content
                                .x
                                .saturating_add(i16::try_from(offset).unwrap_or(i16::MAX)),
                            y: content.y,
                            w: width,
                            h: 1,
                        },
                        clipped_bounds,
                        viewport,
                        enabled,
                        semantic_actions(node),
                        node.id,
                        overlay_order,
                        Vec::new(),
                    )
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn synthetic_node(
    role: SemanticRole,
    name: Option<String>,
    selected: Option<bool>,
    bounds: Rect,
    ancestor_clip: Rect,
    viewport: Rect,
    enabled: bool,
    actions: Vec<SemanticAction>,
    runtime_node: NodeId,
    overlay_order: Option<u64>,
    children: Vec<SemanticNode>,
) -> SemanticNode {
    let clipped_bounds = bounds.intersection(&ancestor_clip).intersection(&viewport);
    let in_view = clipped_bounds.w > 0 && clipped_bounds.h > 0;
    SemanticNode {
        automation_id: None,
        role,
        name,
        value: None,
        focused: false,
        enabled,
        selected,
        expanded: None,
        checked: None,
        bounds,
        clipped_bounds,
        in_view,
        actionable: enabled && in_view && !actions.is_empty(),
        actions,
        children,
        overlay_order,
        runtime_node: Some(runtime_node),
    }
}

fn first_action_target(node: &SemanticNode) -> Option<&SemanticNode> {
    if !node.actions.is_empty() {
        return Some(node);
    }
    node.children.iter().find_map(first_action_target)
}

fn role_for_node(node: &Node) -> SemanticRole {
    match &node.kind {
        NodeKind::Text(_) => SemanticRole::Text,
        NodeKind::Button(_) => SemanticRole::Button,
        NodeKind::Input(_) => SemanticRole::TextBox,
        NodeKind::TextArea(_) => SemanticRole::TextArea,
        NodeKind::Checkbox(_) => SemanticRole::Checkbox,
        NodeKind::List(_) => SemanticRole::List,
        NodeKind::Table(_) => SemanticRole::Table,
        NodeKind::Tabs(_) | NodeKind::DraggableTabBar(_) => SemanticRole::TabList,
        NodeKind::Frame(_) => SemanticRole::Group,
        NodeKind::ScrollView(_) | NodeKind::DocumentView(_) => SemanticRole::ScrollView,
        NodeKind::ProgressBar(_) => SemanticRole::ProgressBar,
        NodeKind::Slider(_) => SemanticRole::Slider,
        NodeKind::Divider(_) => SemanticRole::Separator,
        NodeKind::StatusBarLayout(_) => SemanticRole::Status,
        NodeKind::Canvas(_) | NodeKind::AsciiCanvas(_) => SemanticRole::Image,
        #[cfg(feature = "image")]
        NodeKind::Image(_) => SemanticRole::Image,
        #[cfg(feature = "terminal")]
        NodeKind::Terminal(_) => SemanticRole::Terminal,
        NodeKind::Group(_)
        | NodeKind::Portal(_)
        | NodeKind::VStack(_)
        | NodeKind::HStack(_)
        | NodeKind::Grid(_)
        | NodeKind::Flow(_)
        | NodeKind::ZStack(_)
        | NodeKind::Center(_)
        | NodeKind::CenterPin(_)
        | NodeKind::Popover(_)
        | NodeKind::Splitter(_)
        | NodeKind::MouseRegion(_)
        | NodeKind::DragSource(_)
        | NodeKind::DropTarget(_)
        | NodeKind::EffectScope(_)
        | NodeKind::Animated(_) => SemanticRole::Group,
        _ => SemanticRole::Generic,
    }
}

fn name_for_node(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::Text(text) => Some(
            text.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect(),
        ),
        NodeKind::Button(button) => Some(button.label.to_string()),
        NodeKind::Input(input) => input.placeholder.as_ref().map(ToString::to_string),
        NodeKind::Checkbox(checkbox) => checkbox.label.as_ref().map(ToString::to_string),
        NodeKind::Slider(slider) => slider.label.clone(),
        NodeKind::List(list) => list.title.as_ref().map(ToString::to_string),
        NodeKind::Frame(frame) => frame
            .header
            .left
            .as_ref()
            .map(|label| label.content.plain_content().into_owned()),
        _ => None,
    }
}

fn value_for_node(node: &Node) -> Option<SemanticValue> {
    if node.semantic_value_sensitive {
        return Some(SemanticValue::redacted(ValueSensitivity::Sensitive));
    }
    match &node.kind {
        NodeKind::Input(input) if input.mask.is_some() => {
            Some(SemanticValue::redacted(ValueSensitivity::Masked))
        }
        NodeKind::Input(input) => Some(SemanticValue::public(input.value.to_string())),
        NodeKind::TextArea(text_area) => Some(SemanticValue::public(text_area.value.to_string())),
        NodeKind::Slider(slider) => Some(SemanticValue::public(slider.value.to_string())),
        NodeKind::ProgressBar(progress) => {
            Some(SemanticValue::public(progress.progress.to_string()))
        }
        _ => None,
    }
}

fn states_for_node(node: &Node) -> (Option<bool>, Option<bool>, Option<SemanticChecked>) {
    match &node.kind {
        NodeKind::Checkbox(checkbox) => {
            let checked = match checkbox.state {
                crate::widgets::CheckboxState::Unchecked => SemanticChecked::False,
                crate::widgets::CheckboxState::Checked => SemanticChecked::True,
                crate::widgets::CheckboxState::Indeterminate => SemanticChecked::Mixed,
            };
            (
                Some(matches!(checked, SemanticChecked::True)),
                None,
                Some(checked),
            )
        }
        NodeKind::List(list) => (list.selected.map(|_| true), None, None),
        NodeKind::Table(table) => (table.selected.map(|_| true), None, None),
        _ => (None, None, None),
    }
}

fn semantic_actions(node: &Node) -> Vec<SemanticAction> {
    let mut actions = Vec::new();
    if supports_semantic_click(node) {
        actions.push(SemanticAction::Click);
    }
    if node.is_focusable() {
        actions.push(SemanticAction::Focus);
    }
    match &node.kind {
        NodeKind::Input(_) | NodeKind::TextArea(_) => actions.push(SemanticAction::SetValue),
        NodeKind::Checkbox(_) => actions.push(SemanticAction::Toggle),
        NodeKind::ScrollView(_)
        | NodeKind::DocumentView(_)
        | NodeKind::List(_)
        | NodeKind::Table(_) => actions.push(SemanticAction::Scroll),
        NodeKind::DragSource(_) | NodeKind::DropTarget(_) => actions.push(SemanticAction::Drag),
        NodeKind::DraggableTabBar(tabs)
            if tabs.on_reorder.is_some() || tabs.on_transfer.is_some() =>
        {
            actions.push(SemanticAction::Drag);
        }
        _ => {}
    }
    actions
}

fn supports_semantic_click(node: &Node) -> bool {
    match &node.kind {
        NodeKind::List(list) => {
            list.on_select.is_some()
                || list.on_item_click.is_some()
                || list.on_activate.is_some()
                || list.on_click.is_some()
        }
        NodeKind::Table(table) => {
            table.on_select.is_some() || table.on_activate.is_some() || table.on_click.is_some()
        }
        NodeKind::Tabs(tabs) => tabs.on_change.is_some() || tabs.on_click.is_some(),
        NodeKind::DraggableTabBar(tabs) => {
            tabs.on_change.is_some()
                || tabs.on_action.is_some()
                || tabs.on_close.is_some()
                || tabs.on_click.is_some()
        }
        _ => node.has_on_click(),
    }
}
