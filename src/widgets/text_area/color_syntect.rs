use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::Path;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;

use syntect::easy::HighlightLines;
use syntect::highlighting::ScopeSelectors;
use syntect::highlighting::{
    Color as SyntectColor, FontStyle, StyleModifier, Theme as SyntectTheme, ThemeItem, ThemeSet,
    ThemeSettings,
};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use rustc_hash::FxHasher;

use crate::style::{Color, DocumentPalette, Paint, Span, Style, SyntaxPalette, Theme as AppTheme};
use crate::{Error, Result};

use super::{TextAreaColorInput, TextAreaColorLines, TextAreaColorStrategy};

/// Syntax highlighting strategy powered by syntect.
#[derive(Clone)]
pub struct SyntectStrategy {
    syntax_set: Rc<SyntaxSet>,
    theme_set: Rc<ThemeSet>,
    custom_themes: BTreeMap<String, Rc<SyntectTheme>>,
    default_theme: Arc<str>,
    use_background: bool,
    palette_override: Option<SyntaxPalette>,
    document_override: Option<DocumentPalette>,
    app_theme: Option<AppTheme>,
    /// Lazily computed and cached result of [`TextAreaColorStrategy::cache_key`].
    /// Reset to `None` whenever `app_theme` or other identity-affecting fields change.
    cached_key: Cell<Option<u64>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SyntectHighlightCacheKey {
    value_hash: u64,
    language: Option<Arc<str>>,
    theme: Option<Arc<str>>,
    strategy_hash: u64,
}

/// Thread-local highlight cache shared across all `SyntectStrategy` instances.
///
/// The cache key includes a `strategy_hash` that captures the full strategy
/// configuration (syntax set pointer, theme set pointer, default theme,
/// palette, app theme), so entries from different configurations never collide.
///
/// This survives across `view()` rebuilds where a fresh `SyntectStrategy` is
/// constructed each frame - the expensive syntect tokenisation is only done once
/// per unique (text, language, theme, strategy-config) tuple.
const MAX_HIGHLIGHT_CACHE_ENTRIES: usize = 256;

thread_local! {
    static HIGHLIGHT_CACHE: RefCell<Vec<(SyntectHighlightCacheKey, TextAreaColorLines)>> =
        const { RefCell::new(Vec::new()) };
}

const BUILTIN_THEMES: &[(&str, &[u8])] = &[
    ("Dracula", include_bytes!("themes/Dracula.tmTheme")),
    (
        "Monokai Extended",
        include_bytes!("themes/Monokai Extended.tmTheme"),
    ),
    (
        "One Dark (Atom)",
        include_bytes!("themes/base16-onedark.tmTheme"),
    ),
    (
        "Catppuccin Latte",
        include_bytes!("themes/Catppuccin Latte.tmTheme"),
    ),
    (
        "Catppuccin Frappe",
        include_bytes!("themes/Catppuccin Frappe.tmTheme"),
    ),
    (
        "Catppuccin Macchiato",
        include_bytes!("themes/Catppuccin Macchiato.tmTheme"),
    ),
    (
        "Catppuccin Mocha",
        include_bytes!("themes/Catppuccin Mocha.tmTheme"),
    ),
];

thread_local! {
    static DEFAULT_SETS: (Rc<SyntaxSet>, Rc<ThemeSet>) = {
        #[cfg(feature = "syntax-extra")]
        let syntax_set = two_face::syntax::extra_newlines();
        #[cfg(not(feature = "syntax-extra"))]
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let mut theme_set = ThemeSet::load_defaults();
        add_builtin_themes(&mut theme_set);
        (Rc::new(syntax_set), Rc::new(theme_set))
    };
}

impl SyntectStrategy {
    /// Load default syntaxes and themes from syntect.
    pub fn load_defaults() -> Self {
        DEFAULT_SETS
            .with(|(syntax_set, theme_set)| Self::with_sets(syntax_set.clone(), theme_set.clone()))
    }

    /// Construct with custom syntax/theme sets.
    pub fn with_sets(syntax_set: Rc<SyntaxSet>, theme_set: Rc<ThemeSet>) -> Self {
        Self {
            syntax_set,
            theme_set,
            custom_themes: BTreeMap::new(),
            default_theme: Arc::from("base16-ocean.dark"),
            use_background: false,
            palette_override: None,
            document_override: None,
            app_theme: None,
            cached_key: Cell::new(None),
        }
    }

    /// Set the default theme name used when TextArea doesn't specify one.
    pub fn default_theme(mut self, theme: impl Into<Arc<str>>) -> Self {
        self.default_theme = theme.into();
        self
    }

    /// Control whether syntect background colors are applied.
    ///
    /// Default is `false` to keep TextArea background styling.
    pub fn use_background(mut self, enabled: bool) -> Self {
        self.use_background = enabled;
        self
    }

    /// Override syntect token colors with a semantic syntax palette.
    ///
    /// This is applied as a hybrid layer on top of the syntect theme instead of
    /// replacing tokenization or modifiers entirely.
    pub fn syntax_palette(mut self, palette: SyntaxPalette) -> Self {
        self.palette_override = Some(palette);
        self.cached_key.set(None);
        self
    }

    /// Override markdown scope colors with a document palette.
    ///
    /// Takes precedence over the document palette from `ThemeProvider`. When
    /// neither is set, markdown scopes use the underlying tmTheme's defaults.
    pub fn document_palette(mut self, palette: DocumentPalette) -> Self {
        self.document_override = Some(palette);
        self.cached_key.set(None);
        self
    }

    /// Set the app theme only when one has not already been applied.
    ///
    /// This preserves the innermost `ThemeProvider` semantics, matching how
    /// normal widget styles keep explicitly themed inner values when an outer
    /// provider is applied later during runtime wrapping.
    pub fn set_app_theme_if_absent(&mut self, theme: AppTheme) {
        if self.app_theme.is_none() {
            self.app_theme = Some(theme);
            self.cached_key.set(None);
        }
    }

    pub(crate) fn effective_syntax_palette(&self) -> Option<SyntaxPalette> {
        self.palette_override
            .or(self.app_theme.as_ref().map(|theme| theme.syntax))
    }

    /// Add a custom theme from a tmTheme XML string.
    pub fn custom_theme(
        mut self,
        name: impl Into<Arc<str>>,
        content: impl AsRef<str>,
    ) -> Result<Self> {
        let bytes = content.as_ref().as_bytes();
        self = self.custom_theme_bytes(name, bytes)?;
        Ok(self)
    }

    /// Add a custom theme from tmTheme bytes.
    pub fn custom_theme_bytes(
        mut self,
        name: impl Into<Arc<str>>,
        bytes: impl AsRef<[u8]>,
    ) -> Result<Self> {
        let name = name.into();
        let theme = load_theme_from_bytes(name.as_ref(), bytes.as_ref())?;
        self.custom_themes.insert(name.to_string(), Rc::new(theme));
        Ok(self)
    }

    /// Add a custom theme from a tmTheme file path.
    pub fn custom_theme_from_file(
        self,
        name: impl Into<Arc<str>>,
        path: impl AsRef<Path>,
    ) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        self.custom_theme_bytes(name, bytes)
    }

    fn resolve_base_theme<'a>(&'a self, input: TextAreaColorInput<'a>) -> Option<&'a SyntectTheme> {
        let theme_name = input.theme.unwrap_or(self.default_theme.as_ref());
        if let Some(custom) = self.custom_themes.get(theme_name) {
            return Some(custom);
        }
        self.theme_set
            .themes
            .get(theme_name)
            .or_else(|| self.theme_set.themes.values().next())
    }

