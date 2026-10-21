//! Generic terminal hint discovery.
//!
//! Built-in scanners are dependency-free. Regex-backed scanners are available
//! with the optional `hints-regex` feature.

use std::borrow::Cow;
use std::ops::Range;

use super::open_url::{is_allowed_scheme, parse_scheme};
use super::spans::display_column;

/// Home-row keys used when assigning hint labels.
pub const HOME_ROW_HINT_KEYS: &str = "asdfghjkl;";

/// The kind of a discovered hint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HintKind {
    /// A URL whose scheme is allowed by [`crate::utils::open_url()`].
    Url,
    /// A relative, home-relative, or absolute path using Unix or Windows separators.
    Path,
    /// A hexadecimal Git object abbreviation or full SHA.
    GitSha,
    /// A custom scanner identified by its caller-provided id.
    Custom(u16),
}

impl From<u16> for HintKind {
    fn from(value: u16) -> Self {
        Self::Custom(value)
    }
}

impl HintKind {
    /// Return whether uppercase activation may open this hint.
    pub fn can_open(self) -> bool {
        matches!(self, Self::Url)
    }
}

/// The part of a hint lying on one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HintSpan {
    /// Zero-based line number.
    pub row: usize,
    /// Start display column, inclusive.
    pub start_col: usize,
    /// End display column, exclusive.
    pub end_col: usize,
}

/// A hint found in a newline-delimited terminal snapshot.
///
/// A hint occupies more than one span only when the rows it covers were soft-wrapped by the
/// terminal, which [`HintScan::scan_wrapped`] rejoins before scanning; [`HintScan::scan`] treats
/// every row as a line of its own and always produces single-span matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HintMatch {
    /// The rows this hint covers, in order, each with its own display columns. Never empty.
    pub spans: Vec<HintSpan>,
    /// Matched text after trailing display punctuation is removed.
    pub text: String,
    /// The scanner that produced this match.
    pub kind: HintKind,
}

impl HintMatch {
    /// The row the hint starts on.
    pub fn row(&self) -> usize {
        self.spans.first().map_or(0, |span| span.row)
    }

    /// The display column the hint starts at, on [`Self::row`].
    pub fn start_col(&self) -> usize {
        self.spans.first().map_or(0, |span| span.start_col)
    }

    /// The row the hint ends on.
    pub fn end_row(&self) -> usize {
        self.spans.last().map_or(0, |span| span.row)
    }

    /// The display column, exclusive, the hint ends at on [`Self::end_row`].
    pub fn end_col(&self) -> usize {
        self.spans.last().map_or(0, |span| span.end_col)
    }
}

/// A line scanner returning UTF-8 byte ranges for matches.
pub trait HintScanner {
    /// Append half-open UTF-8 byte ranges found in one line of text.
    fn scan_line(&self, line: &str, out: &mut Vec<Range<usize>>);
}

impl<F> HintScanner for F
where
    F: Fn(&str, &mut Vec<Range<usize>>),
{
    fn scan_line(&self, line: &str, out: &mut Vec<Range<usize>>) {
        self(line, out)
    }
}

/// A builder for combining built-in and custom hint scanners.
pub struct HintScan {
    scanners: Vec<(HintKind, Box<dyn HintScanner>)>,
}

impl HintScan {
    /// Start with all three built-in scanners enabled.
    pub fn new() -> Self {
        Self {
            scanners: vec![
                (HintKind::Url, Box::new(append_url_ranges)),
                (HintKind::Path, Box::new(append_path_ranges)),
                (HintKind::GitSha, Box::new(append_sha_ranges)),
            ],
        }
    }

    /// Enable or disable the built-in allowed-scheme URL scanner.
    pub fn urls(mut self, on: bool) -> Self {
        self.set_builtin(HintKind::Url, on, append_url_ranges);
        self
    }

    /// Enable or disable the built-in path scanner.
    pub fn paths(mut self, on: bool) -> Self {
        self.set_builtin(HintKind::Path, on, append_path_ranges);
        self
    }

    /// Enable or disable the built-in Git SHA scanner.
    pub fn git_shas(mut self, on: bool) -> Self {
        self.set_builtin(HintKind::GitSha, on, append_sha_ranges);
        self
    }

    /// Add a custom scanner identified by `tag`.
    pub fn custom<S>(mut self, tag: u16, scanner: S) -> Self
    where
        S: HintScanner + 'static,
    {
        self.scanners
            .push((HintKind::Custom(tag), Box::new(scanner)));
        self
    }

