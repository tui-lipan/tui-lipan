use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use nucleo::Utf32String;
use nucleo::pattern::{CaseMatching, Normalization};

use super::{SearchItem, SearchMatchMode};
#[cfg(not(target_arch = "wasm32"))]
use crate::utils::nucleo::{MatchMode, NucleoMatcher};

#[derive(Clone, Debug, Default)]
pub(super) struct SearchResult {
    pub(super) item_index: usize,
    pub(super) score: u32,
    pub(super) label_hits: Vec<u32>,
    pub(super) description_hits: Vec<u32>,
    pub(super) description_right_hits: Vec<u32>,
}

#[derive(Clone, Debug)]
pub(super) struct SearchEntry {
    label: Arc<str>,
    description: Option<Arc<str>>,
    description_right: Option<Arc<str>>,
    aliases: Vec<Arc<str>>,
}

/// True when the hit indices form a contiguous run, meaning the query matched
/// the haystack as an exact substring. Nucleo's default scoring under-rewards
/// substring matches relative to scattered prefix matches; callers add a flat
/// boost when this returns true so an exact substring outranks a fuzzy one.
fn is_contiguous_run(hits: &[u32]) -> bool {
    hits.len() >= 2 && hits.windows(2).all(|w| w[1] == w[0] + 1)
}

pub(super) fn build_search_entries<T>(items: &[SearchItem<T>]) -> Vec<SearchEntry> {
    items
        .iter()
        .map(|item| SearchEntry {
            label: item.label.clone(),
            description: item.description.as_ref().and_then(|d| d.left.clone()),
            description_right: item.description.as_ref().and_then(|d| d.right.clone()),
            aliases: item.aliases.clone(),
        })
        .collect()
}

pub(super) fn all_item_results(len: usize) -> Vec<SearchResult> {
    (0..len)
        .map(|index| SearchResult {
            item_index: index,
            score: 0,
            label_hits: Vec::new(),
            description_hits: Vec::new(),
            description_right_hits: Vec::new(),
        })
        .collect()
}

pub(super) fn match_items(
    items: &[SearchEntry],
    query: &str,
    mode: SearchMatchMode,
    case_matching: CaseMatching,
    normalization: Normalization,
) -> Vec<SearchResult> {
    let query = query.trim();
    if query.is_empty() {
        return all_item_results(items.len());
    }

    match mode {
        SearchMatchMode::Fuzzy => match_items_fuzzy(items, query, case_matching, normalization),
        SearchMatchMode::Hybrid => hybrid::match_items_hybrid(items, query, case_matching),
    }
}

