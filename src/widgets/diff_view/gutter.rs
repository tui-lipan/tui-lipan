//! Diff gutter span construction and caching.

use super::*;

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;

use rustc_hash::FxHasher;
use std::sync::Arc;

pub(crate) type DiffGutterSpans = Arc<Vec<Vec<crate::style::Span>>>;
type DiffGutterSpansCache = HashMap<(u64, u64), DiffGutterSpans>;

thread_local! {
    static DIFF_GUTTER_SPANS_CACHE: RefCell<DiffGutterSpansCache> =
        RefCell::new(HashMap::new());
}
fn number_col(value: Option<usize>, width: usize) -> String {
    match value {
        Some(v) => format!("{v:>width$}", width = width),
        None => " ".repeat(width),
    }
}

fn max_digits_for(lines: &[DiffRenderLine], pick: fn(&DiffRenderLine) -> Option<usize>) -> usize {
    lines
        .iter()
        .filter_map(pick)
        .max()
        .unwrap_or(1)
        .to_string()
        .len()
}

pub(crate) fn add_source_line_numbers(
    render: &DiffRender,
    mode: DiffViewMode,
    pane: DiffPane,
    enabled: bool,
    min_digits: usize,
) -> DiffRender {
    if !enabled {
        return render.clone();
    }

    let left_w = max_digits_for(render.lines.as_ref(), |l| l.old_line).max(min_digits);
    let right_w = max_digits_for(render.lines.as_ref(), |l| l.new_line).max(min_digits);

    let mut lines = Vec::with_capacity(render.lines.len());
    for line in render.lines.iter() {
        let mut numbered = line.clone();
        let prefix = match mode {
            DiffViewMode::Split => {
                let (n, w) = match pane {
                    DiffPane::Left => (line.old_line, left_w),
                    DiffPane::Right => (line.new_line, right_w),
                    DiffPane::Unified => (line.old_line, left_w),
                };
                format!("{} {}", number_col(n, w), line.prefix)
            }
            DiffViewMode::Unified => {
                let width = left_w.max(right_w);
                let n = match line.kind {
                    DiffLineKind::Removed => line.old_line,
                    _ => line.new_line,
                };
                format!("{} {}", number_col(n, width), line.prefix)
            }
        };
        numbered.prefix = Arc::from(prefix);
        lines.push(numbered);
    }

    DiffRender::new(lines)
}
type CopyExcluded = (Arc<Vec<usize>>, Arc<Vec<(usize, usize)>>);

/// Compute excluded-copy information for Empty (filler) diff lines.
///
/// Returns:
/// - `excluded_source_lines`: indices of Empty logical lines (for DocumentView)
/// - `excluded_bytes`: byte ranges in `raw_text` to skip during copy (for TextArea)
fn build_copy_excluded(render: &DiffRender) -> CopyExcluded {
    let mut source_lines = Vec::new();
    let mut byte_ranges = Vec::new();
    let mut byte_offset = 0usize;
    for (idx, line) in render.lines.iter().enumerate() {
        if idx > 0 {
            // The \n separator pushed before this line's content is at byte_offset.
            if matches!(line.kind, DiffLineKind::Empty | DiffLineKind::Separator) {
                source_lines.push(idx);
                // For Empty text is "" so +text.len() is 0; for Separator we
                // also exclude the synthetic text content.
                byte_ranges.push((byte_offset, byte_offset + 1 + line.text.len()));
            }
            byte_offset += 1; // the \n separator
        } else if line.kind == DiffLineKind::Separator {
            // Edge case: first line is a separator.
            source_lines.push(idx);
            byte_ranges.push((byte_offset, byte_offset + line.text.len()));
        }
        byte_offset += line.text.len();
    }
    (Arc::new(source_lines), Arc::new(byte_ranges))
}

