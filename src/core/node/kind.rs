use super::id::NodeId;
use super::overlay::ScrollbarZone;
use crate::callback::{Callback, ScopeId};
use crate::style::{Rect, Theme};
#[cfg(feature = "big-text")]
use crate::widgets::internal::BigTextNode;
#[cfg(feature = "terminal")]
use crate::widgets::internal::TerminalNode;

#[cfg(feature = "image")]
use crate::widgets::Image;
use crate::widgets::document_view::node::DocumentViewNode;
#[cfg(feature = "image")]
use crate::widgets::internal::ImageNode;
use crate::widgets::internal::{
    AnimatedNode, AsciiCanvasNode, CanvasNode, CenterPinNode, ChartNode, CheckboxNode,
    ClassDiagramNode, DragSourceNode, DraggableTabBarNode, DropTargetNode, EffectScopeNode,
    ErDiagramNode, FlowNode, FlowchartNode, FrameNode, GanttDiagramNode, GraphRenderNode, GridNode,
    HeatmapNode, HexAreaNode, MouseRegionNode, PanViewNode, ProgressNode, ScrollViewNode,
    SequenceDiagramNode, SliderNode, SparklineNode, SpinnerNode, SplitterNode, StackNode,
    StateDiagramNode, StatusBarLayoutNode, TableNode, TabsNode, ZStackNode,
};
use crate::widgets::{
    Animated, AsciiCanvas, Button, Chart, Checkbox, ClassDiagram, DragSource, DraggableTabBar,
    DropTarget, EffectScope, ErDiagram, Flowchart, FocusScope, GanttDiagram, Graph, Heatmap,
    HexArea, Input, MouseRegion, PanView, ProgressBar, SequenceDiagram, Slider, Sparkline, Spinner,
    Splitter, StateDiagram, Table, Tabs, TextArea,
};

pub(crate) trait WidgetNode {
    fn focus_scope(&self) -> FocusScope {
        FocusScope::None
    }
    fn is_focusable(&self) -> bool {
        false
    }
    fn is_tab_stop(&self) -> bool {
        self.is_focusable()
    }
    fn on_focus_callback(&self) -> Option<&Callback<()>> {
        None
    }
    fn on_blur_callback(&self) -> Option<&Callback<()>> {
        None
    }
    fn has_on_click(&self) -> bool {
        false
    }
    fn has_on_mouse_move(&self) -> bool {
        false
    }
    fn is_hoverable(&self) -> bool {
        self.has_on_click()
    }
    fn is_hoverable_for_theme(&self, _theme: &Theme) -> bool {
        self.is_hoverable()
    }
    /// Whether a hover enter/leave transition on this node changes what is painted.
    ///
    /// Hover tracking runs for anything [`Self::is_hoverable_for_theme`] accepts, which
    /// includes nodes that are hoverable only because they take clicks. Those have no
    /// hover-dependent visuals, so a transition must not force a repaint on its own —
    /// the `Update` returned from the node's hover callback decides.
    fn hover_affects_paint(&self, theme: &Theme) -> bool {
        self.is_hoverable_for_theme(theme)
    }
    /// Refine hit-testing. Returns Some(true/false) to override default interactive check.
    fn hit_test_refinement(&self, _x: i16, _y: i16, _rect: Rect) -> Option<bool> {
        None
    }
    /// Refine hover-testing. Defaults to the hit-test refinement for widgets whose
    /// pointer target shape is the same for clicks and hover.
    fn hover_test_refinement(&self, x: i16, y: i16, rect: Rect) -> Option<bool> {
        self.hit_test_refinement(x, y, rect)
    }
    fn scrollbar_zones(
        &self,
        _id: NodeId,
        _rect: Rect,
        _parent_border_x: Option<i16>,
        _parent_border_y: Option<i16>,
    ) -> Vec<ScrollbarZone> {
        Vec::new()
    }
}

#[derive(Clone)]
pub(crate) struct GroupNode {
    pub scope: ScopeId,
}

impl WidgetNode for GroupNode {}

