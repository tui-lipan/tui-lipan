//! Allocation attribution for the view, expand, and layout pass.
//!
//! **Not public API.** This is public only because attribution has to be readable from the
//! embedding crate's `GlobalAlloc`, which lives in a different crate. It is gated behind the
//! `alloc-probe` feature, is exempt from semver, and may change or disappear in any release. Do
//! not build anything on it that is not a benchmark.
//!
//! # How it is used
//!
//! The embedding crate installs a counting `GlobalAlloc` and reads [`bucket`] on each allocation,
//! attributing it to whichever part of the pass is running. A sampled CPU profile names the hot
//! frames but not the caller that asked for the memory; this names the caller, which is what
//! separates a removable defect from the shape of the render model.
//!
//! # Cost
//!
//! Without the feature, `probe_bucket!` expands to nothing: no branch, no bookkeeping, no symbol.
//! With it, entering a bucket is one thread-local `Cell` write and leaving it is another. The cell
//! is `const`-initialized and holds a `u8`, so it needs neither lazy initialization nor a
//! destructor, and reading it from inside an allocator therefore neither allocates nor re-enters.
//!
//! [`census`] and [`element_sizes`] do allocate. They describe a tree rather than build one, so
//! call them outside the measured window.

use std::cell::Cell;

use crate::core::element::{
    ContextProviderElement, Element, ElementKind, Group, ThemeProviderElement,
};
use crate::core::memo::MemoElement;
use crate::overlay::Portal;
#[cfg(feature = "big-text")]
use crate::widgets::BigText;
#[cfg(feature = "image")]
use crate::widgets::Image;
#[cfg(feature = "terminal")]
use crate::widgets::Terminal;
use crate::widgets::{
    Animated, AsciiCanvas, Button, Canvas, Center, CenterPin, Chart, Checkbox, ClassDiagram,
    Divider, DocumentView, DragSource, DraggableTabBar, DropTarget, EffectScope, ErDiagram, Flow,
    Flowchart, Frame, GanttDiagram, Graph, Grid, HStack, Heatmap, HexArea, Input, List,
    MouseRegion, PanView, ProgressBar, ScrollView, SequenceDiagram, Spacer, Spinner, Splitter,
    StateDiagram, StatusBarLayout, Tabs, Text, VStack, ZStack,
};

/// Anything outside an instrumented region.
pub const OTHER: u8 = 0;
/// Building the sibling reuse plan and its id bookkeeping.
pub const EXPAND_CHILDREN_PLAN: u8 = 1;
/// The `Vec<Element>` a container hands back.
pub const EXPAND_CHILDREN_OUT: u8 = 2;
/// Mounting and expanding a nested component instance.
pub const EXPAND_COMPONENT: u8 = 3;
/// Rebuilding one non-component element and its subtree.
pub const EXPAND_ELEMENT: u8 = 4;
/// The host component's own `view()`.
pub const VIEW: u8 = 5;
/// `apply_document_theme_carve_out` over the expanded tree.
pub const THEME_CARVE_OUT: u8 = 6;
/// Collecting the frame's overlays.
pub const OVERLAY_SNAPSHOT: u8 = 7;
/// Reconciling the expanded tree into the node tree, and laying it out.
pub const RECONCILE: u8 = 8;
/// Freeing component entries that this epoch did not touch.
pub const SWEEP: u8 = 9;

/// How many buckets [`bucket`] can return.
pub const BUCKETS: usize = 10;

/// Human-readable name per bucket, indexed by the bucket value.
pub const NAMES: [&str; BUCKETS] = [
    "other",
    "expand_children/reuse_plan",
    "expand_children/out_vec",
    "expand_children/component",
    "expand_element",
    "view",
    "apply_document_theme_carve_out",
    "overlay_snapshot",
    "reconcile (layout)",
    "sweep",
];

thread_local! {
    static BUCKET: Cell<u8> = const { Cell::new(OTHER) };
}

/// The bucket allocations on this thread should currently be attributed to.
///
/// Safe to call from inside a `GlobalAlloc`: the cell is const-initialized, so reading it neither
/// allocates nor registers a destructor.
#[inline]
pub fn bucket() -> u8 {
    BUCKET.with(|b| b.get())
}