#[cfg(test)]
pub(crate) fn build_diff_gutter_from_lines(
    lines: &[DiffRenderLine],
    style: DiffPalette,
) -> (DiffGutterSpans, u16) {
    use unicode_width::UnicodeWidthStr;
    let col_width = lines
        .iter()
        .map(|l| UnicodeWidthStr::width(l.prefix.as_ref()) as u16)
        .max()
        .unwrap_or(0);

    let lines_hash = diff_render_lines_hash(lines);
    (
        cached_diff_gutter_spans(lines_hash, lines, style),
        col_width,
    )
}

/// Rebuild only the styled gutter spans without recomputing `col_width`.
///
/// `col_width` depends on prefix text widths which are style-independent,
/// so this avoids the redundant `UnicodeWidthStr` pass during theme changes.
pub(crate) fn rebuild_diff_gutter_spans(
    lines: &[DiffRenderLine],
    style: DiffPalette,
) -> DiffGutterSpans {
    let lines_hash = diff_render_lines_hash(lines);
    cached_diff_gutter_spans(lines_hash, lines, style)
}

fn cached_diff_gutter_spans(
    lines_hash: u64,
    lines: &[DiffRenderLine],
    style: DiffPalette,
) -> DiffGutterSpans {
    let key = (lines_hash, diff_style_hash(&style));
    if let Some(spans) = DIFF_GUTTER_SPANS_CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return spans;
    }

    let spans = build_diff_gutter_spans(lines, style);
    DIFF_GUTTER_SPANS_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= DIFF_GUTTER_SPANS_CACHE_LIMIT && !cache.contains_key(&key) {
            cache.clear();
        }
        cache.insert(key, Arc::clone(&spans));
    });
    spans
}

fn diff_render_lines_hash(lines: &[DiffRenderLine]) -> u64 {
    let mut h = FxHasher::default();
    for line in lines {
        line.hash(&mut h);
    }
    h.finish()
}

fn build_diff_gutter_spans(
    lines: &[DiffRenderLine],
    style: DiffPalette,
) -> Arc<Vec<Vec<crate::style::Span>>> {
    // Precompute the derived prefix style for each line kind that produces
    // gutter spans - avoids redundant Style::patch calls per line.
    use crate::style::Style;
    let prefix_style = |kind| {
        let line_style = style.line_style(kind);
        let content_style = line_overlay_style(line_style);
        prefix_overlay_style(content_style, kind)
    };
    let context_prefix_style = prefix_style(DiffLineKind::Context);
    let added_prefix_style = prefix_style(DiffLineKind::Added);
    let removed_prefix_style = prefix_style(DiffLineKind::Removed);
    let empty_prefix_style = prefix_style(DiffLineKind::Empty);

    let mut spans = Vec::with_capacity(lines.len());
    for line in lines {
        if line.prefix.is_empty() || matches!(line.kind, DiffLineKind::Separator) {
            spans.push(Vec::new());
            continue;
        }

        let prefix_style = match line.kind {
            DiffLineKind::Context => context_prefix_style,
            DiffLineKind::Added => added_prefix_style,
            DiffLineKind::Removed => removed_prefix_style,
            DiffLineKind::Empty => empty_prefix_style,
            DiffLineKind::PatchHeader | DiffLineKind::Separator => Style::default(),
        };
        spans.push(build_prefix_spans(
            line.prefix.as_ref(),
            line.kind,
            style,
            prefix_style,
        ));
    }

    Arc::new(spans)
}

fn build_diff_gutter(render: &DiffRender, style: DiffPalette) -> (DiffGutterSpans, u16) {
    use unicode_width::UnicodeWidthStr;
    let lines = render.lines.as_ref();
    let col_width = lines
        .iter()
        .map(|l| UnicodeWidthStr::width(l.prefix.as_ref()) as u16)
        .max()
        .unwrap_or(0);

    (
        cached_diff_gutter_spans(render.diff_hash, lines, style),
        col_width,
    )
}

