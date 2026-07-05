//! Mermaid-style UML sequence diagram widget.

mod layout;
mod node;
mod reconcile;
mod theme;

pub use layout::measure_sequence_diagram;
pub use node::SequenceDiagramNode;
pub(crate) use node::{PositionedFragment, PositionedMessage, autonumber_rect};
pub use reconcile::{reconcile_sequence_diagram, reconcile_sequence_diagram_with_width};
pub use theme::{
    ActivationTheme, AutonumberTheme, FragmentGlyphs, LifelineTheme, MessageGlyphs,
    SequenceDiagramTheme,
};

use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind};
use crate::style::{BorderStyle, Length, Padding, Style};
use crate::widgets::Overflow;

/// Stable actor key used by messages and notes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ActorRef(pub Arc<str>);

impl ActorRef {
    /// Create an actor reference from an alias/key.
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }

    /// Return the actor alias/key.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ActorRef {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ActorRef {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<Arc<str>> for ActorRef {
    fn from(value: Arc<str>) -> Self {
        Self::new(value)
    }
}

/// Participant header rendering kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ActorKind {
    /// Rectangular participant box.
    #[default]
    Participant,
    /// Actor/stick-figure header.
    Actor,
}

/// Sequence diagram rendering style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SequenceDiagramVariant {
    /// Mermaid-style boxed participant headers.
    #[default]
    Boxed,
    /// Compact headers with lifeline tee joints and no participant boxes.
    Minimal,
}

/// Message arrow style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MessageStyle {
    /// Solid line, filled arrow head.
    #[default]
    Sync,
    /// Solid line, open arrow head.
    Async,
    /// Dashed reply with filled arrow head.
    SyncReply,
    /// Dashed reply with open arrow head.
    AsyncReply,
    /// Lost message terminator.
    Lost,
    /// Open message terminator.
    Open,
}

impl MessageStyle {
    /// Number of distinct message styles (size of the per-style theme table).
    pub const INDEX_COUNT: usize = 6;

    /// Dense `0..INDEX_COUNT` index for this style, used to key theme tables.
    pub const fn index(self) -> usize {
        match self {
            Self::Sync => 0,
            Self::Async => 1,
            Self::SyncReply => 2,
            Self::AsyncReply => 3,
            Self::Lost => 4,
            Self::Open => 5,
        }
    }
}

/// Fragment block type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FragmentKind {
    /// Repeated block (`loop`).
    Loop,
    /// Conditional alternatives (`alt` / `else`).
    Alt,
    /// Optional block (`opt`).
    Opt,
    /// Parallel branches (`par` / `and`).
    Par,
    /// Critical region (`critical`).
    Critical,
    /// Break block (`break`).
    Break,
    /// Plain background rectangle grouping (`rect`).
    Rect,
}

impl FragmentKind {
    /// Number of distinct fragment kinds (size of the per-kind theme table).
    pub const INDEX_COUNT: usize = 7;

    /// Dense `0..INDEX_COUNT` index for this kind, used to key theme tables.
    pub const fn index(self) -> usize {
        match self {
            Self::Loop => 0,
            Self::Alt => 1,
            Self::Opt => 2,
            Self::Par => 3,
            Self::Critical => 4,
            Self::Break => 5,
            Self::Rect => 6,
        }
    }
}

/// Note placement relative to actor columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotePlacement {
    /// To the left of a single actor column.
    LeftOf,
    /// To the right of a single actor column.
    RightOf,
    /// Spanning over one or more actor columns.
    Over,
}

/// Stable item path returned by hit testing and pointer callbacks.
///
/// Each variant carries the zero-based index of the item within its category, in
/// the order the steps were added.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SequenceItemPath {
    /// A message arrow between two actors.
    Message(usize),
    /// A self-message loop on one actor.
    SelfMessage(usize),
    /// A participant header.
    Participant(usize),
    /// A note box.
    Note(usize),
    /// A fragment block.
    Fragment(usize),
    /// A divider line.
    Divider(usize),
}

/// Event payload for sequence diagram item interactions.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SequenceItemEvent {
    /// Path identifying the interacted item.
    pub path: SequenceItemPath,
    /// Display label of the interacted item.
    pub label: Arc<str>,
}

