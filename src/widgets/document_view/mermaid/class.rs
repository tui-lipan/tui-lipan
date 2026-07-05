use std::sync::Arc;

use super::super::diagram::{
    ClassMemberSpec, ClassNodeSpec, ClassRelationSpec, ClassSpec, ClassVisibilitySpec,
    ParsedDiagram,
};
use super::{MermaidParseError, significant_lines, strip_quotes};

pub(crate) fn parse(src: &str) -> Result<ParsedDiagram, MermaidParseError> {
    let lines = significant_lines(src);
    let mut classes = Vec::<ClassNodeSpec>::new();
    let mut relations = Vec::new();
    let mut idx = 1usize;
    while idx < lines.len() {
        let (line_no, line) = &lines[idx];
        if let Some(name) = line
            .strip_prefix("class ")
            .and_then(|s| s.strip_suffix("{"))
        {
            let name = name.trim();
            let mut members = Vec::new();
            idx += 1;
            while idx < lines.len() && lines[idx].1 != "}" {
                members.push(parse_member(&lines[idx].1));
                idx += 1;
            }
            upsert_class(&mut classes, name).members.extend(members);
        } else if let Some((arrow, from, to, from_cardinality, to_cardinality, label)) =
            relation_parts(line)
        {
            upsert_class(&mut classes, from);
            upsert_class(&mut classes, to);
            relations.push(ClassRelationSpec {
                from: Arc::from(from),
                to: Arc::from(to),
                arrow: Arc::from(arrow),
                from_cardinality: from_cardinality.map(Arc::from),
                to_cardinality: to_cardinality.map(Arc::from),
                label: label.map(Arc::from),
            });
        } else if let Some((class_name, member)) = line.split_once(':') {
            upsert_class(&mut classes, class_name.trim())
                .members
                .push(parse_member(member));
        } else {
            return Err(MermaidParseError::new(
                "expected class declaration or relation",
                Some(*line_no),
            ));
        }
        idx += 1;
    }
    if classes.is_empty() {
        return Err(MermaidParseError::new("empty class diagram", None));
    }
    Ok(ParsedDiagram::Class(ClassSpec { classes, relations }))
}

type RelationParts<'a> = (
    &'static str,
    &'a str,
    &'a str,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
);

fn relation_parts(line: &str) -> Option<RelationParts<'_>> {
    ["<|--", "<|..", "*--", "o--", "..>", "-->", "--"]
        .into_iter()
        .find_map(|arrow| {
            let (from_raw, rest) = line.split_once(arrow)?;
            let (to_raw, label) = rest
                .split_once(':')
                .map(|(to, label)| (to.trim(), Some(strip_quotes(label))))
                .unwrap_or_else(|| (rest.trim(), None));
            let (from, from_cardinality) = parse_relation_endpoint(from_raw)?;
            let (to, to_cardinality) = parse_relation_endpoint(to_raw)?;
            Some((
                arrow,
                from,
                to,
                from_cardinality,
                to_cardinality,
                label.filter(|label| !label.is_empty()),
            ))
        })
}

fn parse_relation_endpoint(raw: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = raw.trim();
    let (trimmed, leading_cardinality) = take_leading_cardinality(trimmed);
    let (trimmed, trailing_cardinality) = take_trailing_cardinality(trimmed);
    let name = strip_quotes(trimmed).trim();
    (!name.is_empty()).then_some((
        name,
        trailing_cardinality
            .or(leading_cardinality)
            .filter(|card| !card.is_empty()),
    ))
}

fn take_leading_cardinality(raw: &str) -> (&str, Option<&str>) {
    if !raw.starts_with('"') {
        return (raw, None);
    }
    let rest = &raw[1..];
    let Some(end) = rest.find('"') else {
        return (raw, None);
    };
    (rest[end + 1..].trim(), Some(rest[..end].trim()))
}

fn take_trailing_cardinality(raw: &str) -> (&str, Option<&str>) {
    if !raw.ends_with('"') {
        return (raw, None);
    }
    let prefix = &raw[..raw.len().saturating_sub(1)];
    let Some(start) = prefix.rfind('"') else {
        return (raw, None);
    };
    (prefix[..start].trim(), Some(prefix[start + 1..].trim()))
}

fn upsert_class<'a>(classes: &'a mut Vec<ClassNodeSpec>, name: &str) -> &'a mut ClassNodeSpec {
    if let Some(pos) = classes.iter().position(|c| c.name.as_ref() == name) {
        &mut classes[pos]
    } else {
        classes.push(ClassNodeSpec {
            name: Arc::from(strip_quotes(name)),
            members: Vec::new(),
        });
        classes.last_mut().unwrap()
    }
}

fn parse_member(raw: &str) -> ClassMemberSpec {
    let raw = raw.trim();
    let (visibility, rest) = match raw.chars().next() {
        Some('+') => (ClassVisibilitySpec::Public, &raw[1..]),
        Some('-') => (ClassVisibilitySpec::Private, &raw[1..]),
        Some('#') => (ClassVisibilitySpec::Protected, &raw[1..]),
        Some('~') => (ClassVisibilitySpec::Package, &raw[1..]),
        _ => (ClassVisibilitySpec::Public, raw),
    };
    let method = rest.contains('(');
    let (name, ty) = if let Some((name, ty)) = rest.split_once(':') {
        (name.trim(), Some(Arc::from(ty.trim())))
    } else if let Some((first, second)) = rest.split_once(' ') {
        if method {
            (first.trim(), Some(Arc::from(second.trim())))
        } else {
            (second.trim(), Some(Arc::from(first.trim())))
        }
    } else {
        (rest.trim(), None)
    };
    ClassMemberSpec {
        visibility,
        name: Arc::from(name),
        ty,
        method,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relations_with_cardinality_and_labels_without_creating_fake_classes() {
        let ParsedDiagram::Class(spec) = parse(
            "classDiagram\n\
             class User {\n\
                 +String id\n\
             }\n\
             class Order {\n\
                 +String orderId\n\
             }\n\
             User \"1\" --> \"*\" Order : places",
        )
        .unwrap() else {
            panic!("expected class diagram");
        };

        assert_eq!(spec.classes.len(), 2);
        assert_eq!(spec.relations.len(), 1);
        assert_eq!(spec.relations[0].from.as_ref(), "User");
        assert_eq!(spec.relations[0].to.as_ref(), "Order");
        assert_eq!(spec.relations[0].from_cardinality.as_deref(), Some("1"));
        assert_eq!(spec.relations[0].to_cardinality.as_deref(), Some("*"));
        assert_eq!(spec.relations[0].label.as_deref(), Some("places"));
    }

    #[test]
    fn parses_field_members_as_name_colon_type() {
        let member = parse_member("+String username");
        assert_eq!(member.name.as_ref(), "username");
        assert_eq!(member.ty.as_deref(), Some("String"));
        assert!(!member.method);

        let method = parse_member("+login() Boolean");
        assert_eq!(method.name.as_ref(), "login()");
        assert_eq!(method.ty.as_deref(), Some("Boolean"));
        assert!(method.method);
    }
}
