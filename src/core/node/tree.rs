use super::id::NodeId;
use super::iter::NodeDfsIter;
use super::kind::{NodeKind, WidgetNode};
use super::overlay::{OverlayRoot, ScrollbarAxis, ScrollbarTarget, ScrollbarZone};
use crate::core::element::Key;
use crate::style::{Edge, Rect, Theme};
use crate::utils::arena::Arena;
#[cfg(feature = "terminal")]
use crate::widgets::MouseMode;
use crate::widgets::internal::{FrameJoinOverlap, RememberedScrollAnchor, compute_frame_geometry};
use crate::widgets::{DecorationPlacement, SingleDocSelection};

use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::rc::Rc;

thread_local! {
    static DEFAULT_ACTIVE_THEME: Rc<Theme> = Rc::new(Theme::default());
}

fn default_active_theme() -> Rc<Theme> {
    DEFAULT_ACTIVE_THEME.with(Rc::clone)
}

fn node_kind_has_spinners(kind: &NodeKind) -> bool {
    match kind {
        NodeKind::Spinner(_) => true,
        NodeKind::DraggableTabBar(tab_bar) => tab_bar.tabs.iter().any(|tab| {
            tab.leading
                .as_ref()
                .is_some_and(|leading| leading.has_spinner())
        }),
        NodeKind::List(list) => list.items.iter().any(|item| {
            item.status
                .as_ref()
                .is_some_and(|status| status.has_spinner())
                || item
                    .gutter
                    .as_ref()
                    .is_some_and(|gutter| gutter.has_spinner())
        }),
        _ => false,
    }
}

fn node_kind_has_animated_scroll(kind: &NodeKind) -> bool {
    match kind {
        NodeKind::ScrollView(node) => {
            node.smooth_scroll.is_animating() || node.wheel_scroll.is_animating()
        }
        NodeKind::TextArea(node) => node.smooth_scroll.is_animating(),
        NodeKind::DocumentView(node) => node.smooth_scroll.is_animating(),
        _ => false,
    }
}

/// A realized UI node (post-layout, ready for event routing/rendering).
#[derive(Clone)]
pub(crate) struct Node {
    /// Stable id.
    pub id: NodeId,
    /// Optional user-provided key.
    pub key: Option<Key>,
    /// Layout rectangle.
    pub rect: Rect,
    /// Parent node.
    pub parent: Option<NodeId>,
    /// Children node ids.
    pub children: Vec<NodeId>,
    /// Node kind.
    pub kind: NodeKind,
    /// Internal mark used for sweep.
    pub epoch: u32,
    /// Active theme at the point this node was reconciled.
    active_theme: Rc<Theme>,
}

impl Node {
    /// Returns true if this node can receive focus.
    pub fn is_focusable(&self) -> bool {
        self.kind.is_focusable()
    }

    pub fn in_tab_order(&self) -> bool {
        self.kind.in_tab_order()
    }

    /// Returns true if this node has a mouse handler.
    pub fn has_on_click(&self) -> bool {
        self.kind.has_on_click()
    }

    /// Returns true if this node has a pointer-move handler.
    pub fn has_on_mouse_move(&self) -> bool {
        self.kind.has_on_mouse_move()
    }

    /// Returns true if this node can be targeted by hit-testing.
    pub fn is_interactive(&self) -> bool {
        self.is_focusable() || self.has_on_click()
    }

    /// Returns true if this node should show hover feedback.
    /// A node is hoverable if it has `on_click` or defines non-empty hover styles.
    pub fn is_hoverable(&self) -> bool {
        self.kind.is_hoverable_for_theme(self.active_theme())
    }

    /// Active theme for render-time style-slot resolution.
    pub(crate) fn active_theme(&self) -> &Theme {
        &self.active_theme
    }

    pub(crate) fn set_active_theme(&mut self, theme: Rc<Theme>) {
        self.active_theme = theme;
    }

    pub(crate) fn blank(id: NodeId) -> Self {
        Self {
            id,
            key: None,
            rect: Rect::default(),
            parent: None,
            children: Vec::new(),
            kind: NodeKind::Text(crate::widgets::internal::TextNode::default()),
            epoch: 0,
            active_theme: default_active_theme(),
        }
    }

    pub(crate) fn reset_for_reuse(&mut self, id: NodeId) {
        self.id = id;
        self.key = None;
        self.parent = None;
        self.rect = Rect::default();
        self.children.clear();
        self.kind = NodeKind::Text(crate::widgets::internal::TextNode::default());
        self.epoch = 0;
        self.active_theme = default_active_theme();
    }

    pub(crate) fn reset_for_free(&mut self) {
        self.key = None;
        self.parent = None;
        self.children.clear();
        self.epoch = 0;
        self.active_theme = default_active_theme();
    }
}

/// A realized tree of nodes.
#[derive(Clone, Default)]
pub(crate) struct NodeTree {
    /// Root id.
    pub root: NodeId,
    arena: Arena<Node, NodeId>,
    scrollbar_zones_by_x: FxHashMap<i16, Vec<ScrollbarZone>>,
    scrollbar_zones_by_y: FxHashMap<i16, Vec<ScrollbarZone>>,
    scrollbar_zone_vec_pool: Vec<Vec<ScrollbarZone>>,
    overlay_roots: Vec<OverlayRoot>,
    has_hoverables: bool,
    has_mouse_move_handlers: bool,
    has_terminal_any_event: bool,
    has_spinners: bool,
    spinner_ids: Vec<NodeId>,
    has_animated_widgets: bool,
    animated_widget_ids: Vec<NodeId>,
    has_animated_scrolls: bool,
    animated_scroll_ids: Vec<NodeId>,
    has_animated_effect_scopes: bool,
    animated_effect_scope_ids: Vec<NodeId>,
    #[cfg(feature = "image")]
    has_animated_images: bool,
    #[cfg(feature = "image")]
    animated_image_ids: Vec<NodeId>,
    epoch: u32,
    /// Cached sorted focusable node list, lazily populated on first
    /// `focusables()` call per epoch and cleared in `begin_epoch()`.
    cached_focusables: RefCell<Option<Vec<NodeId>>>,
    /// Last frame's "scrolled to bottom" for keyed `ScrollView`s. Survives node-id
    /// churn when layout reparents the same logical timeline (same `Element::key`).
    pub(crate) scroll_was_at_bottom_by_key: FxHashMap<Key, bool>,
    /// Last frame's anchored viewport position for keyed `ScrollView`s. Used to
    /// preserve the visible child when the same logical scroll view is remounted
    /// under a different layout parent.
    pub(crate) remembered_scroll_anchor_by_key: FxHashMap<Key, RememberedScrollAnchor>,
    /// Last input-driven offset for keyed `ScrollView`s. This bridges the frame
    /// where a component mirrors `on_scroll` into state and remounts the same
    /// logical scroll view under a fresh node id.
    pub(crate) scroll_input_offset_by_key: FxHashMap<Key, usize>,
    /// Last input-driven offset for keyed `PanView`s.
    pub(crate) pan_input_offset_by_key: FxHashMap<Key, (i32, i32)>,
    /// Per-child offscreen `DocumentView` states being restored while a keyed
    /// `ScrollView` child is reconciled back into the viewport.
    offscreen_doc_restore_stack: Vec<Vec<SingleDocSelection>>,
    active_theme_stack: Vec<Rc<Theme>>,
}

