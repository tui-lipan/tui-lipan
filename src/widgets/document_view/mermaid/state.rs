use std::sync::Arc;

use super::super::diagram::{
    ParsedDiagram, StateKindSpec, StateNodeSpec, StateSpec, StateTransitionSpec,
};
use super::{MermaidParseError, significant_lines, strip_quotes};

pub(crate) fn parse(src: &str) -> Result<ParsedDiagram, MermaidParseError> {
    let mut states = Vec::<StateNodeSpec>::new();
    let mut transitions = Vec::new();
    for (line_no, line) in significant_lines(src).into_iter().skip(1) {
        if line == "}" || line == "end" {
            continue;
        }
        if let Some((from, rest)) = line.split_once("-->") {
            let from = state_endpoint_id(from.trim(), false);
            let (to, label) = rest
                .split_once(':')
                .map(|(to, label)| {
                    (
                        state_endpoint_id(to.trim(), true),
                        Some(Arc::from(label.trim())),
                    )
                })
                .unwrap_or_else(|| (state_endpoint_id(rest.trim(), true), None));
            upsert_state(&mut states, &from, from == "[*]", false);
            upsert_state(&mut states, &to, false, to == "[*]$end");
            transitions.push(StateTransitionSpec {
                from: Arc::from(from),
                to: Arc::from(to),
                label,
            });
        } else if let Some(rest) = line.strip_prefix("state ") {
            let rest = rest.trim().trim_end_matches('{').trim();
            let (label, id) = rest
                .split_once(" as ")
                .map(|(label, id)| (strip_quotes(label), id.trim()))
                .unwrap_or_else(|| (strip_quotes(rest), strip_quotes(rest)));
            upsert_state(&mut states, id, false, false).label = Arc::from(label);
        } else if let Some((id, label)) = line.split_once(':') {
            upsert_state(&mut states, id.trim(), false, false).label = Arc::from(label.trim());
        } else {
            return Err(MermaidParseError::new(
                "expected state transition or declaration",
                Some(line_no),
            ));
        }
    }
    if states.is_empty() {
        return Err(MermaidParseError::new("empty state diagram", None));
    }
    Ok(ParsedDiagram::State(StateSpec {
        states,
        transitions,
    }))
}

fn state_endpoint_id(id: &str, target: bool) -> String {
    if id == "[*]" && target {
        "[*]$end".to_string()
    } else {
        id.to_string()
    }
}

fn upsert_state<'a>(
    states: &'a mut Vec<StateNodeSpec>,
    id: &str,
    start: bool,
    end: bool,
) -> &'a mut StateNodeSpec {
    let kind = if end {
        StateKindSpec::End
    } else if start || id == "[*]" {
        StateKindSpec::Start
    } else {
        StateKindSpec::State
    };
    if let Some(pos) = states.iter().position(|s| s.id.as_ref() == id) {
        &mut states[pos]
    } else {
        states.push(StateNodeSpec {
            id: Arc::from(id),
            label: Arc::from(if id == "[*]$end" { "[*]" } else { id }),
            kind,
        });
        states.last_mut().unwrap()
    }
}