/// Message between two actors.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SequenceMessage {
    /// Source actor.
    pub from: ActorRef,
    /// Target actor.
    pub to: ActorRef,
    /// Message label drawn on the arrow.
    pub label: Arc<str>,
    /// Arrow style.
    pub style: MessageStyle,
    /// Whether arrival activates (starts an activation bar on) the target.
    pub activate_target: bool,
    /// Whether sending deactivates (ends the activation bar on) the source.
    pub deactivate_source: bool,
    /// Optional per-message override for the arrow line style.
    pub line_style: Option<Style>,
    /// Optional per-message override for the label style.
    pub label_style: Option<Style>,
}

impl SequenceMessage {
    /// Creates a [`Sync`](MessageStyle::Sync) message with the given endpoints and label.
    pub fn new(
        from: impl Into<ActorRef>,
        to: impl Into<ActorRef>,
        label: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            label: label.into(),
            style: MessageStyle::Sync,
            activate_target: false,
            deactivate_source: false,
            line_style: None,
            label_style: None,
        }
    }

    /// Creates a [`Sync`](MessageStyle::Sync) message (solid line, filled arrow).
    pub fn sync(
        from: impl Into<ActorRef>,
        to: impl Into<ActorRef>,
        label: impl Into<Arc<str>>,
    ) -> Self {
        Self::new(from, to, label).message_style(MessageStyle::Sync)
    }

    /// Creates an [`Async`](MessageStyle::Async) message (solid line, open arrow).
    pub fn async_(
        from: impl Into<ActorRef>,
        to: impl Into<ActorRef>,
        label: impl Into<Arc<str>>,
    ) -> Self {
        Self::new(from, to, label).message_style(MessageStyle::Async)
    }

    /// Creates a [`SyncReply`](MessageStyle::SyncReply) message (dashed reply, filled arrow).
    pub fn reply(
        from: impl Into<ActorRef>,
        to: impl Into<ActorRef>,
        label: impl Into<Arc<str>>,
    ) -> Self {
        Self::new(from, to, label).message_style(MessageStyle::SyncReply)
    }

    /// Creates an [`AsyncReply`](MessageStyle::AsyncReply) message (dashed reply, open arrow).
    pub fn async_reply(
        from: impl Into<ActorRef>,
        to: impl Into<ActorRef>,
        label: impl Into<Arc<str>>,
    ) -> Self {
        Self::new(from, to, label).message_style(MessageStyle::AsyncReply)
    }

    /// Creates a [`Lost`](MessageStyle::Lost) message (terminator marker).
    pub fn lost(
        from: impl Into<ActorRef>,
        to: impl Into<ActorRef>,
        label: impl Into<Arc<str>>,
    ) -> Self {
        Self::new(from, to, label).message_style(MessageStyle::Lost)
    }

    /// Creates an [`Open`](MessageStyle::Open) message (open terminator marker).
    pub fn open(
        from: impl Into<ActorRef>,
        to: impl Into<ActorRef>,
        label: impl Into<Arc<str>>,
    ) -> Self {
        Self::new(from, to, label).message_style(MessageStyle::Open)
    }

    /// Sets the arrow style.
    pub fn message_style(mut self, style: MessageStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets whether arrival activates the target.
    pub fn activate_target(mut self, activate: bool) -> Self {
        self.activate_target = activate;
        self
    }

    /// Sets whether sending deactivates the source.
    pub fn deactivate_source(mut self, deactivate: bool) -> Self {
        self.deactivate_source = deactivate;
        self
    }

    /// Overrides the arrow line style for this message.
    pub fn line_style(mut self, style: Style) -> Self {
        self.line_style = Some(style);
        self
    }

    /// Overrides the label style for this message.
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = Some(style);
        self
    }
}

/// Compatibility alias for applications that use Mermaid naming.
pub type Msg = SequenceMessage;

