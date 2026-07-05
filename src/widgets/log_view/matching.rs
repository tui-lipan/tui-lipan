use nucleo::Utf32String;
use nucleo::pattern::{CaseMatching, Normalization};

use super::LogEntry;
use crate::utils::nucleo::{MatchMode, NucleoMatcher};

#[derive(Clone, Debug, Default)]
pub struct LogSearchResult {
    pub source_index: usize,
    pub hits: Vec<u32>,
}

pub fn match_logs(
    entries: &[LogEntry],
    query: &str,
    mode: MatchMode,
    case_matching: CaseMatching,
    normalization: Normalization,
) -> Vec<LogSearchResult> {
    let query = query.trim();
    if query.is_empty() {
        return entries
            .iter()
            .enumerate()
            .map(|(index, _)| LogSearchResult {
                source_index: index,
                hits: Vec::new(),
            })
            .collect();
    }

    let mut matcher = NucleoMatcher::default();

    let mut results = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let mut hits = Vec::new();
        let text_utf32 = Utf32String::from(entry.message.as_ref());

        if matcher
            .match_indices(
                &text_utf32,
                query,
                mode,
                case_matching,
                normalization,
                &mut hits,
            )
            .is_some()
        {
            hits.sort_unstable();
            hits.dedup();
            results.push(LogSearchResult {
                source_index: index,
                hits,
            });
        }
    }

    // For logs, we typically want to preserve chronological order unless it's a pure search.
    // However, if we want "Optimization", sorting by score might be expected for fuzzy search.
    // Let's stick to source order for now as it's a "LogView".
    results.sort_by_key(|r| r.source_index);
    results
}