fn match_items_fuzzy(
    items: &[SearchEntry],
    query: &str,
    case_matching: CaseMatching,
    normalization: Normalization,
) -> Vec<SearchResult> {
    #[cfg(target_arch = "wasm32")]
    {
        return match_items_wasm_fallback(items, query, case_matching, normalization);
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut matcher = NucleoMatcher::default();
        let mode = MatchMode::Fuzzy;

        let mut results = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let mut label_hits = Vec::new();
            let mut desc_hits = Vec::new();
            let mut desc_right_hits = Vec::new();

            let label_utf32 = Utf32String::from(item.label.as_ref());
            let label_score = matcher.match_indices(
                &label_utf32,
                query,
                mode,
                case_matching,
                normalization,
                &mut label_hits,
            );

            let mut score = label_score.unwrap_or(0);
            let mut matched = label_score.is_some();

            if matched && is_contiguous_run(&label_hits) {
                score = score.saturating_add(score / 2);
            }

            // Aliases compete with the label via max(): a strong alias hit can
            // promote a row whose canonical label barely matches (or doesn't
            // match at all), but it never penalizes a stronger label match.
            for alias in &item.aliases {
                let alias_utf32 = Utf32String::from(alias.as_ref());
                let mut alias_hits = Vec::new();
                if let Some(mut alias_score) = matcher.match_indices(
                    &alias_utf32,
                    query,
                    mode,
                    case_matching,
                    normalization,
                    &mut alias_hits,
                ) {
                    if is_contiguous_run(&alias_hits) {
                        alias_score = alias_score.saturating_add(alias_score / 2);
                    }
                    if alias_score > score {
                        score = alias_score;
                    }
                    matched = true;
                }
            }

            if let Some(desc) = &item.description {
                let desc_utf32 = Utf32String::from(desc.as_ref());
                if let Some(desc_score) = matcher.match_indices(
                    &desc_utf32,
                    query,
                    mode,
                    case_matching,
                    normalization,
                    &mut desc_hits,
                ) {
                    score = score.saturating_add(desc_score);
                    matched = true;
                }
            }

            if let Some(right) = &item.description_right {
                let right_utf32 = Utf32String::from(right.as_ref());
                if let Some(right_score) = matcher.match_indices(
                    &right_utf32,
                    query,
                    mode,
                    case_matching,
                    normalization,
                    &mut desc_right_hits,
                ) {
                    score = score.saturating_add(right_score);
                    matched = true;
                }
            }

            if matched {
                label_hits.sort_unstable();
                label_hits.dedup();
                desc_hits.sort_unstable();
                desc_hits.dedup();
                desc_right_hits.sort_unstable();
                desc_right_hits.dedup();

                results.push(SearchResult {
                    item_index: index,
                    score,
                    label_hits,
                    description_hits: desc_hits,
                    description_right_hits: desc_right_hits,
                });
            }
        }

        results.sort_by(|a, b| b.score.cmp(&a.score).then(a.item_index.cmp(&b.item_index)));
        results
    }
}

/// Hybrid matching: exact/prefix/word-prefix/substring/fuzzy tiers evaluated
/// together, per field, so a real substring/prefix match always outranks a
/// fuzzy one and weak scattered fuzzy matches are rejected.
mod hybrid {
    use nucleo::pattern::CaseMatching;

    use super::{SearchEntry, SearchResult};