    fn resolve_theme<'a>(
        &'a self,
        input: TextAreaColorInput<'a>,
    ) -> Option<std::borrow::Cow<'a, SyntectTheme>> {
        let base = self.resolve_base_theme(input);

        if let Some(app_theme) = &self.app_theme {
            let doc = self
                .document_override
                .as_ref()
                .unwrap_or(&app_theme.document);
            return Some(std::borrow::Cow::Owned(build_theme_from_app_theme(
                app_theme,
                self.palette_override.unwrap_or(app_theme.syntax),
                Some(doc),
            )));
        }

        if self.palette_override.is_some() || self.document_override.is_some() {
            let palette = self.palette_override.unwrap_or_default();
            return Some(std::borrow::Cow::Owned(build_theme_from_palette(
                base,
                palette,
                self.use_background,
                self.document_override.as_ref(),
            )));
        }

        base.map(std::borrow::Cow::Borrowed)
    }

    /// Resolve a language name from a file path using extension and filename matching.
    ///
    /// Does **not** read the file - only the path components are inspected.
    /// TypeScript/TSX paths fall back to JavaScript/JSX-compatible syntaxes
    /// when the active syntax set does not provide exact grammars.
    /// Returns the canonical language name (e.g. `"Rust"`, `"Python"`) or `None`.
    pub fn language_for_path(&self, path: impl AsRef<Path>) -> Option<Arc<str>> {
        language_for_path_inner(&self.syntax_set, path.as_ref())
    }

    /// Resolve a language name from a file path, falling back to first-line
    /// (shebang / mode line) detection if extension matching fails.
    ///
    /// The caller must supply the first line - this method does no I/O.
    pub fn language_for_path_and_first_line(
        &self,
        path: impl AsRef<Path>,
        first_line: &str,
    ) -> Option<Arc<str>> {
        self.language_for_path(path).or_else(|| {
            self.syntax_set
                .find_syntax_by_first_line(first_line)
                .map(|s| Arc::from(s.name.as_str()))
        })
    }

    fn resolve_syntax<'a>(&'a self, input: TextAreaColorInput<'a>) -> &'a SyntaxReference {
        if let Some(lang) = input.language {
            self.syntax_set
                .find_syntax_by_token(lang)
                .or_else(|| self.syntax_set.find_syntax_by_name(lang))
                .or_else(|| self.syntax_set.find_syntax_by_extension(lang))
                .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text())
        } else {
            self.syntax_set.find_syntax_plain_text()
        }
    }
}

fn hash_str(value: &str) -> u64 {
    let mut hasher = FxHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

impl Default for SyntectStrategy {
    fn default() -> Self {
        Self::load_defaults()
    }
}

impl TextAreaColorStrategy for SyntectStrategy {
    fn highlight(&self, input: TextAreaColorInput<'_>) -> TextAreaColorLines {
        if input.value.is_empty() {
            return vec![vec![Span::new("")]];
        }

        let cache_key = SyntectHighlightCacheKey {
            value_hash: hash_str(input.value),
            language: input.language.map(Arc::from),
            theme: input.theme.map(Arc::from),
            strategy_hash: self.cache_key(),
        };

        // Check the thread-local cache first.
        let cached = HIGHLIGHT_CACHE.with(|cache| {
            cache
                .borrow()
                .iter()
                .rev()
                .find(|(key, _)| key == &cache_key)
                .map(|(_, lines)| lines.clone())
        });
        if let Some(lines) = cached {
            return lines;
        }

        let syntax = self.resolve_syntax(input);
        let Some(theme) = self.resolve_theme(input) else {
            return plain_lines(input.value);
        };
        let mut highlighter = HighlightLines::new(syntax, theme.as_ref());
        let is_markdown = is_markdown_syntax(syntax);
        let is_rust = is_rust_syntax(syntax);

        let mut lines = Vec::new();
        let mut fence: Option<FenceState<'_>> = None;
        for line in LinesWithEndings::from(input.value) {
            let trimmed_line = line.trim_end_matches('\n');

            // Markdown-aware fenced-block handling: the bundled markdown syntax
            // does not embed inner languages, so tokenise fenced content with a
            // separately-resolved syntax when a recognised language tag is
            // present.
            if is_markdown {
                match fence.as_mut() {
                    Some(state) => {
                        if is_fence_close(trimmed_line, state.ch, state.count) {
                            lines.push(tokenize_outer(
                                &mut highlighter,
                                line,
                                &self.syntax_set,
                                |s| self.syntect_style(s),
                            ));
                            fence = None;
                            continue;
                        }
                        if let Some(inner) = state.inner.as_mut() {
                            // Keep outer highlighter state consistent.
                            let _ = highlighter.highlight_line(line, &self.syntax_set);
                            let mut spans = tokenize_outer(inner, line, &self.syntax_set, |s| {
                                self.syntect_style(s)
                            });
                            if state.inner_is_rust
                                && let Some(palette) = self.effective_syntax_palette()
                            {
                                apply_rust_semantic_fallbacks(
                                    &mut spans,
                                    palette,
                                    plain_fg_from_theme(theme.as_ref()),
                                );
                            }
                            lines.push(spans);
                            continue;
                        }
                        // No inner language resolved - fall through to the
                        // outer markdown pass.
                    }
                    None => {
                        if let Some((ch, count, lang)) = parse_fence_open(trimmed_line) {
                            let inner_syntax = lang.and_then(|l| {
                                self.syntax_set
                                    .find_syntax_by_token(&l)
                                    .or_else(|| self.syntax_set.find_syntax_by_name(&l))
                            });
                            let inner_is_rust = inner_syntax.is_some_and(is_rust_syntax);
                            let inner =
                                inner_syntax.map(|syn| HighlightLines::new(syn, theme.as_ref()));
                            lines.push(tokenize_outer(
                                &mut highlighter,
                                line,
                                &self.syntax_set,
                                |s| self.syntect_style(s),
                            ));
                            fence = Some(FenceState {
                                ch,
                                count,
                                inner,
                                inner_is_rust,
                            });
                            continue;
                        }
                    }
                }
            }

            let mut spans = tokenize_outer(&mut highlighter, line, &self.syntax_set, |s| {
                self.syntect_style(s)
            });
            if is_rust && let Some(palette) = self.effective_syntax_palette() {
                apply_rust_semantic_fallbacks(
                    &mut spans,
                    palette,
                    plain_fg_from_theme(theme.as_ref()),
                );
            }
            lines.push(spans);
        }

        if input.value.ends_with('\n') {
            lines.push(vec![Span::new("")]);
        }

        HIGHLIGHT_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if cache.len() >= MAX_HIGHLIGHT_CACHE_ENTRIES {
                cache.remove(0);
            }
            cache.push((cache_key, lines.clone()));
        });

        lines
    }

    fn cache_key(&self) -> u64 {
        if let Some(key) = self.cached_key.get() {
            return key;
        }
        let mut hasher = FxHasher::default();
        let syntax_ptr = Rc::as_ptr(&self.syntax_set) as usize;
        let theme_ptr = Rc::as_ptr(&self.theme_set) as usize;
        syntax_ptr.hash(&mut hasher);
        theme_ptr.hash(&mut hasher);

        for (name, theme) in &self.custom_themes {
            name.hash(&mut hasher);
            (Rc::as_ptr(theme) as usize).hash(&mut hasher);
        }

        self.default_theme.hash(&mut hasher);
        self.use_background.hash(&mut hasher);
        self.palette_override.hash(&mut hasher);
        self.document_override.hash(&mut hasher);
        if let Some(theme) = &self.app_theme {
            theme.primary.hash(&mut hasher);
            theme.accent.hash(&mut hasher);
            theme.text_selection.hash(&mut hasher);
            theme.hover.hash(&mut hasher);
            theme.border.hash(&mut hasher);
            theme.muted.hash(&mut hasher);
            theme.syntax.hash(&mut hasher);
            theme.document.hash(&mut hasher);
        }
        let key = hasher.finish();
        self.cached_key.set(Some(key));
        key
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl SyntectStrategy {
    fn syntect_style(&self, style: syntect::highlighting::Style) -> Style {
        let mut out = Style::default();
        if style.foreground.a > 0 {
            out.fg =
                Some(Color::rgb(style.foreground.r, style.foreground.g, style.foreground.b).into());
        }
        if self.use_background && style.background.a > 0 {
            out.bg =
                Some(Color::rgb(style.background.r, style.background.g, style.background.b).into());
        }
        if style.font_style.contains(FontStyle::BOLD) {
            out.bold = Some(true);
        }
        if style.font_style.contains(FontStyle::ITALIC) {
            out.italic = Some(true);
        }
        if style.font_style.contains(FontStyle::UNDERLINE) {
            out.underline = Some(true);
        }
        out
    }
}

/// Applies a [`Theme`](crate::style::Theme) to a [`SyntectStrategy`] held in an [`Rc`], matching
/// [`ThemeProvider`](crate::widgets::ThemeProvider) sharing semantics: unique `Rc`s are mutated in
/// place; shared `Rc`s are cloned and replaced.
pub fn apply_syntect_strategy_app_theme(
    strategy: &mut Rc<dyn TextAreaColorStrategy>,
    theme: &AppTheme,
) {
    if let Some(strategy_mut) = Rc::get_mut(strategy)
        && let Some(syntect) = strategy_mut.as_any_mut().downcast_mut::<SyntectStrategy>()
    {
        syntect.set_app_theme_if_absent(theme.clone());
        return;
    }

    let Some(syntect) = strategy.as_ref().as_any().downcast_ref::<SyntectStrategy>() else {
        return;
    };

    let mut cloned = syntect.clone();
    cloned.set_app_theme_if_absent(theme.clone());
    *strategy = Rc::new(cloned);
}

/// Resolve a language name from a file path using default syntect syntax definitions.
///
/// Convenience wrapper that uses the shared default [`SyntaxSet`].
/// TypeScript/TSX paths fall back to JavaScript/JSX-compatible syntaxes when
/// the default syntax set does not provide exact grammars.
/// Returns the canonical language name (e.g. `"Rust"`, `"Python"`) or `None`.
pub fn language_from_path(path: impl AsRef<Path>) -> Option<Arc<str>> {
    DEFAULT_SETS.with(|(syntax_set, _)| language_for_path_inner(syntax_set, path.as_ref()))
}

fn language_for_path_inner(syntax_set: &SyntaxSet, path: &Path) -> Option<Arc<str>> {
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    syntax_set
        .find_syntax_by_extension(file_name)
        .or_else(|| syntax_alias(syntax_set, file_name))
        .or_else(|| syntax_set.find_syntax_by_extension(extension))
        .or_else(|| typescript_fallback_syntax(syntax_set, extension))
        .map(|s| Arc::from(s.name.as_str()))
}

fn syntax_alias<'a>(syntax_set: &'a SyntaxSet, file_name: &str) -> Option<&'a SyntaxReference> {
    match file_name {
        "rust-toolchain" => syntax_set.find_syntax_by_name("TOML"),
        "Dockerfile.dev" => syntax_set.find_syntax_by_name("Dockerfile"),
        _ => None,
    }
}

