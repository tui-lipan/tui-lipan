use std::fmt;

use crate::core::node::NodeKind;

/// Public widget kind tag for UI snapshot descriptions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum UiWidgetKind {
    Text,
    #[cfg(feature = "big-text")]
    BigText,
    AsciiCanvas,
    Button,
    Input,
    #[cfg(feature = "image")]
    Image,
    TextArea,
    HexArea,
    #[cfg(feature = "terminal")]
    Terminal,
    Popover,
    List,
    Table,
    Tabs,
    DraggableTabBar,
    Sparkline,
    Chart,
    Graph,
    SequenceDiagram,
    Flowchart,
    ClassDiagram,
    StateDiagram,
    ErDiagram,
    GanttDiagram,
    Heatmap,
    Group,
    EffectScope,
    Animated,
    DragSource,
    DropTarget,
    MouseRegion,
    ScrollView,
    PanView,
    VStack,
    HStack,
    Grid,
    Flow,
    Canvas,
    ZStack,
    Center,
    CenterPin,
    Frame,
    Divider,
    Spacer,
    Checkbox,
    ProgressBar,
    Spinner,
    Slider,
    Splitter,
    DocumentView,
    Portal,
}

impl fmt::Display for UiWidgetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl UiWidgetKind {
    /// Stable snake-free PascalCase name for agents and export formats.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "Text",
            #[cfg(feature = "big-text")]
            Self::BigText => "BigText",
            Self::AsciiCanvas => "AsciiCanvas",
            Self::Button => "Button",
            Self::Input => "Input",
            #[cfg(feature = "image")]
            Self::Image => "Image",
            Self::TextArea => "TextArea",
            Self::HexArea => "HexArea",
            #[cfg(feature = "terminal")]
            Self::Terminal => "Terminal",
            Self::Popover => "Popover",
            Self::List => "List",
            Self::Table => "Table",
            Self::Tabs => "Tabs",
            Self::DraggableTabBar => "DraggableTabBar",
            Self::Sparkline => "Sparkline",
            Self::Chart => "Chart",
            Self::Graph => "Graph",
            Self::SequenceDiagram => "SequenceDiagram",
            Self::Flowchart => "Flowchart",
            Self::ClassDiagram => "ClassDiagram",
            Self::StateDiagram => "StateDiagram",
            Self::ErDiagram => "ErDiagram",
            Self::GanttDiagram => "GanttDiagram",
            Self::Heatmap => "Heatmap",
            Self::Group => "Group",
            Self::EffectScope => "EffectScope",
            Self::Animated => "Animated",
            Self::DragSource => "DragSource",
            Self::DropTarget => "DropTarget",
            Self::MouseRegion => "MouseRegion",
            Self::ScrollView => "ScrollView",
            Self::PanView => "PanView",
            Self::VStack => "VStack",
            Self::HStack => "HStack",
            Self::Grid => "Grid",
            Self::Flow => "Flow",
            Self::Canvas => "Canvas",
            Self::ZStack => "ZStack",
            Self::Center => "Center",
            Self::CenterPin => "CenterPin",
            Self::Frame => "Frame",
            Self::Divider => "Divider",
            Self::Spacer => "Spacer",
            Self::Checkbox => "Checkbox",
            Self::ProgressBar => "ProgressBar",
            Self::Spinner => "Spinner",
            Self::Slider => "Slider",
            Self::Splitter => "Splitter",
            Self::DocumentView => "DocumentView",
            Self::Portal => "Portal",
        }
    }

    pub(crate) fn from_node_kind(kind: &NodeKind) -> Self {
        match kind {
            NodeKind::Text(_) => Self::Text,
            #[cfg(feature = "big-text")]
            NodeKind::BigText(_) => Self::BigText,
            NodeKind::AsciiCanvas(_) => Self::AsciiCanvas,
            NodeKind::Button(_) => Self::Button,
            NodeKind::Input(_) => Self::Input,
            #[cfg(feature = "image")]
            NodeKind::Image(_) => Self::Image,
            NodeKind::TextArea(_) => Self::TextArea,
            NodeKind::HexArea(_) => Self::HexArea,
            #[cfg(feature = "terminal")]
            NodeKind::Terminal(_) => Self::Terminal,
            NodeKind::Popover(_) => Self::Popover,
            NodeKind::List(_) => Self::List,
            NodeKind::Table(_) => Self::Table,
            NodeKind::Tabs(_) => Self::Tabs,
            NodeKind::DraggableTabBar(_) => Self::DraggableTabBar,
            NodeKind::Sparkline(_) => Self::Sparkline,
            NodeKind::Chart(_) => Self::Chart,
            NodeKind::Graph(_) => Self::Graph,
            NodeKind::SequenceDiagram(_) => Self::SequenceDiagram,
            NodeKind::Flowchart(_) => Self::Flowchart,
            NodeKind::ClassDiagram(_) => Self::ClassDiagram,
            NodeKind::StateDiagram(_) => Self::StateDiagram,
            NodeKind::ErDiagram(_) => Self::ErDiagram,
            NodeKind::GanttDiagram(_) => Self::GanttDiagram,
            NodeKind::Heatmap(_) => Self::Heatmap,
            NodeKind::Group(_) => Self::Group,
            NodeKind::EffectScope(_) => Self::EffectScope,
            NodeKind::Animated(_) => Self::Animated,
            NodeKind::DragSource(_) => Self::DragSource,
            NodeKind::DropTarget(_) => Self::DropTarget,
            NodeKind::MouseRegion(_) => Self::MouseRegion,
            NodeKind::ScrollView(_) => Self::ScrollView,
            NodeKind::PanView(_) => Self::PanView,
            NodeKind::VStack(_) => Self::VStack,
            NodeKind::HStack(_) => Self::HStack,
            NodeKind::Grid(_) => Self::Grid,
            NodeKind::Flow(_) => Self::Flow,
            NodeKind::Canvas(_) => Self::Canvas,
            NodeKind::ZStack(_) => Self::ZStack,
            NodeKind::Center(_) => Self::Center,
            NodeKind::CenterPin(_) => Self::CenterPin,
            NodeKind::StatusBarLayout(_) => Self::HStack,
            NodeKind::Frame(_) => Self::Frame,
            NodeKind::Divider(_) => Self::Divider,
            NodeKind::Spacer(_) => Self::Spacer,
            NodeKind::Checkbox(_) => Self::Checkbox,
            NodeKind::ProgressBar(_) => Self::ProgressBar,
            NodeKind::Spinner(_) => Self::Spinner,
            NodeKind::Slider(_) => Self::Slider,
            NodeKind::Splitter(_) => Self::Splitter,
            NodeKind::DocumentView(_) => Self::DocumentView,
            NodeKind::Portal(_) => Self::Portal,
        }
    }
}