    /// Priority tier a field match falls into. Ordered so that `Ord`
    /// comparison (and the numeric `rank`) directly encodes the required
    /// priority: Exact > Prefix > WordPrefix > Substring > Fuzzy.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum MatchTier {
        Fuzzy,
        Substring,
        WordPrefix,
        Prefix,
        Exact,
    }

    impl MatchTier {
        fn rank(self) -> u32 {
            match self {
                MatchTier::Fuzzy => 0,
                MatchTier::Substring => 1,
                MatchTier::WordPrefix => 2,
                MatchTier::Prefix => 3,
                MatchTier::Exact => 4,
            }
        }
    }

    /// Which searchable field a candidate haystack belongs to. Drives both
    /// the field weight and which tiers are attempted: keybinding-style hints
    /// only use exact/substring matching.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum FieldRole {
        /// Label and aliases: the primary identity of the item.
        Primary,
        /// Description text.
        Description,
        /// Right-hand hint (e.g. a keybinding). Exact/substring only.
        Hint,
    }

    impl FieldRole {
        fn weight(self) -> f64 {
            match self {
                FieldRole::Primary => 3.0,
                FieldRole::Description => 1.0,
                FieldRole::Hint => 1.0,
            }
        }

        fn allow_word_prefix_and_fuzzy(self) -> bool {
            !matches!(self, FieldRole::Hint)
        }
    }

    /// A single field's match result: the tier it was accepted at, a
    /// within-tier quality in roughly `0.0..=1.2`, and the matched character
    /// indices (used for highlighting).
    struct FieldMatch {
        tier: MatchTier,
        quality: f64,
        hits: Vec<u32>,
    }

    /// Tier contribution dwarfs weight, which in turn dwarfs quality, so
    /// match-type priority always wins, field weight is the tie-breaker
    /// across tiers-equal fields, and quality only fine-tunes within that.
    const TIER_UNIT: f64 = 1_000_000.0;
    const WEIGHT_UNIT: f64 = 1_000.0;
    const QUALITY_UNIT: f64 = 500.0;

    /// Minimum composite quality (density, span, start position, and
    /// word-boundary cohesion) a fuzzy candidate must clear to be accepted.
    /// Tuned so that abbreviation-style matches like `prd` -> `production`
    /// pass while sparse, multi-word matches like `layo` -> `Enable pane
    /// synchronization` are rejected.
    const FUZZY_QUALITY_THRESHOLD: f64 = 0.45;

    fn field_total(tier: MatchTier, quality: f64, role: FieldRole) -> f64 {
        tier.rank() as f64 * TIER_UNIT
            + role.weight() * WEIGHT_UNIT
            + quality.clamp(0.0, 1.5) * QUALITY_UNIT
    }

    fn respects_case(case_matching: CaseMatching, query_chars: &[char]) -> bool {
        match case_matching {
            CaseMatching::Respect => true,
            CaseMatching::Ignore => false,
            CaseMatching::Smart => query_chars.iter().any(|ch| ch.is_uppercase()),
            _ => query_chars.iter().any(|ch| ch.is_uppercase()),
        }
    }

    fn fold_char(c: char, respect_case: bool) -> char {
        if respect_case {
            c
        } else {
            c.to_lowercase().next().unwrap_or(c)
        }
    }

    /// Splits `chars` into `[start, end)` ranges of contiguous alphanumeric
    /// runs, treating any other character (space, `-`, `_`, `/`, punctuation)
    /// as a word boundary.
    fn split_words(chars: &[char]) -> Vec<(usize, usize)> {
        let mut words = Vec::new();
        let mut start = None;
        for (i, c) in chars.iter().enumerate() {
            if c.is_alphanumeric() {
                if start.is_none() {
                    start = Some(i);
                }
            } else if let Some(s) = start.take() {
                words.push((s, i));
            }
        }
        if let Some(s) = start {
            words.push((s, chars.len()));
        }
        words
    }

    /// Finds the first word whose prefix matches `query`, returning
    /// `(word_start, word_len, word_index, word_count)`.
    fn find_word_prefix(hay: &[char], query: &[char]) -> Option<(usize, usize, usize, usize)> {
        let words = split_words(hay);
        let count = words.len();
        for (idx, &(s, e)) in words.iter().enumerate() {
            let word_len = e - s;
            if word_len >= query.len() && hay[s..s + query.len()] == query[..] {
                return Some((s, word_len, idx, count));
            }
        }
        None
    }

    /// Finds the first contiguous occurrence of `query` anywhere in `hay`.
    fn find_contiguous(hay: &[char], query: &[char]) -> Option<usize> {
        if query.is_empty() || hay.len() < query.len() {
            return None;
        }
        (0..=hay.len() - query.len()).find(|&i| hay[i..i + query.len()] == query[..])
    }

    /// Fraction of `hits` that fall within the single word most of them land
    /// in. `1.0` when every hit stays inside one word; lower as hits spread
    /// across more words.
    fn word_fraction_for_hits(hits: &[u32], words: &[(usize, usize)]) -> f64 {
        if hits.is_empty() || words.is_empty() {
            return 0.0;
        }
        let mut counts = vec![0usize; words.len()];
        for &h in hits {
            let h = h as usize;
            if let Some(word_idx) = words.iter().position(|&(s, e)| h >= s && h < e) {
                counts[word_idx] += 1;
            }
        }
        let max_count = counts.into_iter().max().unwrap_or(0);
        max_count as f64 / hits.len() as f64
    }

    /// Composite quality score (`0.0..~1.2`) for a fuzzy match: rewards tight
    /// density and short spans, an early start, a decent matcher score, and
    /// staying mostly within a single word; penalizes the opposite.
    fn fuzzy_quality(matcher_score: u32, hits: &[u32], query_len: usize, hay: &[char]) -> f64 {
        let Some(&first) = hits.first() else {
            return 0.0;
        };
        let last = *hits.last().unwrap();
        let span = (last - first + 1) as f64;
        let density = query_len as f64 / span;
        let start_score = 1.0 / (1.0 + first as f64 / 10.0);
        let span_score = 1.0 / (1.0 + (span - query_len as f64).max(0.0) / 8.0);
        let matcher_norm = (matcher_score as f64 / (query_len.max(1) as f64 * 48.0)).min(1.2);

        let words = split_words(hay);
        let word_fraction = word_fraction_for_hits(hits, &words);

        let base = 0.30 * density + 0.25 * span_score + 0.20 * start_score + 0.25 * matcher_norm;
        base * word_fraction.clamp(0.35, 1.0).powf(1.4)
    }

    /// Fuzzy-tier backend, unified behind a single type so `classify_field`
    /// has one signature on every target: `nucleo`'s matcher natively, or a
    /// zero-sized marker on wasm32 (which instead calls the subsequence
    /// fallback used by [`super::match_items_fuzzy`]).
    #[cfg(not(target_arch = "wasm32"))]
    type FuzzyMatcher = super::NucleoMatcher;
    #[cfg(target_arch = "wasm32")]
    type FuzzyMatcher = ();

    #[cfg(not(target_arch = "wasm32"))]
    fn fuzzy_field_hits(
        haystack: &str,
        query: &str,
        case_matching: CaseMatching,
        matcher: &mut FuzzyMatcher,
    ) -> Option<(u32, Vec<u32>)> {
        use nucleo::Utf32String;
        use nucleo::pattern::Normalization;

        let haystack_utf32 = Utf32String::from(haystack);
        let mut hits = Vec::new();
        let score = matcher.match_indices(
            &haystack_utf32,
            query,
            super::MatchMode::Fuzzy,
            case_matching,
            Normalization::Never,
            &mut hits,
        )?;
        hits.sort_unstable();
        hits.dedup();
        Some((score, hits))
    }

    #[cfg(target_arch = "wasm32")]
    fn fuzzy_field_hits(
        haystack: &str,
        query: &str,
        case_matching: CaseMatching,
        _matcher: &mut FuzzyMatcher,
    ) -> Option<(u32, Vec<u32>)> {
        super::simple_match(haystack, query, case_matching)
    }

    /// Classifies a single field independently against `query`, trying tiers
    /// in priority order and stopping at the first that matches (so a real
    /// prefix never falls through to a weaker fuzzy score). Exact/prefix/
    /// word-prefix/substring are plain character comparisons; only the fuzzy
    /// tier delegates to the shared matcher (`nucleo` natively, or the
    /// wasm32 subsequence fallback), and is quality-gated so weak scattered
    /// matches are rejected outright.
    fn classify_field(
        haystack: &str,
        query: &str,
        case_matching: CaseMatching,
        role: FieldRole,
        matcher: &mut FuzzyMatcher,
    ) -> Option<FieldMatch> {
        if haystack.is_empty() {
            return None;
        }

        let hay_chars: Vec<char> = haystack.chars().collect();
        let query_chars: Vec<char> = query.chars().collect();
        if query_chars.is_empty() {
            return None;
        }
        let respect_case = respects_case(case_matching, &query_chars);
        let hay_folded: Vec<char> = hay_chars
            .iter()
            .map(|&c| fold_char(c, respect_case))
            .collect();
        let query_folded: Vec<char> = query_chars
            .iter()
            .map(|&c| fold_char(c, respect_case))
            .collect();

        if hay_folded == query_folded {
            return Some(FieldMatch {
                tier: MatchTier::Exact,
                quality: 1.0,
                hits: (0..hay_chars.len() as u32).collect(),
            });
        }

        if hay_folded.len() > query_folded.len()
            && hay_folded[..query_folded.len()] == query_folded[..]
        {
            let hits: Vec<u32> = (0..query_chars.len() as u32).collect();
            let ratio = query_chars.len() as f64 / hay_chars.len() as f64;
            return Some(FieldMatch {
                tier: MatchTier::Prefix,
                quality: ratio.clamp(0.0, 1.0),
                hits,
            });
        }

        if role.allow_word_prefix_and_fuzzy()
            && let Some((word_start, word_len, word_index, word_count)) =
                find_word_prefix(&hay_folded, &query_folded)
        {
            let hits: Vec<u32> =
                (word_start as u32..(word_start + query_chars.len()) as u32).collect();
            let ratio = query_chars.len() as f64 / word_len as f64;
            let position_bonus = 1.0 - (word_index as f64 / word_count.max(1) as f64) * 0.3;
            let quality = (0.7 * ratio + 0.3 * position_bonus).clamp(0.0, 1.0);
            return Some(FieldMatch {
                tier: MatchTier::WordPrefix,
                quality,
                hits,
            });
        }

        if let Some(start) = find_contiguous(&hay_folded, &query_folded) {
            let hits: Vec<u32> = (start as u32..(start + query_chars.len()) as u32).collect();
            let start_score = 1.0 / (1.0 + start as f64 / 10.0);
            let compactness = query_chars.len() as f64 / hay_chars.len() as f64;
            let quality = (0.6 * start_score + 0.4 * compactness).clamp(0.0, 1.0);
            return Some(FieldMatch {
                tier: MatchTier::Substring,
                quality,
                hits,
            });
        }

        if role.allow_word_prefix_and_fuzzy()
            && let Some((score, hits)) = fuzzy_field_hits(haystack, query, case_matching, matcher)
        {
            let quality = fuzzy_quality(score, &hits, query_chars.len(), &hay_folded);
            if quality >= FUZZY_QUALITY_THRESHOLD {
                return Some(FieldMatch {
                    tier: MatchTier::Fuzzy,
                    quality,
                    hits,
                });
            }
        }

        None
    }

    /// Best field-total across label and aliases: aliases are never
    /// rendered, so they only compete for the score via `max()` and never
    /// contribute their own hits.
    fn best_primary_total(
        item: &SearchEntry,
        query: &str,
        case_matching: CaseMatching,
        matcher: &mut FuzzyMatcher,
        label_match: &Option<FieldMatch>,
    ) -> Option<f64> {
        let mut best = label_match
            .as_ref()
            .map(|m| field_total(m.tier, m.quality, FieldRole::Primary));
        for alias in &item.aliases {
            if let Some(m) = classify_field(
                alias.as_ref(),
                query,
                case_matching,
                FieldRole::Primary,
                matcher,
            ) {
                let total = field_total(m.tier, m.quality, FieldRole::Primary);
                if best.is_none_or(|cur| total > cur) {
                    best = Some(total);
                }
            }
        }
        best
    }

    pub(super) fn match_items_hybrid(
        items: &[SearchEntry],
        query: &str,
        case_matching: CaseMatching,
    ) -> Vec<SearchResult> {
        let mut matcher = FuzzyMatcher::default();

        let mut results = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let label_match = classify_field(
                item.label.as_ref(),
                query,
                case_matching,
                FieldRole::Primary,
                &mut matcher,
            );
            let label_hits = label_match
                .as_ref()
                .map(|m| m.hits.clone())
                .unwrap_or_default();
            let best_primary =
                best_primary_total(item, query, case_matching, &mut matcher, &label_match);

            let desc_match = item.description.as_deref().and_then(|desc| {
                classify_field(
                    desc,
                    query,
                    case_matching,
                    FieldRole::Description,
                    &mut matcher,
                )
            });
            let desc_hits = desc_match
                .as_ref()
                .map(|m| m.hits.clone())
                .unwrap_or_default();
            let desc_total =
                desc_match.map(|m| field_total(m.tier, m.quality, FieldRole::Description));

            let hint_match = item.description_right.as_deref().and_then(|hint| {
                classify_field(hint, query, case_matching, FieldRole::Hint, &mut matcher)
            });
            let hint_hits = hint_match
                .as_ref()
                .map(|m| m.hits.clone())
                .unwrap_or_default();
            let hint_total = hint_match.map(|m| field_total(m.tier, m.quality, FieldRole::Hint));

            let overall = [best_primary, desc_total, hint_total]
                .into_iter()
                .flatten()
                .fold(None, |acc: Option<f64>, v| {
                    Some(acc.map_or(v, |a| a.max(v)))
                });

            if let Some(score) = overall {
                results.push(SearchResult {
                    item_index: index,
                    score: score.round().clamp(0.0, u32::MAX as f64) as u32,
                    label_hits,
                    description_hits: desc_hits,
                    description_right_hits: hint_hits,
                });
            }
        }

        results.sort_by(|a, b| b.score.cmp(&a.score).then(a.item_index.cmp(&b.item_index)));
        results
    }
}

