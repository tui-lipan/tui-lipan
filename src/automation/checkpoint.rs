use std::path::PathBuf;
use std::sync::Arc;

use super::{AutomationError, SemanticNode, SemanticTree};

/// Evidence sink configured for named checkpoints.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum CheckpointSink {
    /// Write agent-readable Markdown files into this directory.
    Markdown {
        /// Output directory.
        directory: PathBuf,
    },
    /// Write semantic JSON files into this directory.
    Json {
        /// Output directory.
        directory: PathBuf,
    },
    /// Write PNG files into this directory.
    Png {
        /// Output directory.
        directory: PathBuf,
    },
    /// Return an in-memory recording marker without writing a file.
    RecordingMarker,
    /// Compare deterministic PNGs against this baseline directory.
    Baseline {
        /// Baseline directory.
        directory: PathBuf,
        /// Maximum accepted changed-pixel ratio.
        tolerance: f64,
    },
}

/// One artifact produced by a checkpoint.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct CheckpointArtifact {
    /// Sink format.
    pub format: CheckpointFormat,
    /// Encoded bytes, when the sink returns an in-memory artifact.
    pub bytes: Option<Arc<[u8]>>,
    /// Written path, when the sink writes a file.
    pub path: Option<PathBuf>,
    /// Typed baseline result, when applicable.
    pub baseline: Option<CheckpointBaseline>,
}

/// Successful visual-baseline outcome.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum CheckpointBaseline {
    /// No baseline existed and one was created.
    Created,
    /// The capture matched within tolerance.
    Matched {
        /// Changed-pixel ratio.
        ratio: f64,
    },
    /// Update mode accepted and replaced the baseline.
    Updated,
}

/// Closed set of checkpoint artifact formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CheckpointFormat {
    /// Markdown report.
    Markdown,
    /// Semantic JSON.
    Json,
    /// PNG image.
    Png,
    /// Recording timeline marker.
    RecordingMarker,
    /// Visual baseline comparison.
    Baseline,
}

/// Evidence committed for one named generation.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Checkpoint {
    /// Checkpoint name.
    pub name: Arc<str>,
    /// Session generation.
    pub generation: u64,
    /// Artifacts in configured sink order.
    pub artifacts: Vec<CheckpointArtifact>,
}

