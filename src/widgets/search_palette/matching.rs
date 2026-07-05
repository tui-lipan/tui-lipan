use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use nucleo::Utf32String;
use nucleo::pattern::{CaseMatching, Normalization};

use super::SearchItem;
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
    case_matching: CaseMatching,
    normalization: Normalization,
) -> Vec<SearchResult> {
    let query = query.trim();
    if query.is_empty() {
        return all_item_results(items.len());
    }

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

#[cfg(test)]
mod tests {
    use super::all_item_results;

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