/// One flat sequence diagram command.
///
/// Steps are stored in order; fragment blocks are delimited by
/// [`FragmentBegin`](Self::FragmentBegin)/[`FragmentEnd`](Self::FragmentEnd) pairs
/// rather than nested values.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SequenceStep {
    /// A message between two actors.
    Message(SequenceMessage),
    /// A self-message loop on a single actor.
    SelfMessage {
        /// Actor the message loops on.
        actor: ActorRef,
        /// Message label.
        label: Arc<str>,
        /// Optional style override.
        style: Option<Style>,
    },
    /// A note attached to one or more actors.
    Note {
        /// Placement relative to the actor column(s).
        placement: NotePlacement,
        /// Actors the note spans/attaches to.
        actors: Arc<[ActorRef]>,
        /// Note text.
        text: Arc<str>,
        /// Optional style override.
        style: Option<Style>,
    },
    /// Begins an activation bar on the given actor.
    Activate(ActorRef),
    /// Ends an activation bar on the given actor.
    Deactivate(ActorRef),
    /// Opens a fragment block.
    FragmentBegin {
        /// Fragment kind.
        kind: FragmentKind,
        /// Fragment title label.
        label: Arc<str>,
        /// Optional first-branch label (e.g. the `alt` condition).
        branch_label: Option<Arc<str>>,
        /// Optional style override.
        style: Option<Style>,
    },
    /// Starts a new branch within the open fragment (e.g. `else`, `and`).
    FragmentBranch {
        /// Fragment kind the branch belongs to.
        kind: FragmentKind,
        /// Branch label.
        label: Arc<str>,
    },
    /// Closes the open fragment block.
    FragmentEnd,
    /// Draws a background rectangle behind subsequent steps until balanced.
    Rect {
        /// Background fill style.
        color: Style,
    },
    /// A labelled divider line spanning the diagram width.
    Divider(Arc<str>),
}

impl SequenceStep {
    /// Wraps a [`SequenceMessage`] into a [`Message`](Self::Message) step.
    pub fn message(message: SequenceMessage) -> Self {
        Self::Message(message)
    }
    /// Builds a [`SelfMessage`](Self::SelfMessage) step.
    pub fn self_msg(actor: impl Into<ActorRef>, label: impl Into<Arc<str>>) -> Self {
        Self::SelfMessage {
            actor: actor.into(),
            label: label.into(),
            style: None,
        }
    }
    /// Builds a [`Note`](Self::Note) step placed over the given actors.
    pub fn note_over(
        actors: impl IntoIterator<Item = impl Into<ActorRef>>,
        text: impl Into<Arc<str>>,
    ) -> Self {
        Self::Note {
            placement: NotePlacement::Over,
            actors: Arc::<[ActorRef]>::from(actors.into_iter().map(Into::into).collect::<Vec<_>>()),
            text: text.into(),
            style: None,
        }
    }
    /// Builds a [`Note`](Self::Note) step with explicit placement.
    pub fn note(
        placement: NotePlacement,
        actors: impl IntoIterator<Item = impl Into<ActorRef>>,
        text: impl Into<Arc<str>>,
    ) -> Self {
        Self::Note {
            placement,
            actors: Arc::<[ActorRef]>::from(actors.into_iter().map(Into::into).collect::<Vec<_>>()),
            text: text.into(),
            style: None,
        }
    }
    /// Builds an [`Activate`](Self::Activate) step.
    pub fn activate(actor: impl Into<ActorRef>) -> Self {
        Self::Activate(actor.into())
    }
    /// Builds a [`Deactivate`](Self::Deactivate) step.
    pub fn deactivate(actor: impl Into<ActorRef>) -> Self {
        Self::Deactivate(actor.into())
    }
    /// Builds a [`FragmentBegin`](Self::FragmentBegin) step.
    pub fn fragment_begin(kind: FragmentKind, label: impl Into<Arc<str>>) -> Self {
        Self::FragmentBegin {
            kind,
            label: label.into(),
            branch_label: None,
            style: None,
        }
    }
    /// Builds a [`FragmentBranch`](Self::FragmentBranch) step.
    pub fn fragment_branch(kind: FragmentKind, label: impl Into<Arc<str>>) -> Self {
        Self::FragmentBranch {
            kind,
            label: label.into(),
        }
    }
    /// Builds a [`FragmentEnd`](Self::FragmentEnd) step.
    pub fn fragment_end() -> Self {
        Self::FragmentEnd
    }
}

/// Compatibility alias for concise examples.
pub type Step = SequenceStep;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ParticipantSpec {
    pub(crate) actor: ActorRef,
    pub(crate) label: Arc<str>,
    pub(crate) kind: ActorKind,
}

