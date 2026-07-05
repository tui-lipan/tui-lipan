//! Diff rendering and data structures.

use super::types::*;
use similar::{ChangeTag, TextDiff};
use std::cmp::max;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use rustc_hash::FxHasher;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DiffLineKind {
    Context,
    /// Unified-diff metadata (e.g. `diff --git …`); not file content - kept when context collapses.
    PatchHeader,
    Added,
    Removed,
    Empty,
    Separator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WordRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DiffRenderLine {
    pub prefix: Arc<str>,
    pub text: Arc<str>,
    pub kind: DiffLineKind,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub word_ranges: Vec<WordRange>,
    pub context_separator: Option<DiffContextSeparator>,
    pub hunk: Option<DiffHunk>,
}

#[derive(Clone)]
pub(crate) struct DiffRender {
    pub raw_text: Arc<str>,
    pub lines: Arc<Vec<DiffRenderLine>>,
    pub diff_hash: u64,
}

impl DiffRender {
    pub fn new(mut lines: Vec<DiffRenderLine>) -> Self {
        if lines.is_empty() {
            lines.push(DiffRenderLine {
                prefix: "".into(),
                text: "".into(),
                kind: DiffLineKind::Context,
                old_line: None,
                new_line: None,
                word_ranges: Vec::new(),
                context_separator: None,
                hunk: None,
            });
        }

        let mut raw = String::new();
        for (idx, line) in lines.iter().enumerate() {
            if idx > 0 {
                raw.push('\n');
            }
            raw.push_str(line.text.as_ref());
        }

        let diff_hash = hash_lines(&lines);
        Self {
            raw_text: raw.into(),
            lines: Arc::new(lines),
            diff_hash,
        }
    }
}

pub(crate) fn hunk_anchors_for_render(render: &DiffRender, pane: DiffPane) -> Vec<DiffHunkAnchor> {
    let mut anchors = Vec::new();
    let mut seen = Vec::new();
    for (logical_line, line) in render.lines.iter().enumerate() {
        let Some(hunk) = line.hunk else {
            continue;
        };
        if seen.contains(&hunk.index) {
            continue;
        }
        seen.push(hunk.index);
        anchors.push(hunk.anchor(pane, logical_line));
    }
    anchors
}

pub(crate) fn hunk_logical_line(render: &DiffRender, hunk_index: usize) -> Option<usize> {
    hunk_anchors_for_render(render, DiffPane::Unified)
        .into_iter()
        .find(|anchor| anchor.index == hunk_index)
        .map(|anchor| anchor.logical_line)
}

fn is_indent_eligible(kind: DiffLineKind, text: &str) -> bool {
    !matches!(
        kind,
        DiffLineKind::Empty | DiffLineKind::Separator | DiffLineKind::PatchHeader
    ) && !text.is_empty()
        && !text.chars().all(|ch| matches!(ch, ' ' | '\t'))
}

fn leading_indent_bytes(text: &str) -> usize {
    text.char_indices()
        .find_map(|(idx, ch)| (!matches!(ch, ' ' | '\t')).then_some(idx))
        .unwrap_or(text.len())
}

fn trim_leading_indent(text: &str, trim: usize) -> (&str, usize) {
    if trim == 0 {
        return (text, 0);
    }

    let mut removed = 0usize;
    for (idx, ch) in text.char_indices() {
        if removed >= trim || !matches!(ch, ' ' | '\t') {
            return (&text[idx..], removed);
        }
        removed += ch.len_utf8();
    }

    ("", removed)
}

pub(crate) fn common_indent_across_lines<'a>(
    lines: impl IntoIterator<Item = &'a DiffRenderLine>,
) -> usize {
    lines
        .into_iter()
        .filter(|line| is_indent_eligible(line.kind, line.text.as_ref()))
        .map(|line| leading_indent_bytes(line.text.as_ref()))
        .min()
        .unwrap_or(0)
}

pub(crate) fn trim_render_common_indent(render: &DiffRender, trim: usize) -> DiffRender {
    if trim == 0 {
        return render.clone();
    }

    let lines = render
        .lines
        .iter()
        .map(|line| {
            if !is_indent_eligible(line.kind, line.text.as_ref()) {
                return line.clone();
            }

            let (trimmed_text, removed) = trim_leading_indent(line.text.as_ref(), trim);
            let word_ranges = line
                .word_ranges
                .iter()
                .filter_map(|range| {
                    if range.end <= removed {
                        None
                    } else {
                        Some(WordRange {
                            start: range.start.saturating_sub(removed),
                            end: range.end.saturating_sub(removed),
                        })
                    }
                })
                .collect();

            DiffRenderLine {
                text: Arc::from(trimmed_text),
                word_ranges,
                ..line.clone()
            }
        })
        .collect();

    DiffRender::new(lines)
}

pub(crate) fn build_diff_data(before: &str, after: &str, config: DiffDataConfig) -> DiffData {
    let diff = TextDiff::from_lines(before, after);
    let mut pending_left: Vec<Arc<str>> = Vec::new();
    let mut pending_right: Vec<Arc<str>> = Vec::new();
    let mut left_lines = Vec::new();
    let mut right_lines = Vec::new();
    let mut unified_lines = Vec::new();
    let mut old_ln = 1usize;
    let mut new_ln = 1usize;

    let flush = |pending_left: &mut Vec<Arc<str>>,
                 pending_right: &mut Vec<Arc<str>>,
                 left_lines: &mut Vec<DiffRenderLine>,
                 right_lines: &mut Vec<DiffRenderLine>,
                 unified_lines: &mut Vec<DiffRenderLine>,
                 old_ln: &mut usize,
                 new_ln: &mut usize| {
        if pending_left.is_empty() && pending_right.is_empty() {
            return;
        }

        let count = max(pending_left.len(), pending_right.len());
        let mut unified_removed = Vec::new();
        let mut unified_added = Vec::new();
        for idx in 0..count {
            let left_text = pending_left.get(idx).cloned();
            let right_text = pending_right.get(idx).cloned();

            let (left_words, right_words) =
                if config.word_diff && left_text.is_some() && right_text.is_some() {
                    word_diff_ranges(
                        left_text.as_deref().unwrap_or(""),
                        right_text.as_deref().unwrap_or(""),
                    )
                } else {
                    (Vec::new(), Vec::new())
                };

            if let Some(text) = left_text.clone() {
                let old_line = Some(*old_ln);
                *old_ln = old_ln.saturating_add(1);
                let line = make_line(
                    text,
                    DiffLineKind::Removed,
                    &config.prefixes,
                    config.show_prefixes,
                    old_line,
                    None,
                    left_words,
                );
                left_lines.push(line.clone());
                unified_removed.push(line);
            } else {
                left_lines.push(make_line(
                    "".into(),
                    DiffLineKind::Empty,
                    &config.prefixes,
                    config.show_prefixes,
                    None,
                    None,
                    Vec::new(),
                ));
            }

            if let Some(text) = right_text.clone() {
                let new_line = Some(*new_ln);
                *new_ln = new_ln.saturating_add(1);
                let line = make_line(
                    text,
                    DiffLineKind::Added,
                    &config.prefixes,
                    config.show_prefixes,
                    None,
                    new_line,
                    right_words,
                );
                right_lines.push(line.clone());
                unified_added.push(line);
            } else {
                right_lines.push(make_line(
                    "".into(),
                    DiffLineKind::Empty,
                    &config.prefixes,
                    config.show_prefixes,
                    None,
                    None,
                    Vec::new(),
                ));
            }
        }

        unified_lines.extend(unified_removed);
        unified_lines.extend(unified_added);

        pending_left.clear();
        pending_right.clear();
    };

    for change in diff.iter_all_changes() {
        let text = strip_newline(change.value());
        let text: Arc<str> = text.into();
        match change.tag() {
            ChangeTag::Equal => {
                flush(
                    &mut pending_left,
                    &mut pending_right,
                    &mut left_lines,
                    &mut right_lines,
                    &mut unified_lines,
                    &mut old_ln,
                    &mut new_ln,
                );
                let old_line = Some(old_ln);
                let new_line = Some(new_ln);
                old_ln = old_ln.saturating_add(1);
                new_ln = new_ln.saturating_add(1);
                let line = make_line(
                    text,
                    DiffLineKind::Context,
                    &config.prefixes,
                    config.show_prefixes,
                    old_line,
                    new_line,
                    Vec::new(),
                );
                left_lines.push(line.clone());
                right_lines.push(line.clone());
                unified_lines.push(line);
            }
            ChangeTag::Delete => pending_left.push(text),
            ChangeTag::Insert => pending_right.push(text),
        }
    }

    flush(
        &mut pending_left,
        &mut pending_right,
        &mut left_lines,
        &mut right_lines,
        &mut unified_lines,
        &mut old_ln,
        &mut new_ln,
    );

    let (left_lines, right_lines, unified_lines) = if let Some(ctx) = config.context_lines {
        collapse_context(
            &left_lines,
            &right_lines,
            &unified_lines,
            ctx,
            context_collapse_options(
                config.show_context_separator,
                config.context_separator_text.as_ref(),
                default_context_separator_min_lines(),
                &[],
            ),
        )
    } else {
        (left_lines, right_lines, unified_lines)
    };

    DiffData::new_internal(
        DiffRender::new(left_lines),
        DiffRender::new(right_lines),
        DiffRender::new(unified_lines),
    )
}