fn typescript_fallback_syntax<'a>(
    syntax_set: &'a SyntaxSet,
    extension: &str,
) -> Option<&'a SyntaxReference> {
    if extension.eq_ignore_ascii_case("tsx") {
        return syntax_set
            .find_syntax_by_extension("jsx")
            .or_else(|| syntax_set.find_syntax_by_extension("js"));
    }

    if extension.eq_ignore_ascii_case("ts") {
        return syntax_set.find_syntax_by_extension("js");
    }

    None
}

fn build_theme_from_app_theme(
    app_theme: &AppTheme,
    palette: SyntaxPalette,
    document: Option<&DocumentPalette>,
) -> SyntectTheme {
    let fg = app_theme
        .primary
        .fg
        .map(Paint::color)
        .unwrap_or(Color::White);
    let bg = app_theme
        .primary
        .bg
        .map(Paint::color)
        .unwrap_or(Color::Black);
    let accent = app_theme
        .accent
        .fg
        .or(app_theme.text_selection.fg)
        .map(Paint::color)
        .unwrap_or(fg);
    let muted = app_theme
        .muted
        .fg
        .map(Paint::color)
        .unwrap_or(fg.blend_toward(bg, 0.50));
    let border = app_theme
        .border
        .fg
        .map(Paint::color)
        .unwrap_or(fg.blend_toward(bg, 0.40));
    let default_selection_bg = bg.blend_toward(accent, 0.18);
    let selection_bg = app_theme
        .text_selection
        .bg
        .map(Paint::color)
        .unwrap_or(default_selection_bg);
    let selection_fg = app_theme.text_selection.fg.map(Paint::color).unwrap_or(fg);
    let line_highlight = app_theme
        .hover
        .bg
        .map(Paint::color)
        .unwrap_or(bg.lighten_by(0.04));

    SyntectTheme {
        name: Some("tui-lipan generated".to_string()),
        author: Some("tui-lipan".to_string()),
        settings: ThemeSettings {
            foreground: Some(to_syntect_color(fg)),
            background: Some(to_syntect_color(bg)),
            caret: Some(to_syntect_color(accent)),
            line_highlight: Some(to_syntect_color(line_highlight)),
            accent: Some(to_syntect_color(accent)),
            gutter: Some(to_syntect_color(bg)),
            gutter_foreground: Some(to_syntect_color(muted)),
            selection: Some(to_syntect_color(selection_bg)),
            selection_foreground: Some(to_syntect_color(selection_fg)),
            guide: Some(to_syntect_color(border)),
            active_guide: Some(to_syntect_color(accent)),
            stack_guide: Some(to_syntect_color(border)),
            brackets_foreground: Some(to_syntect_color(accent)),
            brackets_background: Some(to_syntect_color(selection_bg)),
            tags_foreground: Some(to_syntect_color(accent)),
            highlight: Some(to_syntect_color(selection_bg)),
            find_highlight: Some(to_syntect_color(selection_bg)),
            find_highlight_foreground: Some(to_syntect_color(selection_fg)),
            shadow: Some(to_syntect_color(bg)),
            ..ThemeSettings::default()
        },
        scopes: {
            let mut scopes = syntax_scope_items(palette);
            if let Some(doc) = document {
                scopes.extend(document_scope_items(doc));
            }
            scopes
        },
    }
}

fn build_theme_from_palette(
    base: Option<&SyntectTheme>,
    palette: SyntaxPalette,
    _use_background: bool,
    document: Option<&DocumentPalette>,
) -> SyntectTheme {
    let fg = base
        .and_then(|theme| theme.settings.foreground)
        .map(from_syntect_color)
        .unwrap_or(Color::White);
    let bg = base
        .and_then(|theme| theme.settings.background)
        .map(from_syntect_color)
        .unwrap_or(Color::Black);
    let accent = palette
        .keyword
        .fg
        .or(palette.function.fg)
        .or(palette.type_name.fg)
        .map(Paint::color)
        .unwrap_or(fg);
    let muted = palette
        .comment
        .fg
        .map(Paint::color)
        .unwrap_or(fg.blend_toward(bg, 0.50));
    let selection_bg = base
        .and_then(|theme| theme.settings.selection)
        .map(from_syntect_color)
        .unwrap_or(bg.blend_toward(accent, 0.18));
    let selection_fg = base
        .and_then(|theme| theme.settings.selection_foreground)
        .map(from_syntect_color)
        .unwrap_or(fg);
    let line_highlight = base
        .and_then(|theme| theme.settings.line_highlight)
        .map(from_syntect_color)
        .unwrap_or(bg.lighten_by(0.04));
    let guide = base
        .and_then(|theme| theme.settings.guide)
        .map(from_syntect_color)
        .unwrap_or(fg.blend_toward(bg, 0.40));

    SyntectTheme {
        name: Some("tui-lipan generated palette".to_string()),
        author: Some("tui-lipan".to_string()),
        settings: ThemeSettings {
            foreground: Some(to_syntect_color(fg)),
            background: Some(to_syntect_color(bg)),
            caret: Some(to_syntect_color(accent)),
            line_highlight: Some(to_syntect_color(line_highlight)),
            accent: Some(to_syntect_color(accent)),
            gutter: Some(to_syntect_color(bg)),
            gutter_foreground: Some(to_syntect_color(muted)),
            selection: Some(to_syntect_color(selection_bg)),
            selection_foreground: Some(to_syntect_color(selection_fg)),
            guide: Some(to_syntect_color(guide)),
            active_guide: Some(to_syntect_color(accent)),
            stack_guide: Some(to_syntect_color(guide)),
            brackets_foreground: Some(to_syntect_color(accent)),
            brackets_background: Some(to_syntect_color(selection_bg)),
            tags_foreground: Some(to_syntect_color(accent)),
            highlight: Some(to_syntect_color(selection_bg)),
            find_highlight: Some(to_syntect_color(selection_bg)),
            find_highlight_foreground: Some(to_syntect_color(selection_fg)),
            shadow: Some(to_syntect_color(bg)),
            ..ThemeSettings::default()
        },
        scopes: {
            let mut scopes = syntax_scope_items(palette);
            if let Some(doc) = document {
                scopes.extend(document_scope_items(doc));
            }
            scopes
        },
    }
}

