//! The focus state machine shared by the terminal runner and [`TestBackend`].
//!
//! Both hosts keep their own field layout and build a [`FocusRefs`] over it, so
//! there is exactly one implementation of overlay focus capture, focus-stack
//! save/restore, ring traversal and focus-event delivery. That matters because
//! the focus test suite drives `TestBackend`: a second copy here would mean the
//! tests validate code that never ships.
//!
//! [`TestBackend`]: crate::TestBackend

use crate::app::context::{FocusChanged, FocusChangedHook, FocusEntry, FocusPolicy};
use crate::app::input::focus::{self, FocusDirection};
use crate::callback::Callback;
use crate::core::element::Key;
use crate::core::node::{NodeId, NodeTree, OverlayRoot};
use crate::layout::tag::{Tag, tag_of_node};
use crate::overlay::OverlayId;
use crate::runtime::FocusRequest;

/// Cap on saved focus entries, so a pathological overlay loop cannot grow the
/// stack without bound.
const MAX_FOCUS_STACK_DEPTH: usize = 32;

/// Identifies the overlay that a saved focus entry belongs to.
///
/// Managed overlays carry an [`OverlayId`] that survives reconcile; declarative
/// ones only have a node id, which a reconcile that remounts the overlay can
/// reallocate. A stale `Node` key is compensated for by liveness checks: a save
/// whose overlay no longer resolves to any live overlay root is rebound to the
/// current overlay on the next [`ensure_overlay_focus`], and consumed as the
/// fallback match on dismissal (see [`restore_focus_from_stack`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OverlayKey {
    Managed(OverlayId),
    Node(NodeId),
}

impl OverlayKey {
    pub(crate) fn of(overlay: &OverlayRoot) -> Self {
        overlay
            .overlay_id
            .map_or(Self::Node(overlay.id), Self::Managed)
    }
}

/// Whether `key` still identifies an overlay currently present in the tree.
///
/// A dead key can never match again: either its overlay is gone, or the overlay
/// remounted and its `Node` identity now points at a recycled slot.
fn overlay_is_live(tree: &NodeTree, key: OverlayKey) -> bool {
    tree.overlay_roots()
        .iter()
        .any(|overlay| OverlayKey::of(overlay) == key)
}

/// Focus saved before a capturing overlay took it over.
#[derive(Clone)]
pub(crate) struct FocusStackEntry {
    pub overlay: OverlayKey,
    pub focused: Option<NodeId>,
    pub key: Option<Key>,
    pub tag: Option<Tag>,
}

/// The last transition delivered to focus callbacks.
///
/// `on_blur` is captured at record time rather than re-resolved from `id` when
/// the blur is delivered. `is_valid(id)` only proves the arena slot is live in
/// the current epoch, not that it still holds the same widget - a recycled slot
/// would otherwise fire blur on a widget that never had focus.
#[derive(Clone)]
pub(crate) struct NotifiedFocus {
    pub id: NodeId,
    pub entry: FocusEntry,
    pub on_blur: Option<Callback<()>>,
}

/// Mutable borrows of a host's focus state.
pub(crate) struct FocusRefs<'a> {
    pub policy: FocusPolicy,
    pub focused: &'a mut Option<NodeId>,
    pub focused_key: &'a mut Option<Key>,
    pub focused_tag: &'a mut Option<Tag>,
    pub focus_stack: &'a mut Vec<FocusStackEntry>,
}

impl FocusRefs<'_> {
    fn set_focus(&mut self, tree: &NodeTree, id: NodeId) {
        *self.focused = Some(id);
        *self.focused_key = tree.node(id).key.clone();
        *self.focused_tag = Some(tag_of_node(tree.node(id)));
    }

    fn clear_focus(&mut self) {
        *self.focused = None;
        *self.focused_tag = None;
    }
}

/// Whether a pointer press should act on `node_id` rather than only focus it.
///
/// A focusable widget normally acts only once focused, so the first press focuses
/// it and the next one acts. Non-focusable widgets act immediately. Under
/// [`FocusPolicy::Manual`] every widget acts immediately: the framework never
/// moves focus on click there, so requiring focus first would leave them inert
/// forever.
pub(crate) fn click_target_is_active(
    policy: FocusPolicy,
    focused: Option<NodeId>,
    node_id: NodeId,
    focusable: bool,
) -> bool {
    !focusable || focused == Some(node_id) || policy == FocusPolicy::Manual
}

