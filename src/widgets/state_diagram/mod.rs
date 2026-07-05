//! Static UML state diagram widget.

mod layout;
mod node;
mod reconcile;
mod theme;

pub use layout::measure_state_diagram;
pub use node::StateDiagramNode;
pub use reconcile::reconcile_state_diagram;
pub use theme::StateDiagramTheme;

use crate::core::element::{Element, ElementKind};
use crate::style::{BorderStyle, Length, Padding, Style};
use std::sync::Arc;

/// The kind of node a [`StateSpec`] represents, controlling how it is rendered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StateKind {
    /// A normal named state box.
    #[default]
    State,
    /// The initial pseudo-state (filled dot).
    Start,
    /// The final pseudo-state (ringed dot).
    End,
    /// A choice (decision) pseudo-state (diamond).
    Choice,
    /// A fork pseudo-state (splits into parallel branches).
    Fork,
    /// A join pseudo-state (merges parallel branches).
    Join,
}

/// A single state node in a [`StateDiagram`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StateSpec {
    /// Stable identifier referenced by transitions.
    pub id: Arc<str>,
    /// Display label (defaults to `id`).
    pub label: Arc<str>,
    /// What kind of node this is.
    pub kind: StateKind,
    /// Optional `entry /` action text shown inside the state.
    pub entry: Option<Arc<str>>,
    /// Optional `exit /` action text shown inside the state.
    pub exit: Option<Arc<str>>,
}
impl StateSpec {
    /// Creates a normal state whose label defaults to its `id`.
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        let id = id.into();
        Self {
            label: id.clone(),
            id,
            kind: StateKind::State,
            entry: None,
            exit: None,
        }
    }
    /// Sets the display label.
    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = label.into();
        self
    }
    /// Sets the node kind.
    pub fn kind(mut self, kind: StateKind) -> Self {
        self.kind = kind;
        self
    }
    /// Sets the `entry /` action text.
    pub fn entry(mut self, entry: impl Into<Arc<str>>) -> Self {
        self.entry = Some(entry.into());
        self
    }
    /// Sets the `exit /` action text.
    pub fn exit(mut self, exit: impl Into<Arc<str>>) -> Self {
        self.exit = Some(exit.into());
        self
    }
}

/// A directed transition between two states, with an optional label and guard.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StateTransition {
    /// Source state id.
    pub from: Arc<str>,
    /// Target state id.
    pub to: Arc<str>,
    /// Optional transition (event) label.
    pub label: Option<Arc<str>>,
    /// Optional `[guard]` condition shown on the edge.
    pub guard: Option<Arc<str>>,
}

/// A static UML state diagram laid out automatically from states and transitions.
/// Build it with the chaining setters and convert into an [`Element`].
#[derive(Clone)]
pub struct StateDiagram {
    pub(crate) states: Arc<[StateSpec]>,
    pub(crate) transitions: Arc<[StateTransition]>,
    pub(crate) style: Style,
    pub(crate) state_style: Style,
    pub(crate) edge_style: Style,
    pub(crate) label_style: Style,
    pub(crate) border_style: BorderStyle,
    pub(crate) padding: Padding,
    pub(crate) node_padding: Padding,
    pub(crate) layer_gap: u16,
    pub(crate) node_gap: u16,
    pub(crate) max_node_width: u16,
    pub(crate) theme: StateDiagramTheme,
    pub(crate) width: Length,
    pub(crate) height: Length,
}

impl Default for StateDiagram {
    fn default() -> Self {
        Self {
            states: Arc::new([]),
            transitions: Arc::new([]),
            style: Style::default(),
            state_style: Style::default(),
            edge_style: Style::default(),
            label_style: Style::default(),
            border_style: BorderStyle::Rounded,
            padding: Padding::default(),
            node_padding: (0, 1).into(),
            layer_gap: 1,
            node_gap: 4,
            max_node_width: 32,
            theme: StateDiagramTheme::default(),
            width: Length::Auto,
            height: Length::Auto,
        }
    }
}

