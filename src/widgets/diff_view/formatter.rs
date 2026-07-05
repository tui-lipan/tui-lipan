//! Document formatter for DiffView.

use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

use rustc_hash::FxHasher;

use super::DiffRender;
use super::strategy::{DIFF_FULL_WIDTH_PAD_CELLS, DiffColorStrategy, line_overlay_style};
use crate::style::{DiffPalette, Span};
use crate::widgets::{
    ContentFormatter, FormatInput, FormattedBlock, FormattedDocument, FormattedLine,
    TextAreaColorInput, TextAreaColorStrategy,
};

#[derive(Clone)]
pub(crate) struct DiffDocumentFormatter {
    strategy: DiffColorStrategy,
    language: Option<Arc<str>>,
    theme: Option<Arc<str>>,
    /// [`ContentFormatter::cache_key`] (visual identity, includes palette/theme).
    formatter_cache_key: u64,
    /// [`ContentFormatter::measure_cache_key`] (geometry-only, stable across theme changes).
    formatter_measure_cache_key: u64,
}

impl DiffDocumentFormatter {
    pub fn new(
        render: DiffRender,
        base: Option<Rc<dyn TextAreaColorStrategy>>,
        style: DiffPalette,
        highlight_full_width: bool,
        pad_full_width: bool,
        language: Option<Arc<str>>,
        theme: Option<Arc<str>>,
    ) -> Self {
        let strategy =
            DiffColorStrategy::new(render, base, style, highlight_full_width, pad_full_width);
        let formatter_cache_key = diff_document_formatter_cache_key(&strategy, &language, &theme);
        let formatter_measure_cache_key = strategy.measure_cache_key();
        Self {
            strategy,
            language,
            theme,
            formatter_cache_key,
            formatter_measure_cache_key,
        }
    }

    pub(crate) fn refresh_formatter_cache_key(&mut self) {
        self.formatter_cache_key =
            diff_document_formatter_cache_key(&self.strategy, &self.language, &self.theme);
        self.formatter_measure_cache_key = self.strategy.measure_cache_key();
    }

    pub(crate) fn strategy_mut(&mut self) -> &mut DiffColorStrategy {
        &mut self.strategy
    }

    pub(crate) fn strategy(&self) -> &DiffColorStrategy {
        &self.strategy
    }

    pub(crate) fn logical_line_text_for_copy(
        &self,
        source_line: usize,
    ) -> Option<Option<Arc<str>>> {
        let line = self.strategy.lines.get(source_line)?;
        if matches!(
            line.kind,
            super::DiffLineKind::Empty | super::DiffLineKind::Separator
        ) {
            return Some(None);
        }
        Some(Some(Arc::clone(&line.text)))
    }
}

fn diff_document_formatter_cache_key(
    strategy: &DiffColorStrategy,
    language: &Option<Arc<str>>,
    theme: &Option<Arc<str>>,
) -> u64 {
    let mut hasher = FxHasher::default();
    strategy.cache_key().hash(&mut hasher);
    language.hash(&mut hasher);
    theme.hash(&mut hasher);
    hasher.finish()
}

impl ContentFormatter for DiffDocumentFormatter {
    fn clone_box(&self) -> Box<dyn ContentFormatter> {
        Box::new(self.clone())
    }

    fn format(&self, _input: FormatInput<'_>) -> FormattedDocument {
        let lines = self.strategy.highlight(TextAreaColorInput {
            value: self.strategy.raw_text.as_ref(),
            language: self.language.as_deref(),
            theme: self.theme.as_deref(),
        });

        let lines: Vec<FormattedLine> = lines
            .into_iter()
            .enumerate()
            .map(|(idx, spans)| FormattedLine {
                spans,
                source_line: idx,
                indent: 0,
                links: Vec::new(),
            })
            .collect();

        FormattedDocument {
            blocks: vec![FormattedBlock::Lines(lines)],
        }
    }

    fn measure_format(&self, _input: FormatInput<'_>) -> FormattedDocument {
        let lines: Vec<FormattedLine> = self
            .strategy
            .lines
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                let mut spans = vec![Span::new(Arc::clone(&line.text))];
                let line_style = self.strategy.style.line_style(line.kind);
                let content_style = line_overlay_style(line_style);
                if self.strategy.highlight_full_width
                    && self.strategy.pad_full_width
                    && content_style.bg.is_some()
                {
                    spans.push(Span::new(" ".repeat(DIFF_FULL_WIDTH_PAD_CELLS)));
                }

                FormattedLine {
                    spans,
                    source_line: idx,
                    indent: 0,
                    links: Vec::new(),
                }
            })
            .collect();

        FormattedDocument {
            blocks: vec![FormattedBlock::Lines(lines)],
        }
    }

    fn cache_key(&self) -> u64 {
        self.formatter_cache_key
    }

    fn measure_cache_key(&self) -> u64 {
        self.formatter_measure_cache_key
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::style::{DiffPalette, Span};

    #[derive(Default)]
    struct CountingStrategy {
        calls: Cell<usize>,
    }

    impl TextAreaColorStrategy for CountingStrategy {
        fn highlight(&self, input: TextAreaColorInput<'_>) -> crate::widgets::TextAreaColorLines {
            self.calls.set(self.calls.get() + 1);
            input
                .value
                .split('\n')
                .map(|line| vec![Span::new(line)])
                .collect()
        }

        fn cache_key(&self) -> u64 {
            1
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn measure_format_matches_full_format_geometry() {
        let render = DiffRender::new(vec![super::super::render::DiffRenderLine {
            prefix: Arc::from("+"),
            text: Arc::from("hello"),
            kind: super::super::render::DiffLineKind::Added,
            old_line: None,
            new_line: Some(1),
            word_ranges: Vec::new(),
            context_separator: None,
            hunk: None,
        }]);
        let strategy = Rc::new(CountingStrategy::default());
        let formatter = DiffDocumentFormatter::new(
            render,
            Some(strategy.clone()),
            DiffPalette::default(),
            true,
            true,
            None,
            None,
        );

        let measured = formatter.measure_format(FormatInput {
            value: "hello",
            content_type: None,
            document_styles: None,
        });

        let formatted = formatter.format(FormatInput {
            value: "hello",
            content_type: None,
            document_styles: None,
        });

        assert_eq!(strategy.calls.get(), 1);
        assert_eq!(measured.blocks.len(), formatted.blocks.len());
        assert_eq!(document_text(&measured), document_text(&formatted));
    }

    fn document_text(document: &FormattedDocument) -> String {
        let mut out = String::new();
        for block in &document.blocks {
            let FormattedBlock::Lines(lines) = block else {
                continue;
            };
            for (idx, line) in lines.iter().enumerate() {
                if idx > 0 {
                    out.push('\n');
                }
                for span in &line.spans {
                    out.push_str(span.content.as_ref());
                }
            }
        }
        out
    }
}