fn syntax_scope_items(palette: SyntaxPalette) -> Vec<ThemeItem> {
    vec![
        scope_item(
            "comment, punctuation.definition.comment, comment.block.documentation",
            palette.comment,
        ),
        scope_item(
            "keyword, keyword.control, storage, storage.modifier, storage.type.impl, storage.type.struct, storage.type.enum, keyword.operator.word",
            palette.keyword,
        ),
        scope_item(
            "constant.language.boolean, constant.language, constant.other, constant.character",
            palette.constant,
        ),
        scope_item("constant.numeric", palette.number),
        scope_item(
            "string, constant.other.symbol, constant.character.escape, punctuation.definition.string",
            palette.string,
        ),
        scope_item(
            "entity.name.function, storage.type.function",
            palette.function,
        ),
        scope_item(
            "support.function, support.type, support.class, support.constant, storage.type",
            palette.builtin,
        ),
        scope_item(
            "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait, entity.name.namespace, entity.name.tag, entity.name.impl, meta.generic",
            palette.type_name,
        ),
        scope_item("variable.other.member, variable.member", palette.function),
        scope_item(
            "variable, variable.other, variable.function, meta.function-call, meta.definition.variable, entity.name.variable, entity.other.attribute-name",
            palette.variable,
        ),
        scope_item("variable.parameter", palette.parameter),
        scope_item(
            "keyword.operator, punctuation.separator, punctuation.accessor, punctuation.definition.generic, punctuation.section",
            palette.operator,
        ),
    ]
}

fn document_scope_items(doc: &DocumentPalette) -> Vec<ThemeItem> {
    let mut items = Vec::with_capacity(16);

    items.push(scope_item("markup.heading", doc.heading_styles[0]));
    let heading_scopes = [
        "markup.heading.1",
        "markup.heading.2",
        "markup.heading.3",
        "markup.heading.4",
        "markup.heading.5",
        "markup.heading.6",
    ];
    for (scope, style) in heading_scopes.iter().zip(doc.heading_styles.iter()) {
        items.push(scope_item(scope, *style));
    }

    items.push(scope_item(
        "markup.bold, punctuation.definition.bold",
        doc.strong,
    ));
    items.push(scope_item(
        "markup.italic, punctuation.definition.italic",
        doc.emphasis,
    ));
    items.push(scope_item("markup.strikethrough", doc.strikethrough));
    items.push(scope_item(
        "markup.underline.link, markup.underline.link.image, meta.link, constant.other.reference.link, string.other.link",
        doc.link,
    ));
    items.push(scope_item(
        "markup.raw.inline, markup.inline.raw",
        doc.code_inline,
    ));
    if doc.code_block.fg.is_some() || doc.code_block.bg.is_some() {
        items.push(scope_item(
            "markup.raw.block, markup.raw.code-fence, markup.fenced_code.block",
            doc.code_block,
        ));
    }
    items.push(scope_item(
        "markup.quote, punctuation.definition.blockquote",
        doc.blockquote_bar,
    ));
    items.push(scope_item("meta.separator", doc.hr));
    // Target only the marker scope (`.bullet` variant, emitted on the `-`/`*`
    // or number+period span) - the unqualified parent scope covers the whole
    // line including the content text, which is not what we want.
    items.push(scope_item("markup.list.unnumbered.bullet", doc.list_item));
    items.push(scope_item(
        "markup.list.numbered.bullet",
        doc.list_enumeration,
    ));

    items
}

fn scope_item(selectors: &str, style: Style) -> ThemeItem {
    ThemeItem {
        scope: ScopeSelectors::from_str(selectors).expect("valid syntect scope selectors"),
        style: style_to_modifier(style),
    }
}

fn style_to_modifier(style: Style) -> StyleModifier {
    let mut font_style = FontStyle::empty();
    if style.bold == Some(true) {
        font_style |= FontStyle::BOLD;
    }
    if style.italic == Some(true) {
        font_style |= FontStyle::ITALIC;
    }
    if style.underline == Some(true) {
        font_style |= FontStyle::UNDERLINE;
    }

    StyleModifier {
        foreground: style
            .fg
            .filter(|&c| !matches!(c, Paint::Solid(Color::Transparent)))
            .map(Paint::color)
            .map(to_syntect_color),
        background: style
            .bg
            .filter(|&c| !matches!(c, Paint::Solid(Color::Transparent)))
            .map(Paint::color)
            .map(to_syntect_color),
        font_style: if font_style.is_empty() {
            None
        } else {
            Some(font_style)
        },
    }
}

fn to_syntect_color(color: Color) -> SyntectColor {
    let (r, g, b) = color.to_rgb().unwrap_or((0, 0, 0));
    SyntectColor { r, g, b, a: 0xFF }
}

fn from_syntect_color(color: SyntectColor) -> Color {
    Color::rgb(color.r, color.g, color.b)
}

fn add_builtin_themes(theme_set: &mut ThemeSet) {
    for (name, bytes) in BUILTIN_THEMES {
        if theme_set.themes.contains_key(*name) {
            continue;
        }
        let mut cursor = Cursor::new(*bytes);
        if let Ok(theme) = ThemeSet::load_from_reader(&mut cursor) {
            theme_set.themes.insert((*name).to_string(), theme);
        }
    }
}

fn load_theme_from_bytes(name: &str, bytes: &[u8]) -> Result<SyntectTheme> {
    let mut cursor = Cursor::new(bytes);
    ThemeSet::load_from_reader(&mut cursor).map_err(|err| Error::SyntaxThemeLoad {
        name: name.to_string(),
        message: err.to_string(),
        error: None,
    })
}

fn plain_lines(value: &str) -> TextAreaColorLines {
    if value.is_empty() {
        return vec![vec![Span::new("")]];
    }
    value
        .split('\n')
        .map(|line| vec![Span::new(line)])
        .collect()
}

/// State tracked while traversing a markdown fenced code block.
struct FenceState<'a> {
    /// Fence delimiter character - `` ` `` or `~`.
    ch: char,
    /// Number of leading delimiter characters on the opening fence (≥ 3).
    count: usize,
    /// Inner-language highlighter, when the info string resolved to a syntax.
    /// `None` means the info string was missing or unrecognised - inner lines
    /// fall back to the outer markdown pass.
    inner: Option<HighlightLines<'a>>,
    /// Whether `inner` tokenises Rust code.
    inner_is_rust: bool,
}

fn is_markdown_syntax(syntax: &SyntaxReference) -> bool {
    let scope = syntax.scope.build_string();
    scope.starts_with("text.html.markdown") || scope == "text.html.multimarkdown"
}

fn is_rust_syntax(syntax: &SyntaxReference) -> bool {
    syntax.scope.build_string() == "source.rust"
}

fn plain_fg_from_theme(theme: &SyntectTheme) -> Option<Paint> {
    theme
        .settings
        .foreground
        .map(from_syntect_color)
        .map(Paint::Solid)
}

