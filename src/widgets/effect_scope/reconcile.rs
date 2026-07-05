use crate::core::component::FocusContext;
use crate::core::element::{Element, ElementKind};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{
    OverlayState, ReconcileCtx, SingleChildReconcile, reconcile_single_child,
};
use crate::style::Rect;

use super::{EffectScope, EffectScopeNode};

pub(crate) fn reconcile_effect_scope(
    tree: &mut NodeTree,
    epoch: u32,
    id: NodeId,
    scope: &EffectScope,
    rect: Rect,
    focus: Option<&FocusContext>,
    overlay_state: &mut OverlayState,
) -> NodeId {
    let scoped_portal_child = effect_scoped_portal_child(scope);
    let child = scoped_portal_child.as_ref().or(scope.child.as_deref());
    let effects = if scoped_portal_child.is_some() {
        Vec::new()
    } else {
        scope.effects.clone()
    };

    let old_children = {
        let node = tree.node_mut(id);
        node.rect = rect;
        node.kind = NodeKind::EffectScope(EffectScopeNode { effects });
        std::mem::take(&mut node.children)
    };

    let new_children = reconcile_single_child(
        &mut ReconcileCtx {
            tree,
            epoch,
            focus,
            overlay_state,
        },
        SingleChildReconcile {
            parent_id: id,
            child,
            rect,
            old_children,
        },
    );

    let node = tree.node_mut(id);
    node.children = new_children;

    id
}

fn effect_scoped_portal_child(scope: &EffectScope) -> Option<Element> {
    if scope.effects.is_empty() {
        return None;
    }

    let child = scope.child.as_deref()?;
    effect_scoped_portal_element(child, scope)
}

fn effect_scoped_portal_element(element: &Element, scope: &EffectScope) -> Option<Element> {
    match &element.kind {
        ElementKind::Portal(portal) => {
            let mut portal = portal.clone();
            portal.content = Box::new(
                EffectScope {
                    child: Some(portal.content.clone()),
                    effects: scope.effects.clone(),
                }
                .into(),
            );

            Some(copy_element_metadata(
                Element::new(ElementKind::Portal(portal)),
                element,
            ))
        }
        ElementKind::Group(group) => {
            let scoped_child = effect_scoped_portal_element(group.child.as_ref(), scope)?;
            let mut group = group.clone();
            group.child = Box::new(scoped_child);

            Some(copy_element_metadata(
                Element::new(ElementKind::Group(group)),
                element,
            ))
        }
        _ => None,
    }
}

fn copy_element_metadata(mut element: Element, source: &Element) -> Element {
    element.layout = source.layout;
    element.key = source.key.clone();
    element
}