    /// Scan all enabled scanners and return sorted, non-overlapping matches.
    ///
    /// Each row is scanned as a line of its own, so a hint the terminal soft-wrapped is seen as
    /// the unrelated fragments it was broken into. Use [`Self::scan_wrapped`] wherever the wrap
    /// flags are known.
    pub fn scan(&self, text: &str) -> Vec<HintMatch> {
        self.scan_wrapped(text, &[])
    }

    /// Scan `text`, rejoining rows the terminal soft-wrapped before matching.
    ///
    /// `wrapped_rows[row]` says row `row` continues into `row + 1`; missing entries do not wrap.
    /// A hint spanning a wrap is one match covering one span per row, so a URL longer than the
    /// terminal is wide is copied whole rather than found as several broken pieces - or, since
    /// neither piece is a URL on its own, not found at all.
    pub fn scan_wrapped(&self, text: &str, wrapped_rows: &[bool]) -> Vec<HintMatch> {
        let rows: Vec<&str> = text.split('\n').collect();
        let mut output = Vec::new();
        let mut first = 0usize;
        while first < rows.len() {
            let mut last = first;
            while last + 1 < rows.len() && wrapped_rows.get(last).copied().unwrap_or(false) {
                last += 1;
            }
            let group = &rows[first..=last];
            let line = if group.len() == 1 {
                Cow::Borrowed(group[0])
            } else {
                Cow::Owned(group.concat())
            };

            let mut ranges: Vec<(Range<usize>, HintKind)> = Vec::new();
            for (kind, scanner) in &self.scanners {
                let mut found = Vec::new();
                scanner.scan_line(&line, &mut found);
                for range in found {
                    push_range(&mut ranges, &line, range, *kind);
                }
            }
            ranges.sort_by_key(|(range, _)| (range.start, range.end));
            output.extend(
                ranges
                    .into_iter()
                    .filter_map(|(range, kind)| hint_match(group, first, &line, range, kind)),
            );
            first = last + 1;
        }
        output.sort_by_key(|matched| (matched.row(), matched.start_col(), matched.end_col()));
        output
    }

    fn set_builtin<F>(&mut self, kind: HintKind, on: bool, scanner: F)
    where
        F: HintScanner + 'static,
    {
        self.scanners.retain(|(existing, _)| *existing != kind);
        if on {
            self.scanners.push((kind, Box::new(scanner)));
        }
    }
}

#[cfg(feature = "terminal")]
impl HintScan {
    /// Scan a terminal snapshot: its text rejoined across soft wraps, and its OSC 8 hyperlinks.
    ///
    /// A hyperlink's label is often not its destination - `PR #200` linking to a pull request -
    /// so scanning the text never finds it. While the URL scanner is enabled, each link whose
    /// scheme is allowed becomes one [`HintKind::Url`] hint carrying its URI, joined across the
    /// row breaks of a soft wrap. It replaces every text hint it overlaps: the URI the program
    /// supplied is what the visible label stands for.
    pub fn scan_snapshot(
        &self,
        snapshot: &crate::widgets::TerminalRenderSnapshot,
    ) -> Vec<HintMatch> {
        let mut output = self.scan_wrapped(&snapshot.text, &snapshot.wrapped_rows);
        if !self.scanners.iter().any(|(kind, _)| *kind == HintKind::Url) {
            return output;
        }
        let links = hyperlink_hints(&snapshot.hyperlinks, &snapshot.wrapped_rows);
        output.retain(|matched| !links.iter().any(|link| hints_overlap(link, matched)));
        output.extend(links);
        output.sort_by_key(|matched| (matched.row(), matched.start_col(), matched.end_col()));
        output
    }
}

/// One URL hint per hyperlink, joining the row-local spans a link is reported as.
#[cfg(feature = "terminal")]
fn hyperlink_hints(
    links: &[crate::widgets::TerminalHyperlink],
    wrapped_rows: &[bool],
) -> Vec<HintMatch> {
    let mut sorted: Vec<_> = links.iter().collect();
    sorted.sort_by_key(|link| (link.row, link.start_col));
    let mut output: Vec<(&str, HintMatch)> = Vec::new();
    for link in sorted {
        if parse_scheme(&link.uri).is_none_or(|scheme| !is_allowed_scheme(scheme)) {
            continue;
        }
        let span = HintSpan {
            row: link.row,
            start_col: link.start_col,
            end_col: link.end_col,
        };
        if let Some((uri, previous)) = output.last_mut()
            && *uri == &*link.uri
            && continues_link(previous, span, wrapped_rows)
        {
            previous.spans.push(span);
            continue;
        }
        output.push((
            &link.uri,
            HintMatch {
                spans: vec![span],
                text: link.uri.to_string(),
                kind: HintKind::Url,
            },
        ));
    }
    output.into_iter().map(|(_, matched)| matched).collect()
}

