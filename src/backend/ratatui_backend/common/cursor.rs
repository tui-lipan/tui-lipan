//! Hardware-cursor placement for the widgets that own a caret.

use std::cell::Cell as StdCell;

use ratatui::layout::Position;

use crate::backend::ratatui_backend::render::RenderOffset;
use crate::core::node::{NodeId, NodeKind, NodeTree};

/// Where a focused widget asks the host terminal to put its caret.
///
/// The caret is the terminal's own cursor rather than a cell in the frame buffer, so nothing
/// painted afterwards can cover it: a floating pane, a popover, or any later sibling drawn over the
/// focused widget would still have the caret blinking on top of it. Placement therefore asks the
/// tree what sits topmost at the caret cell and withholds the cursor unless that is the requesting
/// widget itself.
pub(crate) struct CursorPlacement<'a> {
    sink: Option<&'a StdCell<Option<Position>>>,
    owner: Option<(&'a NodeTree, NodeId)>,
}

impl<'a> CursorPlacement<'a> {
    /// Placement for a real node: records the position for the runner and honors occlusion.
    pub(crate) fn tracked(
        sink: &'a StdCell<Option<Position>>,
        tree: &'a NodeTree,
        owner: NodeId,
    ) -> Self {
        Self {
            sink: Some(sink),
            owner: Some((tree, owner)),
        }
    }

    /// Placement with no tree behind it, for renderers exercised outside a node tree.
    #[cfg(test)]
    pub(crate) fn untracked() -> Self {
        Self {
            sink: None,
            owner: None,
        }
    }

    /// Put the caret at `position` unless something above the requesting widget covers that cell.
    pub(crate) fn place(&self, f: &mut ratatui::Frame<'_>, position: Position) {
        if self.occluded(position) {
            return;
        }
        f.set_cursor_position(position);
        if let Some(sink) = self.sink {
            sink.set(Some(position));
        }
    }

    /// Whether the cell at `position` is drawn over by something outside the owner's own chain.
    fn occluded(&self, position: Position) -> bool {
        self.owner
            .is_some_and(|(tree, owner)| caret_occluded(tree, owner, position))
    }
}

/// Whether `position` belongs to a layer drawn over `owner` rather than to `owner` itself.
///
/// The runner needs this too: on the incremental-scroll fast path it places the caret from widget
/// state without running the renderers at all.
pub(crate) fn caret_occluded(tree: &NodeTree, owner: NodeId, position: Position) -> bool {
    let (Ok(x), Ok(y)) = (i16::try_from(position.x), i16::try_from(position.y)) else {
        return false;
    };
    if overlay_painted_above(tree, owner, x, y) {
        return true;
    }
    // `hit_test` walks children back to front, so it answers with the topmost interactive node at
    // that cell - the node a click there would reach. Anything neither containing nor contained by
    // the owner is a separate layer sitting on top of it.
    //
    // Interactivity is the limit of what this can see: a decorative layer painted over the caret is
    // not a hit-test target, so the caret still shows through it. That keeps the failure safe (a
    // caret too many, never one missing) and needs no per-widget notion of which cells a paint
    // actually covers.
    tree.hit_test(x, y)
        .is_some_and(|top| !tree.is_descendant(owner, top) && !tree.is_descendant(top, owner))
}

/// Whether an overlay painted after `owner`'s own layer covers `(x, y)`.
///
/// Overlays are painted in `overlay_roots` order, over the base tree, so any overlay after the one
/// holding `owner` (or any overlay at all, for a base-tree owner) draws over its cells. This catches
/// layers `hit_test` cannot see: a toast is not a pointer target unless it is clickable, yet it
/// still paints over the caret.
fn overlay_painted_above(tree: &NodeTree, owner: NodeId, x: i16, y: i16) -> bool {
    let roots = tree.overlay_roots();
    let above = roots
        .iter()
        .rposition(|root| tree.is_descendant(root.id, owner))
        .map_or(0, |index| index + 1);
    roots[above..].iter().any(|root| {
        if !tree.is_valid(root.id) {
            return false;
        }
        let node = tree.node(root.id);
        // Match the painter: an animated overlay (a toast sliding in) draws at its visual offset.
        let offset = match &node.kind {
            NodeKind::Animated(animated) => {
                RenderOffset::ZERO.add_cells(animated.visual_position_offset_cells())
            }
            _ => RenderOffset::ZERO,
        };
        offset.apply_to_rect(node.rect).contains(x, y)
    })
}
