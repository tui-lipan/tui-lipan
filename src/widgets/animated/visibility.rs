//! Borrowed layout synchronization; cached elements share only scalar runtime measurements.
use std::{cell::Cell, ops::Deref, rc::Rc};

use crate::core::element::{Element, ElementKind};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::widgets::containers::reconcile::stack_reuse_plan;

#[derive(Default)]
pub(crate) struct VisibilityLayout {
    pub progress: Cell<f32>,
    pub natural_size: Cell<(u16, u16)>,
}

/// Element copies own independent measurements; a node explicitly shares its host's handle.
#[derive(Default)]
pub(crate) struct VisibilityLayoutHandle(Rc<VisibilityLayout>);
impl VisibilityLayoutHandle {
    pub fn shared(&self) -> Rc<VisibilityLayout> {
        self.0.clone()
    }
}
impl Deref for VisibilityLayoutHandle {
    type Target = VisibilityLayout;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Clone for VisibilityLayoutHandle {
    fn clone(&self) -> Self {
        Self(Rc::new(VisibilityLayout {
            progress: Cell::new(self.progress.get()),
            natural_size: Cell::new(self.natural_size.get()),
        }))
    }
}

pub(crate) fn prepare_visibility_reflow(
    tree: &NodeTree,
    element: &Element,
    old: Option<NodeId>,
) -> bool {
    let old = old.filter(|id| tree.is_valid(*id));
    let old_children = old
        .map(|id| tree.node(id).children.as_slice())
        .unwrap_or(&[]);
    let children = element.kind.children();
    let reuse = stack_reuse_plan(tree, old_children, &children);
    let mut dynamic = false;
    for (child, old) in children.into_iter().zip(reuse) {
        dynamic |= prepare_visibility_reflow(tree, child, old);
    }
    dynamic |= sync_visibility_layout(tree, element, old);
    if dynamic {
        element.clear_caches();
    }
    dynamic
}

fn sync_visibility_layout(tree: &NodeTree, element: &Element, old: Option<NodeId>) -> bool {
    let ElementKind::Animated(animated) = &element.kind else {
        return false;
    };
    let Some(layout) = &animated.visibility_layout else {
        return false;
    };
    let Some((visible, recipe)) = &animated.visibility else {
        return false;
    };
    let existing = old.and_then(|id| match &tree.node(id).kind {
        NodeKind::Animated(node) => Some(node),
        _ => None,
    });
    let mut state = existing
        .and_then(|node| node.visibility.clone())
        .unwrap_or_else(|| {
            crate::animation::VisibilityAnimationState::for_visibility(recipe.clone(), *visible)
        });
    state.set_visible(*visible, recipe);
    layout.progress.set(state.progress());
    if let Some(previous) = existing.and_then(|node| node.visibility_layout.as_ref()) {
        layout.natural_size.set(previous.natural_size.get());
    }
    element.clear_caches();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{Easing, TransitionConfig, VisibilityAnimation};
    use crate::layout::{LayoutEngine, measure::min_size_constrained};
    use crate::style::{Length, Rect};
    use crate::widgets::{Animated, Text, VStack};
    use std::{rc::Rc, time::Duration};

    #[test]
    fn layout_ticks_reuse_elements_and_invalidate_only_visibility_ancestors() {
        let animated = Animated::new(Text::new("one\ntwo\nthree\nfour"))
            .visibility(
                true,
                VisibilityAnimation::new().enter(TransitionConfig {
                    duration: Duration::from_millis(200),
                    easing: Easing::Linear,
                }),
            )
            .collapse_visibility(true);
        let mirror = animated.visibility_layout.as_ref().unwrap().shared();
        let root: Element = VStack::new()
            .height(Length::Auto)
            .child(Text::new("UNCHANGED"))
            .child(animated)
            .into();
        let copied = root.clone();
        let ElementKind::Animated(copy) = &copied.kind.children()[1].kind else {
            panic!("Animated element");
        };
        let copied_mirror = copy.visibility_layout.as_ref().unwrap().shared();
        assert!(!Rc::ptr_eq(&mirror, &copied_mirror));
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
        let children = root.kind.children();
        let untouched = children[0];
        let pointer = children[1] as *const Element;
        let id = tree.node(tree.root).children[1];
        let NodeKind::Animated(node) = &mut tree.node_mut(id).kind else {
            panic!("Animated node");
        };
        assert!(Rc::ptr_eq(
            node.visibility_layout.as_ref().unwrap(),
            &mirror
        ));
        node.tick(Duration::from_millis(100));
        assert_eq!(mirror.progress.get(), 0.5);
        assert_eq!(copied_mirror.progress.get(), 0.0);
        untouched.layout_hash_cache.set(Some(123));
        let count = Rc::strong_count(&mirror);
        prepare_visibility_reflow(&tree, &root, Some(tree.root));
        assert_eq!(root.kind.children()[1] as *const Element, pointer);
        assert_eq!(Rc::strong_count(&mirror), count);
        assert_eq!(untouched.layout_hash_cache.get(), Some(123));
        assert_eq!(root.layout_hash_cache.get(), None);
        assert_eq!(min_size_constrained(&root, Some(40), None).1, 3);
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
        assert_eq!(tree.node(id).rect.h, 2);
    }
}