impl NodeTree {
    /// Create an empty tree.
    pub fn new() -> Self {
        Self {
            root: NodeId::INVALID,
            arena: Arena::new(),
            scrollbar_zones_by_x: FxHashMap::default(),
            scrollbar_zones_by_y: FxHashMap::default(),
            scrollbar_zone_vec_pool: Vec::new(),
            overlay_roots: Vec::new(),
            has_hoverables: false,
            has_mouse_move_handlers: false,
            has_terminal_any_event: false,
            has_spinners: false,
            spinner_ids: Vec::new(),
            has_animated_widgets: false,
            animated_widget_ids: Vec::new(),
            has_animated_scrolls: false,
            animated_scroll_ids: Vec::new(),
            has_animated_effect_scopes: false,
            animated_effect_scope_ids: Vec::new(),
            #[cfg(feature = "image")]
            has_animated_images: false,
            #[cfg(feature = "image")]
            animated_image_ids: Vec::new(),
            epoch: 0,
            cached_focusables: RefCell::new(None),
            scroll_was_at_bottom_by_key: FxHashMap::default(),
            remembered_scroll_anchor_by_key: FxHashMap::default(),
            scroll_input_offset_by_key: FxHashMap::default(),
            pan_input_offset_by_key: FxHashMap::default(),
            offscreen_doc_restore_stack: Vec::new(),
            active_theme_stack: vec![default_active_theme()],
        }
    }

    pub fn is_valid(&self, id: NodeId) -> bool {
        if id.is_invalid() {
            return false;
        }
        self.arena.is_valid(id)
    }

    /// Returns true if any hoverable nodes exist in the tree.
    pub fn has_hoverables(&self) -> bool {
        self.has_hoverables
    }

    /// Returns true if any pointer-move handlers exist in the tree.
    pub fn has_mouse_move_handlers(&self) -> bool {
        self.has_mouse_move_handlers
    }

    /// Returns true if any terminal node requests any-event mouse forwarding.
    pub fn has_terminal_any_event(&self) -> bool {
        self.has_terminal_any_event
    }

    /// Returns true if any spinner glyphs exist in the tree.
    pub fn has_spinners(&self) -> bool {
        self.has_spinners
    }

    pub fn spinner_ids(&self) -> &[NodeId] {
        &self.spinner_ids
    }

    /// Returns true if any animated wrapper nodes are currently transitioning.
    pub fn has_animated_widgets(&self) -> bool {
        self.has_animated_widgets
    }

    /// Returns animated wrapper node ids collected during reconciliation.
    pub fn animated_widget_ids(&self) -> &[NodeId] {
        &self.animated_widget_ids
    }

    /// Returns true if any scrollable nodes are currently smooth-scrolling.
    pub fn has_animated_scrolls(&self) -> bool {
        self.has_animated_scrolls
    }

    /// Returns smooth-scrolling node ids collected during reconciliation.
    pub fn animated_scroll_ids(&self) -> &[NodeId] {
        &self.animated_scroll_ids
    }

    /// Returns true if any effect scopes require frame-to-frame animation.
    pub fn has_animated_effect_scopes(&self) -> bool {
        self.has_animated_effect_scopes
    }

    /// Returns the smallest requested animation interval among animated effect scopes.
    pub fn animated_effect_scope_interval(&self) -> Option<std::time::Duration> {
        self.animated_effect_scope_ids
            .iter()
            .filter_map(|id| {
                self.is_valid(*id)
                    .then(|| &self.node(*id).kind)
                    .and_then(|kind| {
                        if let NodeKind::EffectScope(scope) = kind {
                            scope.animation_interval()
                        } else {
                            None
                        }
                    })
            })
            .min()
    }

    /// Refresh animated widget tracking after widget ticks mutate animation state.
    pub(crate) fn refresh_animated_widget_activity(&mut self) {
        let mut active_ids = Vec::with_capacity(self.animated_widget_ids.len());
        for id in std::mem::take(&mut self.animated_widget_ids) {
            if !self.is_valid(id) {
                continue;
            }
            if matches!(&self.node(id).kind, NodeKind::Animated(animated) if animated.is_animating())
            {
                active_ids.push(id);
            }
        }
        self.has_animated_widgets = !active_ids.is_empty();
        self.animated_widget_ids = active_ids;
    }

    /// Refresh smooth-scroll tracking after runtime ticks mutate animation state.
    pub(crate) fn refresh_animated_scroll_activity(&mut self) {
        let mut active_ids = Vec::with_capacity(self.animated_scroll_ids.len());
        for id in std::mem::take(&mut self.animated_scroll_ids) {
            if !self.is_valid(id) {
                continue;
            }
            if node_kind_has_animated_scroll(&self.node(id).kind) {
                active_ids.push(id);
            }
        }
        self.has_animated_scrolls = !active_ids.is_empty();
        self.animated_scroll_ids = active_ids;
    }