macro_rules! node_kind_delegate_match {
    ($self:expr, $method:ident($($arg:expr),* $(,)?)) => {
        match $self {
            // BEGIN GENERATED: node_kind_delegate_match arms
            Self::Text(n) => n.$method($($arg),*),
            Self::AsciiCanvas(n) => n.$method($($arg),*),
            Self::Button(n) => n.$method($($arg),*),
            Self::Input(n) => n.$method($($arg),*),
            Self::HexArea(n) => n.$method($($arg),*),
            Self::List(n) => n.$method($($arg),*),
            Self::TextArea(n) => n.$method($($arg),*),
            Self::Table(n) => n.$method($($arg),*),
            Self::Tabs(n) => n.$method($($arg),*),
            Self::DraggableTabBar(n) => n.$method($($arg),*),
            Self::Divider(n) => n.$method($($arg),*),
            Self::Spacer(n) => n.$method($($arg),*),
            Self::Checkbox(n) => n.$method($($arg),*),
            Self::ProgressBar(n) => n.$method($($arg),*),
            Self::Slider(n) => n.$method($($arg),*),
            Self::Spinner(n) => n.$method($($arg),*),
            Self::Splitter(n) => n.$method($($arg),*),
            Self::Heatmap(n) => n.$method($($arg),*),
            Self::DocumentView(n) => n.$method($($arg),*),
            Self::PanView(n) => n.$method($($arg),*),
            Self::Flow(n) => n.$method($($arg),*),
            Self::Canvas(n) => n.$method($($arg),*),
            #[cfg(feature = "image")]
            Self::Image(n) => n.$method($($arg),*),
            Self::Sparkline(n) => n.$method($($arg),*),
            Self::Chart(n) => n.$method($($arg),*),
            Self::Graph(n) => n.$method($($arg),*),
            Self::SequenceDiagram(n) => n.$method($($arg),*),
            Self::Flowchart(n) => n.$method($($arg),*),
            Self::ClassDiagram(n) => n.$method($($arg),*),
            Self::StateDiagram(n) => n.$method($($arg),*),
            Self::ErDiagram(n) => n.$method($($arg),*),
            Self::GanttDiagram(n) => n.$method($($arg),*),
            Self::StatusBarLayout(n) => n.$method($($arg),*),
            #[cfg(feature = "terminal")]
            Self::Terminal(n) => n.$method($($arg),*),
            Self::VStack(n) => n.$method($($arg),*),
            Self::HStack(n) => n.$method($($arg),*),
            Self::ScrollView(n) => n.$method($($arg),*),
            Self::Grid(n) => n.$method($($arg),*),
            Self::Portal(n) => n.$method($($arg),*),
            #[cfg(feature = "big-text")]
            Self::BigText(n) => n.$method($($arg),*),
            Self::ZStack(n) => n.$method($($arg),*),
            Self::Center(n) => n.$method($($arg),*),
            Self::CenterPin(n) => n.$method($($arg),*),
            Self::Animated(n) => n.$method($($arg),*),
            Self::DragSource(n) => n.$method($($arg),*),
            Self::DropTarget(n) => n.$method($($arg),*),
            Self::EffectScope(n) => n.$method($($arg),*),
            Self::Frame(n) => n.$method($($arg),*),
            Self::Group(n) => n.$method($($arg),*),
            Self::MouseRegion(n) => n.$method($($arg),*),
            Self::Popover(n) => n.$method($($arg),*),
            // END GENERATED: node_kind_delegate_match arms
        }
    };
}

/// A node kind.
#[derive(Clone)]
pub(crate) enum NodeKind {
    /// Text node.
    Text(crate::widgets::internal::TextNode),
    /// Big text node.
    #[cfg(feature = "big-text")]
    BigText(BigTextNode),

    /// ASCII canvas node (also handles frame sequences).
    AsciiCanvas(AsciiCanvasNode),

    /// Button node.
    Button(crate::widgets::internal::ButtonNode),

    /// Input node.
    Input(crate::widgets::internal::InputNode),

    /// Image node.
    #[cfg(feature = "image")]
    Image(ImageNode),

    /// Text area node.
    TextArea(Box<crate::widgets::internal::TextAreaNode>),

    /// Hex area node.
    HexArea(HexAreaNode),

    /// Terminal viewport node.
    #[cfg(feature = "terminal")]
    Terminal(TerminalNode),

    /// Popover node.
    Popover(crate::widgets::internal::PopoverNode),

    /// List node.
    List(crate::widgets::internal::ListNode),

    /// Table node.
    Table(TableNode),

    /// Tabs node.
    Tabs(TabsNode),

    /// Draggable tab bar node.
    DraggableTabBar(DraggableTabBarNode),

    /// Sparkline node.
    Sparkline(SparklineNode),

    /// Chart node.
    Chart(ChartNode),

    /// Graph node.
    Graph(GraphRenderNode),

    /// Sequence diagram node.
    SequenceDiagram(SequenceDiagramNode),

    /// Flowchart node.
    Flowchart(Box<FlowchartNode>),

    /// Class diagram node.
    ClassDiagram(Box<ClassDiagramNode>),

    /// State diagram node.
    StateDiagram(Box<StateDiagramNode>),

    /// Entity-relationship diagram node.
    ErDiagram(Box<ErDiagramNode>),

    /// Gantt timeline diagram node.
    GanttDiagram(Box<GanttDiagramNode>),

