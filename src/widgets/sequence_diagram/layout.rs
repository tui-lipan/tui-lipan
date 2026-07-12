use std::collections::HashMap;
use std::sync::Arc;

use crate::style::Rect;
use crate::widgets::Overflow;

use super::node::{
    Lifeline, PositionedActivation, PositionedDivider, PositionedFragment,
    PositionedFragmentBranch, PositionedMessage, PositionedNote, PositionedParticipant,
    SequenceRenderOutput,
};
use super::{
    ActorKind, ActorRef, FragmentKind, MessageStyle, NotePlacement, ParticipantSpec,
    SequenceDiagram, SequenceDiagramVariant, SequenceItemPath, SequenceMessage, SequenceStep,
};

const COLUMN_GAP: u16 = 4;
const HEADER_HEIGHT: u16 = 3;
const ACTOR_HEADER_HEIGHT: u16 = 4;
const STEP_GAP: i16 = 1;
const SELF_MESSAGE_LOOP_WIDTH: u16 = 4;

#[derive(Clone, Debug)]
struct Column {
    spec: ParticipantSpec,
    width: u16,
    right_gap: u16,
    center: i16,
    left: i16,
}

#[derive(Clone, Debug)]
struct OpenFragment {
    kind: FragmentKind,
    label: Arc<str>,
    style: Option<crate::style::Style>,
    y: i16,
    depth: usize,
    branches: Vec<(i16, FragmentKind, Arc<str>)>,
}

pub fn measure_sequence_diagram(diagram: &SequenceDiagram) -> (u16, u16) {
    let output = build_sequence_output(diagram);
    let mut w = output.width.saturating_add(diagram.padding.horizontal());
    let mut h = output.height.saturating_add(diagram.padding.vertical());
    if diagram.border {
        w = w.saturating_add(2);
        h = h.saturating_add(2);
    }
    (w, h)
}

pub(crate) fn build_sequence_output(diagram: &SequenceDiagram) -> SequenceRenderOutput {
    build_sequence_output_for_width(diagram, 0)
}

pub(crate) fn build_sequence_output_for_width(
    diagram: &SequenceDiagram,
    target_content_width: u16,
) -> SequenceRenderOutput {
    let mut columns = collect_columns(diagram);
    if columns.is_empty() && diagram.steps.is_empty() {
        return SequenceRenderOutput::default();
    }

    grow_gaps_for_labels(&mut columns, diagram);
    assign_column_positions(&mut columns);
    let (left_margin, right_margin) = compute_note_margins(diagram, &columns);
    let intrinsic_width = content_width(&columns)
        .saturating_add(left_margin)
        .saturating_add(right_margin)
        .max(1);
    if target_content_width > intrinsic_width {
        distribute_surplus_gap(
            &mut columns,
            target_content_width.saturating_sub(intrinsic_width),
        );
        assign_column_positions(&mut columns);
    }
    if left_margin > 0 {
        let shift = i16::try_from(left_margin).unwrap_or(i16::MAX);
        for column in &mut columns {
            column.left = column.left.saturating_add(shift);
            column.center = column.center.saturating_add(shift);
        }
    }

    let max_header_height = columns
        .iter()
        .map(|column| header_height(column.spec.kind, diagram.variant))
        .max()
        .unwrap_or(0);
    let content_width = content_width(&columns)
        .saturating_add(right_margin)
        .max(target_content_width)
        .max(1);
    let mut current_y = i16::try_from(max_header_height).unwrap_or(i16::MAX);
    if !columns.is_empty() {
        current_y = current_y.saturating_add(STEP_GAP);
    }

    let mut output = SequenceRenderOutput::default();
    let mut fragment_stack = Vec::<OpenFragment>::new();
    let mut activation_stack = HashMap::<ActorRef, Vec<i16>>::new();
    let mut auto_numbers = diagram.autonumber.then(Vec::new);

    for step in &diagram.steps {
        match step {
            SequenceStep::Message(message) => {
                let index = output.messages.len();
                let path = SequenceItemPath::Message(index);
                let positioned =
                    position_message(message, diagram, &columns, current_y, path.clone());
                let message_y = positioned.y;
                let message_bottom = message_block_bottom(&positioned);
                output.messages.push(positioned);
                if let Some(numbers) = auto_numbers.as_mut() {
                    numbers.push((path, u16::try_from(numbers.len() + 1).unwrap_or(u16::MAX)));
                }
                if message.activate_target {
                    activation_stack
                        .entry(message.to.clone())
                        .or_default()
                        .push(message_y);
                }
                if message.deactivate_source {
                    close_activation(
                        &message.from,
                        message_y,
                        &mut activation_stack,
                        &mut output.activations,
                        &columns,
                    );
                }
                current_y = message_bottom.saturating_add(1);
            }
            SequenceStep::SelfMessage {
                actor,
                label,
                style,
            } => {
                let index = output.messages.len();
                let path = SequenceItemPath::SelfMessage(index);
                let positioned = position_self_message(
                    actor,
                    label.clone(),
                    *style,
                    diagram,
                    &columns,
                    current_y,
                    path.clone(),
                );
                let message_bottom = message_block_bottom(&positioned);
                output.messages.push(positioned);
                if let Some(numbers) = auto_numbers.as_mut() {
                    numbers.push((path, u16::try_from(numbers.len() + 1).unwrap_or(u16::MAX)));
                }
                current_y = message_bottom.saturating_add(1);
            }
            SequenceStep::Note {
                placement,
                actors,
                text,
                style,
            } => {
                let index = output.notes.len();
                output.notes.push(position_note(NoteLayoutInput {
                    placement: *placement,
                    actors,
                    text: text.clone(),
                    style: *style,
                    diagram,
                    columns: &columns,
                    y: current_y,
                    path: SequenceItemPath::Note(index),
                }));
                current_y = current_y.saturating_add(
                    i16::try_from(output.notes.last().map_or(3, |note| note.rect.h))
                        .unwrap_or(i16::MAX),
                );
                current_y = current_y.saturating_add(STEP_GAP);
            }
            SequenceStep::Activate(actor) => {
                activation_stack
                    .entry(actor.clone())
                    .or_default()
                    .push(current_y);
            }
            SequenceStep::Deactivate(actor) => {
                close_activation(
                    actor,
                    current_y,
                    &mut activation_stack,
                    &mut output.activations,
                    &columns,
                );
            }
            SequenceStep::FragmentBegin {
                kind, label, style, ..
            } => {
                let depth = fragment_stack.len();
                fragment_stack.push(OpenFragment {
                    kind: *kind,
                    label: label.clone(),
                    style: *style,
                    y: current_y,
                    depth,
                    branches: Vec::new(),
                });
                current_y = current_y.saturating_add(2);
            }
            SequenceStep::FragmentBranch { kind, label } => {
                if let Some(fragment) = fragment_stack.last_mut() {
                    fragment.branches.push((current_y, *kind, label.clone()));
                }
                current_y = current_y.saturating_add(2);
            }
            SequenceStep::FragmentEnd => {
                if let Some(fragment) = fragment_stack.pop() {
                    push_fragment(&mut output.fragments, fragment, current_y, content_width);
                }
                current_y = current_y.saturating_add(1);
            }
            SequenceStep::Rect { color } => {
                let fragment = OpenFragment {
                    kind: FragmentKind::Rect,
                    label: Arc::from("rect"),
                    style: Some(*color),
                    y: current_y,
                    depth: fragment_stack.len(),
                    branches: Vec::new(),
                };
                current_y = current_y.saturating_add(2);
                push_fragment(&mut output.fragments, fragment, current_y, content_width);
            }
            SequenceStep::Divider(label) => {
                let index = output.dividers.len();
                output.dividers.push(PositionedDivider {
                    rect: Rect {
                        x: 0,
                        y: current_y,
                        w: content_width,
                        h: 1,
                    },
                    label_rect: centered_rect(0, content_width, current_y, label_width(label), 1),
                    label: label.clone(),
                    path: SequenceItemPath::Divider(index),
                });
                current_y = current_y.saturating_add(2);
            }
        }
    }

    while let Some(fragment) = fragment_stack.pop() {
        push_fragment(&mut output.fragments, fragment, current_y, content_width);
    }
    for (actor, starts) in activation_stack {
        for start in starts {
            push_activation(&actor, start, current_y, &mut output.activations, &columns);
        }
    }

    let bottom_header_top = if diagram.repeat_participants_at_bottom && !columns.is_empty() {
        Some(current_y.saturating_add(1))
    } else {
        None
    };
    let height = bottom_header_top
        .map(|y| y.saturating_add(i16::try_from(max_header_height).unwrap_or(i16::MAX)))
        .unwrap_or(current_y.max(i16::try_from(max_header_height).unwrap_or(i16::MAX)));

    output.participants = position_participants(
        &columns,
        diagram.variant,
        &diagram.actor_glyph,
        max_header_height,
        bottom_header_top,
    );
    output.lifelines = columns
        .iter()
        .map(|column| Lifeline {
            x: column.center,
            y1: i16::try_from(max_header_height).unwrap_or(i16::MAX),
            y2: height.saturating_sub(1),
        })
        .collect();
    let content_width = content_width.max(rendered_content_width(&output));
    for fragment in &mut output.fragments {
        fragment.rect.w = content_width
            .saturating_sub(u16::try_from(fragment.depth * 2).unwrap_or(u16::MAX))
            .max(1);
    }
    for divider in &mut output.dividers {
        divider.rect.w = content_width;
        divider.label_rect = centered_rect(
            0,
            content_width,
            divider.rect.y,
            label_width(&divider.label),
            1,
        );
    }

    output.fragments.sort_by_key(|fragment| fragment.depth);
    for (index, fragment) in output.fragments.iter_mut().enumerate() {
        fragment.path = SequenceItemPath::Fragment(index);
    }
    output.auto_numbers = auto_numbers;
    output.width = content_width;
    output.height = u16::try_from(height.max(0)).unwrap_or(u16::MAX);
    output
}