    /// Mark a node as needing scroll animation ticks after input starts animation.
    pub(crate) fn mark_animated_scroll(&mut self, id: NodeId) {
        if !self.is_valid(id) {
            return;
        }
        self.has_animated_scrolls = true;
        if !self.animated_scroll_ids.contains(&id) {
            self.animated_scroll_ids.push(id);
        }
    }

    /// Refresh animated effect-scope tracking after effect-state changes.
    pub(crate) fn refresh_animated_effect_scope_activity(&mut self) {
        let mut active_ids = Vec::with_capacity(self.animated_effect_scope_ids.len());
        for id in std::mem::take(&mut self.animated_effect_scope_ids) {
            if !self.is_valid(id) {
                continue;
            }
            if matches!(&self.node(id).kind, NodeKind::EffectScope(scope) if scope.has_animated_effects())
            {
                active_ids.push(id);
            }
        }
        self.has_animated_effect_scopes = !active_ids.is_empty();
        self.animated_effect_scope_ids = active_ids;
    }

    /// Returns true if any animated image nodes exist in the tree.
    #[cfg(feature = "image")]
    pub fn has_animated_images(&self) -> bool {
        self.has_animated_images
    }

    /// Returns the animated image node ids collected during reconciliation.
    #[cfg(feature = "image")]
    pub fn animated_image_ids(&self) -> &[NodeId] {
        &self.animated_image_ids
    }

    pub(crate) fn epoch(&self) -> u32 {
        self.epoch
    }

    /// Access a node by id.
    pub fn node(&self, id: NodeId) -> &Node {
        self.arena.get(id)
    }

    /// Returns true if `node_id` or any of its ancestors carries the given `key`.
    #[cfg(feature = "devtools")]
    pub fn node_has_ancestor_with_key(&self, node_id: NodeId, key: &str) -> bool {
        let mut cur = Some(node_id);
        while let Some(id) = cur {
            if !self.is_valid(id) {
                break;
            }
            let n = self.node(id);
            if n.key.as_ref().is_some_and(|k| k.as_ref() == key) {
                return true;
            }
            cur = n.parent;
        }
        false
    }

    /// Check whether a single `Frame` node exposes an integrated edge on the given axis.
    ///
    /// Returns `true` when the frame has a drawn border (which covers all edges) or a
    /// `DecorationPlacement::Border` decoration matching one of the `edges`.
    fn frame_has_integrated_edge(
        props: &crate::widgets::internal::FrameNode,
        edges: &[Edge],
    ) -> bool {
        props.has_border()
            || props
                .decorations
                .iter()
                .any(|d| d.placement == DecorationPlacement::Border && edges.contains(&d.edge))
    }

    /// Walk ancestors of `node_id` looking for a `Frame` with an integrated edge matching `edges`.
    fn ancestor_has_integrated_edge(&self, node_id: NodeId, edges: &[Edge]) -> Option<bool> {
        if !self.is_valid(node_id) {
            return None;
        }
        let mut cur = self.node(node_id).parent;
        while let Some(id) = cur {
            if !self.is_valid(id) {
                break;
            }
            let n = self.node(id);
            if let NodeKind::Frame(props) = &n.kind
                && Self::frame_has_integrated_edge(props, edges)
            {
                return Some(true);
            }
            cur = n.parent;
        }
        None
    }

    /// Compute the coordinate of the integrated scrollbar track on the first ancestor `Frame`
    /// that provides one.  `primary`/`fallback` select which edge to prefer (e.g. Right then
    /// Left for vertical scrollbars).  `coord_fn` extracts the coordinate from the body rect.
    fn ancestor_frame_integrated_coord(
        &self,
        mut cur: Option<NodeId>,
        primary: Edge,
        _fallback: Edge,
    ) -> Option<i16> {
        while let Some(id) = cur {
            if !self.is_valid(id) {
                break;
            }
            let n = self.node(id);
            if let NodeKind::Frame(props) = &n.kind {
                let geometry =
                    compute_frame_geometry(props, n.rect, FrameJoinOverlap::default(), true);
                if let Some(coord) = match primary {
                    Edge::Right | Edge::Left => geometry.vscrollbar_track_x,
                    Edge::Top | Edge::Bottom => geometry.hscrollbar_track_y,
                } {
                    return Some(coord);
                }
            }
            cur = n.parent;
        }
        None
    }

    /// Whether some ancestor `Frame` exposes a vertical edge for integrated scrollbars (drawn
    /// border or `DecorationPlacement::Border` on left/right).
    pub fn parent_frame_integrated_v_edge(&self, node_id: NodeId) -> Option<bool> {
        self.ancestor_has_integrated_edge(node_id, &[Edge::Left, Edge::Right])
    }

    /// Whether some ancestor `Frame` exposes a horizontal edge for integrated scrollbars (drawn
    /// border or `DecorationPlacement::Border` on top/bottom).
    pub fn parent_frame_integrated_h_edge(&self, node_id: NodeId) -> Option<bool> {
        self.ancestor_has_integrated_edge(node_id, &[Edge::Top, Edge::Bottom])
    }

    /// X-coordinate of the integrated vertical scrollbar column on the first ancestor `Frame` that
    /// provides one (border or left/right `Border` decoration), if any.
    pub(crate) fn ancestor_frame_integrated_vscrollbar_x(
        &self,
        cur: Option<NodeId>,
    ) -> Option<i16> {
        self.ancestor_frame_integrated_coord(cur, Edge::Right, Edge::Left)
    }

    /// Y-coordinate of the integrated horizontal scrollbar row on the first ancestor `Frame` that
    /// provides one, if any.
    pub(crate) fn ancestor_frame_integrated_hscrollbar_y(
        &self,
        cur: Option<NodeId>,
    ) -> Option<i16> {
        self.ancestor_frame_integrated_coord(cur, Edge::Bottom, Edge::Top)
    }

