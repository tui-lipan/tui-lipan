use std::any::Any;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::style::Span;

/// Input data for text coloring strategies.
#[derive(Clone, Copy, Debug)]
pub struct TextAreaColorInput<'a> {
    /// The full text content.
    pub value: &'a str,
    /// Optional language identifier (e.g., "rust", "rs").
    pub language: Option<&'a str>,
    /// Optional theme identifier.
    pub theme: Option<&'a str>,
}

/// Per-line styled spans. Each entry corresponds to a logical line.
pub type TextAreaColorLines = Vec<Vec<Span>>;

/// Strategy for applying per-line styling to text content.
pub trait TextAreaColorStrategy: Any {
    /// Return styled spans per logical line.
    fn highlight(&self, input: TextAreaColorInput<'_>) -> TextAreaColorLines;

    /// Return a stable hash for this strategy's configuration.
    fn cache_key(&self) -> u64 {
        0
    }

    /// Downcast support for framework theme integration.
    fn as_any(&self) -> &dyn Any;

    /// Mutable downcast support for framework theme integration.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TextAreaColorKey {
    pub value_hash: u64,
    pub strategy_hash: u64,
    pub language_hash: u64,
    pub theme_hash: u64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TextAreaColorCache {
    pub key: Option<TextAreaColorKey>,
    pub lines: TextAreaColorLines,
    pub line_starts: Vec<usize>,
    pub line_lengths: Vec<usize>,
}

impl TextAreaColorCache {
    pub(crate) fn update(
        &mut self,
        strategy: Option<&Rc<dyn TextAreaColorStrategy>>,
        value: &str,
        value_hash: u64,
        language: Option<&str>,
        theme: Option<&str>,
    ) {
        let Some(strategy) = strategy else {
            self.key = None;
            self.lines.clear();
            return;
        };

        let key = TextAreaColorKey {
            value_hash,
            strategy_hash: strategy.cache_key(),
            language_hash: hash_optional_str(language),
            theme_hash: hash_optional_str(theme),
        };

        if self.key == Some(key) {
            return;
        }

        let input = TextAreaColorInput {
            value,
            language,
            theme,
        };

        let lines = normalize_color_lines(strategy.highlight(input), value);
        self.lines = lines;
        let (line_starts, line_lengths) = line_meta(value);
        self.line_starts = line_starts;
        self.line_lengths = line_lengths;
        self.key = Some(key);
    }
}

fn logical_lines(value: &str) -> Vec<&str> {
    if value.is_empty() {
        vec![""]
    } else {
        value.split('\n').collect()
    }
}

fn normalize_color_lines(mut lines: TextAreaColorLines, value: &str) -> TextAreaColorLines {
    let expected = logical_lines(value).len();
    if lines.len() > expected {
        lines.truncate(expected);
    }
    while lines.len() < expected {
        lines.push(vec![Span::new("")]);
    }
    lines
}

fn line_meta(value: &str) -> (Vec<usize>, Vec<usize>) {
    if value.is_empty() {
        return (vec![0], vec![0]);
    }

    let mut starts = Vec::new();
    let mut lengths = Vec::new();
    let mut current = 0usize;
    for line in value.split('\n') {
        starts.push(current);
        lengths.push(line.len());
        current = current.saturating_add(line.len()).saturating_add(1);
    }
    (starts, lengths)
}

fn hash_optional_str(value: Option<&str>) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    match value {
        Some(v) => {
            1u8.hash(&mut hasher);
            v.hash(&mut hasher);
        }
        None => {
            0u8.hash(&mut hasher);
        }
    }
    hasher.finish()
}
