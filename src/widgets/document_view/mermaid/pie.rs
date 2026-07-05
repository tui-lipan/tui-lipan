use std::sync::Arc;

use super::super::diagram::{ParsedDiagram, PieSliceSpec, PieSpec};
use super::{MermaidParseError, parse_labelled_pair, significant_lines, strip_quotes};

pub(crate) fn parse(src: &str) -> Result<ParsedDiagram, MermaidParseError> {
    let mut title = None;
    let mut slices = Vec::new();
    for (line_no, line) in significant_lines(src).into_iter().skip(1) {
        if let Some(rest) = line.strip_prefix("title ") {
            title = Some(Arc::from(strip_quotes(rest)));
            continue;
        }
        let (label, value) = parse_labelled_pair(&line, ":", line_no)?;
        let value = value
            .parse::<f64>()
            .map_err(|_| MermaidParseError::new("expected numeric pie value", Some(line_no)))?;
        slices.push(PieSliceSpec {
            label: Arc::from(strip_quotes(label)),
            value,
        });
    }
    if slices.is_empty() {
        return Err(MermaidParseError::new("empty pie diagram", None));
    }
    Ok(ParsedDiagram::Pie(PieSpec { title, slices }))
}