    /// Mutable access to a node by id.
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.arena.get_mut(id)
    }

    pub(crate) fn current_active_theme(&self) -> Rc<Theme> {
        self.active_theme_stack
            .last()
            .cloned()
            .unwrap_or_else(default_active_theme)
    }

    pub(crate) fn push_active_theme(&mut self, theme: Theme) {
        self.active_theme_stack.push(Rc::new(theme));
    }

    pub(crate) fn set_base_active_theme(&mut self, theme: Theme) {
        if self.active_theme_stack.is_empty() {
            self.active_theme_stack.push(Rc::new(theme));
        } else {
            self.active_theme_stack[0] = Rc::new(theme);
            self.active_theme_stack.truncate(1);
        }
    }

    pub(crate) fn pop_active_theme(&mut self) {
        if self.active_theme_stack.len() > 1 {
            self.active_theme_stack.pop();
        }
    }

    /// Iterate nodes in depth-first order (root → leaves).
    pub fn iter(&self) -> NodeDfsIter<'_> {
        let mut stack = Vec::new();
        if self.is_valid(self.root) {
            stack.push(self.root);
        }
        NodeDfsIter { tree: self, stack }
    }

    /// Iterate nodes in depth-first order including all overlay subtrees.
    ///
    /// `iter()` only walks nodes reachable from `self.root`; overlays (modals,
    /// portals, popovers) are kept in a parallel `overlay_roots` list and
    /// therefore invisible to it. Use this when a search must consider
    /// overlay descendants — e.g. resolving a focused-key lookup against a
    /// node mounted inside a modal.
    pub fn iter_with_overlays(&self) -> NodeDfsIter<'_> {
        let mut stack = Vec::new();
        // Push overlays first so the main tree is popped (visited) first.
        for root in self.overlay_roots.iter().rev() {
            if self.is_valid(root.id) {
                stack.push(root.id);
            }
        }
        if self.is_valid(self.root) {
            stack.push(self.root);
        }
        NodeDfsIter { tree: self, stack }
    }

    pub(crate) fn register_scrollbar_zone(&mut self, id: NodeId) {
        let zones = {
            let node = self.node(id);
            scrollbar_zones(self, node)
        };
        for zone in zones {
            match zone.axis {
                ScrollbarAxis::Vertical => {
                    let zones = match self.scrollbar_zones_by_x.entry(zone.rect.x) {
                        Entry::Occupied(entry) => entry.into_mut(),
                        Entry::Vacant(entry) => {
                            let zones = self.scrollbar_zone_vec_pool.pop().unwrap_or_default();
                            entry.insert(zones)
                        }
                    };
                    zones.push(zone);
                }
                ScrollbarAxis::Horizontal => {
                    let zones = match self.scrollbar_zones_by_y.entry(zone.rect.y) {
                        Entry::Occupied(entry) => entry.into_mut(),
                        Entry::Vacant(entry) => {
                            let zones = self.scrollbar_zone_vec_pool.pop().unwrap_or_default();
                            entry.insert(zones)
                        }
                    };
                    zones.push(zone);
                }
            }
        }
    }

    pub(crate) fn scrollbar_target_at(&self, x: i16, y: i16) -> Option<ScrollbarTarget> {
        let vertical = self
            .scrollbar_zones_by_x
            .get(&x)
            // Iterate in reverse to respect z-order (last-registered/top-most first)
            .and_then(|zones| {
                zones.iter().rev().find(|zone| {
                    zone.contains(x, y) && self.scrollbar_zone_visible_at(zone.id, x, y)
                })
            });

        if let Some(zone) = vertical {
            return Some(ScrollbarTarget {
                id: zone.id,
                axis: zone.axis,
            });
        }

        let horizontal = self.scrollbar_zones_by_y.get(&y).and_then(|zones| {
            zones
                .iter()
                .rev()
                .find(|zone| zone.contains(x, y) && self.scrollbar_zone_visible_at(zone.id, x, y))
        });

        horizontal.map(|zone| ScrollbarTarget {
            id: zone.id,
            axis: zone.axis,
        })
    }

    fn scrollbar_zone_visible_at(&self, target: NodeId, x: i16, y: i16) -> bool {
        if !self.is_valid(target) {
            return false;
        }

        let mut current = target;
        loop {
            let Some(parent_id) = self.node(current).parent else {
                return true;
            };
            if !self.is_valid(parent_id) {
                return false;
            }

            let parent = self.node(parent_id);
            if let NodeKind::Canvas(canvas) = &parent.kind {
                if !parent.rect.contains(x, y) {
                    return false;
                }

                if !canvas.passthrough {
                    let top_containing_child =
                        parent.children.iter().rev().copied().find(|child| {
                            self.is_valid(*child) && self.node(*child).rect.contains(x, y)
                        });
                    if top_containing_child != Some(current) {
                        return false;
                    }
                }
            }

            current = parent_id;
        }
    }

    pub(crate) fn set_overlay_roots(&mut self, roots: Vec<OverlayRoot>) {
        self.overlay_roots = roots;
    }

    pub(crate) fn overlay_roots(&self) -> &[OverlayRoot] {
        &self.overlay_roots
    }

    pub(crate) fn top_capturing_overlay(&self) -> Option<&OverlayRoot> {
        self.overlay_roots
            .iter()
            .rev()
            .find(|root| root.captures_focus)
    }

    fn test_overlays(&self, x: i16, y: i16, kind: TestKind) -> OverlayRouting {
        for root in self.overlay_roots.iter().rev() {
            if let Some(hit) = self.depth_first_test(root.id, x, y, kind) {
                return OverlayRouting::Hit(hit);
            }
            if self.overlay_captures_pointer(root, x, y) {
                return OverlayRouting::Blocked;
            }
        }
        OverlayRouting::Miss
    }

    fn overlay_captures_pointer(&self, root: &OverlayRoot, x: i16, y: i16) -> bool {
        match root.captures_pointer {
            crate::overlay::PointerCapture::None => false,
            crate::overlay::PointerCapture::BackdropFullScreen => true,
            crate::overlay::PointerCapture::RectOnly => {
                self.is_valid(root.id) && self.node(root.id).rect.contains(x, y)
            }
        }
    }

    /// Return the deepest interactive node at `(x, y)`, respecting overlay layering.
    pub fn hit_test(&self, x: i16, y: i16) -> Option<NodeId> {
        self.do_test(x, y, TestKind::Hit)
    }

    /// Return the deepest hoverable node at `(x, y)`, respecting overlay layering.
    pub fn hover_test(&self, x: i16, y: i16) -> Option<NodeId> {
        self.do_test(x, y, TestKind::Hover)
    }

    /// Return the deepest pointer-move-enabled node at `(x, y)`, respecting overlay/z-order rules.
    pub fn mouse_move_test(&self, x: i16, y: i16) -> Option<NodeId> {
        self.do_test(x, y, TestKind::MouseMove)
    }

    fn do_test(&self, x: i16, y: i16, kind: TestKind) -> Option<NodeId> {
        if !self.is_valid(self.root) {
            return None;
        }
        match self.test_overlays(x, y, kind) {
            OverlayRouting::Hit(hit) => return Some(hit),
            OverlayRouting::Blocked => return None,
            OverlayRouting::Miss => {}
        }
        self.depth_first_test(self.root, x, y, kind)
    }

    pub(crate) fn is_descendant(&self, root: NodeId, id: NodeId) -> bool {
        if !self.is_valid(root) || !self.is_valid(id) {
            return false;
        }
        let mut current = Some(id);
        while let Some(node_id) = current {
            if node_id == root {
                return true;
            }
            current = self.node(node_id).parent;
        }
        false
    }

    pub(crate) fn focusables_in_subtree(&self, root: NodeId) -> Vec<NodeId> {
        if !self.is_valid(root) {
            return Vec::new();
        }
        let mut out = Vec::new();
        self.collect_focusables(root, &mut out);
        out
    }

    /// Collect focusable nodes in traversal order, sorted by definition order.
    ///
    /// The result is cached for the current epoch - the first call computes and
    /// stores it; subsequent calls within the same epoch return a clone.
    pub fn focusables(&self) -> Vec<NodeId> {
        if let Some(cached) = self.cached_focusables.borrow().as_ref() {
            return cached.clone();
        }
        if !self.is_valid(self.root) {
            return Vec::new();
        }
        let mut out = Vec::new();
        self.collect_focusables(self.root, &mut out);
        out.sort_by_key(|id| id.index());
        *self.cached_focusables.borrow_mut() = Some(out.clone());
        out
    }

    fn collect_focusables(&self, id: NodeId, out: &mut Vec<NodeId>) {
        let node = self.node(id);
        if node.in_tab_order() {
            out.push(id);
        }
        for &child in &node.children {
            if self.is_valid(child) {
                self.collect_focusables(child, out);
            }
        }
    }

    /// Notify the tree that a node's kind was just set during reconciliation.
    ///
    /// This incrementally tracks `has_hoverables`, pointer-move listeners,
    /// any-event terminals, spinner glyph hosts, and
    /// `has_animated_images` so that
    /// the expensive post-reconciliation `refresh_hoverables()` DFS can be
    /// skipped entirely.
    #[inline]
    pub(crate) fn note_kind_set(&mut self, id: NodeId) {
        let node = self.arena.get(id);
        if !self.has_hoverables && node.is_hoverable() {
            self.has_hoverables = true;
        }
        if !self.has_mouse_move_handlers && node.has_on_mouse_move() {
            self.has_mouse_move_handlers = true;
        }
        #[cfg(feature = "terminal")]
        if !self.has_terminal_any_event
            && matches!(&node.kind, NodeKind::Terminal(terminal)
                if terminal.mouse_mode.mode == MouseMode::AnyEvent && terminal.on_mouse_forward.is_some())
        {
            self.has_terminal_any_event = true;
        }
        let has_spinner = node_kind_has_spinners(&node.kind);
        if !self.has_spinners && has_spinner {
            self.has_spinners = true;
        }
        if has_spinner {
            self.spinner_ids.push(id);
        }
        if matches!(&node.kind, NodeKind::Animated(animated) if animated.is_animating()) {
            self.has_animated_widgets = true;
            if !self.animated_widget_ids.contains(&id) {
                self.animated_widget_ids.push(id);
            }
        }
        if node_kind_has_animated_scroll(&node.kind) {
            self.has_animated_scrolls = true;
            if !self.animated_scroll_ids.contains(&id) {
                self.animated_scroll_ids.push(id);
            }
        }
        if matches!(&node.kind, NodeKind::EffectScope(scope) if scope.has_animated_effects()) {
            self.has_animated_effect_scopes = true;
            if !self.animated_effect_scope_ids.contains(&id) {
                self.animated_effect_scope_ids.push(id);
            }
        }
        #[cfg(feature = "image")]
        if !self.has_animated_images
            && matches!(&node.kind, NodeKind::Image(image) if image.is_animated())
        {
            self.has_animated_images = true;
        }
        #[cfg(feature = "image")]
        if matches!(&node.kind, NodeKind::Image(image) if image.is_animated()) {
            self.animated_image_ids.push(id);
        }
    }

    pub(crate) fn begin_epoch(&mut self) -> u32 {
        self.scrollbar_zone_vec_pool
            .extend(self.scrollbar_zones_by_x.drain().map(|(_, mut zones)| {
                zones.clear();
                zones
            }));
        self.scrollbar_zone_vec_pool
            .extend(self.scrollbar_zones_by_y.drain().map(|(_, mut zones)| {
                zones.clear();
                zones
            }));
        self.overlay_roots.clear();
        self.has_hoverables = false;
        self.has_mouse_move_handlers = false;
        self.has_terminal_any_event = false;
        self.has_spinners = false;
        self.spinner_ids.clear();
        self.has_animated_widgets = false;
        self.animated_widget_ids.clear();
        self.has_animated_scrolls = false;
        self.animated_scroll_ids.clear();
        self.has_animated_effect_scopes = false;
        self.animated_effect_scope_ids.clear();
        #[cfg(feature = "image")]
        {
            self.has_animated_images = false;
            self.animated_image_ids.clear();
        }
        self.active_theme_stack.truncate(1);
        *self.cached_focusables.borrow_mut() = None;
        self.offscreen_doc_restore_stack.clear();
        self.epoch = self.epoch.wrapping_add(1).max(1);
        self.epoch
    }

    pub(crate) fn push_offscreen_doc_restore(&mut self, mut docs: Vec<SingleDocSelection>) {
        docs.reverse();
        self.offscreen_doc_restore_stack.push(docs);
    }

    pub(crate) fn pop_offscreen_doc_restore(&mut self) {
        self.offscreen_doc_restore_stack.pop();
    }

    pub(crate) fn take_next_offscreen_doc_restore(&mut self) -> Option<SingleDocSelection> {
        self.offscreen_doc_restore_stack.last_mut()?.pop()
    }

    pub(crate) fn alloc(&mut self) -> NodeId {
        self.arena.alloc_with(Node::blank, Node::reset_for_reuse)
    }

    pub(crate) fn sweep(&mut self, current_epoch: u32) {
        self.arena
            .sweep(|node| node.epoch != current_epoch, Node::reset_for_free);

        if !self.is_valid(self.root) {
            self.root = NodeId::INVALID;
        }
    }

    fn depth_first_test(&self, start: NodeId, x: i16, y: i16, kind: TestKind) -> Option<NodeId> {
        let mut stack = vec![TraversalFrame::new(start)];

        while let Some(frame) = stack.last_mut() {
            if !frame.entered {
                frame.entered = true;
                if !self.is_valid(frame.id) {
                    stack.pop();
                    continue;
                }

                let node = self.node(frame.id);
                if !node.rect.contains(x, y) {
                    stack.pop();
                    continue;
                }

                if let NodeKind::ZStack(zstack) = &node.kind {
                    frame.blocks_self_on_child_miss = !zstack.passthrough;
                    let mut containing: Vec<NodeId> = node
                        .children
                        .iter()
                        .rev()
                        .copied()
                        .filter(|child| {
                            self.is_valid(*child) && self.node(*child).rect.contains(x, y)
                        })
                        .collect();
                    if frame.blocks_self_on_child_miss {
                        containing.truncate(1);
                    }
                    frame.children = containing;
                } else if let NodeKind::Canvas(canvas) = &node.kind {
                    frame.blocks_self_on_child_miss = !canvas.passthrough;
                    let mut containing: Vec<NodeId> = node
                        .children
                        .iter()
                        .rev()
                        .copied()
                        .filter(|child| {
                            self.is_valid(*child) && self.node(*child).rect.contains(x, y)
                        })
                        .collect();
                    if frame.blocks_self_on_child_miss {
                        containing.truncate(1);
                    }
                    frame.children = containing;
                } else if let NodeKind::Splitter(splitter) = &node.kind {
                    // `join_frame` places handles on the shared seam, which can overlap the
                    // trailing edge of the previous pane's rect. Skip pane descent on handle
                    // pixels so hit / hover / drag target the splitter instead of the child.
                    frame.children = if splitter.handle_at(x, y).is_some() {
                        Vec::new()
                    } else {
                        node.children
                            .iter()
                            .rev()
                            .copied()
                            .filter(|child| self.is_valid(*child))
                            .collect()
                    };
                } else {
                    frame.children = node
                        .children
                        .iter()
                        .rev()
                        .copied()
                        .filter(|child| self.is_valid(*child))
                        .collect();
                }
            }

            if frame.next_child < frame.children.len() {
                let child = frame.children[frame.next_child];
                frame.next_child += 1;
                stack.push(TraversalFrame::new(child));
                continue;
            }

            let done = stack.pop().expect("frame must exist");
            if done.blocks_self_on_child_miss && !done.children.is_empty() {
                continue;
            }

            let node = self.node(done.id);
            let is_match = match kind {
                TestKind::Hit => {
                    if let Some(hit) = node.kind.hit_test_refinement(x, y, node.rect) {
                        hit
                    } else {
                        node.is_interactive()
                    }
                }
                TestKind::Hover => {
                    if let Some(hit) = node.kind.hover_test_refinement(x, y, node.rect) {
                        hit && node.is_hoverable()
                    } else {
                        node.is_hoverable()
                    }
                }
                TestKind::MouseMove => {
                    if let Some(hit) = node.kind.hit_test_refinement(x, y, node.rect)
                        && !hit
                    {
                        false
                    } else {
                        node.has_on_mouse_move()
                    }
                }
            };
            if is_match {
                return Some(done.id);
            }
        }

        None
    }

    #[cfg(test)]
    pub(crate) fn assert_tree_sane(&self) {
        use std::collections::HashSet;

        if !self.is_valid(self.root) {
            assert!(
                self.overlay_roots.is_empty(),
                "overlay roots cannot exist when root is invalid"
            );
            return;
        }

        let mut reachable = HashSet::new();
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            if !self.is_valid(id) || !reachable.insert(id) {
                continue;
            }
            let node = self.node(id);
            assert_eq!(
                node.epoch, self.epoch,
                "reachable root-tree node must be in current epoch"
            );
            let mut seen_children = HashSet::new();
            for &child in &node.children {
                assert!(self.is_valid(child), "node contains invalid child id");
                assert!(
                    seen_children.insert(child),
                    "duplicate child id under same parent"
                );
                let child_node = self.node(child);
                assert_eq!(
                    child_node.parent,
                    Some(id),
                    "child must point back to expected parent"
                );
                stack.push(child);
            }
        }

        for overlay in &self.overlay_roots {
            assert!(self.is_valid(overlay.id), "overlay root id must be valid");
            let overlay_node = self.node(overlay.id);
            assert_eq!(
                overlay_node.epoch, self.epoch,
                "overlay root node must be in current epoch"
            );

            let mut overlay_stack = vec![overlay.id];
            while let Some(id) = overlay_stack.pop() {
                if !self.is_valid(id) || !reachable.insert(id) {
                    continue;
                }
                let node = self.node(id);
                assert_eq!(
                    node.epoch, self.epoch,
                    "reachable overlay node must be in current epoch"
                );
                let mut seen_children = HashSet::new();
                for &child in &node.children {
                    assert!(
                        self.is_valid(child),
                        "overlay subtree contains invalid child id"
                    );
                    assert!(
                        seen_children.insert(child),
                        "duplicate child id under overlay node"
                    );
                    let child_node = self.node(child);
                    assert_eq!(
                        child_node.parent,
                        Some(id),
                        "overlay child must point back to expected parent"
                    );
                    overlay_stack.push(child);
                }
            }
        }

        let mut full_hoverables = false;
        let mut full_mouse_move_handlers = false;
        #[cfg(feature = "terminal")]
        let mut full_terminal_any_event = false;
        let mut full_spinners = false;
        let mut full_spinner_ids = Vec::new();
        let mut full_animated_widgets = false;
        let mut full_animated_widget_ids = Vec::new();
        let mut full_animated_scrolls = false;
        let mut full_animated_scroll_ids = Vec::new();
        let mut full_animated_effect_scopes = false;
        let mut full_animated_effect_scope_ids = Vec::new();
        #[cfg(feature = "image")]
        let mut full_animated_images = false;
        #[cfg(feature = "image")]
        let mut full_animated_image_ids = Vec::new();

        for node in self.arena.iter_active() {
            if node.epoch != self.epoch {
                continue;
            }
            full_hoverables |= node.is_hoverable();
            full_mouse_move_handlers |= node.has_on_mouse_move();
            #[cfg(feature = "terminal")]
            {
                full_terminal_any_event |= matches!(&node.kind, NodeKind::Terminal(terminal)
                    if terminal.mouse_mode.mode == MouseMode::AnyEvent && terminal.on_mouse_forward.is_some());
            }
            if node_kind_has_spinners(&node.kind) {
                full_spinners = true;
                full_spinner_ids.push(node.id);
            }
            if matches!(&node.kind, NodeKind::Animated(animated) if animated.is_animating()) {
                full_animated_widgets = true;
                full_animated_widget_ids.push(node.id);
            }
            if node_kind_has_animated_scroll(&node.kind) {
                full_animated_scrolls = true;
                full_animated_scroll_ids.push(node.id);
            }
            if matches!(&node.kind, NodeKind::EffectScope(scope) if scope.has_animated_effects()) {
                full_animated_effect_scopes = true;
                full_animated_effect_scope_ids.push(node.id);
            }
            #[cfg(feature = "image")]
            {
                if matches!(&node.kind, NodeKind::Image(image) if image.is_animated()) {
                    full_animated_images = true;
                    full_animated_image_ids.push(node.id);
                }
            }
        }

        assert_eq!(
            self.has_hoverables, full_hoverables,
            "incremental has_hoverables must match full scan"
        );
        assert_eq!(
            self.has_mouse_move_handlers, full_mouse_move_handlers,
            "incremental has_mouse_move_handlers must match full scan"
        );
        #[cfg(feature = "terminal")]
        assert_eq!(
            self.has_terminal_any_event, full_terminal_any_event,
            "incremental has_terminal_any_event must match full scan"
        );
        assert_eq!(
            self.has_spinners, full_spinners,
            "incremental has_spinners must match full scan"
        );
        assert_eq!(
            self.spinner_ids, full_spinner_ids,
            "incremental spinner ids must match full scan"
        );
        assert_eq!(
            self.has_animated_widgets, full_animated_widgets,
            "incremental has_animated_widgets must match full scan"
        );
        assert_eq!(
            self.animated_widget_ids, full_animated_widget_ids,
            "incremental animated widget ids must match full scan"
        );
        assert_eq!(
            self.has_animated_scrolls, full_animated_scrolls,
            "incremental has_animated_scrolls must match full scan"
        );
        assert_eq!(
            self.animated_scroll_ids, full_animated_scroll_ids,
            "incremental animated scroll ids must match full scan"
        );
        assert_eq!(
            self.has_animated_effect_scopes, full_animated_effect_scopes,
            "incremental has_animated_effect_scopes must match full scan"
        );
        assert_eq!(
            self.animated_effect_scope_ids, full_animated_effect_scope_ids,
            "incremental animated effect scope ids must match full scan"
        );
        #[cfg(feature = "image")]
        assert_eq!(
            self.has_animated_images, full_animated_images,
            "incremental has_animated_images must match full scan"
        );
        #[cfg(feature = "image")]
        assert_eq!(
            self.animated_image_ids, full_animated_image_ids,
            "incremental animated image ids must match full scan"
        );
    }
}

