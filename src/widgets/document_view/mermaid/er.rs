use std::sync::Arc;

use super::super::diagram::{ErAttributeSpec, ErEntitySpec, ErRelationSpec, ErSpec, ParsedDiagram};
use super::{MermaidParseError, significant_lines};

pub(crate) fn parse(src: &str) -> Result<ParsedDiagram, MermaidParseError> {
    let lines = significant_lines(src);
    let mut entities = Vec::<ErEntitySpec>::new();
    let mut relations = Vec::new();
    let mut idx = 1usize;
    while idx < lines.len() {
        let (line_no, line) = &lines[idx];
        if let Some(name) = line.strip_suffix('{') {
            let name = name.trim();
            let mut attributes = Vec::new();
            idx += 1;
            while idx < lines.len() && lines[idx].1 != "}" {
                let parts = lines[idx].1.split_whitespace().collect::<Vec<_>>();
                if parts.len() >= 2 {
                    attributes.push(ErAttributeSpec {
                        ty: Arc::from(parts[0]),
                        name: Arc::from(parts[1]),
                        keys: parts[2..].iter().map(|p| Arc::from(*p)).collect(),
                    });
                }
                idx += 1;
            }
            upsert_entity(&mut entities, name)
                .attributes
                .extend(attributes);
        } else if let Some((left_card, right_card, left, right, label)) = relation_parts(line) {
            upsert_entity(&mut entities, left);
            upsert_entity(&mut entities, right);
            relations.push(ErRelationSpec {
                left: Arc::from(left),
                right: Arc::from(right),
                left_cardinality: Arc::from(left_card),
                right_cardinality: Arc::from(right_card),
                label: label.map(Arc::from),
            });
        } else {
            return Err(MermaidParseError::new(
                "expected ER entity or relation",
                Some(*line_no),
            ));
        }
        idx += 1;
    }
    if entities.is_empty() {
        return Err(MermaidParseError::new("empty ER diagram", None));
    }
    Ok(ParsedDiagram::Er(ErSpec {
        entities,
        relations,
    }))
}

fn relation_parts(line: &str) -> Option<(&str, &str, &str, &str, Option<&str>)> {
    let (left, rest) = line.split_once(' ')?;
    let (edge, rest) = rest.trim().split_once(' ')?;
    let (right, label) = rest
        .split_once(':')
        .map(|(right, label)| (right.trim(), Some(label.trim())))
        .unwrap_or_else(|| (rest.trim(), None));
    let (left_card, right_card) = edge.split_once("--")?;
    Some((left_card, right_card, left.trim(), right, label))
}

fn upsert_entity<'a>(entities: &'a mut Vec<ErEntitySpec>, name: &str) -> &'a mut ErEntitySpec {
    if let Some(pos) = entities.iter().position(|e| e.name.as_ref() == name) {
        &mut entities[pos]
    } else {
        entities.push(ErEntitySpec {
            name: Arc::from(name),
            attributes: Vec::new(),
        });
        entities.last_mut().unwrap()
    }
}