/// Parse a raw unified diff (patch) into a single-pane diff display.
pub(crate) fn build_patch_data(patch: &str, config: DiffDataConfig) -> DiffData {
    let mut left_lines = Vec::new();
    let mut right_lines = Vec::new();
    let mut unified_lines = Vec::new();

    let mut old_ln_opt: Option<usize> = None;
    let mut new_ln_opt: Option<usize> = None;
    let mut current_hunk: Option<DiffHunk> = None;
    let mut next_hunk_index = 0usize;
    let mut pending_left: Vec<Arc<str>> = Vec::new();
    let mut pending_right: Vec<Arc<str>> = Vec::new();

    let flush = |pending_left: &mut Vec<Arc<str>>,
                 pending_right: &mut Vec<Arc<str>>,
                 left_lines: &mut Vec<DiffRenderLine>,
                 right_lines: &mut Vec<DiffRenderLine>,
                 unified_lines: &mut Vec<DiffRenderLine>,
                 old_ln_opt: &mut Option<usize>,
                 new_ln_opt: &mut Option<usize>,
                 hunk: Option<DiffHunk>| {
        if pending_left.is_empty() && pending_right.is_empty() {
            return;
        }
        let count = std::cmp::max(pending_left.len(), pending_right.len());
        let mut unified_removed = Vec::new();
        let mut unified_added = Vec::new();
        for idx in 0..count {
            let left_text = pending_left.get(idx).cloned();
            let right_text = pending_right.get(idx).cloned();

            let (left_words, right_words) =
                if config.word_diff && left_text.is_some() && right_text.is_some() {
                    word_diff_ranges(
                        left_text.as_deref().unwrap_or(""),
                        right_text.as_deref().unwrap_or(""),
                    )
                } else {
                    (Vec::new(), Vec::new())
                };

            if let Some(text) = left_text {
                let old_line = *old_ln_opt;
                if let Some(l) = old_ln_opt {
                    *l = l.saturating_add(1);
                }
                let line = make_line(
                    text,
                    DiffLineKind::Removed,
                    &config.prefixes,
                    config.show_prefixes,
                    old_line,
                    None,
                    left_words,
                );
                let line = with_hunk(line, hunk);
                left_lines.push(line.clone());
                unified_removed.push(line);
            } else {
                left_lines.push(with_hunk(
                    make_line(
                        "".into(),
                        DiffLineKind::Empty,
                        &config.prefixes,
                        config.show_prefixes,
                        None,
                        None,
                        Vec::new(),
                    ),
                    hunk,
                ));
            }

            if let Some(text) = right_text {
                let new_line = *new_ln_opt;
                if let Some(l) = new_ln_opt {
                    *l = l.saturating_add(1);
                }
                let line = make_line(
                    text,
                    DiffLineKind::Added,
                    &config.prefixes,
                    config.show_prefixes,
                    None,
                    new_line,
                    right_words,
                );
                let line = with_hunk(line, hunk);
                right_lines.push(line.clone());
                unified_added.push(line);
            } else {
                right_lines.push(with_hunk(
                    make_line(
                        "".into(),
                        DiffLineKind::Empty,
                        &config.prefixes,
                        config.show_prefixes,
                        None,
                        None,
                        Vec::new(),
                    ),
                    hunk,
                ));
            }
        }
        unified_lines.extend(unified_removed);
        unified_lines.extend(unified_added);
        pending_left.clear();
        pending_right.clear();
    };

    for line in patch.lines() {
        if line.starts_with("@@ ") {
            flush(
                &mut pending_left,
                &mut pending_right,
                &mut left_lines,
                &mut right_lines,
                &mut unified_lines,
                &mut old_ln_opt,
                &mut new_ln_opt,
                current_hunk,
            );

            let parts: Vec<&str> = line.split_whitespace().collect();
            let mut old_start = None;
            let mut new_start = None;
            if parts.len() >= 3 && parts[1].starts_with('-') && parts[2].starts_with('+') {
                old_start = parts[1][1..]
                    .split(',')
                    .next()
                    .unwrap_or("0")
                    .parse::<usize>()
                    .ok();
                new_start = parts[2][1..]
                    .split(',')
                    .next()
                    .unwrap_or("0")
                    .parse::<usize>()
                    .ok();
                old_ln_opt = old_start;
                new_ln_opt = new_start;
            }
            current_hunk = Some(DiffHunk {
                index: next_hunk_index,
                old_start,
                new_start,
            });
            next_hunk_index = next_hunk_index.saturating_add(1);
        } else if line.starts_with("---")
            || line.starts_with("+++")
            || line.starts_with("Index:")
            || line.starts_with("===")
        {
            flush(
                &mut pending_left,
                &mut pending_right,
                &mut left_lines,
                &mut right_lines,
                &mut unified_lines,
                &mut old_ln_opt,
                &mut new_ln_opt,
                current_hunk,
            );

            old_ln_opt = None;
            new_ln_opt = None;
            current_hunk = None;
        } else if let Some(rest) = line.strip_prefix('+') {
            pending_right.push(rest.into());
        } else if let Some(rest) = line.strip_prefix('-') {
            pending_left.push(rest.into());
        } else if let Some(rest) = line.strip_prefix(' ') {
            flush(
                &mut pending_left,
                &mut pending_right,
                &mut left_lines,
                &mut right_lines,
                &mut unified_lines,
                &mut old_ln_opt,
                &mut new_ln_opt,
                current_hunk,
            );

            let old_line = old_ln_opt;
            let new_line = new_ln_opt;
            if let Some(l) = &mut old_ln_opt {
                *l = l.saturating_add(1);
            }
            if let Some(l) = &mut new_ln_opt {
                *l = l.saturating_add(1);
            }

            let ctx_line = with_hunk(
                make_line(
                    rest.into(),
                    DiffLineKind::Context,
                    &config.prefixes,
                    config.show_prefixes,
                    old_line,
                    new_line,
                    Vec::new(),
                ),
                current_hunk,
            );
            left_lines.push(ctx_line.clone());
            right_lines.push(ctx_line.clone());
            unified_lines.push(ctx_line);
        } else if line.is_empty() {
            flush(
                &mut pending_left,
                &mut pending_right,
                &mut left_lines,
                &mut right_lines,
                &mut unified_lines,
                &mut old_ln_opt,
                &mut new_ln_opt,
                current_hunk,
            );

            let old_line = old_ln_opt;
            let new_line = new_ln_opt;
            if let Some(l) = &mut old_ln_opt {
                *l = l.saturating_add(1);
            }
            if let Some(l) = &mut new_ln_opt {
                *l = l.saturating_add(1);
            }

            let ctx_line = with_hunk(
                make_line(
                    "".into(),
                    DiffLineKind::Context,
                    &config.prefixes,
                    config.show_prefixes,
                    old_line,
                    new_line,
                    Vec::new(),
                ),
                current_hunk,
            );
            left_lines.push(ctx_line.clone());
            right_lines.push(ctx_line.clone());
            unified_lines.push(ctx_line);
        } else {
            flush(
                &mut pending_left,
                &mut pending_right,
                &mut left_lines,
                &mut right_lines,
                &mut unified_lines,
                &mut old_ln_opt,
                &mut new_ln_opt,
                current_hunk,
            );

            let fb_kind = if line.starts_with("diff --git") {
                current_hunk = None;
                DiffLineKind::PatchHeader
            } else {
                DiffLineKind::Context
            };
            let fb_line = make_line(
                line.into(),
                fb_kind,
                &config.prefixes,
                false,
                None,
                None,
                Vec::new(),
            );
            left_lines.push(fb_line.clone());
            right_lines.push(fb_line.clone());
            unified_lines.push(fb_line);
        }
    }

    flush(
        &mut pending_left,
        &mut pending_right,
        &mut left_lines,
        &mut right_lines,
        &mut unified_lines,
        &mut old_ln_opt,
        &mut new_ln_opt,
        current_hunk,
    );

    let (left_lines, right_lines, unified_lines) = if let Some(ctx) = config.context_lines {
        collapse_context(
            &left_lines,
            &right_lines,
            &unified_lines,
            ctx,
            context_collapse_options(
                config.show_context_separator,
                config.context_separator_text.as_ref(),
                default_context_separator_min_lines(),
                &[],
            ),
        )
    } else {
        (left_lines, right_lines, unified_lines)
    };

    DiffData::new_internal(
        DiffRender::new(left_lines),
        DiffRender::new(right_lines),
        DiffRender::new(unified_lines),
    )
}