/// Whether `next` carries on the link `previous` ends, on its row or past a soft wrap.
#[cfg(feature = "terminal")]
fn continues_link(previous: &HintMatch, next: HintSpan, wrapped_rows: &[bool]) -> bool {
    let last = previous.spans.last().copied().unwrap_or(next);
    if last.row == next.row {
        return last.end_col == next.start_col;
    }
    next.row == last.row + 1
        && next.start_col == 0
        && wrapped_rows.get(last.row).copied().unwrap_or(false)
}

#[cfg(feature = "terminal")]
fn hints_overlap(left: &HintMatch, right: &HintMatch) -> bool {
    left.spans.iter().any(|a| {
        right
            .spans
            .iter()
            .any(|b| a.row == b.row && a.start_col < b.end_col && b.start_col < a.end_col)
    })
}

impl Default for HintScan {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate unique, prefix-free home-row labels for `count` hints.
///
/// Every label has the same width, which is what makes the set prefix-free.
/// Duplicate characters in `keys` are ignored so they cannot collide. At least
/// two distinct keys are required to label more than one hint; with fewer, an
/// empty vector is returned rather than a set of identical labels.
pub fn assign_labels(count: usize, keys: &str) -> Vec<String> {
    let mut seen = Vec::new();
    for key in keys.chars() {
        if !seen.contains(&key) {
            seen.push(key);
        }
    }
    let keys = seen;
    if keys.is_empty() || (keys.len() == 1 && count > 1) {
        return Vec::new();
    }

    let mut width = 1usize;
    let mut capacity = keys.len();
    while capacity < count {
        width += 1;
        capacity = capacity.saturating_mul(keys.len());
    }

    (0..count)
        .map(|mut index| {
            let mut label = vec![keys[0]; width];
            for position in (0..width).rev() {
                label[position] = keys[index % keys.len()];
                index /= keys.len();
            }
            label.into_iter().collect()
        })
        .collect()
}

/// Result of filtering labels by a typed prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintFilter {
    /// No label starts with the input.
    NoMatch,
    /// More than one label starts with the input.
    Ambiguous,
    /// Exactly one label matches; the value is its zero-based index.
    Selected(usize),
}

/// Filter labels by a typed prefix.
pub fn filter_labels(labels: &[String], input: &str) -> HintFilter {
    let mut selected = None;
    for (index, label) in labels.iter().enumerate() {
        if !label.starts_with(input) {
            continue;
        }
        if selected.replace(index).is_some() {
            return HintFilter::Ambiguous;
        }
    }
    selected.map_or(HintFilter::NoMatch, HintFilter::Selected)
}

/// Accept one scanner range on a logical line, trimmed and free of already-accepted overlap.
///
/// Ranges are byte offsets into the logical line, which is what makes the earlier scanner win an
/// overlap: built-ins are registered before custom ones and are already in `output` by the time a
/// custom range is offered.
fn push_range(
    output: &mut Vec<(Range<usize>, HintKind)>,
    line: &str,
    range: Range<usize>,
    kind: HintKind,
) {
    let start = range.start.min(line.len());
    let end = range.end.min(line.len());
    if start >= end || !line.is_char_boundary(start) || !line.is_char_boundary(end) {
        return;
    }
    let trimmed = trim_display_punctuation(&line[start..end]);
    if trimmed.is_empty() {
        return;
    }
    let end = start + trimmed.len();
    if output
        .iter()
        .any(|(existing, _)| start < existing.end && end > existing.start)
    {
        return;
    }
    output.push((start..end, kind));
}

/// Drop the sentence punctuation and closing brackets that end the prose around a hint.
fn trim_display_punctuation(text: &str) -> &str {
    text.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}'])
}