/// The tab ring for a capturing overlay's subtree.
///
/// Nested `Contain` panes are opaque here just as they are in the global ring,
/// with the same safety valve: when every tab stop in the overlay lives inside
/// a pane, the ring descends through panes rather than leaving the overlay a
/// keyboard dead end (the overlay trap outranks pane containment).
pub(crate) fn overlay_ring(tree: &NodeTree, overlay_id: NodeId) -> Vec<NodeId> {
    let ring = tree.focusables_in_subtree(overlay_id);
    if !ring.is_empty() {
        return ring;
    }
    tree.focusables_in_subtree_unrestricted(overlay_id)
}

/// Whether the top capturing overlay exists and has nothing focusable in it.
///
/// Gates keys that should keep working over an inert overlay (e.g. quit).
pub(crate) fn top_capturing_overlay_is_empty(tree: &NodeTree) -> bool {
    tree.top_capturing_overlay()
        .is_some_and(|overlay| overlay_ring(tree, overlay.id).is_empty())
}

/// Give a capturing overlay focus, or suspend focus underneath it.
///
/// Returns whether focus changed, so the caller can reset caret blink.
pub(crate) fn ensure_overlay_focus(tree: &NodeTree, refs: &mut FocusRefs<'_>) -> bool {
    let Some(overlay) = tree.top_capturing_overlay() else {
        return false;
    };
    let (overlay_id, overlay_key, auto_focus) =
        (overlay.id, OverlayKey::of(overlay), overlay.auto_focus);

    let focused_in_overlay = refs
        .focused
        .is_some_and(|id| tree.is_descendant(overlay_id, id));

    // Save the pre-overlay focus the first time this overlay is seen on top,
    // whatever it went on to do with focus. Skipping the push on some paths is
    // what used to desynchronise the stack: a later dismiss would then pop an
    // entry belonging to a *different* overlay.
    push_focus_stack(tree, refs, overlay_key);

    if !auto_focus {
        if !focused_in_overlay {
            return suspend_focus(refs);
        }
        return false;
    }
    if focused_in_overlay {
        return false;
    }

    let focusables = overlay_ring(tree, overlay_id);
    let Some(&first) = focusables.first() else {
        return suspend_focus(refs);
    };
    refs.set_focus(tree, first);
    true
}

fn push_focus_stack(tree: &NodeTree, refs: &mut FocusRefs<'_>, overlay: OverlayKey) {
    if let Some(top) = refs.focus_stack.last_mut() {
        if top.overlay == overlay {
            return;
        }
        // A dead top entry is this overlay under a stale identity: a reconcile
        // remounted the overlay and recycled its node id (or an earlier overlay
        // closed without restoring). Rebind it rather than pushing a duplicate,
        // which would bury the true pre-overlay focus under a save taken while
        // the overlay already held focus.
        if !overlay_is_live(tree, top.overlay) {
            top.overlay = overlay;
            return;
        }
    }
    if refs.focus_stack.len() >= MAX_FOCUS_STACK_DEPTH {
        refs.focus_stack.remove(0);
    }
    refs.focus_stack.push(FocusStackEntry {
        overlay,
        focused: *refs.focused,
        key: refs.focused_key.clone(),
        tag: *refs.focused_tag,
    });
}

fn suspend_focus(refs: &mut FocusRefs<'_>) -> bool {
    if refs.focused.is_none() && refs.focused_tag.is_none() {
        return false;
    }
    refs.clear_focus();
    true
}

/// Restore the focus saved before `overlay` captured it.
///
/// Only an entry belonging to `overlay` is consumed - with one relaxation: when
/// no entry matches, the topmost entry whose overlay is no longer live is
/// treated as this overlay's save under a stale identity (a declarative overlay
/// remounted between save and dismissal changes its `Node` key). Entries that
/// still match a live overlay are never stolen. If nothing qualifies, focus is
/// left alone; entries stacked above a consumed one are dropped so the stack
/// cannot drift.
pub(crate) fn restore_focus_from_stack(
    tree: &NodeTree,
    refs: &mut FocusRefs<'_>,
    overlay: OverlayKey,
) -> bool {
    let matched = refs
        .focus_stack
        .iter()
        .rposition(|entry| entry.overlay == overlay)
        .or_else(|| {
            refs.focus_stack
                .iter()
                .rposition(|entry| !overlay_is_live(tree, entry.overlay))
        });
    let Some(at) = matched else {
        return false;
    };
    refs.focus_stack.truncate(at + 1);
    let Some(saved) = refs.focus_stack.pop() else {
        return false;
    };

    *refs.focused = saved
        .focused
        .filter(|id| tree.is_valid(*id) && tree.node(*id).is_focusable());
    *refs.focused_key = saved.key;
    *refs.focused_tag = saved.tag;
    focus::restore_focus(
        tree,
        refs.focused,
        refs.focused_key,
        refs.focused_tag,
        refs.policy,
    );
    true
}