/// Direct-paint UML sequence diagram widget.
#[derive(Clone)]
pub struct SequenceDiagram {
    pub(crate) participants: Vec<ParticipantSpec>,
    pub(crate) steps: Vec<SequenceStep>,
    pub(crate) variant: SequenceDiagramVariant,
    pub(crate) actor_glyph: Arc<str>,
    pub(crate) style: Style,
    pub(crate) theme: SequenceDiagramTheme,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) padding: Padding,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) max_label_cells: Option<u16>,
    pub(crate) message_label_overflow: Overflow,
    pub(crate) autonumber: bool,
    pub(crate) repeat_participants_at_bottom: bool,
    pub(crate) on_item_click: Option<Callback<SequenceItemEvent>>,
    pub(crate) on_item_hover: Option<Callback<SequenceItemEvent>>,
}

impl Default for SequenceDiagram {
    fn default() -> Self {
        Self {
            participants: Vec::new(),
            steps: Vec::new(),
            variant: SequenceDiagramVariant::Boxed,
            actor_glyph: Arc::from("○ "),
            style: Style::default(),
            theme: SequenceDiagramTheme::classic(),
            border: false,
            border_style: BorderStyle::Plain,
            padding: Padding::default(),
            width: Length::Auto,
            height: Length::Auto,
            max_label_cells: Some(32),
            message_label_overflow: Overflow::Ellipsis,
            autonumber: false,
            repeat_participants_at_bottom: false,
            on_item_click: None,
            on_item_hover: None,
        }
    }
}

impl SequenceDiagram {
    /// Creates an empty diagram with the default (boxed/classic) theme.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares a participant whose label equals its key. Re-declaring updates it.
    pub fn participant(mut self, actor: impl Into<ActorRef>) -> Self {
        let actor = actor.into();
        let label = actor.0.clone();
        self.upsert_participant(actor, label, ActorKind::Participant);
        self
    }

    /// Declares a participant with a separate alias (key) and display label.
    pub fn participant_aliased(
        mut self,
        alias: impl Into<ActorRef>,
        label: impl Into<Arc<str>>,
    ) -> Self {
        self.upsert_participant(alias.into(), label.into(), ActorKind::Participant);
        self
    }

    /// Sets the header rendering kind for an actor, declaring it if needed.
    pub fn actor_kind(mut self, actor: impl Into<ActorRef>, kind: ActorKind) -> Self {
        let actor = actor.into();
        if let Some(participant) = self.participants.iter_mut().find(|p| p.actor == actor) {
            participant.kind = kind;
        } else {
            let label = actor.0.clone();
            self.upsert_participant(actor, label, kind);
        }
        self
    }

    /// Appends a raw [`SequenceStep`].
    pub fn step(mut self, step: SequenceStep) -> Self {
        self.steps.push(step);
        self
    }
    /// Appends a message step.
    pub fn message(self, message: SequenceMessage) -> Self {
        self.step(SequenceStep::Message(message))
    }
    /// Appends a self-message step.
    pub fn self_msg(self, actor: impl Into<ActorRef>, label: impl Into<Arc<str>>) -> Self {
        self.step(SequenceStep::self_msg(actor, label))
    }
    /// Appends a note spanning over the given actors.
    pub fn note_over(
        self,
        actors: impl IntoIterator<Item = impl Into<ActorRef>>,
        text: impl Into<Arc<str>>,
    ) -> Self {
        self.step(SequenceStep::note_over(actors, text))
    }
    /// Appends a note to the left of an actor.
    pub fn note_left_of(self, actor: impl Into<ActorRef>, text: impl Into<Arc<str>>) -> Self {
        self.note_one(NotePlacement::LeftOf, actor, text)
    }
    /// Appends a note to the right of an actor.
    pub fn note_right_of(self, actor: impl Into<ActorRef>, text: impl Into<Arc<str>>) -> Self {
        self.note_one(NotePlacement::RightOf, actor, text)
    }
    /// Appends an activate step for an actor.
    pub fn activate(self, actor: impl Into<ActorRef>) -> Self {
        self.step(SequenceStep::Activate(actor.into()))
    }
    /// Appends a deactivate step for an actor.
    pub fn deactivate(self, actor: impl Into<ActorRef>) -> Self {
        self.step(SequenceStep::Deactivate(actor.into()))
    }
    /// Opens a fragment block of the given kind. Prefer the scoped helpers
    /// ([`loop_`](Self::loop_), [`alt`](Self::alt), …) when possible.
    pub fn fragment_begin(self, kind: FragmentKind, label: impl Into<Arc<str>>) -> Self {
        self.step(SequenceStep::fragment_begin(kind, label))
    }
    /// Starts a new branch within the open fragment.
    pub fn fragment_branch(self, kind: FragmentKind, label: impl Into<Arc<str>>) -> Self {
        self.step(SequenceStep::FragmentBranch {
            kind,
            label: label.into(),
        })
    }
    /// Closes the open fragment block.
    pub fn fragment_end(self) -> Self {
        self.step(SequenceStep::FragmentEnd)
    }
    /// Appends a background rectangle step.
    pub fn rect(self, color: Style) -> Self {
        self.step(SequenceStep::Rect { color })
    }
    /// Appends a labelled divider step.
    pub fn divider(self, label: impl Into<Arc<str>>) -> Self {
        self.step(SequenceStep::Divider(label.into()))
    }

