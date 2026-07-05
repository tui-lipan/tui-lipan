//! Shared helpers for nucleo-based matching.

use nucleo::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32String};

/// Matching mode for nucleo-based widgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MatchMode {
    /// Fuzzy matching (default).
    #[default]
    Fuzzy,
    /// Substring matching.
    Substring,
    /// Exact matching.
    Exact,
}

impl MatchMode {
    /// Return a short human-readable label for UI controls.
    pub fn label(self) -> &'static str {
        match self {
            MatchMode::Fuzzy => "Mode: FUZZY",
            MatchMode::Substring => "Mode: SUBSTRING",
            MatchMode::Exact => "Mode: EXACT",
        }
    }

    pub(crate) fn atom_kind(self) -> AtomKind {
        match self {
            MatchMode::Fuzzy => AtomKind::Fuzzy,
            MatchMode::Substring => AtomKind::Substring,
            MatchMode::Exact => AtomKind::Exact,
        }
    }
}

pub(crate) struct NucleoMatcher {
    matcher: Matcher,
    /// Cached pattern state to avoid re-creating Pattern on every call.
    cached_query: String,
    cached_mode: MatchMode,
    cached_case: Option<CaseMatching>,
    cached_norm: Option<Normalization>,
    cached_pattern: Option<Pattern>,
}

impl Default for NucleoMatcher {
    fn default() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            cached_query: String::new(),
            cached_mode: MatchMode::Fuzzy,
            cached_case: None,
            cached_norm: None,
            cached_pattern: None,
        }
    }
}

impl NucleoMatcher {
    pub(crate) fn match_indices(
        &mut self,
        haystack: &Utf32String,
        query: &str,
        mode: MatchMode,
        case_matching: CaseMatching,
        normalization: Normalization,
        hits: &mut Vec<u32>,
    ) -> Option<u32> {
        // Only rebuild the Pattern when the query or matching parameters change.
        let needs_rebuild = self.cached_pattern.is_none()
            || self.cached_query != query
            || self.cached_mode != mode
            || self.cached_case != Some(case_matching)
            || self.cached_norm != Some(normalization);

        if needs_rebuild {
            self.cached_query.clear();
            self.cached_query.push_str(query);
            self.cached_mode = mode;
            self.cached_case = Some(case_matching);
            self.cached_norm = Some(normalization);
            self.cached_pattern = Some(Pattern::new(
                query,
                case_matching,
                normalization,
                mode.atom_kind(),
            ));
        }

        self.cached_pattern
            .as_ref()
            .unwrap()
            .indices(haystack.slice(..), &mut self.matcher, hits)
    }
}