/// Attributes allocations to `bucket` until dropped, then restores the previous bucket.
///
/// Nesting is last-writer-wins, so an inner guard takes the allocations of the region it covers
/// away from its caller. That is what makes the buckets sum rather than double-count.
pub struct Guard(u8);

impl Guard {
    #[inline]
    /// Start attributing this thread's allocations to `bucket`.
    pub fn new(bucket: u8) -> Self {
        Self(BUCKET.with(|b| b.replace(bucket)))
    }
}

impl Drop for Guard {
    #[inline]
    fn drop(&mut self) {
        BUCKET.with(|b| b.set(self.0));
    }
}

/// What one expanded element tree is made of.
///
/// The counts here are what a per-node or per-container allocation would be multiplied by, so
/// they turn a raw allocation total into "allocations per element".
#[derive(Clone, Copy, Debug, Default)]
pub struct Census {
    /// Every `Element` reachable from the root, root included.
    pub nodes: usize,
    /// Nodes whose children live in a `Vec<Element>` that expansion rebuilds.
    pub vec_containers: usize,
    /// Children summed over those containers.
    pub vec_children: usize,
    /// Spare capacity summed over those containers.
    pub vec_slack: usize,
    /// Nodes holding a single `Box<Element>`-style child.
    pub box_wrappers: usize,
    /// Nodes carrying a `Key`.
    pub keyed: usize,
    /// `Group` nodes, one per expanded nested component.
    pub groups: usize,
    /// Unexpanded `Component` nodes (should be zero in a fully expanded tree).
    pub components: usize,
    /// Deepest path from the root.
    pub depth: usize,
}

/// Walk `root` and report what it is made of.
pub fn census(root: &Element) -> Census {
    let mut out = Census::default();
    walk(root, 1, &mut out);
    out
}

fn walk(element: &Element, depth: usize, out: &mut Census) {
    out.nodes += 1;
    out.depth = out.depth.max(depth);
    if element.key.is_some() {
        out.keyed += 1;
    }
    match &element.kind {
        ElementKind::Group(_) => out.groups += 1,
        ElementKind::Component(_) => out.components += 1,
        _ => {}
    }
    if let Some((len, cap)) = vec_container_shape(&element.kind) {
        out.vec_containers += 1;
        out.vec_children += len;
        out.vec_slack += cap - len;
    } else if !element.kind.children().is_empty() {
        out.box_wrappers += 1;
    }
    for child in element.kind.children() {
        walk(child, depth + 1, out);
    }
}

/// `(len, capacity)` of a kind whose children live in a `Vec<Element>` that expansion rebuilds.
fn vec_container_shape(kind: &ElementKind) -> Option<(usize, usize)> {
    let v = match kind {
        ElementKind::VStack(v) => &v.children,
        ElementKind::HStack(h) => &h.children,
        ElementKind::ZStack(z) => &z.children,
        ElementKind::Flow(f) => &f.children,
        ElementKind::ScrollView(sv) => &sv.children,
        ElementKind::Splitter(sp) => &sp.children,
        _ => return None,
    };
    Some((v.len(), v.capacity()))
}

