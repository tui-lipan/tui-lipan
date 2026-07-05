use std::any::{Any, TypeId};
use std::cell::Cell;
use std::sync::Arc;

use smallvec::SmallVec;

use crate::callback::ScopeId;
use crate::core::memo::MemoElement;
use crate::overlay::Portal;
use crate::style::{LayoutConstraints, Length, ShrinkPriority, Theme};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MeasureCacheEntry {
    pub max_w: Option<u16>,
    pub max_h: Option<u16>,
    pub size: (u16, u16),
}

/// A stable identity used for reconciliation and focus.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key(Arc<str>);

impl From<&'static str> for Key {
    fn from(s: &'static str) -> Self {
        Self(Arc::from(s))
    }
}

impl From<String> for Key {
    fn from(s: String) -> Self {
        Self(Arc::from(s))
    }
}

impl From<Arc<str>> for Key {
    fn from(s: Arc<str>) -> Self {
        Self(s)
    }
}

impl AsRef<str> for Key {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A declarative UI node (the Virtual DOM).
#[derive(Clone)]
pub struct Element {
    /// Optional key for stable identity.
    pub(crate) key: Option<Key>,
    /// Concrete element kind.
    pub(crate) kind: ElementKind,
    /// Layout constraints for stack sizing.
    pub(crate) layout: LayoutConstraints,
    /// Cached layout hash for repeated layout-only reconciles.
    pub(crate) layout_hash_cache: Cell<Option<u64>>,
    /// Small MRU cache for repeated min-size measurements.
    pub(crate) measure_cache: Cell<[Option<MeasureCacheEntry>; 2]>,
    /// Memoized [`crate::widgets::element_subtree_has_split_wrap_sync`] result for this subtree.
    pub(crate) split_wrap_probe_cache: Cell<Option<bool>>,
}

impl Element {
    /// Create a new element.
    pub(crate) fn new(kind: ElementKind) -> Self {
        Self {
            key: None,
            kind,
            layout: LayoutConstraints::default(),
            layout_hash_cache: Cell::new(None),
            measure_cache: Cell::new([None, None]),
            split_wrap_probe_cache: Cell::new(None),
        }
    }

    pub(crate) fn clear_caches(&self) {
        self.layout_hash_cache.set(None);
        self.measure_cache.set([None, None]);
        self.split_wrap_probe_cache.set(None);
    }