fn collect_columns(diagram: &SequenceDiagram) -> Vec<Column> {
    let mut specs = diagram.participants.clone();
    for step in &diagram.steps {
        for actor in step_actors(step) {
            if !specs.iter().any(|spec| spec.actor == *actor) {
                specs.push(ParticipantSpec {
                    actor: actor.clone(),
                    label: actor.0.clone(),
                    kind: ActorKind::Participant,
                });
            }
        }
    }
    specs
        .into_iter()
        .map(|spec| Column {
            width: participant_width(&spec, diagram),
            right_gap: COLUMN_GAP,
            spec,
            center: 0,
            left: 0,
        })
        .collect()
}

fn step_actors(step: &SequenceStep) -> Vec<&ActorRef> {
    match step {
        SequenceStep::Message(message) => vec![&message.from, &message.to],
        SequenceStep::SelfMessage { actor, .. } => vec![actor],
        SequenceStep::Note { actors, .. } => actors.iter().collect(),
        SequenceStep::Activate(actor) | SequenceStep::Deactivate(actor) => vec![actor],
        _ => Vec::new(),
    }
}

fn grow_gaps_for_labels(columns: &mut [Column], diagram: &SequenceDiagram) {
    for step in &diagram.steps {
        match step {
            SequenceStep::Message(message) => grow_span(
                columns,
                &message.from,
                &message.to,
                display_label_width(&message.label, diagram).saturating_add(4),
            ),
            SequenceStep::SelfMessage { actor, label, .. } => {
                grow_self(
                    columns,
                    actor,
                    SELF_MESSAGE_LOOP_WIDTH
                        .saturating_add(1)
                        .saturating_add(display_label_width(label, diagram)),
                );
            }
            SequenceStep::Note { actors, text, .. } => {
                if let (Some(first), Some(last)) = (actors.first(), actors.last()) {
                    grow_span(
                        columns,
                        first,
                        last,
                        wrapped_label_width(text, diagram).saturating_add(4),
                    );
                }
            }
            SequenceStep::Divider(label) => {
                if let Some(first) = columns.first().map(|c| c.spec.actor.clone())
                    && let Some(last) = columns.last().map(|c| c.spec.actor.clone())
                {
                    grow_span(
                        columns,
                        &first,
                        &last,
                        display_label_width(label, diagram).saturating_add(4),
                    );
                }
            }
            _ => {}
        }
    }
}

