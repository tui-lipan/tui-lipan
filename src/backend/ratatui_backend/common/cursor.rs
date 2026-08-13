//! Hardware-cursor placement for the widgets that own a caret.

use std::cell::Cell as StdCell;

use ratatui::layout::Position;

use crate::core::node::{NodeId, NodeTree};

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
