use std::sync::Arc;

use crate::style::Color;

use super::super::diagram::{
    DiagramDirection, FlowEdgeSpec, FlowNodeShape, FlowNodeSpec, FlowchartSpec, NodeStyle,
    ParsedDiagram,
};
use super::{MermaidParseError, significant_lines, strip_quotes};

pub(crate) fn parse(src: &str) -> Result<ParsedDiagram, MermaidParseError> {
    let lines = significant_lines(src);
    let (_, header) = lines
        .first()
        .ok_or_else(|| MermaidParseError::new("empty flowchart", None))?;
    let direction = match header.split_whitespace().nth(1).unwrap_or("TD") {
        "TD" | "TB" => DiagramDirection::TopDown,
        "BT" => DiagramDirection::BottomUp,
        "LR" => DiagramDirection::LeftRight,
        "RL" => DiagramDirection::RightLeft,
        _ => DiagramDirection::TopDown,
    };
    let mut nodes: Vec<FlowNodeSpec> = Vec::new();
    let mut node_styles = std::collections::HashMap::<Arc<str>, NodeStyle>::new();
    let mut edges = Vec::new();
    for (line_no, line) in lines.into_iter().skip(1) {
        if is_ignored_flowchart_structure_line(&line) {
            continue;
        }
        if line.starts_with("style ") {
            parse_style_directive(&line, &mut node_styles);
            apply_pending_styles(&mut nodes, &node_styles);
            continue;
        }

        let (arrow, dashed) = if line.contains("-.->") {
            ("-.->", true)
        } else if line.contains("-->") {
            ("-->", false)
        } else if line.contains("---") {
            ("---", false)
        } else {
            let (_, node) = parse_endpoint(&line);
            if node.id.is_empty() {
                return Err(MermaidParseError::new(
                    "expected flowchart edge or node",
                    Some(line_no),
                ));
            }
            upsert_node(&mut nodes, node, &node_styles);
            continue;
        };
        let (left, right) = line.split_once(arrow).unwrap();
        let (from, from_node) = parse_endpoint(left.trim());
        let (mut right_part, label) = if let Some(rest) = right.trim().strip_prefix('|') {
            let (label, after) = rest
                .split_once('|')
                .ok_or_else(|| MermaidParseError::new("unterminated edge label", Some(line_no)))?;
            (after.trim(), Some(Arc::from(strip_quotes(label))))
        } else {
            (right.trim(), None)
        };
        if let Some((before, after)) = right_part.split_once('|') {
            right_part = before.trim();
            let _ = after;
        }
        let (to, to_node) = parse_endpoint(right_part);
        upsert_node(&mut nodes, from_node, &node_styles);
        upsert_node(&mut nodes, to_node, &node_styles);
        edges.push(FlowEdgeSpec {
            from,
            to,
            label,
            dashed,
        });
    }
    Ok(ParsedDiagram::Flowchart(FlowchartSpec {
        direction,
        nodes,
        edges,
    }))
}

fn is_ignored_flowchart_structure_line(line: &str) -> bool {
    line == "end" || line.starts_with("subgraph ") || line.starts_with("direction ")
}

fn upsert_node(
    nodes: &mut Vec<FlowNodeSpec>,
    mut node: FlowNodeSpec,
    node_styles: &std::collections::HashMap<Arc<str>, NodeStyle>,
) {
    if let Some(style) = node_styles.get(&node.id) {
        node.style.merge(*style);
    }
    if let Some(existing) = nodes.iter_mut().find(|n| n.id == node.id) {
        existing.style.merge(node.style);
    } else {
        nodes.push(node);
    }
}

fn parse_style_directive(
    line: &str,
    node_styles: &mut std::collections::HashMap<Arc<str>, NodeStyle>,
) {
    let Some(rest) = line.trim().strip_prefix("style ") else {
        return;
    };
    let mut parts = rest.splitn(2, char::is_whitespace);
    let Some(id) = parts.next().filter(|id| !id.is_empty()) else {
        return;
    };
    let Some(style_src) = parts.next() else {
        return;
    };

    let mut style = NodeStyle::default();
    for entry in style_src.split(',') {
        let Some((key, value)) = entry.trim().split_once(':') else {
            continue;
        };
        let Some(color) = Color::try_hex(value.trim()) else {
            continue;
        };
        match key.trim() {
            "fill" => style.fill = Some(color),
            "color" => style.label_fg = Some(color),
            "stroke" => style.border_fg = Some(color),
            _ => {}
        }
    }

    node_styles
        .entry(Arc::from(id.trim()))
        .or_default()
        .merge(style);
}