fn make_line(
    text: Arc<str>,
    kind: DiffLineKind,
    prefixes: &DiffPrefixes,
    show_prefixes: bool,
    old_line: Option<usize>,
    new_line: Option<usize>,
    word_ranges: Vec<WordRange>,
) -> DiffRenderLine {
    let prefix = if show_prefixes {
        match kind {
            DiffLineKind::Context | DiffLineKind::PatchHeader | DiffLineKind::Empty => {
                prefixes.context.clone()
            }
            DiffLineKind::Added => prefixes.added.clone(),
            DiffLineKind::Removed => prefixes.removed.clone(),
            DiffLineKind::Separator => "".into(),
        }
    } else {
        "".into()
    };

    DiffRenderLine {
        prefix,
        text,
        kind,
        old_line,
        new_line,
        word_ranges,
        context_separator: None,
        hunk: None,
    }
}

fn with_hunk(mut line: DiffRenderLine, hunk: Option<DiffHunk>) -> DiffRenderLine {
    line.hunk = hunk;
    line
}

fn strip_newline(value: &str) -> &str {
    value.strip_suffix('\n').unwrap_or(value)
}

pub(crate) fn word_diff_ranges(left: &str, right: &str) -> (Vec<WordRange>, Vec<WordRange>) {
    if left.is_empty() && right.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let diff = TextDiff::from_words(left, right);
    let mut left_ranges = Vec::new();
    let mut right_ranges = Vec::new();
    let mut left_cursor = 0usize;
    let mut right_cursor = 0usize;

    for change in diff.iter_all_changes() {
        let value = change.value();
        match change.tag() {
            ChangeTag::Equal => {
                left_cursor = left_cursor.saturating_add(value.len());
                right_cursor = right_cursor.saturating_add(value.len());
            }
            ChangeTag::Delete => {
                if should_highlight_word(value) {
                    left_ranges.push(WordRange {
                        start: left_cursor,
                        end: left_cursor.saturating_add(value.len()),
                    });
                }
                left_cursor = left_cursor.saturating_add(value.len());
            }
            ChangeTag::Insert => {
                if should_highlight_word(value) {
                    right_ranges.push(WordRange {
                        start: right_cursor,
                        end: right_cursor.saturating_add(value.len()),
                    });
                }
                right_cursor = right_cursor.saturating_add(value.len());
            }
        }
    }

    (
        finalize_word_diff_ranges(left, left_ranges),
        finalize_word_diff_ranges(right, right_ranges),
    )
}

fn finalize_word_diff_ranges(text: &str, mut ranges: Vec<WordRange>) -> Vec<WordRange> {
    if ranges.is_empty() {
        return ranges;
    }

    ranges.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.end.cmp(&b.end)));

    let mut merged: Vec<WordRange> = Vec::new();
    for r in ranges {
        if let Some(last) = merged.last_mut() {
            if r.start <= last.end {
                last.end = last.end.max(r.end);
                continue;
            }
            let gap = &text[last.end..r.start];
            if !gap.is_empty() && gap.chars().all(|ch| ch.is_whitespace()) {
                last.end = r.end;
                continue;
            }
        }
        merged.push(r);
    }

    if merged.len() == 1 {
        let r = &merged[0];
        if r.start == 0 && r.end == text.len() {
            return Vec::new();
        }
    }

    merged
}

fn should_highlight_word(value: &str) -> bool {
    value.chars().any(|ch| !ch.is_whitespace())
}

fn hash_lines(lines: &[DiffRenderLine]) -> u64 {
    let mut hasher = FxHasher::default();
    for line in lines {
        line.hash(&mut hasher);
    }
    hasher.finish()
}

// ---------------------------------------------------------------------------
// Context-line collapsing
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub(crate) struct ContextCollapseOptions<'a> {
    pub show_context_separator: bool,
    pub context_separator_text: &'a str,
    pub min_separator_lines: usize,
    pub expanded_contexts: &'a [DiffContextExpansion],
}

pub(crate) fn context_collapse_options<'a>(
    show_context_separator: bool,
    context_separator_text: &'a str,
    min_separator_lines: usize,
    expanded_contexts: &'a [DiffContextExpansion],
) -> ContextCollapseOptions<'a> {
    ContextCollapseOptions {
        show_context_separator,
        context_separator_text,
        min_separator_lines,
        expanded_contexts,
    }
}

/// Build a boolean keep-mask: `true` for every index within `context` lines of
/// a position where `is_changed(i)` returns `true`.
fn compute_keep_mask(n: usize, context: usize, is_changed: impl Fn(usize) -> bool) -> Vec<bool> {
    let mut keep = vec![false; n];
    for i in 0..n {
        if is_changed(i) {
            let start = i.saturating_sub(context);
            let end = (i + context + 1).min(n);
            for k in &mut keep[start..end] {
                *k = true;
            }
        }
    }
    keep
}

