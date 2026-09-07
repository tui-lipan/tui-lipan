use std::sync::Arc;

use super::{AutomationId, SemanticNode, SemanticRole, SemanticTree};

/// Explicit strategy for locating semantic nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Selector {
    /// Match one app-authored automation ID.
    Id(AutomationId),
    /// Match a semantic role and optional accessible name.
    Role {
        /// Required role.
        role: SemanticRole,
        /// Exact accessible name, when set.
        name: Option<Arc<str>>,
    },
    /// Match safe semantic name or value text containing this string.
    TextContains(Arc<str>),
    /// Match the deepest actionable node containing a viewport point.
    Point {
        /// Column.
        x: u16,
        /// Row.
        y: u16,
    },
}

impl Selector {
    /// Select by stable app-authored ID.
    pub fn id(value: impl Into<AutomationId>) -> Self {
        Self::Id(value.into())
    }

    /// Select by semantic role.
    pub fn role(role: SemanticRole) -> Self {
        Self::Role { role, name: None }
    }

    /// Add an exact accessible-name constraint to a role selector.
    ///
    /// Calling this on another selector leaves it unchanged.
    #[must_use]
    pub fn name(mut self, name: impl Into<Arc<str>>) -> Self {
        if let Self::Role {
            name: selector_name,
            ..
        } = &mut self
        {
            *selector_name = Some(name.into());
        }
        self
    }

    /// Select nodes whose safe name or value contains `text`.
    pub fn text_contains(text: impl Into<Arc<str>>) -> Self {
        Self::TextContains(text.into())
    }

    /// Select by a viewport point. This selector is intentionally layout-unstable.
    pub fn point(x: u16, y: u16) -> Self {
        Self::Point { x, y }
    }

    pub(crate) fn matches(&self, node: &SemanticNode) -> bool {
        match self {
            Self::Id(id) => node.automation_id.as_ref() == Some(id),
            Self::Role { role, name } => {
                node.role == *role
                    && name
                        .as_ref()
                        .is_none_or(|name| node.name.as_deref() == Some(name.as_ref()))
            }
            Self::TextContains(needle) => node.searchable_text().contains(needle.as_ref()),
            Self::Point { x, y } => {
                let (Ok(x), Ok(y)) = (i16::try_from(*x), i16::try_from(*y)) else {
                    return false;
                };
                node.in_view && node.clipped_bounds.contains(x, y)
            }
        }
    }
}

/// Public selector match without an internal runtime node ID.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SelectorMatch {
    /// Automation ID, when present.
    pub automation_id: Option<AutomationId>,
    /// Semantic role.
    pub role: SemanticRole,
    /// Accessible name.
    pub name: Option<String>,
    /// Layout bounds.
    pub bounds: crate::style::Rect,
    /// Whether this match intersects the viewport.
    pub in_view: bool,
    /// Whether this match has a valid semantic action target.
    pub actionable: bool,
}

impl From<&SemanticNode> for SelectorMatch {
    fn from(node: &SemanticNode) -> Self {
        Self {
            automation_id: node.automation_id.clone(),
            role: node.role,
            name: node.name.clone(),
            bounds: node.bounds,
            in_view: node.in_view,
            actionable: node.actionable,
        }
    }
}

pub(crate) fn resolve<'a>(tree: &'a SemanticTree, selector: &Selector) -> Vec<&'a SemanticNode> {
    let mut matches: Vec<(&SemanticNode, usize)> = Vec::new();
    for child in &tree.root.children {
        collect_matches(child, selector, 1, &mut matches);
    }
    if matches!(selector, Selector::Point { .. }) {
        matches.sort_by_key(|(node, depth)| {
            (
                std::cmp::Reverse(node.overlay_order.unwrap_or(0)),
                !node.actionable,
                u32::from(node.clipped_bounds.w) * u32::from(node.clipped_bounds.h),
                std::cmp::Reverse(*depth),
            )
        });
        matches.truncate(1);
    }
    matches.into_iter().map(|(node, _)| node).collect()
}

fn collect_matches<'a>(
    node: &'a SemanticNode,
    selector: &Selector,
    depth: usize,
    matches: &mut Vec<(&'a SemanticNode, usize)>,
) {
    if selector.matches(node) {
        matches.push((node, depth));
    }
    for child in &node.children {
        collect_matches(child, selector, depth + 1, matches);
    }
}