impl StateDiagram {
    /// Creates an empty diagram with default styling.
    pub fn new() -> Self {
        Self::default()
    }
    /// Replaces the state set with `states`.
    pub fn states(mut self, states: impl IntoIterator<Item = StateSpec>) -> Self {
        self.states = states.into_iter().collect::<Vec<_>>().into();
        self
    }
    /// Replaces the transition set with `transitions`.
    pub fn transitions(mut self, transitions: impl IntoIterator<Item = StateTransition>) -> Self {
        self.transitions = transitions.into_iter().collect::<Vec<_>>().into();
        self
    }
    /// Appends a normal state by id.
    pub fn state(mut self, id: impl Into<Arc<str>>) -> Self {
        let mut v = self.states.to_vec();
        v.push(StateSpec::new(id));
        self.states = v.into();
        self
    }
    /// Appends a [`Choice`](StateKind::Choice) pseudo-state by id.
    pub fn choice(mut self, id: impl Into<Arc<str>>) -> Self {
        let mut v = self.states.to_vec();
        v.push(StateSpec::new(id).kind(StateKind::Choice));
        self.states = v.into();
        self
    }
    /// Appends a [`Fork`](StateKind::Fork) pseudo-state by id.
    pub fn fork(mut self, id: impl Into<Arc<str>>) -> Self {
        let mut v = self.states.to_vec();
        v.push(StateSpec::new(id).kind(StateKind::Fork));
        self.states = v.into();
        self
    }
    /// Appends a [`Join`](StateKind::Join) pseudo-state by id.
    pub fn join(mut self, id: impl Into<Arc<str>>) -> Self {
        let mut v = self.states.to_vec();
        v.push(StateSpec::new(id).kind(StateKind::Join));
        self.states = v.into();
        self
    }
    /// Adds a transition between two states with an optional label.
    pub fn transition(
        mut self,
        from: impl Into<Arc<str>>,
        to: impl Into<Arc<str>>,
        label: impl Into<Option<Arc<str>>>,
    ) -> Self {
        let mut v = self.transitions.to_vec();
        v.push(StateTransition {
            from: from.into(),
            to: to.into(),
            label: label.into(),
            guard: None,
        });
        self.transitions = v.into();
        self
    }
    /// Adds the initial pseudo-state (if absent) and a transition from it to `to`.
    pub fn start_to(mut self, to: impl Into<Arc<str>>) -> Self {
        let start = Arc::<str>::from("[*]");
        if !self.states.iter().any(|s| s.id == start) {
            let mut states = self.states.to_vec();
            states.push(StateSpec::new(start.clone()).kind(StateKind::Start));
            self.states = states.into();
        }
        self.transition(start, to, None::<Arc<str>>)
    }
    /// Adds the final pseudo-state (if absent) and a transition from `from` to it.
    pub fn end_from(mut self, from: impl Into<Arc<str>>) -> Self {
        let end = Arc::<str>::from("[end]");
        if !self.states.iter().any(|s| s.id == end) {
            let mut states = self.states.to_vec();
            states.push(StateSpec::new(end.clone()).kind(StateKind::End));
            self.states = states.into();
        }
        self.transition(from, end, None::<Arc<str>>)
    }
    /// Sets the base style of the diagram container.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
    /// Sets the style applied to state boxes.
    pub fn state_style(mut self, style: Style) -> Self {
        self.state_style = style;
        self
    }
    /// Sets the style applied to transition edges.
    pub fn edge_style(mut self, style: Style) -> Self {
        self.edge_style = style;
        self
    }
    /// Sets the style applied to edge labels.
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }
    /// Sets the border line style for state boxes.
    pub fn border_style(mut self, style: BorderStyle) -> Self {
        self.border_style = style;
        self
    }
    /// Sets the outer padding of the diagram.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }
    /// Sets the inner padding of each state box.
    pub fn node_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.node_padding = padding.into();
        self
    }
    /// Caps the rendered width of a state box (minimum 1).
    pub fn max_node_width(mut self, width: u16) -> Self {
        self.max_node_width = width.max(1);
        self
    }
    /// Sets the width of the diagram container.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }
    /// Sets the height of the diagram container.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }
}

impl From<StateDiagram> for Element {
    fn from(value: StateDiagram) -> Self {
        Element::new(ElementKind::StateDiagram(Box::new(value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layer_gap_is_compact() {
        assert_eq!(StateDiagram::default().layer_gap, 1);
    }
}
