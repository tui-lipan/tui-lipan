use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::callback::Callback;
use crate::core::node::WidgetNode;
use crate::style::{BorderStyle, Length, Padding, Rect, Style, Theme};
use crate::widgets::Overflow;

use super::{
    ActorKind, FragmentKind, MessageStyle, ParticipantSpec, SequenceDiagram, SequenceDiagramTheme,
    SequenceDiagramVariant, SequenceItemEvent, SequenceItemPath, SequenceStep,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SequenceCacheKey {
    pub(crate) hash: u64,
}

impl SequenceCacheKey {
    pub(crate) fn new(diagram: &SequenceDiagram) -> Self {
        Self {
            hash: structural_hash(diagram),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SequenceWidgetKey {
    pub(crate) hash: u64,
}

impl SequenceWidgetKey {
    pub(crate) fn new(diagram: &SequenceDiagram) -> Self {
        Self {
            hash: widget_hash(diagram),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PositionedParticipant {
    pub(crate) rect: Rect,
    pub(crate) bottom_rect: Option<Rect>,
    pub(crate) label_rect: Rect,
    pub(crate) label: Arc<str>,
    pub(crate) display_label: Arc<str>,
    pub(crate) kind: ActorKind,
    pub(crate) path: SequenceItemPath,
}

#[derive(Clone, Debug)]
pub(crate) struct Lifeline {
    pub(crate) x: i16,
    pub(crate) y1: i16,
    pub(crate) y2: i16,
}

#[derive(Clone, Debug)]
pub(crate) struct PositionedMessage {
    pub(crate) line_rect: Rect,
    pub(crate) label_rect: Rect,
    pub(crate) from_x: i16,
    pub(crate) to_x: i16,
    pub(crate) y: i16,
    pub(crate) label: Arc<str>,
    pub(crate) display_lines: Arc<[Arc<str>]>,
    pub(crate) style: MessageStyle,
    pub(crate) line_style: Option<Style>,
    pub(crate) label_style: Option<Style>,
    pub(crate) path: SequenceItemPath,
}

#[derive(Clone, Debug)]
pub(crate) struct PositionedNote {
    pub(crate) rect: Rect,
    pub(crate) label_rect: Rect,
    pub(crate) text: Arc<str>,
    pub(crate) lines: Arc<[Arc<str>]>,
    pub(crate) style: Option<Style>,
    pub(crate) path: SequenceItemPath,
}

#[derive(Clone, Debug)]
pub(crate) struct PositionedFragmentBranch {
    pub(crate) y: i16,
    pub(crate) label: Arc<str>,
    pub(crate) label_rect: Rect,
}

#[derive(Clone, Debug)]
pub(crate) struct PositionedFragment {
    pub(crate) rect: Rect,
    pub(crate) label_rect: Rect,
    pub(crate) kind: FragmentKind,
    pub(crate) header_label: Arc<str>,
    pub(crate) branches: Vec<PositionedFragmentBranch>,
    pub(crate) style: Option<Style>,
    pub(crate) depth: usize,
    pub(crate) path: SequenceItemPath,
}

#[derive(Clone, Debug)]
pub(crate) struct PositionedActivation {
    pub(crate) rect: Rect,
}

#[derive(Clone, Debug)]
pub(crate) struct PositionedDivider {
    pub(crate) rect: Rect,
    pub(crate) label_rect: Rect,
    pub(crate) label: Arc<str>,
    pub(crate) path: SequenceItemPath,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SequenceRenderOutput {
    pub(crate) participants: Vec<PositionedParticipant>,
    pub(crate) lifelines: Vec<Lifeline>,
    pub(crate) fragments: Vec<PositionedFragment>,
    pub(crate) activations: Vec<PositionedActivation>,
    pub(crate) messages: Vec<PositionedMessage>,
    pub(crate) notes: Vec<PositionedNote>,
    pub(crate) dividers: Vec<PositionedDivider>,
    pub(crate) auto_numbers: Option<Vec<(SequenceItemPath, u16)>>,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

/// Runtime node for the [`crate::widgets::SequenceDiagram`] widget.
#[derive(Clone)]
pub struct SequenceDiagramNode {
    pub(crate) participants: Vec<ParticipantSpec>,
    pub(crate) steps: Vec<SequenceStep>,
    pub(crate) variant: SequenceDiagramVariant,
    pub(crate) actor_glyph: Arc<str>,
    pub(crate) style: Style,
    pub(crate) theme: Arc<SequenceDiagramTheme>,
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
    pub(crate) output: Arc<SequenceRenderOutput>,
    pub(crate) output_content_width: Option<u16>,
    pub(crate) cache_key: SequenceCacheKey,
    pub(crate) widget_key: SequenceWidgetKey,
}

impl Default for SequenceDiagramNode {
    fn default() -> Self {
        Self {
            participants: Vec::new(),
            steps: Vec::new(),
            variant: SequenceDiagramVariant::Boxed,
            actor_glyph: Arc::from("○ "),
            style: Style::default(),
            theme: Arc::new(SequenceDiagramTheme::classic()),
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
            output: Arc::new(SequenceRenderOutput::default()),
            output_content_width: None,
            cache_key: SequenceCacheKey { hash: 0 },
            widget_key: SequenceWidgetKey { hash: 0 },
        }
    }
}

impl From<SequenceDiagram> for SequenceDiagramNode {
    fn from(value: SequenceDiagram) -> Self {
        let mut node = Self::default();
        super::reconcile_sequence_diagram(&value, &mut node);
        node
    }
}

impl SequenceDiagramNode {
    pub(crate) fn content_rect(&self, rect: Rect) -> Rect {
        sequence_content_rect(rect, self.border, self.padding)
    }

    pub(crate) fn local_content_point(&self, rect: Rect, x: i16, y: i16) -> Option<(u16, u16)> {
        let content = self.content_rect(rect);
        if !content.contains(x, y) {
            return None;
        }
        Some((
            u16::try_from(x.saturating_sub(content.x)).ok()?,
            u16::try_from(y.saturating_sub(content.y)).ok()?,
        ))
    }

    pub(crate) fn hit_test(&self, local_x: u16, local_y: u16) -> Option<SequenceItemPath> {
        let x = i16::try_from(local_x).unwrap_or(i16::MAX);
        let y = i16::try_from(local_y).unwrap_or(i16::MAX);
        hit_test_output(&self.output, x, y, &self.theme.autonumber.format)
    }

    pub(crate) fn item_event(&self, path: SequenceItemPath) -> Option<SequenceItemEvent> {
        let label = item_label(&self.output, &path)?;
        Some(SequenceItemEvent { path, label })
    }
}

impl WidgetNode for SequenceDiagramNode {
    fn has_on_click(&self) -> bool {
        self.on_item_click.is_some()
    }

    fn is_hoverable(&self) -> bool {
        self.has_on_click() || self.on_item_hover.is_some() || !self.theme.hover_style.is_empty()
    }

    fn is_hoverable_for_theme(&self, _theme: &Theme) -> bool {
        self.is_hoverable()
    }

    fn hit_test_refinement(&self, x: i16, y: i16, rect: Rect) -> Option<bool> {
        self.on_item_click.as_ref()?;
        Some(
            self.local_content_point(rect, x, y)
                .and_then(|(local_x, local_y)| self.hit_test(local_x, local_y))
                .is_some(),
        )
    }

    fn hover_test_refinement(&self, x: i16, y: i16, rect: Rect) -> Option<bool> {
        if !self.is_hoverable() {
            return Some(false);
        }
        Some(
            self.local_content_point(rect, x, y)
                .and_then(|(local_x, local_y)| self.hit_test(local_x, local_y))
                .is_some(),
        )
    }
}

pub(crate) fn sequence_content_rect(rect: Rect, border: bool, padding: Padding) -> Rect {
    rect.inner(border, padding)
}

pub(crate) fn hit_test_output(
    output: &SequenceRenderOutput,
    x: i16,
    y: i16,
    autonumber_format: &str,
) -> Option<SequenceItemPath> {
    if let Some(numbers) = output.auto_numbers.as_ref() {
        for (path, number) in numbers {
            if autonumber_rect(path, *number, &output.messages, autonumber_format)
                .is_some_and(|rect| rect.contains(x, y))
            {
                return Some(path.clone());
            }
        }
    }
    for message in &output.messages {
        if message.label_rect.contains(x, y) || message.line_rect.contains(x, y) {
            return Some(message.path.clone());
        }
    }
    for note in &output.notes {
        if note.rect.contains(x, y) || note.label_rect.contains(x, y) {
            return Some(note.path.clone());
        }
    }
    for participant in &output.participants {
        if participant.rect.contains(x, y) || participant.label_rect.contains(x, y) {
            return Some(participant.path.clone());
        }
        if participant
            .bottom_rect
            .is_some_and(|rect| rect.contains(x, y))
        {
            return Some(participant.path.clone());
        }
    }
    for fragment in output.fragments.iter().rev() {
        if fragment.label_rect.contains(x, y) {
            return Some(fragment.path.clone());
        }
    }
    for divider in &output.dividers {
        if divider.rect.contains(x, y) || divider.label_rect.contains(x, y) {
            return Some(divider.path.clone());
        }
    }
    None
}

pub(crate) fn autonumber_rect(
    path: &SequenceItemPath,
    number: u16,
    messages: &[PositionedMessage],
    format: &str,
) -> Option<Rect> {
    let anchor = match path {
        SequenceItemPath::Message(index) | SequenceItemPath::SelfMessage(index) => {
            messages.get(*index).map(|message| message.line_rect)?
        }
        _ => return None,
    };
    let text = format.replace("{n}", &number.to_string());
    let w = text.chars().count().min(u16::MAX as usize) as u16;
    Some(Rect {
        x: anchor.x.saturating_sub(w as i16 + 1),
        y: anchor.y,
        w,
        h: 1,
    })
}

fn item_label(output: &SequenceRenderOutput, path: &SequenceItemPath) -> Option<Arc<str>> {
    match path {
        SequenceItemPath::Message(index) | SequenceItemPath::SelfMessage(index) => {
            output.messages.get(*index).map(|item| item.label.clone())
        }
        SequenceItemPath::Participant(index) => output
            .participants
            .get(*index)
            .map(|item| item.label.clone()),
        SequenceItemPath::Note(index) => output.notes.get(*index).map(|item| item.text.clone()),
        SequenceItemPath::Fragment(index) => output
            .fragments
            .get(*index)
            .map(|item| item.header_label.clone()),
        SequenceItemPath::Divider(index) => {
            output.dividers.get(*index).map(|item| item.label.clone())
        }
    }
}

fn structural_hash(diagram: &SequenceDiagram) -> u64 {
    let mut hasher = DefaultHasher::new();
    diagram.participants.hash(&mut hasher);
    diagram.steps.hash(&mut hasher);
    diagram.variant.hash(&mut hasher);
    diagram.actor_glyph.hash(&mut hasher);
    diagram.border.hash(&mut hasher);
    diagram.padding.hash(&mut hasher);
    diagram.max_label_cells.hash(&mut hasher);
    diagram.message_label_overflow.hash(&mut hasher);
    diagram.autonumber.hash(&mut hasher);
    diagram.repeat_participants_at_bottom.hash(&mut hasher);
    hasher.finish()
}

fn widget_hash(diagram: &SequenceDiagram) -> u64 {
    let mut hasher = DefaultHasher::new();
    structural_hash(diagram).hash(&mut hasher);
    diagram.style.hash(&mut hasher);
    diagram.theme.hash(&mut hasher);
    diagram.border_style.hash(&mut hasher);
    diagram.width.hash(&mut hasher);
    diagram.height.hash(&mut hasher);
    hasher.finish()
}