fn grow_span(columns: &mut [Column], from: &ActorRef, to: &ActorRef, required: u16) {
    let Some(a) = column_index(columns, from) else {
        return;
    };
    let Some(b) = column_index(columns, to) else {
        return;
    };
    let (start, end) = if a <= b { (a, b) } else { (b, a) };
    let current = columns[start..=end]
        .iter()
        .map(|column| column.width)
        .fold(0u16, u16::saturating_add)
        .saturating_add(
            columns[start..end]
                .iter()
                .map(|column| column.right_gap)
                .fold(0u16, u16::saturating_add),
        );
    if required <= current {
        return;
    }
    let gap_count = end.saturating_sub(start);
    if gap_count == 0 {
        return;
    }
    let count = u16::try_from(gap_count).unwrap_or(u16::MAX).max(1);
    let per_gap = required.saturating_sub(current).saturating_add(count - 1) / count;
    for column in &mut columns[start..end] {
        column.right_gap = column.right_gap.saturating_add(per_gap);
    }
}

fn compute_note_margins(diagram: &SequenceDiagram, columns: &[Column]) -> (u16, u16) {
    if columns.is_empty() {
        return (0, 0);
    }
    let first_col = &columns[0];
    let last_col = &columns[columns.len() - 1];
    let first_actor = &first_col.spec.actor;
    let last_actor = &last_col.spec.actor;
    let mut left = 0u16;
    let mut right = 0u16;
    for step in &diagram.steps {
        let SequenceStep::Note {
            placement,
            actors,
            text,
            ..
        } = step
        else {
            continue;
        };
        let lines = wrap_label(text, diagram);
        let line_w = lines
            .iter()
            .map(|line| label_width(line))
            .max()
            .unwrap_or(0);
        let note_w = line_w.saturating_add(4).max(6);
        match placement {
            NotePlacement::LeftOf if actors.first() == Some(first_actor) => {
                let half = first_col.width / 2;
                let needed = note_w.saturating_add(1).saturating_sub(half);
                left = left.max(needed);
            }
            NotePlacement::RightOf if actors.first() == Some(last_actor) => {
                let half = last_col.width / 2;
                let needed = note_w.saturating_add(1).saturating_sub(half);
                right = right.max(needed);
            }
            _ => {}
        }
    }
    for step in &diagram.steps {
        if let SequenceStep::SelfMessage { actor, label, .. } = step
            && actor == last_actor
        {
            let label_w = display_label_width(label, diagram);
            let extent = SELF_MESSAGE_LOOP_WIDTH
                .saturating_add(1)
                .saturating_add(label_w);
            let half = last_col.width / 2;
            right = right.max(extent.saturating_sub(half));
        }
    }
    (left, right)
}

fn grow_self(columns: &mut [Column], actor: &ActorRef, extent: u16) {
    let Some(index) = column_index(columns, actor) else {
        return;
    };
    if index + 1 >= columns.len() {
        return;
    }
    let half = columns[index].width / 2;
    let needed = extent.saturating_sub(half);
    if columns[index].right_gap < needed {
        columns[index].right_gap = needed;
    }
}

fn distribute_surplus_gap(columns: &mut [Column], surplus: u16) {
    let gap_count = columns.len().saturating_sub(1);
    if gap_count == 0 || surplus == 0 {
        return;
    }
    let count = u16::try_from(gap_count).unwrap_or(u16::MAX).max(1);
    let per_gap = surplus / count;
    let mut remainder = surplus % count;
    for column in &mut columns[..gap_count] {
        column.right_gap = column.right_gap.saturating_add(per_gap);
        if remainder > 0 {
            column.right_gap = column.right_gap.saturating_add(1);
            remainder -= 1;
        }
    }
}

fn assign_column_positions(columns: &mut [Column]) {
    let mut x = 0i16;
    for column in columns {
        column.left = x;
        column.center = x.saturating_add(i16::try_from(column.width / 2).unwrap_or(i16::MAX));
        x = x
            .saturating_add(i16::try_from(column.width).unwrap_or(i16::MAX))
            .saturating_add(i16::try_from(column.right_gap).unwrap_or(i16::MAX));
    }
}

fn position_participants(
    columns: &[Column],
    variant: SequenceDiagramVariant,
    actor_glyph: &Arc<str>,
    max_header_height: u16,
    bottom_y: Option<i16>,
) -> Vec<PositionedParticipant> {
    columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let h = header_height(column.spec.kind, variant);
            let y = i16::try_from(max_header_height.saturating_sub(h)).unwrap_or(i16::MAX);
            let rect = Rect {
                x: column.left,
                y,
                w: column.width,
                h,
            };
            let bottom_rect = bottom_y.map(|y| Rect { y, ..rect });
            let display_label = participant_display_label(&column.spec, variant, actor_glyph);
            let label_y = match (variant, column.spec.kind) {
                (SequenceDiagramVariant::Minimal, _) => rect.y,
                (SequenceDiagramVariant::Boxed, ActorKind::Participant) => rect.y.saturating_add(1),
                (SequenceDiagramVariant::Boxed, ActorKind::Actor) => {
                    rect.y.saturating_add(rect.h as i16 - 1)
                }
            };
            PositionedParticipant {
                rect,
                bottom_rect,
                label_rect: centered_rect(
                    column.left,
                    column.width,
                    label_y,
                    label_width(&display_label),
                    1,
                ),
                label: column.spec.label.clone(),
                display_label,
                kind: column.spec.kind,
                path: SequenceItemPath::Participant(index),
            }
        })
        .collect()
}

fn position_message(
    message: &SequenceMessage,
    diagram: &SequenceDiagram,
    columns: &[Column],
    y: i16,
    path: SequenceItemPath,
) -> PositionedMessage {
    let from_x = actor_center(columns, &message.from);
    let to_x = actor_center(columns, &message.to);
    let left = from_x.min(to_x);
    let right = from_x.max(to_x);
    let line_width = u16::try_from((right - left).max(1))
        .unwrap_or(u16::MAX)
        .saturating_add(1);
    let label_capacity = message_label_capacity(line_width, diagram.variant);
    let display_lines = message_display_lines(&message.label, diagram, label_capacity);
    let label_h = u16::try_from(display_lines.len())
        .unwrap_or(u16::MAX)
        .max(1);
    let label_w = message_label_rect_width(&display_lines, label_capacity);
    let line_y = y.saturating_sub(1).saturating_add(label_h as i16);
    let label_rect = match diagram.variant {
        SequenceDiagramVariant::Boxed => {
            centered_rect(left, line_width, y.saturating_sub(1), label_w, label_h)
        }
        SequenceDiagramVariant::Minimal => {
            start_aligned_rect(left, line_width, y.saturating_sub(1), label_w, label_h)
        }
    };
    PositionedMessage {
        line_rect: Rect {
            x: left,
            y: line_y,
            w: line_width,
            h: 1,
        },
        label_rect,
        from_x,
        to_x,
        y: line_y,
        label: message.label.clone(),
        display_lines: Arc::<[Arc<str>]>::from(display_lines),
        style: message.style,
        line_style: message.line_style,
        label_style: message.label_style,
        path,
    }
}