/// Cut a logical-line byte range into one span per row it covers.
///
/// `group` holds the rows the line was joined from, in order, and `first` is the row number of
/// `group[0]`. Rows the range misses contribute nothing, so a match never carries an empty span.
fn hint_match(
    group: &[&str],
    first: usize,
    line: &str,
    range: Range<usize>,
    kind: HintKind,
) -> Option<HintMatch> {
    let mut spans = Vec::new();
    let mut base = 0usize;
    for (offset, row) in group.iter().enumerate() {
        let row_end = base + row.len();
        let start = range.start.max(base);
        let end = range.end.min(row_end);
        if start < end {
            let start_col = display_column(row, start - base);
            let end_col = display_column(row, end - base);
            if start_col < end_col {
                spans.push(HintSpan {
                    row: first + offset,
                    start_col,
                    end_col,
                });
            }
        }
        base = row_end;
    }
    if spans.is_empty() {
        return None;
    }
    Some(HintMatch {
        spans,
        text: line[range].to_string(),
        kind,
    })
}

/// Return whether `byte` may appear inside a URL scheme name.
///
/// Schemes are ASCII-only per RFC 3986, so a byte comparison is also a UTF-8
/// character boundary check: no continuation byte can match.
fn is_scheme_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')
}

fn url_ranges(line: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    // Anchor on the scheme separator and walk left to the scheme start. Anchoring
    // on the first alphabetic character instead would let ordinary prose ahead of
    // the URL consume the separator, dropping the match entirely.
    while let Some(relative) = line[cursor..].find(':') {
        let colon = cursor + relative;
        let bytes = line.as_bytes();
        let mut start = colon;
        while start > 0 && is_scheme_byte(bytes[start - 1]) {
            start -= 1;
        }
        // A scheme must begin with a letter and sit at a word boundary, so a bare
        // `foo.bar:8080` host:port pair is not mistaken for one.
        if start == colon
            || !bytes[start].is_ascii_alphabetic()
            || (start > 0 && bytes[start - 1] == b'_')
            || parse_scheme(&line[start..]).is_none_or(|parsed| !is_allowed_scheme(parsed))
        {
            cursor = colon + 1;
            continue;
        }
        let end = url_end(line, start, colon);
        if end > colon + 1 && !is_elided(&line[start..end]) {
            ranges.push(start..end);
        }
        cursor = end.max(colon + 1);
    }
    ranges
}

/// Where a URL whose scheme separator sits at `colon` stops.
///
/// Whitespace and angle brackets end it, and so do `"` and `` ` ``, which a URL never carries
/// unescaped: without that, `"url":"https://example.com"` copies the closing quote. An apostrophe
/// is legal inside a URL, so it ends one only when one opened it.
fn url_end(line: &str, start: usize, colon: usize) -> usize {
    let opened_by_apostrophe = line[..start].ends_with('\'');
    line[colon + 1..]
        .char_indices()
        .find(|(_, ch)| {
            ch.is_whitespace()
                || matches!(ch, '<' | '>' | '"' | '`')
                || (opened_by_apostrophe && *ch == '\'')
        })
        .map_or(line.len(), |(offset, _)| colon + 1 + offset)
}

/// Whether a candidate carries the ellipsis a program prints where it cut text short.
///
/// What is left is a fragment of the real URL or path, and copying it gives neither.
fn is_elided(candidate: &str) -> bool {
    candidate.contains('\u{2026}')
}

fn append_url_ranges(line: &str, out: &mut Vec<Range<usize>>) {
    out.extend(url_ranges(line));
}

fn is_path_separator(byte: u8) -> bool {
    matches!(byte, b'/' | b'\\')
}

fn is_path_boundary(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '"' | '\'')
}

fn path_token_start(line: &str, separator: usize) -> usize {
    line[..separator]
        .char_indices()
        .rev()
        .find(|(_, ch)| is_path_boundary(*ch))
        .map_or(0, |(index, ch)| index + ch.len_utf8())
}

fn has_drive_prefix(bytes: &[u8], start: usize) -> bool {
    bytes.get(start).is_some_and(u8::is_ascii_alphabetic) && bytes.get(start + 1) == Some(&b':')
}

/// How a path candidate begins, which decides how much evidence it needs to count as a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathForm {
    /// `./`, `../`, `~/`, or a drive letter: unmistakably a path.
    Anchored,
    /// A leading separator, as in `/etc/hosts`.
    Rooted,
    /// A bare relative path, as in `src/main.rs`.
    Bare,
}