    /// Adds a `loop` fragment whose body is built by `f`.
    pub fn loop_(self, label: impl Into<Arc<str>>, f: impl FnOnce(Self) -> Self) -> Self {
        self.fragment(FragmentKind::Loop, label, f)
    }
    /// Adds an `alt` fragment whose first branch body is built by `f`. Use
    /// [`else_`](Self::else_) inside `f` to start alternative branches.
    pub fn alt(self, label: impl Into<Arc<str>>, f: impl FnOnce(Self) -> Self) -> Self {
        self.fragment(FragmentKind::Alt, label, f)
    }
    /// Starts an `else` branch within an open `alt` fragment.
    pub fn else_(self, label: impl Into<Arc<str>>) -> Self {
        self.fragment_branch(FragmentKind::Alt, label)
    }
    /// Adds a `par` fragment whose first branch body is built by `f`. Use
    /// [`and`](Self::and) inside `f` to start parallel branches.
    pub fn par(self, label: impl Into<Arc<str>>, f: impl FnOnce(Self) -> Self) -> Self {
        self.fragment(FragmentKind::Par, label, f)
    }
    /// Starts an `and` branch within an open `par` fragment.
    pub fn and(self, label: impl Into<Arc<str>>) -> Self {
        self.fragment_branch(FragmentKind::Par, label)
    }
    /// Adds an `opt` fragment whose body is built by `f`.
    pub fn opt(self, label: impl Into<Arc<str>>, f: impl FnOnce(Self) -> Self) -> Self {
        self.fragment(FragmentKind::Opt, label, f)
    }
    /// Adds a `critical` fragment whose body is built by `f`.
    pub fn critical(self, label: impl Into<Arc<str>>, f: impl FnOnce(Self) -> Self) -> Self {
        self.fragment(FragmentKind::Critical, label, f)
    }
    /// Adds a `break` fragment whose body is built by `f`.
    pub fn break_(self, label: impl Into<Arc<str>>, f: impl FnOnce(Self) -> Self) -> Self {
        self.fragment(FragmentKind::Break, label, f)
    }