    /// Assign a stable sibling key used during reconciliation.
    ///
    /// For all multi-child containers, keyed children are matched by key first,
    /// so identity is preserved across reorders and insertions when tags are
    /// compatible.
    pub fn key(mut self, key: impl Into<Key>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Assign a path-independent persistence key for a nested component.
    ///
    /// Unlike [`key`](Self::key), which only disambiguates siblings at the same
    /// resolved container path, `component_state_key` makes the component's
    /// instance (and therefore its state) survive *ancestor* reshaping: wrapping
    /// or unwrapping parent containers, context providers, portals, or other
    /// restructuring that would normally invalidate the path-based reuse.
    ///
    /// Only meaningful on elements produced from [`crate::child`]; a no-op on
    /// other element kinds.
    pub fn component_state_key(mut self, key: impl Into<Key>) -> Self {
        if let ElementKind::Component(component) = &mut self.kind {
            component.state_key = Some(key.into());
        }
        self
    }

    /// Set minimum width constraint. See [`LayoutConstraints::min_w`] for semantics.
    pub fn min_width(mut self, w: Length) -> Self {
        self.layout.min_w = w;
        self.clear_caches();
        self
    }

    /// Set minimum height constraint. See [`LayoutConstraints::min_h`] for semantics.
    pub fn min_height(mut self, h: Length) -> Self {
        self.layout.min_h = h;
        self.clear_caches();
        self
    }

    /// Set maximum width constraint. See [`LayoutConstraints::max_w`] for semantics.
    ///
    /// When the new max is a concrete `Px` value smaller than the current `min_w`,
    /// `min_w` is capped so that `clamp_width` does not override the max.
    pub fn max_width(mut self, w: Length) -> Self {
        self.layout.max_w = Some(w);
        if let (Length::Px(max_px), Length::Px(min_px)) = (w, self.layout.min_w)
            && min_px > max_px
        {
            self.layout.min_w = Length::Px(max_px);
        }
        self.clear_caches();
        self
    }

    /// Set maximum height constraint. See [`LayoutConstraints::max_h`] for semantics.
    ///
    /// When the new max is a concrete `Px` value smaller than the current `min_h`,
    /// `min_h` is capped so that `clamp_height` does not override the max.
    pub fn max_height(mut self, h: Length) -> Self {
        self.layout.max_h = Some(h);
        if let (Length::Px(max_px), Length::Px(min_px)) = (h, self.layout.min_h)
            && min_px > max_px
        {
            self.layout.min_h = Length::Px(max_px);
        }
        self.clear_caches();
        self
    }

    /// Mark whether this element's cross-axis size can change when its
    /// main-axis allocation changes. See [`LayoutConstraints::reflows`].
    pub fn reflows(mut self, reflows: bool) -> Self {
        self.layout.reflows = reflows;
        self.clear_caches();
        self
    }

    /// Set this element's stack shrink priority. See
    /// [`LayoutConstraints::shrink_priority`].
    pub fn shrink_priority(mut self, priority: ShrinkPriority) -> Self {
        self.layout.shrink_priority = priority;
        self.clear_caches();
        self
    }

    pub(crate) fn with_layout(mut self, layout: LayoutConstraints) -> Self {
        self.layout = layout;
        self.clear_caches();
        self
    }

    pub(crate) fn layout_constraints(&self) -> LayoutConstraints {
        match &self.kind {
            ElementKind::Group(group) => group.child.layout_constraints(),
            ElementKind::EffectScope(scope) => scope
                .child
                .as_deref()
                .map(Element::layout_constraints)
                .unwrap_or(self.layout),
            ElementKind::Animated(animated) => {
                if animated.height.is_some() {
                    let mut layout = animated.child.layout_constraints();
                    layout.min_h = self.layout.min_h;
                    layout.max_h = self.layout.max_h;
                    layout.focus_min_h = self.layout.focus_min_h;
                    layout.collapse_h = self.layout.collapse_h;
                    layout.force_compact = false;
                    layout
                } else {
                    animated.child.layout_constraints()
                }
            }
            ElementKind::DragSource(source) => source
                .child
                .as_deref()
                .map(Element::layout_constraints)
                .unwrap_or(self.layout),
            ElementKind::DropTarget(target) => target
                .child
                .as_deref()
                .map(Element::layout_constraints)
                .unwrap_or(self.layout),
            ElementKind::ThemeProvider(tp) => tp.child.layout_constraints(),
            ElementKind::ContextProvider(cp) => cp.child.layout_constraints(),
            ElementKind::Memo(_) => self.layout,
            _ => self.layout,
        }
    }

    pub(crate) fn contains_unexpanded_component(&self) -> bool {
        matches!(self.kind, ElementKind::Component(_))
            || self
                .kind
                .children()
                .into_iter()
                .any(Self::contains_unexpanded_component)
    }

    /// Replace the child of the `Group` with the matching scope, clearing all
    /// cached layout/measurement state on the replacement path.
    pub(crate) fn replace_group_child_by_scope(
        &mut self,
        scope: ScopeId,
        replacement: &mut Option<Element>,
    ) -> bool {
        if let ElementKind::Group(group) = &mut self.kind
            && group.scope == scope
        {
            if let Some(replacement) = replacement.take() {
                *group.child = replacement;
                self.clear_caches();
                return true;
            }
            return false;
        }

        let replaced = self
            .kind
            .children_mut()
            .iter_mut()
            .any(|child| child.replace_group_child_by_scope(scope, replacement));
        if replaced {
            self.clear_caches();
        }
        replaced
    }
}

/// Concrete element kinds.
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ElementKind {
    /// Static or dynamic text.
    Text(Text),
    /// ASCII art text.
    #[cfg(feature = "big-text")]
    BigText(BigText),
    /// ASCII canvas grid (also handles frame sequences).
    AsciiCanvas(AsciiCanvas),

    /// Clickable, focusable button.
    Button(Box<Button>),
    /// Single-line text input.
    Input(Box<Input>),
    /// Protocol-aware image widget.
    #[cfg(feature = "image")]
    Image(Image),
    /// Vertically scrolling list.
    List(Box<List>),
    /// Multi-line text input.
    TextArea(Box<crate::widgets::TextArea>),
    /// Hex/ASCII binary data viewer.
    HexArea(Box<HexArea>),
    /// Terminal viewport.
    #[cfg(feature = "terminal")]
    Terminal(Terminal),
    /// Popover overlay.
    Popover(crate::widgets::Popover),
    Portal(Portal),
    /// Table with columns and rows.
    Table(Box<crate::widgets::Table>),
    /// Tab bar.
    Tabs(Tabs),
    /// Draggable tab bar.
    DraggableTabBar(Box<DraggableTabBar>),
    /// Nested component.
    Component(crate::core::nested::ComponentElement),
    /// Internal layout-transparent wrapper.
    Group(Group),
    /// Subtree color/effect post-processing wrapper.
    EffectScope(EffectScope),
    /// Animated wrapper (opacity + height transitions).
    Animated(Animated),
    /// Drag source wrapper.
    DragSource(DragSource),
    /// Drop target wrapper.
    DropTarget(DropTarget),
    /// Pointer-interaction region wrapper.
    MouseRegion(MouseRegion),
    /// Scrollable vertical container.
    ScrollView(Box<ScrollView>),
    /// Two-dimensional pan viewport.
    PanView(PanView),
    /// Vertical stack layout.
    VStack(VStack),
    /// Horizontal stack layout.
    HStack(HStack),
    /// CSS-like explicit grid layout.
    Grid(Grid),
    /// Horizontal wrapping flow layout.
    Flow(Flow),
    /// Absolute-positioned child layout.
    Canvas(Canvas),
    /// Mermaid-style flowchart visualization.
    Flowchart(Box<Flowchart>),
    /// Overlay stack layout.
    ZStack(ZStack),
    /// Centering helper for overlays/modals.
    Center(Center),
    /// Center-pinned layout with collision-aware top/bottom zones.
    CenterPin(CenterPin),
    /// Framed container with optional title.
    Frame(Frame),
    /// Divider line.
    Divider(Divider),
    /// Flexible empty space.
    Spacer(Spacer),
    /// Sparkline chart.
    Sparkline(crate::widgets::Sparkline),
    /// Multi-series chart.
    Chart(Box<Chart>),
    /// Node-edge graph visualization.
    Graph(Box<Graph>),
    /// UML sequence diagram visualization.
    SequenceDiagram(Box<SequenceDiagram>),
    /// UML class diagram visualization.
    ClassDiagram(Box<ClassDiagram>),
    /// UML state diagram visualization.
    StateDiagram(Box<StateDiagram>),
    /// Entity-relationship diagram visualization.
    ErDiagram(Box<ErDiagram>),
    /// Gantt timeline diagram visualization.
    GanttDiagram(Box<GanttDiagram>),
    /// Internal status bar layout container.
    StatusBarLayout(StatusBarLayout),
    /// Heatmap visualization.
    Heatmap(Heatmap),
    /// Checkbox toggle.
    Checkbox(Checkbox),
    /// Progress bar.
    ProgressBar(ProgressBar),
    /// Slider.
    Slider(crate::widgets::Slider),
    /// Loading spinner.
    Spinner(Spinner),
    /// Resizable splitter container.
    Splitter(Splitter),
    /// Read-only rich text document viewer.
    DocumentView(Box<DocumentView>),
    /// Deferred theme application wrapper (consumed during expansion).
    ThemeProvider(Box<ThemeProviderElement>),
    /// Deferred typed context provider (consumed during expansion).
    ContextProvider(Box<ContextProviderElement>),
    /// In-view memoized subtree wrapper.
    Memo(MemoElement),
}

/// Generate the `dimensions()` match arms from the widget manifest.
macro_rules! impl_element_dimensions {
    (
        @direct [ $($v:ident,)* ]
        @direct_gated [ $($gv:ident => $gf:literal,)* ]
        @direct_no_hash [ $($dnh:ident,)* ]
        @direct_no_hash_gated [ $($dnhg:ident => $dnhgf:literal,)* ]
        @props_dims [ $($pd:ident,)* ]
        @const_auto_hash [ $($cah:ident,)* ]
        @const_auto_hash_gated [ $($cahg:ident => $cahgf:literal,)* ]
        @const_flex [ $($cf:ident,)* ]
        @const_flex_no_hash [ $($cfnh:ident,)* ]
        @no_dims [ $($nd:ident,)* ]
        @element_only_const_auto [ $($eo:ident,)* ]
    ) => {
        impl ElementKind {
            /// Returns the `(width, height)` dimensions for this element kind, if applicable.
            ///
            /// Returns `None` for variants that need recursive delegation (Group,
            /// MouseRegion, Popover) or special per-axis handling (Frame).
            pub(crate) fn dimensions(&self) -> Option<(Length, Length)> {
                match self {
                    // Standard widgets with direct width/height fields
                    $( Self::$v(w) => Some((w.width, w.height)), )*
                    $( Self::$dnh(w) => Some((w.width, w.height)), )*
                    $( #[cfg(feature = $gf)] Self::$gv(w) => Some((w.width, w.height)), )*
                    $( #[cfg(feature = $dnhgf)] Self::$dnhg(w) => Some((w.width, w.height)), )*

                    // Containers with props struct
                    $( Self::$pd(w) => Some((w.props.width, w.props.height)), )*

                    // Always-constant dimensions: Auto
                    $( Self::$cah(_) )|* $( | Self::$eo(_) )* => {
                        Some((Length::Auto, Length::Auto))
                    }
                    $( #[cfg(feature = $cahgf)] Self::$cahg(_) => {
                        Some((Length::Auto, Length::Auto))
                    } )*

                    // Always-constant dimensions: Flex(1)
                    $( Self::$cf(_) )|* $( | Self::$cfnh(_) )* => {
                        Some((Length::Flex(1), Length::Flex(1)))
                    }

                    // Special cases: delegation or per-axis logic - handled by caller
                    $( Self::$nd(_) )|* => None,
                }
            }
        }
    };
}

for_all_widget_variants!(impl_element_dimensions);

impl ElementKind {
    /// Returns references to all `Element` children of this variant.
    ///
    /// Leaf widgets and `Component` (which is expanded before layout) return
    /// an empty collection. Container variants return children in a consistent
    /// order: for named slots (e.g. Frame header/child, CenterPin top/center/bottom),
    /// only `Some` slots are included.
    pub(crate) fn children(&self) -> SmallVec<[&Element; 4]> {
        match self {
            Self::Group(g) => SmallVec::from_buf_and_len([g.child.as_ref(); 4], 1),
            Self::ThemeProvider(tp) => SmallVec::from_buf_and_len([&tp.child; 4], 1),
            Self::ContextProvider(cp) => SmallVec::from_buf_and_len([&cp.child; 4], 1),
            Self::Portal(p) => SmallVec::from_buf_and_len([p.content.as_ref(); 4], 1),
            Self::Popover(p) => {
                smallvec::smallvec![p.trigger.as_ref(), p.content.as_ref()]
            }
            Self::EffectScope(e) => match &e.child {
                Some(c) => SmallVec::from_buf_and_len([c.as_ref(); 4], 1),
                None => SmallVec::new(),
            },
            Self::Animated(a) => SmallVec::from_buf_and_len([a.child.as_ref(); 4], 1),
            Self::DragSource(ds) => match &ds.child {
                Some(c) => SmallVec::from_buf_and_len([c.as_ref(); 4], 1),
                None => SmallVec::new(),
            },
            Self::DropTarget(dt) => match &dt.child {
                Some(c) => SmallVec::from_buf_and_len([c.as_ref(); 4], 1),
                None => SmallVec::new(),
            },
            Self::MouseRegion(m) => match &m.child {
                Some(c) => SmallVec::from_buf_and_len([c.as_ref(); 4], 1),
                None => SmallVec::new(),
            },
            Self::Center(c) => match &c.child {
                Some(c) => SmallVec::from_buf_and_len([c.as_ref(); 4], 1),
                None => SmallVec::new(),
            },
            Self::CenterPin(cp) => {
                let mut out = SmallVec::new();
                if let Some(c) = &cp.top {
                    out.push(c.as_ref());
                }
                if let Some(c) = &cp.center {
                    out.push(c.as_ref());
                }
                if let Some(c) = &cp.bottom {
                    out.push(c.as_ref());
                }
                out
            }
            Self::StatusBarLayout(layout) => smallvec::smallvec![
                layout.left.as_ref(),
                layout.center.as_ref(),
                layout.right.as_ref()
            ],
            Self::Frame(f) => {
                let mut out = SmallVec::new();
                if let Some(c) = &f.header {
                    out.push(c.as_ref());
                }
                if let Some(c) = &f.child {
                    out.push(c.as_ref());
                }
                out
            }
            Self::Divider(d) => match &d.label {
                Some(c) => SmallVec::from_buf_and_len([c.as_ref(); 4], 1),
                None => SmallVec::new(),
            },
            Self::ScrollView(sv) => sv.children.iter().collect(),
            Self::PanView(pan) => match &pan.child {
                Some(c) => SmallVec::from_buf_and_len([c.as_ref(); 4], 1),
                None => SmallVec::new(),
            },
            Self::VStack(v) => v.children.iter().collect(),
            Self::HStack(h) => h.children.iter().collect(),
            Self::Grid(g) => g.items.iter().map(|i| &i.element).collect(),
            Self::Flow(f) => f.children.iter().collect(),
            Self::Canvas(c) => c.items.iter().map(|i| &i.element).collect(),
            Self::ZStack(z) => z.children.iter().collect(),
            Self::Splitter(sp) => sp.children.iter().collect(),
            _ => SmallVec::new(),
        }
    }

    /// Returns mutable references to all `Element` children of this variant.
    ///
    /// Same semantics as [`children`](Self::children) but with `&mut` access.
    pub(crate) fn children_mut(&mut self) -> SmallVec<[&mut Element; 4]> {
        match self {
            Self::Group(g) => smallvec::smallvec![g.child.as_mut()],
            Self::ThemeProvider(tp) => smallvec::smallvec![&mut tp.child],
            Self::ContextProvider(cp) => smallvec::smallvec![&mut cp.child],
            Self::Portal(p) => smallvec::smallvec![p.content.as_mut()],
            Self::Popover(p) => {
                smallvec::smallvec![p.trigger.as_mut(), p.content.as_mut()]
            }
            Self::EffectScope(e) => match &mut e.child {
                Some(c) => smallvec::smallvec![c.as_mut()],
                None => SmallVec::new(),
            },
            Self::Animated(a) => smallvec::smallvec![a.child.as_mut()],
            Self::DragSource(ds) => match &mut ds.child {
                Some(c) => smallvec::smallvec![c.as_mut()],
                None => SmallVec::new(),
            },
            Self::DropTarget(dt) => match &mut dt.child {
                Some(c) => smallvec::smallvec![c.as_mut()],
                None => SmallVec::new(),
            },
            Self::MouseRegion(m) => match &mut m.child {
                Some(c) => smallvec::smallvec![c.as_mut()],
                None => SmallVec::new(),
            },
            Self::Center(c) => match &mut c.child {
                Some(c) => smallvec::smallvec![c.as_mut()],
                None => SmallVec::new(),
            },
            Self::CenterPin(cp) => {
                let mut out = SmallVec::new();
                if let Some(c) = &mut cp.top {
                    out.push(c.as_mut());
                }
                if let Some(c) = &mut cp.center {
                    out.push(c.as_mut());
                }
                if let Some(c) = &mut cp.bottom {
                    out.push(c.as_mut());
                }
                out
            }
            Self::StatusBarLayout(layout) => smallvec::smallvec![
                layout.left.as_mut(),
                layout.center.as_mut(),
                layout.right.as_mut()
            ],
            Self::Frame(f) => {
                let mut out = SmallVec::new();
                if let Some(c) = &mut f.header {
                    out.push(c.as_mut());
                }
                if let Some(c) = &mut f.child {
                    out.push(c.as_mut());
                }
                out
            }
            Self::Divider(d) => match &mut d.label {
                Some(c) => smallvec::smallvec![c.as_mut()],
                None => SmallVec::new(),
            },
            Self::ScrollView(sv) => sv.children.iter_mut().collect(),
            Self::PanView(pan) => match &mut pan.child {
                Some(c) => smallvec::smallvec![c.as_mut()],
                None => SmallVec::new(),
            },
            Self::VStack(v) => v.children.iter_mut().collect(),
            Self::HStack(h) => h.children.iter_mut().collect(),
            Self::Grid(g) => g.items.iter_mut().map(|i| &mut i.element).collect(),
            Self::Flow(f) => f.children.iter_mut().collect(),
            Self::Canvas(c) => c.items.iter_mut().map(|i| &mut i.element).collect(),
            Self::ZStack(z) => z.children.iter_mut().collect(),
            Self::Splitter(sp) => sp.children.iter_mut().collect(),
            _ => SmallVec::new(),
        }
    }
}

/// A deferred theme application wrapper, consumed during component expansion.
#[derive(Clone)]
pub(crate) struct ThemeProviderElement {
    pub theme: Theme,
    pub child: Element,
}

/// A deferred typed context provider wrapper, consumed during expansion.
#[derive(Clone)]
pub(crate) struct ContextProviderElement {
    pub type_id: TypeId,
    pub value: Arc<dyn Any>,
    pub equals: fn(&dyn Any, &dyn Any) -> bool,
    pub generation: u64,
    pub child: Element,
}

impl ContextProviderElement {
    pub(crate) fn new<T>(value: T, child: Element) -> Self
    where
        T: Clone + PartialEq + 'static,
    {
        Self {
            type_id: TypeId::of::<T>(),
            value: Arc::new(value),
            equals: context_value_equals::<T>,
            generation: 1,
            child,
        }
    }
}

fn context_value_equals<T: PartialEq + 'static>(a: &dyn Any, b: &dyn Any) -> bool {
    a.downcast_ref::<T>() == b.downcast_ref::<T>()
}

/// A layout-transparent wrapper node used to preserve scoping boundaries.
#[derive(Clone)]
pub(crate) struct Group {
    pub scope: ScopeId,
    pub child: Box<Element>,
}

impl crate::layout::hash::LayoutHash for Group {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        recurse(self.child.as_ref())?.hash(hasher);
        Some(())
    }
}

/// Extension trait for ergonomically building `Element`s.
///
/// These methods consume the widget and return `Element`, not the original
/// widget type, so they must always be the **last calls** in a builder chain.
/// Placing them before widget-specific methods causes a compile error because
/// `Element` does not expose widget setters.
///
/// If a constraint is needed mid-chain, add it as a dedicated builder method
/// on that widget (see `Toast::max_width` as an example).
pub trait IntoElement: Into<Element> + Sized {
    /// Convert into an element and assign a stable sibling key used during
    /// multi-child container reconciliation.
    fn key(self, key: impl Into<Key>) -> Element {
        self.into().key(key)
    }