fn context_range_for_lines(lines: &[DiffRenderLine]) -> DiffContextRange {
    let old_start = lines.iter().find_map(|line| line.old_line);
    let old_end = lines.iter().rev().find_map(|line| line.old_line);
    let new_start = lines.iter().find_map(|line| line.new_line);
    let new_end = lines.iter().rev().find_map(|line| line.new_line);

    DiffContextRange {
        old_start,
        old_end,
        new_start,
        new_end,
    }
}

fn revealed_lines_for_range(
    range: &DiffContextRange,
    expanded_contexts: &[DiffContextExpansion],
) -> usize {
    expanded_contexts
        .iter()
        .find(|expansion| expansion.range == *range)
        .map(|expansion| expansion.lines_revealed)
        .unwrap_or(0)
}

fn push_context_separator(
    result: &mut Vec<DiffRenderLine>,
    skipped_lines: &[DiffRenderLine],
    skipped: usize,
    direction: DiffContextSeparatorDirection,
    context_separator_text: &str,
    range: Option<DiffContextRange>,
) {
    let range = range.unwrap_or_else(|| context_range_for_lines(skipped_lines));
    result.push(DiffRenderLine {
        prefix: "".into(),
        text: format_context_separator_text(context_separator_text, skipped, direction),
        kind: DiffLineKind::Separator,
        old_line: None,
        new_line: None,
        word_ranges: Vec::new(),
        context_separator: Some(DiffContextSeparator {
            range,
            hidden_lines: skipped,
            direction,
        }),
        hunk: None,
    });
}

fn apply_partial_context_reveal(
    result: &mut Vec<DiffRenderLine>,
    skipped_lines: &[DiffRenderLine],
    revealed: usize,
    direction: DiffContextSeparatorDirection,
    show_context_separator: bool,
    context_separator_text: &str,
    min_separator_lines: usize,
) {
    let skipped = skipped_lines.len();
    let range = context_range_for_lines(skipped_lines);
    debug_assert!(revealed > 0 && revealed < skipped);

    match direction {
        DiffContextSeparatorDirection::Above => {
            let remain = skipped - revealed;
            if remain > 0 && remain < min_separator_lines {
                result.extend(skipped_lines.iter().cloned());
                return;
            }
            if remain > 0 && show_context_separator {
                push_context_separator(
                    result,
                    &skipped_lines[..remain],
                    remain,
                    direction,
                    context_separator_text,
                    Some(range),
                );
            }
            result.extend(skipped_lines[remain..].iter().cloned());
        }
        DiffContextSeparatorDirection::Below => {
            result.extend(skipped_lines[..revealed].iter().cloned());
            let remain = skipped - revealed;
            if remain > 0 && remain < min_separator_lines {
                result.extend(skipped_lines[revealed..].iter().cloned());
                return;
            }
            if remain > 0 && show_context_separator {
                push_context_separator(
                    result,
                    &skipped_lines[revealed..],
                    remain,
                    direction,
                    context_separator_text,
                    Some(range),
                );
            }
        }
        DiffContextSeparatorDirection::Between => {
            let top = revealed / 2;
            let bottom = revealed - top;
            let middle_end = skipped.saturating_sub(bottom);
            result.extend(skipped_lines[..top].iter().cloned());
            if top < middle_end {
                let middle = &skipped_lines[top..middle_end];
                let middle_len = middle.len();
                if middle_len > 0 && middle_len < min_separator_lines {
                    result.extend(middle.iter().cloned());
                } else if middle_len > 0 && show_context_separator {
                    push_context_separator(
                        result,
                        middle,
                        middle_len,
                        direction,
                        context_separator_text,
                        Some(range),
                    );
                }
            }
            if bottom > 0 {
                result.extend(skipped_lines[skipped - bottom..].iter().cloned());
            }
        }
    }
}

/// Replace non-kept runs with a single [`DiffLineKind::Separator`] line,
/// or simply drop them when `show_context_separator` is `false`.
fn apply_keep_mask(
    lines: &[DiffRenderLine],
    keep: &[bool],
    options: ContextCollapseOptions<'_>,
) -> Vec<DiffRenderLine> {
    let ContextCollapseOptions {
        show_context_separator,
        context_separator_text,
        min_separator_lines,
        expanded_contexts,
    } = options;
    let n = lines.len();
    let mut result = Vec::new();
    let mut i = 0;
    while i < n {
        if keep[i] {
            result.push(lines[i].clone());
            i += 1;
        } else {
            let skip_start = i;
            while i < n && !keep[i] {
                i += 1;
            }
            let skipped_lines = &lines[skip_start..i];
            let range = context_range_for_lines(skipped_lines);
            let skipped = skipped_lines.len();
            let direction = match (skip_start == 0, i == n) {
                (true, false) => DiffContextSeparatorDirection::Above,
                (false, true) => DiffContextSeparatorDirection::Below,
                _ => DiffContextSeparatorDirection::Between,
            };
            let revealed = revealed_lines_for_range(&range, expanded_contexts);
            if revealed >= skipped {
                result.extend(skipped_lines.iter().cloned());
            } else if revealed > 0 {
                apply_partial_context_reveal(
                    &mut result,
                    skipped_lines,
                    revealed,
                    direction,
                    show_context_separator,
                    context_separator_text,
                    min_separator_lines,
                );
            } else if skipped < min_separator_lines {
                result.extend(skipped_lines.iter().cloned());
            } else if show_context_separator {
                push_context_separator(
                    &mut result,
                    skipped_lines,
                    skipped,
                    direction,
                    context_separator_text,
                    None,
                );
            }
        }
    }
    result
}

/// Collapse context lines in split (left/right) and unified views.
///
/// For split mode the keep-mask is computed from both sides in lockstep (a row
/// is "changed" when either side is non-Context).  For unified the mask is
/// computed independently.
fn collapse_context(
    left: &[DiffRenderLine],
    right: &[DiffRenderLine],
    unified: &[DiffRenderLine],
    context: usize,
    options: ContextCollapseOptions<'_>,
) -> (
    Vec<DiffRenderLine>,
    Vec<DiffRenderLine>,
    Vec<DiffRenderLine>,
) {
    let options = ContextCollapseOptions {
        // `context_lines(0)` means “show only changed rows” (plus separators and
        // patch metadata). The normal short-run reveal rule intentionally keeps
        // tiny hidden context ranges visible, but with zero requested context it
        // would reintroduce unchanged rows and patch preambles.
        min_separator_lines: if context == 0 {
            1
        } else {
            options.min_separator_lines
        },
        ..options
    };

    // Split: row is changed if either side is non-Context and not patch metadata.
    let split_n = left.len();
    let mut split_keep = compute_keep_mask(split_n, context, |i| {
        left.get(i)
            .is_some_and(|l| !matches!(l.kind, DiffLineKind::Context | DiffLineKind::PatchHeader))
            || right.get(i).is_some_and(|r| {
                !matches!(r.kind, DiffLineKind::Context | DiffLineKind::PatchHeader)
            })
    });
    for (i, keep) in split_keep.iter_mut().enumerate().take(split_n) {
        if left
            .get(i)
            .is_some_and(|l| l.kind == DiffLineKind::PatchHeader)
            || right
                .get(i)
                .is_some_and(|r| r.kind == DiffLineKind::PatchHeader)
        {
            *keep = true;
        }
    }
    let new_left = apply_keep_mask(left, &split_keep, options);
    let new_right = apply_keep_mask(right, &split_keep, options);

    // Unified: unchanged context only; patch metadata is always kept but does not
    // expand the context window around neighboring lines.
    let uni_n = unified.len();
    let mut uni_keep = compute_keep_mask(uni_n, context, |i| {
        unified
            .get(i)
            .is_some_and(|l| !matches!(l.kind, DiffLineKind::Context | DiffLineKind::PatchHeader))
    });
    for (i, keep) in uni_keep.iter_mut().enumerate().take(uni_n) {
        if unified
            .get(i)
            .is_some_and(|l| l.kind == DiffLineKind::PatchHeader)
        {
            *keep = true;
        }
    }
    let new_unified = apply_keep_mask(unified, &uni_keep, options);

    (new_left, new_right, new_unified)
}

