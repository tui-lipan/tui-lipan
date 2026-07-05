use std::sync::Arc;

use super::super::diagram::{
    ParsedDiagram, SEQUENCE_NOTE_LABEL_PREFIX, SequenceMessageSpec, SequenceParticipantSpec,
    SequenceSpec,
};
use super::{MermaidParseError, parse_labelled_pair, significant_lines, strip_quotes};

pub(crate) fn parse(src: &str) -> Result<ParsedDiagram, MermaidParseError> {
    let mut participants = Vec::<SequenceParticipantSpec>::new();
    let mut messages = Vec::new();
    for (line_no, line) in significant_lines(src).into_iter().skip(1) {
        if is_ignored_sequence_structure_line(&line) {
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("participant ")
            .or_else(|| line.strip_prefix("actor "))
        {
            let actor = line.starts_with("actor ");
            let (id, label) = rest
                .split_once(" as ")
                .map(|(id, label)| (id.trim(), strip_quotes(label)))
                .unwrap_or_else(|| (rest.trim(), rest.trim()));
            push_participant(&mut participants, id, label, actor);
            continue;
        }
        if let Some((actors, text)) = parse_note(&line, line_no)? {
            for actor in &actors {
                push_participant(&mut participants, actor, actor, false);
            }
            let from = actors.first().copied().unwrap_or_default();
            let to = actors.last().copied().unwrap_or(from);
            messages.push(SequenceMessageSpec {
                from: Arc::from(from),
                to: Arc::from(to),
                label: Arc::from(format!("{SEQUENCE_NOTE_LABEL_PREFIX}{text}")),
                dashed: false,
                open_arrow: true,
            });
            continue;
        }
        let arrow = ["-->>", "->>", "-->", "->"]
            .into_iter()
            .find(|a| line.contains(a));
        let Some(arrow) = arrow else {
            return Err(MermaidParseError::new(
                "expected sequence message",
                Some(line_no),
            ));
        };
        let (from, rest) = line.split_once(arrow).unwrap();
        let (to, label) = parse_labelled_pair(rest, ":", line_no)?;
        push_participant(&mut participants, from.trim(), from.trim(), false);
        push_participant(&mut participants, to.trim(), to.trim(), false);
        messages.push(SequenceMessageSpec {
            from: Arc::from(from.trim()),
            to: Arc::from(to.trim()),
            label: Arc::from(label),
            dashed: arrow.starts_with("--"),
            open_arrow: !arrow.ends_with(">>"),
        });
    }
    if messages.is_empty() && participants.is_empty() {
        return Err(MermaidParseError::new("empty sequence diagram", None));
    }
    Ok(ParsedDiagram::Sequence(SequenceSpec {
        participants,
        messages,
    }))
}

fn is_ignored_sequence_structure_line(line: &str) -> bool {
    line == "end"
        || line == "else"
        || line.starts_with("loop ")
        || line.starts_with("alt ")
        || line.starts_with("opt ")
        || line.starts_with("par ")
        || line.starts_with("and ")
        || line.starts_with("rect ")
        || line.starts_with("critical ")
        || line.starts_with("break ")
        || line.starts_with("activate ")
        || line.starts_with("deactivate ")
}

fn push_participant(
    participants: &mut Vec<SequenceParticipantSpec>,
    id: &str,
    label: &str,
    actor: bool,
) {
    if !participants.iter().any(|p| p.id.as_ref() == id) {
        participants.push(SequenceParticipantSpec {
            id: Arc::from(id),
            label: Arc::from(label),
            actor,
        });
    }
}

fn parse_note(line: &str, line_no: usize) -> Result<Option<(Vec<&str>, &str)>, MermaidParseError> {
    let Some(rest) = line.strip_prefix("Note ") else {
        return Ok(None);
    };
    let Some(actor_text) = rest
        .strip_prefix("over ")
        .or_else(|| rest.strip_prefix("left of "))
        .or_else(|| rest.strip_prefix("right of "))
    else {
        return Err(MermaidParseError::new(
            "expected `Note over`, `Note left of`, or `Note right of`",
            Some(line_no),
        ));
    };
    let (actors, text) = parse_labelled_pair(actor_text, ":", line_no)?;
    let actors = actors
        .split(',')
        .map(str::trim)
        .filter(|actor| !actor.is_empty())
        .collect::<Vec<_>>();
    if actors.is_empty() {
        return Err(MermaidParseError::new("expected note actor", Some(line_no)));
    }
    Ok(Some((actors, strip_quotes(text))))
}
