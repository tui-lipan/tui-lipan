//! Layout reconciliation.

use crate::core::component::FocusContext;
use crate::core::element::Element;
use crate::core::node::NodeTree;
use crate::style::Rect;

pub(crate) mod element;
pub(crate) mod overlay;
pub(crate) mod state;
#[cfg(test)]
pub(crate) mod tests;

pub(crate) use self::element::{ElementReconcile, reconcile_element};
pub(crate) use self::overlay::{collect_popover_overlay_roots, reconcile_overlay_entries};
#[cfg(any(feature = "image", feature = "big-text"))]
pub(crate) use self::state::reuse_or_replace_kind;
pub(crate) use self::state::{
    OverlayState, ReconcileCtx, SimpleLeafReconcile, SingleChildReconcile, apply_constraints,
    reconcile_simple_leaf, reconcile_single_child, resolve_rect_with_auto,
};

use crate::layout::tag::can_reuse;

pub(crate) fn reconcile_with_overlays_mode(
    tree: &mut NodeTree,
    root: &Element,
    bounds: Rect,
    focus: Option<&FocusContext>,
    overlays: &[crate::overlay::OverlayEntry],
    allow_root_overlays: bool,
) {
    let epoch = tree.begin_epoch();

    let root_node = tree.root;
    let reuse_root = if tree.is_valid(root_node) {
        can_reuse(tree.node(root_node), root)
    } else {
        false
    };

    let mut overlay_state = OverlayState::new(bounds, allow_root_overlays);
    let _root_id = {
        let mut ctx = ReconcileCtx {
            tree,
            epoch,
            focus,
            overlay_state: &mut overlay_state,
        };
        let root_id = reconcile_element(
            &mut ctx,
            ElementReconcile {
                reuse: reuse_root.then_some(root_node),
                parent: None,
                el: root,
                rect: bounds,
            },
        );
        ctx.tree.root = root_id;
        reconcile_overlay_entries(&mut ctx, overlays);
        root_id
    };

    collect_popover_overlay_roots(tree, &mut overlay_state);
    overlay_state
        .roots
        .sort_by(|a, b| a.layer.cmp(&b.layer).then(a.order.cmp(&b.order)));
    tree.set_overlay_roots(overlay_state.roots);

    tree.sweep(epoch);
    #[cfg(test)]
    tree.assert_tree_sane();
}