fn position_self_message(
    actor: &ActorRef,
    label: Arc<str>,
    style: Option<crate::style::Style>,
    diagram: &SequenceDiagram,
    columns: &[Column],
    y: i16,
    path: SequenceItemPath,
) -> PositionedMessage {
    let from_x = actor_center(columns, actor);
    let label_capacity = self_message_label_capacity(&label, diagram);
    let display_lines = message_display_lines(&label, diagram, label_capacity);
    let label_h = u16::try_from(display_lines.len())
        .unwrap_or(u16::MAX)
        .max(1);
    let label_w = self_message_label_rect_width(&display_lines, label_capacity);
    PositionedMessage {
        line_rect: Rect {
            x: from_x,
            y,
            w: SELF_MESSAGE_LOOP_WIDTH,
            h: 2,
        },
        label_rect: Rect {
            x: from_x.saturating_add(SELF_MESSAGE_LOOP_WIDTH as i16 + 1),
            y: y.saturating_add(1),
            w: label_w,
            h: label_h,
        },
        from_x,
        to_x: from_x,
        y,
        label,
        display_lines: Arc::<[Arc<str>]>::from(display_lines),
        style: MessageStyle::Sync,
        line_style: style,
        label_style: style,
        path,
    }
}

struct NoteLayoutInput<'a> {
    placement: NotePlacement,
    actors: &'a [ActorRef],
    text: Arc<str>,
    style: Option<crate::style::Style>,
    diagram: &'a SequenceDiagram,
    columns: &'a [Column],
    y: i16,
    path: SequenceItemPath,
}

fn position_note(input: NoteLayoutInput<'_>) -> PositionedNote {
    let lines = wrap_label(&input.text, input.diagram);
    let line_width = lines
        .iter()
        .map(|line| label_width(line))
        .max()
        .unwrap_or(0);
    let width = line_width.saturating_add(4).max(6);
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let first = input
        .actors
        .first()
        .map(|actor| actor_center(input.columns, actor))
        .unwrap_or(0);
    let last = input
        .actors
        .last()
        .map(|actor| actor_center(input.columns, actor))
        .unwrap_or(first);
    let x = match input.placement {
        NotePlacement::Over => first
            .min(last)
            .saturating_add((first.max(last) - first.min(last)) / 2)
            .saturating_sub(i16::try_from(width / 2).unwrap_or(i16::MAX)),
        NotePlacement::LeftOf => first
            .saturating_sub(i16::try_from(width).unwrap_or(i16::MAX))
            .saturating_sub(1),
        NotePlacement::RightOf => first.saturating_add(1),
    };
    PositionedNote {
        rect: Rect {
            x: x.max(0),
            y: input.y,
            w: width,
            h: height,
        },
        label_rect: Rect {
            x: x.max(0).saturating_add(2),
            y: input.y.saturating_add(1),
            w: line_width,
            h: height.saturating_sub(2),
        },
        text: input.text,
        lines: Arc::<[Arc<str>]>::from(lines),
        style: input.style,
        path: input.path,
    }
}

fn push_fragment(
    fragments: &mut Vec<PositionedFragment>,
    fragment: OpenFragment,
    end_y: i16,
    width: u16,
) {
    let index = fragments.len();
    let label = fragment_label(fragment.kind, &fragment.label);
    let rect = Rect {
        x: i16::try_from(fragment.depth).unwrap_or(i16::MAX),
        y: fragment.y,
        w: width
            .saturating_sub(u16::try_from(fragment.depth * 2).unwrap_or(u16::MAX))
            .max(1),
        h: u16::try_from(end_y.saturating_sub(fragment.y).max(1)).unwrap_or(u16::MAX),
    };
    let branches = fragment
        .branches
        .into_iter()
        .map(|(y, _, label)| PositionedFragmentBranch {
            y,
            label_rect: Rect {
                x: rect.x.saturating_add(2),
                y,
                w: label_width(&label),
                h: 1,
            },
            label,
        })
        .collect();
    fragments.push(PositionedFragment {
        rect,
        label_rect: Rect {
            x: rect.x.saturating_add(1),
            y: rect.y,
            w: label_width(&label),
            h: 1,
        },
        kind: fragment.kind,
        header_label: label,
        branches,
        style: fragment.style,
        depth: fragment.depth,
        path: SequenceItemPath::Fragment(index),
    });
}

fn close_activation(
    actor: &ActorRef,
    y: i16,
    stacks: &mut HashMap<ActorRef, Vec<i16>>,
    activations: &mut Vec<PositionedActivation>,
    columns: &[Column],
) {
    if let Some(start) = stacks.get_mut(actor).and_then(Vec::pop) {
        push_activation(actor, start, y, activations, columns);
    }
}

fn push_activation(
    actor: &ActorRef,
    start: i16,
    end: i16,
    activations: &mut Vec<PositionedActivation>,
    columns: &[Column],
) {
    let center = actor_center(columns, actor);
    activations.push(PositionedActivation {
        rect: Rect {
            x: center.saturating_sub(1),
            y: start,
            w: 3,
            h: u16::try_from(end.saturating_sub(start).max(1)).unwrap_or(u16::MAX),
        },
    });
}

fn participant_width(spec: &ParticipantSpec, diagram: &SequenceDiagram) -> u16 {
    match diagram.variant {
        SequenceDiagramVariant::Boxed => label_width(&spec.label).saturating_add(4).max(5),
        SequenceDiagramVariant::Minimal => label_width(&participant_display_label(
            spec,
            diagram.variant,
            &diagram.actor_glyph,
        ))
        .max(3),
    }
}

fn participant_display_label(
    spec: &ParticipantSpec,
    variant: SequenceDiagramVariant,
    actor_glyph: &Arc<str>,
) -> Arc<str> {
    match (variant, spec.kind) {
        (SequenceDiagramVariant::Minimal, ActorKind::Actor) => {
            Arc::from(format!("{actor_glyph}{}", spec.label))
        }
        _ => spec.label.clone(),
    }
}