/// Cycle focus inside a capturing overlay's trap.
///
/// Returns `true` when the overlay consumed the key, which includes the cases
/// where it traps Tab without taking focus (`auto_focus(false)`, or an overlay
/// with nothing focusable in it).
pub(crate) fn overlay_step(
    tree: &NodeTree,
    refs: &mut FocusRefs<'_>,
    direction: FocusDirection,
) -> bool {
    let Some(overlay) = tree.top_capturing_overlay() else {
        return false;
    };
    let focused_in_overlay = refs
        .focused
        .is_some_and(|id| tree.is_descendant(overlay.id, id));
    if !overlay.auto_focus && !focused_in_overlay {
        return true;
    }

    let focusables = overlay_ring(tree, overlay.id);
    let Some(target) = focus::ring_step(&focusables, *refs.focused, direction) else {
        return true;
    };
    refs.set_focus(tree, target);
    true
}

/// Apply a `Ctx::request_focus` / `blur` / `focus_next` / `focus_prev` request.
///
/// Traversal requests are honoured under every policy: `FocusPolicy::Manual`
/// suppresses framework-*initiated* movement, and these are explicit user
/// initiative.
pub(crate) fn apply_focus_request(
    tree: &NodeTree,
    refs: &mut FocusRefs<'_>,
    request: FocusRequest,
) {
    match request {
        FocusRequest::Key(key) => {
            *refs.focused = None;
            *refs.focused_key = Some(key);
            *refs.focused_tag = None;
        }
        FocusRequest::Clear => {
            *refs.focused = None;
            *refs.focused_key = None;
            *refs.focused_tag = None;
        }
        FocusRequest::Next => {
            if !overlay_step(tree, refs, FocusDirection::Next) {
                focus::focus_next(tree, refs.focused, refs.focused_key, refs.focused_tag);
            }
        }
        FocusRequest::Prev => {
            if !overlay_step(tree, refs, FocusDirection::Prev) {
                focus::focus_prev(tree, refs.focused, refs.focused_key, refs.focused_tag);
            }
        }
    }
}

/// Deliver `on_blur` / `on_focus` / the app hook for a completed transition.
///
/// Callbacks are emitted onto the normal queue, so handlers run on the next pump
/// and can never re-enter reconcile.
pub(crate) fn notify_focus_change(
    tree: &NodeTree,
    focused: Option<NodeId>,
    last_notified: &mut Option<NotifiedFocus>,
    hook: Option<&FocusChangedHook>,
) {
    let current = focused.filter(|id| tree.is_valid(*id)).map(|id| {
        let node = tree.node(id);
        NotifiedFocus {
            id,
            entry: FocusEntry {
                key: node.key.clone(),
                tag: tag_of_node(node),
            },
            on_blur: node.on_blur_callback().cloned(),
        }
    });

    let unchanged = match (&*last_notified, &current) {
        (None, None) => true,
        (Some(old), Some(new)) => {
            old.id == new.id
                || old
                    .entry
                    .key
                    .as_ref()
                    .zip(new.entry.key.as_ref())
                    .is_some_and(|(old, new)| old == new)
        }
        _ => false,
    };
    if unchanged {
        // Keep the live node id and blur callback after a keyed remount so a
        // later blur still reaches the remounted widget.
        *last_notified = current;
        return;
    }

    let previous = last_notified.take();
    *last_notified = current.clone();

    if let Some(callback) = previous.as_ref().and_then(|old| old.on_blur.clone()) {
        callback.emit(());
    }
    if let Some(callback) = current
        .as_ref()
        .and_then(|new| tree.node(new.id).on_focus_callback().cloned())
    {
        callback.emit(());
    }
    if let Some(hook) = hook {
        hook(&FocusChanged {
            old: previous.map(|old| old.entry),
            new: current.map(|new| new.entry),
        });
    }
}