pub(crate) fn apply_runtime_context_collapse_to_diff_renders(
    left: DiffRender,
    right: DiffRender,
    unified: DiffRender,
    context_lines: Option<usize>,
    options: ContextCollapseOptions<'_>,
) -> (DiffRender, DiffRender, DiffRender) {
    let Some(ctx) = context_lines else {
        return (left, right, unified);
    };
    let (l, r, u) = collapse_context(&left.lines, &right.lines, &unified.lines, ctx, options);
    (DiffRender::new(l), DiffRender::new(r), DiffRender::new(u))
}

fn format_context_separator_text(
    template: &str,
    count: usize,
    direction: DiffContextSeparatorDirection,
) -> Arc<str> {
    let line_word = if count == 1 { "line" } else { "lines" };
    let (direction_word, arrow) = match direction {
        DiffContextSeparatorDirection::Above => ("above", "↑"),
        DiffContextSeparatorDirection::Below => ("below", "↓"),
        DiffContextSeparatorDirection::Between => ("between", "↑↓"),
    };

    template
        .replace("{count}", &count.to_string())
        .replace("{line_word}", line_word)
        .replace("{direction}", direction_word)
        .replace("{arrow}", arrow)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> DiffDataConfig {
        DiffDataConfig::default()
    }

    #[test]
    fn identical_content_produces_context_lines_only() {
        let diff = build_diff_data("alpha\nbeta\n", "alpha\nbeta\n", default_config());

        assert_eq!(diff.left.lines.len(), 2);
        assert_eq!(diff.right.lines.len(), 2);
        assert_eq!(diff.unified.lines.len(), 2);

        for line in diff.unified.lines.iter() {
            assert_eq!(line.kind, DiffLineKind::Context);
            assert!(line.word_ranges.is_empty());
            assert_eq!(
                line.prefix.as_ref(),
                DiffPrefixes::default().context.as_ref()
            );
        }
    }

    #[test]
    fn pure_additions_and_deletions_create_empty_counterparts() {
        let additions = build_diff_data("", "new one\nnew two\n", default_config());
        let deletions = build_diff_data("gone one\ngone two\n", "", default_config());

        assert_eq!(additions.left.lines.len(), additions.right.lines.len());
        assert!(
            additions
                .left
                .lines
                .iter()
                .all(|line| line.kind == DiffLineKind::Empty && line.text.is_empty())
        );
        assert!(
            additions
                .right
                .lines
                .iter()
                .all(|line| line.kind == DiffLineKind::Added && !line.text.is_empty())
        );

        assert_eq!(deletions.left.lines.len(), deletions.right.lines.len());
        assert!(
            deletions
                .left
                .lines
                .iter()
                .all(|line| line.kind == DiffLineKind::Removed && !line.text.is_empty())
        );
        assert!(
            deletions
                .right
                .lines
                .iter()
                .all(|line| line.kind == DiffLineKind::Empty && line.text.is_empty())
        );

        assert!(
            additions
                .unified
                .lines
                .iter()
                .all(|line| line.kind == DiffLineKind::Added)
        );
        assert!(
            deletions
                .unified
                .lines
                .iter()
                .all(|line| line.kind == DiffLineKind::Removed)
        );
    }

    #[test]
    fn mixed_changes_keep_left_right_lengths_aligned() {
        let diff = build_diff_data(
            "keep\nold value\nend\n",
            "keep\nnew value\nend\nextra\n",
            default_config(),
        );

        assert_eq!(diff.left.lines.len(), diff.right.lines.len());
        assert_eq!(diff.left.lines.len(), 4);

        assert_eq!(diff.left.lines[0].kind, DiffLineKind::Context);
        assert_eq!(diff.right.lines[0].kind, DiffLineKind::Context);

        assert_eq!(diff.left.lines[1].kind, DiffLineKind::Removed);
        assert_eq!(diff.right.lines[1].kind, DiffLineKind::Added);

        assert_eq!(diff.left.lines[2].kind, DiffLineKind::Context);
        assert_eq!(diff.right.lines[2].kind, DiffLineKind::Context);

        assert_eq!(diff.left.lines[3].kind, DiffLineKind::Empty);
        assert_eq!(diff.right.lines[3].kind, DiffLineKind::Added);
    }

    #[test]
    fn unified_replacement_blocks_group_removed_before_added_from_text() {
        let diff = build_diff_data("old1\nold2\nold3\n", "new1\nnew2\n", default_config());

        let unified: Vec<_> = diff
            .unified
            .lines
            .iter()
            .map(|line| (line.kind, line.text.as_ref()))
            .collect();
        assert_eq!(
            unified,
            vec![
                (DiffLineKind::Removed, "old1"),
                (DiffLineKind::Removed, "old2"),
                (DiffLineKind::Removed, "old3"),
                (DiffLineKind::Added, "new1"),
                (DiffLineKind::Added, "new2"),
            ]
        );

        assert_eq!(diff.left.lines.len(), diff.right.lines.len());
        assert_eq!(diff.left.lines[0].text.as_ref(), "old1");
        assert_eq!(diff.right.lines[0].text.as_ref(), "new1");
        assert_eq!(diff.left.lines[1].text.as_ref(), "old2");
        assert_eq!(diff.right.lines[1].text.as_ref(), "new2");
        assert_eq!(diff.left.lines[2].text.as_ref(), "old3");
        assert_eq!(diff.right.lines[2].kind, DiffLineKind::Empty);
    }

    #[test]
    fn unified_replacement_blocks_group_removed_before_added_from_patch() {
        let patch = concat!(
            "@@ -1,3 +1,2 @@\n",
            "-old1\n",
            "-old2\n",
            "-old3\n",
            "+new1\n",
            "+new2\n",
        );
        let diff = build_patch_data(patch, default_config());

        let unified: Vec<_> = diff
            .unified
            .lines
            .iter()
            .map(|line| (line.kind, line.text.as_ref()))
            .collect();
        assert_eq!(
            unified,
            vec![
                (DiffLineKind::Removed, "old1"),
                (DiffLineKind::Removed, "old2"),
                (DiffLineKind::Removed, "old3"),
                (DiffLineKind::Added, "new1"),
                (DiffLineKind::Added, "new2"),
            ]
        );

        assert_eq!(diff.left.lines.len(), diff.right.lines.len());
        assert_eq!(diff.left.lines[0].text.as_ref(), "old1");
        assert_eq!(diff.right.lines[0].text.as_ref(), "new1");
        assert_eq!(diff.left.lines[1].text.as_ref(), "old2");
        assert_eq!(diff.right.lines[1].text.as_ref(), "new2");
        assert_eq!(diff.left.lines[2].text.as_ref(), "old3");
        assert_eq!(diff.right.lines[2].kind, DiffLineKind::Empty);
    }

    #[test]
    fn word_diff_ranges_marks_localized_word_changes() {
        let left = "alpha beta gamma";
        let right = "alpha delta gamma";
        let (left_ranges, right_ranges) = word_diff_ranges(left, right);

        assert_eq!(left_ranges.len(), 1);
        assert_eq!(right_ranges.len(), 1);

        let left_changed = &left[left_ranges[0].start..left_ranges[0].end];
        let right_changed = &right[right_ranges[0].start..right_ranges[0].end];

        assert_eq!(left_changed.trim(), "beta");
        assert_eq!(right_changed.trim(), "delta");
    }

    #[test]
    fn word_diff_ranges_bridge_whitespace_between_changed_tokens() {
        let left = "alpha foo bar gamma";
        let right = "alpha baz qux gamma";
        let (left_ranges, right_ranges) = word_diff_ranges(left, right);

        assert_eq!(left_ranges.len(), 1);
        assert_eq!(right_ranges.len(), 1);
        assert_eq!(&left[left_ranges[0].start..left_ranges[0].end], "foo bar");
        assert_eq!(
            &right[right_ranges[0].start..right_ranges[0].end],
            "baz qux"
        );
    }

    #[test]
    fn word_diff_ranges_omit_per_word_layer_when_entire_line_changes() {
        let left = "aaaa";
        let right = "bbbb";
        let (left_ranges, right_ranges) = word_diff_ranges(left, right);

        assert!(left_ranges.is_empty());
        assert!(right_ranges.is_empty());
    }

    #[test]
    fn show_prefixes_toggles_rendered_prefixes_without_changing_raw_text() {
        let before = "one\n";
        let after = "one\ntwo\n";

        let with_prefixes = build_diff_data(
            before,
            after,
            DiffDataConfig {
                show_prefixes: true,
                ..default_config()
            },
        );
        let without_prefixes = build_diff_data(
            before,
            after,
            DiffDataConfig {
                show_prefixes: false,
                ..default_config()
            },
        );

        assert_eq!(
            with_prefixes.unified.raw_text,
            without_prefixes.unified.raw_text
        );
        assert!(
            with_prefixes
                .unified
                .lines
                .iter()
                .any(|line| !line.prefix.is_empty())
        );

        assert!(
            without_prefixes
                .unified
                .lines
                .iter()
                .all(|line| line.prefix.is_empty())
        );
        assert!(
            with_prefixes
                .unified
                .lines
                .iter()
                .all(|line| !line.prefix.is_empty())
        );
    }

    #[test]
    fn common_indent_ignores_empty_separator_and_blank_lines() {
        let lines = [
            DiffRenderLine {
                prefix: "  ".into(),
                text: "        context".into(),
                kind: DiffLineKind::Context,
                old_line: Some(1),
                new_line: Some(1),
                word_ranges: Vec::new(),
                context_separator: None,
                hunk: None,
            },
            DiffRenderLine {
                prefix: "  ".into(),
                text: "".into(),
                kind: DiffLineKind::Empty,
                old_line: None,
                new_line: None,
                word_ranges: Vec::new(),
                context_separator: None,
                hunk: None,
            },
            DiffRenderLine {
                prefix: "  ".into(),
                text: "    ".into(),
                kind: DiffLineKind::Context,
                old_line: Some(2),
                new_line: Some(2),
                word_ranges: Vec::new(),
                context_separator: None,
                hunk: None,
            },
            DiffRenderLine {
                prefix: "".into(),
                text: "separator".into(),
                kind: DiffLineKind::Separator,
                old_line: None,
                new_line: None,
                word_ranges: Vec::new(),
                context_separator: None,
                hunk: None,
            },
            DiffRenderLine {
                prefix: "+ ".into(),
                text: "    added".into(),
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: Some(3),
                word_ranges: Vec::new(),
                context_separator: None,
                hunk: None,
            },
        ];

        assert_eq!(common_indent_across_lines(lines.iter()), 4);
    }

    #[test]
    fn trim_render_common_indent_shifts_text_and_word_ranges() {
        let render = DiffRender::new(vec![DiffRenderLine {
            prefix: "- ".into(),
            text: "    alpha beta".into(),
            kind: DiffLineKind::Removed,
            old_line: Some(1),
            new_line: None,
            word_ranges: vec![WordRange { start: 4, end: 9 }],
            context_separator: None,
            hunk: None,
        }]);

        let trimmed = trim_render_common_indent(&render, 4);

        assert_eq!(trimmed.raw_text.as_ref(), "alpha beta");
        assert_eq!(trimmed.lines[0].text.as_ref(), "alpha beta");
        assert_eq!(
            trimmed.lines[0].word_ranges,
            vec![WordRange { start: 0, end: 5 }]
        );
    }

    // -----------------------------------------------------------------------
    // Context-lines collapse tests
    // -----------------------------------------------------------------------

    fn config_with_context(n: usize) -> DiffDataConfig {
        DiffDataConfig {
            context_lines: Some(n),
            ..DiffDataConfig::default()
        }
    }

    #[test]
    fn context_lines_collapses_distant_unchanged_regions() {
        // 10 context, 1 change, 10 context => with context=2 we keep 2+1+2=5
        let before: String = (1..=21).map(|i| format!("line{i}\n")).collect();
        let mut after_lines: Vec<String> = (1..=21).map(|i| format!("line{i}\n")).collect();
        after_lines[10] = "CHANGED\n".to_string();
        let after: String = after_lines.concat();

        let diff = build_diff_data(&before, &after, config_with_context(2));

        // Unified: should have separator + 2 ctx + removed + added + 2 ctx + separator
        let kinds: Vec<_> = diff.unified.lines.iter().map(|l| l.kind).collect();
        assert!(kinds.contains(&DiffLineKind::Separator));
        assert!(kinds.contains(&DiffLineKind::Removed));
        assert!(kinds.contains(&DiffLineKind::Added));

        // Count separators: should be exactly 2 (top gap + bottom gap)
        let sep_count = kinds
            .iter()
            .filter(|k| **k == DiffLineKind::Separator)
            .count();
        assert_eq!(sep_count, 2);

        // Total lines should be much less than 21
        assert!(diff.unified.lines.len() < 15);
    }

    #[test]
    fn context_lines_preserves_all_when_everything_is_changed() {
        let diff = build_diff_data("a\nb\nc\n", "x\ny\nz\n", config_with_context(2));

        // Everything is changed, nothing to collapse.
        let sep_count = diff
            .unified
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Separator)
            .count();
        assert_eq!(sep_count, 0);
    }

    #[test]
    fn context_lines_split_keeps_alignment() {
        let before: String = (1..=20).map(|i| format!("line{i}\n")).collect();
        let mut after_lines: Vec<String> = (1..=20).map(|i| format!("line{i}\n")).collect();
        after_lines[9] = "CHANGED\n".to_string();
        let after: String = after_lines.concat();

        let diff = build_diff_data(&before, &after, config_with_context(1));

        assert_eq!(diff.left.lines.len(), diff.right.lines.len());
        // Both sides should have matching separators at same positions.
        for (l, r) in diff.left.lines.iter().zip(diff.right.lines.iter()) {
            if l.kind == DiffLineKind::Separator {
                assert_eq!(r.kind, DiffLineKind::Separator);
            }
        }
    }

    #[test]
    fn context_lines_merges_overlapping_regions() {
        // Two changes 3 lines apart, context=2 => the context windows overlap,
        // so no separator between them.
        let before: String = (1..=10).map(|i| format!("line{i}\n")).collect();
        let mut after_lines: Vec<String> = (1..=10).map(|i| format!("line{i}\n")).collect();
        after_lines[2] = "CHANGED_A\n".to_string();
        after_lines[5] = "CHANGED_B\n".to_string();
        let after: String = after_lines.concat();

        let diff = build_diff_data(&before, &after, config_with_context(2));

        // Between line 3 (idx 2) and line 6 (idx 5) there are only 2 context
        // lines - they overlap with context=2, so no separator in between.
        let kinds: Vec<_> = diff.unified.lines.iter().map(|l| l.kind).collect();
        let changed_indices: Vec<_> = kinds
            .iter()
            .enumerate()
            .filter(|(_, k)| **k == DiffLineKind::Removed || **k == DiffLineKind::Added)
            .map(|(i, _)| i)
            .collect();
        if changed_indices.len() >= 2 {
            let first = changed_indices[0];
            let last = *changed_indices.last().unwrap();
            let between_seps = kinds[first..=last]
                .iter()
                .filter(|k| **k == DiffLineKind::Separator)
                .count();
            assert_eq!(
                between_seps, 0,
                "overlapping context should not insert separators between changes"
            );
        }
    }

    #[test]
    fn context_lines_zero_keeps_only_changed_lines() {
        let diff = build_diff_data("a\nb\nc\n", "a\nB\nc\n", config_with_context(0));

        // Unified: only changed lines + separators
        for line in diff.unified.lines.iter() {
            assert_ne!(line.kind, DiffLineKind::Context);
        }
    }

    #[test]
    fn context_lines_separator_text_mentions_count() {
        let before: String = (1..=20).map(|i| format!("line{i}\n")).collect();
        let mut after_lines: Vec<String> = (1..=20).map(|i| format!("line{i}\n")).collect();
        after_lines[9] = "CHANGED\n".to_string();
        let after: String = after_lines.concat();

        let diff = build_diff_data(&before, &after, config_with_context(1));

        for line in diff.unified.lines.iter() {
            if line.kind == DiffLineKind::Separator {
                assert!(
                    line.text.contains("hidden"),
                    "separator text should mention hidden count, got: {}",
                    line.text
                );
            }
        }
    }

    #[test]
    fn context_separator_metadata_identifies_hidden_range() {
        let before: String = (1..=21).map(|i| format!("line{i}\n")).collect();
        let mut after_lines: Vec<String> = (1..=21).map(|i| format!("line{i}\n")).collect();
        after_lines[10] = "CHANGED\n".to_string();
        let after: String = after_lines.concat();

        let diff = build_diff_data(&before, &after, config_with_context(2));
        let separator = diff
            .unified
            .lines
            .iter()
            .find_map(|line| line.context_separator.as_ref())
            .expect("expected separator metadata");

        assert_eq!(separator.hidden_lines, 8);
        assert_eq!(separator.direction, DiffContextSeparatorDirection::Above);
        assert_eq!(separator.range.old_start, Some(1));
        assert_eq!(separator.range.old_end, Some(8));
        assert_eq!(separator.range.new_start, Some(1));
        assert_eq!(separator.range.new_end, Some(8));
    }

    #[test]
    fn expanded_context_range_restores_hidden_lines() {
        let before: String = (1..=21).map(|i| format!("line{i}\n")).collect();
        let mut after_lines: Vec<String> = (1..=21).map(|i| format!("line{i}\n")).collect();
        after_lines[10] = "CHANGED\n".to_string();
        let after: String = after_lines.concat();

        let base = build_diff_data(&before, &after, DiffDataConfig::default());
        let (_, _, collapsed) = apply_runtime_context_collapse_to_diff_renders(
            base.left.clone(),
            base.right.clone(),
            base.unified.clone(),
            Some(2),
            context_collapse_options(
                true,
                default_context_separator_text().as_ref(),
                default_context_separator_min_lines(),
                &[],
            ),
        );
        let expanded_range = collapsed
            .lines
            .iter()
            .find_map(|line| {
                line.context_separator
                    .as_ref()
                    .map(|separator| separator.range)
            })
            .expect("expected collapsed separator range");

        let (_, _, expanded) = apply_runtime_context_collapse_to_diff_renders(
            base.left,
            base.right,
            base.unified,
            Some(2),
            context_collapse_options(
                true,
                default_context_separator_text().as_ref(),
                default_context_separator_min_lines(),
                &[DiffContextExpansion::full(expanded_range)],
            ),
        );

        assert!(
            expanded
                .lines
                .iter()
                .any(|line| line.text.as_ref() == "line1")
        );
        assert!(
            expanded
                .lines
                .iter()
                .any(|line| line.text.as_ref() == "line8")
        );
        assert_eq!(
            expanded
                .lines
                .iter()
                .filter(|line| line.kind == DiffLineKind::Separator)
                .count(),
            1
        );
    }

    #[test]
    fn default_separator_text_uses_directional_arrows() {
        let before: String = (1..=21).map(|i| format!("line{i}\n")).collect();
        let mut after_lines: Vec<String> = (1..=21).map(|i| format!("line{i}\n")).collect();
        after_lines[10] = "CHANGED\n".to_string();
        let after: String = after_lines.concat();

        let diff = build_diff_data(&before, &after, config_with_context(2));
        let separators: Vec<_> = diff
            .unified
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Separator)
            .map(|l| l.text.as_ref())
            .collect();

        assert_eq!(separators.len(), 2);
        assert!(separators[0].contains('↑'));
        assert!(separators[0].contains("above"));
        assert!(separators[1].contains('↓'));
        assert!(separators[1].contains("below"));
    }

    #[test]
    fn custom_separator_text_replaces_placeholders() {
        let before: String = (1..=20).map(|i| format!("line{i}\n")).collect();
        let mut after_lines: Vec<String> = (1..=20).map(|i| format!("line{i}\n")).collect();
        after_lines[9] = "CHANGED\n".to_string();
        let after: String = after_lines.concat();

        let diff = build_diff_data(
            &before,
            &after,
            DiffDataConfig {
                context_lines: Some(1),
                context_separator_text: "{arrow} {count} {line_word} omitted {direction}".into(),
                ..DiffDataConfig::default()
            },
        );

        let separators: Vec<_> = diff
            .unified
            .lines
            .iter()
            .filter(|l| l.kind == DiffLineKind::Separator)
            .map(|l| l.text.as_ref())
            .collect();

        assert!(
            separators
                .iter()
                .any(|text| text.contains("↑ 8 lines omitted above"))
        );
        assert!(
            separators
                .iter()
                .any(|text| text.contains("↓ 9 lines omitted below"))
        );
    }

    #[test]
    fn custom_separator_text_without_placeholders_is_used_verbatim() {
        let before: String = (1..=20).map(|i| format!("line{i}\n")).collect();
        let mut after_lines: Vec<String> = (1..=20).map(|i| format!("line{i}\n")).collect();
        after_lines[9] = "CHANGED\n".to_string();
        let after: String = after_lines.concat();

        let diff = build_diff_data(
            &before,
            &after,
            DiffDataConfig {
                context_lines: Some(1),
                context_separator_text: "collapsed".into(),
                ..DiffDataConfig::default()
            },
        );

        assert!(
            diff.unified
                .lines
                .iter()
                .filter(|l| l.kind == DiffLineKind::Separator)
                .all(|l| l.text.as_ref() == "collapsed")
        );
    }

    #[test]
    fn show_context_separator_false_omits_separator_lines() {
        let before: String = (1..=20).map(|i| format!("line{i}\n")).collect();
        let mut after_lines: Vec<String> = (1..=20).map(|i| format!("line{i}\n")).collect();
        after_lines[9] = "CHANGED\n".to_string();
        let after: String = after_lines.concat();

        let config = DiffDataConfig {
            context_lines: Some(1),
            show_context_separator: false,
            ..DiffDataConfig::default()
        };
        let diff = build_diff_data(&before, &after, config);

        // No separator lines should be present.
        assert!(
            diff.unified
                .lines
                .iter()
                .all(|l| l.kind != DiffLineKind::Separator),
            "expected no separators when show_context_separator=false"
        );

        // Should still have fewer lines than full diff.
        assert!(diff.unified.lines.len() < 20);
        // Split sides remain aligned.
        assert_eq!(diff.left.lines.len(), diff.right.lines.len());
    }

    #[test]
    fn single_hidden_context_line_renders_without_separator() {
        let before = "line1\nline2\nline3\nline4\nline5\n";
        let after = "line1\nline2\nCHANGED\nline4\nline5\n";

        let base = build_diff_data(before, after, DiffDataConfig::default());
        let (_, _, collapsed) = apply_runtime_context_collapse_to_diff_renders(
            base.left,
            base.right,
            base.unified,
            Some(1),
            context_collapse_options(true, default_context_separator_text().as_ref(), 2, &[]),
        );

        assert!(
            collapsed
                .lines
                .iter()
                .all(|line| line.kind != DiffLineKind::Separator),
            "single-line collapsed runs should render as context, not separators"
        );
        assert!(
            collapsed
                .lines
                .iter()
                .any(|line| line.text.as_ref() == "line1"),
            "expected lone hidden context line to remain visible"
        );
        assert!(
            collapsed
                .lines
                .iter()
                .any(|line| line.text.as_ref() == "line5"),
            "expected trailing lone hidden context line to remain visible"
        );
    }

    #[test]
    fn partial_context_expansion_reveals_lines_incrementally() {
        let before: String = (1..=21).map(|i| format!("line{i}\n")).collect();
        let mut after_lines: Vec<String> = (1..=21).map(|i| format!("line{i}\n")).collect();
        after_lines[10] = "CHANGED\n".to_string();
        let after: String = after_lines.concat();

        let base = build_diff_data(&before, &after, DiffDataConfig::default());
        let range = apply_runtime_context_collapse_to_diff_renders(
            base.left.clone(),
            base.right.clone(),
            base.unified.clone(),
            Some(2),
            context_collapse_options(true, default_context_separator_text().as_ref(), 2, &[]),
        )
        .2
        .lines
        .iter()
        .find_map(|line| line.context_separator.as_ref().map(|sep| sep.range))
        .expect("expected collapsed separator");

        let (_, _, partial) = apply_runtime_context_collapse_to_diff_renders(
            base.left,
            base.right,
            base.unified,
            Some(2),
            context_collapse_options(
                true,
                default_context_separator_text().as_ref(),
                2,
                &[DiffContextExpansion {
                    range,
                    lines_revealed: 2,
                }],
            ),
        );

        assert!(
            partial
                .lines
                .iter()
                .any(|line| line.text.as_ref() == "line7"),
            "partial expansion should reveal lines closest to the hunk"
        );
        let above_separator = partial
            .lines
            .iter()
            .find_map(|line| line.context_separator.as_ref())
            .expect("expected remaining collapsed separator");
        assert_eq!(above_separator.hidden_lines, 6);
        assert_eq!(above_separator.range, range);

        let current = DiffContextExpansion {
            range,
            lines_revealed: 2,
        };
        let next = above_separator
            .event(DiffPane::Unified, 2)
            .next_expansion(Some(&current));
        assert_eq!(next.range, range);
        assert_eq!(next.lines_revealed, 4);
    }

    #[test]
    fn partial_context_expansion_without_separators_omits_remaining_hidden_lines() {
        let before: String = (1..=21).map(|i| format!("line{i}\n")).collect();
        let mut after_lines: Vec<String> = (1..=21).map(|i| format!("line{i}\n")).collect();
        after_lines[10] = "CHANGED\n".to_string();
        let after: String = after_lines.concat();

        let base = build_diff_data(&before, &after, DiffDataConfig::default());
        let (_, _, collapsed) = apply_runtime_context_collapse_to_diff_renders(
            base.left.clone(),
            base.right.clone(),
            base.unified.clone(),
            Some(2),
            context_collapse_options(true, default_context_separator_text().as_ref(), 2, &[]),
        );
        let ranges = collapsed
            .lines
            .iter()
            .filter_map(|line| line.context_separator.as_ref())
            .map(|separator| (separator.direction, separator.range))
            .collect::<Vec<_>>();
        let above_range = ranges
            .iter()
            .find_map(|(direction, range)| {
                (*direction == DiffContextSeparatorDirection::Above).then_some(*range)
            })
            .expect("expected above separator");
        let below_range = ranges
            .iter()
            .find_map(|(direction, range)| {
                (*direction == DiffContextSeparatorDirection::Below).then_some(*range)
            })
            .expect("expected below separator");

        let expansions = [
            DiffContextExpansion {
                range: above_range,
                lines_revealed: 2,
            },
            DiffContextExpansion {
                range: below_range,
                lines_revealed: 2,
            },
        ];
        let (_, _, partial) = apply_runtime_context_collapse_to_diff_renders(
            base.left,
            base.right,
            base.unified,
            Some(2),
            context_collapse_options(
                false,
                default_context_separator_text().as_ref(),
                2,
                &expansions,
            ),
        );

        assert!(
            partial
                .lines
                .iter()
                .all(|line| line.kind != DiffLineKind::Separator),
            "show_context_separator=false should suppress residual separators"
        );
        assert!(
            partial
                .lines
                .iter()
                .any(|line| line.text.as_ref() == "line7")
        );
        assert!(
            partial
                .lines
                .iter()
                .any(|line| line.text.as_ref() == "line14")
        );
        assert!(
            partial
                .lines
                .iter()
                .all(|line| line.text.as_ref() != "line1")
        );
        assert!(
            partial
                .lines
                .iter()
                .all(|line| line.text.as_ref() != "line21")
        );
    }

    #[test]
    fn patch_git_header_line_survives_context_collapse() {
        let patch = concat!(
            "diff --git a/x.rs b/x.rs\n",
            "--- a/x.rs\n",
            "+++ b/x.rs\n",
            "@@ -1,2 +1,2 @@\n",
            " a\n",
            "-b\n",
            "+c\n",
        );
        let diff = build_patch_data(
            patch,
            DiffDataConfig {
                context_lines: Some(0),
                ..DiffDataConfig::default()
            },
        );
        assert!(diff.unified.lines.iter().any(|l| {
            l.kind == DiffLineKind::PatchHeader && l.text.as_ref() == "diff --git a/x.rs b/x.rs"
        }));
    }

    #[test]
    fn patch_preamble_not_kept_by_context_window_around_git_header() {
        let patch = concat!(
            "preamble before diff\n",
            "diff --git a/x.rs b/x.rs\n",
            "--- a/x.rs\n",
            "+++ b/x.rs\n",
            "@@ -1,1 +1,1 @@\n",
            "-old\n",
            "+new\n",
        );
        let diff = build_patch_data(
            patch,
            DiffDataConfig {
                context_lines: Some(0),
                ..DiffDataConfig::default()
            },
        );
        assert!(
            !diff
                .unified
                .lines
                .iter()
                .any(|l| l.text.as_ref() == "preamble before diff"),
        );
        assert!(
            diff.unified
                .lines
                .iter()
                .any(|l| l.kind == DiffLineKind::PatchHeader)
        );
    }
}