fn label_width(label: &str) -> u16 {
    label.chars().count().min(u16::MAX as usize) as u16
}

fn max_label_cells(diagram: &SequenceDiagram) -> Option<u16> {
    diagram.max_label_cells.map(|cells| cells.max(1))
}

fn display_label(label: &Arc<str>, diagram: &SequenceDiagram) -> Arc<str> {
    match max_label_cells(diagram) {
        Some(max) => truncate_label(label, max),
        None => label.clone(),
    }
}

fn display_label_width(label: &Arc<str>, diagram: &SequenceDiagram) -> u16 {
    label_width(&display_label(label, diagram))
}

fn message_label_capacity(line_width: u16, variant: SequenceDiagramVariant) -> u16 {
    match variant {
        SequenceDiagramVariant::Boxed => line_width,
        SequenceDiagramVariant::Minimal => {
            line_width.saturating_sub(if line_width > 4 { 2 } else { 0 })
        }
    }
    .max(1)
}

fn self_message_label_capacity(label: &Arc<str>, diagram: &SequenceDiagram) -> u16 {
    max_label_cells(diagram)
        .unwrap_or_else(|| label_width(label))
        .max(1)
}

fn message_label_rect_width(lines: &[Arc<str>], capacity: u16) -> u16 {
    lines
        .iter()
        .map(|line| label_width(line))
        .max()
        .unwrap_or(0)
        .min(capacity)
        .max(1)
}

fn self_message_label_rect_width(lines: &[Arc<str>], capacity: u16) -> u16 {
    lines
        .iter()
        .map(|line| label_width(line))
        .max()
        .unwrap_or(0)
        .min(capacity)
        .max(1)
}

fn message_display_lines(
    label: &Arc<str>,
    diagram: &SequenceDiagram,
    capacity: u16,
) -> Vec<Arc<str>> {
    let capacity = capacity.max(1);
    match diagram.message_label_overflow {
        Overflow::Wrap => wrap_words(label, capacity as usize)
            .into_iter()
            .map(Arc::from)
            .collect(),
        Overflow::Clip => vec![clip_label(label, capacity)],
        Overflow::ClipStart => vec![clip_label_start(label, capacity)],
        Overflow::Auto | Overflow::Ellipsis => vec![truncate_label(label, capacity)],
    }
}

fn clip_label(label: &Arc<str>, max: u16) -> Arc<str> {
    let max = max as usize;
    if label.chars().count() <= max {
        return label.clone();
    }
    Arc::from(label.chars().take(max).collect::<String>())
}

fn clip_label_start(label: &Arc<str>, max: u16) -> Arc<str> {
    let max = max as usize;
    let len = label.chars().count();
    if len <= max {
        return label.clone();
    }
    Arc::from(
        label
            .chars()
            .skip(len.saturating_sub(max))
            .collect::<String>(),
    )
}

fn message_block_bottom(message: &PositionedMessage) -> i16 {
    rect_bottom(message.line_rect).max(rect_bottom(message.label_rect))
}

fn rect_bottom(rect: Rect) -> i16 {
    rect.y.saturating_add(rect.h as i16)
}

fn wrapped_label_width(label: &Arc<str>, diagram: &SequenceDiagram) -> u16 {
    wrap_label(label, diagram)
        .iter()
        .map(|line| label_width(line))
        .max()
        .unwrap_or(0)
}

fn truncate_label(label: &Arc<str>, max: u16) -> Arc<str> {
    let max = max as usize;
    let len = label.chars().count();
    if len <= max {
        return label.clone();
    }
    if max == 1 {
        return Arc::from("…");
    }
    let mut value = String::new();
    value.extend(label.chars().take(max.saturating_sub(1)));
    value.push('…');
    Arc::from(value)
}

fn wrap_label(label: &Arc<str>, diagram: &SequenceDiagram) -> Vec<Arc<str>> {
    let Some(max) = max_label_cells(diagram) else {
        return vec![label.clone()];
    };
    wrap_words(label, max as usize)
        .into_iter()
        .map(Arc::from)
        .collect()
}

