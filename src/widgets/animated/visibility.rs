//! Snapshot controlled visibility before layout; measure content at its allocated width.
use std::borrow::Cow;

use crate::core::element::{Element, ElementKind};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::widgets::containers::reconcile::stack_reuse_plan;

fn has_collapse(element: &Element) -> bool {
    matches!(&element.kind, ElementKind::Animated(animated) if animated.collapse_visibility && animated.visibility.is_some())
        || element
            .kind
            .children()
            .iter()
            .any(|child| has_collapse(child))
}

pub(crate) fn prepare_visibility_reflow<'a>(
    tree: &NodeTree,
    element: &'a Element,
    old: Option<NodeId>,
) -> Cow<'a, Element> {
    if !has_collapse(element) {
        return Cow::Borrowed(element);
    }
    let mut prepared = element.clone();
    prepare_element(tree, &mut prepared, old);
    Cow::Owned(prepared)
}

fn prepare_element(tree: &NodeTree, element: &mut Element, old: Option<NodeId>) {
    let old = old.filter(|id| tree.is_valid(*id));
    let old_children = old
        .map(|id| tree.node(id).children.as_slice())
        .unwrap_or(&[]);
    let children = element.kind.children();
    let reuse = stack_reuse_plan(tree, old_children, &children);
    drop(children);
    for (child, old) in element.kind.children_mut().iter_mut().zip(reuse) {
        prepare_element(tree, child, old);
    }
    let ElementKind::Animated(animated) = &mut element.kind else {
        return;
    };
    if !animated.collapse_visibility {
        return;
    }
    let Some((visible, recipe)) = &animated.visibility else {
        return;
    };
    let mut state = old
        .and_then(|id| match &tree.node(id).kind {
            NodeKind::Animated(node) => node.visibility.clone(),
            _ => None,
        })
        .unwrap_or_else(|| {
            crate::animation::VisibilityAnimationState::for_visibility(recipe.clone(), *visible)
        });
    state.set_visible(*visible, recipe);
    animated.visibility_progress = Some(state.progress());
}