/// Get or compute cached per-pane derived data.
pub(crate) fn get_pane_data(
    cache: &RefCell<Vec<PaneCacheEntry>>,
    render: &DiffRender,
    pane: DiffPane,
    options: PaneRenderOptions,
) -> PaneCacheEntry {
    let key = PaneCacheKey {
        diff_hash: render.diff_hash,
        mode: options.mode,
        pane,
        line_numbers: options.line_numbers,
        min_digits: options.min_digits,
    };
    let global_key = (key.clone(), options.gutter_style_hash);

    // Check cache.
    if let Some((index, entry)) = cache
        .borrow()
        .iter()
        .enumerate()
        .find(|(_, e)| e.key == key)
        .map(|(i, e)| (i, e.clone()))
    {
        if entry.gutter_style_hash == options.gutter_style_hash {
            return entry;
        }

        let gutter_spans =
            rebuild_diff_gutter_spans(entry.numbered_render.lines.as_ref(), options.style);
        let refreshed = PaneCacheEntry {
            gutter_style_hash: options.gutter_style_hash,
            gutter_spans,
            ..entry
        };
        cache.borrow_mut()[index] = refreshed.clone();
        PANE_DATA_CACHE
            .with(|global| store_global_pane_cache_entry(global, global_key, refreshed.clone()));
        return refreshed;
    }

    if let Some(entry) = PANE_DATA_CACHE.with(|global| global.borrow().get(&global_key).cloned()) {
        store_pane_cache_entry(cache, entry.clone());
        return entry;
    }

    // Compute.
    let numbered_render = add_source_line_numbers(
        render,
        options.mode,
        pane,
        options.line_numbers,
        options.min_digits,
    );
    let (gutter_spans, gutter_col_width) = build_diff_gutter(&numbered_render, options.style);
    let (excluded_source_lines, excluded_bytes) = build_copy_excluded(&numbered_render);

    let entry = PaneCacheEntry {
        key,
        gutter_style_hash: options.gutter_style_hash,
        numbered_render,
        gutter_spans,
        gutter_col_width,
        excluded_source_lines,
        excluded_bytes,
    };

    store_pane_cache_entry(cache, entry.clone());
    PANE_DATA_CACHE.with(|global| store_global_pane_cache_entry(global, global_key, entry.clone()));

    entry
}

fn store_global_pane_cache_entry(
    cache: &RefCell<HashMap<(PaneCacheKey, u64), PaneCacheEntry>>,
    key: (PaneCacheKey, u64),
    entry: PaneCacheEntry,
) {
    let mut cache = cache.borrow_mut();
    if cache.len() >= PANE_DATA_CACHE_LIMIT && !cache.contains_key(&key) {
        cache.clear();
    }
    cache.insert(key, entry);
}

fn store_pane_cache_entry(cache: &RefCell<Vec<PaneCacheEntry>>, entry: PaneCacheEntry) {
    let mut c = cache.borrow_mut();
    if let Some(existing) = c.iter_mut().find(|existing| {
        existing.key == entry.key && existing.gutter_style_hash == entry.gutter_style_hash
    }) {
        *existing = entry;
        return;
    }
    if c.len() >= 24 {
        c.remove(0);
    }
    c.push(entry);
}

/// Standalone vertical scrollbar columns for `pane` (aligned with DocumentView / TextArea layout).
pub(crate) fn split_pane_standalone_scrollbar_cols(view: &DiffView, pane: DiffPane) -> u16 {
    use crate::widgets::document_view::layout::standalone_scrollbar_cols;

    let enabled = if matches!(view.mode, DiffViewMode::Split) && view.single_scrollbar {
        matches!(pane, DiffPane::Right) && view.effective_scrollbar()
    } else {
        view.effective_scrollbar()
    };

    match view.backend {
        DiffViewBackend::DocumentView => {
            let d = &view.document_view;
            standalone_scrollbar_cols(
                enabled,
                d.scrollbar_config.variant,
                d.scrollbar_config.gap,
                d.border,
            )
        }
        DiffViewBackend::TextArea => {
            let t = &view.text_area;
            let scrollbar_over_border = matches!(t.height, Length::Auto);
            if enabled && !scrollbar_over_border {
                1u16.saturating_add(t.scrollbar_config.gap)
            } else {
                0
            }
        }
    }
}
