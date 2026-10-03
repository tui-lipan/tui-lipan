//! Borrowed layout synchronization; cached elements share only scalar runtime measurements.
use std::{cell::Cell, ops::Deref, rc::Rc};

use crate::core::element::{Element, ElementKind};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::widgets::containers::reconcile::stack_reuse_plan;

#[derive(Default)]
pub(crate) struct VisibilityLayout {
    progress: Cell<f32>,
    natural_size: Cell<(u16, u16)>,
    dirty: Cell<bool>,
}

impl VisibilityLayout {
    pub fn progress(&self) -> f32 {
        self.progress.get()
    }
    pub fn natural_size(&self) -> (u16, u16) {
        self.natural_size.get()
    }
    pub fn set_progress(&self, progress: f32) {
        if self.progress.replace(progress).to_bits() != progress.to_bits() {
            self.dirty.set(true);
        }
    }
    pub fn set_natural_size(&self, size: (u16, u16)) {
        if self.natural_size.replace(size) != size {
            self.dirty.set(true);
        }
    }
    fn take_dirty(&self) -> bool {
        self.dirty.replace(false)
    }
}

#[cfg(test)]
thread_local! {
    static PROBE_MISSES: Cell<usize> = const { Cell::new(0) };
    static REUSE_PLANS: Cell<usize> = const { Cell::new(0) };
}

fn contains_visibility_layout(element: &Element) -> bool {
    if let Some(contains) = element.visibility_layout_probe_cache.get() {
        return contains;
    }
    #[cfg(test)]
    PROBE_MISSES.with(|count| count.set(count.get() + 1));
    let contains = matches!(&element.kind, ElementKind::Animated(animated) if animated.visibility_layout.is_some())
        || element
            .kind
            .children()
            .into_iter()
            .any(contains_visibility_layout);
    element.visibility_layout_probe_cache.set(Some(contains));
    contains
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
            dirty: Cell::new(false),
        }))
    }
}

pub(crate) fn prepare_visibility_reflow(
    tree: &NodeTree,
    element: &Element,
    old: Option<NodeId>,
) -> bool {
    if !contains_visibility_layout(element) {
        return false;
    }
    let old = old.filter(|id| tree.is_valid(*id));
    let old_children = old
        .map(|id| tree.node(id).children.as_slice())
        .unwrap_or(&[]);
    let children = element.kind.children();
    #[cfg(test)]
    REUSE_PLANS.with(|count| count.set(count.get() + 1));
    let reuse = stack_reuse_plan(tree, old_children, &children);
    let mut dynamic = false;
    for (child, old) in children.into_iter().zip(reuse) {
        dynamic |= prepare_visibility_reflow(tree, child, old);
    }
    dynamic |= sync_visibility_layout(tree, element, old);
    if dynamic {
        element.clear_layout_measure_caches();
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
    if existing
        .and_then(|node| node.visibility_layout.as_ref())
        .is_some_and(|previous| Rc::ptr_eq(previous, &layout.0))
    {
        return layout.take_dirty();
    }
    let mut state = existing
        .and_then(|node| node.visibility.clone())
        .unwrap_or_else(|| {
            crate::animation::VisibilityAnimationState::for_visibility(recipe.clone(), *visible)
        });
    state.set_visible(*visible, recipe);
    layout.set_progress(state.progress());
    if let Some(previous) = existing.and_then(|node| node.visibility_layout.as_ref()) {
        layout.set_natural_size(previous.natural_size());
    }
    // A fresh handle may already have cached measurements from view construction.
    layout.take_dirty();
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
        assert_eq!(mirror.progress(), 0.5);
        assert_eq!(copied_mirror.progress(), 0.0);
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

    #[test]
    fn settled_cached_reconciliation_preserves_visibility_ancestor_caches() {
        let animated = Animated::new(Text::new("one\ntwo\nthree\nfour"))
            .visibility(
                true,
                VisibilityAnimation::new().enter(TransitionConfig {
                    duration: Duration::from_millis(200),
                    easing: Easing::Linear,
                }),
            )
            .collapse_visibility(true);
        let root: Element = VStack::new()
            .height(Length::Auto)
            .child(VStack::new().height(Length::Auto).child(animated))
            .into();
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
        let ancestor_id = tree.node(tree.root).children[0];
        let id = tree.node(ancestor_id).children[0];
        let NodeKind::Animated(node) = &mut tree.node_mut(id).kind else {
            panic!("Animated node");
        };
        node.tick(Duration::from_millis(200));
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
        assert!(!prepare_visibility_reflow(&tree, &root, Some(tree.root)));
        min_size_constrained(&root, Some(40), None);
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
        let ancestor = root.kind.children()[0];
        let visibility = ancestor.kind.children()[0];
        let cached = [&root, ancestor, visibility].map(|element| {
            assert!(element.layout_hash_cache.get().is_some());
            assert!(element.measure_cache.get().iter().any(Option::is_some));
            (element.layout_hash_cache.get(), element.measure_cache.get())
        });
        for _ in 0..3 {
            LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
            for (element, saved) in [&root, ancestor, visibility].into_iter().zip(cached) {
                assert_eq!(
                    (element.layout_hash_cache.get(), element.measure_cache.get()),
                    saved
                );
                assert_eq!(element.visibility_layout_probe_cache.get(), Some(true));
            }
        }
    }

    #[test]
    fn visibility_free_large_tree_builds_no_reuse_plans_and_cached_probe_skips_children() {
        use crate::widgets::HStack;
        let root: Element = VStack::new()
            .children((0..100).map(|_| {
                HStack::new()
                    .children((0..10).map(|_| Text::new("static").into()))
                    .into()
            }))
            .into();
        let tree = NodeTree::new();
        PROBE_MISSES.with(|count| count.set(0));
        REUSE_PLANS.with(|count| count.set(0));
        assert!(!prepare_visibility_reflow(&tree, &root, None));
        assert_eq!(root.visibility_layout_probe_cache.get(), Some(false));
        PROBE_MISSES.with(|count| assert_eq!(count.get(), 1101));
        REUSE_PLANS.with(|count| assert_eq!(count.get(), 0));
        PROBE_MISSES.with(|count| count.set(0));
        root.layout_hash_cache.set(Some(123));
        for _ in 0..64 {
            assert!(!prepare_visibility_reflow(&tree, &root, None));
        }
        PROBE_MISSES.with(|count| assert_eq!(count.get(), 0));
        REUSE_PLANS.with(|count| assert_eq!(count.get(), 0));
        assert_eq!(root.layout_hash_cache.get(), Some(123));
    }
}