fn apply_rust_semantic_fallbacks(
    spans: &mut Vec<Span>,
    palette: SyntaxPalette,
    plain_fg: Option<Paint>,
) {
    let line = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    if line.trim().is_empty() {
        return;
    }

    let mut ranges: Vec<(std::ops::Range<usize>, Style)> = Vec::new();

    collect_rust_path_ranges(&line, &mut ranges, palette);
    collect_rust_type_context_ranges(&line, &mut ranges, palette);
    collect_leading_rust_variant_range(&line, &mut ranges, palette.function);
    collect_leading_rust_member_range(&line, &mut ranges, palette.function);

    if !ranges.is_empty() {
        apply_style_ranges(spans, &ranges, plain_fg, palette);
    }
}

fn collect_leading_rust_member_range(
    line: &str,
    ranges: &mut Vec<(std::ops::Range<usize>, Style)>,
    style: Style,
) {
    let cursor = skip_rust_visibility(line, leading_ws_len(line));
    let Some(range) = identifier_at(line, cursor) else {
        return;
    };
    let after = skip_ws(line, range.end);
    if line[after..].starts_with(':') {
        ranges.push((range, style));
    }
}

fn collect_leading_rust_variant_range(
    line: &str,
    ranges: &mut Vec<(std::ops::Range<usize>, Style)>,
    style: Style,
) {
    let cursor = leading_ws_len(line);
    let Some(range) = identifier_at(line, cursor) else {
        return;
    };
    let ident = &line[range.clone()];
    if !ident.starts_with(|c: char| c.is_ascii_uppercase()) || rust_builtin_ident(ident) {
        return;
    }
    let after = skip_ws(line, range.end);
    if line[after..].starts_with(',')
        || line[after..].starts_with('(')
        || line[after..].starts_with('{')
        || line[after..].is_empty()
    {
        ranges.push((range, style));
    }
}

fn collect_rust_type_context_ranges(
    line: &str,
    ranges: &mut Vec<(std::ops::Range<usize>, Style)>,
    palette: SyntaxPalette,
) {
    let bytes = line.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b':' {
            if idx + 1 < bytes.len() && bytes[idx + 1] == b':' {
                idx += 2;
                continue;
            }
            collect_type_like_idents_until(line, idx + 1, ranges, palette);
        } else if bytes[idx] == b'-' && idx + 1 < bytes.len() && bytes[idx + 1] == b'>' {
            collect_type_like_idents_until(line, idx + 2, ranges, palette);
            idx += 1;
        }
        idx += 1;
    }

    if let Some(rest) = line.trim_start().strip_prefix("impl ") {
        let start = line.len() - rest.len();
        if let Some(range) = identifier_at(line, skip_ws(line, start)) {
            push_rust_type_ident(line, range, ranges, palette);
        }
    }
}

fn collect_type_like_idents_until(
    line: &str,
    start: usize,
    ranges: &mut Vec<(std::ops::Range<usize>, Style)>,
    palette: SyntaxPalette,
) {
    let mut idx = start;
    while idx < line.len() {
        let Some(ch) = line[idx..].chars().next() else {
            break;
        };
        if matches!(ch, ',' | '=' | '{' | ')' | ';') {
            break;
        }
        if is_ident_start(ch)
            && let Some(range) = identifier_at(line, idx)
        {
            push_rust_type_ident(line, range.clone(), ranges, palette);
            idx = range.end;
            continue;
        }
        idx += ch.len_utf8();
    }
}

fn collect_rust_path_ranges(
    line: &str,
    ranges: &mut Vec<(std::ops::Range<usize>, Style)>,
    palette: SyntaxPalette,
) {
    let mut search = 0;
    while let Some(rel) = line[search..].find("::") {
        let sep = search + rel;
        if let Some(left) = identifier_before(line, sep) {
            push_rust_type_ident(line, left, ranges, palette);
        }
        if let Some(right) = identifier_at(line, sep + 2) {
            let ident = &line[right.clone()];
            let style = if rust_builtin_ident(ident) {
                palette.builtin
            } else {
                palette.function
            };
            ranges.push((right, style));
        }
        search = sep + 2;
    }
}

fn push_rust_type_ident(
    line: &str,
    range: std::ops::Range<usize>,
    ranges: &mut Vec<(std::ops::Range<usize>, Style)>,
    palette: SyntaxPalette,
) {
    let ident = &line[range.clone()];
    if rust_builtin_ident(ident) {
        ranges.push((range, palette.builtin));
    } else if ident.starts_with(|c: char| c.is_ascii_uppercase()) {
        ranges.push((range, palette.type_name));
    }
}

fn apply_style_ranges(
    spans: &mut Vec<Span>,
    ranges: &[(std::ops::Range<usize>, Style)],
    plain_fg: Option<Paint>,
    palette: SyntaxPalette,
) {
    let old = std::mem::take(spans);
    let mut rebuilt = Vec::with_capacity(old.len() + ranges.len() * 2);
    let mut line_offset = 0;

    for span in old {
        let span_start = line_offset;
        let span_end = span_start + span.content.len();
        line_offset = span_end;

        let mut cuts = vec![0, span.content.len()];
        for (range, _) in ranges {
            if range.start < span_end && range.end > span_start {
                cuts.push(range.start.saturating_sub(span_start));
                cuts.push(range.end.min(span_end).saturating_sub(span_start));
            }
        }
        cuts.sort_unstable();
        cuts.dedup();

        for window in cuts.windows(2) {
            let local_start = window[0];
            let local_end = window[1];
            if local_start == local_end {
                continue;
            }
            let global_start = span_start + local_start;
            let global_end = span_start + local_end;
            let next = Span::new(&span.content[local_start..local_end])
                .style(style_for_range(
                    global_start..global_end,
                    span.style,
                    ranges,
                    plain_fg,
                    palette,
                ))
                .row_style_policy(span.row_style_policy);
            rebuilt.push(next);
        }
    }

    *spans = rebuilt;
}

fn style_for_range(
    range: std::ops::Range<usize>,
    fallback: Style,
    ranges: &[(std::ops::Range<usize>, Style)],
    plain_fg: Option<Paint>,
    palette: SyntaxPalette,
) -> Style {
    let Some((_, semantic)) = ranges
        .iter()
        .rev()
        .find(|(target, _)| target.start <= range.start && target.end >= range.end)
    else {
        return fallback;
    };

    if span_allows_rust_semantic_fallback(fallback, *semantic, plain_fg, palette) {
        fallback.patch(*semantic)
    } else {
        fallback
    }
}

fn span_allows_rust_semantic_fallback(
    base: Style,
    semantic: Style,
    plain_fg: Option<Paint>,
    palette: SyntaxPalette,
) -> bool {
    if matches!(plain_fg, Some(plain_fg) if base.fg == Some(plain_fg))
        || plain_fg.is_none() && base.fg.is_none()
    {
        return true;
    }

    // Syntect's Rust grammar reports some regular type identifiers (for
    // example `Vec`) as support/builtin types. Let Rust's syntactic fallback
    // promote those to the upstream type color, but never recolor strings or
    // comments that merely contain Rust-looking text.
    semantic.fg == palette.type_name.fg
        && base.fg == palette.builtin.fg
        && base.fg != palette.string.fg
        && base.fg != palette.comment.fg
}

fn leading_ws_len(line: &str) -> usize {
    line.chars()
        .take_while(|c| c.is_whitespace())
        .map(char::len_utf8)
        .sum()
}

fn skip_ws(line: &str, start: usize) -> usize {
    let mut idx = start;
    while idx < line.len() {
        let Some(ch) = line[idx..].chars().next() else {
            break;
        };
        if !ch.is_whitespace() {
            break;
        }
        idx += ch.len_utf8();
    }
    idx
}