    /// Convert into an element and assign a path-independent component state
    /// key. See [`Element::component_state_key`].
    fn component_state_key(self, key: impl Into<Key>) -> Element {
        self.into().component_state_key(key)
    }

    /// Set minimum width constraint. See [`LayoutConstraints::min_w`] for semantics.
    fn min_width(self, w: Length) -> Element {
        self.into().min_width(w)
    }

    /// Set minimum height constraint. See [`LayoutConstraints::min_h`] for semantics.
    fn min_height(self, h: Length) -> Element {
        self.into().min_height(h)
    }

    /// Set maximum width constraint. See [`LayoutConstraints::max_w`] for semantics.
    fn max_width(self, w: Length) -> Element {
        self.into().max_width(w)
    }

    /// Set maximum height constraint. See [`LayoutConstraints::max_h`] for semantics.
    fn max_height(self, h: Length) -> Element {
        self.into().max_height(h)
    }

    /// Mark whether this element reflows when its main-axis allocation changes.
    fn reflows(self, reflows: bool) -> Element {
        self.into().reflows(reflows)
    }

    /// Set this element's stack shrink priority.
    fn shrink_priority(self, priority: ShrinkPriority) -> Element {
        self.into().shrink_priority(priority)
    }
}

impl<T> IntoElement for T where T: Into<Element> + Sized {}

impl Default for Element {
    /// Creates a zero-cost placeholder element (empty [`Spacer`]).
    ///
    /// This is primarily used by [`std::mem::take`] inside theme application
    /// to avoid deep-cloning children.
    fn default() -> Self {
        Element::new(ElementKind::Spacer(Spacer::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::{Element, ElementKind, Group, MeasureCacheEntry};
    use crate::callback::ScopeId;
    use crate::core::memo::Memo;
    use crate::overlay::{DismissPolicy, OverlayLayer, OverlayPlacement, PointerCapture, Portal};
    use crate::style::Theme;
    use crate::widgets::{ContextProvider, Divider, Grid, Splitter, Text, ThemeProvider};

    const TARGET_SCOPE: ScopeId = ScopeId(42);

    fn scoped_group(label: &str) -> Element {
        Element::new(ElementKind::Group(Group {
            scope: TARGET_SCOPE,
            child: Box::new(Text::new(label).into()),
        }))
    }

    fn replacement() -> Element {
        Text::new("new").into()
    }

    fn mark_caches(element: &Element) {
        element.layout_hash_cache.set(Some(99));
        element.measure_cache.set([
            Some(MeasureCacheEntry {
                max_w: Some(1),
                max_h: Some(1),
                size: (1, 1),
            }),
            None,
        ]);
        element.split_wrap_probe_cache.set(Some(true));
    }

    fn text_contents(element: &Element, out: &mut Vec<String>) {
        if let ElementKind::Text(text) = &element.kind {
            out.push(text.plain_content());
        }
        for child in element.kind.children() {
            text_contents(child, out);
        }
    }

    fn group_for_scope(element: &Element, scope: ScopeId) -> Option<&Element> {
        if let ElementKind::Group(group) = &element.kind
            && group.scope == scope
        {
            return Some(element);
        }
        element
            .kind
            .children()
            .into_iter()
            .find_map(|child| group_for_scope(child, scope))
    }

    fn assert_replaces_through_wrapper(mut root: Element) {
        mark_caches(&root);
        let group = group_for_scope(&root, TARGET_SCOPE).expect("target group before replace");
        mark_caches(group);

        let mut replacement = Some(replacement());
        assert!(
            root.replace_group_child_by_scope(TARGET_SCOPE, &mut replacement),
            "scope replacement should succeed"
        );
        assert!(replacement.is_none());

        let mut texts = Vec::new();
        text_contents(&root, &mut texts);
        assert!(texts.contains(&"new".to_string()), "texts: {texts:?}");
        assert!(!texts.contains(&"old".to_string()), "texts: {texts:?}");
        assert_eq!(root.layout_hash_cache.get(), None);
        assert_eq!(root.measure_cache.get(), [None, None]);
        assert_eq!(root.split_wrap_probe_cache.get(), None);

        let group = group_for_scope(&root, TARGET_SCOPE).expect("target group after replace");
        assert_eq!(group.layout_hash_cache.get(), None);
        assert_eq!(group.measure_cache.get(), [None, None]);
        assert_eq!(group.split_wrap_probe_cache.get(), None);
    }

    #[test]
    fn replace_group_child_traverses_children_mut_wrappers() {
        let portal = Element::new(ElementKind::Portal(Portal {
            layer: OverlayLayer::Modal,
            content: Box::new(scoped_group("old")),
            placement: OverlayPlacement::Center,
            dismiss_policy: DismissPolicy::None,
            on_close: None,
            backdrop: None,
            captures_focus: false,
            captures_pointer: PointerCapture::None,
        }));

        let cases: Vec<(&str, Element)> = vec![
            ("grid", Grid::new().child(scoped_group("old")).into()),
            (
                "splitter",
                Splitter::vertical().child(scoped_group("old")).into(),
            ),
            (
                "divider_label",
                Divider::horizontal().label(scoped_group("old")).into(),
            ),
            (
                "theme_provider",
                ThemeProvider::new(Theme::default())
                    .child(scoped_group("old"))
                    .into(),
            ),
            (
                "context_provider",
                ContextProvider::new(7u32).child(scoped_group("old")).into(),
            ),
            ("portal", portal),
        ];

        for (_name, root) in cases {
            assert_replaces_through_wrapper(root);
        }
    }

    #[test]
    fn replace_group_child_does_not_expand_unresolved_memo_builders() {
        let mut root = Memo::with_call_site(1, 1).build(|| scoped_group("old"));
        let mut replacement = Some(replacement());
        assert!(!root.replace_group_child_by_scope(TARGET_SCOPE, &mut replacement));

        let mut texts = Vec::new();
        text_contents(
            &replacement.expect("unexpanded memo keeps replacement"),
            &mut texts,
        );
        assert_eq!(texts, ["new".to_string()]);
    }
}