fn wrap_words(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        if current.is_empty() {
            if word_len <= max {
                current.push_str(word);
            } else {
                lines.extend(chunk_word(word, max));
            }
            continue;
        }

        let next_len = current
            .chars()
            .count()
            .saturating_add(1)
            .saturating_add(word_len);
        if next_len <= max {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            if word_len <= max {
                current.push_str(word);
            } else {
                lines.extend(chunk_word(word, max));
            }
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn chunk_word(word: &str, max: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in word.chars() {
        if current.chars().count() == max {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn header_height(kind: ActorKind, variant: SequenceDiagramVariant) -> u16 {
    match (variant, kind) {
        (SequenceDiagramVariant::Minimal, _) => 2,
        (SequenceDiagramVariant::Boxed, ActorKind::Participant) => HEADER_HEIGHT,
        (SequenceDiagramVariant::Boxed, ActorKind::Actor) => ACTOR_HEADER_HEIGHT,
    }
}

fn content_width(columns: &[Column]) -> u16 {
    columns
        .last()
        .map(|column| {
            u16::try_from(column.left.max(0))
                .unwrap_or(u16::MAX)
                .saturating_add(column.width)
        })
        .unwrap_or(0)
}

fn rendered_content_width(output: &SequenceRenderOutput) -> u16 {
    fn right(rect: Rect) -> u16 {
        u16::try_from(rect.x.max(0))
            .unwrap_or(u16::MAX)
            .saturating_add(rect.w)
    }

    let mut width = 0u16;
    for participant in &output.participants {
        width = width.max(right(participant.rect));
        if let Some(bottom_rect) = participant.bottom_rect {
            width = width.max(right(bottom_rect));
        }
        width = width.max(right(participant.label_rect));
    }
    for message in &output.messages {
        width = width.max(right(message.line_rect));
        width = width.max(right(message.label_rect));
    }
    for note in &output.notes {
        width = width.max(right(note.rect));
        width = width.max(right(note.label_rect));
    }
    for fragment in &output.fragments {
        width = width.max(right(fragment.rect));
        width = width.max(right(fragment.label_rect));
        for branch in &fragment.branches {
            width = width.max(right(branch.label_rect));
        }
    }
    for divider in &output.dividers {
        width = width.max(right(divider.rect));
        width = width.max(right(divider.label_rect));
    }
    width
}

fn column_index(columns: &[Column], actor: &ActorRef) -> Option<usize> {
    columns
        .iter()
        .position(|column| &column.spec.actor == actor)
}

fn actor_center(columns: &[Column], actor: &ActorRef) -> i16 {
    column_index(columns, actor)
        .map(|index| columns[index].center)
        .unwrap_or(0)
}

fn centered_rect(x: i16, width: u16, y: i16, label_width: u16, height: u16) -> Rect {
    let label_x =
        x.saturating_add(i16::try_from(width.saturating_sub(label_width) / 2).unwrap_or(i16::MAX));
    Rect {
        x: label_x,
        y,
        w: label_width.min(width),
        h: height,
    }
}

fn start_aligned_rect(x: i16, width: u16, y: i16, label_width: u16, height: u16) -> Rect {
    let inset = if width > 4 { 2 } else { 0 };
    Rect {
        x: x.saturating_add(inset),
        y,
        w: label_width.min(width.saturating_sub(inset as u16)),
        h: height,
    }
}

fn fragment_label(kind: FragmentKind, label: &Arc<str>) -> Arc<str> {
    let prefix = match kind {
        FragmentKind::Loop => "loop",
        FragmentKind::Alt => "alt",
        FragmentKind::Opt => "opt",
        FragmentKind::Par => "par",
        FragmentKind::Critical => "critical",
        FragmentKind::Break => "break",
        FragmentKind::Rect => "rect",
    };
    if label.is_empty() {
        Arc::from(prefix)
    } else {
        Arc::from(format!("{prefix} {label}"))
    }
}

#[cfg(test)]
mod tests {
    use crate::core::node::WidgetNode;
    use crate::style::{Color, Padding, Style};

    use super::SequenceMessage as Message;
    use super::*;

    #[test]
    fn empty_diagram_measures_to_chrome_only() {
        assert_eq!(
            measure_sequence_diagram(&SequenceDiagram::new().padding(1).border(true)),
            (4, 4)
        );
    }

    #[test]
    fn single_message_two_participants() {
        let output = build_sequence_output(
            &SequenceDiagram::new().message(Message::sync("Alice", "Bob", "Hello")),
        );
        assert_eq!(output.participants.len(), 2);
        assert_eq!(output.messages.len(), 1);
        assert!(output.height > HEADER_HEIGHT);
    }

    #[test]
    fn participant_label_sits_inside_box_not_on_border() {
        let output = build_sequence_output(&SequenceDiagram::new().participant("Coordinator"));
        let participant = &output.participants[0];
        assert_eq!(participant.rect.h, 3);
        assert_eq!(participant.label_rect.y, participant.rect.y + 1);
    }

    #[test]
    fn default_variant_is_boxed() {
        let diagram = SequenceDiagram::new().participant("Browser");
        assert_eq!(diagram.variant, SequenceDiagramVariant::Boxed);
        let output = build_sequence_output(&diagram);
        assert_eq!(output.participants[0].rect.h, HEADER_HEIGHT);
    }

    #[test]
    fn minimal_header_is_compact_with_label_on_first_row() {
        let output =
            build_sequence_output(&SequenceDiagram::new().minimal().participant("Browser"));
        let participant = &output.participants[0];
        assert_eq!(participant.rect.h, 2);
        assert_eq!(participant.rect.y, 0);
        assert_eq!(participant.label_rect.y, 0);
        assert_eq!(output.lifelines[0].y1, 2);
        assert_eq!(participant.display_label.as_ref(), "Browser");
    }

    #[test]
    fn minimal_actor_glyph_affects_display_label_and_width() {
        let plain = build_sequence_output(
            &SequenceDiagram::new()
                .minimal()
                .actor_kind("User", ActorKind::Actor),
        );
        let custom = build_sequence_output(
            &SequenceDiagram::new()
                .minimal()
                .actor_glyph("ACT ")
                .actor_kind("User", ActorKind::Actor),
        );
        assert_eq!(plain.participants[0].display_label.as_ref(), "○ User");
        assert_eq!(custom.participants[0].display_label.as_ref(), "ACT User");
        assert_eq!(plain.participants[0].rect.w, 6);
        assert_eq!(custom.participants[0].rect.w, 8);
    }

    #[test]
    fn minimal_message_labels_align_to_span_start() {
        let output = build_sequence_output(
            &SequenceDiagram::new()
                .minimal()
                .participant("Browser")
                .participant("Server")
                .message(Message::sync("Browser", "Server", "GET /"))
                .message(Message::reply("Server", "Browser", "401 WWW-Auth")),
        );

        assert_eq!(
            output.messages[0].label_rect.x,
            output.messages[0].line_rect.x + 2
        );
        assert_eq!(
            output.messages[1].label_rect.x,
            output.messages[1].line_rect.x + 2
        );
    }

    #[test]
    fn variant_reconcile_rebuilds_output() {
        let mut node = crate::widgets::internal::SequenceDiagramNode::from(
            SequenceDiagram::new().participant("Browser"),
        );
        assert_eq!(node.output.participants[0].rect.h, HEADER_HEIGHT);

        let changed = crate::widgets::sequence_diagram::reconcile_sequence_diagram(
            &SequenceDiagram::new().minimal().participant("Browser"),
            &mut node,
        );

        assert!(changed);
        assert_eq!(node.variant, SequenceDiagramVariant::Minimal);
        assert_eq!(node.output.participants[0].rect.h, 2);
    }

    #[test]
    fn auto_added_participant_appears_after_declared() {
        let output = build_sequence_output(
            &SequenceDiagram::new()
                .participant("Alice")
                .participant("Bob")
                .message(Message::sync("Alice", "Carol", "Hello")),
        );
        assert_eq!(output.participants[2].label.as_ref(), "Carol");
    }

    #[test]
    fn nested_fragments_layer_correctly() {
        let output = build_sequence_output(&SequenceDiagram::new().loop_("pending", |b| {
            b.alt("ok", |b| {
                b.message(Message::sync("Alice", "Bob", "ping"))
                    .else_("err")
            })
        }));
        assert_eq!(output.fragments.len(), 2);
        assert!(output.fragments[0].depth < output.fragments[1].depth);
    }

    #[test]
    fn activation_span_matches_activate_deactivate() {
        let output = build_sequence_output(
            &SequenceDiagram::new()
                .participant("Bob")
                .activate("Bob")
                .message(Message::sync("Bob", "Bob", "work"))
                .deactivate("Bob"),
        );
        assert_eq!(output.activations.len(), 1);
        assert!(output.activations[0].rect.h >= 1);
    }

    #[test]
    fn implicit_activation_from_message_postfix() {
        let output = build_sequence_output(
            &SequenceDiagram::new()
                .message(Message::sync("Alice", "Bob", "call").activate_target(true))
                .deactivate("Bob"),
        );
        assert_eq!(output.activations.len(), 1);
    }

    #[test]
    fn column_widths_grow_to_fit_long_message() {
        let short =
            build_sequence_output(&SequenceDiagram::new().message(Message::sync("A", "B", "x")));
        let long = build_sequence_output(&SequenceDiagram::new().message(Message::sync(
            "A",
            "B",
            "a very long message label",
        )));
        assert!(long.width > short.width);
    }

    #[test]
    fn long_span_labels_widen_gap_not_participant_boxes() {
        let output = build_sequence_output(
            &SequenceDiagram::new()
                .participant("A")
                .participant("B")
                .message(Message::sync("A", "B", "a very long message label")),
        );
        assert_eq!(output.participants[0].rect.w, 5);
        assert_eq!(output.participants[1].rect.w, 5);
        assert!(output.participants[1].rect.x > 9);
    }

    #[test]
    fn message_ellipsis_uses_final_arrow_label_rect_width() {
        let label = "this label is much wider than thirty two cells and should ellipsize at the actual final arrow label rectangle width";
        let output = build_sequence_output_for_width(
            &SequenceDiagram::new()
                .participant("A")
                .participant("B")
                .message(Message::sync("A", "B", label)),
            60,
        );
        let message = &output.messages[0];
        let display_label = message.display_lines.first().expect("display line");

        assert!(label_width(display_label.as_ref()) > 32);
        assert_eq!(display_label.chars().last(), Some('…'));
        assert_eq!(label_width(display_label.as_ref()), message.label_rect.w);
    }

    #[test]
    fn wide_arrow_label_can_exceed_default_max_label_cells() {
        let label = "a label long enough to use more than the default reserved label cells";
        let output = build_sequence_output_for_width(
            &SequenceDiagram::new()
                .participant("A")
                .participant("B")
                .participant("C")
                .message(Message::sync("A", "C", label)),
            90,
        );
        let message = &output.messages[0];

        assert!(message.label_rect.w > 32);
        let display_label = message.display_lines.first().expect("display line");
        assert!(label_width(display_label.as_ref()) > 32);
    }

    #[test]
    fn wrapped_message_labels_reserve_vertical_space() {
        let output = build_sequence_output(
            &SequenceDiagram::new()
                .max_label_cells(Some(12))
                .message_label_overflow(Overflow::Wrap)
                .participant("A")
                .participant("B")
                .message(Message::sync("A", "B", "wrap this label over several rows"))
                .message(Message::sync("A", "B", "after")),
        );
        let first = &output.messages[0];
        let second = &output.messages[1];

        assert!(first.label_rect.h > 1);
        assert!(
            second.label_rect.y >= first.label_rect.y.saturating_add(first.label_rect.h as i16)
        );
        assert!(second.line_rect.y > first.label_rect.y.saturating_add(first.label_rect.h as i16));
    }

    #[test]
    fn hit_test_second_wrapped_label_line_returns_message_path() {
        let diagram = SequenceDiagram::new()
            .max_label_cells(Some(12))
            .message_label_overflow(Overflow::Wrap)
            .participant("A")
            .participant("B")
            .message(Message::sync("A", "B", "wrap this label over several rows"));
        let node = crate::widgets::internal::SequenceDiagramNode::from(diagram);
        let message = &node.output.messages[0];

        assert!(message.label_rect.h > 1);
        assert_eq!(
            node.hit_test(message.label_rect.x as u16, message.label_rect.y as u16 + 1),
            Some(SequenceItemPath::Message(0))
        );
    }

    #[test]
    fn long_notes_wrap_with_default_label_limit() {
        let output = build_sequence_output(&SequenceDiagram::new().step(SequenceStep::note_over(
            ["Scheduler", "Worker"],
            "Self messages stay anchored to one lifeline",
        )));
        assert_eq!(output.notes[0].lines.len(), 2);
        assert!(output.notes[0].rect.h > 3);
        assert!(output.notes[0].rect.w <= 36);
    }

    #[test]
    fn message_after_note_preserves_label_row() {
        let output = build_sequence_output(
            &SequenceDiagram::new()
                .participant("Scheduler")
                .participant("Worker")
                .participant_aliased("Q", "Queue")
                .message(Message::async_("Scheduler", "Q", "enqueue job"))
                .message(Message::async_("Q", "Worker", "deliver job"))
                .step(SequenceStep::self_msg("Worker", "validate cache"))
                .step(SequenceStep::note_over(
                    ["Worker"],
                    "Self messages stay anchored to one lifeline",
                ))
                .message(Message::reply("Worker", "Q", "ack")),
        );

        let note = &output.notes[0];
        let message_after_note = &output.messages[3];
        let note_bottom = note.rect.y.saturating_add(note.rect.h as i16);
        assert!(message_after_note.label_rect.y >= note_bottom);
        assert!(message_after_note.line_rect.y > message_after_note.label_rect.y);
    }

    #[test]
    fn wider_target_distributes_surplus_into_gaps() {
        let intrinsic = build_sequence_output(
            &SequenceDiagram::new()
                .participant("Alice")
                .participant("Bob"),
        );
        let wide = build_sequence_output_for_width(
            &SequenceDiagram::new()
                .participant("Alice")
                .participant("Bob"),
            80,
        );
        assert_eq!(wide.width, 80);
        assert_eq!(
            wide.participants[0].rect.w,
            intrinsic.participants[0].rect.w
        );
        assert_eq!(
            wide.participants[1].rect.w,
            intrinsic.participants[1].rect.w
        );
        assert!(wide.participants[1].rect.x > intrinsic.participants[1].rect.x);
    }

    #[test]
    fn reconcile_rebuilds_output_when_content_width_changes() {
        let diagram = SequenceDiagram::new()
            .participant("Alice")
            .participant("Bob");
        let mut node = crate::widgets::internal::SequenceDiagramNode::default();
        crate::widgets::sequence_diagram::reconcile_sequence_diagram_with_width(
            &diagram,
            &mut node,
            Some(20),
        );
        let narrow_bob_x = node.output.participants[1].rect.x;

        crate::widgets::sequence_diagram::reconcile_sequence_diagram_with_width(
            &diagram,
            &mut node,
            Some(80),
        );

        assert_eq!(node.output_content_width, Some(80));
        assert!(node.output.participants[1].rect.x > narrow_bob_x);
    }

    #[test]
    fn self_message_extent_is_included_in_width() {
        let output = build_sequence_output(
            &SequenceDiagram::new().step(SequenceStep::self_msg("Worker", "validate cache")),
        );
        let message = &output.messages[0];
        let right = u16::try_from(message.line_rect.x.max(0))
            .unwrap_or(u16::MAX)
            .saturating_add(message.line_rect.w);
        assert!(output.width >= right);
    }

    #[test]
    fn right_of_note_extent_is_included_in_width() {
        let output = build_sequence_output(&SequenceDiagram::new().step(SequenceStep::note(
            NotePlacement::RightOf,
            ["BlobStore"],
            "Right-of note",
        )));
        let note = &output.notes[0];
        let right = u16::try_from(note.rect.x.max(0))
            .unwrap_or(u16::MAX)
            .saturating_add(note.rect.w);
        assert!(output.width >= right);
    }

    #[test]
    fn right_of_note_does_not_clip_under_surplus_distribution() {
        let diagram = SequenceDiagram::new()
            .participant("Alice")
            .participant("Bob")
            .step(SequenceStep::note(
                NotePlacement::RightOf,
                ["Bob"],
                "Right-of note",
            ));
        let intrinsic = build_sequence_output(&diagram);
        let target = intrinsic.width.saturating_add(20);
        let output = build_sequence_output_for_width(&diagram, target);
        let note = &output.notes[0];
        let note_right = u16::try_from(note.rect.x.max(0))
            .unwrap_or(u16::MAX)
            .saturating_add(note.rect.w);
        assert!(
            note_right <= output.width,
            "note right {note_right} exceeds output width {}",
            output.width
        );
        assert!(
            output.width >= target,
            "output width {} should honor target {target}",
            output.width
        );
    }

    #[test]
    fn self_message_on_middle_actor_widens_right_gap_for_label() {
        let diagram = SequenceDiagram::new()
            .participant("A")
            .participant("B")
            .participant("C")
            .step(SequenceStep::self_msg("B", "validate cache"));
        let output = build_sequence_output(&diagram);
        let message = &output.messages[0];
        let label_right = u16::try_from(message.label_rect.x.max(0))
            .unwrap_or(u16::MAX)
            .saturating_add(message.label_rect.w);
        let participant_c = &output.participants[2];
        let c_left = u16::try_from(participant_c.rect.x.max(0)).unwrap_or(u16::MAX);
        assert!(
            label_right <= c_left,
            "self-message label right {label_right} should not bleed into next column at {c_left}"
        );
    }

    #[test]
    fn self_message_on_last_actor_does_not_overflow() {
        let diagram = SequenceDiagram::new()
            .participant("Alice")
            .participant("Bob")
            .step(SequenceStep::self_msg("Bob", "validate cache"));
        let output = build_sequence_output(&diagram);
        let message = &output.messages[0];
        let line_right = u16::try_from(message.line_rect.x.max(0))
            .unwrap_or(u16::MAX)
            .saturating_add(message.line_rect.w);
        let label_right = u16::try_from(message.label_rect.x.max(0))
            .unwrap_or(u16::MAX)
            .saturating_add(message.label_rect.w);
        assert!(
            line_right <= output.width,
            "self-message loop right {line_right} exceeds output width {}",
            output.width
        );
        assert!(
            label_right <= output.width,
            "self-message label right {label_right} exceeds output width {}",
            output.width
        );
        assert!(
            message.line_rect.w <= 8,
            "self-message loop should be small (got width {})",
            message.line_rect.w
        );
    }

    #[test]
    fn left_of_note_anchors_after_left_margin_shift() {
        let diagram = SequenceDiagram::new()
            .participant("Alice")
            .participant("Bob")
            .step(SequenceStep::note(
                NotePlacement::LeftOf,
                ["Alice"],
                "Left-of note",
            ));
        let output = build_sequence_output(&diagram);
        let note = &output.notes[0];
        assert!(
            note.rect.x >= 0,
            "left-of note must not have negative x (got {})",
            note.rect.x
        );
    }

    #[test]
    fn hit_test_returns_correct_message_index() {
        let diagram = SequenceDiagram::new().message(Message::sync("Alice", "Bob", "Hello"));
        let node = crate::widgets::internal::SequenceDiagramNode::from(diagram);
        let message = &node.output.messages[0];
        assert_eq!(
            node.hit_test(message.from_x as u16, message.y as u16),
            Some(SequenceItemPath::Message(0))
        );
    }

    #[test]
    fn hit_test_autonumber_chip_returns_message_path() {
        let diagram = SequenceDiagram::new()
            .participant("Scheduler")
            .participant("Worker")
            .message(Message::sync("Scheduler", "Worker", "Hello"))
            .autonumber(true);
        let node = crate::widgets::internal::SequenceDiagramNode::from(diagram);
        let (path, number) = &node.output.auto_numbers.as_ref().unwrap()[0];
        let rect = crate::widgets::internal::autonumber_rect(
            path,
            *number,
            &node.output.messages,
            &node.theme.autonumber.format,
        )
        .unwrap();

        assert_eq!(
            node.hit_test(rect.x as u16, rect.y as u16),
            Some(SequenceItemPath::Message(0))
        );
    }

    #[test]
    fn hover_only_sequence_diagram_does_not_refine_click_hit_testing() {
        let diagram = SequenceDiagram::new()
            .message(Message::sync("Alice", "Bob", "Hello"))
            .item_hover_style(Style::new().bg(Color::Blue));
        let node = crate::widgets::internal::SequenceDiagramNode::from(diagram);
        let rect = Rect {
            x: 0,
            y: 0,
            w: node.output.width,
            h: node.output.height,
        };
        assert_eq!(node.hit_test_refinement(0, 0, rect), None);
        let message = &node.output.messages[0];
        assert_eq!(
            node.hover_test_refinement(message.from_x, message.y, rect),
            Some(true)
        );
    }

    #[test]
    fn padding_and_border_are_counted() {
        let diagram = SequenceDiagram::new()
            .padding(Padding::from(1))
            .border(true);
        assert_eq!(measure_sequence_diagram(&diagram), (4, 4));
    }
}