pub(crate) fn validate_checkpoint_support(sinks: &[CheckpointSink]) -> Result<(), AutomationError> {
    for sink in sinks {
        match sink {
            CheckpointSink::Json { .. } if !cfg!(feature = "ui-snapshot-json") => {
                return Err(AutomationError::UnsupportedFormat("semantic JSON"));
            }
            CheckpointSink::Png { .. } | CheckpointSink::Baseline { .. }
                if !cfg!(feature = "ui-snapshot-png") =>
            {
                return Err(AutomationError::UnsupportedFormat("PNG"));
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn validate_checkpoint_name(name: &str) -> Result<(), AutomationError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AutomationError::InvalidCheckpointName(name.to_owned()));
    }
    Ok(())
}

pub(crate) fn semantic_json(tree: &SemanticTree) -> Vec<u8> {
    let mut out = String::new();
    out.push_str("{\"schema\":\"tui-lipan.semantic/1\",\"generation\":");
    out.push_str(&tree.generation.to_string());
    out.push_str(",\"viewport\":");
    append_rect_json(&mut out, tree.viewport);
    out.push_str(",\"root\":");
    append_node_json(&mut out, &tree.root);
    out.push('}');
    out.into_bytes()
}

pub(crate) fn semantic_markdown(tree: &SemanticTree) -> Vec<u8> {
    let mut out = format!(
        "# Automation checkpoint\n\n- Generation: `{}`\n- Viewport: `{}x{}`\n\n## Semantic tree\n",
        tree.generation, tree.viewport.w, tree.viewport.h
    );
    for child in &tree.root.children {
        append_node_markdown(&mut out, child, 0);
    }
    out.into_bytes()
}

fn append_node_markdown(out: &mut String, node: &SemanticNode, depth: usize) {
    use std::fmt::Write as _;

    let _ = write!(out, "\n{}- `{:?}`", "  ".repeat(depth), node.role);
    if let Some(id) = &node.automation_id {
        let _ = write!(out, " id=`{}`", escape_markdown(id.as_ref()));
    }
    if let Some(name) = &node.name {
        let _ = write!(out, " name=\"{}\"", escape_markdown(name));
    }
    if let Some(value) = &node.value {
        match &value.text {
            Some(text) => {
                let _ = write!(out, " value=\"{}\"", escape_markdown(text));
            }
            None => {
                let _ = write!(out, " value=<redacted:{:?}>", value.sensitivity);
            }
        }
    }
    let _ = write!(
        out,
        " bounds=`{},{} {}x{}` in_view={} actionable={}",
        node.bounds.x, node.bounds.y, node.bounds.w, node.bounds.h, node.in_view, node.actionable
    );
    for child in &node.children {
        append_node_markdown(out, child, depth + 1);
    }
}

fn escape_markdown(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn append_node_json(out: &mut String, node: &SemanticNode) {
    out.push('{');
    out.push_str("\"automation_id\":");
    append_optional_json_string(out, node.automation_id.as_ref().map(AsRef::as_ref));
    out.push_str(",\"role\":");
    append_json_string(out, &enum_name(node.role));
    out.push_str(",\"name\":");
    append_optional_json_string(out, node.name.as_deref());
    out.push_str(",\"value\":");
    match &node.value {
        None => out.push_str("null"),
        Some(value) => {
            out.push('{');
            out.push_str("\"text\":");
            append_optional_json_string(out, value.text.as_deref());
            out.push_str(",\"sensitivity\":");
            append_json_string(out, &enum_name(value.sensitivity));
            out.push('}');
        }
    }
    out.push_str(",\"focused\":");
    out.push_str(if node.focused { "true" } else { "false" });
    out.push_str(",\"enabled\":");
    out.push_str(if node.enabled { "true" } else { "false" });
    out.push_str(",\"selected\":");
    append_optional_bool(out, node.selected);
    out.push_str(",\"expanded\":");
    append_optional_bool(out, node.expanded);
    out.push_str(",\"checked\":");
    match node.checked {
        Some(value) => append_json_string(out, &enum_name(value)),
        None => out.push_str("null"),
    }
    out.push_str(",\"bounds\":");
    append_rect_json(out, node.bounds);
    out.push_str(",\"clipped_bounds\":");
    append_rect_json(out, node.clipped_bounds);
    out.push_str(",\"in_view\":");
    out.push_str(if node.in_view { "true" } else { "false" });
    out.push_str(",\"actionable\":");
    out.push_str(if node.actionable { "true" } else { "false" });
    out.push_str(",\"actions\":[");
    for (index, action) in node.actions.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        append_json_string(out, &enum_name(*action));
    }
    out.push_str("],\"overlay_order\":");
    match node.overlay_order {
        Some(order) => out.push_str(&order.to_string()),
        None => out.push_str("null"),
    }
    out.push_str(",\"children\":[");
    for (index, child) in node.children.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        append_node_json(out, child);
    }
    out.push_str("]}");
}

fn append_optional_bool(out: &mut String, value: Option<bool>) {
    match value {
        Some(true) => out.push_str("true"),
        Some(false) => out.push_str("false"),
        None => out.push_str("null"),
    }
}

fn enum_name(value: impl std::fmt::Debug) -> String {
    let debug = format!("{value:?}");
    let mut name = String::with_capacity(debug.len() + 4);
    for (index, ch) in debug.chars().enumerate() {
        if index > 0 && ch.is_ascii_uppercase() {
            name.push('_');
        }
        name.push(ch.to_ascii_lowercase());
    }
    name
}

fn append_rect_json(out: &mut String, rect: crate::style::Rect) {
    out.push_str("{\"x\":");
    out.push_str(&rect.x.to_string());
    out.push_str(",\"y\":");
    out.push_str(&rect.y.to_string());
    out.push_str(",\"w\":");
    out.push_str(&rect.w.to_string());
    out.push_str(",\"h\":");
    out.push_str(&rect.h.to_string());
    out.push('}');
}

fn append_optional_json_string(out: &mut String, value: Option<&str>) {
    match value {
        Some(value) => append_json_string(out, value),
        None => out.push_str("null"),
    }
}

fn append_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch <= '\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}