#[derive(Clone, Copy)]
enum TestKind {
    Hit,
    Hover,
    MouseMove,
}

#[derive(Clone, Copy)]
enum OverlayRouting {
    Hit(NodeId),
    Blocked,
    Miss,
}

struct TraversalFrame {
    id: NodeId,
    entered: bool,
    children: Vec<NodeId>,
    next_child: usize,
    blocks_self_on_child_miss: bool,
}

impl TraversalFrame {
    fn new(id: NodeId) -> Self {
        Self {
            id,
            entered: false,
            children: Vec::new(),
            next_child: 0,
            blocks_self_on_child_miss: false,
        }
    }
}

pub(crate) fn scrollbar_zones(tree: &NodeTree, node: &Node) -> Vec<ScrollbarZone> {
    let parent_border_x = tree.ancestor_frame_integrated_vscrollbar_x(node.parent);
    let parent_border_y = tree.ancestor_frame_integrated_hscrollbar_y(node.parent);

    node.kind
        .scrollbar_zones(node.id, node.rect, parent_border_x, parent_border_y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::{DismissPolicy, OverlayLayer, PointerCapture};
    use crate::style::{Color, Style};
    use crate::widgets::internal::SplitterNode;
    use crate::widgets::{Button, MouseRegion, Splitter, SplitterHandleMode};

    fn overlay_root(id: NodeId, order: u64, captures_pointer: PointerCapture) -> OverlayRoot {
        OverlayRoot {
            id,
            overlay_id: None,
            layer: OverlayLayer::Modal,
            order,
            dismiss_policy: DismissPolicy::None,
            on_dismiss: None,
            backdrop: None,
            opacity: 1.0,
            captures_focus: false,
            captures_pointer,
            copy_text: None,
            copy_zone: None,
            copy_feedback_active: false,
        }
    }

    fn build_base_tree() -> (NodeTree, NodeId) {
        let mut tree = NodeTree::new();

        let root_id = tree.alloc();
        tree.root = root_id;
        let base_id = tree.alloc();

        {
            let root = tree.node_mut(root_id);
            root.rect = Rect {
                x: 0,
                y: 0,
                w: 30,
                h: 12,
            };
            root.children = vec![base_id];
        }

        {
            let base = tree.node_mut(base_id);
            base.parent = Some(root_id);
            base.rect = Rect {
                x: 0,
                y: 0,
                w: 30,
                h: 12,
            };
            base.children.clear();
            base.kind = NodeKind::from(Button::new("base"));
        }

        (tree, base_id)
    }

    fn alloc_overlay_button(tree: &mut NodeTree, rect: Rect) -> NodeId {
        let id = tree.alloc();
        let overlay = tree.node_mut(id);
        overlay.parent = None;
        overlay.rect = rect;
        overlay.children.clear();
        overlay.kind = NodeKind::from(Button::new("overlay"));
        id
    }

    fn alloc_overlay_frame(tree: &mut NodeTree, rect: Rect) -> NodeId {
        let id = tree.alloc();
        let overlay = tree.node_mut(id);
        overlay.parent = None;
        overlay.rect = rect;
        overlay.children.clear();
        id
    }

    #[test]
    fn overlay_backdrop_full_screen_blocks_base_hits() {
        let (mut tree, _base_id) = build_base_tree();
        let overlay_id = alloc_overlay_frame(
            &mut tree,
            Rect {
                x: 8,
                y: 3,
                w: 10,
                h: 4,
            },
        );

        tree.set_overlay_roots(vec![overlay_root(
            overlay_id,
            0,
            PointerCapture::BackdropFullScreen,
        )]);

        assert_eq!(tree.hit_test(1, 1), None);
        assert_eq!(tree.hit_test(29, 11), None);
    }

    #[test]
    fn overlay_rect_only_blocks_outside_and_hits_inside() {
        let (mut tree, base_id) = build_base_tree();
        let overlay_id = alloc_overlay_button(
            &mut tree,
            Rect {
                x: 10,
                y: 4,
                w: 6,
                h: 3,
            },
        );

        tree.set_overlay_roots(vec![overlay_root(overlay_id, 0, PointerCapture::RectOnly)]);

        assert_eq!(tree.hit_test(11, 5), Some(overlay_id));
        assert_eq!(tree.hit_test(2, 2), Some(base_id));
    }

    #[test]
    fn overlay_none_does_not_block_base_tree() {
        let (mut tree, base_id) = build_base_tree();
        let overlay_id = alloc_overlay_button(
            &mut tree,
            Rect {
                x: 10,
                y: 4,
                w: 6,
                h: 3,
            },
        );

        tree.set_overlay_roots(vec![overlay_root(overlay_id, 0, PointerCapture::None)]);

        assert_eq!(tree.hit_test(11, 5), Some(overlay_id));
        assert_eq!(tree.hit_test(2, 2), Some(base_id));
    }

    #[test]
    fn join_frame_splitter_prefers_handle_overlapping_pane_rect() {
        let splitter_el = Splitter::vertical()
            .handle_mode(SplitterHandleMode::Border)
            .weights(vec![0.5, 0.5])
            .child(Button::new("left"))
            .child(Button::new("right"));

        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        };
        let left_rect = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 5,
        };
        let right_rect = Rect {
            x: 10,
            y: 0,
            w: 10,
            h: 5,
        };
        let handle_rect = Rect {
            x: 9,
            y: 0,
            w: 1,
            h: 5,
        };
        let probe_x = handle_rect.x;
        let probe_y = handle_rect.y + (handle_rect.h as i16 / 2).max(0);
        assert!(
            left_rect.contains(probe_x, probe_y),
            "test expects join_frame handle pixel inside first pane rect"
        );

        let mut tree = NodeTree::new();
        tree.root = NodeId::INVALID;

        let split_id = tree.alloc();
        let left_id = tree.alloc();
        let right_id = tree.alloc();
        tree.root = split_id;

        let mut split_kind = SplitterNode::from(splitter_el);
        split_kind.handle_rects = vec![handle_rect];
        split_kind.pane_sizes = vec![left_rect.w, right_rect.w];

        {
            let n = tree.node_mut(split_id);
            n.rect = bounds;
            n.children = vec![left_id, right_id];
            n.kind = NodeKind::Splitter(split_kind);
        }
        {
            let n = tree.node_mut(left_id);
            n.parent = Some(split_id);
            n.rect = left_rect;
            n.children.clear();
            n.kind = NodeKind::from(Button::new("left"));
        }
        {
            let n = tree.node_mut(right_id);
            n.parent = Some(split_id);
            n.rect = right_rect;
            n.children.clear();
            n.kind = NodeKind::from(Button::new("right"));
        }

        assert_eq!(tree.hit_test(probe_x, probe_y), Some(split_id));
        assert_eq!(tree.hover_test(probe_x, probe_y), Some(split_id));
    }

    #[test]
    fn hover_test_respects_hit_test_refinement() {
        let mut tree = NodeTree::new();
        let region_id = tree.alloc();
        tree.root = region_id;

        let region = tree.node_mut(region_id);
        region.rect = Rect {
            x: 0,
            y: 0,
            w: 4,
            h: 4,
        };
        region.kind = NodeKind::from(
            MouseRegion::new()
                .hover_style(Style::new().bg(Color::Blue))
                .hit_test(|x, y| x == 1 && y == 2),
        );

        assert_eq!(tree.hover_test(1, 2), Some(region_id));
        assert_eq!(tree.hover_test(0, 0), None);
    }

    #[test]
    fn stacked_overlays_respect_top_hit_then_lower_capture() {
        let (mut tree, _base_id) = build_base_tree();

        let bottom_id = alloc_overlay_frame(
            &mut tree,
            Rect {
                x: 7,
                y: 3,
                w: 12,
                h: 4,
            },
        );
        let top_id = alloc_overlay_button(
            &mut tree,
            Rect {
                x: 12,
                y: 5,
                w: 5,
                h: 2,
            },
        );

        tree.set_overlay_roots(vec![
            overlay_root(bottom_id, 0, PointerCapture::BackdropFullScreen),
            overlay_root(top_id, 1, PointerCapture::RectOnly),
        ]);

        assert_eq!(tree.hit_test(2, 2), None);
        assert_eq!(tree.hit_test(13, 5), Some(top_id));
    }
}