fn apply_pending_styles(
    nodes: &mut [FlowNodeSpec],
    node_styles: &std::collections::HashMap<Arc<str>, NodeStyle>,
) {
    for node in nodes {
        if let Some(style) = node_styles.get(&node.id) {
            node.style.merge(*style);
        }
    }
}

fn parse_endpoint(raw: &str) -> (Arc<str>, FlowNodeSpec) {
    let raw = raw.trim().trim_end_matches(';');
    let open = raw.find(['[', '(', '{']);
    if let Some(pos) = open {
        let id = raw[..pos].trim();
        let syntax = raw[pos..].trim();
        let (label, shape) = if let Some(label) = syntax
            .strip_prefix("[(")
            .and_then(|value| value.strip_suffix(")]"))
        {
            (label.trim(), FlowNodeShape::Cylinder)
        } else {
            let label = raw[pos + 1..]
                .trim_matches(['[', ']', '(', ')', '{', '}'])
                .trim();
            let shape = match raw.as_bytes()[pos] as char {
                '(' => FlowNodeShape::Round,
                '{' => FlowNodeShape::Diamond,
                _ => FlowNodeShape::Rect,
            };
            (label, shape)
        };
        let id = Arc::<str>::from(id);
        (
            id.clone(),
            FlowNodeSpec {
                id,
                label: Arc::from(strip_quotes(label)),
                shape,
                style: NodeStyle::default(),
            },
        )
    } else {
        let id = Arc::<str>::from(raw);
        (
            id.clone(),
            FlowNodeSpec {
                id: id.clone(),
                label: id,
                shape: FlowNodeShape::Rect,
                style: NodeStyle::default(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_style_fill_color_stroke() {
        let diagram = parse(
            "graph TD\nA[Client Request] --> B{API Gateway}\nstyle A fill:#4CAF50,stroke:#333,color:#fff",
        )
        .unwrap();

        let ParsedDiagram::Flowchart(spec) = diagram else {
            panic!("expected flowchart");
        };
        let client = spec
            .nodes
            .iter()
            .find(|node| node.id.as_ref() == "A")
            .unwrap();
        assert_eq!(client.style.fill, Some(Color::Rgb(0x4c, 0xaf, 0x50)));
        assert_eq!(client.style.border_fg, Some(Color::Rgb(0x33, 0x33, 0x33)));
        assert_eq!(client.style.label_fg, Some(Color::Rgb(0xff, 0xff, 0xff)));
        assert_eq!(spec.edges.len(), 1);
    }

    #[test]
    fn malformed_style_line_does_not_break_parse() {
        let diagram = parse(
            "graph TD\nA[Client Request] --> B{API Gateway}\nstyle A fill:not-hex,stroke,color:#fff,unknown:#123456",
        )
        .unwrap();

        let ParsedDiagram::Flowchart(spec) = diagram else {
            panic!("expected flowchart");
        };
        let client = spec
            .nodes
            .iter()
            .find(|node| node.id.as_ref() == "A")
            .unwrap();
        assert_eq!(client.style.fill, None);
        assert_eq!(client.style.border_fg, None);
        assert_eq!(client.style.label_fg, Some(Color::Rgb(0xff, 0xff, 0xff)));
    }

    #[test]
    fn style_directive_can_precede_node_definition() {
        let diagram = parse("graph TD\nstyle A fill:#fff\nA --> B").unwrap();

        let ParsedDiagram::Flowchart(spec) = diagram else {
            panic!("expected flowchart");
        };
        let a = spec
            .nodes
            .iter()
            .find(|node| node.id.as_ref() == "A")
            .unwrap();
        assert_eq!(a.style.fill, Some(Color::Rgb(0xff, 0xff, 0xff)));
    }

    #[test]
    fn parses_cylinder_endpoint() {
        let diagram = parse("graph TD\nF --> I[(Database)]").unwrap();

        let ParsedDiagram::Flowchart(spec) = diagram else {
            panic!("expected flowchart");
        };
        let database = spec
            .nodes
            .iter()
            .find(|node| node.id.as_ref() == "I")
            .unwrap();
        assert_eq!(database.label.as_ref(), "Database");
        assert_eq!(database.shape, FlowNodeShape::Cylinder);
    }
}