fn path_start(line: &str, separator: usize) -> Option<(usize, PathForm)> {
    let bytes = line.as_bytes();
    if separator > 0 && bytes[separator - 1] == bytes[separator] {
        return None;
    }
    if separator >= 2 && bytes[separator - 2..separator] == *b".." {
        return Some((separator - 2, PathForm::Anchored));
    }
    if separator >= 1 && matches!(bytes[separator - 1], b'.' | b'~') {
        return Some((separator - 1, PathForm::Anchored));
    }
    let token_start = path_token_start(line, separator);
    if has_drive_prefix(bytes, token_start) {
        return Some((token_start, PathForm::Anchored));
    }
    if token_start == separator {
        return Some((separator, PathForm::Rooted));
    }
    // Only the first separator of a bare token starts a candidate; a later one would judge the
    // same token again from its middle.
    line[token_start..separator]
        .bytes()
        .all(is_bare_path_head_byte)
        .then_some((token_start, PathForm::Bare))
}

fn is_bare_path_head_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+' | b'@')
}

/// Whether a candidate has enough shape to be a path rather than prose.
///
/// A bare `word/word` is far more often `and/or` or a branch name than a file, and a lone `/word`
/// is as often a slash command as a directory, so both need a file extension; a rooted path may
/// show a second separator instead. `name` is the candidate without its `:line` suffix or
/// trailing punctuation.
fn has_path_shape(name: &str, form: PathForm) -> bool {
    match form {
        PathForm::Anchored => true,
        PathForm::Rooted => name[1..].bytes().any(is_path_separator) || has_file_extension(name),
        PathForm::Bare => has_file_extension(name),
    }
}

fn has_file_extension(name: &str) -> bool {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    file.rsplit_once('.').is_some_and(|(_, extension)| {
        !extension.is_empty()
            && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
            && extension.bytes().any(|byte| byte.is_ascii_alphabetic())
    })
}

fn path_quote(line: &str, start: usize) -> Option<char> {
    line[..start]
        .chars()
        .next_back()
        .filter(|ch| matches!(ch, '"' | '\''))
}

fn is_drive_colon(bytes: &[u8], index: usize) -> bool {
    index > 0
        && bytes[index - 1].is_ascii_alphabetic()
        && bytes.get(index + 1).copied().is_some_and(is_path_separator)
}

fn unquoted_path_end(line: &str, separator: usize) -> usize {
    line[separator..]
        .char_indices()
        .find(|(offset, ch)| {
            ch.is_whitespace()
                || (*ch == ':' && !is_drive_colon(line.as_bytes(), separator + offset))
        })
        .map_or(line.len(), |(offset, _)| separator + offset)
}

fn path_end(line: &str, separator: usize, quote: Option<char>) -> usize {
    quote
        .and_then(|quote| line[separator..].find(quote))
        .map_or_else(
            || unquoted_path_end(line, separator),
            |offset| separator + offset,
        )
}

fn extend_line_number(line: &str, end: usize) -> usize {
    if line.as_bytes().get(end) != Some(&b':') {
        return end;
    }
    let number_start = end + 1;
    let number_end = line[number_start..]
        .char_indices()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map_or(line.len(), |(offset, _)| number_start + offset);
    if number_end > number_start {
        number_end
    } else {
        end
    }
}

fn has_path_body(bytes: &[u8], start: usize, separator: usize, end: usize) -> bool {
    end > separator + 1
        || (has_drive_prefix(bytes, start) && separator == start + 2 && end == separator + 1)
}

fn path_ranges(line: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = line[cursor..].bytes().position(is_path_separator) {
        let separator = cursor + relative;
        let Some((start, form)) = path_start(line, separator) else {
            cursor = separator + 1;
            continue;
        };
        if start > 0
            && !line[..start]
                .chars()
                .next_back()
                .is_some_and(is_path_boundary)
        {
            cursor = separator + 1;
            continue;
        }
        let quote = path_quote(line, start);
        let name_end = path_end(line, separator, quote);
        let end = if quote.is_none() {
            extend_line_number(line, name_end)
        } else {
            name_end
        };
        if has_path_body(line.as_bytes(), start, separator, end)
            && has_path_shape(trim_display_punctuation(&line[start..name_end]), form)
            && !is_elided(&line[start..end])
        {
            ranges.push(start..end);
        }
        cursor = end.max(separator + 1);
    }
    ranges
}

fn append_path_ranges(line: &str, out: &mut Vec<Range<usize>>) {
    out.extend(path_ranges(line));
}