    /// Internal status bar layout node.
    StatusBarLayout(StatusBarLayoutNode),

    /// Heatmap node.
    Heatmap(HeatmapNode),

    /// Layout-transparent wrapper node.
    Group(GroupNode),

    /// Subtree effect wrapper node.
    EffectScope(EffectScopeNode),

    /// Animated wrapper node.
    Animated(AnimatedNode),

    /// Drag source wrapper node.
    DragSource(DragSourceNode),

    /// Drop target wrapper node.
    DropTarget(DropTargetNode),

    /// Pointer-interaction region node.
    MouseRegion(MouseRegionNode),

    /// Scroll view node.
    ScrollView(ScrollViewNode),

    /// Two-dimensional pan viewport node.
    PanView(PanViewNode),

    /// Vertical stack container.
    VStack(StackNode),

    /// Horizontal stack container.
    HStack(StackNode),

    /// Grid layout container.
    Grid(GridNode),

    /// Horizontal wrapping flow container.
    Flow(FlowNode),

    /// Absolute-positioned child container.
    Canvas(CanvasNode),

    /// Overlay stack container.
    ZStack(ZStackNode),

    /// Centering helper.
    Center(crate::widgets::internal::CenterNode),

    /// Center-pinned layout.
    CenterPin(CenterPinNode),

    /// Frame container.
    Frame(FrameNode),

    /// Divider node.
    Divider(crate::widgets::internal::DividerNode),

    /// Spacer node.
    Spacer(crate::widgets::internal::SpacerNode),

    /// Checkbox node.
    Checkbox(CheckboxNode),

    /// Progress bar node.
    ProgressBar(ProgressNode),

    /// Spinner node.
    Spinner(SpinnerNode),

    /// Slider node.
    Slider(SliderNode),

    /// Splitter node.
    Splitter(SplitterNode),

    /// Document view node.
    DocumentView(Box<DocumentViewNode>),

    Portal(crate::overlay::PortalNode),
}

impl WidgetNode for NodeKind {
    fn focus_scope(&self) -> FocusScope {
        node_kind_delegate_match!(self, focus_scope())
    }

    fn is_focusable(&self) -> bool {
        node_kind_delegate_match!(self, is_focusable())
    }

    fn is_tab_stop(&self) -> bool {
        node_kind_delegate_match!(self, is_tab_stop())
    }

    fn on_focus_callback(&self) -> Option<&Callback<()>> {
        node_kind_delegate_match!(self, on_focus_callback())
    }

    fn on_blur_callback(&self) -> Option<&Callback<()>> {
        node_kind_delegate_match!(self, on_blur_callback())
    }

    fn has_on_click(&self) -> bool {
        node_kind_delegate_match!(self, has_on_click())
    }

    fn has_on_mouse_move(&self) -> bool {
        node_kind_delegate_match!(self, has_on_mouse_move())
    }

    fn is_hoverable(&self) -> bool {
        node_kind_delegate_match!(self, is_hoverable())
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        node_kind_delegate_match!(self, is_hoverable_for_theme(theme))
    }

    fn hover_affects_paint(&self, theme: &Theme) -> bool {
        node_kind_delegate_match!(self, hover_affects_paint(theme))
    }

    fn hit_test_refinement(&self, x: i16, y: i16, rect: Rect) -> Option<bool> {
        node_kind_delegate_match!(self, hit_test_refinement(x, y, rect))
    }

    fn hover_test_refinement(&self, x: i16, y: i16, rect: Rect) -> Option<bool> {
        node_kind_delegate_match!(self, hover_test_refinement(x, y, rect))
    }

    fn scrollbar_zones(
        &self,
        id: NodeId,
        rect: Rect,
        parent_border_x: Option<i16>,
        parent_border_y: Option<i16>,
    ) -> Vec<ScrollbarZone> {
        node_kind_delegate_match!(
            self,
            scrollbar_zones(id, rect, parent_border_x, parent_border_y)
        )
    }
}

impl From<Button> for NodeKind {
    fn from(value: Button) -> Self {
        NodeKind::Button(crate::widgets::internal::ButtonNode::from(value))
    }
}

impl From<Input> for NodeKind {
    fn from(value: Input) -> Self {
        NodeKind::Input(crate::widgets::internal::reconcile_input(&value))
    }
}

#[cfg(feature = "image")]
impl From<Image> for NodeKind {
    fn from(value: Image) -> Self {
        NodeKind::Image(crate::widgets::internal::ImageNode::from(value))
    }
}

impl From<TextArea> for NodeKind {
    fn from(value: TextArea) -> Self {
        NodeKind::TextArea(Box::new(crate::widgets::internal::TextAreaNode::from(
            value,
        )))
    }
}