/// `(variant, payload size)` for every `ElementKind` variant, plus the whole-`Element` size.
///
/// The enum is as wide as its widest payload, and an `Element` is moved, boxed, or copied into a
/// fresh `Vec` several times per node per frame, so this width is the multiplier on nearly every
/// allocation the render pass makes.
pub fn element_sizes() -> Vec<(&'static str, usize)> {
    let mut out: Vec<(&'static str, usize)> = vec![
        ("*Element*", std::mem::size_of::<Element>()),
        ("*ElementKind*", std::mem::size_of::<ElementKind>()),
        (
            "*LayoutConstraints*",
            std::mem::size_of::<crate::style::LayoutConstraints>(),
        ),
    ];
    out.push(("Text", std::mem::size_of::<Text>()));
    #[cfg(feature = "big-text")]
    out.push(("BigText", std::mem::size_of::<BigText>()));
    out.push(("AsciiCanvas", std::mem::size_of::<AsciiCanvas>()));
    out.push(("Button", std::mem::size_of::<Box<Button>>()));
    out.push(("Input", std::mem::size_of::<Box<Input>>()));
    #[cfg(feature = "image")]
    out.push(("Image", std::mem::size_of::<Image>()));
    out.push(("List", std::mem::size_of::<Box<List>>()));
    out.push((
        "TextArea",
        std::mem::size_of::<Box<crate::widgets::TextArea>>(),
    ));
    out.push(("HexArea", std::mem::size_of::<Box<HexArea>>()));
    #[cfg(feature = "terminal")]
    out.push(("Terminal", std::mem::size_of::<Terminal>()));
    out.push(("Popover", std::mem::size_of::<crate::widgets::Popover>()));
    out.push(("Portal", std::mem::size_of::<Portal>()));
    out.push(("Table", std::mem::size_of::<Box<crate::widgets::Table>>()));
    out.push(("Tabs", std::mem::size_of::<Tabs>()));
    out.push((
        "DraggableTabBar",
        std::mem::size_of::<Box<DraggableTabBar>>(),
    ));
    out.push((
        "Component",
        std::mem::size_of::<crate::core::nested::ComponentElement>(),
    ));
    out.push(("Group", std::mem::size_of::<Group>()));
    out.push(("EffectScope", std::mem::size_of::<EffectScope>()));
    out.push(("Animated", std::mem::size_of::<Animated>()));
    out.push(("DragSource", std::mem::size_of::<DragSource>()));
    out.push(("DropTarget", std::mem::size_of::<DropTarget>()));
    out.push(("MouseRegion", std::mem::size_of::<MouseRegion>()));
    out.push(("ScrollView", std::mem::size_of::<Box<ScrollView>>()));
    out.push(("PanView", std::mem::size_of::<PanView>()));
    out.push(("VStack", std::mem::size_of::<VStack>()));
    out.push(("HStack", std::mem::size_of::<HStack>()));
    out.push(("Grid", std::mem::size_of::<Grid>()));
    out.push(("Flow", std::mem::size_of::<Flow>()));
    out.push(("Canvas", std::mem::size_of::<Canvas>()));
    out.push(("Flowchart", std::mem::size_of::<Box<Flowchart>>()));
    out.push(("ZStack", std::mem::size_of::<ZStack>()));
    out.push(("Center", std::mem::size_of::<Center>()));
    out.push(("CenterPin", std::mem::size_of::<CenterPin>()));
    out.push(("Frame", std::mem::size_of::<Frame>()));
    out.push(("Divider", std::mem::size_of::<Divider>()));
    out.push(("Spacer", std::mem::size_of::<Spacer>()));
    out.push((
        "Sparkline",
        std::mem::size_of::<crate::widgets::Sparkline>(),
    ));
    out.push(("Chart", std::mem::size_of::<Box<Chart>>()));
    out.push(("Graph", std::mem::size_of::<Box<Graph>>()));
    out.push((
        "SequenceDiagram",
        std::mem::size_of::<Box<SequenceDiagram>>(),
    ));
    out.push(("ClassDiagram", std::mem::size_of::<Box<ClassDiagram>>()));
    out.push(("StateDiagram", std::mem::size_of::<Box<StateDiagram>>()));
    out.push(("ErDiagram", std::mem::size_of::<Box<ErDiagram>>()));
    out.push(("GanttDiagram", std::mem::size_of::<Box<GanttDiagram>>()));
    out.push(("StatusBarLayout", std::mem::size_of::<StatusBarLayout>()));
    out.push(("Heatmap", std::mem::size_of::<Heatmap>()));
    out.push(("Checkbox", std::mem::size_of::<Checkbox>()));
    out.push(("ProgressBar", std::mem::size_of::<ProgressBar>()));
    out.push(("Slider", std::mem::size_of::<crate::widgets::Slider>()));
    out.push(("Spinner", std::mem::size_of::<Spinner>()));
    out.push(("Splitter", std::mem::size_of::<Splitter>()));
    out.push(("DocumentView", std::mem::size_of::<Box<DocumentView>>()));
    out.push((
        "ThemeProvider",
        std::mem::size_of::<Box<ThemeProviderElement>>(),
    ));
    out.push((
        "ContextProvider",
        std::mem::size_of::<Box<ContextProviderElement>>(),
    ));
    out.push(("Memo", std::mem::size_of::<MemoElement>()));
    out.sort_by_key(|(_, size)| std::cmp::Reverse(*size));
    out
}