fn sha_ranges(line: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    while cursor < line.len() {
        let Some(relative) = line[cursor..]
            .char_indices()
            .find_map(|(offset, ch)| ch.is_ascii_hexdigit().then_some(offset))
        else {
            break;
        };
        let start = cursor + relative;
        if start > 0 && is_word_byte(line.as_bytes()[start - 1]) {
            cursor = start + 1;
            continue;
        }
        let end = line[start..]
            .char_indices()
            .find(|(_, ch)| !ch.is_ascii_hexdigit())
            .map(|(offset, _)| start + offset)
            .unwrap_or(line.len());
        if (7..=40).contains(&(end - start))
            && (end == line.len() || !is_word_byte(line.as_bytes()[end]))
            && is_mixed_hex(&line[start..end])
        {
            ranges.push(start..end);
        }
        cursor = end.max(start + 1);
    }
    ranges
}

/// Whether a hex run holds both a digit and a letter.
///
/// An abbreviated object name almost always does, while a decimal number has no letter and an
/// English word spelled from `a`-`f`, such as `defaced`, has no digit.
fn is_mixed_hex(run: &str) -> bool {
    run.bytes().any(|byte| byte.is_ascii_digit())
        && run.bytes().any(|byte| byte.is_ascii_alphabetic())
}

