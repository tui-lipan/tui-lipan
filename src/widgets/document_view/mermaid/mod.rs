//! Small hand-written Mermaid subset parser used by markdown `DocumentView`.

mod class;
mod er;
mod flowchart;
mod gantt;
mod lex;
mod pie;
mod sequence;
mod state;

use super::diagram::ParsedDiagram;

/// Mermaid parse error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MermaidParseError {
    /// Human-readable message.
    pub message: String,
    /// One-based source line number when known.
    pub line: Option<usize>,
}

impl MermaidParseError {
    pub(crate) fn new(message: impl Into<String>, line: Option<usize>) -> Self {
        Self {
            message: message.into(),
            line,
        }
    }
}

/// Parse a supported Mermaid diagram.
pub fn parse(src: &str) -> Result<ParsedDiagram, MermaidParseError> {
    let (line_no, first) = lex::significant_lines(src)
        .into_iter()
        .next()
        .ok_or_else(|| MermaidParseError::new("empty Mermaid diagram", None))?;
    let keyword = first.split_whitespace().next().unwrap_or_default();
    match keyword {
        "flowchart" | "graph" => flowchart::parse(src),
        "sequenceDiagram" => sequence::parse(src),
        "classDiagram" => class::parse(src),
        "stateDiagram-v2" => state::parse(src),
        "erDiagram" => er::parse(src),
        "gantt" => gantt::parse(src),
        "pie" => pie::parse(src),
        other => Err(MermaidParseError::new(
            format!("unsupported Mermaid diagram type `{other}`"),
            Some(line_no),
        )),
    }
}

pub(crate) use lex::{parse_labelled_pair, significant_lines, strip_quotes};
