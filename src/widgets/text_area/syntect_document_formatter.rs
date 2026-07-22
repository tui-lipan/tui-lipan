//! [`ContentFormatter`] adapter that feeds [`SyntectStrategy`] into a
//! [`DocumentView`](crate::widgets::DocumentView).
//!
//! Mirrors `DiffDocumentFormatter` so syntect-based highlighting is available
//! on read-only document surfaces with the same theme-propagation semantics as
//! on [`TextArea`](crate::widgets::TextArea).

use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

use rustc_hash::FxHasher;

use super::{SyntectStrategy, apply_syntect_strategy_app_theme};
use crate::style::Theme;
use crate::widgets::{
    ContentFormatter, FormatInput, FormattedBlock, FormattedDocument, FormattedLine,
    TextAreaColorInput, TextAreaColorStrategy,
};

/// Wraps a [`SyntectStrategy`] as a [`ContentFormatter`] for
/// [`DocumentView`](crate::widgets::DocumentView).
#[derive(Clone)]
pub struct SyntectDocumentFormatter {
    strategy: Rc<dyn TextAreaColorStrategy>,
    language: Option<Arc<str>>,
    formatter_cache_key: u64,
}

impl SyntectDocumentFormatter {
    /// Construct with a default [`SyntectStrategy`] and an optional language tag.
    ///
    /// The language is forwarded to `strategy.highlight(...)` and also serves
    /// as the fallback when the [`DocumentView`](crate::widgets::DocumentView)
    /// caller supplies no `content_type` in [`FormatInput`].
    pub fn new(language: Option<Arc<str>>) -> Self {
        Self::with_strategy(Rc::new(SyntectStrategy::default()), language)
    }

    /// Construct with a caller-provided strategy - useful for pre-configured
    /// [`SyntectStrategy`] instances (custom themes, palettes, etc.).
    pub fn with_strategy(
        strategy: Rc<dyn TextAreaColorStrategy>,
        language: Option<Arc<str>>,
    ) -> Self {
        let mut s = Self {
            strategy,
            language,
            formatter_cache_key: 0,
        };
        s.recompute_cache_key();
        s
    }

    fn recompute_cache_key(&mut self) {
        let mut hasher = FxHasher::default();
        self.strategy.cache_key().hash(&mut hasher);
        self.language.hash(&mut hasher);
        self.formatter_cache_key = hasher.finish();
    }
}

impl ContentFormatter for SyntectDocumentFormatter {
    fn clone_box(&self) -> Box<dyn ContentFormatter> {
        Box::new(self.clone())
    }

    fn set_app_theme_if_absent(&mut self, theme: &Theme) {
        apply_syntect_strategy_app_theme(&mut self.strategy, theme);
        self.recompute_cache_key();
    }

    fn format(&self, input: FormatInput<'_>) -> FormattedDocument {
        let lines = self.strategy.highlight(TextAreaColorInput {
            value: input.value,
            language: self.language.as_deref().or(input.content_type),
            theme: None,
        });

        FormattedDocument {
            blocks: vec![FormattedBlock::Lines(
                lines
                    .into_iter()
                    .enumerate()
                    .map(|(idx, spans)| FormattedLine {
                        spans,
                        source_line: idx,
                        indent: 0,
                        links: Vec::new(),
                    })
                    .collect(),
            )],
        }
    }

    fn measure_format(&self, input: FormatInput<'_>) -> FormattedDocument {
        self.format(input)
    }

    fn cache_key(&self) -> u64 {
        self.formatter_cache_key
    }

    fn measure_cache_key(&self) -> u64 {
        self.formatter_cache_key
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
    use super::*;
    use crate::style::{Color, Style, SyntaxPalette, Theme as AppTheme};

    #[test]
    fn formatter_highlights_via_strategy() {
        let formatter = SyntectDocumentFormatter::new(Some(Arc::from("rust")));
        let doc = formatter.format(FormatInput {
            value: "fn a() {}",
            content_type: None,
            document_styles: None,
        });
        let FormattedBlock::Lines(lines) = &doc.blocks[0] else {
            panic!("expected Lines");
        };
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans.len() >= 2);
    }

    #[test]
    fn set_app_theme_propagates_and_bumps_cache_key() {
        let mut formatter = SyntectDocumentFormatter::new(Some(Arc::from("rust")));
        let before = formatter.cache_key();
        let theme = AppTheme::default().syntax(SyntaxPalette {
            keyword: Style::new().fg(Color::Rgb(200, 10, 10)),
            ..AppTheme::default().syntax
        });
        formatter.set_app_theme_if_absent(&theme);
        assert_ne!(before, formatter.cache_key());
    }

    #[test]
    fn content_type_used_as_fallback_language() {
        let formatter = SyntectDocumentFormatter::new(None);
        let doc = formatter.format(FormatInput {
            value: "fn a() {}",
            content_type: Some("rust"),
            document_styles: None,
        });
        let FormattedBlock::Lines(lines) = &doc.blocks[0] else {
            panic!("expected Lines");
        };
        assert!(lines[0].spans.len() >= 2);
    }

    #[cfg(feature = "syntax-extra")]
    #[test]
    fn formatter_produces_styled_toml_spans() {
        let formatter = SyntectDocumentFormatter::new(Some(Arc::from("toml")));
        let doc = formatter.format(FormatInput {
            value: "name = \"tui-lipan\"",
            content_type: None,
            document_styles: None,
        });
        let FormattedBlock::Lines(lines) = &doc.blocks[0] else {
            panic!("expected Lines");
        };
        assert!(lines[0].spans.iter().any(|span| span.style.fg.is_some()));
    }
}