#[cfg(test)]
mod tests {
    use nucleo::pattern::{CaseMatching, Normalization};

    use super::{SearchEntry, SearchItem, SearchMatchMode, all_item_results, match_items};

    fn entries(labels: &[&str]) -> Vec<SearchEntry> {
        let items: Vec<SearchItem<usize>> = labels
            .iter()
            .enumerate()
            .map(|(i, label)| SearchItem::new(*label, i))
            .collect();
        super::build_search_entries(&items)
    }

    fn hybrid_match(labels: &[&str], query: &str) -> Vec<super::SearchResult> {
        match_items(
            &entries(labels),
            query,
            SearchMatchMode::Hybrid,
            CaseMatching::Smart,
            Normalization::Smart,
        )
    }

    #[test]
    fn all_item_results_preserves_item_order_with_empty_hits() {
        let results = all_item_results(3);

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].item_index, 0);
        assert_eq!(results[1].item_index, 1);
        assert_eq!(results[2].item_index, 2);
        assert!(results.iter().all(|result| result.score == 0));
        assert!(results.iter().all(|result| result.label_hits.is_empty()));
        assert!(
            results
                .iter()
                .all(|result| result.description_hits.is_empty())
        );
        assert!(
            results
                .iter()
                .all(|result| result.description_right_hits.is_empty())
        );
    }

    #[test]
    fn hybrid_substring_ranks_above_fuzzy() {
        // "concatenate" contains "cat" as a contiguous substring; "cabinet"
        // only matches "cat" via scattered fuzzy characters (c-a...-t).
        let results = hybrid_match(&["concatenate", "cabinet"], "cat");

        assert_eq!(results.len(), 2, "both items should match: {results:?}");
        assert_eq!(
            results[0].item_index, 0,
            "substring match must rank first: {results:?}"
        );
        assert_eq!(results[1].item_index, 1);
    }

    #[test]
    fn hybrid_rejects_weak_sparse_fuzzy_matches() {
        // "layo" only reaches "Enable pane synchronization" through four
        // scattered characters spread across three different words - too
        // weak to be a useful match.
        let results = hybrid_match(&["Enable pane synchronization"], "layo");

        assert!(
            results.is_empty(),
            "weak scattered fuzzy match should be rejected: {results:?}"
        );
    }

    #[test]
    fn hybrid_keeps_abbreviation_fuzzy_matches() {
        // "prd" -> "production" is a tight, single-word, early-starting
        // abbreviation and should still match.
        let results = hybrid_match(&["production"], "prd");

        assert_eq!(
            results.len(),
            1,
            "abbreviation match should pass: {results:?}"
        );
        assert_eq!(results[0].item_index, 0);
    }

    #[test]
    fn hybrid_matches_cannot_span_multiple_fields() {
        let items = vec![
            SearchItem::new("fooa", 0).description("bc"),
            SearchItem::new("other", 1),
        ];
        let entries = super::build_search_entries(&items);

        // "abc" is not present in the label ("fooa") nor the description
        // ("bc") alone; it must not match by combining characters across
        // both fields.
        let results = match_items(
            &entries,
            "abc",
            SearchMatchMode::Hybrid,
            CaseMatching::Smart,
            Normalization::Smart,
        );

        assert!(
            results.is_empty(),
            "fields must not combine into a single match: {results:?}"
        );
    }

    #[test]
    fn hybrid_prefix_ranks_above_inner_substring() {
        // "logger" starts with "log" (prefix); "catalog" only contains "log"
        // as an inner substring.
        let results = hybrid_match(&["logger", "catalog"], "log");

        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].item_index, 0,
            "prefix match must rank above inner substring: {results:?}"
        );
        assert_eq!(results[1].item_index, 1);
    }

    #[test]
    fn hybrid_empty_query_preserves_original_order() {
        let results = hybrid_match(&["zebra", "apple", "mango"], "");

        assert_eq!(
            results.iter().map(|r| r.item_index).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(results.iter().all(|r| r.score == 0));
    }
}

