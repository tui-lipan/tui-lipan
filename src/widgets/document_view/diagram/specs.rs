use std::sync::Arc;

use crate::style::Color;
use crate::style::Style;
use crate::widgets::gantt_diagram::GanttSpec;

/// Parsed Mermaid diagram data supported by `DocumentView`.
#[derive(Clone, Debug, PartialEq)]
pub enum ParsedDiagram {
    /// Mermaid `flowchart` / `graph`.
    Flowchart(FlowchartSpec),
    /// Mermaid `sequenceDiagram`.
    Sequence(SequenceSpec),
    /// Mermaid `classDiagram`.
    Class(ClassSpec),
    /// Mermaid `stateDiagram-v2`.
    State(StateSpec),
    /// Mermaid `erDiagram`.
    Er(ErSpec),
    /// Mermaid `pie`.
    Pie(PieSpec),
    /// Mermaid `gantt`.
    Gantt(GanttSpec),
}

/// Flowchart orientation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiagramDirection {
    /// Top-to-bottom.
    #[default]
    TopDown,
    /// Bottom-to-top.
    BottomUp,
    /// Left-to-right.
    LeftRight,
    /// Right-to-left.
    RightLeft,
}

/// Flowchart node shape subset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlowNodeShape {
    /// Rectangular node.
    #[default]
    Rect,
    /// Rounded node.
    Round,
    /// Diamond node.
    Diamond,
    /// Circle node.
    Circle,
    /// Cylinder/database node.
    Cylinder,
}

/// Flowchart node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowNodeSpec {
    pub id: Arc<str>,
    pub label: Arc<str>,
    pub shape: FlowNodeShape,
    pub style: NodeStyle,
}

/// Flowchart node style parsed from Mermaid `style` directives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeStyle {
    pub fill: Option<Color>,
    pub label_fg: Option<Color>,
    pub border_fg: Option<Color>,
}

impl NodeStyle {
    #[cfg(feature = "markdown")]
    pub(crate) fn merge(&mut self, other: Self) {
        self.fill = other.fill.or(self.fill);
        self.label_fg = other.label_fg.or(self.label_fg);
        self.border_fg = other.border_fg.or(self.border_fg);
    }

    pub(super) fn fill_style(self) -> Style {
        Style {
            bg: self.fill.map(Into::into),
            ..Style::default()
        }
    }

    pub(super) fn label_style(self) -> Style {
        Style {
            fg: self.label_fg.map(Into::into),
            ..Style::default()
        }
    }

    pub(super) fn border_style(self) -> Style {
        Style {
            fg: self.border_fg.map(Into::into),
            ..Style::default()
        }
    }
}

/// Flowchart edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowEdgeSpec {
    pub from: Arc<str>,
    pub to: Arc<str>,
    pub label: Option<Arc<str>>,
    pub dashed: bool,
}

/// Flowchart diagram spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowchartSpec {
    pub direction: DiagramDirection,
    pub nodes: Vec<FlowNodeSpec>,
    pub edges: Vec<FlowEdgeSpec>,
}

/// Sequence participant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequenceParticipantSpec {
    pub id: Arc<str>,
    pub label: Arc<str>,
    pub actor: bool,
}

/// Sequence message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequenceMessageSpec {
    pub from: Arc<str>,
    pub to: Arc<str>,
    pub label: Arc<str>,
    pub dashed: bool,
    pub open_arrow: bool,
}

/// Sequence diagram spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequenceSpec {
    pub participants: Vec<SequenceParticipantSpec>,
    pub messages: Vec<SequenceMessageSpec>,
}

/// UML visibility.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClassVisibilitySpec {
    /// Public member.
    #[default]
    Public,
    /// Private member.
    Private,
    /// Protected member.
    Protected,
    /// Package member.
    Package,
}

/// Class member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassMemberSpec {
    pub visibility: ClassVisibilitySpec,
    pub name: Arc<str>,
    pub ty: Option<Arc<str>>,
    pub method: bool,
}

/// Class definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassNodeSpec {
    pub name: Arc<str>,
    pub members: Vec<ClassMemberSpec>,
}

/// Class relation kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassRelationSpec {
    pub from: Arc<str>,
    pub to: Arc<str>,
    pub arrow: Arc<str>,
    pub from_cardinality: Option<Arc<str>>,
    pub to_cardinality: Option<Arc<str>>,
    pub label: Option<Arc<str>>,
}

/// Class diagram spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassSpec {
    pub classes: Vec<ClassNodeSpec>,
    pub relations: Vec<ClassRelationSpec>,
}

/// State kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StateKindSpec {
    /// Normal state.
    #[default]
    State,
    /// Start marker.
    Start,
    /// End marker.
    End,
    /// Choice marker.
    Choice,
}

/// State node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateNodeSpec {
    pub id: Arc<str>,
    pub label: Arc<str>,
    pub kind: StateKindSpec,
}

/// State transition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateTransitionSpec {
    pub from: Arc<str>,
    pub to: Arc<str>,
    pub label: Option<Arc<str>>,
}

/// State diagram spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateSpec {
    pub states: Vec<StateNodeSpec>,
    pub transitions: Vec<StateTransitionSpec>,
}

/// ER attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErAttributeSpec {
    pub ty: Arc<str>,
    pub name: Arc<str>,
    pub keys: Vec<Arc<str>>,
}

/// ER entity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErEntitySpec {
    pub name: Arc<str>,
    pub attributes: Vec<ErAttributeSpec>,
}

/// ER relation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErRelationSpec {
    pub left: Arc<str>,
    pub right: Arc<str>,
    pub left_cardinality: Arc<str>,
    pub right_cardinality: Arc<str>,
    pub label: Option<Arc<str>>,
}

/// ER diagram spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErSpec {
    pub entities: Vec<ErEntitySpec>,
    pub relations: Vec<ErRelationSpec>,
}

/// Pie slice.
#[derive(Clone, Debug, PartialEq)]
pub struct PieSliceSpec {
    pub label: Arc<str>,
    pub value: f64,
}

/// Pie chart spec.
#[derive(Clone, Debug, PartialEq)]
pub struct PieSpec {
    pub title: Option<Arc<str>>,
    pub slices: Vec<PieSliceSpec>,
}