fn skip_rust_visibility(line: &str, start: usize) -> usize {
    if !line[start..].starts_with("pub") {
        return start;
    }
    let mut idx = start + 3;
    match line[idx..].chars().next() {
        Some(ch) if is_ident_continue(ch) => return start,
        Some('(') => {
            if let Some(close) = line[idx..].find(')') {
                idx += close + 1;
            } else {
                return start;
            }
        }
        _ => {}
    }
    skip_ws(line, idx)
}

fn identifier_at(line: &str, start: usize) -> Option<std::ops::Range<usize>> {
    let ch = line[start..].chars().next()?;
    if !is_ident_start(ch) {
        return None;
    }
    let mut end = start + ch.len_utf8();
    while end < line.len() {
        let ch = line[end..].chars().next()?;
        if !is_ident_continue(ch) {
            break;
        }
        end += ch.len_utf8();
    }
    Some(start..end)
}

fn identifier_before(line: &str, end: usize) -> Option<std::ops::Range<usize>> {
    let mut idx = end;
    while idx > 0 {
        let (prev, ch) = line[..idx].char_indices().next_back()?;
        if !ch.is_whitespace() {
            break;
        }
        idx = prev;
    }
    let ident_end = idx;
    while idx > 0 {
        let (prev, ch) = line[..idx].char_indices().next_back()?;
        if !is_ident_continue(ch) {
            break;
        }
        idx = prev;
    }
    if idx == ident_end {
        return None;
    }
    let ch = line[idx..ident_end].chars().next()?;
    is_ident_start(ch).then_some(idx..ident_end)
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn rust_builtin_ident(ident: &str) -> bool {
    matches!(
        ident,
        "Self"
            | "None"
            | "Some"
            | "Ok"
            | "Err"
            | "bool"
            | "char"
            | "str"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "f32"
            | "f64"
    )
}

fn parse_fence_open(line: &str) -> Option<(char, usize, Option<String>)> {
    let stripped = line.trim_start_matches(' ');
    let first = stripped.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let count = stripped.chars().take_while(|&c| c == first).count();
    if count < 3 {
        return None;
    }
    let rest = &stripped[count..];
    let info = rest.trim();
    // The info string may be comma- or space-delimited; the first token is the
    // language tag.
    let lang = info
        .split(|c: char| c == ',' || c.is_whitespace())
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    // A line of all backticks with no info string is still a valid open fence
    // (and also a valid close fence - callers handle open-before-close).
    Some((first, count, lang))
}

fn is_fence_close(line: &str, ch: char, open_count: usize) -> bool {
    let stripped = line.trim_start_matches(' ').trim_end();
    if stripped.chars().any(|c| c != ch) {
        return false;
    }
    stripped.len() >= open_count
}

fn tokenize_outer(
    highlighter: &mut HighlightLines<'_>,
    line: &str,
    syntax_set: &SyntaxSet,
    convert: impl Fn(syntect::highlighting::Style) -> Style,
) -> Vec<Span> {
    match highlighter.highlight_line(line, syntax_set) {
        Ok(ranges) => {
            let mut spans: Vec<Span> = ranges
                .into_iter()
                .map(|(style, text)| Span::new(text).style(convert(style)))
                .collect();
            if line.ends_with('\n') {
                strip_trailing_newline(&mut spans);
            }
            spans
        }
        Err(_) => {
            let text = line.strip_suffix('\n').unwrap_or(line);
            vec![Span::new(text)]
        }
    }
}

fn strip_trailing_newline(spans: &mut Vec<Span>) {
    if let Some(last) = spans.last_mut()
        && last.content.ends_with('\n')
    {
        let trimmed = last
            .content
            .strip_suffix('\n')
            .unwrap_or(last.content.as_ref());
        if trimmed.is_empty() {
            spans.pop();
        } else {
            last.content = Arc::from(trimmed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span_fg_containing(lines: &[Vec<Span>], needle: &str) -> Option<Paint> {
        lines
            .iter()
            .flat_map(|line| line.iter())
            .find(|span| span.content.as_ref().contains(needle))
            .and_then(|span| span.style.fg)
    }

    #[test]
    fn app_theme_generates_syntect_theme_from_syntax_palette() {
        let palette = SyntaxPalette {
            comment: Style::new().fg(Color::Rgb(1, 2, 3)).italic(),
            keyword: Style::new().fg(Color::Rgb(4, 5, 6)),
            string: Style::new().fg(Color::Rgb(7, 8, 9)),
            number: Style::new().fg(Color::Rgb(10, 11, 12)),
            constant: Style::new().fg(Color::Rgb(11, 12, 13)).italic(),
            function: Style::new().fg(Color::Rgb(13, 14, 15)),
            builtin: Style::new().fg(Color::Rgb(14, 15, 16)).italic(),
            type_name: Style::new().fg(Color::Rgb(16, 17, 18)),
            variable: Style::new().fg(Color::Rgb(19, 20, 21)),
            parameter: Style::new().fg(Color::Rgb(20, 21, 22)).italic(),
            operator: Style::new().fg(Color::Rgb(22, 23, 24)),
        };
        let app_theme = AppTheme::default().syntax(palette).primary(
            Style::new()
                .fg(Color::Rgb(30, 31, 32))
                .bg(Color::Rgb(5, 6, 7)),
        );
        let generated =
            build_theme_from_app_theme(&app_theme, app_theme.syntax, Some(&app_theme.document));

        assert_eq!(
            generated.settings.foreground,
            Some(to_syntect_color(Color::Rgb(30, 31, 32)))
        );
        assert_eq!(
            generated.settings.background,
            Some(to_syntect_color(Color::Rgb(5, 6, 7)))
        );
        assert!(generated.scopes.len() >= 11);
        assert_eq!(
            generated.scopes[0].style.foreground,
            palette.comment.fg.map(Paint::color).map(to_syntect_color)
        );
        assert_eq!(
            generated.scopes[1].style.foreground,
            palette.keyword.fg.map(Paint::color).map(to_syntect_color)
        );
        assert_eq!(
            generated.scopes[2].style.foreground,
            palette.constant.fg.map(Paint::color).map(to_syntect_color)
        );
        assert_eq!(
            generated.scopes[3].style.foreground,
            palette.number.fg.map(Paint::color).map(to_syntect_color)
        );
        assert_eq!(
            generated.scopes[4].style.foreground,
            palette.string.fg.map(Paint::color).map(to_syntect_color)
        );
        assert_eq!(
            generated.scopes[6].style.foreground,
            palette.builtin.fg.map(Paint::color).map(to_syntect_color)
        );
        assert_eq!(
            generated.scopes[10].style.foreground,
            palette.parameter.fg.map(Paint::color).map(to_syntect_color)
        );
    }

    #[test]
    fn syntax_palette_can_drive_real_rust_highlighting() {
        let palette = SyntaxPalette {
            keyword: Style::new().fg(Color::Rgb(200, 10, 10)).bold(),
            function: Style::new().fg(Color::Rgb(10, 200, 120)),
            number: Style::new().fg(Color::Rgb(240, 180, 30)),
            ..AppTheme::default().syntax
        };
        let mut strategy = SyntectStrategy::default().default_theme("One Dark (Atom)");
        strategy.set_app_theme_if_absent(AppTheme::default().syntax(palette));

        let lines = strategy.highlight(TextAreaColorInput {
            value: "fn greet() -> usize { 42 }",
            language: Some("rust"),
            theme: Some("One Dark (Atom)"),
        });

        let spans = &lines[0];
        let fn_fg = spans
            .iter()
            .find(|span| span.content.as_ref() == "fn")
            .and_then(|span| span.style.fg);
        let function_fg = spans
            .iter()
            .find(|span| span.content.as_ref().contains("greet"))
            .and_then(|span| span.style.fg);
        let number_fg = spans
            .iter()
            .find(|span| span.content.as_ref().contains("42"))
            .and_then(|span| span.style.fg);

        assert_eq!(fn_fg, palette.function.fg);
        assert_eq!(function_fg, palette.function.fg);
        assert_eq!(number_fg, palette.number.fg);
    }

    #[test]
    fn different_app_themes_produce_different_syntect_colors() {
        let mut one_dark = SyntectStrategy::default().default_theme("One Dark (Atom)");
        one_dark.set_app_theme_if_absent(AppTheme::one_dark());

        let mut dracula = SyntectStrategy::default().default_theme("One Dark (Atom)");
        dracula.set_app_theme_if_absent(AppTheme::dracula());

        let input = TextAreaColorInput {
            value: "fn greet() -> usize { 42 }",
            language: Some("rust"),
            theme: Some("One Dark (Atom)"),
        };

        let one_dark_lines = one_dark.highlight(input);
        let dracula_lines = dracula.highlight(input);

        let one_dark_function_keyword = one_dark_lines[0]
            .iter()
            .find(|span| span.content.as_ref() == "fn")
            .and_then(|span| span.style.fg);
        let dracula_function_keyword = dracula_lines[0]
            .iter()
            .find(|span| span.content.as_ref() == "fn")
            .and_then(|span| span.style.fg);

        assert_ne!(one_dark_function_keyword, dracula_function_keyword);
        assert_eq!(
            one_dark_function_keyword,
            AppTheme::one_dark().syntax.function.fg
        );
        assert_eq!(
            dracula_function_keyword,
            AppTheme::dracula().syntax.function.fg
        );
    }

    #[test]
    fn markdown_list_marker_is_styled_without_painting_item_content() {
        let primary_fg = Color::Rgb(222, 222, 222);
        let list_item_color = Color::Rgb(255, 100, 0);
        let list_enum_color = Color::Rgb(0, 100, 255);

        let mut strategy = SyntectStrategy::default().default_theme("One Dark (Atom)");
        let mut doc = AppTheme::default().document;
        doc.list_item = Style::new().fg(list_item_color);
        doc.list_enumeration = Style::new().fg(list_enum_color);
        strategy.set_app_theme_if_absent(
            AppTheme::default()
                .document(doc)
                .primary(Style::new().fg(primary_fg)),
        );

        let md = "- bullet one\n1. numbered one\n";
        let lines = strategy.highlight(TextAreaColorInput {
            value: md,
            language: Some("markdown"),
            theme: Some("One Dark (Atom)"),
        });

        // Bullet line: the span containing the `-` marker gets list_item
        // color; the content span stays at primary foreground.
        let bullet_content = lines[0]
            .iter()
            .find(|s| s.content.as_ref().contains("bullet one"))
            .expect("bullet content span");
        assert_eq!(
            bullet_content.style.fg,
            Some(primary_fg.into()),
            "unnumbered content must not inherit list_item color"
        );
        assert!(
            lines[0].iter().any(|s| {
                s.content.as_ref().contains('-') && s.style.fg == Some(list_item_color.into())
            }),
            "marker `-` should be styled with list_item color"
        );

        // Numbered line: the marker (`1` and `.` spans) gets list_enumeration;
        // the following content span stays at primary.
        let numbered_content = lines[1]
            .iter()
            .find(|s| s.content.as_ref().contains("numbered one"))
            .expect("numbered content span");
        assert_eq!(
            numbered_content.style.fg,
            Some(primary_fg.into()),
            "numbered content must not inherit list_enumeration color"
        );
        assert!(
            lines[1]
                .iter()
                .any(|s| s.content.as_ref() == "1" && s.style.fg == Some(list_enum_color.into())),
            "numeric marker digit should be styled with list_enumeration color"
        );
    }

    #[test]
    fn fenced_rust_block_in_markdown_applies_syntax_palette() {
        let keyword_color = Color::Rgb(200, 10, 10);
        let function_color = Color::Rgb(10, 200, 120);
        let primary_fg = Color::Rgb(222, 222, 222);

        let palette = SyntaxPalette {
            keyword: Style::new().fg(keyword_color).bold(),
            function: Style::new().fg(function_color),
            ..AppTheme::default().syntax
        };
        let mut strategy = SyntectStrategy::default().default_theme("One Dark (Atom)");
        strategy.set_app_theme_if_absent(
            AppTheme::default()
                .syntax(palette)
                .primary(Style::new().fg(primary_fg)),
        );

        let markdown = "# Title\n\n```rust\nfn greet() -> usize { 42 }\n```\n";
        let lines = strategy.highlight(TextAreaColorInput {
            value: markdown,
            language: Some("markdown"),
            theme: Some("One Dark (Atom)"),
        });

        let code_line = lines
            .iter()
            .find(|spans| spans.iter().any(|s| s.content.as_ref().contains("greet")))
            .expect("fenced code line present");
        let fn_fg = code_line
            .iter()
            .find(|s| s.content.as_ref() == "fn")
            .and_then(|s| s.style.fg);
        let greet_fg = code_line
            .iter()
            .find(|s| s.content.as_ref().contains("greet"))
            .and_then(|s| s.style.fg);

        assert_eq!(
            fn_fg,
            Some(function_color.into()),
            "fenced-block rust function keyword"
        );
        assert_eq!(
            greet_fg,
            Some(function_color.into()),
            "fenced-block rust function"
        );
    }

    #[test]
    fn rust_semantic_fallbacks_color_diff_like_members_types_and_variants() {
        let function_color = Color::Rgb(250, 178, 131);
        let type_color = Color::Rgb(229, 192, 123);
        let builtin_color = Color::Rgb(224, 108, 117);

        let palette = SyntaxPalette {
            function: Style::new().fg(function_color),
            type_name: Style::new().fg(type_color),
            builtin: Style::new().fg(builtin_color),
            ..AppTheme::default().syntax
        };
        let mut strategy = SyntectStrategy::default().default_theme("One Dark (Atom)");
        strategy.set_app_theme_if_absent(AppTheme::default().syntax(palette));

        let rust = concat!(
            "pub struct MoveSwapHint {\n",
            "    pub pane: PaneId,\n",
            "    pub return_direction: Direction,\n",
            "    pub(crate) scoped_pane: crate::model::QualifiedPaneId,\n",
            "}\n",
            "pub enum Mode {\n",
            "    Normal,\n",
            "}\n",
            "impl Workspace {\n",
            "    pub fn new(index: usize) -> Self {\n",
            "        Self {\n",
            "            split_ratios: Vec::<f32>::new(),\n",
            "            last_move_swap: None,\n",
            "        }\n",
            "    }\n",
            "}\n",
        );
        let lines = strategy.highlight(TextAreaColorInput {
            value: rust,
            language: Some("rust"),
            theme: Some("One Dark (Atom)"),
        });

        assert_eq!(
            span_fg_containing(&lines, "pane"),
            Some(function_color.into())
        );
        assert_eq!(
            span_fg_containing(&lines, "return_direction"),
            Some(function_color.into())
        );
        assert_eq!(
            span_fg_containing(&lines, "PaneId"),
            Some(type_color.into())
        );
        assert_eq!(
            span_fg_containing(&lines, "Direction"),
            Some(type_color.into())
        );
        assert_eq!(
            span_fg_containing(&lines, "scoped_pane"),
            Some(function_color.into())
        );
        assert_eq!(
            span_fg_containing(&lines, "QualifiedPaneId"),
            Some(type_color.into())
        );
        assert_eq!(
            span_fg_containing(&lines, "Normal"),
            Some(function_color.into())
        );
        assert_eq!(span_fg_containing(&lines, "Vec"), Some(type_color.into()));
        assert_eq!(
            span_fg_containing(&lines, "f32"),
            Some(builtin_color.into())
        );
        assert_eq!(
            span_fg_containing(&lines, "last_move_swap"),
            Some(function_color.into())
        );
        assert_eq!(
            span_fg_containing(&lines, "None"),
            Some(builtin_color.into())
        );
    }

    #[test]
    fn rust_semantic_fallbacks_do_not_recolor_strings_or_comments() {
        let function_color = Color::Rgb(250, 178, 131);
        let type_color = Color::Rgb(229, 192, 123);
        let comment_color = Color::Rgb(90, 100, 110);
        let string_color = Color::Rgb(80, 190, 120);

        let palette = SyntaxPalette {
            function: Style::new().fg(function_color),
            type_name: Style::new().fg(type_color),
            comment: Style::new().fg(comment_color),
            string: Style::new().fg(string_color),
            ..AppTheme::default().syntax
        };
        let mut strategy = SyntectStrategy::default().default_theme("One Dark (Atom)");
        strategy.set_app_theme_if_absent(AppTheme::default().syntax(palette));

        let lines = strategy.highlight(TextAreaColorInput {
            value: "let s = \"StringVec::<StringType>::new()\";\n// comment_field: CommentPaneId\n",
            language: Some("rust"),
            theme: Some("One Dark (Atom)"),
        });

        assert_eq!(
            span_fg_containing(&lines, "StringVec"),
            Some(string_color.into())
        );
        assert_eq!(
            span_fg_containing(&lines, "StringType"),
            Some(string_color.into())
        );
        assert_eq!(
            span_fg_containing(&lines, "CommentPaneId"),
            Some(comment_color.into())
        );
    }

    #[test]
    fn rust_semantic_fallbacks_color_associated_and_path_function_calls() {
        let function_color = Color::Rgb(250, 178, 131);
        let type_color = Color::Rgb(229, 192, 123);

        let palette = SyntaxPalette {
            function: Style::new().fg(function_color),
            type_name: Style::new().fg(type_color),
            ..AppTheme::default().syntax
        };
        let mut strategy = SyntectStrategy::default().default_theme("One Dark (Atom)");
        strategy.set_app_theme_if_absent(AppTheme::default().syntax(palette));

        let lines = strategy.highlight(TextAreaColorInput {
            value: concat!(
                "let root: Element = ScrollView::new()\n",
                "crate::style::apply_document_theme_carve_out(&theme, root)\n",
            ),
            language: Some("rust"),
            theme: Some("One Dark (Atom)"),
        });

        assert_eq!(
            span_fg_containing(&lines, "ScrollView"),
            Some(type_color.into())
        );
        assert_eq!(
            span_fg_containing(&lines, "new"),
            Some(function_color.into())
        );
        assert_eq!(
            span_fg_containing(&lines, "apply_document_theme_carve_out"),
            Some(function_color.into())
        );
    }

    #[test]
    fn fenced_bash_command_in_markdown_uses_variable_palette() {
        let function_color = Color::Rgb(10, 200, 120);
        let variable_color = Color::Rgb(220, 80, 90);

        let palette = SyntaxPalette {
            function: Style::new().fg(function_color),
            variable: Style::new().fg(variable_color),
            ..AppTheme::default().syntax
        };
        let mut strategy = SyntectStrategy::default().default_theme("One Dark (Atom)");
        strategy.set_app_theme_if_absent(AppTheme::default().syntax(palette));

        let markdown = "```bash\nopencode run\n```\n";
        let lines = strategy.highlight(TextAreaColorInput {
            value: markdown,
            language: Some("markdown"),
            theme: Some("One Dark (Atom)"),
        });

        let code_line = lines
            .iter()
            .find(|spans| {
                spans
                    .iter()
                    .any(|s| s.content.as_ref().contains("opencode"))
            })
            .expect("fenced bash line present");
        let command_fg = code_line
            .iter()
            .find(|s| s.content.as_ref().contains("opencode"))
            .and_then(|s| s.style.fg);

        assert_eq!(
            command_fg,
            Some(variable_color.into()),
            "fenced-block bash command should match upstream function.call/variable styling"
        );
        assert_ne!(command_fg, Some(function_color.into()));
    }

    #[test]
    fn language_for_path_resolves_rust() {
        let strategy = SyntectStrategy::default();
        assert_eq!(
            strategy.language_for_path("src/main.rs").as_deref(),
            Some("Rust")
        );
    }

    #[test]
    fn language_for_path_resolves_python() {
        let strategy = SyntectStrategy::default();
        assert_eq!(
            strategy.language_for_path("scripts/run.py").as_deref(),
            Some("Python")
        );
    }

    #[cfg(not(feature = "syntax-extra"))]
    #[test]
    fn language_for_path_falls_back_typescript_to_javascript() {
        let strategy = SyntectStrategy::default();
        assert_eq!(
            strategy.language_for_path("src/app.ts").as_deref(),
            Some("JavaScript")
        );
        assert_eq!(
            strategy.language_for_path("src/app.tsx").as_deref(),
            Some("JavaScript")
        );
    }

    #[cfg(not(feature = "syntax-extra"))]
    #[test]
    fn baseline_syntax_set_does_not_include_toml() {
        let strategy = SyntectStrategy::default();
        assert!(strategy.language_for_path("Cargo.toml").is_none());
    }

    #[cfg(feature = "syntax-extra")]
    #[test]
    fn extra_syntax_set_resolves_broad_grammar_coverage() {
        let strategy = SyntectStrategy::default();
        for (path, language) in [
            ("Cargo.toml", "TOML"),
            ("Dockerfile", "Dockerfile"),
            ("src/App.vue", "Vue Component"),
            ("build/main.zig", "Zig"),
            ("infra/main.tf", "Terraform"),
            ("src/app.ts", "TypeScript"),
            ("src/app.tsx", "TypeScriptReact"),
        ] {
            assert_eq!(
                strategy.language_for_path(path).as_deref(),
                Some(language),
                "language for {path}"
            );
        }
    }

    #[cfg(feature = "syntax-extra")]
    #[test]
    fn extra_syntax_set_resolves_only_committed_filename_aliases() {
        let strategy = SyntectStrategy::default();
        assert_eq!(
            strategy.language_for_path("rust-toolchain").as_deref(),
            Some("TOML")
        );
        assert_eq!(
            strategy.language_for_path("Dockerfile.dev").as_deref(),
            Some("Dockerfile")
        );

        // Lock files vary by package manager: package-lock.json is JSON,
        // pnpm-lock.yaml is YAML, and yarn.lock has its own format. Do not
        // add a generic `.lock` alias.
        assert!(strategy.language_for_path("yarn.lock").is_none());
        assert!(strategy.language_for_path("Justfile").is_none());
    }

    #[cfg(feature = "syntax-extra")]
    #[test]
    fn extra_syntax_set_styles_toml_spans() {
        let strategy = SyntectStrategy::default();
        let lines = strategy.highlight(TextAreaColorInput {
            value: "[package]\nname = \"tui-lipan\"\nversion = 1",
            language: Some("toml"),
            theme: Some("base16-ocean.dark"),
        });

        assert!(
            lines
                .iter()
                .flat_map(|line| line.iter())
                .any(|span| span.style.fg.is_some()),
            "TOML highlighting should produce styled spans"
        );
    }

    #[test]
    fn custom_syntax_sets_remain_authoritative() {
        let strategy = SyntectStrategy::with_sets(
            Rc::new(SyntaxSet::load_defaults_newlines()),
            Rc::new(ThemeSet::load_defaults()),
        );
        assert_eq!(
            strategy.language_for_path("src/app.ts").as_deref(),
            Some("JavaScript")
        );
        assert!(strategy.language_for_path("Cargo.toml").is_none());
    }

    #[test]
    fn language_for_path_returns_none_for_unknown() {
        let strategy = SyntectStrategy::default();
        assert!(strategy.language_for_path("data.xyz123").is_none());
    }

    #[cfg(not(feature = "syntax-extra"))]
    #[test]
    fn language_from_path_free_function() {
        assert_eq!(
            super::language_from_path("foo/bar.py").as_deref(),
            Some("Python")
        );
        assert_eq!(
            super::language_from_path("foo/bar.tsx").as_deref(),
            Some("JavaScript")
        );
        assert!(super::language_from_path("unknown.xyz123").is_none());
    }

    #[test]
    fn language_for_path_and_first_line_detects_shebang() {
        let strategy = SyntectStrategy::default();
        let lang = strategy.language_for_path_and_first_line("script", "#!/usr/bin/env python3");
        assert_eq!(lang.as_deref(), Some("Python"));
    }
}