#[cfg(target_arch = "wasm32")]
fn match_items_wasm_fallback(
    items: &[SearchEntry],
    query: &str,
    case_matching: CaseMatching,
    _normalization: Normalization,
) -> Vec<SearchResult> {
    let mut results = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let (label_score, mut label_hits) =
            simple_match(item.label.as_ref(), query, case_matching).unwrap_or((0, Vec::new()));

        let mut score = label_score;
        let mut matched = !label_hits.is_empty();

        if matched && is_contiguous_run(&label_hits) {
            score = score.saturating_add(score / 2);
        }

        for alias in &item.aliases {
            if let Some((mut alias_score, alias_hits)) =
                simple_match(alias.as_ref(), query, case_matching)
            {
                if is_contiguous_run(&alias_hits) {
                    alias_score = alias_score.saturating_add(alias_score / 2);
                }
                if alias_score > score {
                    score = alias_score;
                }
                matched = true;
            }
        }

        let (desc_score, mut desc_hits) = item
            .description
            .as_deref()
            .and_then(|desc| simple_match(desc, query, case_matching))
            .unwrap_or((0, Vec::new()));
        if desc_score > 0 {
            score = score.saturating_add(desc_score);
            matched = true;
        }

        let (desc_right_score, mut desc_right_hits) = item
            .description_right
            .as_deref()
            .and_then(|desc| simple_match(desc, query, case_matching))
            .unwrap_or((0, Vec::new()));
        if desc_right_score > 0 {
            score = score.saturating_add(desc_right_score);
            matched = true;
        }

        if matched {
            label_hits.sort_unstable();
            label_hits.dedup();
            desc_hits.sort_unstable();
            desc_hits.dedup();
            desc_right_hits.sort_unstable();
            desc_right_hits.dedup();

            results.push(SearchResult {
                item_index: index,
                score,
                label_hits,
                description_hits: desc_hits,
                description_right_hits: desc_right_hits,
            });
        }
    }

    results.sort_by(|a, b| b.score.cmp(&a.score).then(a.item_index.cmp(&b.item_index)));
    results
}