fn append_sha_ranges(line: &str, out: &mut Vec<Range<usize>>) {
    out.extend(sha_ranges(line));
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Regex-lite-backed line scanner for string-configured custom patterns.
#[cfg(feature = "hints-regex")]
#[derive(Clone, Debug)]
pub struct RegexHintScanner {
    regex: regex_lite::Regex,
}

#[cfg(feature = "hints-regex")]
impl RegexHintScanner {
    /// Compile a string-configured regex scanner.
    pub fn new(pattern: &str) -> Result<Self, regex_lite::Error> {
        Ok(Self {
            regex: regex_lite::Regex::new(pattern)?,
        })
    }
}

#[cfg(feature = "hints-regex")]
impl HintScanner for RegexHintScanner {
    fn scan_line(&self, line: &str, out: &mut Vec<Range<usize>>) {
        out.extend(
            self.regex
                .find_iter(line)
                .map(|matched| matched.start()..matched.end()),
        );
    }
}

#[cfg(feature = "hints-regex")]
impl HintScanner for regex_lite::Regex {
    fn scan_line(&self, line: &str, out: &mut Vec<Range<usize>>) {
        out.extend(
            self.find_iter(line)
                .map(|matched| matched.start()..matched.end()),
        );
    }
}

#[cfg(feature = "hints-regex")]
impl HintScan {
    /// Add a regex scanner from a string-configured pattern.
    pub fn custom_regex(self, tag: u16, pattern: &str) -> Result<Self, regex_lite::Error> {
        Ok(self.custom(tag, RegexHintScanner::new(pattern)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_scan_allowed_urls_paths_and_shas_in_display_columns() {
        let found = HintScan::new()
            .scan("你 mailto:user@example.test https://example.com/a). ./src/main.rs:12 9983d705");
        assert_eq!(
            found
                .iter()
                .map(|matched| matched.text.as_str())
                .collect::<Vec<_>>(),
            vec![
                "mailto:user@example.test",
                "https://example.com/a",
                "./src/main.rs:12",
                "9983d705"
            ]
        );
        assert_eq!(found[0].kind, HintKind::Url);
        assert_eq!(found[0].start_col(), 3);
    }

    #[test]
    fn builder_and_closure_scanner_assign_custom_ids() {
        let found = HintScan::new()
            .custom(7u16, |line: &str, out: &mut Vec<Range<usize>>| {
                out.push(line.find("issue").unwrap()..line.len());
            })
            .scan("你 issue-123");
        assert_eq!(found[0].kind, HintKind::Custom(7));
        assert_eq!(found[0].start_col(), 3);
    }

    #[test]
    fn wrapped_rows_are_rejoined_into_one_hint_with_a_span_per_row() {
        let text = "see https://example.com/a\nvery/long/path?q=1 rest\nnope";
        let wrapped = [true, false, false];
        let found = HintScan::new().urls(true).paths(false).git_shas(false);

        // Neither row carries a whole URL, so scanning them apart finds one truncated match.
        let split = found.scan(text);
        assert_eq!(split.len(), 1);
        assert_eq!(split[0].text, "https://example.com/a");

        let joined = found.scan_wrapped(text, &wrapped);
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].text, "https://example.com/avery/long/path?q=1");
        assert_eq!(
            joined[0].spans,
            vec![
                HintSpan {
                    row: 0,
                    start_col: 4,
                    end_col: 25
                },
                HintSpan {
                    row: 1,
                    start_col: 0,
                    end_col: 18
                },
            ]
        );
        assert_eq!(joined[0].row(), 0);
        assert_eq!(joined[0].start_col(), 4);
        assert_eq!(joined[0].end_row(), 1);
        assert_eq!(joined[0].end_col(), 18);
    }

    #[test]
    fn labels_filter_to_the_three_planned_states() {
        let labels = assign_labels(11, HOME_ROW_HINT_KEYS);
        assert_eq!(filter_labels(&labels, "z"), HintFilter::NoMatch);
        assert_eq!(filter_labels(&labels, "aa"), HintFilter::Selected(0));
        assert_eq!(filter_labels(&labels, "a"), HintFilter::Ambiguous);

        let many = assign_labels(101, HOME_ROW_HINT_KEYS);
        let unique = many.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), many.len());
        assert!(many.iter().all(|label| label.chars().count() == 3));
    }

    #[test]
    fn urls_are_found_after_ascii_prose_on_the_same_line() {
        // Anchoring on the first alphabetic character used to let the leading words
        // consume the scheme separator, dropping every URL on the line.
        let found = HintScan::new().scan("error at https://a.test and https://b.test");
        assert_eq!(
            found
                .iter()
                .map(|matched| matched.text.as_str())
                .collect::<Vec<_>>(),
            vec!["https://a.test", "https://b.test"]
        );
        assert_eq!(found[0].start_col(), 9);
    }

    #[test]
    fn scheme_detection_requires_a_word_boundary_and_allowed_scheme() {
        assert!(
            HintScan::new()
                .urls(true)
                .paths(false)
                .git_shas(false)
                .scan("localhost:8080")
                .is_empty()
        );
        assert!(
            HintScan::new()
                .urls(true)
                .paths(false)
                .git_shas(false)
                .scan("javascript:alert(1)")
                .is_empty()
        );
        assert!(
            HintScan::new()
                .urls(true)
                .paths(false)
                .git_shas(false)
                .scan("x_https://a.test")
                .is_empty()
        );
    }

    #[test]
    fn labels_reject_degenerate_alphabets_and_ignore_duplicate_keys() {
        // A single distinct key cannot produce a prefix-free set for more than one
        // hint; returning identical labels would make every one of them ambiguous.
        assert!(assign_labels(5, "a").is_empty());
        assert_eq!(assign_labels(1, "a"), vec!["a".to_string()]);
        assert_eq!(assign_labels(2, "aab"), assign_labels(2, "ab"));

        let labels = assign_labels(4, "ab");
        assert_eq!(
            labels
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            labels.len()
        );
    }

    #[test]
    fn path_scanning_never_slices_inside_multibyte_prefixes() {
        let found = HintScan::new().scan("你/relative ./safe/path");
        assert!(found.iter().any(|hint| hint.text == "./safe/path"));
    }

    #[test]
    fn paths_accept_windows_forms() {
        let found = HintScan::new().scan(
            r#"C:\Users\me\src\main.rs:12 D:src\lib.rs E:/src/main.rs C:\ ..\tests\hints.rs \\server\share\logs\run.log \\?\C:\ProgramData\Rozi\config.toml "C:\Program Files\Rozi\rozi.exe""#,
        );
        assert_eq!(
            found
                .iter()
                .map(|matched| (matched.text.as_str(), matched.kind))
                .collect::<Vec<_>>(),
            vec![
                (r"C:\Users\me\src\main.rs:12", HintKind::Path),
                (r"D:src\lib.rs", HintKind::Path),
                ("E:/src/main.rs", HintKind::Path),
                (r"C:\", HintKind::Path),
                (r"..\tests\hints.rs", HintKind::Path),
                (r"\\server\share\logs\run.log", HintKind::Path),
                (r"\\?\C:\ProgramData\Rozi\config.toml", HintKind::Path),
                (r"C:\Program Files\Rozi\rozi.exe", HintKind::Path),
            ]
        );
    }

    #[test]
    fn windows_path_scanning_does_not_promote_embedded_separators() {
        assert!(
            HintScan::new()
                .scan(r"word\wrap escaped\n xC:\embedded")
                .is_empty()
        );
    }

    fn texts(found: &[HintMatch]) -> Vec<&str> {
        found.iter().map(|matched| matched.text.as_str()).collect()
    }

    #[test]
    fn urls_stop_at_the_quote_that_delimits_them() {
        let found = HintScan::new().scan(
            r#"{"url":"https://a.test/pull/200"} url='https://b.test/it's' `https://c.test/x`"#,
        );
        assert_eq!(
            texts(&found),
            vec![
                "https://a.test/pull/200",
                "https://b.test/it",
                "https://c.test/x"
            ]
        );
        // An apostrophe nobody opened is part of the URL.
        let found = HintScan::new().scan("see https://d.test/it's here");
        assert_eq!(texts(&found), vec!["https://d.test/it's"]);
    }

    #[test]
    fn text_a_program_elided_is_not_a_hint() {
        let found = HintScan::new()
            .scan("tail -2 /home/me/projects/rozi-review/pr\u{2026} https://a.test/pu\u{2026}");
        assert!(texts(&found).is_empty(), "{found:?}");
    }

    #[test]
    fn bare_relative_paths_need_a_file_extension() {
        let found = HintScan::new().scan(
            "git diff -- src/ops/session/remotes.rs .agents/ui.md:12, a/b/c.rs and/or 3/4 fix/projects-navigation-ux v1.2/x",
        );
        assert_eq!(
            texts(&found),
            vec!["src/ops/session/remotes.rs", ".agents/ui.md:12", "a/b/c.rs"]
        );
        assert!(found.iter().all(|matched| matched.kind == HintKind::Path));
    }

    #[test]
    fn a_lone_rooted_word_is_not_a_path() {
        let found =
            HintScan::new().scan("Run /status for a breakdown, see /etc/hosts /tmp/ /README.md");
        assert_eq!(texts(&found), vec!["/etc/hosts", "/tmp/", "/README.md"]);
    }

    #[test]
    fn hex_words_without_digits_are_not_shas() {
        let found = HintScan::new().scan("the defaced and effaced deadbeef 9983d705 1234567");
        assert_eq!(texts(&found), vec!["9983d705"]);
    }

    #[cfg(feature = "terminal")]
    #[test]
    fn snapshot_scan_hints_hyperlinks_by_their_destination() {
        let mut screen = crate::widgets::TerminalScreen::new(3, 20, 0);
        screen.process_bytes(
            b"\x1b]8;;https://a.test/pull/200\x1b\\PR #200\x1b]8;;\x1b\\ 9983d705\r\n\
              \x1b]8;;https://b.test/x\x1b\\https://b.test/x\x1b]8;;\x1b\\ \
              \x1b]8;;javascript:alert(1)\x1b\\bad\x1b]8;;\x1b\\",
        );
        let found = HintScan::new().scan_snapshot(&screen.render_snapshot());
        assert_eq!(
            found
                .iter()
                .map(|matched| (matched.text.as_str(), matched.kind))
                .collect::<Vec<_>>(),
            vec![
                ("https://a.test/pull/200", HintKind::Url),
                ("9983d705", HintKind::GitSha),
                // The link replaces the identical text hint under it rather than doubling it.
                ("https://b.test/x", HintKind::Url),
            ]
        );
        assert_eq!(found[0].start_col(), 0);
        assert_eq!(found[0].end_col(), 7);
    }

    #[cfg(feature = "terminal")]
    #[test]
    fn snapshot_scan_joins_a_hyperlink_across_a_soft_wrap() {
        let mut screen = crate::widgets::TerminalScreen::new(3, 8, 0);
        screen.process_bytes(b"ab \x1b]8;;https://a.test\x1b\\linked label\x1b]8;;\x1b\\");
        let found = HintScan::new().scan_snapshot(&screen.render_snapshot());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].text, "https://a.test");
        assert_eq!(
            found[0].spans,
            vec![
                HintSpan {
                    row: 0,
                    start_col: 3,
                    end_col: 8
                },
                HintSpan {
                    row: 1,
                    start_col: 0,
                    end_col: 7
                },
            ]
        );

        let no_urls = HintScan::new()
            .urls(false)
            .scan_snapshot(&screen.render_snapshot());
        assert!(no_urls.is_empty(), "{no_urls:?}");
    }

    #[cfg(feature = "hints-regex")]
    #[test]
    fn regex_scanner_accepts_string_configured_patterns() {
        let found = HintScan::new()
            .custom_regex(3, r"(?:[0-9]{1,3}\.){3}[0-9]{1,3}")
            .unwrap()
            .scan("IP 10.0.0.1");
        assert_eq!(found[0].kind, HintKind::Custom(3));
        assert_eq!(found[0].text, "10.0.0.1");
    }
}