    /// Sets the base style of the diagram container.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
    /// Sets the style of participant header boxes.
    pub fn participant_style(mut self, style: Style) -> Self {
        self.theme.participant_style = style;
        self
    }
    /// Sets the style of lifelines.
    pub fn lifeline_style(mut self, style: Style) -> Self {
        self.theme.lifeline.style = style;
        self
    }
    /// Sets the default style of message labels.
    pub fn message_label_style(mut self, style: Style) -> Self {
        self.theme.message_label_style = style;
        self
    }
    /// Sets the style of note boxes.
    pub fn note_style(mut self, style: Style) -> Self {
        self.theme.note_style = style;
        self
    }
    /// Sets a single style for all fragment kinds.
    pub fn fragment_style(mut self, style: Style) -> Self {
        self.theme.fragment_styles.fill(style);
        self
    }
    /// Sets the style of activation bars.
    pub fn activation_style(mut self, style: Style) -> Self {
        self.theme.activation.style = style;
        self
    }
    /// Sets the style applied to a hovered item.
    pub fn item_hover_style(mut self, style: Style) -> Self {
        self.theme.hover_style = style;
        self
    }
    /// Sets the style of autonumber badges.
    pub fn autonumber_style(mut self, style: Style) -> Self {
        self.theme.autonumber.style = style;
        self
    }
    /// Replaces the entire theme.
    pub fn theme(mut self, theme: SequenceDiagramTheme) -> Self {
        self.theme = theme;
        self
    }
    /// Overrides the style for a single message kind.
    pub fn message_kind_style(mut self, kind: MessageStyle, style: Style) -> Self {
        *self.theme.message_style_mut(kind) = style;
        self
    }
    /// Overrides the style for a single fragment kind.
    pub fn fragment_kind_style(mut self, kind: FragmentKind, style: Style) -> Self {
        *self.theme.fragment_style_mut(kind) = style;
        self
    }
    /// Sets the glyph used to draw lifelines.
    pub fn lifeline_glyph(mut self, glyph: char) -> Self {
        self.theme.lifeline.glyph = glyph;
        self
    }
    /// Sets the glyph used to fill activation bars.
    pub fn activation_glyph(mut self, glyph: char) -> Self {
        self.theme.activation.fill_glyph = glyph;
        self
    }
    /// Sets the autonumber badge format string.
    pub fn autonumber_format(mut self, format: impl Into<Arc<str>>) -> Self {
        self.theme.autonumber.format = format.into();
        self
    }
    /// Toggles the outer border around the diagram.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }
    /// Sets the outer border line style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }
    /// Sets the outer padding of the diagram.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
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
    /// Caps the cell width of message labels (`None` = unbounded; minimum 1).
    pub fn max_label_cells(mut self, max_label_cells: Option<u16>) -> Self {
        self.max_label_cells = max_label_cells.map(|cells| cells.max(1));
        self
    }
    /// Sets how over-long message labels are handled.
    pub fn message_label_overflow(mut self, overflow: Overflow) -> Self {
        self.message_label_overflow = overflow;
        self
    }
    /// Toggles automatic numbering of messages.
    pub fn autonumber(mut self, autonumber: bool) -> Self {
        self.autonumber = autonumber;
        self
    }
    /// Sets the rendering variant without changing the theme.
    pub fn variant(mut self, variant: SequenceDiagramVariant) -> Self {
        self.variant = variant;
        self
    }
    /// Switches to the [`Minimal`](SequenceDiagramVariant::Minimal) variant and its theme.
    pub fn minimal(self) -> Self {
        self.variant(SequenceDiagramVariant::Minimal)
            .theme(SequenceDiagramTheme::minimal())
    }
    /// Switches to the [`Boxed`](SequenceDiagramVariant::Boxed) variant and the classic theme.
    pub fn boxed(self) -> Self {
        self.variant(SequenceDiagramVariant::Boxed)
            .theme(SequenceDiagramTheme::classic())
    }
    /// Sets the glyph prefix used for actor-kind headers.
    pub fn actor_glyph(mut self, glyph: impl Into<Arc<str>>) -> Self {
        self.actor_glyph = glyph.into();
        self
    }
    /// Repeats the participant headers at the bottom of the diagram.
    pub fn repeat_participants_at_bottom(mut self, repeat: bool) -> Self {
        self.repeat_participants_at_bottom = repeat;
        self
    }
    /// Sets a callback invoked when an item is clicked.
    pub fn on_item_click(mut self, cb: Callback<SequenceItemEvent>) -> Self {
        self.on_item_click = Some(cb);
        self
    }
    /// Sets a callback invoked when an item is hovered.
    pub fn on_item_hover(mut self, cb: Callback<SequenceItemEvent>) -> Self {
        self.on_item_hover = Some(cb);
        self
    }

    fn fragment(
        self,
        kind: FragmentKind,
        label: impl Into<Arc<str>>,
        f: impl FnOnce(Self) -> Self,
    ) -> Self {
        f(self.fragment_begin(kind, label)).fragment_end()
    }

    fn note_one(
        self,
        placement: NotePlacement,
        actor: impl Into<ActorRef>,
        text: impl Into<Arc<str>>,
    ) -> Self {
        self.step(SequenceStep::Note {
            placement,
            actors: Arc::<[ActorRef]>::from(vec![actor.into()]),
            text: text.into(),
            style: None,
        })
    }

    fn upsert_participant(&mut self, actor: ActorRef, label: Arc<str>, kind: ActorKind) {
        if let Some(participant) = self.participants.iter_mut().find(|p| p.actor == actor) {
            participant.label = label;
            participant.kind = kind;
        } else {
            self.participants
                .push(ParticipantSpec { actor, label, kind });
        }
    }
}

impl From<SequenceDiagram> for Element {
    fn from(value: SequenceDiagram) -> Self {
        Element::new(ElementKind::SequenceDiagram(Box::new(value)))
    }
}