#[cfg(target_arch = "wasm32")]
fn simple_match(
    haystack: &str,
    query: &str,
    case_matching: CaseMatching,
) -> Option<(u32, Vec<u32>)> {
    let haystack_chars: Vec<char> = haystack.chars().collect();
    let query_chars: Vec<char> = query.chars().collect();

    let respects_case = match case_matching {
        CaseMatching::Respect => true,
        CaseMatching::Ignore => false,
        CaseMatching::Smart => query_chars.iter().any(|ch| ch.is_uppercase()),
        _ => query_chars.iter().any(|ch| ch.is_uppercase()),
    };

    let chars_equal = |a: char, b: char| {
        if respects_case {
            a == b
        } else {
            a.to_lowercase().to_string() == b.to_lowercase().to_string()
        }
    };

    let mut hits = Vec::with_capacity(query_chars.len());
    let mut search_from = 0usize;
    for query_ch in query_chars {
        let Some(pos) = haystack_chars
            .iter()
            .enumerate()
            .skip(search_from)
            .find_map(|(idx, hay_ch)| chars_equal(*hay_ch, query_ch).then_some(idx))
        else {
            return None;
        };
        hits.push(pos as u32);
        search_from = pos.saturating_add(1);
    }

    let span = hits.last().copied().unwrap_or(0).saturating_sub(hits[0]);
    let contiguous_bonus = hits
        .windows(2)
        .filter(|pair| pair[1] == pair[0] + 1)
        .count() as u32
        * 8;
    let start_bonus = 64u32.saturating_sub(hits[0]);
    let compact_bonus = 32u32.saturating_sub(span);
    let length_bonus = (query.len() as u32).saturating_mul(16);
    let score = length_bonus
        .saturating_add(contiguous_bonus)
        .saturating_add(start_bonus)
        .saturating_add(compact_bonus);

    Some((score, hits))
}