impl From<HexArea> for NodeKind {
    fn from(value: HexArea) -> Self {
        NodeKind::HexArea(HexAreaNode::from(value))
    }
}

impl From<Table> for NodeKind {
    fn from(value: Table) -> Self {
        NodeKind::Table(crate::widgets::internal::TableNode::from(value))
    }
}

impl From<Tabs> for NodeKind {
    fn from(value: Tabs) -> Self {
        NodeKind::Tabs(TabsNode::from(value))
    }
}

impl From<DraggableTabBar> for NodeKind {
    fn from(value: DraggableTabBar) -> Self {
        NodeKind::DraggableTabBar(DraggableTabBarNode::from(value))
    }
}

impl From<Sparkline> for NodeKind {
    fn from(value: Sparkline) -> Self {
        NodeKind::Sparkline(SparklineNode::from(value))
    }
}

impl From<Chart> for NodeKind {
    fn from(value: Chart) -> Self {
        NodeKind::Chart(ChartNode::from(value))
    }
}

impl From<Graph> for NodeKind {
    fn from(value: Graph) -> Self {
        NodeKind::Graph(GraphRenderNode::from(value))
    }
}

impl From<SequenceDiagram> for NodeKind {
    fn from(value: SequenceDiagram) -> Self {
        NodeKind::SequenceDiagram(SequenceDiagramNode::from(value))
    }
}

impl From<Flowchart> for NodeKind {
    fn from(value: Flowchart) -> Self {
        NodeKind::Flowchart(Box::new(FlowchartNode::from(value)))
    }
}

impl From<ClassDiagram> for NodeKind {
    fn from(value: ClassDiagram) -> Self {
        NodeKind::ClassDiagram(Box::new(ClassDiagramNode::from(value)))
    }
}

impl From<StateDiagram> for NodeKind {
    fn from(value: StateDiagram) -> Self {
        NodeKind::StateDiagram(Box::new(StateDiagramNode::from(value)))
    }
}

impl From<ErDiagram> for NodeKind {
    fn from(value: ErDiagram) -> Self {
        NodeKind::ErDiagram(Box::new(ErDiagramNode::from(value)))
    }
}

impl From<GanttDiagram> for NodeKind {
    fn from(value: GanttDiagram) -> Self {
        NodeKind::GanttDiagram(Box::new(GanttDiagramNode::from(value)))
    }
}

impl From<Heatmap> for NodeKind {
    fn from(value: Heatmap) -> Self {
        NodeKind::Heatmap(HeatmapNode::from(value))
    }
}

impl From<Checkbox> for NodeKind {
    fn from(value: Checkbox) -> Self {
        NodeKind::Checkbox(crate::widgets::internal::CheckboxNode::from(value))
    }
}

impl From<ProgressBar> for NodeKind {
    fn from(value: ProgressBar) -> Self {
        NodeKind::ProgressBar(crate::widgets::internal::ProgressNode::from(value))
    }
}

impl From<Spinner> for NodeKind {
    fn from(value: Spinner) -> Self {
        NodeKind::Spinner(crate::widgets::internal::SpinnerNode::from(value))
    }
}

impl From<Slider> for NodeKind {
    fn from(value: Slider) -> Self {
        NodeKind::Slider(crate::widgets::internal::SliderNode::from(value))
    }
}

impl From<Splitter> for NodeKind {
    fn from(value: Splitter) -> Self {
        NodeKind::Splitter(crate::widgets::internal::SplitterNode::from(value))
    }
}

impl From<AsciiCanvas> for NodeKind {
    fn from(value: AsciiCanvas) -> Self {
        NodeKind::AsciiCanvas(AsciiCanvasNode::from(value))
    }
}

impl From<EffectScope> for NodeKind {
    fn from(value: EffectScope) -> Self {
        NodeKind::EffectScope(EffectScopeNode::from(value))
    }
}

impl From<Animated> for NodeKind {
    fn from(value: Animated) -> Self {
        NodeKind::Animated(AnimatedNode::from(value))
    }
}

impl From<DragSource> for NodeKind {
    fn from(value: DragSource) -> Self {
        NodeKind::DragSource(DragSourceNode::from(value))
    }
}

impl From<DropTarget> for NodeKind {
    fn from(value: DropTarget) -> Self {
        NodeKind::DropTarget(DropTargetNode::from(value))
    }
}

impl From<MouseRegion> for NodeKind {
    fn from(value: MouseRegion) -> Self {
        NodeKind::MouseRegion(MouseRegionNode::from(value))
    }
}

impl From<PanView> for NodeKind {
    fn from(value: PanView) -> Self {
        NodeKind::PanView(PanViewNode::from(value))
    }
}
